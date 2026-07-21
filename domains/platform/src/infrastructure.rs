//! Platform infrastructure — SQLite adapters over the shared [`Db`] handle.
//!
//! Owns its two tables' schema via [`migrate`]; each store holds a clone of the
//! shared connection, so it participates in the one connection every context
//! shares.

use endora_kernel::RepositoryError;
use endora_kernel::ids::{AuditId, Timestamp};
use endora_persistence::{Db, backend, corrupt, id_text, parse_id};
use rusqlite::params;

use crate::application::{ActivityEvent, AuditLog, EventLog};
use crate::domain::AuditRecord;

/// Creates the platform tables if absent (idempotent). Called once at startup.
///
/// # Errors
/// [`RepositoryError::Backend`] if the schema cannot be applied.
pub fn migrate(db: &Db) -> Result<(), RepositoryError> {
    let conn = db.lock()?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS audit_log (
            id      TEXT PRIMARY KEY,
            at_ms   INTEGER NOT NULL,
            summary TEXT NOT NULL
        ) STRICT;
        CREATE INDEX IF NOT EXISTS idx_audit_at ON audit_log(at_ms);
        CREATE TABLE IF NOT EXISTS events (
            id      INTEGER PRIMARY KEY AUTOINCREMENT,
            at_ms   INTEGER NOT NULL,
            summary TEXT NOT NULL
        ) STRICT;",
    )
    .map_err(backend)?;
    Ok(())
}

/// SQLite-backed [`AuditLog`].
pub struct AuditStore {
    db: Db,
}

impl AuditStore {
    /// Builds an audit store over the shared connection handle.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl AuditLog for AuditStore {
    fn append(&self, record: &AuditRecord) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "INSERT INTO audit_log (id, at_ms, summary) VALUES (?1, ?2, ?3)",
            params![
                id_text(record.id().value()),
                record.at().unix_millis(),
                record.summary()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn recent(&self, limit: usize) -> Result<Vec<AuditRecord>, RepositoryError> {
        let conn = self.db.lock()?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = conn
            .prepare(
                "SELECT id, at_ms, summary FROM audit_log ORDER BY at_ms DESC, id DESC LIMIT ?1",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(backend)?;

        let mut records = Vec::new();
        for row in rows {
            let (id, at_ms, summary) = row.map_err(backend)?;
            let record = AuditRecord::new(
                AuditId::new(parse_id(&id)?),
                Timestamp::from_unix_millis(at_ms),
                &summary,
            )
            .map_err(corrupt)?;
            records.push(record);
        }
        Ok(records)
    }
}

/// SQLite-backed [`EventLog`].
pub struct EventStore {
    db: Db,
}

impl EventStore {
    /// Builds an event store over the shared connection handle.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl EventLog for EventStore {
    fn record(&self, at: Timestamp, summary: &str) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "INSERT INTO events (at_ms, summary) VALUES (?1, ?2)",
            params![at.unix_millis(), summary],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn recent(&self, limit: usize) -> Result<Vec<ActivityEvent>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT at_ms, summary FROM events ORDER BY id DESC LIMIT ?1")
            .map_err(backend)?;
        let rows = stmt
            .query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |r| {
                Ok(ActivityEvent {
                    at: Timestamp::from_unix_millis(r.get::<_, i64>(0)?),
                    summary: r.get::<_, String>(1)?,
                })
            })
            .map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend)
    }
}

#[cfg(test)]
mod tests {
    use super::{AuditStore, EventStore, migrate};
    use crate::application::{AuditLog, EventLog};
    use crate::domain::AuditRecord;
    use endora_kernel::ids::{AuditId, Timestamp};
    use endora_persistence::Db;

    #[test]
    fn audit_appends_and_reads_newest_first() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = AuditStore::new(db);
        for (i, at) in [(1u128, 100i64), (2, 300), (3, 200)] {
            store
                .append(
                    &AuditRecord::new(
                        AuditId::new(i),
                        Timestamp::from_unix_millis(at),
                        "did a thing",
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let recent = store.recent(10).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].at(), Timestamp::from_unix_millis(300));
    }

    #[test]
    fn events_record_and_read_newest_first() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = EventStore::new(db);
        store
            .record(Timestamp::from_unix_millis(1), "first")
            .unwrap();
        store
            .record(Timestamp::from_unix_millis(2), "second")
            .unwrap();
        let recent = store.recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].summary, "second");
    }
}
