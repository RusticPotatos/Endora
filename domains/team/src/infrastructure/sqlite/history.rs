//! The team's history in HQ's store (ADR 0129). Its tables are made only if missing, so a live
//! store gains them empty and no row of any other table changes; running the schema again
//! changes nothing.

use rusqlite::params;

use super::{SqliteTeamStore, backend};
use crate::application::history_ports::HistoryStore;
use crate::application::ports::Stored;
use crate::domain::history::{self, Kept, Transcript};
use crate::domain::{MemberId, RunId, TaskId};

pub(super) const SCHEMA: &str = "
-- One turn's conversation as plain text, scrubbed by the node and capped (ADR 0129). Kept for
-- thirty days and the last five of a member on a task; the retention rule lets the rest go.
CREATE TABLE IF NOT EXISTS transcripts (
    run INTEGER PRIMARY KEY REFERENCES runs(id), member INTEGER NOT NULL REFERENCES members(id),
    task INTEGER REFERENCES tasks(id), runtime TEXT NOT NULL, brain TEXT NOT NULL,
    tier TEXT NOT NULL, body TEXT NOT NULL, est_tokens INTEGER NOT NULL, cut INTEGER NOT NULL,
    at_ms INTEGER NOT NULL, kept_until INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS transcripts_by_member ON transcripts(member, task);
";

/// The full-text index over the conversations, kept apart because it is a virtual table: SQLite
/// built with FTS5 (the bundled one is) makes it, and the triggers keep it as the rows come and
/// go. A store that cannot make it searches the conversations without it.
pub(super) const FULL_TEXT: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS transcripts_fts
    USING fts5(body, content='transcripts', content_rowid='run');
CREATE TRIGGER IF NOT EXISTS transcripts_fts_added AFTER INSERT ON transcripts BEGIN
    INSERT INTO transcripts_fts(rowid, body) VALUES (new.run, new.body);
END;
CREATE TRIGGER IF NOT EXISTS transcripts_fts_gone AFTER DELETE ON transcripts BEGIN
    INSERT INTO transcripts_fts(transcripts_fts, rowid, body) VALUES ('delete', old.run, old.body);
END;
";

const TRANSCRIPT_SELECT: &str = "SELECT run, member, task, runtime, brain, tier, body, est_tokens, \
    cut, at_ms, kept_until FROM transcripts";

impl HistoryStore for SqliteTeamStore {
    fn add_transcript(&self, t: &Transcript) -> Stored<()> {
        self.conn()?
            .execute(
                "INSERT INTO transcripts (run, member, task, runtime, brain, tier, body,
                    est_tokens, cut, at_ms, kept_until)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    t.run.0,
                    t.member.0,
                    t.task.map(|x| x.0),
                    t.runtime,
                    t.brain,
                    history::tier_name(t.tier),
                    t.body,
                    t.est_tokens,
                    t.cut,
                    t.at,
                    t.kept_until
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn transcript(&self, run: RunId) -> Stored<Option<Transcript>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{TRANSCRIPT_SELECT} WHERE run = ?1"))
            .map_err(backend)?;
        let mut rows = stmt
            .query_map([run.0], |r| {
                Ok(Transcript {
                    run: RunId(r.get(0)?),
                    member: MemberId(r.get(1)?),
                    task: r.get::<_, Option<i64>>(2)?.map(TaskId),
                    runtime: r.get(3)?,
                    brain: r.get(4)?,
                    tier: history::tier_from_name(&r.get::<_, String>(5)?),
                    body: r.get(6)?,
                    est_tokens: r.get(7)?,
                    cut: r.get(8)?,
                    at: r.get(9)?,
                    kept_until: r.get(10)?,
                })
            })
            .map_err(backend)?;
        rows.next().transpose().map_err(backend)
    }

    fn transcripts_kept(&self) -> Stored<Vec<Kept>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT run, member, task, at_ms, kept_until FROM transcripts ORDER BY at_ms, run",
            )
            .map_err(backend)?;
        stmt.query_map([], |r| {
            Ok(Kept {
                run: RunId(r.get(0)?),
                member: MemberId(r.get(1)?),
                task: r.get::<_, Option<i64>>(2)?.map(TaskId),
                at: r.get(3)?,
                kept_until: r.get(4)?,
            })
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn forget_transcripts(&self, runs: &[RunId]) -> Stored<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(backend)?;
        let mut gone = 0;
        for run in runs {
            gone += tx
                .execute("DELETE FROM transcripts WHERE run = ?1", [run.0])
                .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(gone)
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;

    /// The bundled SQLite is built with FTS5 (rusqlite's `bundled` feature compiles it in), so
    /// the conversations are searched through an index with no new dependency.
    #[test]
    fn the_bundled_sqlite_has_fts5_and_the_history_schema_makes_its_index() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE VIRTUAL TABLE probe USING fts5(body)")
            .expect("FTS5 is built into the bundled SQLite");
        conn.execute(
            "INSERT INTO probe (body) VALUES ('cap the sonarr memory')",
            [],
        )
        .unwrap();
        let found: i64 = conn
            .query_row(
                "SELECT count(*) FROM probe WHERE probe MATCH '\"cap\"* AND \"sonarr\"*'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(found, 1);
        let store = SqliteTeamStore::in_memory().unwrap();
        let made: i64 = store
            .conn()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'transcripts_fts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(made, 1);
    }

    /// Making the tables again, as a second start does, changes nothing and does not fail.
    #[test]
    fn the_schema_runs_again_over_a_store_that_has_it() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        for _ in 0..2 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS runs (id INTEGER PRIMARY KEY) STRICT;
                 CREATE TABLE IF NOT EXISTS members (id INTEGER PRIMARY KEY) STRICT;
                 CREATE TABLE IF NOT EXISTS tasks (id INTEGER PRIMARY KEY) STRICT;",
            )
            .unwrap();
            conn.execute_batch(SCHEMA).unwrap();
            conn.execute_batch(FULL_TEXT).unwrap();
        }
    }
}
