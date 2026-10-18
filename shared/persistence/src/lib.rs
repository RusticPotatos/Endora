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

pub mod secrets;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use zeroize::Zeroizing;

pub use endora_kernel::RepositoryError;
use secrets::{DataKey, KeySource, Keyring, MemoryKey, SEALED, SecretColumn};

/// Where the data keys live, each wrapped by the [`KeySource`] (ADR 0125).
const KEYS_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS secret_keys (
    kid        TEXT PRIMARY KEY,
    wrapped    TEXT NOT NULL,
    created_ms INTEGER NOT NULL,
    active     INTEGER NOT NULL DEFAULT 0
) STRICT;";

/// A shared handle to the one SQLite connection. Cloning is cheap (an `Arc`
/// bump) and every clone locks the same underlying connection.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    /// The data keys, once a [`KeySource`] is attached. Locked after `conn` when both are
    /// held, never before.
    secrets: Arc<RwLock<Option<Keyring>>>,
    /// The data keys are wrapped by a vault that is sealed (ADR 0127): until a source is
    /// attached, asking for a secret says so rather than "no key source".
    waiting_for_vault: Arc<AtomicBool>,
}

impl Db {
    /// Opens (creating if needed) the database at `path` with foreign keys on, and makes the
    /// file owner-only: it holds the node's sealed secrets and their wrapped keys.
    ///
    /// **No key source is attached**: sealing and unsealing refuse until
    /// [`attach_key_source`](Self::attach_key_source) is called.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be opened.
    pub fn open(path: &str) -> Result<Self, RepositoryError> {
        let conn = Connection::open(path).map_err(backend)?;
        secrets::owner_only(Path::new(path));
        Self::wrap(conn)
    }

    /// Opens a private in-memory database, mainly for tests, with a key source that lives
    /// only in this process ([`MemoryKey`]) — the contents die with it anyway.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be created.
    pub fn open_in_memory() -> Result<Self, RepositoryError> {
        let conn = Connection::open_in_memory().map_err(backend)?;
        let db = Self::wrap(conn)?;
        db.attach_key_source(Arc::new(MemoryKey::random()))?;
        Ok(db)
    }

    /// Wraps an already-open connection as a shared handle (foreign keys are
    /// enabled). Useful for tests that seed a legacy schema before opening; it gets an
    /// in-process key source, as [`open_in_memory`](Self::open_in_memory) does.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the foreign-keys pragma cannot be set.
    pub fn from_connection(conn: Connection) -> Result<Self, RepositoryError> {
        let db = Self::wrap(conn)?;
        db.attach_key_source(Arc::new(MemoryKey::random()))?;
        Ok(db)
    }

    fn wrap(conn: Connection) -> Result<Self, RepositoryError> {
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(backend)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            secrets: Arc::new(RwLock::new(None)),
            waiting_for_vault: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Marks this database as waiting for a sealed vault (ADR 0127): its data keys are
    /// wrapped by OpenBao's Transit key and cannot be unwrapped until the vault is unlocked.
    /// Until [`attach_key_source`](Self::attach_key_source) succeeds, everything that needs
    /// a secret fails saying so, and nothing can be sealed.
    pub fn wait_for_the_vault(&self) {
        self.waiting_for_vault.store(true, Ordering::SeqCst);
    }

    /// Whether the data keys are open: a key source is attached.
    #[must_use]
    pub fn has_key_source(&self) -> bool {
        self.read_secrets().is_ok_and(|guard| guard.is_some())
    }

    /// The names of the key sources that wrapped this database's data keys (`file`,
    /// `openbao`), read without unwrapping any: how the node knows, before it opens them,
    /// whether it needs the key file or the vault. Empty when there are no keys yet.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be read.
    pub fn key_sources_on_disk(&self) -> Result<Vec<String>, RepositoryError> {
        if !self.has_data_keys()? {
            return Ok(Vec::new());
        }
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT wrapped FROM secret_keys ORDER BY created_ms")
            .map_err(backend)?;
        let wrapped: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .map_err(backend)?
            .collect::<Result<_, _>>()
            .map_err(backend)?;
        let mut names: Vec<String> = Vec::new();
        for name in wrapped.iter().filter_map(|w| w.split(':').next()) {
            if !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
        }
        Ok(names)
    }

    fn no_keys(&self) -> RepositoryError {
        if self.waiting_for_vault.load(Ordering::SeqCst) {
            return backend(
                "the vault is locked, so stored credentials cannot be opened: unlock it in \
                 the console (Settings › Vault)",
            );
        }
        no_key_source()
    }

    /// Whether this database already holds data keys — which is to say, secrets sealed
    /// under some key source. A missing key file over such a database must be an error,
    /// never a fresh key.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be read.
    pub fn has_data_keys(&self) -> Result<bool, RepositoryError> {
        let conn = self.lock()?;
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type='table' AND name='secret_keys')",
                [],
                |r| r.get(0),
            )
            .map_err(backend)?;
        if !exists {
            return Ok(false);
        }
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM secret_keys", [], |r| r.get(0))
            .map_err(backend)?;
        Ok(count > 0)
    }

    /// Attaches the key source: unwraps every data key this database has, making the first
    /// one if it has none.
    ///
    /// Also turns on `secure_delete`, so a secret replaced or removed later is overwritten on
    /// disk rather than left in a free page for a copy of the file to carry.
    ///
    /// # Errors
    /// [`RepositoryError`] if the keys cannot be read, unwrapped or made.
    pub fn attach_key_source(&self, source: Arc<dyn KeySource>) -> Result<(), RepositoryError> {
        let mut conn = self.lock()?;
        conn.pragma_update(None, "secure_delete", "ON")
            .map_err(backend)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        tx.execute_batch(KEYS_SCHEMA).map_err(backend)?;
        let rows: Vec<(String, String, bool)> = {
            let mut stmt = tx
                .prepare("SELECT kid, wrapped, active FROM secret_keys ORDER BY created_ms")
                .map_err(backend)?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)))
                .map_err(backend)?
                .collect::<Result<_, _>>()
                .map_err(backend)?
        };
        let mut keys = HashMap::new();
        let mut active = None;
        for (kid, wrapped, is_active) in rows {
            keys.insert(kid.clone(), source.unwrap(&kid, &wrapped)?);
            if is_active {
                active = Some(kid);
            }
        }
        let active = match active {
            Some(kid) => kid,
            None => {
                let kid = secrets::new_kid();
                let key = DataKey::random();
                tx.execute(
                    "INSERT INTO secret_keys (kid, wrapped, created_ms, active) VALUES (?1, ?2, ?3, 1)",
                    params![kid, source.wrap(&kid, &key)?, now_ms()],
                )
                .map_err(backend)?;
                keys.insert(kid.clone(), key);
                kid
            }
        };
        tx.commit().map_err(backend)?;
        drop(conn);
        *self.write_secrets()? = Some(Keyring {
            source,
            active,
            keys,
        });
        self.waiting_for_vault.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Seals a secret for storage. An empty string stays empty: it means "none".
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if no key source is attached.
    pub fn seal(&self, plaintext: &str) -> Result<String, RepositoryError> {
        if plaintext.is_empty() {
            return Ok(String::new());
        }
        let guard = self.read_secrets()?;
        let ring = guard.as_ref().ok_or_else(|| self.no_keys())?;
        ring.seal(plaintext)
    }

    /// Opens a value [`seal`](Self::seal) made. `label` (`table.column`) names it in any
    /// error, which never carries the value. A value that is not sealed is an error, logged
    /// by column: after the migration nothing should be, and using one would be trusting
    /// whatever wrote it around the store.
    ///
    /// # Errors
    /// [`RepositoryError`] if no key source is attached, or the value is plaintext, damaged,
    /// or sealed under a key this database does not have.
    pub fn unseal(&self, label: &str, stored: &str) -> Result<String, RepositoryError> {
        if stored.is_empty() {
            return Ok(String::new());
        }
        let guard = self.read_secrets()?;
        let ring = guard.as_ref().ok_or_else(|| self.no_keys())?;
        ring.unseal(label, stored)
    }

    /// Seals every plaintext value left in `columns`: the first start after secrets at rest,
    /// or a value somebody wrote around the store. Guarded and idempotent — a sealed value
    /// is left exactly as it is — and done in one transaction.
    ///
    /// When anything was sealed, the file is `VACUUM`ed afterwards, so the plaintext does not
    /// survive in free pages for the next copy of the file to carry.
    ///
    /// Returns how many values were sealed.
    ///
    /// # Errors
    /// [`RepositoryError`] if no key source is attached or the database fails.
    pub fn seal_plaintext(&self, columns: &[SecretColumn]) -> Result<usize, RepositoryError> {
        let mut conn = self.lock()?;
        let guard = self.read_secrets()?;
        let ring = guard.as_ref().ok_or_else(|| self.no_keys())?;
        let tx = conn.transaction().map_err(backend)?;
        let mut sealed = 0;
        for column in columns {
            let (table, col) = (column.table, column.column);
            let rows: Vec<(i64, Zeroizing<String>)> = {
                let mut stmt = tx
                    .prepare(&format!(
                        "SELECT rowid, {col} FROM {table} \
                         WHERE {col} != '' AND substr({col}, 1, {len}) != '{SEALED}'",
                        len = SEALED.len()
                    ))
                    .map_err(backend)?;
                stmt.query_map([], |r| Ok((r.get(0)?, Zeroizing::new(r.get(1)?))))
                    .map_err(backend)?
                    .collect::<Result<_, _>>()
                    .map_err(backend)?
            };
            for (rowid, plain) in rows {
                tx.execute(
                    &format!("UPDATE {table} SET {col} = ?1 WHERE rowid = ?2"),
                    params![ring.seal(&plain)?, rowid],
                )
                .map_err(backend)?;
                sealed += 1;
            }
        }
        tx.commit().map_err(backend)?;
        if sealed > 0 {
            conn.execute_batch("VACUUM").map_err(backend)?;
        }
        Ok(sealed)
    }

    /// Rotates the data key: makes a new one, re-seals every sealed value in `columns` under
    /// it, and forgets the old ones, all in one transaction. Returns the new key's id.
    ///
    /// # Errors
    /// [`RepositoryError`] if no key source is attached, a value does not open, or the
    /// database fails; nothing changes then.
    pub fn rekey(&self, columns: &[SecretColumn]) -> Result<String, RepositoryError> {
        let mut conn = self.lock()?;
        let mut guard = self.write_secrets()?;
        let ring = guard.as_mut().ok_or_else(|| self.no_keys())?;
        let kid = secrets::new_kid();
        let key = DataKey::random();
        let wrapped = ring.source.wrap(&kid, &key)?;
        ring.keys.insert(kid.clone(), key);
        let before = std::mem::replace(&mut ring.active, kid.clone());
        let done = (|| {
            let tx = conn.transaction().map_err(backend)?;
            tx.execute("UPDATE secret_keys SET active = 0", [])
                .map_err(backend)?;
            tx.execute(
                "INSERT INTO secret_keys (kid, wrapped, created_ms, active) VALUES (?1, ?2, ?3, 1)",
                params![kid, wrapped, now_ms()],
            )
            .map_err(backend)?;
            for column in columns {
                let (table, col) = (column.table, column.column);
                let rows: Vec<(i64, String)> = {
                    let mut stmt = tx
                        .prepare(&format!(
                            "SELECT rowid, {col} FROM {table} \
                             WHERE substr({col}, 1, {len}) = '{SEALED}'",
                            len = SEALED.len()
                        ))
                        .map_err(backend)?;
                    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                        .map_err(backend)?
                        .collect::<Result<_, _>>()
                        .map_err(backend)?
                };
                for (rowid, stored) in rows {
                    let plain = Zeroizing::new(ring.unseal(&column.label(), &stored)?);
                    tx.execute(
                        &format!("UPDATE {table} SET {col} = ?1 WHERE rowid = ?2"),
                        params![ring.seal(&plain)?, rowid],
                    )
                    .map_err(backend)?;
                }
            }
            tx.execute("DELETE FROM secret_keys WHERE kid != ?1", params![kid])
                .map_err(backend)?;
            tx.commit().map_err(backend)
        })();
        if let Err(e) = done {
            ring.keys.remove(&kid);
            ring.active = before;
            return Err(e);
        }
        ring.keys.retain(|k, _| *k == kid);
        Ok(kid)
    }

    /// Re-wraps every data key under `source` and uses it from then on. **No value changes**:
    /// this is how the key-encryption key moves (a new key file, or a Transit engine) without
    /// a data migration.
    ///
    /// # Errors
    /// [`RepositoryError`] if no key source is attached, `source` cannot wrap, or the
    /// database fails; nothing changes then.
    pub fn rewrap(&self, source: Arc<dyn KeySource>) -> Result<(), RepositoryError> {
        let mut conn = self.lock()?;
        let mut guard = self.write_secrets()?;
        let ring = guard.as_mut().ok_or_else(|| self.no_keys())?;
        let tx = conn.transaction().map_err(backend)?;
        for (kid, key) in &ring.keys {
            tx.execute(
                "UPDATE secret_keys SET wrapped = ?1 WHERE kid = ?2",
                params![source.wrap(kid, key)?, kid],
            )
            .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        ring.source = source;
        Ok(())
    }

    /// The way out when the key is gone for good: empties every sealed value in `columns` and
    /// drops the data keys, so the node can start again with a new key and the secrets can be
    /// entered again. Needs no key. Returns how many values were emptied.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database fails; nothing changes then.
    pub fn forget_sealed(&self, columns: &[SecretColumn]) -> Result<usize, RepositoryError> {
        let had_keys = self.has_data_keys()?;
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(backend)?;
        let mut emptied = 0;
        for column in columns {
            let (table, col) = (column.table, column.column);
            emptied += tx
                .execute(
                    &format!(
                        "UPDATE {table} SET {col} = '' WHERE substr({col}, 1, {len}) = '{SEALED}'",
                        len = SEALED.len()
                    ),
                    [],
                )
                .map_err(backend)?;
        }
        if had_keys {
            tx.execute("DELETE FROM secret_keys", []).map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        drop(conn);
        *self.write_secrets()? = None;
        Ok(emptied)
    }

    fn read_secrets(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, Option<Keyring>>, RepositoryError> {
        self.secrets
            .read()
            .map_err(|_| RepositoryError::Backend("secrets lock poisoned".to_owned()))
    }

    fn write_secrets(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, Option<Keyring>>, RepositoryError> {
        self.secrets
            .write()
            .map_err(|_| RepositoryError::Backend("secrets lock poisoned".to_owned()))
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

fn no_key_source() -> RepositoryError {
    backend("no key source is attached to this database, so it cannot keep secrets")
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
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
        // The node writes constantly (workers record as they go), so
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
