//! The guards in HQ's store (ADR 0124). Their tables are made only if missing, so a live store
//! gains them empty and no row of any other table changes. A nudge's receipt is append-only:
//! a trigger refuses to change or remove one.

use rusqlite::{OptionalExtension, params};

use super::{SqliteTeamStore, add_column, backend, corrupt};
use crate::application::guard_ports::GuardStore;
use crate::application::ports::Stored;
use crate::domain::closure::{Closed, Closure};
use crate::domain::guards::{Finding, Guard, Verdict};
use crate::domain::retraction::{self, Retraction};
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
CREATE TABLE IF NOT EXISTS retractions (
    id INTEGER PRIMARY KEY, target TEXT NOT NULL, text TEXT NOT NULL, fingerprint TEXT NOT NULL,
    at_ms INTEGER NOT NULL
) STRICT;
CREATE TRIGGER IF NOT EXISTS retractions_are_kept_update
    BEFORE UPDATE ON retractions BEGIN SELECT RAISE(ABORT, 'a retraction is kept as made'); END;
CREATE TRIGGER IF NOT EXISTS retractions_are_kept_delete
    BEFORE DELETE ON retractions BEGIN SELECT RAISE(ABORT, 'a retraction is kept as made'); END;
CREATE TABLE IF NOT EXISTS note_parents (
    note INTEGER NOT NULL, parent INTEGER NOT NULL, PRIMARY KEY (note, parent)
) STRICT;
CREATE INDEX IF NOT EXISTS note_parents_by_parent ON note_parents(parent);
CREATE TABLE IF NOT EXISTS desk_attached (
    desk INTEGER NOT NULL REFERENCES desk(id), task INTEGER NOT NULL REFERENCES tasks(id),
    member INTEGER REFERENCES members(id), PRIMARY KEY (desk, task)
) STRICT;
";

/// The columns a store whose findings were made before the quieter desk (ADR 0132) gains: when a
/// finding was last seen, how many times it has opened, and what it rests on. Adding a column
/// changes no row; a finding with no time seen is read as seen when it opened.
const COLUMNS: [(&str, &str); 3] = [
    ("findings", "seen_at INTEGER"),
    ("findings", "episodes INTEGER NOT NULL DEFAULT 1"),
    ("findings", "mark INTEGER NOT NULL DEFAULT 0"),
];

/// Adds the findings' newer columns where a store lacks them; a second call changes nothing.
pub(super) fn gain_columns(conn: &rusqlite::Connection) -> Stored<()> {
    for (table, column) in COLUMNS {
        add_column(conn, table, column)?;
    }
    Ok(())
}

const FINDING_SELECT: &str = "SELECT id, guard, key, task, project, rule, evidence, opened_at, \
    cleared_at, verdict, fix_worked, judged_by, desk, COALESCE(seen_at, opened_at), episodes, \
    mark FROM findings";

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
            seen_at: r.get(13)?,
            episodes: r.get(14)?,
            mark: r.get(15)?,
        },
        r.get(1)?,
        r.get(9)?,
    ))
}

/// A finding as read, with its guard and verdict named by the words they are stored as.
fn finding_from((mut f, guard, verdict): FindingRow) -> Stored<Finding> {
    f.guard = Guard::from_name(&guard).ok_or_else(|| corrupt("a finding's guard"))?;
    f.verdict = match verdict {
        None => None,
        Some(v) => Some(Verdict::from_name(&v).ok_or_else(|| corrupt("a finding's verdict"))?),
    };
    Ok(f)
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
        rows.into_iter().map(finding_from).collect()
    }

    fn open_finding(&self, f: &Finding) -> Stored<Option<i64>> {
        let conn = self.conn()?;
        let added = conn
            .execute(
                "INSERT OR IGNORE INTO findings
                    (guard, key, task, project, rule, evidence, opened_at, seen_at, episodes, mark)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    f.guard.name(),
                    f.key,
                    f.task,
                    f.project,
                    f.rule,
                    f.evidence,
                    f.opened_at,
                    f.seen_at,
                    f.episodes,
                    f.mark
                ],
            )
            .map_err(backend)?;
        Ok((added == 1).then(|| conn.last_insert_rowid()))
    }

    fn latest_finding(&self, key: &str) -> Stored<Option<Finding>> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                &format!("{FINDING_SELECT} WHERE key = ?1 ORDER BY id DESC LIMIT 1"),
                [key],
                finding_row,
            )
            .optional()
            .map_err(backend)?;
        row.map(finding_from).transpose()
    }

    fn refresh_finding(
        &self,
        id: i64,
        evidence: &str,
        seen_at: Millis,
        mark: Option<i64>,
    ) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE findings SET evidence = ?2, seen_at = ?3,
                    mark = CASE WHEN ?4 IS NULL THEN mark ELSE MAX(mark, ?4) END
                 WHERE id = ?1 AND cleared_at IS NULL",
                params![id, evidence, seen_at, mark],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn reopen_finding(&self, id: i64, evidence: &str, at: Millis, mark: i64) -> Stored<bool> {
        let opened = self
            .conn()?
            .execute(
                "UPDATE findings SET cleared_at = NULL, opened_at = ?3, seen_at = ?3, evidence = ?2,
                    episodes = episodes + 1, mark = MAX(mark, ?4)
                 WHERE id = ?1 AND cleared_at IS NOT NULL",
                params![id, evidence, at, mark],
            )
            .map_err(backend)?;
        Ok(opened == 1)
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

    fn add_retraction(&self, r: &Retraction) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO retractions (target, text, fingerprint, at_ms) VALUES (?1, ?2, ?3, ?4)",
            params![r.target, r.text, retraction::kept(&r.fingerprint), r.at],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn retractions(&self) -> Stored<Vec<Retraction>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT id, target, text, fingerprint, at_ms FROM retractions ORDER BY id")
            .map_err(backend)?;
        stmt.query_map([], |r| {
            Ok(Retraction {
                id: r.get(0)?,
                target: r.get(1)?,
                text: r.get(2)?,
                fingerprint: retraction::read(&r.get::<_, String>(3)?),
                at: r.get(4)?,
            })
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn add_note_parent(&self, note: i64, parent: i64) -> Stored<()> {
        self.conn()?
            .execute(
                "INSERT OR IGNORE INTO note_parents (note, parent) VALUES (?1, ?2)",
                params![note, parent],
            )
            .map_err(backend)?;
        Ok(())
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

    fn note_children(&self, parent: i64) -> Stored<Vec<i64>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT note FROM note_parents WHERE parent = ?1 ORDER BY note")
            .map_err(backend)?;
        stmt.query_map([parent], |r| r.get(0))
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
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
            seen_at: 10,
            episodes: 1,
            mark: 14,
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
        // What it rests on and when it was seen are kept as they were given.
        assert_eq!(
            (all[0].seen_at, all[0].episodes, all[0].mark),
            (10, 1, 14),
            "{:?}",
            all[0]
        );
    }

    fn ceremony_finding(task: TaskId) -> Finding {
        Finding {
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
            seen_at: 10,
            episodes: 1,
            mark: 14,
        }
    }

    /// One finding for a key (ADR 0132): seen again it is refreshed in place, its mark only ever
    /// rising and left alone when none is given; cleared, it opens again as itself with one more
    /// episode; and the newest for the key is what a later detection is weighed against.
    #[test]
    fn a_finding_is_refreshed_and_opened_again_in_place_and_the_newest_for_its_key_is_found() {
        let (store, task, _) = store_with_a_task();
        assert_eq!(store.latest_finding("ceremony:p1").unwrap(), None);
        let id = store
            .open_finding(&ceremony_finding(task))
            .unwrap()
            .expect("a new finding");

        store
            .refresh_finding(id, "16 coordination turns, 1 product change", 20, Some(16))
            .unwrap();
        let seen = store.latest_finding("ceremony:p1").unwrap().unwrap();
        assert_eq!(seen.evidence, "16 coordination turns, 1 product change");
        assert_eq!(
            (seen.seen_at, seen.mark, seen.episodes, seen.opened_at),
            (20, 16, 1, 10)
        );
        // A lower count never lowers it; none given leaves it as it stands (a judged one).
        store.refresh_finding(id, "13 turns", 25, Some(13)).unwrap();
        store.refresh_finding(id, "15 turns", 26, None).unwrap();
        let seen = store.latest_finding("ceremony:p1").unwrap().unwrap();
        assert_eq!((seen.seen_at, seen.mark), (26, 16), "{seen:?}");

        // Opened again: cleared first, and only a cleared one opens.
        assert!(
            !store.reopen_finding(id, "x", 30, 99).unwrap(),
            "an open finding is not opened again"
        );
        store.clear_finding(id, 30).unwrap();
        assert!(
            store
                .reopen_finding(id, "18 coordination turns, 1 product change", 40, 18)
                .unwrap()
        );
        let again = store.latest_finding("ceremony:p1").unwrap().unwrap();
        assert_eq!(again.id, id, "the same finding");
        assert_eq!(
            (
                again.cleared_at,
                again.opened_at,
                again.seen_at,
                again.episodes,
                again.mark
            ),
            (None, 40, 40, 2, 18)
        );
        assert_eq!(again.evidence, "18 coordination turns, 1 product change");
        assert_eq!(store.findings().unwrap().len(), 1, "still one for the key");

        // The newest for a key is the last one opened: the one judged and cleared before it is
        // what it was weighed against.
        store.clear_finding(id, 50).unwrap();
        let second = store
            .open_finding(&Finding {
                opened_at: 60,
                seen_at: 60,
                ..ceremony_finding(task)
            })
            .unwrap()
            .expect("a new one once cleared");
        assert_eq!(
            store.latest_finding("ceremony:p1").unwrap().unwrap().id,
            second
        );
        assert_eq!(store.latest_finding("ceremony:p2").unwrap(), None);
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
