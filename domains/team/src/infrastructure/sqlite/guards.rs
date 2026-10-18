//! The guards in HQ's store (ADR 0124). Their tables are made only if missing, so a live store
//! gains them empty and no row of any other table changes. A nudge's receipt is append-only:
//! a trigger refuses to change or remove one.

use rusqlite::{OptionalExtension, params};

use super::{SqliteTeamStore, backend, corrupt};
use crate::application::guard_ports::GuardStore;
use crate::application::ports::Stored;
use crate::domain::closure::{Closed, Closure};
use crate::domain::guards::{Finding, Guard, Verdict};
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
CREATE TABLE IF NOT EXISTS findings (
    id INTEGER PRIMARY KEY, guard TEXT NOT NULL, key TEXT NOT NULL,
    task INTEGER REFERENCES tasks(id), project INTEGER REFERENCES projects(id),
    rule TEXT NOT NULL, evidence TEXT NOT NULL, opened_at INTEGER NOT NULL, cleared_at INTEGER,
    verdict TEXT, fix_worked INTEGER, judged_by TEXT NOT NULL DEFAULT '', judged_at INTEGER,
    desk INTEGER REFERENCES desk(id)
) STRICT;
CREATE UNIQUE INDEX IF NOT EXISTS findings_open_by_key ON findings(key) WHERE cleared_at IS NULL;
CREATE TABLE IF NOT EXISTS closures (
    task INTEGER PRIMARY KEY REFERENCES tasks(id), status TEXT NOT NULL, why TEXT NOT NULL,
    duplicate_of INTEGER REFERENCES tasks(id), successor INTEGER REFERENCES tasks(id),
    by TEXT NOT NULL, at_ms INTEGER NOT NULL
) STRICT;
";

const FINDING_SELECT: &str = "SELECT id, guard, key, task, project, rule, evidence, opened_at, \
    cleared_at, verdict, fix_worked, judged_by, desk FROM findings";

type FindingRow = (Finding, String, Option<String>);

fn finding_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<FindingRow> {
    Ok((
        Finding {
            id: r.get(0)?,
            guard: Guard::Stuck,
            key: r.get(2)?,
            task: r.get(3)?,
            project: r.get(4)?,
            rule: r.get(5)?,
            evidence: r.get(6)?,
            opened_at: r.get(7)?,
            cleared_at: r.get(8)?,
            verdict: None,
            fix_worked: r.get::<_, Option<i64>>(10)?.map(|f| f != 0),
            judged_by: r.get(11)?,
            desk: r.get(12)?,
        },
        r.get(1)?,
        r.get(9)?,
    ))
}

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

    fn findings(&self) -> Stored<Vec<Finding>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{FINDING_SELECT} ORDER BY id"))
            .map_err(backend)?;
        let rows = stmt
            .query_map([], finding_row)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|(mut f, guard, verdict)| {
                f.guard = Guard::from_name(&guard).ok_or_else(|| corrupt("a finding's guard"))?;
                f.verdict = match verdict {
                    None => None,
                    Some(v) => {
                        Some(Verdict::from_name(&v).ok_or_else(|| corrupt("a finding's verdict"))?)
                    }
                };
                Ok(f)
            })
            .collect()
    }

    fn open_finding(&self, f: &Finding) -> Stored<Option<i64>> {
        let conn = self.conn()?;
        let added = conn
            .execute(
                "INSERT OR IGNORE INTO findings (guard, key, task, project, rule, evidence, opened_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    f.guard.name(),
                    f.key,
                    f.task,
                    f.project,
                    f.rule,
                    f.evidence,
                    f.opened_at
                ],
            )
            .map_err(backend)?;
        Ok((added == 1).then(|| conn.last_insert_rowid()))
    }

    fn clear_finding(&self, id: i64, at: Millis) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE findings SET cleared_at = ?2 WHERE id = ?1 AND cleared_at IS NULL",
                params![id, at],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn judge_finding(
        &self,
        id: i64,
        verdict: Verdict,
        fix_worked: Option<bool>,
        by: &str,
        at: Millis,
    ) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE findings SET verdict = ?2, fix_worked = COALESCE(?3, fix_worked),
                    judged_by = ?4, judged_at = ?5 WHERE id = ?1",
                params![id, verdict.name(), fix_worked.map(i64::from), by, at],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn set_finding_desk(&self, id: i64, desk: i64) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE findings SET desk = ?2 WHERE id = ?1",
                params![id, desk],
            )
            .map_err(backend)?;
        Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn one_finding_is_open_a_key_and_it_gains_its_clearing_and_verdict() {
        let (store, task, _) = store_with_a_task();
        let f = Finding {
            id: 0,
            guard: Guard::Ceremony,
            key: "ceremony:p1".to_owned(),
            task: Some(task.0),
            project: None,
            rule: Guard::Ceremony.rule(),
            evidence: "14 coordination turns, 1 product change".to_owned(),
            opened_at: 10,
            cleared_at: None,
            verdict: None,
            fix_worked: None,
            judged_by: String::new(),
            desk: None,
        };
        let id = store.open_finding(&f).unwrap().expect("a new finding");
        assert_eq!(store.open_finding(&f).unwrap(), None, "still open");
        store.clear_finding(id, 20).unwrap();
        store
            .open_finding(&f)
            .unwrap()
            .expect("a new one, once cleared");
        store
            .judge_finding(id, Verdict::Real, Some(true), "owner", 30)
            .unwrap();
        let all = store.findings().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].cleared_at, Some(20));
        assert_eq!(all[0].verdict, Some(Verdict::Real));
        assert_eq!(all[0].fix_worked, Some(true));
        assert_eq!(all[0].judged_by, "owner");
        assert_eq!(all[1].cleared_at, None);
    }
}
