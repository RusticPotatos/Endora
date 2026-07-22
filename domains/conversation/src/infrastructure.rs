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

impl ChatRepository for ChatStore {
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
}
