//! Content-free provenance for forward-authored goal/task inputs (ADR 0138).

use super::{backend, corrupt};
use crate::application::ports::Stored;
use crate::domain::RunId;
use crate::domain::history::{InputOrigin, InputRef};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS input_origins (
 kind TEXT NOT NULL CHECK(kind IN ('goal','task_body','task_criteria','post')),
 resource INTEGER NOT NULL, origin TEXT NOT NULL CHECK(origin IN ('owner','run')),
 source_run INTEGER REFERENCES runs(id) ON DELETE CASCADE,
 CHECK((origin='owner' AND source_run IS NULL) OR (origin='run' AND source_run IS NOT NULL)),
 PRIMARY KEY(kind,resource)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS input_origins_by_run ON input_origins(source_run);
CREATE TRIGGER IF NOT EXISTS goal_input_changed AFTER UPDATE OF title,reason,parent ON goals
 BEGIN DELETE FROM input_origins WHERE kind='goal' AND resource=NEW.id; END;
CREATE TRIGGER IF NOT EXISTS task_body_input_changed AFTER UPDATE OF title,description,goal,parent ON tasks
 BEGIN DELETE FROM input_origins WHERE kind='task_body' AND resource=NEW.id; END;
CREATE TRIGGER IF NOT EXISTS task_criteria_input_changed AFTER UPDATE OF criteria ON tasks
 BEGIN DELETE FROM input_origins WHERE kind='task_criteria' AND resource=NEW.id; END;
CREATE TRIGGER IF NOT EXISTS post_input_changed AFTER UPDATE OF task,author,text,at_ms,evidence,unsupported ON posts
 BEGIN DELETE FROM input_origins WHERE kind='post' AND resource=NEW.id; END;
CREATE TRIGGER IF NOT EXISTS goal_input_deleted AFTER DELETE ON goals
 BEGIN DELETE FROM input_origins WHERE kind='goal' AND resource=OLD.id; END;
CREATE TRIGGER IF NOT EXISTS task_input_deleted AFTER DELETE ON tasks
 BEGIN DELETE FROM input_origins WHERE kind IN ('task_body','task_criteria') AND resource=OLD.id; END;
CREATE TRIGGER IF NOT EXISTS post_input_deleted AFTER DELETE ON posts
 BEGIN DELETE FROM input_origins WHERE kind='post' AND resource=OLD.id; END;
";

fn key(input: InputRef) -> (&'static str, i64) {
    match input {
        InputRef::Goal(id) => ("goal", id.0),
        InputRef::TaskBody(id) => ("task_body", id.0),
        InputRef::TaskCriteria(id) => ("task_criteria", id.0),
        InputRef::Post(id) => ("post", id),
    }
}

/// Called only inside the transaction that authored the associated input.
pub(super) fn record(conn: &Connection, input: InputRef, origin: InputOrigin) -> Stored<()> {
    let (kind, id) = key(input);
    let (origin, source) = match origin {
        InputOrigin::Unknown => return Ok(()),
        InputOrigin::Owner => ("owner", None),
        InputOrigin::Run(run) => ("run", Some(run.0)),
    };
    conn.execute(
        "INSERT INTO input_origins(kind,resource,origin,source_run) VALUES(?1,?2,?3,?4)",
        params![kind, id, origin, source],
    )
    .map_err(backend)?;
    Ok(())
}

pub(super) fn read(conn: &Connection, input: InputRef) -> Stored<InputOrigin> {
    let (kind, id) = key(input);
    let row: Option<(String, Option<i64>)> = conn
        .query_row(
            "SELECT origin,source_run FROM input_origins WHERE kind=?1 AND resource=?2",
            params![kind, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    match row {
        None => Ok(InputOrigin::Unknown),
        Some((kind, None)) if kind == "owner" => Ok(InputOrigin::Owner),
        Some((kind, Some(run))) if kind == "run" => Ok(InputOrigin::Run(RunId(run))),
        _ => Err(corrupt("input provenance")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::SqliteTeamStore;
    use crate::application::history_ports::HistoryStore;
    use crate::application::ports::{NewTask, Post, TeamStore};
    use crate::domain::history::{InputOrigin, InputRef};
    use crate::domain::{GoalStatus, Priority, RunId};

    fn task(goal: crate::domain::GoalId) -> NewTask {
        NewTask {
            goal,
            project: None,
            parent: None,
            title: "Synthetic check".into(),
            description: "Inspect the fixture".into(),
            assignee: None,
            priority: Priority::Normal,
            waits_on: vec![],
        }
    }

    #[test]
    fn forward_writes_record_origin_without_guessing_from_display_author() {
        let store = SqliteTeamStore::in_memory().unwrap();
        let goal = store
            .add_goal_from("Fixture", "", GoalStatus::Active, None, InputOrigin::Owner)
            .unwrap();
        assert_eq!(
            store.input_origin(InputRef::Goal(goal)).unwrap(),
            InputOrigin::Owner
        );
        let task = store
            .add_task_from(&task(goal), 0, InputOrigin::Owner)
            .unwrap();
        assert_eq!(
            store.input_origin(InputRef::TaskBody(task)).unwrap(),
            InputOrigin::Owner
        );
        assert_eq!(
            store.input_origin(InputRef::TaskCriteria(task)).unwrap(),
            InputOrigin::Owner
        );
        store
            .post(task, "owner", "Legacy author text is not authority", 1)
            .unwrap();
        let first = store.conn().unwrap().last_insert_rowid();
        assert_eq!(
            store.input_origin(InputRef::Post(first)).unwrap(),
            InputOrigin::Unknown
        );
        store
            .post_from(
                task,
                &Post {
                    author: "owner".into(),
                    text: "Actual owner input".into(),
                    at: 2,
                    unsupported: false,
                },
                &[],
                InputOrigin::Owner,
            )
            .unwrap();
        let second = store.conn().unwrap().last_insert_rowid();
        assert_eq!(
            store.input_origin(InputRef::Post(second)).unwrap(),
            InputOrigin::Owner
        );
    }

    #[test]
    fn an_invalid_source_cannot_leave_a_task_or_post_behind() {
        let store = SqliteTeamStore::in_memory().unwrap();
        let goal = store
            .add_goal("Fixture", "", GoalStatus::Active, None)
            .unwrap();
        assert!(
            store
                .add_task_from(&task(goal), 0, InputOrigin::Run(RunId(999)))
                .is_err()
        );
        assert!(store.tasks().unwrap().is_empty());
        let task = store.add_task(&task(goal), 0).unwrap();
        assert!(
            store
                .post_from(
                    task,
                    &Post {
                        author: "Member 1".into(),
                        text: "Uncommitted".into(),
                        at: 0,
                        unsupported: false
                    },
                    &[],
                    InputOrigin::Run(RunId(999))
                )
                .is_err()
        );
        assert!(store.thread(task).unwrap().is_empty());
    }

    #[test]
    fn untracked_content_changes_invalidate_only_the_affected_input() {
        let store = SqliteTeamStore::in_memory().unwrap();
        let goal = store
            .add_goal_from("Fixture", "", GoalStatus::Active, None, InputOrigin::Owner)
            .unwrap();
        let task = store
            .add_task_from(&task(goal), 0, InputOrigin::Owner)
            .unwrap();
        store
            .conn()
            .unwrap()
            .execute("UPDATE tasks SET nudged=1 WHERE id=?1", [task.0])
            .unwrap();
        assert_eq!(
            store.input_origin(InputRef::TaskBody(task)).unwrap(),
            InputOrigin::Owner
        );
        store.set_criteria(task, &[]).unwrap();
        assert_eq!(
            store.input_origin(InputRef::TaskCriteria(task)).unwrap(),
            InputOrigin::Unknown
        );
        assert_eq!(
            store.input_origin(InputRef::TaskBody(task)).unwrap(),
            InputOrigin::Owner
        );
        store
            .set_criteria_from(task, &[], InputOrigin::Owner)
            .unwrap();
        store
            .conn()
            .unwrap()
            .execute(
                "UPDATE tasks SET title='Changed by an untracked writer' WHERE id=?1",
                [task.0],
            )
            .unwrap();
        assert_eq!(
            store.input_origin(InputRef::TaskBody(task)).unwrap(),
            InputOrigin::Unknown
        );
        assert_eq!(
            store.input_origin(InputRef::TaskCriteria(task)).unwrap(),
            InputOrigin::Owner
        );
        store
            .post_from(
                task,
                &Post {
                    author: "owner".into(),
                    text: "Initial finding".into(),
                    at: 0,
                    unsupported: false,
                },
                &[],
                InputOrigin::Owner,
            )
            .unwrap();
        let post = store.conn().unwrap().last_insert_rowid();
        store
            .conn()
            .unwrap()
            .execute("UPDATE posts SET text='Untracked edit' WHERE id=?1", [post])
            .unwrap();
        assert_eq!(
            store.input_origin(InputRef::Post(post)).unwrap(),
            InputOrigin::Unknown
        );
        store
            .conn()
            .unwrap()
            .execute(
                "UPDATE goals SET reason='Untracked reason' WHERE id=?1",
                [goal.0],
            )
            .unwrap();
        assert_eq!(
            store.input_origin(InputRef::Goal(goal)).unwrap(),
            InputOrigin::Unknown
        );
    }

    #[test]
    fn provenance_failure_rolls_back_the_content_and_previous_proof() {
        use crate::domain::Criterion;
        let store = SqliteTeamStore::in_memory().unwrap();
        let goal = store
            .add_goal_from("Fixture", "", GoalStatus::Active, None, InputOrigin::Owner)
            .unwrap();
        let task = store
            .add_task_from(&task(goal), 0, InputOrigin::Owner)
            .unwrap();
        let original = [Criterion::new("Initial criterion", false)];
        store
            .set_criteria_from(task, &original, InputOrigin::Owner)
            .unwrap();
        store.conn().unwrap().execute_batch("CREATE TRIGGER fail_input_provenance BEFORE INSERT ON input_origins BEGIN SELECT RAISE(ABORT,'synthetic provenance failure'); END;").unwrap();
        assert!(
            store
                .set_criteria_from(
                    task,
                    &[Criterion::new("Changed criterion", false)],
                    InputOrigin::Owner
                )
                .is_err()
        );
        assert_eq!(store.task(task).unwrap().unwrap().criteria, original);
        assert_eq!(
            store.input_origin(InputRef::TaskCriteria(task)).unwrap(),
            InputOrigin::Owner
        );
        assert!(
            store
                .post_from(
                    task,
                    &Post {
                        author: "owner".into(),
                        text: "Failed finding".into(),
                        at: 0,
                        unsupported: false
                    },
                    &[],
                    InputOrigin::Owner
                )
                .is_err()
        );
        assert!(store.thread(task).unwrap().is_empty());
        assert!(
            store
                .add_task_from(&super::tests::task(goal), 0, InputOrigin::Owner)
                .is_err()
        );
        assert_eq!(store.tasks().unwrap().len(), 1);
        assert!(
            store
                .add_goal_from(
                    "Failed goal",
                    "",
                    GoalStatus::Active,
                    None,
                    InputOrigin::Owner
                )
                .is_err()
        );
        assert_eq!(store.goals().unwrap().len(), 1);
        store.conn().unwrap().execute_batch("DROP TRIGGER fail_input_provenance;
            CREATE TRIGGER fail_later_provenance BEFORE INSERT ON input_origins WHEN NEW.kind='task_criteria'
            BEGIN SELECT RAISE(ABORT,'synthetic later provenance failure'); END;").unwrap();
        let before: i64 = store
            .conn()
            .unwrap()
            .query_row("SELECT count(*) FROM input_origins", [], |r| r.get(0))
            .unwrap();
        assert!(
            store
                .add_task_from(&super::tests::task(goal), 0, InputOrigin::Owner)
                .is_err()
        );
        let after: i64 = store
            .conn()
            .unwrap()
            .query_row("SELECT count(*) FROM input_origins", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            before, after,
            "earlier provenance in the same transaction must also roll back"
        );
        assert_eq!(store.tasks().unwrap().len(), 1);
    }

    #[test]
    fn a_run_origin_never_becomes_an_owner_and_disappears_with_its_source() {
        let store = SqliteTeamStore::in_memory().unwrap();
        let goal = store
            .add_goal("Fixture", "", GoalStatus::Active, None)
            .unwrap();
        let task = store.add_task(&task(goal), 0).unwrap();
        store.conn().unwrap().execute_batch("INSERT INTO members(id,name,role,share_percent,slots) VALUES(1,'owner','developer',50,2);
            INSERT INTO wakes(id,member,reason,receipt,at_ms) VALUES(1,1,'assigned','queued',0);
            INSERT INTO runs(id,member,wake,status,queued_at) VALUES(1,1,1,'running',0);").unwrap();
        store
            .post_from(
                task,
                &Post {
                    author: "owner".into(),
                    text: "Member-supplied finding".into(),
                    at: 0,
                    unsupported: false,
                },
                &[],
                InputOrigin::Run(RunId(1)),
            )
            .unwrap();
        let post = store.conn().unwrap().last_insert_rowid();
        store.mark_confidential_input(RunId(1)).unwrap();
        assert_eq!(
            store.input_origin(InputRef::Post(post)).unwrap(),
            InputOrigin::Run(RunId(1))
        );
        assert!(store.run_read_confidential(RunId(1)).unwrap());
        store
            .conn()
            .unwrap()
            .execute_batch(
                "DELETE FROM run_confidential_inputs WHERE run=1; DELETE FROM runs WHERE id=1;",
            )
            .unwrap();
        assert_eq!(
            store.input_origin(InputRef::Post(post)).unwrap(),
            InputOrigin::Unknown
        );
        assert_eq!(
            store.thread(task).unwrap()[0].text,
            "Member-supplied finding"
        );
    }

    #[test]
    fn a_real_legacy_store_gains_no_invented_owner_lineage_on_reopen() {
        struct TempDir(std::path::PathBuf);
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = TempDir(std::env::temp_dir().join(format!(
            "endora-input-lineage-{}-{nanos}",
            std::process::id()
        )));
        std::fs::create_dir(&folder.0).unwrap();
        let path = folder.0.join("hq.db");
        let old =
            SqliteTeamStore::before_the_world(rusqlite::Connection::open(&path).unwrap()).unwrap();
        old.execute_batch("INSERT INTO goals(id,title,reason,status) VALUES(1,'Legacy fixture','','active');
            INSERT INTO tasks(id,goal,title,description,status,priority,depth) VALUES(1,1,'Legacy task','','todo','normal',0);
            INSERT INTO posts(id,task,author,text,at_ms) VALUES(1,1,'owner','Historical display author',0);").unwrap();
        drop(old);
        let store = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
        assert_eq!(
            store.input_origin(InputRef::Post(1)).unwrap(),
            InputOrigin::Unknown
        );
        assert_eq!(
            store
                .input_origin(InputRef::TaskBody(crate::domain::TaskId(1)))
                .unwrap(),
            InputOrigin::Unknown
        );
        let goal = store
            .add_goal_from(
                "Fresh fixture",
                "",
                GoalStatus::Active,
                None,
                InputOrigin::Owner,
            )
            .unwrap();
        drop(store);
        let reopened = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
        assert_eq!(
            reopened.input_origin(InputRef::Goal(goal)).unwrap(),
            InputOrigin::Owner
        );
        assert_eq!(
            reopened.thread(crate::domain::TaskId(1)).unwrap()[0].text,
            "Historical display author"
        );
        assert_eq!(
            reopened.input_origin(InputRef::Post(1)).unwrap(),
            InputOrigin::Unknown
        );
    }
}
