//! Call receipts, scenario actions and citations in HQ's store.

use rusqlite::{OptionalExtension, params};

use super::{RECEIPT_SELECT, SqliteTeamStore, backend, call_receipt_from};
use crate::application::ports::{Post, Stored, ThreadPost};
use crate::application::receipt_ports::ReceiptStore;
use crate::domain::{MemberId, RunId, TaskId};

impl ReceiptStore for SqliteTeamStore {
    fn add_call_receipt(&self, r: &crate::domain::CallReceipt) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO call_receipts (run, member, task, tool, args, excerpt, ok, act,
                confidential, at_ms, sandbox) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                r.run.0,
                r.member.0,
                r.task.map(|t| t.0),
                r.tool,
                r.args,
                r.excerpt,
                r.ok,
                r.act,
                r.confidential,
                r.at,
                r.sandbox
            ],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn call_receipt(&self, id: i64) -> Stored<Option<crate::domain::CallReceipt>> {
        self.conn()?
            .query_row(
                &format!("{RECEIPT_SELECT} WHERE id = ?1"),
                [id],
                call_receipt_from,
            )
            .optional()
            .map_err(backend)
    }

    fn call_receipts_of(&self, run: RunId) -> Stored<Vec<crate::domain::CallReceipt>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{RECEIPT_SELECT} WHERE run = ?1 ORDER BY id"))
            .map_err(backend)?;
        let rows = stmt
            .query_map([run.0], call_receipt_from)
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn add_scenario_action(&self, a: &crate::application::ports::ScenarioAction) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO scenario_actions (task, run, member, action, ok, said, at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![a.task.0, a.run.0, a.member.0, a.action, a.ok, a.said, a.at],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn scenario_actions(
        &self,
        task: TaskId,
    ) -> Stored<Vec<crate::application::ports::ScenarioAction>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task, run, member, action, ok, said, at_ms FROM scenario_actions
                 WHERE task = ?1 ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([task.0], |row| {
                Ok(crate::application::ports::ScenarioAction {
                    id: row.get(0)?,
                    task: TaskId(row.get(1)?),
                    run: RunId(row.get(2)?),
                    member: MemberId(row.get(3)?),
                    action: row.get(4)?,
                    ok: row.get(5)?,
                    said: row.get(6)?,
                    at: row.get(7)?,
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn add_cited(&self, c: &crate::application::ports::Cited) -> Stored<()> {
        self.conn()?
            .execute(
                "INSERT INTO cited (task, run, member, stage, criterion, refs, at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    c.task.0,
                    c.run.0,
                    c.member.0,
                    c.stage,
                    i64::try_from(c.criterion).unwrap_or(0),
                    crate::domain::Ref::kept(&c.refs),
                    c.at
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn cited(&self, task: TaskId) -> Stored<Vec<crate::application::ports::Cited>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT task, run, member, stage, criterion, refs, at_ms FROM cited
                 WHERE task = ?1 ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([task.0], |r| {
                Ok(crate::application::ports::Cited {
                    task: TaskId(r.get(0)?),
                    run: RunId(r.get(1)?),
                    member: MemberId(r.get(2)?),
                    stage: r.get(3)?,
                    criterion: usize::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
                    refs: crate::domain::Ref::read(&r.get::<_, String>(5)?),
                    at: r.get(6)?,
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn thread_cited(&self, task: TaskId) -> Stored<Vec<ThreadPost>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT author, text, at_ms, unsupported, evidence FROM posts WHERE task = ?1
                 ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([task.0], |r| {
                Ok(ThreadPost {
                    task,
                    post: Post {
                        author: r.get(0)?,
                        text: r.get(1)?,
                        at: r.get(2)?,
                        unsupported: r.get::<_, i64>(3)? != 0,
                    },
                    evidence: crate::domain::Ref::read(&r.get::<_, String>(4)?),
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }
}
