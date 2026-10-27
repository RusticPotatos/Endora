//! Turns and the money they spend, in HQ's store.

use rusqlite::{OptionalExtension, params};

use super::{
    PROJECTS_ROWS, RUN_COLUMNS, RUNS_OWNER, RUNS_PROJECT, SqliteTeamStore, backend, history,
    run_from,
};
use crate::application::ports::{LedgerRow, Run, Stored};
use crate::application::run_ports::RunStore;
use crate::domain::money::Micros;
use crate::domain::{Brain, MemberId, Millis, ProjectId, RunId, RunStatus, TaskId, WakeId};

impl RunStore for SqliteTeamStore {
    fn add_run(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        wake: WakeId,
        at: Millis,
    ) -> Stored<RunId> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO runs (member, task, wake, status, queued_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                member.0,
                task.map(|t| t.0),
                wake.0,
                RunStatus::Queued.name(),
                at
            ],
        )
        .map_err(backend)?;
        Ok(RunId(conn.last_insert_rowid()))
    }

    fn run(&self, id: RunId) -> Stored<Option<Run>> {
        self.conn()?
            .query_row(
                &format!("SELECT {RUN_COLUMNS} FROM runs WHERE id = ?1"),
                [id.0],
                run_from,
            )
            .optional()
            .map_err(backend)
    }

    fn runs(&self) -> Stored<Vec<Run>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("SELECT {RUN_COLUMNS} FROM runs ORDER BY id"))
            .map_err(backend)?;
        let rows = stmt.query_map([], run_from).map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn runs_of(&self, task: TaskId) -> Stored<Vec<Run>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT {RUN_COLUMNS} FROM runs WHERE task = ?1 ORDER BY id"
            ))
            .map_err(backend)?;
        let rows = stmt.query_map([task.0], run_from).map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn live_runs(&self) -> Stored<Vec<Run>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT {RUN_COLUMNS} FROM runs \
                 WHERE status IN ('queued','running','retry_waiting') ORDER BY id"
            ))
            .map_err(backend)?;
        let rows = stmt.query_map([], run_from).map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn issue_token(&self, run: RunId) -> Stored<String> {
        // SQLite's own randomness: 32 bytes, as hex.
        self.conn()?
            .query_row(
                "INSERT INTO run_tokens (run, token) VALUES (?1, lower(hex(randomblob(32))))
                 ON CONFLICT (run) DO UPDATE SET token = lower(hex(randomblob(32)))
                 RETURNING token",
                [run.0],
                |r| r.get(0),
            )
            .map_err(backend)
    }

    fn run_of_token(&self, token: &str) -> Stored<Option<RunId>> {
        self.conn()?
            .query_row(
                "SELECT run FROM run_tokens WHERE token = ?1",
                [token],
                |r| r.get(0).map(RunId),
            )
            .optional()
            .map_err(backend)
    }

    fn add_tokens(
        &self,
        run: RunId,
        fresh: i64,
        read: i64,
        written: i64,
        output: i64,
    ) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE runs SET input_tokens = input_tokens + ?2,
                    cache_read_tokens = cache_read_tokens + ?3,
                    cache_write_tokens = cache_write_tokens + ?4,
                    output_tokens = output_tokens + ?5
                 WHERE id = ?1",
                params![
                    run.0,
                    fresh.max(0),
                    read.max(0),
                    written.max(0),
                    output.max(0)
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn save_run(&self, run: &Run) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE runs SET status = ?2, attempts = ?3, started_at = ?4, retry_at = ?5,
                    ended_at = ?6, reserved = ?7, spent = ?8, told = ?9, end_reason = ?10,
                    moved_forward = ?11, posted = ?12, blocked = ?13, label = ?14,
                    asked_by_owner = ?15, brain = ?16, interrupted = ?17
                 WHERE id = ?1",
                params![
                    run.id.0,
                    run.status.name(),
                    run.attempts,
                    run.started_at,
                    run.retry_at,
                    run.ended_at,
                    run.reserved,
                    run.spent,
                    run.told,
                    run.end_reason,
                    run.moved_forward,
                    run.posted,
                    run.blocked,
                    run.label.map(crate::domain::Label::name),
                    run.asked_by_owner,
                    run.brain.map(Brain::name),
                    run.interrupted
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn reserve(
        &self,
        run: RunId,
        member: MemberId,
        day: i64,
        amount: Micros,
        at: Millis,
    ) -> Stored<()> {
        self.conn()?
            .execute(
                &format!(
                    "INSERT INTO ledger (run, member, day, kind, amount, at_ms, project, owner)
                     VALUES (?1, ?2, ?3, 'reserve', ?4, ?5,
                        COALESCE({consult_project},
                                 (SELECT t.project FROM runs r JOIN tasks t ON t.id = r.task WHERE r.id = ?1)),
                        {RUNS_OWNER})",consult_project=history::CONSULT_PROJECT_FOR_RUN
                ),
                params![run.0, member.0, day, amount, at],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn record_use(
        &self,
        run: RunId,
        member: MemberId,
        day: i64,
        amount: Micros,
        at: Millis,
    ) -> Stored<()> {
        self.conn()?
            .execute(
                &format!(
                    "INSERT INTO ledger (run, member, day, kind, amount, at_ms, project, owner)
                     VALUES (?1, ?2, ?3, 'use', ?4, ?5, {RUNS_PROJECT}, {RUNS_OWNER})"
                ),
                params![run.0, member.0, day, amount, at],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn settle(&self, run: RunId, member: MemberId, day: i64, at: Millis) -> Stored<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(backend)?;
        let sum = |kind: &str| -> Stored<Micros> {
            tx.query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM ledger WHERE run = ?1 AND kind = ?2",
                params![run.0, kind],
                |r| r.get(0),
            )
            .map_err(backend)
        };
        let to_charge = sum("use")? - sum("charge")?;
        let to_release = sum("reserve")? - sum("release")?;
        // Booked on the day the reservation was made, so a turn that crosses midnight is
        // counted once, on the day it started.
        let booked: i64 = tx
            .query_row(
                "SELECT day FROM ledger WHERE run = ?1 AND kind = 'reserve' ORDER BY id DESC LIMIT 1",
                [run.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(backend)?
            .unwrap_or(day);
        for (kind, amount) in [("charge", to_charge), ("release", to_release)] {
            if amount != 0 {
                tx.execute(
                    &format!(
                        "INSERT INTO ledger (run, member, day, kind, amount, at_ms, project, owner)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, {RUNS_PROJECT}, {RUNS_OWNER})"
                    ),
                    params![run.0, member.0, booked, kind, amount, at],
                )
                .map_err(backend)?;
            }
        }
        tx.commit().map_err(backend)
    }

    fn committed(&self, member: Option<MemberId>, day: i64) -> Stored<Micros> {
        self.conn()?
            .query_row(
                "SELECT COALESCE(SUM(CASE kind WHEN 'charge' THEN amount WHEN 'reserve' THEN amount
                                              WHEN 'release' THEN -amount ELSE 0 END), 0)
                 FROM ledger WHERE day = ?1 AND (?2 IS NULL OR member = ?2)",
                params![day, member.map(|m| m.0)],
                |r| r.get(0),
            )
            .map_err(backend)
    }

    fn share_committed(&self, member: MemberId, day: i64) -> Stored<Micros> {
        self.conn()?
            .query_row(
                "SELECT COALESCE(SUM(CASE kind WHEN 'charge' THEN amount WHEN 'reserve' THEN amount
                                              WHEN 'release' THEN -amount ELSE 0 END), 0)
                 FROM ledger WHERE day = ?1 AND member = ?2 AND owner = 0",
                params![day, member.0],
                |r| r.get(0),
            )
            .map_err(backend)
    }

    fn spent(&self, member: Option<MemberId>, day: i64) -> Stored<Micros> {
        self.conn()?
            .query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM ledger
                 WHERE kind = 'use' AND day = ?1 AND (?2 IS NULL OR member = ?2)",
                params![day, member.map(|m| m.0)],
                |r| r.get(0),
            )
            .map_err(backend)
    }

    fn project_committed(&self, project: ProjectId, from: Millis, until: Millis) -> Stored<Micros> {
        self.conn()?
            .query_row(
                &format!(
                    "SELECT COALESCE(SUM(CASE l.kind WHEN 'charge' THEN l.amount
                        WHEN 'reserve' THEN l.amount WHEN 'release' THEN -l.amount ELSE 0 END), 0)
                     {PROJECTS_ROWS}"
                ),
                params![project.0, from, until],
                |r| r.get(0),
            )
            .map_err(backend)
    }

    fn project_spent(&self, project: ProjectId, from: Millis, until: Millis) -> Stored<Micros> {
        self.conn()?
            .query_row(
                &format!(
                    "SELECT COALESCE(SUM(CASE l.kind WHEN 'use' THEN l.amount ELSE 0 END), 0)
                     {PROJECTS_ROWS}"
                ),
                params![project.0, from, until],
                |r| r.get(0),
            )
            .map_err(backend)
    }

    fn ledger_of(&self, run: RunId) -> Stored<Vec<LedgerRow>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT kind, amount, at_ms FROM ledger WHERE run = ?1 ORDER BY id")
            .map_err(backend)?;
        let rows = stmt
            .query_map([run.0], |r| {
                Ok(LedgerRow {
                    run,
                    kind: r.get(0)?,
                    amount: r.get(1)?,
                    at: r.get(2)?,
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }
}
