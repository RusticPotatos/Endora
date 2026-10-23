//! Content-free provenance for forward-authored goal/task inputs (ADR 0138).

use super::{TASK_COLUMNS, backend, task_from};
use crate::application::history_ports::{GoalInput, PostInput, TaskInputs};
use crate::application::ports::{Post, Stored};
use crate::domain::history::{InputOrigin, InputRef};
use crate::domain::{Goal, GoalId, GoalStatus, Ref, RunId, TaskId};
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
    let row = conn
        .query_row(
            "SELECT origin,source_run FROM input_origins WHERE kind=?1 AND resource=?2",
            params![kind, id],
            |r| origin_at(r, 0),
        )
        .optional()
        .map_err(backend)?;
    Ok(row.unwrap_or(InputOrigin::Unknown))
}

pub(super) fn origin_at(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<InputOrigin> {
    match (
        row.get::<_, Option<String>>(column)?,
        row.get::<_, Option<i64>>(column + 1)?,
    ) {
        (None, None) => Ok(InputOrigin::Unknown),
        (Some(kind), None) if kind == "owner" => Ok(InputOrigin::Owner),
        (Some(kind), Some(run)) if kind == "run" && run > 0 => Ok(InputOrigin::Run(RunId(run))),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid input provenance",
            )),
        )),
    }
}

pub(super) fn task_inputs(conn: &Connection, task: TaskId) -> Stored<Option<TaskInputs>> {
    // The joined row and task_from's dependent reads share this SELECT snapshot.
    conn.query_row(&format!("SELECT {TASK_COLUMNS},b.origin,b.source_run,c.origin,c.source_run
        FROM tasks LEFT JOIN input_origins b ON b.kind='task_body' AND b.resource=tasks.id
        LEFT JOIN input_origins c ON c.kind='task_criteria' AND c.resource=tasks.id WHERE tasks.id=?1"),
        [task.0], |r| Ok(TaskInputs { task:task_from(conn,r)?,body_origin:origin_at(r,13)?,criteria_origin:origin_at(r,15)? }))
        .optional().map_err(backend)
}

pub(super) fn goal_input(conn: &Connection, goal: GoalId) -> Stored<Option<GoalInput>> {
    conn.query_row(
        "SELECT g.id,g.title,g.reason,g.status,g.parent,o.origin,o.source_run
        FROM goals g LEFT JOIN input_origins o ON o.kind='goal' AND o.resource=g.id WHERE g.id=?1",
        [goal.0],
        |r| {
            let status: String = r.get(3)?;
            Ok(GoalInput {
                goal: Goal {
                    id: GoalId(r.get(0)?),
                    title: r.get(1)?,
                    reason: r.get(2)?,
                    status: GoalStatus::from_name(&status).unwrap_or(GoalStatus::Proposed),
                    parent: r.get::<_, Option<i64>>(4)?.map(GoalId),
                },
                origin: origin_at(r, 5)?,
            })
        },
    )
    .optional()
    .map_err(backend)
}

pub(super) fn latest_authored_posts(
    conn: &Connection,
    task: TaskId,
    most: usize,
) -> Stored<Vec<PostInput>> {
    posts(conn, task, 0, most, true)
}

pub(super) fn authored_posts(
    conn: &Connection,
    task: TaskId,
    after: i64,
    most: usize,
) -> Stored<Vec<PostInput>> {
    posts(conn, task, after, most, false)
}

fn posts(
    conn: &Connection,
    task: TaskId,
    after: i64,
    most: usize,
    newest: bool,
) -> Stored<Vec<PostInput>> {
    if most == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(&format!(
            "SELECT p.id,p.author,p.text,p.at_ms,p.unsupported,p.evidence,o.origin,o.source_run
        FROM posts p LEFT JOIN input_origins o ON o.kind='post' AND o.resource=p.id
        WHERE p.task=?1 AND p.id>?2 ORDER BY p.id {} LIMIT ?3",
            if newest { "DESC" } else { "ASC" }
        ))
        .map_err(backend)?;
    let mut found = stmt
        .query_map(params![task.0, after.max(0), most.min(50) as i64], |r| {
            Ok(PostInput {
                id: r.get(0)?,
                post: Post {
                    author: r.get(1)?,
                    text: r.get(2)?,
                    at: r.get(3)?,
                    unsupported: r.get::<_, i64>(4)? != 0,
                },
                evidence: Ref::read(&r.get::<_, String>(5)?),
                origin: origin_at(r, 6)?,
            })
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)?;
    if newest {
        found.reverse();
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::super::SqliteTeamStore;
    use crate::application::history_ports::HistoryStore;
    use crate::application::ports::{NewTask, Post, TeamStore};
    use crate::domain::history::{InputOrigin, InputRef};
    use crate::domain::{GoalStatus, Priority, RunId};

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new() -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "endora-input-lineage-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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
    fn snapshot_readers_keep_content_sources_and_stable_post_ids_together() {
        let store = SqliteTeamStore::in_memory().unwrap();
        let goal = store
            .add_goal_from(
                "Fixture",
                "Owner-authored goal",
                GoalStatus::Active,
                None,
                InputOrigin::Owner,
            )
            .unwrap();
        let task = store
            .add_task_from(&task(goal), 0, InputOrigin::Owner)
            .unwrap();
        store
            .post(task, "owner", "Unclassified historical display author", 7)
            .unwrap();
        store
            .post_from(
                task,
                &Post {
                    author: "owner".into(),
                    text: "Genuine owner input".into(),
                    at: 7,
                    unsupported: false,
                },
                &[crate::domain::Ref::Task(task)],
                InputOrigin::Owner,
            )
            .unwrap();
        let context = store.task_inputs(task).unwrap().expect("task snapshot");
        assert_eq!(context.task.id, task);
        assert_eq!(context.task.description, "Inspect the fixture");
        assert_eq!(context.body_origin, InputOrigin::Owner);
        assert_eq!(context.criteria_origin, InputOrigin::Owner);
        let goal_context = store.goal_input(goal).unwrap().expect("goal snapshot");
        assert_eq!(goal_context.goal.reason, "Owner-authored goal");
        assert_eq!(goal_context.origin, InputOrigin::Owner);
        let posts = store.authored_posts(task, 0, 50).unwrap();
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0].origin, InputOrigin::Unknown);
        assert_eq!(posts[1].origin, InputOrigin::Owner);
        assert_eq!(posts[1].post.text, "Genuine owner input");
        assert_eq!(posts[1].evidence, [crate::domain::Ref::Task(task)]);
        assert!(posts[0].id < posts[1].id);
        assert_eq!(
            store.authored_posts(task, posts[0].id, 50).unwrap(),
            posts[1..]
        );
        store
            .conn()
            .unwrap()
            .execute(
                "UPDATE tasks SET title='Untracked edit' WHERE id=?1",
                [task.0],
            )
            .unwrap();
        let changed = store.task_inputs(task).unwrap().unwrap();
        assert_eq!(changed.task.title, "Untracked edit");
        assert_eq!(changed.body_origin, InputOrigin::Unknown);
        assert_eq!(changed.criteria_origin, InputOrigin::Owner);
    }

    #[test]
    fn authored_post_pages_are_bounded_without_losing_same_timestamp_history() {
        let store = SqliteTeamStore::in_memory().unwrap();
        let goal = store
            .add_goal("Fixture", "", GoalStatus::Active, None)
            .unwrap();
        let task = store.add_task(&task(goal), 0).unwrap();
        for n in 0..101 {
            store
                .post(task, "owner", &format!("Synthetic post {n}"), 0)
                .unwrap();
        }
        let first = store.authored_posts(task, -1, usize::MAX).unwrap();
        assert_eq!(first.len(), 50);
        let second = store
            .authored_posts(task, first.last().unwrap().id, 50)
            .unwrap();
        assert_eq!(second.len(), 50);
        let last = store
            .authored_posts(task, second.last().unwrap().id, 50)
            .unwrap();
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].post.text, "Synthetic post 100");
        assert!(
            first
                .iter()
                .chain(&second)
                .chain(&last)
                .all(|p| p.origin == InputOrigin::Unknown)
        );
        assert!(store.authored_posts(task, 0, 0).unwrap().is_empty());
    }

    #[test]
    fn latest_authored_posts_select_the_tail_without_reading_the_archive() {
        let store = SqliteTeamStore::in_memory().unwrap();
        let goal = store
            .add_goal("Fixture", "", GoalStatus::Active, None)
            .unwrap();
        let task = store.add_task(&task(goal), 0).unwrap();
        for n in 0..101 {
            store
                .post_from(
                    task,
                    &Post {
                        author: "owner".into(),
                        text: format!("Synthetic post {n}"),
                        at: 0,
                        unsupported: n == 100,
                    },
                    &[],
                    InputOrigin::Owner,
                )
                .unwrap();
        }
        let tail = store.latest_authored_posts(task, 12).unwrap();
        assert_eq!(tail.len(), 12);
        assert_eq!(tail[0].post.text, "Synthetic post 89");
        assert_eq!(tail[11].post.text, "Synthetic post 100");
        assert!(tail[11].post.unsupported);
        assert!(tail.windows(2).all(|pair| pair[0].id < pair[1].id));
        assert!(tail.iter().all(|p| p.origin == InputOrigin::Owner));
        assert_eq!(
            store.latest_authored_posts(task, usize::MAX).unwrap().len(),
            50
        );
        assert!(store.latest_authored_posts(task, 0).unwrap().is_empty());
    }

    #[test]
    fn missing_source_runs_cannot_be_read_as_internal() {
        let store = SqliteTeamStore::in_memory().unwrap();
        assert!(store.run_read_confidential(RunId(999)).unwrap());
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
        let folder = TempDir::new();
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

    #[test]
    fn two_connections_cannot_pair_old_criteria_with_a_new_author() {
        use crate::application::history_ports::TaskInputs;
        use crate::domain::Criterion;
        fn paired(input: &TaskInputs) -> bool {
            let Some(criterion) = input.task.criteria.first() else {
                return false;
            };
            match input.criteria_origin {
                InputOrigin::Owner => criterion.text == "Owner criterion",
                InputOrigin::Run(RunId(1)) => criterion.text == "Member criterion",
                _ => false,
            }
        }
        let folder = TempDir::new();
        let path = folder.0.join("hq.db");
        let reader = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
        // Test-only WAL permits a writer to commit while another connection reads.
        // This does not change the production database's journal policy.
        reader
            .conn()
            .unwrap()
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .unwrap();
        let goal = reader
            .add_goal_from("Fixture", "", GoalStatus::Active, None, InputOrigin::Owner)
            .unwrap();
        let task = reader
            .add_task_from(&task(goal), 0, InputOrigin::Owner)
            .unwrap();
        reader.conn().unwrap().execute_batch("INSERT INTO members(id,name,role,share_percent,slots) VALUES(1,'Member 1','developer',50,2);
            INSERT INTO wakes(id,member,reason,receipt,at_ms) VALUES(1,1,'assigned','queued',0);
            INSERT INTO runs(id,member,wake,status,queued_at) VALUES(1,1,1,'running',0);").unwrap();
        let writer = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
        writer
            .conn()
            .unwrap()
            .execute_batch("PRAGMA busy_timeout=5000;")
            .unwrap();
        reader
            .set_criteria_from(
                task,
                &[Criterion::new("Owner criterion", false)],
                InputOrigin::Owner,
            )
            .unwrap();
        let old = reader.task(task).unwrap().unwrap();
        writer
            .set_criteria_from(
                task,
                &[Criterion::new("Member criterion", false)],
                InputOrigin::Run(RunId(1)),
            )
            .unwrap();
        // Deterministic negative control: two individually valid old APIs can mix
        // snapshots. The same validator must reject that broken pairing.
        let mixed = TaskInputs {
            task: old,
            body_origin: InputOrigin::Owner,
            criteria_origin: reader.input_origin(InputRef::TaskCriteria(task)).unwrap(),
        };
        assert!(!paired(&mixed));
        assert!(paired(&reader.task_inputs(task).unwrap().unwrap()));
        let ready = std::sync::Arc::new(std::sync::Barrier::new(2));
        let writer_ready = ready.clone();
        let work = std::thread::spawn(move || {
            writer_ready.wait();
            for n in 0..400 {
                let (text, origin) = if n % 2 == 0 {
                    ("Owner criterion", InputOrigin::Owner)
                } else {
                    ("Member criterion", InputOrigin::Run(RunId(1)))
                };
                writer
                    .set_criteria_from(task, &[Criterion::new(text, false)], origin)
                    .unwrap();
            }
        });
        ready.wait();
        for _ in 0..800 {
            assert!(paired(&reader.task_inputs(task).unwrap().unwrap()));
        }
        work.join().unwrap();
    }
}
