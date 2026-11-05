//! The wing's documents, branches and checks in HQ's store.

use rusqlite::{OptionalExtension, params};

use super::{DOCUMENT_SELECT, SqliteTeamStore, backend, document_row};
use crate::application::ports::Stored;
use crate::application::wing_ports::WingStore;
use crate::domain::{Branch, Check, Document, MemberId, RunId, TaskId};

impl WingStore for SqliteTeamStore {
    fn add_document(&self, d: &Document) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO documents (task, run, title, text, at_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![d.task.0, d.run.map(|r| r.0), d.title, d.text, d.at],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn document(&self, id: i64) -> Stored<Option<Document>> {
        self.conn()?
            .query_row(
                &format!("{DOCUMENT_SELECT} WHERE id = ?1"),
                [id],
                document_row,
            )
            .optional()
            .map_err(backend)
    }

    fn documents_of(&self, task: TaskId) -> Stored<Vec<Document>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{DOCUMENT_SELECT} WHERE task = ?1 ORDER BY id"))
            .map_err(backend)?;
        let rows = stmt.query_map([task.0], document_row).map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn add_branch(&self, b: &Branch) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO branches (task, run, member, name, head, base, files, plus, minus,
                can_push, status, at_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                b.task.0,
                b.run.0,
                b.member.0,
                b.name,
                b.head,
                b.base,
                i64::try_from(b.files).unwrap_or(i64::MAX),
                b.plus,
                b.minus,
                b.can_push,
                b.status,
                b.at
            ],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn branches(&self) -> Stored<Vec<Branch>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task, run, member, name, head, base, files, plus, minus, can_push,
                 status, at_ms FROM branches ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Branch {
                    id: r.get(0)?,
                    task: TaskId(r.get(1)?),
                    run: RunId(r.get(2)?),
                    member: MemberId(r.get(3)?),
                    name: r.get(4)?,
                    head: r.get(5)?,
                    base: r.get(6)?,
                    files: usize::try_from(r.get::<_, i64>(7)?).unwrap_or(0),
                    plus: r.get(8)?,
                    minus: r.get(9)?,
                    can_push: r.get::<_, i64>(10)? != 0,
                    status: r.get(11)?,
                    at: r.get(12)?,
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn save_branch(&self, b: &Branch) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE branches SET status = ?2 WHERE id = ?1",
                params![b.id, b.status],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn add_check(&self, c: &Check) -> Stored<(i64, String)> {
        // SQLite's own randomness for the job's token: 32 bytes, as hex.
        self.conn()?
            .query_row(
                "INSERT INTO checks (task, branch, run, token, status, queued_at)
                 VALUES (?1, ?2, ?3, lower(hex(randomblob(32))), 'waiting', ?4)
                 RETURNING id, token",
                params![c.task.0, c.branch, c.run.0, c.queued_at],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(backend)
    }

    fn checks(&self) -> Stored<Vec<Check>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task, branch, run, token, status, ok, summary, queued_at, started_at,
                 done_at FROM checks ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Check {
                    id: r.get(0)?,
                    task: TaskId(r.get(1)?),
                    branch: r.get(2)?,
                    run: RunId(r.get(3)?),
                    token: r.get(4)?,
                    status: r.get(5)?,
                    ok: r.get::<_, Option<i64>>(6)?.map(|ok| ok != 0),
                    summary: r.get(7)?,
                    queued_at: r.get(8)?,
                    started_at: r.get(9)?,
                    done_at: r.get(10)?,
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn save_check(&self, c: &Check) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE checks SET status = ?2, ok = ?3, summary = ?4, started_at = ?5, done_at = ?6
                 WHERE id = ?1",
                params![c.id, c.status, c.ok, c.summary, c.started_at, c.done_at],
            )
            .map_err(backend)?;
        Ok(())
    }
}
