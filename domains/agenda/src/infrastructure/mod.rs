//! The agenda's tables, in the node's one database.

use crate::application::{AgendaError, AgendaStore, Done, Item, Rule};
use crate::domain::{Family, Kind, Level, Response, Source};
use endora_persistence::Db;
use rusqlite::{OptionalExtension, Row, params};

/// Makes the agenda's tables.
///
/// # Errors
/// The database failed.
pub fn migrate(db: &Db) -> Done<()> {
    db.lock()
        .map_err(failed)?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS agenda_items (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                key         TEXT NOT NULL,
                source      TEXT NOT NULL,
                kind        TEXT NOT NULL,
                about       TEXT NOT NULL,
                sentence    TEXT NOT NULL,
                why         TEXT NOT NULL,
                level       TEXT NOT NULL,
                response    TEXT NOT NULL,
                target      TEXT,
                opened_at   INTEGER NOT NULL,
                updated_at  INTEGER NOT NULL,
                watch_until INTEGER,
                then_level  TEXT,
                pushed      INTEGER NOT NULL DEFAULT 0,
                closed_at   INTEGER,
                closed_how  TEXT
            );
            CREATE INDEX IF NOT EXISTS agenda_items_open ON agenda_items (closed_at);
            CREATE INDEX IF NOT EXISTS agenda_items_key ON agenda_items (key);
            CREATE TABLE IF NOT EXISTS agenda_restarts (container TEXT NOT NULL, at INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS agenda_rules (
                id     INTEGER PRIMARY KEY AUTOINCREMENT,
                family TEXT NOT NULL,
                about  TEXT NOT NULL,
                at     INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS agenda_responses (kind TEXT PRIMARY KEY, response TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS agenda_pushes (at INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS agenda_checked (source TEXT PRIMARY KEY, at INTEGER NOT NULL);",
        )
        .map_err(failed)
}

/// The agenda's store, over the node's shared connection.
pub struct SqliteAgendaStore {
    db: Db,
}

impl SqliteAgendaStore {
    /// Over the shared connection; call [`migrate`] first.
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }
}

fn failed(e: impl std::fmt::Display) -> AgendaError {
    AgendaError(format!("agenda store: {e}"))
}

const ITEM: &str = "SELECT id, key, source, kind, about, sentence, why, level, response, target,
    opened_at, updated_at, watch_until, then_level, pushed, closed_at, closed_how FROM agenda_items";

fn item(row: &Row<'_>) -> rusqlite::Result<Item> {
    let bad = |what: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown {what}").into(),
        )
    };
    let source: String = row.get(2)?;
    let kind: String = row.get(3)?;
    let level: String = row.get(7)?;
    let response: String = row.get(8)?;
    let then: Option<String> = row.get(13)?;
    Ok(Item {
        id: row.get(0)?,
        key: row.get(1)?,
        source: Source::from_name(&source).ok_or_else(|| bad("source"))?,
        kind: Kind::from_name(&kind).ok_or_else(|| bad("kind"))?,
        about: row.get(4)?,
        sentence: row.get(5)?,
        why: row.get(6)?,
        level: Level::from_name(&level).ok_or_else(|| bad("level"))?,
        response: Response::from_name(&response).ok_or_else(|| bad("response"))?,
        target: row.get(9)?,
        opened_at: row.get(10)?,
        updated_at: row.get(11)?,
        watch_until: row.get(12)?,
        then: then.as_deref().and_then(Level::from_name),
        pushed: row.get::<_, i64>(14)? != 0,
        closed_at: row.get(15)?,
        closed_how: row.get(16)?,
    })
}

impl AgendaStore for SqliteAgendaStore {
    fn open_items(&self) -> Done<Vec<Item>> {
        let conn = self.db.lock().map_err(failed)?;
        let mut stmt = conn
            .prepare(&format!("{ITEM} WHERE closed_at IS NULL ORDER BY id"))
            .map_err(failed)?;
        let rows = stmt.query_map([], item).map_err(failed)?;
        rows.collect::<Result<_, _>>().map_err(failed)
    }

    fn item(&self, id: i64) -> Done<Option<Item>> {
        let conn = self.db.lock().map_err(failed)?;
        conn.query_row(&format!("{ITEM} WHERE id = ?1"), [id], item)
            .optional()
            .map_err(failed)
    }

    fn ever_had(&self, key: &str) -> Done<bool> {
        let conn = self.db.lock().map_err(failed)?;
        conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM agenda_items WHERE key = ?1)",
            [key],
            |r| r.get(0),
        )
        .map_err(failed)
    }

    fn add_item(&self, i: &Item) -> Done<i64> {
        let conn = self.db.lock().map_err(failed)?;
        conn.execute(
            "INSERT INTO agenda_items (key, source, kind, about, sentence, why, level, response,
                target, opened_at, updated_at, watch_until, then_level, pushed, closed_at, closed_how)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                i.key,
                i.source.name(),
                i.kind.name(),
                i.about,
                i.sentence,
                i.why,
                i.level.name(),
                i.response.name(),
                i.target,
                i.opened_at,
                i.updated_at,
                i.watch_until,
                i.then.map(Level::name),
                i64::from(i.pushed),
                i.closed_at,
                i.closed_how,
            ],
        )
        .map_err(failed)?;
        Ok(conn.last_insert_rowid())
    }

    fn save_item(&self, i: &Item) -> Done<()> {
        let conn = self.db.lock().map_err(failed)?;
        conn.execute(
            "UPDATE agenda_items SET sentence = ?2, why = ?3, level = ?4, target = ?5,
                updated_at = ?6, watch_until = ?7, then_level = ?8, pushed = ?9, closed_at = ?10,
                closed_how = ?11
             WHERE id = ?1",
            params![
                i.id,
                i.sentence,
                i.why,
                i.level.name(),
                i.target,
                i.updated_at,
                i.watch_until,
                i.then.map(Level::name),
                i64::from(i.pushed),
                i.closed_at,
                i.closed_how,
            ],
        )
        .map_err(failed)?;
        Ok(())
    }

    fn prune_closed(&self, before: i64) -> Done<usize> {
        let conn = self.db.lock().map_err(failed)?;
        let gone = conn
            .execute(
                "DELETE FROM agenda_items WHERE closed_at IS NOT NULL AND closed_at < ?1",
                [before],
            )
            .map_err(failed)?;
        conn.execute("DELETE FROM agenda_restarts WHERE at < ?1", [before])
            .map_err(failed)?;
        conn.execute("DELETE FROM agenda_pushes WHERE at < ?1", [before])
            .map_err(failed)?;
        Ok(gone)
    }

    fn restarts_since(&self, since: i64) -> Done<Vec<(String, i64)>> {
        let conn = self.db.lock().map_err(failed)?;
        let mut stmt = conn
            .prepare("SELECT container, at FROM agenda_restarts WHERE at >= ?1 ORDER BY at")
            .map_err(failed)?;
        let rows = stmt
            .query_map([since], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(failed)?;
        rows.collect::<Result<_, _>>().map_err(failed)
    }

    fn add_restart(&self, container: &str, at: i64) -> Done<()> {
        let conn = self.db.lock().map_err(failed)?;
        conn.execute(
            "INSERT INTO agenda_restarts (container, at) VALUES (?1, ?2)",
            params![container, at],
        )
        .map_err(failed)?;
        Ok(())
    }

    fn rules(&self) -> Done<Vec<Rule>> {
        let conn = self.db.lock().map_err(failed)?;
        let mut stmt = conn
            .prepare("SELECT id, family, about, at FROM agenda_rules ORDER BY id")
            .map_err(failed)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(failed)?;
        let mut rules = Vec::new();
        for row in rows {
            let (id, family, about, at) = row.map_err(failed)?;
            // A rule of a family this build does not know is kept, and simply covers nothing.
            if let Some(family) = Family::from_name(&family) {
                rules.push(Rule {
                    id,
                    family,
                    about,
                    at,
                });
            }
        }
        Ok(rules)
    }

    fn add_rule(&self, family: Family, about: &str, at: i64) -> Done<i64> {
        let conn = self.db.lock().map_err(failed)?;
        conn.execute(
            "INSERT INTO agenda_rules (family, about, at) VALUES (?1, ?2, ?3)",
            params![family.name(), about, at],
        )
        .map_err(failed)?;
        Ok(conn.last_insert_rowid())
    }

    fn remove_rule(&self, id: i64) -> Done<Option<Rule>> {
        let found = self.rules()?.into_iter().find(|r| r.id == id);
        if found.is_some() {
            let conn = self.db.lock().map_err(failed)?;
            conn.execute("DELETE FROM agenda_rules WHERE id = ?1", [id])
                .map_err(failed)?;
        }
        Ok(found)
    }

    fn responses(&self) -> Done<Vec<(Kind, Response)>> {
        let conn = self.db.lock().map_err(failed)?;
        let mut stmt = conn
            .prepare("SELECT kind, response FROM agenda_responses")
            .map_err(failed)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(failed)?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, response) = row.map_err(failed)?;
            if let (Some(k), Some(r)) = (Kind::from_name(&kind), Response::from_name(&response)) {
                out.push((k, r));
            }
        }
        Ok(out)
    }

    fn set_response(&self, kind: Kind, response: Option<Response>) -> Done<()> {
        let conn = self.db.lock().map_err(failed)?;
        match response {
            Some(r) => conn.execute(
                "INSERT INTO agenda_responses (kind, response) VALUES (?1, ?2)
                 ON CONFLICT (kind) DO UPDATE SET response = excluded.response",
                params![kind.name(), r.name()],
            ),
            None => conn.execute(
                "DELETE FROM agenda_responses WHERE kind = ?1",
                [kind.name()],
            ),
        }
        .map_err(failed)?;
        Ok(())
    }

    fn pushes_since(&self, since: i64) -> Done<usize> {
        let conn = self.db.lock().map_err(failed)?;
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agenda_pushes WHERE at >= ?1",
                [since],
                |r| r.get(0),
            )
            .map_err(failed)?;
        Ok(usize::try_from(n).unwrap_or(0))
    }

    fn add_push(&self, at: i64) -> Done<()> {
        let conn = self.db.lock().map_err(failed)?;
        conn.execute("INSERT INTO agenda_pushes (at) VALUES (?1)", [at])
            .map_err(failed)?;
        Ok(())
    }

    fn checked(&self) -> Done<Vec<(Source, i64)>> {
        let conn = self.db.lock().map_err(failed)?;
        let mut stmt = conn
            .prepare("SELECT source, at FROM agenda_checked")
            .map_err(failed)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(failed)?;
        let mut out = Vec::new();
        for row in rows {
            let (source, at) = row.map_err(failed)?;
            if let Some(s) = Source::from_name(&source) {
                out.push((s, at));
            }
        }
        Ok(out)
    }

    fn set_checked(&self, source: Source, at: i64) -> Done<()> {
        let conn = self.db.lock().map_err(failed)?;
        conn.execute(
            "INSERT INTO agenda_checked (source, at) VALUES (?1, ?2)
             ON CONFLICT (source) DO UPDATE SET at = excluded.at",
            params![source.name(), at],
        )
        .map_err(failed)?;
        Ok(())
    }
}
