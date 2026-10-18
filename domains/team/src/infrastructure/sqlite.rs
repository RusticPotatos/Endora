//! HQ's own SQLite database (spec section 14).

use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};

use crate::application::ports::{
    DeskItem, NewProject, NewRoutine, NewTask, Post, Run, StoreError, Stored, TeamStore, Timer,
    Wake,
};
use crate::domain::{
    Brain, CheckBack, CheckBackStatus, DeskId, Event, Evidence, Goal, GoalId, GoalStatus, Member,
    MemberId, Micros, Millis, Pause, Priority, Project, ProjectId, ProjectStatus, Receipt, Refused,
    Role, Routine, RoutineId, RunId, RunStatus, Task, TaskId, TaskStatus, WakeId, WakeReason,
};
use crate::domain::{Branch, Check, Document};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS members (
    id INTEGER PRIMARY KEY, name TEXT NOT NULL, role TEXT NOT NULL,
    share_percent INTEGER NOT NULL, slots INTEGER NOT NULL, paused INTEGER NOT NULL DEFAULT 0,
    look INTEGER NOT NULL DEFAULT 0, brain TEXT NOT NULL DEFAULT 'local',
    reports_to INTEGER REFERENCES members(id)
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
    gate INTEGER, project INTEGER REFERENCES projects(id), criteria TEXT NOT NULL DEFAULT ''
) STRICT;
CREATE TABLE IF NOT EXISTS task_waits (
    task INTEGER NOT NULL REFERENCES tasks(id), waits_on INTEGER NOT NULL REFERENCES tasks(id),
    PRIMARY KEY (task, waits_on)
) STRICT;
CREATE TABLE IF NOT EXISTS posts (
    id INTEGER PRIMARY KEY, task INTEGER NOT NULL REFERENCES tasks(id), author TEXT NOT NULL,
    text TEXT NOT NULL, at_ms INTEGER NOT NULL, evidence TEXT NOT NULL DEFAULT '',
    unsupported INTEGER NOT NULL DEFAULT 0
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
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    posted INTEGER NOT NULL DEFAULT 0, blocked INTEGER NOT NULL DEFAULT 0, label TEXT
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
    amount INTEGER NOT NULL, at_ms INTEGER NOT NULL, project INTEGER REFERENCES projects(id)
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
    member INTEGER REFERENCES members(id), project INTEGER REFERENCES projects(id)
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
    last_task INTEGER REFERENCES tasks(id), project INTEGER REFERENCES projects(id)
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
-- A piece of one goal, with its own lead and, if the owner gives it one, a monthly cap
-- (ADR 0113).
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY, goal INTEGER NOT NULL REFERENCES goals(id), title TEXT NOT NULL,
    reason TEXT NOT NULL, lead INTEGER NOT NULL REFERENCES members(id), status TEXT NOT NULL,
    pause TEXT, cap INTEGER, asked_cap INTEGER, created_by TEXT NOT NULL,
    proposer INTEGER REFERENCES members(id), proposed_in INTEGER REFERENCES tasks(id),
    evidence TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
) STRICT;
-- What a turn called of Endora's tools, as the node recorded it (ADR 0114): written by the
-- node with the turn's token, never by the model. Never changed.
CREATE TABLE IF NOT EXISTS call_receipts (
    id INTEGER PRIMARY KEY, run INTEGER NOT NULL REFERENCES runs(id),
    member INTEGER NOT NULL REFERENCES members(id), task INTEGER REFERENCES tasks(id),
    tool TEXT NOT NULL, args TEXT NOT NULL, excerpt TEXT NOT NULL, ok INTEGER NOT NULL,
    act INTEGER NOT NULL, confidential INTEGER NOT NULL, at_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS call_receipts_by_run ON call_receipts(run);
CREATE TRIGGER IF NOT EXISTS call_receipts_are_kept_update
    BEFORE UPDATE ON call_receipts BEGIN SELECT RAISE(ABORT, 'a receipt is never changed'); END;
-- Evidence cited for a task's criteria, at each stage of its checking (ADR 0114).
CREATE TABLE IF NOT EXISTS cited (
    id INTEGER PRIMARY KEY, task INTEGER NOT NULL REFERENCES tasks(id),
    run INTEGER NOT NULL REFERENCES runs(id), member INTEGER NOT NULL REFERENCES members(id),
    stage TEXT NOT NULL, criterion INTEGER NOT NULL, refs TEXT NOT NULL, at_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS cited_by_task ON cited(task);
-- The engineering wing (ADR 0115): a task's documents (a branch's diff, a check's report), the
-- branches it submitted, and the checks of them.
CREATE TABLE IF NOT EXISTS documents (
    id INTEGER PRIMARY KEY, task INTEGER NOT NULL REFERENCES tasks(id),
    run INTEGER REFERENCES runs(id), title TEXT NOT NULL, text TEXT NOT NULL,
    at_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS documents_by_task ON documents(task);
CREATE TABLE IF NOT EXISTS branches (
    id INTEGER PRIMARY KEY, task INTEGER NOT NULL REFERENCES tasks(id),
    run INTEGER NOT NULL REFERENCES runs(id), member INTEGER NOT NULL REFERENCES members(id),
    name TEXT NOT NULL, head TEXT NOT NULL, base TEXT NOT NULL, files INTEGER NOT NULL,
    plus INTEGER NOT NULL, minus INTEGER NOT NULL, can_push INTEGER NOT NULL,
    status TEXT NOT NULL, at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS checks (
    id INTEGER PRIMARY KEY, task INTEGER NOT NULL REFERENCES tasks(id),
    branch INTEGER NOT NULL REFERENCES branches(id), run INTEGER NOT NULL REFERENCES runs(id),
    token TEXT NOT NULL UNIQUE, status TEXT NOT NULL, ok INTEGER, summary TEXT NOT NULL DEFAULT '',
    queued_at INTEGER NOT NULL, started_at INTEGER, done_at INTEGER
) STRICT;
";

/// The columns a store made before evidence (ADR 0114) gains: no criteria, no post cited
/// anything or was unsupported, and no turn posted, was blocked or has a label. The new tables
/// come with the schema, which makes what it lacks.
const EVIDENCE_COLUMNS: [(&str, &str); 6] = [
    ("tasks", "criteria TEXT NOT NULL DEFAULT ''"),
    ("posts", "evidence TEXT NOT NULL DEFAULT ''"),
    ("posts", "unsupported INTEGER NOT NULL DEFAULT 0"),
    ("runs", "posted INTEGER NOT NULL DEFAULT 0"),
    ("runs", "blocked INTEGER NOT NULL DEFAULT 0"),
    ("runs", "label TEXT"),
];

/// The columns a store made before projects (ADR 0113) gains, each empty: no member reports to
/// anyone yet, no task or routine is in a project, no desk item is about one, and no ledger row
/// is a project's. Adding a column changes no row, so the ledger's append-only triggers stay.
const PROJECT_COLUMNS: [(&str, &str); 5] = [
    ("members", "reports_to INTEGER REFERENCES members(id)"),
    ("tasks", "project INTEGER REFERENCES projects(id)"),
    ("routines", "project INTEGER REFERENCES projects(id)"),
    ("desk", "project INTEGER REFERENCES projects(id)"),
    ("ledger", "project INTEGER REFERENCES projects(id)"),
];

/// Adds `column` (its name, then its type) to `table`, unless the table has it already.
fn add_column(conn: &Connection, table: &str, column: &str) -> Stored<()> {
    let name = column.split_whitespace().next().unwrap_or(column);
    let has: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info(?1) WHERE name = ?2",
            [table, name],
            |r| r.get(0),
        )
        .map_err(backend)?;
    if has {
        return Ok(());
    }
    conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column};"))
        .map_err(backend)
}

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
        // A store made before runs counted their tokens gains the columns, at none.
        for column in [
            "input_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
            "output_tokens",
        ] {
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
        for (table, column) in PROJECT_COLUMNS.into_iter().chain(EVIDENCE_COLUMNS) {
            add_column(&conn, table, column)?;
        }
        // A project's month is read on every wake and turn about its work, each row with its
        // turn's reservation: by run and by project, not by reading the whole ledger each time.
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS ledger_by_run ON ledger(run);
             CREATE INDEX IF NOT EXISTS ledger_by_project ON ledger(project);",
        )
        .map_err(backend)?;
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
        project: row.get::<_, Option<i64>>(11)?.map(ProjectId),
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
        criteria: crate::domain::Criterion::read(&row.get::<_, String>(12)?),
    })
}

const TASK_COLUMNS: &str = "id, goal, parent, title, description, owner, assignee, status, \
    priority, depth, nudged, project, criteria";

const PROJECT_SELECT: &str = "SELECT id, goal, title, reason, lead, status, pause, cap, \
    asked_cap, created_by, proposer, proposed_in, evidence, created_at, updated_at FROM projects";

fn project_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let status: String = r.get(5)?;
    let pause: Option<String> = r.get(6)?;
    Ok(Project {
        id: ProjectId(r.get(0)?),
        goal: GoalId(r.get(1)?),
        title: r.get(2)?,
        reason: r.get(3)?,
        lead: MemberId(r.get(4)?),
        // A status the store does not know holds the project rather than spending on it.
        status: ProjectStatus::from_name(&status).unwrap_or(ProjectStatus::Paused),
        pause: pause.as_deref().and_then(Pause::from_name),
        cap: r.get(7)?,
        asked_cap: r.get(8)?,
        created_by: r.get(9)?,
        proposer: r.get::<_, Option<i64>>(10)?.map(MemberId),
        proposed_in: r.get::<_, Option<i64>>(11)?.map(TaskId),
        evidence: Evidence::read(&r.get::<_, String>(12)?),
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
    })
}

/// The project of a run's turn, as its ledger rows carry it: the one its task was in when it
/// reserved, for the rows after the reservation.
const RUNS_PROJECT: &str =
    "(SELECT project FROM ledger WHERE run = ?1 AND kind = 'reserve' ORDER BY id DESC LIMIT 1)";

/// A project's rows of the ledger, booked in the month its turn reserved, from ?2 until ?3.
const PROJECTS_ROWS: &str = "FROM ledger l WHERE l.project = ?1 AND COALESCE((SELECT MAX(r.at_ms) \
    FROM ledger r WHERE r.run = l.run AND r.kind = 'reserve'), l.at_ms) >= ?2 AND COALESCE((SELECT \
    MAX(r.at_ms) FROM ledger r WHERE r.run = l.run AND r.kind = 'reserve'), l.at_ms) < ?3";

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
        output_tokens: row.get(18)?,
        posted: row.get::<_, i64>(19)? != 0,
        blocked: row.get::<_, i64>(20)? != 0,
        label: row
            .get::<_, Option<String>>(21)?
            .as_deref()
            .and_then(crate::domain::Label::from_name),
    })
}

const RUN_COLUMNS: &str = "id, member, task, wake, status, attempts, queued_at, started_at, \
    retry_at, ended_at, reserved, spent, told, end_reason, moved_forward, input_tokens, \
    cache_read_tokens, cache_write_tokens, output_tokens, posted, blocked, label";

const RECEIPT_SELECT: &str = "SELECT id, run, member, task, tool, args, excerpt, ok, act, \
    confidential, at_ms FROM call_receipts";

fn call_receipt_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<crate::domain::CallReceipt> {
    Ok(crate::domain::CallReceipt {
        id: r.get(0)?,
        run: RunId(r.get(1)?),
        member: MemberId(r.get(2)?),
        task: r.get::<_, Option<i64>>(3)?.map(TaskId),
        tool: r.get(4)?,
        args: r.get(5)?,
        excerpt: r.get(6)?,
        ok: r.get::<_, i64>(7)? != 0,
        act: r.get::<_, i64>(8)? != 0,
        confidential: r.get::<_, i64>(9)? != 0,
        at: r.get(10)?,
    })
}

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
                "SELECT id, name, role, share_percent, slots, paused, look, brain, reports_to FROM members ORDER BY id",
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
                    reports_to: r.get::<_, Option<i64>>(8)?.map(MemberId),
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

    fn set_reports_to(&self, member: MemberId, to: Option<MemberId>) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE members SET reports_to = ?2 WHERE id = ?1",
                params![member.0, to.map(|t| t.0)],
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
            "INSERT INTO tasks (goal, parent, title, description, assignee, status, priority, depth,
                project) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                task.goal.0,
                task.parent.map(|p| p.0),
                task.title,
                task.description,
                task.assignee.map(|a| a.0),
                TaskStatus::Todo.name(),
                task.priority.name(),
                depth,
                task.project.map(|p| p.0)
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

    fn set_task_project(&self, task: TaskId, project: Option<ProjectId>) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE tasks SET project = ?2 WHERE id = ?1",
                params![task.0, project.map(|p| p.0)],
            )
            .map_err(backend)?;
        Ok(())
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

    fn post_marked(
        &self,
        task: TaskId,
        author: &str,
        text: &str,
        at: Millis,
        evidence: &[crate::domain::Ref],
        unsupported: bool,
    ) -> Stored<()> {
        self.conn()?
            .execute(
                "INSERT INTO posts (task, author, text, at_ms, evidence, unsupported)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    task.0,
                    author,
                    text,
                    at,
                    crate::domain::Ref::kept(evidence),
                    unsupported
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn set_criteria(&self, task: TaskId, criteria: &[crate::domain::Criterion]) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE tasks SET criteria = ?2 WHERE id = ?1",
                params![task.0, crate::domain::Criterion::kept(criteria)],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn set_assignee(&self, task: TaskId, member: Option<MemberId>) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE tasks SET assignee = ?2 WHERE id = ?1",
                params![task.0, member.map(|m| m.0)],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn add_call_receipt(&self, r: &crate::domain::CallReceipt) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO call_receipts (run, member, task, tool, args, excerpt, ok, act,
                confidential, at_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                r.at
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

    fn thread(&self, task: TaskId) -> Stored<Vec<Post>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT author, text, at_ms, unsupported FROM posts WHERE task = ?1 ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([task.0], |r| {
                Ok(Post {
                    author: r.get(0)?,
                    text: r.get(1)?,
                    at: r.get(2)?,
                    unsupported: r.get::<_, i64>(3)? != 0,
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
                    moved_forward = ?11, posted = ?12, blocked = ?13, label = ?14
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
                    run.label.map(crate::domain::Label::name)
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
                "INSERT INTO ledger (run, member, day, kind, amount, at_ms, project) VALUES (?1, ?2, ?3, 'reserve', ?4, ?5,
                    (SELECT t.project FROM runs r JOIN tasks t ON t.id = r.task WHERE r.id = ?1))",
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
                    "INSERT INTO ledger (run, member, day, kind, amount, at_ms, project)
                     VALUES (?1, ?2, ?3, 'use', ?4, ?5, {RUNS_PROJECT})"
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
                        "INSERT INTO ledger (run, member, day, kind, amount, at_ms, project)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, {RUNS_PROJECT})"
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

    fn add_project(&self, project: &NewProject, at: Millis) -> Stored<ProjectId> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO projects (goal, title, reason, lead, status, cap, asked_cap, created_by,
                proposer, proposed_in, evidence, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            params![
                project.goal.0,
                project.title,
                project.reason,
                project.lead.0,
                project.status.name(),
                project.cap,
                project.asked_cap,
                project.created_by,
                project.proposer.map(|m| m.0),
                project.proposed_in.map(|t| t.0),
                Evidence::kept(&project.evidence),
                at
            ],
        )
        .map_err(backend)?;
        Ok(ProjectId(conn.last_insert_rowid()))
    }

    fn projects(&self) -> Stored<Vec<Project>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{PROJECT_SELECT} ORDER BY id"))
            .map_err(backend)?;
        let rows = stmt.query_map([], project_from).map_err(backend)?;
        rows.collect::<rusqlite::Result<_>>().map_err(backend)
    }

    fn save_project(&self, project: &Project) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE projects SET lead = ?2, status = ?3, pause = ?4, cap = ?5, asked_cap = ?6,
                    updated_at = ?7 WHERE id = ?1",
                params![
                    project.id.0,
                    project.lead.0,
                    project.status.name(),
                    project.pause.map(Pause::name),
                    project.cap,
                    project.asked_cap,
                    project.updated_at
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn set_desk_project(&self, id: DeskId, project: ProjectId) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE desk SET project = ?2 WHERE id = ?1",
                params![id.0, project.0],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn has_event(&self, id: i64) -> Stored<bool> {
        self.conn()?
            .query_row("SELECT COUNT(*) > 0 FROM events WHERE id = ?1", [id], |r| {
                r.get(0)
            })
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

    fn event(&self, id: i64) -> Stored<Option<Event>> {
        self.conn()?
            .query_row(
                "SELECT id, at_ms, kind, data FROM events WHERE id = ?1",
                [id],
                |r| {
                    let data: String = r.get(3)?;
                    Ok(Event {
                        id: r.get(0)?,
                        at: r.get(1)?,
                        kind: r.get(2)?,
                        data: serde_json::from_str(&data).unwrap_or(serde_json::Value::Null),
                    })
                },
            )
            .optional()
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
                "SELECT id, kind, task, goal, text, status, at_ms, member, project FROM desk ORDER BY id",
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
                    project: r.get::<_, Option<i64>>(8)?.map(ProjectId),
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
                on_lab_trouble, project) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                routine.title,
                routine.description,
                routine.goal.0,
                routine.assignee.0,
                routine.every_ms,
                next_due_at,
                routine.on_lab_trouble,
                routine.project.map(|p| p.0)
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
                    on_lab_trouble, last_fired_at, last_task, project FROM routines ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Routine {
                    id: RoutineId(r.get(0)?),
                    title: r.get(1)?,
                    description: r.get(2)?,
                    goal: GoalId(r.get(3)?),
                    project: r.get::<_, Option<i64>>(11)?.map(ProjectId),
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
                    last_task = ?5, project = ?6 WHERE id = ?1",
                params![
                    routine.id.0,
                    routine.enabled,
                    routine.next_due_at,
                    routine.last_fired_at,
                    routine.last_task.map(|t| t.0),
                    routine.project.map(|p| p.0)
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

const DOCUMENT_SELECT: &str = "SELECT id, task, run, title, text, at_ms FROM documents";

fn document_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: r.get(0)?,
        task: TaskId(r.get(1)?),
        run: r.get::<_, Option<i64>>(2)?.map(RunId),
        title: r.get(3)?,
        text: r.get(4)?,
        at: r.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The schema as it was before projects (ADR 0113): the live store was made with it.
    const SCHEMA_BEFORE_PROJECTS: &str = "
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
                    project: None,
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
    fn a_store_made_before_runs_counted_output_tokens_gains_the_column_at_none() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(SCHEMA_BEFORE_PROJECTS).unwrap();
        conn.execute_batch(
            "INSERT INTO members (id, name, role, share_percent, slots) VALUES
                (1, 'Ada', 'developer', 40, 2);
             INSERT INTO goals (id, title, reason, status) VALUES (1, 'Ship it', '', 'active');
             INSERT INTO tasks (id, goal, title, description, status, priority, depth) VALUES
                (1, 1, 'Write it', '', 'doing', 'normal', 0);
             INSERT INTO wakes (id, member, task, reason, receipt, at_ms) VALUES
                (1, 1, 1, 'assigned', 'started', 0);
             INSERT INTO runs (id, member, task, wake, status, queued_at, input_tokens,
                cache_read_tokens, cache_write_tokens) VALUES
                (1, 1, 1, 1, 'running', 0, 100, 9000, 800);",
        )
        .unwrap();
        let store = SqliteTeamStore::over(conn).unwrap();
        let run = store.run(RunId(1)).unwrap().unwrap();
        assert_eq!(
            (
                run.input_tokens,
                run.cache_read_tokens,
                run.cache_write_tokens,
                run.output_tokens
            ),
            (100, 9_000, 800, 0),
            "what it counted stays; what it did not is none"
        );
        store.add_tokens(RunId(1), 1, 2, 3, 40).unwrap();
        let run = store.run(RunId(1)).unwrap().unwrap();
        assert_eq!(
            (
                run.input_tokens,
                run.cache_read_tokens,
                run.cache_write_tokens,
                run.output_tokens
            ),
            (101, 9_002, 803, 40)
        );
        // Opened again, the column is already there: nothing is added twice.
        assert!(SqliteTeamStore::in_memory().is_ok());
    }

    /// 17 October 2026, at noon UTC.
    const NOON: Millis = 1_792_238_400_000;

    struct Noon;
    impl crate::application::ports::Clock for Noon {
        fn now(&self) -> Millis {
            NOON
        }
    }

    #[test]
    fn a_store_made_before_projects_gains_their_columns_and_its_home_lab_work_one_project() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(SCHEMA_BEFORE_PROJECTS).unwrap();
        conn.execute_batch(
            "INSERT INTO members (id, name, role, share_percent, slots, brain) VALUES
                (1, 'Mara', 'ceo', 35, 1, 'claude-opus-5-5'), (2, 'Ada', 'lead', 25, 2, 'local'),
                (3, 'Linus', 'developer', 40, 2, 'local'), (4, 'Grace', 'lead', 25, 2, 'local');
             INSERT INTO goals (id, title, reason, status) VALUES
                (1, 'Keep the home lab healthy', '', 'active'), (2, 'Ship the tracker', '', 'active');
             INSERT INTO tasks (id, goal, title, description, status, priority, depth) VALUES
                (1, 1, 'Check the home lab', '', 'doing', 'high', 0),
                (2, 1, 'Fix sonarr', '', 'done', 'normal', 1),
                (3, 2, 'Write it', '', 'todo', 'normal', 0);
             INSERT INTO routines (id, title, description, goal, assignee, every_ms, next_due_at,
                on_lab_trouble, last_task) VALUES
                (1, 'Check the home lab', '', 1, 2, 21600000, 0, 1, 1),
                (2, 'Daily review', '', 1, 1, 86400000, 0, 0, NULL);
             INSERT INTO wakes (id, member, task, reason, receipt, at_ms) VALUES
                (1, 2, 1, 'assigned', 'started', 0), (2, 2, 1, 'alarm', 'started', 0);
             INSERT INTO runs (id, member, task, wake, status, queued_at) VALUES
                (1, 2, 1, 1, 'succeeded', 0), (2, 2, 1, 2, 'running', 0);
             INSERT INTO ledger (run, member, day, kind, amount, at_ms) VALUES
                (1, 2, 1, 'use', 1000, 0);",
        )
        .unwrap();
        let store = SqliteTeamStore::over(conn).unwrap();
        let hq = crate::application::Engine::new(
            store,
            crate::infrastructure::SimulatedRunner::new(1),
            Noon,
            crate::application::Settings::default(),
        );
        // Whom each reports to, filled in as each would have been hired.
        assert_eq!(hq.backfill_reports_to(), Ok(3));
        let reports: Vec<Option<i64>> = hq
            .store()
            .members()
            .unwrap()
            .iter()
            .map(|m| m.reports_to.map(|r| r.0))
            .collect();
        assert_eq!(reports, [None, Some(1), Some(2), Some(1)]);
        let made = hq.migrate_projects().unwrap().expect("made");
        assert_eq!(hq.migrate_projects(), Ok(None), "once");
        let projects = hq.store().projects().unwrap();
        assert_eq!(projects.len(), 1);
        let upkeep = &projects[0];
        assert_eq!(
            (
                upkeep.id,
                upkeep.title.as_str(),
                upkeep.goal,
                upkeep.lead,
                upkeep.status,
                upkeep.cap
            ),
            (
                made,
                crate::application::engine::LAB_UPKEEP,
                GoalId(1),
                MemberId(2),
                ProjectStatus::Active,
                None
            )
        );
        let routines: Vec<Option<ProjectId>> = hq
            .store()
            .routines()
            .unwrap()
            .iter()
            .map(|r| r.project)
            .collect();
        assert_eq!(routines, [Some(made), None], "the CEO's review stays out");
        let tasks: Vec<Option<ProjectId>> = hq
            .store()
            .tasks()
            .unwrap()
            .iter()
            .map(|t| t.project)
            .collect();
        assert_eq!(
            tasks,
            [Some(made), None, None],
            "open work of its goal only"
        );
        // The old ledger row is no project's; a new turn's rows are its task's project's; and
        // nothing in the ledger may be changed.
        let (from, until) = crate::domain::projects::month_around(NOON);
        hq.store()
            .reserve(RunId(2), MemberId(2), 1, 2_000, NOON)
            .unwrap();
        hq.store()
            .record_use(RunId(2), MemberId(2), 1, 1_500, NOON)
            .unwrap();
        assert_eq!(hq.store().project_spent(made, from, until), Ok(1_500));
        assert_eq!(hq.store().project_committed(made, from, until), Ok(2_000));
        hq.store().settle(RunId(2), MemberId(2), 1, NOON).unwrap();
        assert_eq!(hq.store().project_committed(made, from, until), Ok(1_500));
        let conn = hq.store().conn().unwrap();
        assert!(conn.execute("UPDATE ledger SET amount = 0", []).is_err());
        assert!(conn.execute("DELETE FROM ledger", []).is_err());
        let indexed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_index_list('ledger')
                 WHERE name IN ('ledger_by_run', 'ledger_by_project')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 2, "a project's month reads the ledger by index");
    }

    /// The schema as it was before evidence (ADR 0114): the live store was made with it.
    const SCHEMA_BEFORE_EVIDENCE: &str = "
CREATE TABLE IF NOT EXISTS members (
    id INTEGER PRIMARY KEY, name TEXT NOT NULL, role TEXT NOT NULL,
    share_percent INTEGER NOT NULL, slots INTEGER NOT NULL, paused INTEGER NOT NULL DEFAULT 0,
    look INTEGER NOT NULL DEFAULT 0, brain TEXT NOT NULL DEFAULT 'local',
    reports_to INTEGER REFERENCES members(id)
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
    gate INTEGER, project INTEGER REFERENCES projects(id)
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
    amount INTEGER NOT NULL, at_ms INTEGER NOT NULL, project INTEGER REFERENCES projects(id)
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
    member INTEGER REFERENCES members(id), project INTEGER REFERENCES projects(id)
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
    last_task INTEGER REFERENCES tasks(id), project INTEGER REFERENCES projects(id)
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
-- A piece of one goal, with its own lead and, if the owner gives it one, a monthly cap
-- (ADR 0113).
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY, goal INTEGER NOT NULL REFERENCES goals(id), title TEXT NOT NULL,
    reason TEXT NOT NULL, lead INTEGER NOT NULL REFERENCES members(id), status TEXT NOT NULL,
    pause TEXT, cap INTEGER, asked_cap INTEGER, created_by TEXT NOT NULL,
    proposer INTEGER REFERENCES members(id), proposed_in INTEGER REFERENCES tasks(id),
    evidence TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
) STRICT;
";

    #[test]
    fn a_store_made_before_evidence_gains_its_columns_and_tables_and_keeps_its_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(SCHEMA_BEFORE_EVIDENCE).unwrap();
        conn.execute_batch(
            "INSERT INTO members (id, name, role, share_percent, slots) VALUES
                (1, 'Ada', 'lead', 25, 2);
             INSERT INTO goals (id, title, reason, status) VALUES (1, 'Keep it up', '', 'active');
             INSERT INTO tasks (id, goal, title, description, status, priority, depth) VALUES
                (1, 1, 'Check the home lab', '', 'doing', 'high', 0);
             INSERT INTO posts (task, author, text, at_ms) VALUES (1, 'Ada', 'All up.', 5);
             INSERT INTO wakes (id, member, task, reason, receipt, at_ms) VALUES
                (1, 1, 1, 'assigned', 'started', 0);
             INSERT INTO runs (id, member, task, wake, status, queued_at, started_at, ended_at,
                moved_forward) VALUES (1, 1, 1, 1, 'succeeded', 0, 0, 1, 1);",
        )
        .unwrap();
        let store = SqliteTeamStore::over(conn).unwrap();
        // Old rows read as they were: no criteria, a post that is not marked, a turn with no
        // label (the back-off reads it by whether it moved its task forward).
        let task = store.task(TaskId(1)).unwrap().unwrap();
        assert!(task.criteria.is_empty());
        let post = store.thread(TaskId(1)).unwrap().pop().unwrap();
        assert_eq!((post.text.as_str(), post.unsupported), ("All up.", false));
        let run = store.run(RunId(1)).unwrap().unwrap();
        assert_eq!(
            (run.moved_forward, run.posted, run.blocked, run.label),
            (true, false, false, None)
        );
        // And new ones are kept.
        store
            .set_criteria(TaskId(1), &crate::domain::Criterion::lab())
            .unwrap();
        assert_eq!(
            store.task(TaskId(1)).unwrap().unwrap().criteria,
            crate::domain::Criterion::lab()
        );
        let id = store
            .add_call_receipt(&crate::domain::CallReceipt {
                id: 0,
                run: RunId(1),
                member: MemberId(1),
                task: Some(TaskId(1)),
                tool: "homelab_status".to_owned(),
                args: String::new(),
                excerpt: "all up".to_owned(),
                ok: true,
                act: false,
                confidential: false,
                at: 7,
            })
            .unwrap();
        assert_eq!(store.call_receipts_of(RunId(1)).unwrap()[0].id, id);
        let mut run = store.run(RunId(1)).unwrap().unwrap();
        run.label = Some(crate::domain::Label::Checked);
        store.save_run(&run).unwrap();
        assert_eq!(
            store.run(RunId(1)).unwrap().unwrap().label,
            Some(crate::domain::Label::Checked)
        );
        let conn = store.conn().unwrap();
        assert!(
            conn.execute("UPDATE call_receipts SET ok = 0", []).is_err(),
            "a receipt is never changed"
        );
        drop(conn);
        // The wing's tables (ADR 0115) are there too, and take rows.
        let doc = store
            .add_document(&Document {
                id: 0,
                task: TaskId(1),
                run: Some(RunId(1)),
                title: "branch hq/task-1-x (1 file, +1 −0)".to_owned(),
                text: "diff".to_owned(),
                at: 8,
            })
            .unwrap();
        assert_eq!(store.documents_of(TaskId(1)).unwrap()[0].id, doc);
        assert_eq!(store.document(doc).unwrap().unwrap().text, "diff");
        let branch = store
            .add_branch(&Branch {
                id: 0,
                task: TaskId(1),
                run: RunId(1),
                member: MemberId(1),
                name: "hq/task-1-x".to_owned(),
                head: "abc".to_owned(),
                base: "def".to_owned(),
                files: 1,
                plus: 1,
                minus: 0,
                can_push: false,
                status: "submitted".to_owned(),
                at: 8,
            })
            .unwrap();
        let (check, token) = store
            .add_check(&Check {
                id: 0,
                task: TaskId(1),
                branch,
                run: RunId(1),
                token: String::new(),
                status: "waiting".to_owned(),
                ok: None,
                summary: String::new(),
                queued_at: 9,
                started_at: None,
                done_at: None,
            })
            .unwrap();
        assert_eq!(token.len(), 64, "the job's own token, made by the store");
        let mut kept = store.checks().unwrap().pop().unwrap();
        assert_eq!(
            (kept.id, kept.token.as_str(), kept.status.as_str()),
            (check, token.as_str(), "waiting")
        );
        kept.status = "done".to_owned();
        kept.ok = Some(true);
        kept.summary = "syntax ok".to_owned();
        kept.done_at = Some(10);
        store.save_check(&kept).unwrap();
        assert_eq!(store.checks().unwrap()[0], kept);
        let mut b = store.branches().unwrap().pop().unwrap();
        b.status = "pushed".to_owned();
        store.save_branch(&b).unwrap();
        assert_eq!(store.branches().unwrap()[0].status, "pushed");
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
