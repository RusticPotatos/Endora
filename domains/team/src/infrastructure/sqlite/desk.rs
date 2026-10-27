//! The owner's desk in HQ's store.

use rusqlite::params;

use super::{SqliteTeamStore, backend};
use crate::application::desk_ports::DeskStore;
use crate::application::ports::{DeskItem, Stored};
use crate::domain::{DeskId, GoalId, MemberId, Millis, ProjectId, TaskId};

impl DeskStore for SqliteTeamStore {
    fn set_desk_project(&self, id: DeskId, project: ProjectId) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE desk SET project = ?2 WHERE id = ?1",
                params![id.0, project.0],
            )
            .map_err(backend)?;
        Ok(())
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

    fn set_desk_text(&self, id: DeskId, text: &str) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE desk SET text = ?2 WHERE id = ?1 AND status = 'waiting'",
                params![id.0, text],
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
}
