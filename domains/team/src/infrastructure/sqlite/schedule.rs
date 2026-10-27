//! Timers, routines and check-backs in HQ's store.

use rusqlite::params;

use super::{SqliteTeamStore, backend};
use crate::application::ports::{NewRoutine, Stored, Timer};
use crate::application::schedule_ports::ScheduleStore;
use crate::domain::{
    CheckBack, CheckBackStatus, GoalId, MemberId, Millis, ProjectId, Routine, RoutineId, TaskId,
    WakeReason,
};

impl ScheduleStore for SqliteTeamStore {
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
                    last_task = ?5, project = ?6, assignee = ?7 WHERE id = ?1",
                params![
                    routine.id.0,
                    routine.enabled,
                    routine.next_due_at,
                    routine.last_fired_at,
                    routine.last_task.map(|t| t.0),
                    routine.project.map(|p| p.0),
                    routine.assignee.0
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
