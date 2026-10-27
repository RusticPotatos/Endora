//! The team's history in HQ's store (ADR 0129). Its tables are made only if missing, so a live
//! store gains them empty and no row of any other table changes; running the schema again
//! changes nothing.

use rusqlite::params;

use super::{SqliteTeamStore, backend};
use crate::application::history_ports::{
    HistoryStore, LegacyConsultation, LegacyRetirementBatch, PostInput,
};
use crate::application::ports::{Post, Stored};
use crate::domain::consult::{Consult, Status as ConsultStatus};
use crate::domain::history::{self, Kept, Transcript};
use crate::domain::record::{Passage, Query};
use crate::domain::{MemberId, RunId, TaskId};

pub(super) const SCHEMA: &str = "
-- Runtime-observed history input taint survives restart and transcript expiry.
CREATE TABLE IF NOT EXISTS run_confidential_inputs (
 run INTEGER PRIMARY KEY REFERENCES runs(id)
) STRICT;
-- One turn's conversation as plain text, scrubbed by the node and capped (ADR 0129). Kept for
-- thirty days and the last five of a member on a task; the retention rule lets the rest go.
CREATE TABLE IF NOT EXISTS transcripts (
    run INTEGER PRIMARY KEY REFERENCES runs(id), member INTEGER NOT NULL REFERENCES members(id),
    task INTEGER REFERENCES tasks(id), runtime TEXT NOT NULL, brain TEXT NOT NULL,
    tier TEXT NOT NULL, body TEXT NOT NULL, est_tokens INTEGER NOT NULL, cut INTEGER NOT NULL,
    at_ms INTEGER NOT NULL, kept_until INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS transcripts_by_member ON transcripts(member, task);
-- A member's question to another, asked directly (ADR 0129): who, in which turn, whom, about
-- which task, where the answer goes, and how it was answered. Never removed.
CREATE TABLE IF NOT EXISTS consults (
    id INTEGER PRIMARY KEY, asker INTEGER NOT NULL REFERENCES members(id),
    asker_run INTEGER NOT NULL REFERENCES runs(id), expert INTEGER NOT NULL REFERENCES members(id),
    topic INTEGER REFERENCES tasks(id), thread INTEGER NOT NULL REFERENCES tasks(id),
    question TEXT NOT NULL, status TEXT NOT NULL, answer_run INTEGER REFERENCES runs(id),
    why TEXT NOT NULL DEFAULT '', asker_woken INTEGER NOT NULL DEFAULT 0, at_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS consults_by_asker ON consults(asker, at_ms);
CREATE INDEX IF NOT EXISTS consults_thread_history ON consults(thread,id);
CREATE INDEX IF NOT EXISTS consults_asker_run ON consults(asker_run,id);
CREATE INDEX IF NOT EXISTS consults_answer_run ON consults(answer_run,id) WHERE answer_run IS NOT NULL;
CREATE INDEX IF NOT EXISTS consults_pending ON consults(expert,id) WHERE status='queued' AND answer_run IS NULL;
CREATE INDEX IF NOT EXISTS consults_unread ON consults(asker,id) WHERE status!='queued' AND asker_woken=0;
CREATE INDEX IF NOT EXISTS consults_undispatched ON consults(id) WHERE status='queued' AND answer_run IS NULL;
CREATE INDEX IF NOT EXISTS consults_unnotified ON consults(id) WHERE status!='queued' AND asker_woken=0;
CREATE INDEX IF NOT EXISTS consults_open ON consults(id) WHERE status='queued';
CREATE TABLE IF NOT EXISTS consult_deliveries (
 consult INTEGER NOT NULL REFERENCES consults(id), stage TEXT NOT NULL CHECK(stage IN ('expert','answer')),
 wake INTEGER NOT NULL REFERENCES wakes(id), run INTEGER REFERENCES runs(id),
 receipt TEXT NOT NULL, refused TEXT, PRIMARY KEY(consult,stage)
) STRICT;
CREATE INDEX IF NOT EXISTS consult_deliveries_run ON consult_deliveries(run,stage,consult);
CREATE TABLE IF NOT EXISTS consult_answer_posts (
 consult INTEGER PRIMARY KEY REFERENCES consults(id), post INTEGER NOT NULL REFERENCES posts(id)
) STRICT;
CREATE INDEX IF NOT EXISTS consults_retry ON consults(asker_run,expert,thread,topic,question,id);
CREATE TABLE IF NOT EXISTS consult_projects (
 consult INTEGER PRIMARY KEY REFERENCES consults(id), project INTEGER REFERENCES projects(id)
) STRICT;
";

/// The full-text index over the conversations, kept apart because it is a virtual table: SQLite
/// built with FTS5 (the bundled one is) makes it, and the triggers keep it as the rows come and
/// go. A store that cannot make it searches the conversations without it.
pub(super) const FULL_TEXT: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS transcripts_fts
    USING fts5(body, content='transcripts', content_rowid='run');
CREATE TRIGGER IF NOT EXISTS transcripts_fts_added AFTER INSERT ON transcripts BEGIN
    INSERT INTO transcripts_fts(rowid, body) VALUES (new.run, new.body);
END;
CREATE TRIGGER IF NOT EXISTS transcripts_fts_gone AFTER DELETE ON transcripts BEGIN
    INSERT INTO transcripts_fts(transcripts_fts, rowid, body) VALUES ('delete', old.run, old.body);
END;
";

const TRANSCRIPT_SELECT: &str = "SELECT run, member, task, runtime, brain, tier, body, est_tokens, \
    cut, at_ms, kept_until FROM transcripts";

const CONSULT_SELECT: &str = "SELECT id,asker,asker_run,expert,topic,thread,question,status,answer_run,why,asker_woken,at_ms FROM consults";
const LEGACY_TASK_PAGE: &str =
    "SELECT c.id,c.asker,c.asker_run,c.expert,c.answer_run,c.topic,c.thread,
    cp.project,c.question,c.status,c.why,c.at_ms,a.post,
    p.id,p.author,p.text,p.at_ms,p.unsupported,p.evidence,o.origin,o.source_run
    FROM consults c LEFT JOIN consult_projects cp ON cp.consult=c.id
    LEFT JOIN consult_answer_posts a ON a.consult=c.id
    LEFT JOIN posts p ON p.id=a.post AND p.task=c.thread
    LEFT JOIN input_origins o ON o.kind='post' AND o.resource=p.id
    WHERE c.thread=?1 AND c.id>?2 ORDER BY c.id LIMIT 51";
pub(super) const CONSULT_PROJECT_FOR_RUN: &str = "(SELECT p.project FROM consults c JOIN consult_projects p ON p.consult=c.id WHERE c.answer_run=?1 ORDER BY c.id LIMIT 1)";

// A task-bearing turn keeps its task responsibility. Canonical expert/result
// associations take precedence even though their wake may also say "consulted".
// The live-run index keeps completed archive rows out of this recovery scan.
const LEGACY_LIVE_RUNS: &str = "SELECT r.id FROM runs r
    JOIN wakes w ON w.id=r.wake
    WHERE r.task IS NULL AND r.status IN ('queued','running','retry_waiting')
    AND (w.reason='consulted'
      OR EXISTS(SELECT 1 FROM consults c WHERE c.answer_run=r.id)
      OR EXISTS(SELECT 1 FROM consult_deliveries d WHERE d.run=r.id AND d.stage='expert'))
    ORDER BY r.id LIMIT 50";

const RETIREMENT_REASON: &str = "Legacy peer help retired; use protocol v2 assistance.";
const RETIRE_LEGACY_REQUESTS: &str = "UPDATE consults SET status='cancelled',
    why=CASE WHEN why='' THEN ?1 ELSE why || char(10) || ?1 END
    WHERE id IN (SELECT id FROM consults WHERE status='queued' ORDER BY id LIMIT 50)";

fn consult_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Consult> {
    let status: String = r.get(7)?;
    let status = ConsultStatus::from_name(&status).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            7,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid consult status",
            )),
        )
    })?;
    Ok(Consult {
        id: r.get(0)?,
        asker: MemberId(r.get(1)?),
        asker_run: RunId(r.get(2)?),
        expert: MemberId(r.get(3)?),
        topic: r.get::<_, Option<i64>>(4)?.map(TaskId),
        thread: TaskId(r.get(5)?),
        question: r.get(6)?,
        status,
        answer_run: r.get::<_, Option<i64>>(8)?.map(RunId),
        why: r.get(9)?,
        asker_woken: r.get(10)?,
        at: r.get(11)?,
    })
}

impl HistoryStore for SqliteTeamStore {
    fn retire_legacy_consultations(&self) -> Stored<LegacyRetirementBatch> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let cancelled = tx
            .execute(RETIRE_LEGACY_REQUESTS, [RETIREMENT_REASON])
            .map_err(backend)?;
        let runs = {
            let mut stmt = tx.prepare(LEGACY_LIVE_RUNS).map_err(backend)?;
            stmt.query_map([], |r| Ok(RunId(r.get(0)?)))
                .map_err(backend)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(backend)?
        };
        tx.commit().map_err(backend)?;
        Ok(LegacyRetirementBatch { cancelled, runs })
    }

    fn legacy_task_consultations(
        &self,
        task: TaskId,
        after: i64,
    ) -> Stored<Vec<LegacyConsultation>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(LEGACY_TASK_PAGE).map_err(backend)?;
        stmt.query_map(params![task.0, after.max(0)], |row| {
            let post_id: Option<i64> = row.get(13)?;
            let answer = post_id
                .map(|id| {
                    Ok::<_, rusqlite::Error>(PostInput {
                        id,
                        post: Post {
                            author: row.get(14)?,
                            text: row.get(15)?,
                            at: row.get(16)?,
                            unsupported: row.get(17)?,
                        },
                        evidence: crate::domain::Ref::read(&row.get::<_, String>(18)?),
                        origin: super::input_lineage::origin_at(row, 19)?,
                    })
                })
                .transpose()?;
            Ok(LegacyConsultation {
                id: row.get(0)?,
                requester: MemberId(row.get(1)?),
                requester_run: RunId(row.get(2)?),
                expert: MemberId(row.get(3)?),
                expert_run: row.get::<_, Option<i64>>(4)?.map(RunId),
                topic: row.get::<_, Option<i64>>(5)?.map(TaskId),
                thread: TaskId(row.get(6)?),
                project: row.get::<_, Option<i64>>(7)?.map(crate::domain::ProjectId),
                question: row.get(8)?,
                outcome: row.get(9)?,
                reason: row.get(10)?,
                at: row.get(11)?,
                answer_post: row.get(12)?,
                answer,
            })
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }
    fn task_inputs(
        &self,
        task: TaskId,
    ) -> Stored<Option<crate::application::history_ports::TaskInputs>> {
        let conn = self.conn()?;
        super::input_lineage::task_inputs(&conn, task)
    }
    fn goal_input(
        &self,
        goal: crate::domain::GoalId,
    ) -> Stored<Option<crate::application::history_ports::GoalInput>> {
        let conn = self.conn()?;
        super::input_lineage::goal_input(&conn, goal)
    }
    fn authored_posts(
        &self,
        task: TaskId,
        after: i64,
        most: usize,
    ) -> Stored<Vec<crate::application::history_ports::PostInput>> {
        let conn = self.conn()?;
        super::input_lineage::authored_posts(&conn, task, after, most)
    }
    fn latest_authored_posts(
        &self,
        task: TaskId,
        most: usize,
    ) -> Stored<Vec<crate::application::history_ports::PostInput>> {
        let conn = self.conn()?;
        super::input_lineage::latest_authored_posts(&conn, task, most)
    }
    fn input_origin(&self, input: history::InputRef) -> Stored<history::InputOrigin> {
        let conn = self.conn()?;
        super::input_lineage::read(&conn, input)
    }
    fn mark_confidential_input(&self, run: RunId) -> Stored<()> {
        self.conn()?
            .execute(
                "INSERT OR IGNORE INTO run_confidential_inputs(run) VALUES(?1)",
                [run.0],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn run_read_confidential(&self, run: RunId) -> Stored<bool> {
        self.conn()?
            .query_row(
                &format!(
                    "SELECT NOT EXISTS(SELECT 1 FROM runs WHERE id=?1) OR {}",
                    super::record::read_secrets("?1")
                ),
                [run.0],
                |r| r.get(0),
            )
            .map_err(backend)
    }

    fn add_transcript(&self, t: &Transcript) -> Stored<()> {
        self.conn()?
            .execute(
                "INSERT INTO transcripts (run, member, task, runtime, brain, tier, body,
                    est_tokens, cut, at_ms, kept_until)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    t.run.0,
                    t.member.0,
                    t.task.map(|x| x.0),
                    t.runtime,
                    t.brain,
                    history::tier_name(t.tier),
                    t.body,
                    t.est_tokens,
                    t.cut,
                    t.at,
                    t.kept_until
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn replace_transcript(&self, t: &Transcript) -> Stored<()> {
        // A delete and an insert, not an update: the search index follows those two.
        self.conn()?
            .execute("DELETE FROM transcripts WHERE run = ?1", [t.run.0])
            .map_err(backend)?;
        self.add_transcript(t)
    }

    fn transcript(&self, run: RunId) -> Stored<Option<Transcript>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{TRANSCRIPT_SELECT} WHERE run = ?1"))
            .map_err(backend)?;
        let mut rows = stmt
            .query_map([run.0], |r| {
                Ok(Transcript {
                    run: RunId(r.get(0)?),
                    member: MemberId(r.get(1)?),
                    task: r.get::<_, Option<i64>>(2)?.map(TaskId),
                    runtime: r.get(3)?,
                    brain: r.get(4)?,
                    tier: history::tier_from_name(&r.get::<_, String>(5)?),
                    body: r.get(6)?,
                    est_tokens: r.get(7)?,
                    cut: r.get(8)?,
                    at: r.get(9)?,
                    kept_until: r.get(10)?,
                })
            })
            .map_err(backend)?;
        rows.next().transpose().map_err(backend)
    }

    fn transcripts_kept(&self) -> Stored<Vec<Kept>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT run, member, task, at_ms, kept_until FROM transcripts ORDER BY at_ms, run",
            )
            .map_err(backend)?;
        stmt.query_map([], |r| {
            Ok(Kept {
                run: RunId(r.get(0)?),
                member: MemberId(r.get(1)?),
                task: r.get::<_, Option<i64>>(2)?.map(TaskId),
                at: r.get(3)?,
                kept_until: r.get(4)?,
            })
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn forget_transcripts(&self, runs: &[RunId]) -> Stored<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(backend)?;
        let mut gone = 0;
        for run in runs {
            gone += tx
                .execute("DELETE FROM transcripts WHERE run = ?1", [run.0])
                .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(gone)
    }

    fn search_record(&self, query: &Query) -> Stored<Vec<Passage>> {
        self.search(query)
    }

    #[cfg(test)]
    fn seed_legacy_consultation(&self, c: &Consult) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO consults (asker, asker_run, expert, topic, thread, question, status,
                answer_run, why, asker_woken, at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                c.asker.0,
                c.asker_run.0,
                c.expert.0,
                c.topic.map(|t| t.0),
                c.thread.0,
                c.question,
                c.status.name(),
                c.answer_run.map(|r| r.0),
                c.why,
                c.asker_woken,
                c.at
            ],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn consults(&self) -> Stored<Vec<Consult>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{CONSULT_SELECT} ORDER BY id"))
            .map_err(backend)?;
        stmt.query_map([], consult_row)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }
}

#[cfg(test)]
mod tests {
    use crate::application::ports::TeamStore;
    use crate::application::run_ports::RunStore;
    use rusqlite::Connection;

    use super::*;
    use crate::domain::{Receipt, WakeReason};

    fn consult_fixture(store: &SqliteTeamStore) {
        store.conn().unwrap().execute_batch("INSERT INTO members(id,name,role,share_percent,slots) VALUES(1,'Member 1','lead',50,2),(2,'Member 2','developer',50,2);
            INSERT INTO goals(id,title,reason,status) VALUES(1,'Fixture','','active');
            INSERT INTO tasks(id,goal,title,description,status,priority,depth) VALUES(1,1,'Fixture','','ready','normal',0);
            INSERT INTO wakes(id,member,reason,receipt,at_ms) VALUES(1,1,'assigned','queued',0);
            INSERT INTO runs(id,member,wake,status,queued_at) VALUES(1,1,1,'running',0),(2,2,1,'running',0),(3,2,1,'running',0);
            INSERT INTO consults(id,asker,asker_run,expert,thread,question,status,answer_run,at_ms) VALUES(1,1,1,2,1,'Fixture?','queued',2,0);").unwrap();
    }

    #[test]
    fn legacy_retirement_cancels_without_answering_or_rewriting_history() {
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        let before = store.legacy_task_consultations(TaskId(1), 0).unwrap();
        let wakes = store.wakes().unwrap();
        let batch = store.retire_legacy_consultations().unwrap();
        assert_eq!(batch.cancelled, 1);
        assert_eq!(batch.runs, vec![RunId(2)]);
        let mut expected = before;
        expected[0].outcome = "cancelled".into();
        expected[0].reason = "Legacy peer help retired; use protocol v2 assistance.".into();
        assert_eq!(
            store.legacy_task_consultations(TaskId(1), 0).unwrap(),
            expected
        );
        assert_eq!(store.wakes().unwrap(), wakes);
        assert_eq!(
            store
                .conn()
                .unwrap()
                .query_row("SELECT count(*) FROM posts", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(store.runs().unwrap().len(), 3);
        assert_eq!(
            store.run(RunId(2)).unwrap().unwrap().status.name(),
            "running"
        );
        let repeated = store.retire_legacy_consultations().unwrap();
        assert_eq!(repeated.cancelled, 0);
        assert_eq!(
            repeated.runs, batch.runs,
            "unsettled work remains discoverable"
        );
        assert_eq!(store.wakes().unwrap(), wakes);
    }

    #[test]
    fn startup_retirement_failure_is_recoverable_without_repeated_accounting() {
        use crate::application::engine::{Engine, Settings};
        use crate::application::ports::Clock;
        use crate::infrastructure::simulated::SimulatedRunner;
        struct Fixed;
        impl Clock for Fixed {
            fn now(&self) -> i64 {
                100
            }
        }
        let folder = std::path::Path::new("target/consult-recovery");
        std::fs::create_dir_all(folder).unwrap();
        let path = folder.join(format!("{}-startup-retirement.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let store = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
            consult_fixture(&store);
            store.conn().unwrap().execute_batch("UPDATE runs SET started_at=1,reserved=100,spent=20 WHERE id=2;
                INSERT INTO ledger(run,member,day,kind,amount,at_ms) VALUES(2,2,0,'reserve',100,1),(2,2,0,'use',20,2);
                CREATE TRIGGER fail_cleanup BEFORE UPDATE ON runs WHEN new.id=2 BEGIN SELECT RAISE(ABORT,'fixture cleanup fault'); END;").unwrap();
            let hq = Engine::new(store, SimulatedRunner::new(8), Fixed, Settings::default());
            assert!(hq.retire_legacy_peer_work().is_err(), "startup must abort");
            assert_eq!(
                hq.store().run(RunId(2)).unwrap().unwrap().status.name(),
                "running"
            );
            assert_eq!(hq.store().ledger_of(RunId(2)).unwrap().len(), 4);
        }
        {
            let store = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
            store
                .conn()
                .unwrap()
                .execute_batch("DROP TRIGGER fail_cleanup;")
                .unwrap();
            let hq = Engine::new(store, SimulatedRunner::new(8), Fixed, Settings::default());
            assert_eq!(hq.retire_legacy_peer_work().unwrap(), 1);
            let ledger = hq.store().ledger_of(RunId(2)).unwrap();
            assert_eq!(ledger.len(), 4, "recovery cannot repeat charge/release");
            assert_eq!(hq.retire_legacy_peer_work().unwrap(), 0);
            assert_eq!(hq.store().ledger_of(RunId(2)).unwrap(), ledger);
            assert_eq!(
                hq.store().legacy_task_consultations(TaskId(1), 0).unwrap()[0].outcome,
                "cancelled"
            );
            assert_eq!(hq.store().wakes().unwrap().len(), 1);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn single_wake_lookup_does_not_decode_unrelated_retained_history() {
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        store.conn().unwrap().execute_batch("INSERT INTO wakes(id,member,reason,receipt,at_ms) VALUES(2,2,'consulted','invalid historical receipt',0);").unwrap();
        let wake = store.wake(crate::domain::WakeId(1)).unwrap().unwrap();
        assert_eq!(wake.reason, WakeReason::Assigned);
        assert_eq!(wake.receipt, Receipt::Queued);
        assert!(store.wake(crate::domain::WakeId(2)).is_err());
        assert!(store.wake(crate::domain::WakeId(999)).unwrap().is_none());
        assert!(store.wakes().is_err(), "whole-history negative control");
    }

    #[test]
    fn single_wake_lookup_work_stays_constant_as_the_archive_grows() {
        use rusqlite::StatementStatus;
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        let conn = store.conn().unwrap();
        let mut steps = Vec::new();
        for size in [1_000, 100_000] {
            conn.execute("DELETE FROM wakes WHERE id>=1000", [])
                .unwrap();
            conn.execute("WITH RECURSIVE n(id) AS (VALUES(1000) UNION ALL SELECT id+1 FROM n WHERE id<999+?1)
                INSERT INTO wakes(id,member,reason,receipt,at_ms) SELECT id,2,'assigned','finished',0 FROM n", [size]).unwrap();
            let mut stmt = conn.prepare(super::super::WAKE_BY_ID).unwrap();
            assert_eq!(
                stmt.query_row([1], super::super::wake_from).unwrap().0.id.0,
                1
            );
            steps.push(stmt.get_status(StatementStatus::VmStep));
        }
        assert_eq!(steps[0], steps[1]);
        assert!(steps[1] < 100);
    }

    #[test]
    fn legacy_retirement_batches_pending_rows_and_keeps_exact_saved_answers() {
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        store
            .conn()
            .unwrap()
            .execute_batch(
                "UPDATE consults SET status='answered',why='Original outcome';
            INSERT INTO posts(id,task,author,text,at_ms) VALUES(1,1,'Member 2','Original answer',0);
            INSERT INTO consult_answer_posts(consult,post) VALUES(1,1);
            WITH RECURSIVE n(id) AS (VALUES(2) UNION ALL SELECT id+1 FROM n WHERE id<122)
            INSERT INTO consults(id,asker,asker_run,expert,thread,question,status,why,at_ms)
            SELECT id,1,1,2,1,'Pending question','queued','Earlier explanation',0 FROM n;",
            )
            .unwrap();
        let saved = store
            .legacy_task_consultations(TaskId(1), 0)
            .unwrap()
            .remove(0);
        for expected in [50, 50, 21, 0] {
            let batch = store.retire_legacy_consultations().unwrap();
            assert_eq!(batch.cancelled, expected);
            assert!(batch.runs.len() <= 50);
        }
        let rows = store.legacy_task_consultations(TaskId(1), 0).unwrap();
        assert_eq!(rows[0], saved);
        assert_eq!(rows[1].outcome, "cancelled");
        assert_eq!(
            rows[1].reason,
            format!("Earlier explanation\n{RETIREMENT_REASON}")
        );
        assert_eq!(
            store.consults().unwrap().len(),
            122,
            "decoder retains cancelled history too"
        );
    }

    #[test]
    fn legacy_retirement_rolls_back_the_whole_batch_on_failure() {
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        store.conn().unwrap().execute_batch("INSERT INTO consults(id,asker,asker_run,expert,thread,question,status,at_ms) VALUES(2,1,1,2,1,'Pending question','queued',0);
            CREATE TRIGGER fail_retirement BEFORE UPDATE ON consults WHEN new.id=2 BEGIN SELECT RAISE(ABORT,'fixture fault'); END;").unwrap();
        let before = store.legacy_task_consultations(TaskId(1), 0).unwrap();
        assert!(store.retire_legacy_consultations().is_err());
        assert_eq!(
            store.legacy_task_consultations(TaskId(1), 0).unwrap(),
            before
        );
    }

    #[test]
    fn legacy_retirement_finds_unsettled_turns_after_database_reopen() {
        let folder = std::path::Path::new("target/consult-recovery");
        std::fs::create_dir_all(folder).unwrap();
        let path = folder.join(format!("{}-retirement.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let store = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
            consult_fixture(&store);
            let batch = store.retire_legacy_consultations().unwrap();
            assert_eq!(batch.cancelled, 1);
            assert_eq!(batch.runs, vec![RunId(2)]);
        }
        {
            let reopened = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
            let batch = reopened.retire_legacy_consultations().unwrap();
            assert_eq!(batch.cancelled, 0);
            assert_eq!(batch.runs, vec![RunId(2)]);
            // Simulate application settlement after recovering from the interruption.
            reopened
                .conn()
                .unwrap()
                .execute("UPDATE runs SET status='stopped' WHERE id=2", [])
                .unwrap();
            assert_eq!(
                reopened.retire_legacy_consultations().unwrap(),
                LegacyRetirementBatch::default()
            );
            assert_eq!(
                reopened.legacy_task_consultations(TaskId(1), 0).unwrap()[0].outcome,
                "cancelled"
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn legacy_retirement_run_selection_is_bounded_and_keeps_unsettled_tail() {
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        store.conn().unwrap().execute_batch("INSERT INTO wakes(id,member,reason,receipt,at_ms) VALUES(2,2,'consulted','queued',0);
            WITH RECURSIVE n(id) AS (VALUES(4) UNION ALL SELECT id+1 FROM n WHERE id<124)
            INSERT INTO runs(id,member,wake,status,queued_at) SELECT id,2,2,'queued',0 FROM n;").unwrap();
        let mut stopped = Vec::new();
        loop {
            let batch = store.retire_legacy_consultations().unwrap();
            assert!(batch.runs.len() <= 50);
            if batch.runs.is_empty() {
                break;
            }
            for run in batch.runs {
                assert!(!stopped.contains(&run));
                store
                    .conn()
                    .unwrap()
                    .execute("UPDATE runs SET status='stopped' WHERE id=?1", [run.0])
                    .unwrap();
                stopped.push(run);
            }
        }
        assert_eq!(stopped.len(), 122);
        assert_eq!(stopped[0], RunId(2));
        assert_eq!(stopped.last(), Some(&RunId(124)));
        assert_eq!(
            store.run(RunId(1)).unwrap().unwrap().status.name(),
            "running"
        );
        assert_eq!(
            store.run(RunId(3)).unwrap().unwrap().status.name(),
            "running"
        );
    }

    #[test]
    fn legacy_retirement_queries_do_not_scan_completed_archives() {
        use rusqlite::StatementStatus;
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        let mut measurements = Vec::new();
        for size in [1_000, 100_000] {
            let mut conn = store.conn().unwrap();
            conn.execute("DELETE FROM runs WHERE id>=1000", []).unwrap();
            conn.execute("DELETE FROM consults WHERE id>=1000", [])
                .unwrap();
            conn.execute("WITH RECURSIVE n(id) AS (VALUES(1000) UNION ALL SELECT id+1 FROM n WHERE id<999+?1)
                INSERT INTO runs(id,member,wake,status,queued_at) SELECT id,2,1,'succeeded',0 FROM n", [size]).unwrap();
            conn.execute("WITH RECURSIVE n(id) AS (VALUES(1000) UNION ALL SELECT id+1 FROM n WHERE id<999+?1)
                INSERT INTO consults(id,asker,asker_run,expert,thread,question,status,at_ms) SELECT id,1,1,2,1,'Saved question','cancelled',0 FROM n", [size]).unwrap();
            let tx = conn.transaction().unwrap();
            let mut retire = tx.prepare(RETIRE_LEGACY_REQUESTS).unwrap();
            assert_eq!(retire.execute([RETIREMENT_REASON]).unwrap(), 1);
            let cancel_steps = retire.get_status(StatementStatus::VmStep);
            let mut live = tx.prepare(LEGACY_LIVE_RUNS).unwrap();
            assert_eq!(
                live.query_map([], |r| r.get::<_, i64>(0))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap(),
                vec![2]
            );
            let live_steps = live.get_status(StatementStatus::VmStep);
            measurements.push((cancel_steps, live_steps));
            drop(live);
            drop(retire);
            tx.rollback().unwrap();
        }
        assert_eq!(measurements[0], measurements[1]);
        eprintln!("legacy retirement VM steps (cancel, live): {measurements:?}");
        assert!(measurements[1].0 < 1_000);
        assert!(measurements[1].1 < 1_000);
        // The same query loses the bound when its live-run indexes are removed.
        let conn = store.conn().unwrap();
        conn.execute_batch("DROP INDEX runs_live_taskless; DROP INDEX runs_live;")
            .unwrap();
        let mut unindexed = conn.prepare(LEGACY_LIVE_RUNS).unwrap();
        assert_eq!(
            unindexed
                .query_map([], |r| r.get::<_, i64>(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap(),
            vec![2]
        );
        assert!(unindexed.get_status(StatementStatus::VmStep) > 100_000);
    }

    #[test]
    fn legacy_history_uses_only_exact_post_and_project_associations() {
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        store.conn().unwrap().execute_batch("INSERT INTO projects(id,goal,title,reason,lead,status,created_by,created_at,updated_at) VALUES(1,1,'Fixture','',1,'active','owner',0,0);
            UPDATE tasks SET project=1 WHERE id=1;
            INSERT INTO tasks(id,goal,title,description,status,priority,depth) VALUES(2,1,'Other task','','ready','normal',0);
            UPDATE consults SET status='answered' WHERE id=1;
            INSERT INTO consults(id,asker,asker_run,expert,thread,question,status,answer_run,at_ms) VALUES(2,1,1,2,1,'No answer link','answered',3,0),(3,1,1,2,1,'Wrong thread link','answered',3,0);
            INSERT INTO posts(id,task,author,text,at_ms) VALUES(1,1,'Member 2','Matching text is not an identity',0),(2,1,'Member 2','The exact linked answer',0),(3,2,'Member 2','Another task answer',0);
            INSERT INTO consult_answer_posts(consult,post) VALUES(1,2),(3,3);
            INSERT INTO consult_projects(consult,project) VALUES(1,1);").unwrap();
        let before = (
            store.consults().unwrap().len(),
            store.wakes().unwrap().len(),
            store.runs().unwrap().len(),
        );
        let rows = store.legacy_task_consultations(TaskId(1), -1).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id, 1);
        assert_eq!(rows[0].question, "Fixture?");
        assert_eq!(rows[0].requester, MemberId(1));
        assert_eq!(rows[0].requester_run, RunId(1));
        assert_eq!(rows[0].expert_run, Some(RunId(2)));
        assert_eq!(rows[0].project, Some(crate::domain::ProjectId(1)));
        assert_eq!(rows[0].answer_post, Some(2));
        assert_eq!(
            rows[0].answer.as_ref().unwrap().post.text,
            "The exact linked answer"
        );
        assert!(rows[1].answer.is_none());
        assert_eq!(
            rows[1].project, None,
            "current task project is not a historical project snapshot"
        );
        assert_eq!(rows[2].answer_post, Some(3));
        assert!(
            rows[2].answer.is_none(),
            "a linked post on another task must remain unavailable"
        );
        assert_eq!(
            store.legacy_task_consultations(TaskId(1), 1).unwrap().len(),
            2
        );
        assert!(
            store
                .legacy_task_consultations(TaskId(2), 0)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            before,
            (
                store.consults().unwrap().len(),
                store.wakes().unwrap().len(),
                store.runs().unwrap().len()
            )
        );
    }

    #[test]
    fn legacy_task_history_work_stays_bounded_as_other_history_grows() {
        use rusqlite::StatementStatus;
        let mut steps = Vec::new();
        for size in [1_000, 100_000] {
            let store = SqliteTeamStore::in_memory().unwrap();
            consult_fixture(&store);
            store.conn().unwrap().execute_batch(&format!("INSERT INTO tasks(id,goal,title,description,status,priority,depth) VALUES(2,1,'Other task','','ready','normal',0);
                WITH RECURSIVE n(x) AS (SELECT 2 UNION ALL SELECT x+1 FROM n WHERE x<{size})
                INSERT INTO consults(id,asker,asker_run,expert,thread,question,status,at_ms)
                SELECT x,1,1,2,2,'Other historical question','from_record',0 FROM n;")).unwrap();
            let measure = || {
                let conn = store.conn().unwrap();
                let mut query = conn.prepare(LEGACY_TASK_PAGE).unwrap();
                let ids = query
                    .query_map([1, 0], |row| row.get::<_, i64>(0))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                assert_eq!(ids, [1]);
                query.get_status(StatementStatus::VmStep)
            };
            steps.push(measure());
            assert_eq!(
                store.legacy_task_consultations(TaskId(1), 0).unwrap().len(),
                1
            );
            if size == 100_000 {
                store
                    .conn()
                    .unwrap()
                    .execute_batch("DROP INDEX consults_thread_history")
                    .unwrap();
                assert!(
                    measure() > 10_000,
                    "negative control must detect the missing task-history index"
                );
            }
        }
        assert_eq!(steps[0], steps[1]);
        assert!(steps[1] < 1_000, "{steps:?}");
    }

    #[test]
    fn the_bundled_sqlite_has_fts5_and_the_history_schema_makes_its_index() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE VIRTUAL TABLE probe USING fts5(body)")
            .expect("FTS5 is built into the bundled SQLite");
        conn.execute(
            "INSERT INTO probe (body) VALUES ('cap the sonarr memory')",
            [],
        )
        .unwrap();
        let found: i64 = conn
            .query_row(
                "SELECT count(*) FROM probe WHERE probe MATCH '\"cap\"* AND \"sonarr\"*'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(found, 1);
        let store = SqliteTeamStore::in_memory().unwrap();
        let made: i64 = store
            .conn()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'transcripts_fts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(made, 1);
    }

    /// Making the tables again, as a second start does, changes nothing and does not fail.
    #[test]
    fn the_schema_runs_again_over_a_store_that_has_it() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        for _ in 0..2 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS runs (id INTEGER PRIMARY KEY) STRICT;
                 CREATE TABLE IF NOT EXISTS members (id INTEGER PRIMARY KEY) STRICT;
                 CREATE TABLE IF NOT EXISTS tasks (id INTEGER PRIMARY KEY) STRICT;",
            )
            .unwrap();
            conn.execute_batch(SCHEMA).unwrap();
            conn.execute_batch(FULL_TEXT).unwrap();
        }
    }
}
