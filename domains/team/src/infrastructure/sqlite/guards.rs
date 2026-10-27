//! The guards in HQ's store (ADR 0124). Their tables are made only if missing, so a live store
//! gains them empty and no row of any other table changes. A nudge's receipt is append-only:
//! a trigger refuses to change or remove one.

use rusqlite::{OptionalExtension, params};

use super::{SqliteTeamStore, backend, corrupt};
use crate::application::guard_ports::GuardStore;
use crate::application::ports::Stored;
use crate::domain::closure::{Closed, Closure};
use crate::domain::{MemberId, Millis, TaskId, TaskStatus};

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS nudges (
    key TEXT PRIMARY KEY, task INTEGER NOT NULL REFERENCES tasks(id),
    member INTEGER NOT NULL REFERENCES members(id), reason TEXT NOT NULL, at_ms INTEGER NOT NULL
) STRICT;
CREATE TRIGGER IF NOT EXISTS nudges_are_kept_update
    BEFORE UPDATE ON nudges BEGIN SELECT RAISE(ABORT, 'a nudge is kept as sent'); END;
CREATE TRIGGER IF NOT EXISTS nudges_are_kept_delete
    BEFORE DELETE ON nudges BEGIN SELECT RAISE(ABORT, 'a nudge is kept as sent'); END;
CREATE TABLE IF NOT EXISTS closures (
    task INTEGER PRIMARY KEY REFERENCES tasks(id), status TEXT NOT NULL, why TEXT NOT NULL,
    duplicate_of INTEGER REFERENCES tasks(id), successor INTEGER REFERENCES tasks(id),
    by TEXT NOT NULL, at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS desk_attached (
    desk INTEGER NOT NULL REFERENCES desk(id), task INTEGER NOT NULL REFERENCES tasks(id),
    member INTEGER REFERENCES members(id), PRIMARY KEY (desk, task)
) STRICT;
";

impl GuardStore for SqliteTeamStore {
    fn record_nudge(
        &self,
        key: &str,
        task: TaskId,
        member: MemberId,
        reason: &str,
        at: Millis,
    ) -> Stored<bool> {
        let conn = self.conn()?;
        let added = conn
            .execute(
                "INSERT OR IGNORE INTO nudges (key, task, member, reason, at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![key, task.0, member.0, reason, at],
            )
            .map_err(backend)?;
        Ok(added == 1)
    }

    fn record_closure(&self, c: &Closed) -> Stored<()> {
        self.conn()?
            .execute(
                "INSERT OR REPLACE INTO closures (task, status, why, duplicate_of, successor, by, at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    c.task.0,
                    c.closure.status().name(),
                    c.closure.name(),
                    c.closure.duplicate_of().map(|t| t.0),
                    c.successor.map(|t| t.0),
                    c.by,
                    c.at
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn closure(&self, task: TaskId) -> Stored<Option<Closed>> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT status, why, duplicate_of, successor, by, at_ms FROM closures WHERE task = ?1",
                [task.0],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        let Some((status, why, duplicate_of, successor, by, at)) = row else {
            return Ok(None);
        };
        let status = TaskStatus::from_name(&status).ok_or_else(|| corrupt("a closure's status"))?;
        let closure = Closure::read(status, &why, duplicate_of.map(TaskId))
            .map_err(|_| corrupt("a closure's reason"))?;
        Ok(Some(Closed {
            task,
            closure,
            successor: successor.map(TaskId),
            by,
            at,
        }))
    }

    fn attach_to_desk(&self, desk: i64, task: TaskId, member: Option<MemberId>) -> Stored<()> {
        self.conn()?
            .execute(
                "INSERT OR IGNORE INTO desk_attached (desk, task, member) VALUES (?1, ?2, ?3)",
                params![desk, task.0, member.map(|m| m.0)],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn attached(&self, desk: i64) -> Stored<Vec<(TaskId, Option<MemberId>)>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT task, member FROM desk_attached WHERE desk = ?1 ORDER BY rowid")
            .map_err(backend)?;
        stmt.query_map([desk], |r| {
            Ok((TaskId(r.get(0)?), r.get::<_, Option<i64>>(1)?.map(MemberId)))
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::desk_ports::DeskStore;
    use crate::application::ports::TeamStore;
    use crate::domain::Role;

    fn store_with_a_task() -> (SqliteTeamStore, TaskId, MemberId) {
        let store = SqliteTeamStore::in_memory().unwrap();
        let member = store.add_member("Jane", Role::Developer, 40, 1, 0).unwrap();
        let goal = store
            .add_goal(
                "Keep the home lab healthy",
                "",
                crate::domain::GoalStatus::Active,
                None,
            )
            .unwrap();
        let task = store
            .add_task(
                &crate::application::ports::NewTask {
                    goal,
                    project: None,
                    parent: None,
                    title: "Look".to_owned(),
                    description: String::new(),
                    assignee: Some(member),
                    priority: crate::domain::Priority::Normal,
                    waits_on: Vec::new(),
                },
                0,
            )
            .unwrap();
        (store, task, member)
    }

    #[test]
    fn a_nudge_is_recorded_once_and_kept_as_sent() {
        let (store, task, member) = store_with_a_task();
        assert!(
            store
                .record_nudge("k1", task, member, "recovery", 5)
                .unwrap()
        );
        assert!(
            !store
                .record_nudge("k1", task, member, "recovery", 6)
                .unwrap()
        );
        assert!(
            store
                .record_nudge("k2", task, member, "recovery", 7)
                .unwrap()
        );
        let conn = store.conn().unwrap();
        assert!(conn.execute("DELETE FROM nudges", []).is_err());
        assert!(conn.execute("UPDATE nudges SET at_ms = 0", []).is_err());
    }

    /// A finding seen again says so on the desk card it put up, while the card waits; a card the
    /// owner has decided keeps the words it was decided on.
    #[test]
    fn a_desk_item_that_waits_is_reworded_and_one_that_was_decided_is_not() {
        let (store, task, member) = store_with_a_task();
        let item = store
            .add_desk("guard", Some(task), None, Some(member), "first words", 5)
            .unwrap();
        store.set_desk_text(item, "newer words").unwrap();
        let shown = |store: &SqliteTeamStore| store.desk().unwrap().remove(0).text;
        assert_eq!(shown(&store), "newer words");
        store.set_desk_status(item, "real").unwrap();
        store
            .set_desk_text(item, "words after it was decided")
            .unwrap();
        assert_eq!(shown(&store), "newer words");
    }
}
