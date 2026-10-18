//! The team's map in HQ's store (ADR 0126). Its tables are made only if missing, so a live store
//! gains them empty and no row of any other table changes.

use rusqlite::params;

use super::{SqliteTeamStore, backend};
use crate::application::map_ports::MapStore;
use crate::application::ports::Stored;
use crate::domain::Millis;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS world_pages (
    slug TEXT PRIMARY KEY, created_at INTEGER NOT NULL
) STRICT;
";

impl MapStore for SqliteTeamStore {
    fn world_pages(&self) -> Stored<Vec<String>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT slug FROM world_pages ORDER BY created_at, rowid")
            .map_err(backend)?;
        stmt.query_map([], |r| r.get(0))
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<String>>>()
            .map_err(backend)
    }

    fn add_world_page(&self, slug: &str, at: Millis) -> Stored<bool> {
        let added = self
            .conn()?
            .execute(
                "INSERT OR IGNORE INTO world_pages (slug, created_at) VALUES (?1, ?2)",
                params![slug, at],
            )
            .map_err(backend)?;
        Ok(added == 1)
    }
}
