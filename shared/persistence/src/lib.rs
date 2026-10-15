//! # Endora shared persistence handle
//!
//! Every bounded context stores its data in one SQLite database. Rather than a
//! single god-store implementing every repository, each context owns its own
//! repositories — but they all share **one** connection through [`Db`], a cheap
//! clone around an `Arc<Mutex<Connection>>`. This preserves the exact
//! single-connection-behind-a-Mutex semantics the store had before the
//! Responsibility-Oriented reorg (ADR 0050), while letting repositories live in
//! their own crates.
//!
//! `Db` deliberately exposes only `lock()`; each context's infrastructure writes
//! its own SQL and owns its own table migrations. Errors surface as the shared
//! [`RepositoryError`].

#![forbid(unsafe_code)]

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

pub use endora_kernel::RepositoryError;

/// A shared handle to the one SQLite connection. Cloning is cheap (an `Arc`
/// bump) and every clone locks the same underlying connection.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Opens (creating if needed) the database at `path` with foreign keys on.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be opened.
    pub fn open(path: &str) -> Result<Self, RepositoryError> {
        let conn = Connection::open(path).map_err(backend)?;
        Self::wrap(conn)
    }

    /// Opens a private in-memory database, mainly for tests.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be created.
    pub fn open_in_memory() -> Result<Self, RepositoryError> {
        let conn = Connection::open_in_memory().map_err(backend)?;
        Self::wrap(conn)
    }

    /// Wraps an already-open connection as a shared handle (foreign keys are
    /// enabled). Useful for tests that seed a legacy schema before opening.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the foreign-keys pragma cannot be set.
    pub fn from_connection(conn: Connection) -> Result<Self, RepositoryError> {
        Self::wrap(conn)
    }

    fn wrap(conn: Connection) -> Result<Self, RepositoryError> {
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(backend)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Locks the shared connection for the duration of one operation.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the lock is poisoned.
    pub fn lock(&self) -> Result<MutexGuard<'_, Connection>, RepositoryError> {
        self.conn
            .lock()
            .map_err(|_| RepositoryError::Backend("connection lock poisoned".to_owned()))
    }
}

/// How long a snapshot waits for a writer to finish committing before giving up.
const SNAPSHOT_PATIENCE: Duration = Duration::from_secs(15);

/// Writes a consistent copy of the database at `source` to a **new** file at `dest`,
/// and checks the copy before returning.
///
/// This is the safe way to back the database up while the node is running. The node
/// writes continuously, and the database uses SQLite's default rollback journal, so a
/// plain file copy can land in the middle of a write and capture a torn page — a
/// backup that looks fine and fails to open on the day it is needed. `VACUUM INTO` takes
/// the copy through SQLite itself, as one consistent view of what had been committed.
///
/// Deliberately conservative, because a backup that misbehaves is worse than none:
///
/// - the source is opened **read-only**, so a mistyped path is an error and never a
///   freshly created empty database that "succeeds";
/// - `dest` must not exist: a second run must not replace yesterday's good copy;
/// - `dest` is created `0600` before anything is written. The database holds the node's
///   credentials (API keys, MCP bearer tokens, sign-in secrets), so a copy must not be
///   world-readable even for the instant between creation and a later `chmod`;
/// - the finished copy is opened and integrity-checked, and **removed** if that fails,
///   so a half-written file is never left looking like a backup.
///
/// # Errors
/// [`RepositoryError::Backend`] if the source cannot be read, `dest` already exists or
/// cannot be created, or SQLite fails while copying; [`RepositoryError::Corrupt`] if the
/// finished copy fails its integrity check.
pub fn snapshot(source: &Path, dest: &Path) -> Result<(), RepositoryError> {
    let dest_text = dest
        .to_str()
        .ok_or_else(|| backend("snapshot destination is not valid UTF-8"))?;
    let conn =
        Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(backend)?;
    conn.busy_timeout(SNAPSHOT_PATIENCE).map_err(backend)?;

    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(backend)?;
    }
    reserve(dest)?;
    let finished = copy_into(&conn, dest, dest_text);
    if finished.is_err() {
        // Whatever stopped it, do not leave an empty or partial file behind.
        let _ = std::fs::remove_file(dest);
    }
    finished
}

/// Creates `dest` empty and owner-only, failing if it is already there. SQLite's
/// `VACUUM INTO` accepts an existing *empty* file, which is what makes creating it first
/// (with the right mode) possible at all.
fn reserve(dest: &Path) -> Result<(), RepositoryError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(dest) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(backend(format!(
            "{} already exists; a snapshot never overwrites an earlier file",
            dest.display()
        ))),
        Err(e) => Err(backend(e)),
    }
}

fn copy_into(conn: &Connection, dest: &Path, dest_text: &str) -> Result<(), RepositoryError> {
    conn.execute("VACUUM INTO ?1", [dest_text])
        .map_err(backend)?;
    let copy =
        Connection::open_with_flags(dest, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(backend)?;
    let verdict: String = copy
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(backend)?;
    if verdict != "ok" {
        return Err(corrupt(format!(
            "snapshot at {} failed its own integrity check: {verdict}",
            dest.display()
        )));
    }
    Ok(())
}

/// Maps any backend failure into a shared [`RepositoryError::Backend`].
#[must_use]
pub fn backend(error: impl core::fmt::Display) -> RepositoryError {
    RepositoryError::Backend(error.to_string())
}

/// Maps a data-reconstruction failure into a shared [`RepositoryError::Corrupt`].
#[must_use]
pub fn corrupt(error: impl core::fmt::Display) -> RepositoryError {
    RepositoryError::Corrupt(error.to_string())
}

/// Renders a `u128` identifier as its stored text form.
#[must_use]
pub fn id_text(value: u128) -> String {
    value.to_string()
}

/// Parses a stored id back into a `u128`, or [`RepositoryError::Corrupt`].
///
/// # Errors
/// [`RepositoryError::Corrupt`] if the text is not a valid `u128`.
pub fn parse_id(text: &str) -> Result<u128, RepositoryError> {
    text.parse::<u128>()
        .map_err(|e| RepositoryError::Corrupt(format!("invalid stored id {text:?}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::{Db, snapshot};
    use rusqlite::{Connection, OpenFlags};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A directory unique to one test, removed when the guard drops. A hand-rolled guard
    /// rather than a `tempfile` dependency: this crate has none, and one test helper is
    /// not a reason to give it one.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "endora-persistence-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).expect("a scratch directory");
            Self(dir)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn rows(db: &Path) -> Vec<String> {
        let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("the snapshot opens as a database");
        let mut stmt = conn
            .prepare("SELECT value FROM settings ORDER BY value")
            .expect("the settings table survived");
        stmt.query_map([], |r| r.get::<_, String>(0))
            .expect("rows")
            .collect::<Result<_, _>>()
            .expect("values")
    }

    fn a_node_with(scratch: &Scratch, values: &[&str]) -> (Db, PathBuf) {
        let path = scratch.path("endora.db");
        let db = Db::open(path.to_str().expect("utf-8")).expect("opens");
        db.lock()
            .expect("lock")
            .execute_batch("CREATE TABLE settings (value TEXT NOT NULL);")
            .expect("schema");
        for v in values {
            db.lock()
                .expect("lock")
                .execute("INSERT INTO settings (value) VALUES (?1)", [v])
                .expect("insert");
        }
        (db, path)
    }

    #[test]
    fn a_snapshot_holds_what_the_database_held() {
        let scratch = Scratch::new();
        let (_db, source) = a_node_with(&scratch, &["alpha", "beta"]);
        let dest = scratch.path("snapshot.db");

        snapshot(&source, &dest).expect("a snapshot");

        assert_eq!(rows(&dest), vec!["alpha", "beta"]);
    }

    #[test]
    fn a_snapshot_taken_mid_write_is_whole_and_leaves_the_live_node_alone() {
        // The node writes constantly (the goal worker checkpoints every few seconds), so
        // a backup will often land inside a transaction. The snapshot must be a view of
        // what was committed — never half of a change — and must not stop the node from
        // finishing its write. This pins those two things. It does not reproduce a tear
        // during the commit itself, which is where a plain file copy goes wrong; that
        // moment cannot be hit deterministically from a test, and is the reason the
        // snapshot goes through SQLite rather than around it.
        let scratch = Scratch::new();
        let (db, source) = a_node_with(&scratch, &["committed"]);
        let conn = db.lock().expect("lock");
        conn.execute_batch("BEGIN; INSERT INTO settings (value) VALUES ('in flight');")
            .expect("a write in progress");

        let dest = scratch.path("snapshot.db");
        snapshot(&source, &dest).expect("a snapshot while a write is open");

        assert_eq!(
            rows(&dest),
            vec!["committed"],
            "only what had been committed"
        );
        conn.execute_batch("COMMIT;")
            .expect("the live node can still finish its write");
    }

    #[test]
    fn a_snapshot_never_overwrites_an_existing_file() {
        // The second run of a backup job must not silently replace yesterday's good copy.
        let scratch = Scratch::new();
        let (_db, source) = a_node_with(&scratch, &["alpha"]);
        let dest = scratch.path("snapshot.db");
        std::fs::write(&dest, b"yesterday's backup").expect("an earlier file");

        assert!(snapshot(&source, &dest).is_err());
        assert_eq!(
            std::fs::read(&dest).expect("still there"),
            b"yesterday's backup",
            "the earlier file is untouched"
        );
    }

    #[test]
    fn a_missing_source_is_an_error_and_not_a_fresh_empty_database() {
        // `Connection::open` creates a database where there is none. A backup of a
        // mistyped path that "succeeds" with an empty file is the worst result a backup
        // can give: it looks like one.
        let scratch = Scratch::new();
        let source = scratch.path("typo.db");
        let dest = scratch.path("snapshot.db");

        assert!(snapshot(&source, &dest).is_err());
        assert!(!source.exists(), "reading must never create the source");
        assert!(
            !dest.exists(),
            "and no empty file is left that looks like a backup"
        );
    }

    #[test]
    fn a_source_that_is_not_a_database_leaves_no_partial_file() {
        let scratch = Scratch::new();
        let source = scratch.path("garbage.db");
        std::fs::write(&source, b"this is not a sqlite database, it is a text file").expect("junk");
        let dest = scratch.path("snapshot.db");

        assert!(snapshot(&source, &dest).is_err());
        assert!(
            !dest.exists(),
            "a failed snapshot must not leave a file behind"
        );
    }

    #[test]
    fn missing_parent_directories_are_created() {
        let scratch = Scratch::new();
        let (_db, source) = a_node_with(&scratch, &["alpha"]);
        let dest = scratch.path("backups/2026/snapshot.db");

        snapshot(&source, &dest).expect("a snapshot into a new directory");

        assert_eq!(rows(&dest), vec!["alpha"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_snapshot_is_readable_only_by_its_owner() {
        // The database holds the node's credentials — API keys, MCP bearer tokens,
        // sign-in secrets. A backup of it must not be world-readable by default.
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new();
        let (_db, source) = a_node_with(&scratch, &["alpha"]);
        let dest = scratch.path("snapshot.db");

        snapshot(&source, &dest).expect("a snapshot");

        let mode = std::fs::metadata(&dest)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "mode was {:o}", mode & 0o777);
    }
}
