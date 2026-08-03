//! Conversation infrastructure — the SQLite-backed chat repository.

use endora_kernel::RepositoryError;
use endora_kernel::ids::{MessageId, Timestamp};
use endora_persistence::{Db, backend, corrupt, id_text, parse_id};
use rusqlite::params;

use crate::application::ChatRepository;
use crate::domain::{ChatMessage, MessageRole};

/// Creates the conversation table if absent (idempotent).
///
/// # Errors
/// [`RepositoryError::Backend`] if the schema cannot be applied.
pub fn migrate(db: &Db) -> Result<(), RepositoryError> {
    db.lock()?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                id    TEXT PRIMARY KEY,
                role  TEXT NOT NULL,
                body  TEXT NOT NULL,
                at_ms INTEGER NOT NULL
            ) STRICT;
            CREATE TABLE IF NOT EXISTS message_actions (
                message_id TEXT PRIMARY KEY,
                actions    TEXT NOT NULL
            ) STRICT;
            CREATE TABLE IF NOT EXISTS conversation_summary (
                id      INTEGER PRIMARY KEY CHECK (id = 0),
                body    TEXT NOT NULL,
                covered INTEGER NOT NULL
            ) STRICT;",
        )
        .map_err(backend)?;
    Ok(())
}

/// SQLite-backed [`ChatRepository`] over the shared connection handle.
pub struct ChatStore {
    db: Db,
}

impl ChatStore {
    /// Builds a chat store over the shared connection handle.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Stores a butler message's action trail (the steps it took + the sources it
    /// cited, as a JSON blob) keyed by the message id — so a past reply keeps its
    /// expandable actions and Sources after a reload, not just live.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the write fails.
    pub fn save_actions(
        &self,
        message_id: &str,
        actions_json: &str,
    ) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO message_actions (message_id, actions) VALUES (?1, ?2)",
                params![message_id, actions_json],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Loads the running conversation summary as `(body, covered)`, if one has been
    /// stored (ADR 0053 context compaction). `None` before the first summary.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the read fails.
    pub fn load_summary(&self) -> Result<Option<(String, usize)>, RepositoryError> {
        let conn = self.db.lock()?;
        let row = conn
            .query_row(
                "SELECT body, covered FROM conversation_summary WHERE id = 0",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .map(|(body, covered)| (body, covered.max(0) as usize));
        match row {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(backend(e)),
        }
    }

    /// Stores the running conversation summary (single row), replacing any prior one.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the write fails.
    pub fn save_summary(&self, body: &str, covered: usize) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO conversation_summary (id, body, covered) \
                 VALUES (0, ?1, ?2)",
                params![body, covered as i64],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Returns every stored action trail as `(message_id, actions_json)`, for the
    /// chat history to attach to its messages.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the read fails.
    pub fn all_actions(&self) -> Result<Vec<(String, String)>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT message_id, actions FROM message_actions")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(backend)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(backend)?);
        }
        Ok(out)
    }
}

/// Decodes message rows, so every query that returns messages decodes them identically —
/// a second hand-rolled copy is how two reads start disagreeing about what is corrupt.
fn rows_into_messages<I>(rows: I) -> Result<Vec<ChatMessage>, RepositoryError>
where
    I: Iterator<Item = Result<(String, String, String, i64), rusqlite::Error>>,
{
    let mut out = Vec::new();
    for row in rows {
        let (id, role, body, at_ms) = row.map_err(backend)?;
        let role = MessageRole::from_name(&role)
            .ok_or_else(|| RepositoryError::Corrupt(format!("unknown message role {role:?}")))?;
        out.push(
            ChatMessage::new(
                MessageId::new(parse_id(&id)?),
                role,
                &body,
                Timestamp::from_unix_millis(at_ms),
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
}

impl ChatRepository for ChatStore {
    /// What a past reply found — the outputs of the tools it ran (ADR 0053's trail, read
    /// back rather than only displayed).
    ///
    /// The trail was stored for the person, so a reply keeps its expandable actions after a
    /// reload. It was never given back to the butler, so a second turn began with the prose
    /// and no trace of the reading behind it — and asking the same thing twice meant looking
    /// twice, or being asked to say more.
    ///
    /// A malformed or missing trail is no findings rather than an error: this is context,
    /// and a turn that loses it is exactly the turn everyone had before.
    fn what_it_found(&self, message_id: &str) -> Result<Vec<String>, RepositoryError> {
        let conn = self.db.lock()?;
        let stored: Option<String> = conn
            .query_row(
                "SELECT actions FROM message_actions WHERE message_id = ?1",
                params![message_id],
                |row| row.get(0),
            )
            .ok();
        let Some(stored) = stored else {
            return Ok(Vec::new());
        };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&stored) else {
            return Ok(Vec::new());
        };
        Ok(parsed
            .get("steps")
            .and_then(serde_json::Value::as_array)
            .map(|steps| {
                steps
                    .iter()
                    .filter_map(|s| s.get("output").and_then(serde_json::Value::as_str))
                    .filter(|o| !o.trim().is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default())
    }

    fn append(&self, message: &ChatMessage) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO messages (id, role, body, at_ms) VALUES (?1, ?2, ?3, ?4)",
            params![
                id_text(message.id().value()),
                message.role().name(),
                message.text(),
                message.at().unix_millis()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn between(&self, from_ms: i64, to_ms: i64) -> Result<Vec<ChatMessage>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, role, body, at_ms FROM messages \
                 WHERE at_ms >= ?1 AND at_ms < ?2 ORDER BY at_ms, rowid",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![from_ms, to_ms], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(backend)?;
        rows_into_messages(rows)
    }

    fn days(&self, offset_minutes: i64) -> Result<Vec<(String, usize)>, RepositoryError> {
        let conn = self.db.lock()?;
        // Grouped on the CALLER's midnights: the stored instant is shifted by their
        // distance from UTC before the date is taken, so the days line up with the ones
        // they actually lived through.
        let mut stmt = conn
            .prepare(
                "SELECT date((at_ms / 1000) + (?1 * 60), 'unixepoch') AS day, COUNT(*) \
                 FROM messages GROUP BY day ORDER BY day",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![offset_minutes], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for row in rows {
            let (day, count) = row.map_err(backend)?;
            out.push((day, usize::try_from(count).unwrap_or(0)));
        }
        Ok(out)
    }

    fn list(&self) -> Result<Vec<ChatMessage>, RepositoryError> {
        let conn = self.db.lock()?;
        // Insertion order (rowid) breaks ties so same-millisecond turns keep their
        // real order — ids are random and cannot be a tiebreak.
        let mut stmt = conn
            .prepare("SELECT id, role, body, at_ms FROM messages ORDER BY at_ms, rowid")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, role, body, at_ms) = row.map_err(backend)?;
            let role = MessageRole::from_name(&role).ok_or_else(|| {
                RepositoryError::Corrupt(format!("unknown message role {role:?}"))
            })?;
            out.push(
                ChatMessage::new(
                    MessageId::new(parse_id(&id)?),
                    role,
                    &body,
                    Timestamp::from_unix_millis(at_ms),
                )
                .map_err(corrupt)?,
            );
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatStore, migrate};
    use crate::application::ChatRepository;
    use crate::domain::{ChatMessage, MessageRole};
    use endora_kernel::ids::{MessageId, Timestamp};
    use endora_persistence::Db;

    #[test]
    fn appends_and_lists_in_time_then_insertion_order() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = ChatStore::new(db);
        let msg = |id, role, text, at| {
            ChatMessage::new(
                MessageId::new(id),
                role,
                text,
                Timestamp::from_unix_millis(at),
            )
            .unwrap()
        };
        store.append(&msg(1, MessageRole::User, "hi", 100)).unwrap();
        store
            .append(&msg(2, MessageRole::Butler, "hello", 100))
            .unwrap();
        let all = store.list().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].text(), "hi"); // same ms → insertion order
        assert_eq!(all[1].text(), "hello");
    }

    #[test]
    fn conversation_summary_round_trips_and_replaces() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = ChatStore::new(db);

        // Nothing stored yet.
        assert_eq!(store.load_summary().unwrap(), None);

        // Store, then read it back.
        store.save_summary("earlier today", 8).unwrap();
        assert_eq!(
            store.load_summary().unwrap(),
            Some(("earlier today".to_owned(), 8))
        );

        // A second save replaces the single row (doesn't accumulate).
        store.save_summary("earlier, updated", 20).unwrap();
        assert_eq!(
            store.load_summary().unwrap(),
            Some(("earlier, updated".to_owned(), 20))
        );
    }
}
