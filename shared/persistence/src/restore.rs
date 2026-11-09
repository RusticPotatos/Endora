//! The restore proof (milestone f): a backup is a hope until it has been opened.
//!
//! [`snapshot`](crate::snapshot) checks that a copy is a whole SQLite file. That passes a copy that
//! is empty, or from before a table existed. [`prove_restore`] goes further: it restores the
//! copy to a scratch file, as a restore would, and passes it only if the restored database
//! is whole, has the live database's schema, and holds rows wherever the live database does.
//! Neither the copy nor the live database is written: both are opened read-only.

use std::fmt;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use rusqlite::{Connection, OpenFlags};

/// Why a copy was not proven. Each says what to tell the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreFailure {
    /// The copy is missing, would not open, or failed SQLite's `integrity_check`.
    Integrity(String),
    /// The copy's tables and indexes (names and SQL) are not the live database's.
    Schema(String),
    /// The named table has rows in the live database and none in the copy.
    Empty(String),
}

impl fmt::Display for RestoreFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integrity(why) => write!(f, "the copy is not whole: {why}"),
            Self::Schema(why) => write!(f, "the copy's schema is not the database's: {why}"),
            Self::Empty(table) => {
                write!(f, "the copy holds no rows of \"{table}\", which has some")
            }
        }
    }
}

impl std::error::Error for RestoreFailure {}

/// The folder beside a copy where its proof restores it. Never the system's temporary folder:
/// HQ's container has a read-only root with only its data folder writable, and a copy of a
/// store (sealed secrets, goals, the desk) should not pass through a folder every process on the
/// host can list.
pub const SCRATCH: &str = ".scratch";

/// A scratch file older than this is a crash's leftover.
const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(3600);

/// Proves that `copy` restores `live`, restoring it to a private `.scratch` folder beside the
/// copy (removed again when empty). See the module documentation for what passes.
///
/// A row added to `live` after `copy` was taken to a table the copy has empty reads as a failure:
/// prove a copy soon after taking it.
///
/// # Errors
/// The first check the copy fails, as a [`RestoreFailure`].
pub fn prove_restore(copy: &Path, live: &Path) -> Result<(), RestoreFailure> {
    let beside = copy
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let scratch = beside.join(SCRATCH);
    private_folder(&scratch).map_err(|e| {
        RestoreFailure::Integrity(format!("could not make {}: {e}", scratch.display()))
    })?;
    let proof = prove_restore_in(copy, live, &scratch);
    // Only if empty: a proof running beside this one keeps its own file.
    let _ = std::fs::remove_dir(&scratch);
    proof
}

/// The scratch folder, owner-only, with a crash's old leftovers cleared out of it.
fn private_folder(scratch: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(scratch)?;
    for entry in std::fs::read_dir(scratch)?.filter_map(Result::ok) {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age > STALE_AFTER);
        let ours = entry
            .file_name()
            .to_string_lossy()
            .starts_with("endora-restore-proof-");
        if old && ours {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// [`prove_restore`] with the scratch file made in `scratch_dir`. The scratch file is
/// owner-only (a copy holds the node's sealed secrets) and is removed before returning.
///
/// # Errors
/// The first check the copy fails, as a [`RestoreFailure`].
pub fn prove_restore_in(
    copy: &Path,
    live: &Path,
    scratch_dir: &Path,
) -> Result<(), RestoreFailure> {
    let restored = Restored::from(copy, scratch_dir).map_err(|e| {
        RestoreFailure::Integrity(format!("could not restore {}: {e}", copy.display()))
    })?;
    let candidate = open_read_only(&restored.0).map_err(integrity)?;
    check_whole(&candidate)?;
    let live = open_read_only(live)
        .map_err(|e| RestoreFailure::Schema(format!("the live database cannot be read: {e}")))?;
    let (wanted, found) = (schema(&live), schema(&candidate));
    let (wanted, found) = (
        wanted.map_err(|e| RestoreFailure::Schema(e.to_string()))?,
        found.map_err(integrity)?,
    );
    if wanted != found {
        return Err(RestoreFailure::Schema(difference(&wanted, &found)));
    }
    for (kind, name, _) in &wanted {
        if kind != "table" {
            continue;
        }
        let rows = has_rows(&live, name).map_err(|e| RestoreFailure::Schema(e.to_string()))?;
        if rows && !has_rows(&candidate, name).map_err(integrity)? {
            return Err(RestoreFailure::Empty(name.clone()));
        }
    }
    Ok(())
}

fn integrity(error: impl fmt::Display) -> RestoreFailure {
    RestoreFailure::Integrity(error.to_string())
}

/// The copy as restored: a private file in the scratch folder, removed on drop.
struct Restored(PathBuf);

impl Restored {
    fn from(copy: &Path, scratch_dir: &Path) -> io::Result<Self> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "endora-restore-proof-{}-{}.db",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let mut source = std::fs::File::open(copy)?;
        let path = scratch_dir.join(name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut scratch = options.open(&path)?;
        let restored = Self(path);
        io::copy(&mut source, &mut scratch)?;
        scratch.flush()?;
        Ok(restored)
    }
}

impl Drop for Restored {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

fn check_whole(conn: &Connection) -> Result<(), RestoreFailure> {
    let mut check = conn.prepare("PRAGMA integrity_check").map_err(integrity)?;
    let verdicts = check
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(integrity)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(integrity)?;
    if verdicts == ["ok"] {
        return Ok(());
    }
    let first: Vec<String> = verdicts.into_iter().take(3).collect();
    Err(RestoreFailure::Integrity(first.join("; ")))
}

/// Every table and index SQLite has not made for itself: kind, name and the SQL that made it.
type Schema = Vec<(String, String, Option<String>)>;

fn schema(conn: &Connection) -> rusqlite::Result<Schema> {
    conn.prepare(
        "SELECT type, name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         ORDER BY type, name",
    )?
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
    .collect()
}

/// The first thing that differs, in words.
fn difference(wanted: &Schema, found: &Schema) -> String {
    if let Some((kind, name, _)) = wanted.iter().find(|w| !found.iter().any(|f| f.1 == w.1)) {
        return format!("the copy has no {kind} \"{name}\"");
    }
    if let Some((kind, name, _)) = found.iter().find(|f| !wanted.iter().any(|w| w.1 == f.1)) {
        return format!("the copy has a {kind} \"{name}\" the database does not");
    }
    let name = wanted
        .iter()
        .zip(found)
        .find(|(w, f)| w != f)
        .map_or("?", |(w, _)| w.1.as_str());
    format!("\"{name}\" is defined differently")
}

fn has_rows(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    let quoted = table.replace('"', "\"\"");
    conn.query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM \"{quoted}\")"),
        [],
        |r| r.get(0),
    )
}

#[cfg(test)]
#[path = "restore_tests.rs"]
mod tests;
