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

use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;

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
