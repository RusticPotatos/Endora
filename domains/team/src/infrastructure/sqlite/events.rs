//! The event feed in HQ's store.

use rusqlite::{OptionalExtension, params};

use super::{SqliteTeamStore, backend, event_from};
use crate::application::event_ports::EventStore;
use crate::application::ports::Stored;
use crate::domain::{Event, Millis};

impl EventStore for SqliteTeamStore {
    fn has_event(&self, id: i64) -> Stored<bool> {
        self.conn()?
            .query_row("SELECT COUNT(*) > 0 FROM events WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .map_err(backend)
    }

    fn emit(&self, at: Millis, kind: &str, data: &serde_json::Value) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO events (at_ms, kind, data) VALUES (?1, ?2, ?3)",
            params![at, kind, data.to_string()],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn last_event(&self) -> Stored<i64> {
        self.conn()?
            .query_row("SELECT COALESCE(MAX(id), 0) FROM events", [], |r| r.get(0))
            .map_err(backend)
    }

    fn count_events(&self, kind: &str) -> Stored<(i64, Option<Millis>)> {
        self.conn()?
            .query_row(
                "SELECT COUNT(*), MAX(at_ms) FROM events WHERE kind = ?1",
                [kind],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(backend)
    }

    fn prune_events(&self, keep: i64) -> Stored<usize> {
        // Everything older than the oldest of the newest `keep` that are not ticks goes; a tick
        // also goes once it is not among the newest `keep` of all.
        self.conn()?
            .execute(
                "DELETE FROM events
                 WHERE id <= COALESCE((SELECT id FROM events WHERE kind <> ?2
                                       ORDER BY id DESC LIMIT 1 OFFSET ?1), 0)
                    OR (kind = ?2 AND id <= (SELECT COALESCE(MAX(id), 0) FROM events) - ?1)",
                params![keep, crate::domain::EventKind::Tick.name()],
            )
            .map_err(backend)
    }

    fn event(&self, id: i64) -> Stored<Option<Event>> {
        self.conn()?
            .query_row(
                "SELECT id, at_ms, kind, data FROM events WHERE id = ?1",
                [id],
                event_from,
            )
            .optional()
            .map_err(backend)
    }

    fn events_after(&self, id: i64) -> Stored<Vec<Event>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT id, at_ms, kind, data FROM events WHERE id > ?1 ORDER BY id")
            .map_err(backend)?;
        let rows = stmt.query_map([id], event_from).map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn events_of(&self, kinds: &[&str]) -> Stored<Vec<Event>> {
        let conn = self.conn()?;
        let marks = vec!["?"; kinds.len()].join(", ");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT id, at_ms, kind, data FROM events WHERE kind IN ({marks}) ORDER BY id"
            ))
            .map_err(backend)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(kinds), event_from)
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn first_event_at(&self) -> Stored<Option<Millis>> {
        self.conn()?
            .query_row("SELECT at_ms FROM events ORDER BY id LIMIT 1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(backend)
    }

    fn events_before(&self, id: i64, limit: usize) -> Stored<Vec<Event>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, at_ms, kind, data FROM events WHERE id < ?1 ORDER BY id DESC LIMIT ?2",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(
                params![id, i64::try_from(limit).unwrap_or(i64::MAX)],
                event_from,
            )
            .map_err(backend)?;
        let mut events: Vec<Event> = rows.collect::<rusqlite::Result<_>>().map_err(backend)?;
        events.reverse();
        Ok(events)
    }
}
