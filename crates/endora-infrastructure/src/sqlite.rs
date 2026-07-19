//! SQLite-backed implementations of the application repository ports.
//!
//! A single [`SqliteStore`] owns one connection behind a mutex and implements
//! the repository traits. Identifiers are `u128`, which SQLite integers cannot
//! hold, so they are stored as their decimal `TEXT` form and parsed back on
//! read. The store is synchronous; when the async node uses it, calls run on a
//! blocking thread (see `docs/adr/0007-async-web-stack.md`).

use std::sync::Mutex;

use endora_application::{DirectionRepository, GoalRepository, RepositoryError};
use endora_domain::{Direction, DirectionId, Goal, GoalId};
use rusqlite::{Connection, OptionalExtension, params};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS directions (
    id    TEXT PRIMARY KEY,
    title TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS goals (
    id           TEXT PRIMARY KEY,
    direction_id TEXT NOT NULL REFERENCES directions(id),
    statement    TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_goals_direction ON goals(direction_id);
";

/// A SQLite-backed store implementing the persistence ports.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Opens (creating if needed) a store at `path` and applies the schema.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be opened or migrated.
    pub fn open(path: &str) -> Result<Self, RepositoryError> {
        let conn = Connection::open(path).map_err(backend)?;
        Self::from_connection(conn)
    }

    /// Opens a private in-memory store, mainly for tests.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be created or migrated.
    pub fn open_in_memory() -> Result<Self, RepositoryError> {
        let conn = Connection::open_in_memory().map_err(backend)?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, RepositoryError> {
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(backend)?;
        conn.execute_batch(SCHEMA).map_err(backend)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, RepositoryError> {
        self.conn
            .lock()
            .map_err(|_| RepositoryError::Backend("connection lock poisoned".to_owned()))
    }
}

impl DirectionRepository for SqliteStore {
    fn save(&self, direction: &Direction) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO directions (id, title) VALUES (?1, ?2)",
            params![id_text(direction.id().value()), direction.title()],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: DirectionId) -> Result<Option<Direction>, RepositoryError> {
        let conn = self.lock()?;
        let title: Option<String> = conn
            .query_row(
                "SELECT title FROM directions WHERE id = ?1",
                params![id_text(id.value())],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(title) = title else {
            return Ok(None);
        };
        let direction = Direction::new(id, &title).map_err(corrupt)?;
        Ok(Some(direction))
    }
}

impl GoalRepository for SqliteStore {
    fn save(&self, goal: &Goal) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO goals (id, direction_id, statement) VALUES (?1, ?2, ?3)",
            params![
                id_text(goal.id().value()),
                id_text(goal.direction().value()),
                goal.statement()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: GoalId) -> Result<Option<Goal>, RepositoryError> {
        let conn = self.lock()?;
        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT direction_id, statement FROM goals WHERE id = ?1",
                params![id_text(id.value())],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((direction_id, statement)) = row else {
            return Ok(None);
        };
        let direction = DirectionId::new(parse_id(&direction_id)?);
        let goal = Goal::new(id, direction, &statement).map_err(corrupt)?;
        Ok(Some(goal))
    }

    fn list_for_direction(&self, direction: DirectionId) -> Result<Vec<Goal>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, statement FROM goals WHERE direction_id = ?1 ORDER BY id")
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![id_text(direction.value())], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(backend)?;

        let mut goals = Vec::new();
        for row in rows {
            let (id_text, statement) = row.map_err(backend)?;
            let goal = Goal::new(GoalId::new(parse_id(&id_text)?), direction, &statement)
                .map_err(corrupt)?;
            goals.push(goal);
        }
        Ok(goals)
    }
}

/// Renders a `u128` identifier as the decimal `TEXT` used for storage.
fn id_text(value: u128) -> String {
    value.to_string()
}

/// Parses a stored identifier back into a `u128`, or reports corruption.
fn parse_id(text: &str) -> Result<u128, RepositoryError> {
    text.parse::<u128>()
        .map_err(|e| RepositoryError::Corrupt(format!("invalid stored id {text:?}: {e}")))
}

/// Maps any backend error into a [`RepositoryError::Backend`].
fn backend(error: impl core::fmt::Display) -> RepositoryError {
    RepositoryError::Backend(error.to_string())
}

/// Maps a domain reconstruction failure into a [`RepositoryError::Corrupt`].
fn corrupt(error: impl core::fmt::Display) -> RepositoryError {
    RepositoryError::Corrupt(error.to_string())
}

#[cfg(test)]
mod tests {
    // Callers depend on a *port*, not the concrete store: `SqliteStore`
    // implements several repository traits, so we exercise it through the trait
    // (as real use cases do) rather than calling ambiguous methods directly.
    use super::SqliteStore;
    use endora_application::{DirectionRepository, GoalRepository};
    use endora_domain::{Direction, DirectionId, Goal, GoalId};

    fn store() -> SqliteStore {
        SqliteStore::open_in_memory().unwrap()
    }

    #[test]
    fn direction_round_trips() {
        let store = store();
        let repo: &dyn DirectionRepository = &store;
        let direction = Direction::new(DirectionId::new(1), "Be healthier").unwrap();
        repo.save(&direction).unwrap();
        assert_eq!(repo.get(DirectionId::new(1)).unwrap(), Some(direction));
    }

    #[test]
    fn missing_direction_is_none() {
        let store = store();
        let repo: &dyn DirectionRepository = &store;
        assert_eq!(repo.get(DirectionId::new(99)).unwrap(), None);
    }

    #[test]
    fn save_replaces_existing_direction() {
        let store = store();
        let repo: &dyn DirectionRepository = &store;
        repo.save(&Direction::new(DirectionId::new(1), "First").unwrap())
            .unwrap();
        repo.save(&Direction::new(DirectionId::new(1), "Second").unwrap())
            .unwrap();
        assert_eq!(
            repo.get(DirectionId::new(1)).unwrap().unwrap().title(),
            "Second"
        );
    }

    #[test]
    fn goal_round_trips_and_lists_by_direction() {
        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let goals: &dyn GoalRepository = &store;
        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();

        let g1 = Goal::new(GoalId::new(10), direction, "Run a 5k").unwrap();
        let g2 = Goal::new(GoalId::new(11), direction, "Sleep 8h").unwrap();
        goals.save(&g1).unwrap();
        goals.save(&g2).unwrap();

        assert_eq!(goals.get(GoalId::new(10)).unwrap(), Some(g1.clone()));
        assert_eq!(goals.list_for_direction(direction).unwrap(), vec![g1, g2]);
    }

    #[test]
    fn a_large_u128_id_survives_storage() {
        let store = store();
        let repo: &dyn DirectionRepository = &store;
        let big = u128::MAX;
        repo.save(&Direction::new(DirectionId::new(big), "Edge").unwrap())
            .unwrap();
        assert_eq!(
            repo.get(DirectionId::new(big)).unwrap().unwrap().id(),
            DirectionId::new(big)
        );
    }
}
