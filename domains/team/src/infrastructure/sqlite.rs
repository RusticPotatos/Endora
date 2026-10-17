//! HQ's own SQLite database (spec section 14).

use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};

use crate::application::ports::{
    DeskItem, NewRoutine, NewTask, Post, Run, StoreError, Stored, TeamStore, Timer, Wake,
};
use crate::domain::{
    Brain, CheckBack, CheckBackStatus, DeskId, Event, Goal, GoalId, GoalStatus, Member, MemberId,
    Micros, Millis, Priority, Receipt, Refused, Role, Routine, RoutineId, RunId, RunStatus, Task,
    TaskId, TaskStatus, WakeId, WakeReason,
};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS members (
    id INTEGER PRIMARY KEY, name TEXT NOT NULL, role TEXT NOT NULL,
    share_percent INTEGER NOT NULL, slots INTEGER NOT NULL, paused INTEGER NOT NULL DEFAULT 0,
    look INTEGER NOT NULL DEFAULT 0, brain TEXT NOT NULL DEFAULT 'local'
) STRICT;
CREATE TABLE IF NOT EXISTS goals (
    id INTEGER PRIMARY KEY, title TEXT NOT NULL, reason TEXT NOT NULL, status TEXT NOT NULL,
    parent INTEGER REFERENCES goals(id)
) STRICT;
CREATE TABLE IF NOT EXISTS tasks (
    id INTEGER PRIMARY KEY, goal INTEGER NOT NULL REFERENCES goals(id),
    parent INTEGER REFERENCES tasks(id), title TEXT NOT NULL, description TEXT NOT NULL,
    owner INTEGER REFERENCES members(id), assignee INTEGER REFERENCES members(id),
    status TEXT NOT NULL, priority TEXT NOT NULL, depth INTEGER NOT NULL,
    nudged INTEGER NOT NULL DEFAULT 0, woken_after_waits INTEGER NOT NULL DEFAULT 0,
    gate INTEGER
) STRICT;
CREATE TABLE IF NOT EXISTS task_waits (
    task INTEGER NOT NULL REFERENCES tasks(id), waits_on INTEGER NOT NULL REFERENCES tasks(id),
    PRIMARY KEY (task, waits_on)
) STRICT;
CREATE TABLE IF NOT EXISTS posts (
    id INTEGER PRIMARY KEY, task INTEGER NOT NULL REFERENCES tasks(id), author TEXT NOT NULL,
    text TEXT NOT NULL, at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS wakes (
    id INTEGER PRIMARY KEY, member INTEGER NOT NULL REFERENCES members(id),
    task INTEGER REFERENCES tasks(id), reason TEXT NOT NULL, receipt TEXT NOT NULL,
    refused TEXT, run INTEGER, at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS runs (
    id INTEGER PRIMARY KEY, member INTEGER NOT NULL REFERENCES members(id),
    task INTEGER REFERENCES tasks(id), wake INTEGER NOT NULL REFERENCES wakes(id),
    status TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0, queued_at INTEGER NOT NULL,
    started_at INTEGER, retry_at INTEGER, ended_at INTEGER,
    reserved INTEGER NOT NULL DEFAULT 0, spent INTEGER NOT NULL DEFAULT 0,
    told TEXT NOT NULL DEFAULT '', end_reason TEXT NOT NULL DEFAULT '',
    moved_forward INTEGER NOT NULL DEFAULT 0, input_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_write_tokens INTEGER NOT NULL DEFAULT 0
) STRICT;
-- A turn's token for the team protocol: its worker's only credential, valid while it runs.
CREATE TABLE IF NOT EXISTS run_tokens (
    run INTEGER PRIMARY KEY REFERENCES runs(id), token TEXT NOT NULL UNIQUE
) STRICT;
-- Append-only: a reservation before a turn starts, use while it runs, the charge and the
-- release when it ends. Rows are never changed or removed.
CREATE TABLE IF NOT EXISTS ledger (
    id INTEGER PRIMARY KEY, run INTEGER NOT NULL REFERENCES runs(id),
    member INTEGER NOT NULL REFERENCES members(id), day INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('reserve', 'use', 'charge', 'release')),
    amount INTEGER NOT NULL, at_ms INTEGER NOT NULL
) STRICT;
CREATE TRIGGER IF NOT EXISTS ledger_is_append_only_update
    BEFORE UPDATE ON ledger BEGIN SELECT RAISE(ABORT, 'the ledger is append-only'); END;
CREATE TRIGGER IF NOT EXISTS ledger_is_append_only_delete
    BEFORE DELETE ON ledger BEGIN SELECT RAISE(ABORT, 'the ledger is append-only'); END;
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY, at_ms INTEGER NOT NULL, kind TEXT NOT NULL, data TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS desk (
    id INTEGER PRIMARY KEY, kind TEXT NOT NULL, task INTEGER REFERENCES tasks(id),
    goal INTEGER REFERENCES goals(id), text TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'waiting', at_ms INTEGER NOT NULL,
    member INTEGER REFERENCES members(id)
) STRICT;
CREATE TABLE IF NOT EXISTS timers (
    id INTEGER PRIMARY KEY, member INTEGER NOT NULL REFERENCES members(id),
    reason TEXT NOT NULL, due_at INTEGER NOT NULL, every_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
-- Work that comes round again (ADR 0107): a task handed to its member on a rhythm, or when an
-- alarm says something is wrong.
CREATE TABLE IF NOT EXISTS routines (
    id INTEGER PRIMARY KEY, title TEXT NOT NULL, description TEXT NOT NULL,
    goal INTEGER NOT NULL REFERENCES goals(id), assignee INTEGER NOT NULL REFERENCES members(id),
    every_ms INTEGER NOT NULL, next_due_at INTEGER NOT NULL, enabled INTEGER NOT NULL DEFAULT 1,
    on_lab_trouble INTEGER NOT NULL DEFAULT 0, last_fired_at INTEGER,
    last_task INTEGER REFERENCES tasks(id)
) STRICT;
-- What the owner wants every member working towards a goal to know (ADR 0108).
CREATE TABLE IF NOT EXISTS goal_notes (
    goal INTEGER PRIMARY KEY REFERENCES goals(id), note TEXT NOT NULL
) STRICT;
-- A member's word that it will come back to a task: one wake, later (ADR 0107).
CREATE TABLE IF NOT EXISTS check_backs (
    id INTEGER PRIMARY KEY, member INTEGER NOT NULL REFERENCES members(id),
    task INTEGER NOT NULL REFERENCES tasks(id), why TEXT NOT NULL, due_at INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending', set_at INTEGER NOT NULL, done_at INTEGER
) STRICT;
";

/// HQ's store, over one SQLite connection.
pub struct SqliteTeamStore {
    conn: Mutex<Connection>,
}

fn backend(error: impl core::fmt::Display) -> StoreError {
    StoreError(error.to_string())
}

fn corrupt(what: &str) -> StoreError {
    StoreError(format!("stored data is not what it should be: {what}"))
}

impl SqliteTeamStore {
    /// Opens (and if need be creates) the store at `path`.
    ///
    /// # Errors
    /// [`StoreError`] if the file cannot be opened or the schema applied.
    pub fn open(path: &str) -> Stored<Self> {
        Self::over(Connection::open(path).map_err(backend)?)
    }

    /// A store in memory, for tests and the demo.
    ///
    /// # Errors
    /// [`StoreError`] if the schema cannot be applied.
    pub fn in_memory() -> Stored<Self> {
        Self::over(Connection::open_in_memory().map_err(backend)?)
    }

    fn over(conn: Connection) -> Stored<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(backend)?;
        conn.execute_batch(SCHEMA).map_err(backend)?;
        // A store made before the desk knew who an item is from gains the column, empty.
        let has_member: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('desk') WHERE name = 'member'",
                [],
                |r| r.get(0),
            )
            .map_err(backend)?;
        if !has_member {
            conn.execute_batch(
                "ALTER TABLE desk ADD COLUMN member INTEGER REFERENCES members(id);",
            )
            .map_err(backend)?;
        }
        // A store made before members had a brain gains the column: every one local.
        let has_brain: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('members') WHERE name = 'brain'",
                [],
                |r| r.get(0),
            )
            .map_err(backend)?;
        if !has_brain {
            conn.execute_batch(
                "ALTER TABLE members ADD COLUMN brain TEXT NOT NULL DEFAULT 'local';",
            )
            .map_err(backend)?;
        }
        // A store made before runs counted their input tokens gains the columns, at none.
        for column in ["input_tokens", "cache_read_tokens", "cache_write_tokens"] {
            let has: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('runs') WHERE name = ?1",
                    [column],
                    |r| r.get(0),
                )
                .map_err(backend)?;
            if !has {
                conn.execute_batch(&format!(
                    "ALTER TABLE runs ADD COLUMN {column} INTEGER NOT NULL DEFAULT 0;"
                ))
                .map_err(backend)?;
            }
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> Stored<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| StoreError("the store's lock was poisoned".to_owned()))
    }
}

fn task_from(conn: &Connection, row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let id: i64 = row.get(0)?;
    let mut waits =
        conn.prepare_cached("SELECT waits_on FROM task_waits WHERE task = ?1 ORDER BY waits_on")?;
    let waits_on = waits
        .query_map([id], |r| r.get::<_, i64>(0).map(TaskId))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let status: String = row.get(7)?;
    let priority: String = row.get(8)?;
    Ok(Task {
        id: TaskId(id),
        goal: GoalId(row.get(1)?),
        parent: row.get::<_, Option<i64>>(2)?.map(TaskId),
        title: row.get(3)?,
        description: row.get(4)?,
        owner: row.get::<_, Option<i64>>(5)?.map(MemberId),
        assignee: row.get::<_, Option<i64>>(6)?.map(MemberId),
        status: TaskStatus::from_name(&status).unwrap_or(TaskStatus::Blocked),
        priority: Priority::from_name(&priority).unwrap_or(Priority::Normal),
        depth: u8::try_from(row.get::<_, i64>(9)?).unwrap_or(u8::MAX),
        waits_on,
        nudged: row.get::<_, i64>(10)? != 0,
    })
}

const TASK_COLUMNS: &str =
    "id, goal, parent, title, description, owner, assignee, status, priority, depth, nudged";

fn receipt_from(name: &str, refused: Option<&str>) -> Stored<Receipt> {
    Ok(match name {
        "queued" => Receipt::Queued,
        "folded" => Receipt::Folded,
        "held" => Receipt::Held,
        "started" => Receipt::Started,
        "finished" => Receipt::Finished,
        "refused" => Receipt::Refused(
            refused
                .and_then(Refused::from_name)
                .ok_or_else(|| corrupt("a refused wake without its reason"))?,
        ),
        _ => return Err(corrupt("a wake's receipt")),
    })
}

fn wake_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Wake, String, Option<String>)> {
    let reason: String = row.get(3)?;
    Ok((
        Wake {
            id: WakeId(row.get(0)?),
            member: MemberId(row.get(1)?),
            task: row.get::<_, Option<i64>>(2)?.map(TaskId),
            reason: WakeReason::from_name(&reason).unwrap_or(WakeReason::Recovery),
            receipt: Receipt::Queued,
            run: row.get::<_, Option<i64>>(6)?.map(RunId),
            at: row.get(7)?,
        },
        row.get(4)?,
        row.get(5)?,
    ))
}

fn run_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    let status: String = row.get(4)?;
    Ok(Run {
        id: RunId(row.get(0)?),
        member: MemberId(row.get(1)?),
        task: row.get::<_, Option<i64>>(2)?.map(TaskId),
        wake: WakeId(row.get(3)?),
        status: RunStatus::from_name(&status).unwrap_or(RunStatus::Failed),
        attempts: u8::try_from(row.get::<_, i64>(5)?).unwrap_or(u8::MAX),
        queued_at: row.get(6)?,
        started_at: row.get(7)?,
        retry_at: row.get(8)?,
        ended_at: row.get(9)?,
        reserved: row.get(10)?,
        spent: row.get(11)?,
        told: row.get(12)?,
        end_reason: row.get(13)?,
        moved_forward: row.get::<_, i64>(14)? != 0,
        input_tokens: row.get(15)?,
        cache_read_tokens: row.get(16)?,
        cache_write_tokens: row.get(17)?,
    })
}

const RUN_COLUMNS: &str = "id, member, task, wake, status, attempts, queued_at, started_at, \
    retry_at, ended_at, reserved, spent, told, end_reason, moved_forward, input_tokens, \
    cache_read_tokens, cache_write_tokens";

impl TeamStore for SqliteTeamStore {
    fn add_member(
        &self,
        name: &str,
        role: Role,
        share_percent: u8,
        slots: u8,
        look: u8,
    ) -> Stored<MemberId> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO members (name, role, share_percent, slots, look) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, role.name(), share_percent, slots, look],
        )
        .map_err(backend)?;
        Ok(MemberId(conn.last_insert_rowid()))
    }

    fn members(&self) -> Stored<Vec<Member>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, role, share_percent, slots, paused, look, brain FROM members ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                let role: String = r.get(2)?;
                Ok(Member {
                    id: MemberId(r.get(0)?),
                    name: r.get(1)?,
                    role: Role::from_name(&role).unwrap_or(Role::Developer),
                    share_percent: u8::try_from(r.get::<_, i64>(3)?).unwrap_or(0),
                    slots: u8::try_from(r.get::<_, i64>(4)?).unwrap_or(1),
                    paused: r.get::<_, i64>(5)? != 0,
                    look: u8::try_from(r.get::<_, i64>(6)?).unwrap_or(0),
                    // A brain the store does not know thinks at home: nothing leaves on a
                    // misreading.
                    brain: Brain::from_name(&r.get::<_, String>(7)?).unwrap_or(Brain::Local),
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn set_paused(&self, member: MemberId, paused: bool) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE members SET paused = ?2 WHERE id = ?1",
                params![member.0, paused],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn set_brain(&self, member: MemberId, brain: Brain) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE members SET brain = ?2 WHERE id = ?1",
                params![member.0, brain.name()],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn rename_member(&self, member: MemberId, name: &str) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE members SET name = ?2 WHERE id = ?1",
                params![member.0, name],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn add_goal(
        &self,
        title: &str,
        reason: &str,
        status: GoalStatus,
        parent: Option<GoalId>,
    ) -> Stored<GoalId> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO goals (title, reason, status, parent) VALUES (?1, ?2, ?3, ?4)",
            params![title, reason, status.name(), parent.map(|p| p.0)],
        )
        .map_err(backend)?;
        Ok(GoalId(conn.last_insert_rowid()))
    }

    fn goal(&self, id: GoalId) -> Stored<Option<Goal>> {
        self.conn()?
            .query_row(
                "SELECT id, title, reason, status, parent FROM goals WHERE id = ?1",
                [id.0],
                |r| {
                    let status: String = r.get(3)?;
                    Ok(Goal {
                        id: GoalId(r.get(0)?),
                        title: r.get(1)?,
                        reason: r.get(2)?,
                        status: GoalStatus::from_name(&status).unwrap_or(GoalStatus::Proposed),
                        parent: r.get::<_, Option<i64>>(4)?.map(GoalId),
                    })
                },
            )
            .optional()
            .map_err(backend)
    }

    fn goals(&self) -> Stored<Vec<Goal>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT id, title, reason, status, parent FROM goals ORDER BY id")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                let status: String = r.get(3)?;
                Ok(Goal {
                    id: GoalId(r.get(0)?),
                    title: r.get(1)?,
                    reason: r.get(2)?,
                    status: GoalStatus::from_name(&status).unwrap_or(GoalStatus::Proposed),
                    parent: r.get::<_, Option<i64>>(4)?.map(GoalId),
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn set_goal_status(&self, id: GoalId, status: GoalStatus) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE goals SET status = ?2 WHERE id = ?1",
                params![id.0, status.name()],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn goal_note(&self, id: GoalId) -> Stored<String> {
        Ok(self
            .conn()?
            .query_row("SELECT note FROM goal_notes WHERE goal = ?1", [id.0], |r| {
                r.get(0)
            })
            .optional()
            .map_err(backend)?
            .unwrap_or_default())
    }

    fn set_goal_note(&self, id: GoalId, note: &str) -> Stored<()> {
        let conn = self.conn()?;
        if note.is_empty() {
            conn.execute("DELETE FROM goal_notes WHERE goal = ?1", [id.0])
                .map_err(backend)?;
            return Ok(());
        }
        conn.execute(
            "INSERT INTO goal_notes (goal, note) VALUES (?1, ?2)
             ON CONFLICT (goal) DO UPDATE SET note = excluded.note",
            params![id.0, note],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn add_task(&self, task: &NewTask, depth: u8) -> Stored<TaskId> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(backend)?;
        tx.execute(
            "INSERT INTO tasks (goal, parent, title, description, assignee, status, priority, depth)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                task.goal.0,
                task.parent.map(|p| p.0),
                task.title,
                task.description,
                task.assignee.map(|a| a.0),
                TaskStatus::Todo.name(),
                task.priority.name(),
                depth
            ],
        )
        .map_err(backend)?;
        let id = tx.last_insert_rowid();
        for wait in &task.waits_on {
            tx.execute(
                "INSERT OR IGNORE INTO task_waits (task, waits_on) VALUES (?1, ?2)",
                params![id, wait.0],
            )
            .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(TaskId(id))
    }

    fn task(&self, id: TaskId) -> Stored<Option<Task>> {
        let conn = self.conn()?;
        conn.query_row(
            &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
            [id.0],
            |r| task_from(&conn, r),
        )
        .optional()
        .map_err(backend)
    }

    fn tasks(&self) -> Stored<Vec<Task>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("SELECT {TASK_COLUMNS} FROM tasks ORDER BY id"))
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| task_from(&conn, r))
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn claim(&self, task: TaskId, member: MemberId) -> Stored<Option<MemberId>> {
        let conn = self.conn()?;
        // One statement: the task becomes this member's only if it has no owner, or is
        // already theirs. Whoever's statement runs second finds it taken.
        let changed = conn
            .execute(
                "UPDATE tasks SET owner = ?2 WHERE id = ?1 AND (owner IS NULL OR owner = ?2)",
                params![task.0, member.0],
            )
            .map_err(backend)?;
        if changed == 1 {
            return Ok(None);
        }
        let owner: Option<i64> = conn
            .query_row("SELECT owner FROM tasks WHERE id = ?1", [task.0], |r| {
                r.get(0)
            })
            .optional()
            .map_err(backend)?
            .flatten();
        Ok(owner.map(MemberId))
    }

    fn release(&self, task: TaskId) -> Stored<()> {
        self.conn()?
            .execute("UPDATE tasks SET owner = NULL WHERE id = ?1", [task.0])
            .map_err(backend)?;
        Ok(())
    }

    fn set_task_status(&self, task: TaskId, status: TaskStatus) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE tasks SET status = ?2 WHERE id = ?1",
                params![task.0, status.name()],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn set_nudged(&self, task: TaskId, nudged: bool) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE tasks SET nudged = ?2 WHERE id = ?1",
                params![task.0, nudged],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn set_woken_after_waits(&self, task: TaskId, woken: bool) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE tasks SET woken_after_waits = ?2 WHERE id = ?1",
                params![task.0, woken],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn woken_after_waits(&self, task: TaskId) -> Stored<bool> {
        let woken: Option<i64> = self
            .conn()?
            .query_row(
                "SELECT woken_after_waits FROM tasks WHERE id = ?1",
                [task.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(backend)?;
        Ok(woken.unwrap_or(0) != 0)
    }

    fn set_gate(&self, task: TaskId, passed: bool) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE tasks SET gate = ?2 WHERE id = ?1",
                params![task.0, passed],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn gate(&self, task: TaskId) -> Stored<Option<bool>> {
        let gate: Option<Option<i64>> = self
            .conn()?
            .query_row("SELECT gate FROM tasks WHERE id = ?1", [task.0], |r| {
                r.get(0)
            })
            .optional()
            .map_err(backend)?;
        Ok(gate.flatten().map(|g| g != 0))
    }

    fn post(&self, task: TaskId, author: &str, text: &str, at: Millis) -> Stored<()> {
        self.conn()?
            .execute(
                "INSERT INTO posts (task, author, text, at_ms) VALUES (?1, ?2, ?3, ?4)",
                params![task.0, author, text, at],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn thread(&self, task: TaskId) -> Stored<Vec<Post>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT author, text, at_ms FROM posts WHERE task = ?1 ORDER BY id")
            .map_err(backend)?;
        let rows = stmt
            .query_map([task.0], |r| {
                Ok(Post {
                    author: r.get(0)?,
                    text: r.get(1)?,
                    at: r.get(2)?,
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn add_wake(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        reason: WakeReason,
        receipt: Receipt,
        run: Option<RunId>,
        at: Millis,
    ) -> Stored<WakeId> {
        let refused = match receipt {
            Receipt::Refused(why) => Some(why.name()),
            _ => None,
        };
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO wakes (member, task, reason, receipt, refused, run, at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                member.0,
                task.map(|t| t.0),
                reason.name(),
                receipt.name(),
                refused,
                run.map(|r| r.0),
                at
            ],
        )
        .map_err(backend)?;
        Ok(WakeId(conn.last_insert_rowid()))
    }

    fn set_wake(&self, id: WakeId, receipt: Receipt, run: Option<RunId>) -> Stored<()> {
        let refused = match receipt {
            Receipt::Refused(why) => Some(why.name()),
            _ => None,
        };
        self.conn()?
            .execute(
                "UPDATE wakes SET receipt = ?2, refused = ?3, run = ?4 WHERE id = ?1",
                params![id.0, receipt.name(), refused, run.map(|r| r.0)],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn held_wakes(&self, task: TaskId) -> Stored<Vec<Wake>> {
        Ok(self
            .wakes()?
            .into_iter()
            .filter(|w| w.task == Some(task) && w.receipt == Receipt::Held)
            .collect())
    }

    fn wakes(&self) -> Stored<Vec<Wake>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, member, task, reason, receipt, refused, run, at_ms FROM wakes ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], wake_from)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|(mut wake, receipt, refused)| {
                wake.receipt = receipt_from(&receipt, refused.as_deref())?;
                Ok(wake)
            })
            .collect()
    }

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

    fn add_tokens(&self, run: RunId, fresh: i64, read: i64, written: i64) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE runs SET input_tokens = input_tokens + ?2,
                    cache_read_tokens = cache_read_tokens + ?3,
                    cache_write_tokens = cache_write_tokens + ?4
                 WHERE id = ?1",
                params![run.0, fresh.max(0), read.max(0), written.max(0)],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn save_run(&self, run: &Run) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE runs SET status = ?2, attempts = ?3, started_at = ?4, retry_at = ?5,
                    ended_at = ?6, reserved = ?7, spent = ?8, told = ?9, end_reason = ?10,
                    moved_forward = ?11
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
                    run.moved_forward
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
                "INSERT INTO ledger (run, member, day, kind, amount, at_ms) VALUES (?1, ?2, ?3, 'reserve', ?4, ?5)",
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
                "INSERT INTO ledger (run, member, day, kind, amount, at_ms) VALUES (?1, ?2, ?3, 'use', ?4, ?5)",
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
                    "INSERT INTO ledger (run, member, day, kind, amount, at_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
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

    fn emit(&self, at: Millis, kind: &str, data: &serde_json::Value) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO events (at_ms, kind, data) VALUES (?1, ?2, ?3)",
            params![at, kind, data.to_string()],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn last_event(&self) -> Stored<i64> {
        self.conn()?
            .query_row("SELECT COALESCE(MAX(id), 0) FROM events", [], |r| r.get(0))
            .map_err(backend)
    }

    fn count_events(&self, kind: &str) -> Stored<(i64, Option<Millis>)> {
        self.conn()?
            .query_row(
                "SELECT COUNT(*), MAX(at_ms) FROM events WHERE kind = ?1",
                [kind],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(backend)
    }

    fn prune_events(&self, keep: i64) -> Stored<usize> {
        self.conn()?
            .execute(
                "DELETE FROM events WHERE id <= (SELECT COALESCE(MAX(id), 0) FROM events) - ?1",
                [keep],
            )
            .map_err(backend)
    }

    fn events_after(&self, id: i64) -> Stored<Vec<Event>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT id, at_ms, kind, data FROM events WHERE id > ?1 ORDER BY id")
            .map_err(backend)?;
        let rows = stmt
            .query_map([id], |r| {
                let data: String = r.get(3)?;
                Ok(Event {
                    id: r.get(0)?,
                    at: r.get(1)?,
                    kind: r.get(2)?,
                    data: serde_json::from_str(&data).unwrap_or(serde_json::Value::Null),
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn add_desk(
        &self,
        kind: &str,
        task: Option<TaskId>,
        goal: Option<GoalId>,
        from: Option<MemberId>,
        text: &str,
        at: Millis,
    ) -> Stored<DeskId> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO desk (kind, task, goal, member, text, at_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![kind, task.map(|t| t.0), goal.map(|g| g.0), from.map(|m| m.0), text, at],
        )
        .map_err(backend)?;
        Ok(DeskId(conn.last_insert_rowid()))
    }

    fn set_desk_status(&self, id: DeskId, status: &str) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE desk SET status = ?2 WHERE id = ?1",
                params![id.0, status],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn desk(&self) -> Stored<Vec<DeskItem>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, task, goal, text, status, at_ms, member FROM desk ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(DeskItem {
                    id: DeskId(r.get(0)?),
                    kind: r.get(1)?,
                    task: r.get::<_, Option<i64>>(2)?.map(TaskId),
                    goal: r.get::<_, Option<i64>>(3)?.map(GoalId),
                    text: r.get(4)?,
                    status: r.get(5)?,
                    at: r.get(6)?,
                    member: r.get::<_, Option<i64>>(7)?.map(MemberId),
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn stopped(&self) -> Stored<bool> {
        let value: Option<String> = self
            .conn()?
            .query_row(
                "SELECT value FROM settings WHERE key = 'stopped'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(backend)?;
        Ok(value.as_deref() == Some("1"))
    }

    fn set_stopped(&self, stopped: bool) -> Stored<()> {
        self.set_setting("stopped", if stopped { "1" } else { "0" })
    }

    fn setting(&self, key: &str) -> Stored<Option<String>> {
        self.conn()?
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(backend)
    }

    fn set_setting(&self, key: &str, value: &str) -> Stored<()> {
        self.conn()?
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, value],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn add_timer(
        &self,
        member: MemberId,
        reason: WakeReason,
        due_at: Millis,
        every_ms: Millis,
    ) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO timers (member, reason, due_at, every_ms) VALUES (?1, ?2, ?3, ?4)",
            params![member.0, reason.name(), due_at, every_ms],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn timers(&self) -> Stored<Vec<Timer>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT id, member, reason, due_at, every_ms FROM timers ORDER BY id")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                let reason: String = r.get(2)?;
                Ok(Timer {
                    id: r.get(0)?,
                    member: MemberId(r.get(1)?),
                    reason: WakeReason::from_name(&reason).unwrap_or(WakeReason::LeadReview),
                    due_at: r.get(3)?,
                    every_ms: r.get(4)?,
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn set_timer_due(&self, id: i64, due_at: Millis) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE timers SET due_at = ?2 WHERE id = ?1",
                params![id, due_at],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn add_routine(&self, routine: &NewRoutine, next_due_at: Millis) -> Stored<RoutineId> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO routines (title, description, goal, assignee, every_ms, next_due_at,
                on_lab_trouble) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                routine.title,
                routine.description,
                routine.goal.0,
                routine.assignee.0,
                routine.every_ms,
                next_due_at,
                routine.on_lab_trouble
            ],
        )
        .map_err(backend)?;
        Ok(RoutineId(conn.last_insert_rowid()))
    }

    fn routines(&self) -> Stored<Vec<Routine>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, title, description, goal, assignee, every_ms, next_due_at, enabled,
                    on_lab_trouble, last_fired_at, last_task FROM routines ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Routine {
                    id: RoutineId(r.get(0)?),
                    title: r.get(1)?,
                    description: r.get(2)?,
                    goal: GoalId(r.get(3)?),
                    assignee: MemberId(r.get(4)?),
                    every_ms: r.get(5)?,
                    next_due_at: r.get(6)?,
                    enabled: r.get::<_, i64>(7)? != 0,
                    on_lab_trouble: r.get::<_, i64>(8)? != 0,
                    last_fired_at: r.get(9)?,
                    last_task: r.get::<_, Option<i64>>(10)?.map(TaskId),
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn save_routine(&self, routine: &Routine) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE routines SET enabled = ?2, next_due_at = ?3, last_fired_at = ?4,
                    last_task = ?5 WHERE id = ?1",
                params![
                    routine.id.0,
                    routine.enabled,
                    routine.next_due_at,
                    routine.last_fired_at,
                    routine.last_task.map(|t| t.0)
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn add_check_back(
        &self,
        member: MemberId,
        task: TaskId,
        why: &str,
        due_at: Millis,
        at: Millis,
    ) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO check_backs (member, task, why, due_at, set_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![member.0, task.0, why, due_at, at],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn check_backs(&self) -> Stored<Vec<CheckBack>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, member, task, why, due_at, status, set_at, done_at FROM check_backs
                 ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                let status: String = r.get(5)?;
                Ok(CheckBack {
                    id: r.get(0)?,
                    member: MemberId(r.get(1)?),
                    task: TaskId(r.get(2)?),
                    why: r.get(3)?,
                    due_at: r.get(4)?,
                    status: CheckBackStatus::from_name(&status).unwrap_or(CheckBackStatus::Dropped),
                    set_at: r.get(6)?,
                    done_at: r.get(7)?,
                })
            })
            .map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn set_check_back(&self, id: i64, status: CheckBackStatus, at: Millis) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE check_backs SET status = ?2, done_at = ?3 WHERE id = ?1",
                params![id, status.name(), at],
            )
            .map_err(backend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_is_claimed_by_exactly_one_member() {
        let store = SqliteTeamStore::in_memory().unwrap();
        let a = store.add_member("Ada", Role::Developer, 40, 2, 0).unwrap();
        let b = store
            .add_member("Linus", Role::Developer, 40, 2, 0)
            .unwrap();
        let goal = store
            .add_goal("Ship it", "", GoalStatus::Active, None)
            .unwrap();
        let task = store
            .add_task(
                &NewTask {
                    goal,
                    parent: None,
                    title: "Write it".to_owned(),
                    description: String::new(),
                    assignee: None,
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
                0,
            )
            .unwrap();
        assert_eq!(store.claim(task, a).unwrap(), None);
        assert_eq!(store.claim(task, b).unwrap(), Some(a), "taken");
        assert_eq!(
            store.claim(task, a).unwrap(),
            None,
            "claiming your own again is fine"
        );
    }

    #[test]
    fn a_desk_made_before_it_knew_who_asked_gains_the_column_and_keeps_its_items() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE desk (
                id INTEGER PRIMARY KEY, kind TEXT NOT NULL, task INTEGER, goal INTEGER,
                text TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'waiting', at_ms INTEGER NOT NULL
            ) STRICT;
            INSERT INTO desk (kind, text, at_ms) VALUES ('question', 'Linus asks: why?', 5);",
        )
        .unwrap();
        let store = SqliteTeamStore::over(conn).unwrap();
        let a = store.add_member("Ada", Role::Lead, 25, 2, 0).unwrap();
        store
            .add_desk("question", None, None, Some(a), "Ada asks: when?", 6)
            .unwrap();
        let desk = store.desk().unwrap();
        assert_eq!(
            desk.iter().map(|d| d.member).collect::<Vec<_>>(),
            [None, Some(a)]
        );
        assert_eq!(desk[0].text, "Linus asks: why?");
    }

    #[test]
    fn the_ledger_is_append_only() {
        let store = SqliteTeamStore::in_memory().unwrap();
        let a = store.add_member("Ada", Role::Developer, 40, 2, 0).unwrap();
        let wake = store
            .add_wake(a, None, WakeReason::LeadReview, Receipt::Queued, None, 0)
            .unwrap();
        let run = store.add_run(a, None, wake, 0).unwrap();
        store.reserve(run, a, 1, 400_000, 0).unwrap();
        let conn = store.conn().unwrap();
        assert!(conn.execute("UPDATE ledger SET amount = 0", []).is_err());
        assert!(conn.execute("DELETE FROM ledger", []).is_err());
    }

    #[test]
    fn settling_charges_use_and_frees_the_reservation_on_the_day_it_was_made() {
        let store = SqliteTeamStore::in_memory().unwrap();
        let a = store.add_member("Ada", Role::Developer, 40, 2, 0).unwrap();
        let wake = store
            .add_wake(a, None, WakeReason::LeadReview, Receipt::Queued, None, 0)
            .unwrap();
        let run = store.add_run(a, None, wake, 0).unwrap();
        store.reserve(run, a, 10, 400_000, 0).unwrap();
        assert_eq!(store.committed(Some(a), 10).unwrap(), 400_000);
        store.record_use(run, a, 10, 120_000, 1).unwrap();
        assert_eq!(
            store.committed(Some(a), 10).unwrap(),
            400_000,
            "use is inside the hold"
        );
        store.settle(run, a, 11, 2).unwrap();
        assert_eq!(store.committed(Some(a), 10).unwrap(), 120_000);
        assert_eq!(
            store.committed(Some(a), 11).unwrap(),
            0,
            "not counted on the next day"
        );
        assert_eq!(store.committed(None, 10).unwrap(), 120_000, "the pool");
        // Settling twice changes nothing.
        store.settle(run, a, 11, 3).unwrap();
        assert_eq!(store.committed(Some(a), 10).unwrap(), 120_000);
    }
}
