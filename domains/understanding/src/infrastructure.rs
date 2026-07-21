//! Understanding infrastructure — SQLite-backed belief + preference repositories.

use endora_kernel::RepositoryError;
use endora_kernel::ids::{BeliefId, PreferenceId, Timestamp};
use endora_persistence::{Db, backend, corrupt, id_text, parse_id};
use rusqlite::{Connection, params};

use crate::application::{BeliefRepository, PreferenceRepository};
use crate::domain::{Belief, BeliefKind, BeliefStatus, Confidence, Preference, PreferenceKind};

/// Creates the understanding tables if absent (idempotent).
///
/// # Errors
/// [`RepositoryError::Backend`] if the schema cannot be applied.
pub fn migrate(db: &Db) -> Result<(), RepositoryError> {
    db.lock()?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS preferences (
                id    TEXT PRIMARY KEY,
                body  TEXT NOT NULL,
                kind  TEXT NOT NULL,
                at_ms INTEGER NOT NULL
            ) STRICT;
            CREATE TABLE IF NOT EXISTS beliefs (
                id               TEXT PRIMARY KEY,
                statement        TEXT NOT NULL,
                kind             TEXT NOT NULL,
                confidence       TEXT NOT NULL,
                evidence         TEXT NOT NULL,
                created_ms       INTEGER NOT NULL,
                last_affirmed_ms INTEGER NOT NULL,
                status           TEXT NOT NULL
            ) STRICT;",
        )
        .map_err(backend)?;
    Ok(())
}

/// SQLite-backed store for beliefs and preferences over the shared connection.
pub struct UnderstandingStore {
    db: Db,
}

impl UnderstandingStore {
    /// Builds the store over the shared connection handle.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl PreferenceRepository for UnderstandingStore {
    fn save(&self, preference: &Preference) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO preferences (id, body, kind, at_ms) VALUES (?1, ?2, ?3, ?4)",
                params![
                    id_text(preference.id().value()),
                    preference.text(),
                    preference.kind().name(),
                    preference.at().unix_millis()
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn list_all(&self) -> Result<Vec<Preference>, RepositoryError> {
        let conn = self.db.lock()?;
        all_preferences(&conn)
    }

    fn delete(&self, id: PreferenceId) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "DELETE FROM preferences WHERE id = ?1",
                params![id_text(id.value())],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl BeliefRepository for UnderstandingStore {
    fn save(&self, belief: &Belief) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO beliefs \
                 (id, statement, kind, confidence, evidence, created_ms, last_affirmed_ms, status) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id_text(belief.id().value()),
                    belief.statement(),
                    belief.kind().name(),
                    belief.confidence().name(),
                    belief.evidence(),
                    belief.created_at().unix_millis(),
                    belief.last_affirmed_at().unix_millis(),
                    belief.status().name(),
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: BeliefId) -> Result<Option<Belief>, RepositoryError> {
        let conn = self.db.lock()?;
        Ok(all_beliefs(&conn)?.into_iter().find(|b| b.id() == id))
    }

    fn list(&self) -> Result<Vec<Belief>, RepositoryError> {
        let conn = self.db.lock()?;
        all_beliefs(&conn)
    }
}

fn all_preferences(conn: &Connection) -> Result<Vec<Preference>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, body, kind, at_ms FROM preferences ORDER BY at_ms, rowid")
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
        let (id, body, kind, at_ms) = row.map_err(backend)?;
        let kind = PreferenceKind::from_name(&kind)
            .ok_or_else(|| RepositoryError::Corrupt(format!("unknown preference kind {kind:?}")))?;
        out.push(
            Preference::new(
                PreferenceId::new(parse_id(&id)?),
                &body,
                kind,
                Timestamp::from_unix_millis(at_ms),
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
}

fn all_beliefs(conn: &Connection) -> Result<Vec<Belief>, RepositoryError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, statement, kind, confidence, evidence, created_ms, last_affirmed_ms, status \
             FROM beliefs ORDER BY last_affirmed_ms DESC, rowid DESC",
        )
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, String>(7)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, statement, kind, confidence, evidence, created_ms, affirmed_ms, status) =
            row.map_err(backend)?;
        out.push(Belief::from_parts(
            BeliefId::new(parse_id(&id)?),
            statement,
            BeliefKind::from_name(&kind),
            Confidence::from_name(&confidence),
            evidence,
            Timestamp::from_unix_millis(created_ms),
            Timestamp::from_unix_millis(affirmed_ms),
            BeliefStatus::from_name(&status),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{UnderstandingStore, migrate};
    use crate::application::{BeliefRepository, PreferenceRepository};
    use crate::domain::{Preference, PreferenceKind};
    use endora_kernel::ids::{PreferenceId, Timestamp};
    use endora_persistence::Db;

    #[test]
    fn preferences_round_trip_and_delete() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);
        let p = Preference::new(
            PreferenceId::new(1),
            "prefers mornings",
            PreferenceKind::Taste,
            Timestamp::from_unix_millis(10),
        )
        .unwrap();
        PreferenceRepository::save(&store, &p).unwrap();
        assert_eq!(store.list_all().unwrap().len(), 1);
        store.delete(PreferenceId::new(1)).unwrap();
        assert!(store.list_all().unwrap().is_empty());
    }

    #[test]
    fn beliefs_list_is_empty_initially() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);
        assert!(BeliefRepository::list(&store).unwrap().is_empty());
    }
}
