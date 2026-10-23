//! The team's history in HQ's store (ADR 0129). Its tables are made only if missing, so a live
//! store gains them empty and no row of any other table changes; running the schema again
//! changes nothing.

use rusqlite::{OptionalExtension, params};

use super::{SqliteTeamStore, backend};
use crate::application::history_ports::{
    ConsultDelivery, ConsultPostEffects, HistoryStore, LegacyConsultation, LegacyRetirementBatch,
    PostInput,
};
use crate::application::ports::{Post, Stored};
use crate::domain::consult::{Consult, Status as ConsultStatus};
use crate::domain::history::{self, Kept, Transcript};
use crate::domain::record::{Passage, Query};
use crate::domain::{MemberId, Receipt, RunId, TaskId, WakeDecision, WakeReason};

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
-- Cancelled rows retain their original notification history; they must not make
-- the remaining legacy answer scans walk the cancelled archive during cutover.
CREATE INDEX IF NOT EXISTS consults_answer_unread ON consults(asker,id)
 WHERE status IN ('answered','from_record') AND asker_woken=0;
CREATE INDEX IF NOT EXISTS consults_answer_unnotified ON consults(id)
 WHERE status IN ('answered','from_record') AND asker_woken=0;
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
const CONSULT_FOR_RUN: &str = "WHERE answer_run=?1 ORDER BY id LIMIT 1";
const CONSULT_NEXT: &str =
    "WHERE expert=?1 AND status='queued' AND answer_run IS NULL ORDER BY id LIMIT 1";
const CONSULT_UNREAD: &str = "WHERE asker=?1 AND status IN ('answered','from_record') AND asker_woken=0 ORDER BY id LIMIT 50";
pub(super) const CONSULT_TURN_COUNT: &str =
    "SELECT count(*) FROM (SELECT id FROM consults WHERE asker_run=?1 LIMIT 2)";
pub(super) const CONSULT_DAY_COUNT: &str = "SELECT count(*) FROM (SELECT id FROM consults WHERE asker=?1 AND at_ms>=?2 AND at_ms<?3 LIMIT 6)";
const CONSULT_DISPATCHES: &str =
    "WHERE status='queued' AND answer_run IS NULL ORDER BY id LIMIT 50";
const CONSULT_NOTICES: &str = "WHERE status IN ('answered','from_record') AND asker_woken=0 AND NOT EXISTS(
    SELECT 1 FROM consult_deliveries d JOIN runs r ON r.id=d.run WHERE d.consult=consults.id AND d.stage='answer' AND d.receipt='folded' AND r.status IN ('queued','running','retry_waiting')) ORDER BY id LIMIT 50";
const CONSULT_ENDED: &str = "WHERE status='queued' AND answer_run IS NOT NULL AND EXISTS(
    SELECT 1 FROM runs r WHERE r.id=answer_run AND r.status NOT IN ('queued','running','retry_waiting')) ORDER BY id LIMIT 50";

// A task-bearing turn keeps its task responsibility. Canonical expert/result
// associations take precedence even though their wake may also say "consulted".
// The live-run index keeps completed archive rows out of this recovery scan.
const LEGACY_LIVE_RUNS: &str = "SELECT r.id FROM runs r
    JOIN wakes w ON w.id=r.wake
    WHERE r.task IS NULL AND r.status IN ('queued','running','retry_waiting')
    AND NOT EXISTS(SELECT 1 FROM assistance_requests a WHERE a.expert_run=r.id)
    AND NOT EXISTS(SELECT 1 FROM assistance_requests a WHERE a.notice_run=r.id)
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

    fn add_consult(&self, c: &Consult) -> Stored<i64> {
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
            .prepare(
                "SELECT id, asker, asker_run, expert, topic, thread, question, status, answer_run,
                    why, asker_woken, at_ms FROM consults ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    Consult {
                        id: r.get(0)?,
                        asker: MemberId(r.get(1)?),
                        asker_run: RunId(r.get(2)?),
                        expert: MemberId(r.get(3)?),
                        topic: r.get::<_, Option<i64>>(4)?.map(TaskId),
                        thread: TaskId(r.get(5)?),
                        question: r.get(6)?,
                        status: ConsultStatus::Queued,
                        answer_run: r.get::<_, Option<i64>>(8)?.map(RunId),
                        why: r.get(9)?,
                        asker_woken: r.get(10)?,
                        at: r.get(11)?,
                    },
                    r.get::<_, String>(7)?,
                ))
            })
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|(mut c, status)| {
                c.status = ConsultStatus::from_name(&status)
                    .ok_or_else(|| super::corrupt("a consult's status"))?;
                Ok(c)
            })
            .collect()
    }

    fn accept_consult(
        &self,
        c: &Consult,
        day: i64,
        day_start: i64,
    ) -> Stored<Result<(Consult, bool), crate::domain::history::HistoryRefusal>> {
        use crate::domain::history::HistoryRefusal;
        if c.status != ConsultStatus::Queued || c.answer_run.is_some() || c.asker_woken {
            return Err(super::corrupt(
                "consult admission requires a new queued intent",
            ));
        }
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let prior=tx.query_row(&format!("{CONSULT_SELECT} WHERE asker_run=?1 AND expert=?2 AND thread=?3 AND topic IS ?4 AND question=?5 AND asker=?6 ORDER BY id LIMIT 1"),
            params![c.asker_run.0,c.expert.0,c.thread.0,c.topic.map(|id|id.0),c.question,c.asker.0],consult_row).optional().map_err(backend)?;
        if let Some(prior) = prior {
            return Ok(Ok((prior, false)));
        }
        let (turn, today) =
            super::assistance::consultation_usage(&tx, c.asker_run, c.asker, day, day_start)?;
        if turn >= crate::domain::consult::PER_TURN {
            return Ok(Err(HistoryRefusal::TooManyThisTurn));
        }
        if today >= crate::domain::consult::PER_DAY {
            return Ok(Err(HistoryRefusal::TooManyToday));
        }
        tx.execute("INSERT INTO consults(asker,asker_run,expert,topic,thread,question,status,at_ms) VALUES(?1,?2,?3,?4,?5,?6,'queued',?7)",
            params![c.asker.0,c.asker_run.0,c.expert.0,c.topic.map(|id|id.0),c.thread.0,c.question,c.at]).map_err(backend)?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO consult_projects(consult,project) SELECT ?1,project FROM tasks WHERE id=?2",params![id,c.thread.0]).map_err(backend)?;
        let mut accepted = c.clone();
        accepted.id = id;
        tx.commit().map_err(backend)?;
        Ok(Ok((accepted, true)))
    }

    fn consult_counts(
        &self,
        run: RunId,
        asker: MemberId,
        start: i64,
        end: i64,
    ) -> Stored<(usize, usize)> {
        let conn = self.conn()?;
        let turn: u8 = conn
            .query_row(CONSULT_TURN_COUNT, [run.0], |row| row.get(0))
            .map_err(backend)?;
        let day: u8 = conn
            .query_row(CONSULT_DAY_COUNT, params![asker.0, start, end], |row| {
                row.get(0)
            })
            .map_err(backend)?;
        Ok((usize::from(turn), usize::from(day)))
    }

    fn consult_for_run(&self, run: RunId) -> Stored<Option<Consult>> {
        self.conn()?
            .query_row(
                &format!("{CONSULT_SELECT} {CONSULT_FOR_RUN}"),
                [run.0],
                consult_row,
            )
            .optional()
            .map_err(backend)
    }

    fn claim_consult(&self, run: RunId, expert: MemberId) -> Stored<Option<Consult>> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        if let Some(own) = tx
            .query_row(
                &format!("{CONSULT_SELECT} {CONSULT_FOR_RUN}"),
                [run.0],
                consult_row,
            )
            .optional()
            .map_err(backend)?
        {
            return Ok(Some(own));
        }
        let Some(mut next) = tx
            .query_row(
                &format!("{CONSULT_SELECT} {CONSULT_NEXT}"),
                [expert.0],
                consult_row,
            )
            .optional()
            .map_err(backend)?
        else {
            return Ok(None);
        };
        tx.execute("UPDATE consults SET answer_run=?2 WHERE id=?1 AND status='queued' AND answer_run IS NULL",params![next.id,run.0]).map_err(backend)?;
        next.answer_run = Some(run);
        tx.commit().map_err(backend)?;
        Ok(Some(next))
    }

    fn unread_consults(&self, asker: MemberId) -> Stored<Vec<Consult>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{CONSULT_SELECT} {CONSULT_UNREAD}"))
            .map_err(backend)?;
        stmt.query_map([asker.0], consult_row)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn save_consult(&self, c: &Consult) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE consults SET status = ?2, answer_run = ?3, why = ?4, asker_woken = ?5
                 WHERE id = ?1",
                params![
                    c.id,
                    c.status.name(),
                    c.answer_run.map(|r| r.0),
                    c.why,
                    c.asker_woken
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn finish_consult_with_effects(
        &self,
        c: &Consult,
        post: &Post,
        effects: &ConsultPostEffects,
    ) -> Stored<bool> {
        if c.status == ConsultStatus::Queued {
            return Err(super::corrupt("a consult completion must be terminal"));
        }
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let changed = tx.execute("UPDATE consults SET status=?2,why=?3 WHERE id=?1 AND status='queued' AND answer_run IS ?4 AND thread=?5",
            params![c.id,c.status.name(),c.why,c.answer_run.map(|run|run.0),c.thread.0]).map_err(backend)?;
        if changed == 0 {
            return Ok(false);
        }
        tx.execute("INSERT INTO posts(task,author,text,at_ms,evidence,unsupported) VALUES(?1,?2,?3,?4,'',?5)",
            params![c.thread.0,post.author,post.text,post.at,post.unsupported]).map_err(backend)?;
        let post_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO consult_answer_posts(consult,post) VALUES(?1,?2)",
            params![c.id, post_id],
        )
        .map_err(backend)?;
        let place: i64 = tx
            .query_row(
                "SELECT count(*)-1 FROM posts WHERE task=?1",
                [c.thread.0],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let mut preview: String = post.text.chars().take(80).collect();
        let cut = post.text.chars().count() > 80;
        if cut {
            preview.push('…');
        }
        let data = serde_json::json!({"task":c.thread.0,"author":post.author,"preview":preview,"cut":cut,"post":place,"unsupported":post.unsupported});
        if let Some(member) = effects.member {
            tx.execute(
                "UPDATE runs SET posted=1 WHERE member=?1 AND task=?2 AND status='running'",
                params![member.0, c.thread.0],
            )
            .map_err(backend)?;
            for story in &effects.stories {
                tx.execute(
                    "UPDATE stories SET last_used_at=MAX(last_used_at,?2) WHERE id=?1",
                    params![story, post.at],
                )
                .map_err(backend)?;
            }
            for section in &effects.sections {
                tx.execute(
                    "UPDATE world_sections SET last_used_at=MAX(last_used_at,?2) WHERE id=?1",
                    params![section, post.at],
                )
                .map_err(backend)?;
            }
        }
        tx.execute(
            "INSERT INTO events(at_ms,kind,data) VALUES(?1,?2,?3)",
            params![
                post.at,
                crate::domain::EventKind::ThreadPosted.name(),
                data.to_string()
            ],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(true)
    }

    fn pending_consult_dispatches(&self) -> Stored<Vec<Consult>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{CONSULT_SELECT} {CONSULT_DISPATCHES}"))
            .map_err(backend)?;
        stmt.query_map([], consult_row)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn consult_answers_for_run(&self, run: RunId) -> Stored<Vec<Post>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT p.author,p.text,p.at_ms,p.unsupported FROM consult_deliveries d
            JOIN consult_answer_posts a ON a.consult=d.consult JOIN posts p ON p.id=a.post
            WHERE d.run=?1 AND d.stage='answer' AND d.receipt='queued' ORDER BY d.consult LIMIT 50",
            )
            .map_err(backend)?;
        stmt.query_map([run.0], |row| {
            Ok(Post {
                author: row.get(0)?,
                text: row.get(1)?,
                at: row.get(2)?,
                unsupported: row.get(3)?,
            })
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn pending_consult_notices(&self) -> Stored<Vec<Consult>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{CONSULT_SELECT} {CONSULT_NOTICES}"))
            .map_err(backend)?;
        stmt.query_map([], consult_row)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn ended_consults(&self) -> Stored<Vec<Consult>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{CONSULT_SELECT} {CONSULT_ENDED}"))
            .map_err(backend)?;
        stmt.query_map([], consult_row)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn deliver_consult(
        &self,
        id: i64,
        stage: ConsultDelivery,
        decision: WakeDecision,
        by_owner: bool,
        now: i64,
    ) -> Stored<Receipt> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let found = tx
            .query_row(&format!("{CONSULT_SELECT} WHERE id=?1"), [id], consult_row)
            .map_err(backend)?;
        if found.status == ConsultStatus::Cancelled {
            return Ok(Receipt::Finished);
        }
        let prior: Option<(String, Option<String>, Option<i64>)> = tx
            .query_row(
                "SELECT receipt,refused,run FROM consult_deliveries WHERE consult=?1 AND stage=?2",
                params![id, stage.name()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some((receipt, why, run)) = prior {
            let receipt = super::receipt_from(&receipt, why.as_deref())?;
            if stage == ConsultDelivery::Expert || receipt != Receipt::Folded {
                return Ok(receipt);
            }
            let live:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1 AND status IN ('queued','running','retry_waiting'))",[run],|row|row.get(0)).map_err(backend)?;
            if live {
                return Ok(receipt);
            }
        }
        if (stage == ConsultDelivery::Expert && found.status != ConsultStatus::Queued)
            || (stage == ConsultDelivery::Answer && found.asker_woken)
        {
            return Ok(Receipt::Finished);
        }
        if stage == ConsultDelivery::Expert && found.answer_run.is_some() {
            return Ok(Receipt::Queued);
        }
        if stage == ConsultDelivery::Answer && found.status == ConsultStatus::Queued {
            return Err(super::corrupt("an unanswered consult cannot notify"));
        }
        // Adopt an unclaimed pre-upgrade queued expert turn using the old oldest-question
        // rule. Never take a run already associated with either assistance origin.
        if let (ConsultDelivery::Expert, WakeDecision::Fold(run)) = (stage, decision) {
            let wake:Option<i64>=tx.query_row("SELECT r.wake FROM runs r JOIN wakes w ON w.id=r.wake
                WHERE r.id=?1 AND r.member=?2 AND r.task IS NULL AND r.started_at IS NULL
                AND r.status IN ('queued','retry_waiting') AND w.reason='consulted'
                AND NOT EXISTS(SELECT 1 FROM consults c WHERE c.answer_run=r.id)
                AND NOT EXISTS(SELECT 1 FROM assistance_requests a WHERE a.expert_run=r.id)
                AND NOT EXISTS(SELECT 1 FROM consults c WHERE c.expert=?2 AND c.status='queued' AND c.answer_run IS NULL AND c.id<?3)",
                params![run.0,found.expert.0,id],|row|row.get(0)).optional().map_err(backend)?;
            if let Some(wake) = wake {
                tx.execute(
                    "UPDATE consults SET answer_run=?2 WHERE id=?1",
                    params![id, run.0],
                )
                .map_err(backend)?;
                tx.execute("INSERT INTO consult_deliveries(consult,stage,wake,run,receipt) VALUES(?1,'expert',?2,?3,'queued')",params![id,wake,run.0]).map_err(backend)?;
                tx.commit().map_err(backend)?;
                return Ok(Receipt::Queued);
            }
        }
        let (member, task, reason) = match stage {
            ConsultDelivery::Expert => (found.expert, None, WakeReason::Consulted),
            ConsultDelivery::Answer => (
                found.asker,
                Some(found.thread),
                WakeReason::QuestionAnswered,
            ),
        };
        let (receipt, run, why) = match decision {
            WakeDecision::Queue => (Receipt::Queued, None, None),
            WakeDecision::Fold(run) => (Receipt::Folded, Some(run), None),
            WakeDecision::Hold => (Receipt::Held, None, None),
            WakeDecision::Refuse(why) => (Receipt::Refused(why), None, Some(why.name())),
        };
        let (wake, run) = if receipt == Receipt::Queued {
            let (wake, run) =
                super::assistance::queue_turn(&tx, member, task, reason, by_owner, now)?;
            (wake.0, Some(run))
        } else {
            tx.execute("INSERT INTO wakes(member,task,reason,receipt,refused,run,at_ms,by_owner) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![member.0,task.map(|id|id.0),reason.name(),receipt.name(),why,run.map(|id|id.0),now,by_owner]).map_err(backend)?;
            let wake = tx.last_insert_rowid();
            if let (true, Some(run)) = (by_owner, run) {
                tx.execute(
                    "UPDATE runs SET asked_by_owner=1 WHERE id=?1 AND started_at IS NULL",
                    [run.0],
                )
                .map_err(backend)?;
            }
            (wake, run)
        };
        if stage == ConsultDelivery::Expert && receipt == Receipt::Queued {
            tx.execute(
                "UPDATE consults SET answer_run=?2 WHERE id=?1",
                params![id, run.map(|id| id.0)],
            )
            .map_err(backend)?;
        }
        if stage == ConsultDelivery::Answer {
            tx.execute(
                "UPDATE consults SET asker_woken=?2 WHERE id=?1",
                params![id, receipt != Receipt::Folded],
            )
            .map_err(backend)?;
        }
        tx.execute("INSERT INTO consult_deliveries(consult,stage,wake,run,receipt,refused) VALUES(?1,?2,?3,?4,?5,?6)
            ON CONFLICT(consult,stage) DO UPDATE SET wake=excluded.wake,run=excluded.run,receipt=excluded.receipt,refused=excluded.refused",
            params![id,stage.name(),wake,run.map(|id|id.0),receipt.name(),why]).map_err(backend)?;
        let kind = match receipt {
            Receipt::Queued => crate::domain::EventKind::WakeQueued,
            Receipt::Folded => crate::domain::EventKind::WakeFolded,
            Receipt::Held => crate::domain::EventKind::WakeHeld,
            _ => crate::domain::EventKind::WakeRefused,
        };
        let mut data =
            serde_json::json!({"member":member.0,"task":task.map(|id|id.0),"reason":reason.name()});
        if let Some(why) = why {
            data["why"] = serde_json::json!(why);
        }
        tx.execute(
            "INSERT INTO events(at_ms,kind,data) VALUES(?1,?2,?3)",
            params![now, kind.name(), data.to_string()],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use crate::application::ports::TeamStore;
    use rusqlite::Connection;

    use super::*;

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
        assert!(store.pending_consult_dispatches().unwrap().is_empty());
        assert!(store.pending_consult_notices().unwrap().is_empty());
        assert!(store.unread_consults(MemberId(1)).unwrap().is_empty());
        for stage in [ConsultDelivery::Expert, ConsultDelivery::Answer] {
            assert_eq!(
                store
                    .deliver_consult(1, stage, WakeDecision::Queue, false, 0)
                    .unwrap(),
                Receipt::Finished
            );
        }
        assert_eq!(store.wakes().unwrap(), wakes);
    }

    #[test]
    fn legacy_retirement_preserves_task_work_and_canonical_expert_and_result_turns() {
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        store.conn().unwrap().execute_batch("INSERT INTO wakes(id,member,reason,receipt,at_ms) VALUES(2,2,'consulted','queued',0);
            INSERT INTO runs(id,member,wake,task,status,queued_at) VALUES
            (4,2,2,NULL,'queued',0),(5,2,1,NULL,'retry_waiting',0),
            (6,2,2,1,'running',0),(7,2,2,NULL,'running',0),
            (8,1,2,NULL,'queued',0),(9,2,2,NULL,'succeeded',0);
            INSERT INTO consult_deliveries(consult,stage,wake,run,receipt) VALUES(1,'expert',1,5,'queued');
            INSERT INTO projects(id,goal,title,reason,lead,status,created_by,created_at,updated_at) VALUES(1,1,'Fixture','',1,'active','owner',0,0);
            INSERT INTO conversations(id,project,member1,member2,at_ms) VALUES(1,1,1,2,0);
            INSERT INTO assistance_requests(conversation,project,requester,requester_run,expert,client_key,objective,evidence,accepted_day,accepted_at,deadline_at,status,expert_run,notice_run)
            VALUES(1,1,1,1,2,'fixture-key','Fixture','',0,0,100,'answered',7,8);
            INSERT INTO run_tokens(run,token) VALUES(2,'fixture-token');
            INSERT INTO ledger(run,member,day,kind,amount,at_ms) VALUES(2,2,0,'reserve',100,0);").unwrap();
        let batch = store.retire_legacy_consultations().unwrap();
        assert_eq!(batch.cancelled, 1);
        assert_eq!(batch.runs, vec![RunId(2), RunId(4), RunId(5)]);
        let conn = store.conn().unwrap();
        for table in [
            "run_tokens",
            "ledger",
            "assistance_requests",
            "consult_deliveries",
        ] {
            assert_eq!(
                conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            conn.query_row("SELECT status FROM assistance_requests", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "answered"
        );
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
            let mut unread = tx
                .prepare(&format!("{CONSULT_SELECT} {CONSULT_UNREAD}"))
                .unwrap();
            assert_eq!(unread.query_map([1], consult_row).unwrap().count(), 0);
            let unread_steps = unread.get_status(StatementStatus::VmStep);
            let mut notices = tx
                .prepare(&format!("{CONSULT_SELECT} {CONSULT_NOTICES}"))
                .unwrap();
            assert_eq!(notices.query_map([], consult_row).unwrap().count(), 0);
            let notice_steps = notices.get_status(StatementStatus::VmStep);
            measurements.push((cancel_steps, live_steps, unread_steps, notice_steps));
            drop(notices);
            drop(unread);
            drop(live);
            drop(retire);
            tx.rollback().unwrap();
        }
        assert_eq!(measurements[0], measurements[1]);
        eprintln!("legacy retirement VM steps (cancel, live, unread, notices): {measurements:?}");
        assert!(measurements[1].0 < 1_000);
        assert!(measurements[1].1 < 1_000);
        assert!(measurements[1].2 < 1_000);
        assert!(measurements[1].3 < 1_000);
        // The same query loses the bound when its live-run indexes are removed.
        let conn = store.conn().unwrap();
        conn.execute_batch("DROP INDEX runs_live_taskless; DROP INDEX assistance_live_member_run;")
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
    fn knowledge_effect_failure_rolls_back_the_answer_feed_and_usage_markers() {
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        store.conn().unwrap().execute_batch("INSERT INTO stories(id,scope,handle,instead,stage,author,written_at,last_used_at) VALUES(1,'house','fixture-story','Read the check','observation','owner',0,0);
            INSERT INTO world_sections(id,page,anchor,heading,headline,stage,written_by,revision,updated_at,last_used_at) VALUES(1,'house','fixture','Fixture','Synthetic check','observation','owner',1,0,0);
            CREATE TRIGGER fail_consult_knowledge BEFORE UPDATE ON world_sections BEGIN SELECT RAISE(ABORT,'synthetic knowledge failure'); END;").unwrap();
        let mut found = store.consult_for_run(RunId(2)).unwrap().unwrap();
        found.status = ConsultStatus::Answered;
        let post = Post {
            author: "Member 2".into(),
            text: "See fixture-story and world:house#fixture.".into(),
            at: 10,
            unsupported: false,
        };
        let effects = ConsultPostEffects {
            member: Some(MemberId(2)),
            stories: vec![1],
            sections: vec![1],
        };
        assert!(
            store
                .finish_consult_with_effects(&found, &post, &effects)
                .unwrap_err()
                .to_string()
                .contains("synthetic knowledge failure")
        );
        assert_eq!(
            store.consult_for_run(RunId(2)).unwrap().unwrap().status,
            ConsultStatus::Queued
        );
        let unchanged:bool=store.conn().unwrap().query_row("SELECT NOT EXISTS(SELECT 1 FROM posts) AND NOT EXISTS(SELECT 1 FROM events WHERE kind='thread.posted') AND (SELECT last_used_at FROM stories WHERE id=1)=0",[],|row|row.get(0)).unwrap();
        assert!(unchanged);
        store
            .conn()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_consult_knowledge")
            .unwrap();
        assert!(
            store
                .finish_consult_with_effects(&found, &post, &effects)
                .unwrap()
        );
        assert!(
            !store
                .finish_consult_with_effects(&found, &post, &effects)
                .unwrap()
        );
        let markers:bool=store.conn().unwrap().query_row("SELECT (SELECT last_used_at FROM stories WHERE id=1)=10 AND (SELECT last_used_at FROM world_sections WHERE id=1)=10 AND (SELECT count(*) FROM events WHERE kind='thread.posted')=1",[],|row|row.get(0)).unwrap();
        assert!(markers);
    }

    #[test]
    fn a_consult_keeps_its_admitted_project_through_all_ledger_rows() {
        use crate::application::conversation_ports::ConversationStore;
        use crate::application::ports::TeamStore;
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        let mut intent = store.consult_for_run(RunId(2)).unwrap().unwrap();
        intent.id = 0;
        intent.answer_run = None;
        store.conn().unwrap().execute_batch("DELETE FROM consults;
            INSERT INTO projects(id,goal,title,reason,lead,status,created_by,created_at,updated_at) VALUES(1,1,'Fixture','',1,'active','owner',0,0),(2,1,'Other fixture','',1,'active','owner',0,0);
            UPDATE tasks SET project=1 WHERE id=1;").unwrap();
        let (accepted, fresh) = store.accept_consult(&intent, 0, 0).unwrap().unwrap();
        assert!(fresh);
        store
            .conn()
            .unwrap()
            .execute("UPDATE tasks SET project=2 WHERE id=1", [])
            .unwrap();
        store
            .deliver_consult(
                accepted.id,
                ConsultDelivery::Expert,
                WakeDecision::Queue,
                false,
                0,
            )
            .unwrap();
        let run = RunId(4);
        assert_eq!(
            store.assistance_charging_project(run).unwrap(),
            Some(crate::domain::ProjectId(1))
        );
        store.reserve(run, MemberId(2), 0, 100, 0).unwrap();
        store.record_use(run, MemberId(2), 0, 20, 1).unwrap();
        store.settle(run, MemberId(2), 0, 2).unwrap();
        store
            .record_use(run, MemberId(2), 1, 5, 86_400_001)
            .unwrap();
        let conn = store.conn().unwrap();
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM ledger WHERE run=?1", [run.0], |row| {
                row.get(0)
            })
            .unwrap();
        let wrong: i64 = conn
            .query_row(
                "SELECT count(*) FROM ledger WHERE run=?1 AND project IS NOT 1",
                [run.0],
                |row| row.get(0),
            )
            .unwrap();
        assert!(rows >= 5);
        assert_eq!(
            wrong, 0,
            "reserve, use, settle and late charges retain the admitted project"
        );
    }

    #[test]
    fn routine_consult_queries_stay_bounded_as_retained_history_grows() {
        use rusqlite::StatementStatus;
        let mut observed = Vec::new();
        for size in [1_000, 100_000] {
            let store = SqliteTeamStore::in_memory().unwrap();
            consult_fixture(&store);
            store.conn().unwrap().execute_batch(&format!("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<{size})
                INSERT INTO consults(asker,asker_run,expert,thread,question,status,asker_woken,at_ms)
                SELECT 1,2,2,1,'Retained fixture','answered',1,-1 FROM n")).unwrap();
            let queries = [
                format!("{CONSULT_SELECT} {CONSULT_FOR_RUN}"),
                format!("{CONSULT_SELECT} {CONSULT_NEXT}"),
                format!("{CONSULT_SELECT} {CONSULT_UNREAD}"),
                CONSULT_TURN_COUNT.to_owned(),
                CONSULT_DAY_COUNT.to_owned(),
                format!("{CONSULT_SELECT} {CONSULT_DISPATCHES}"),
                format!("{CONSULT_SELECT} {CONSULT_NOTICES}"),
                format!("{CONSULT_SELECT} {CONSULT_ENDED}"),
            ];
            let measure = || {
                let conn = store.conn().unwrap();
                let mut costs = Vec::new();
                for (index, sql) in queries.iter().enumerate() {
                    let mut stmt = conn.prepare(sql).unwrap();
                    let parameters: Vec<i64> = match index {
                        0 => vec![3],
                        1 => vec![2],
                        4 => vec![1, 0, 86_400_000],
                        5.. => vec![],
                        _ => vec![1],
                    };
                    let mut rows = stmt.query(rusqlite::params_from_iter(parameters)).unwrap();
                    while rows.next().unwrap().is_some() {}
                    drop(rows);
                    costs.push(stmt.get_status(StatementStatus::VmStep));
                }
                costs
            };
            let costs = measure();
            assert!(
                costs.iter().all(|cost| *cost < 1_000),
                "unbounded query costs at {size}: {costs:?}"
            );
            observed.push(costs);
            assert_eq!(
                store
                    .consult_counts(RunId(1), MemberId(1), 0, 86_400_000)
                    .unwrap(),
                (1, 1)
            );
            assert_eq!(store.consult_for_run(RunId(2)).unwrap().unwrap().id, 1);
            assert!(store.unread_consults(MemberId(1)).unwrap().is_empty());
            assert!(
                store
                    .claim_consult(RunId(3), MemberId(2))
                    .unwrap()
                    .is_none()
            );
            if size == 100_000 {
                store.conn().unwrap().execute_batch("DROP INDEX consults_answer_run; DROP INDEX consults_pending; DROP INDEX consults_unread; DROP INDEX consults_asker_run;").unwrap();
                assert!(
                    measure().iter().any(|cost| *cost > 100_000),
                    "missing-index negative control must fail the bound"
                );
            }
        }
        assert_eq!(observed[0], observed[1]);
    }

    #[test]
    fn completion_rolls_back_on_post_failure_and_a_terminal_result_cannot_be_overwritten() {
        for status in [ConsultStatus::Answered, ConsultStatus::FromRecord] {
            let store = SqliteTeamStore::in_memory().unwrap();
            consult_fixture(&store);
            let mut found = store.consult_for_run(RunId(2)).unwrap().unwrap();
            found.status = status;
            found.why = if status == ConsultStatus::FromRecord {
                "did not answer".into()
            } else {
                String::new()
            };
            let post = Post {
                author: "Member 2".into(),
                text: "Synthetic answer".into(),
                at: 0,
                unsupported: false,
            };
            store.conn().unwrap().execute_batch("CREATE TRIGGER fail_consult_post BEFORE INSERT ON posts BEGIN SELECT RAISE(ABORT,'synthetic post failure'); END;").unwrap();
            assert!(
                store
                    .finish_consult(&found, &post)
                    .unwrap_err()
                    .to_string()
                    .contains("synthetic post failure")
            );
            assert_eq!(
                store.consult_for_run(RunId(2)).unwrap().unwrap().status,
                ConsultStatus::Queued
            );
            store
                .conn()
                .unwrap()
                .execute_batch("DROP TRIGGER fail_consult_post")
                .unwrap();
            assert!(store.finish_consult(&found, &post).unwrap());
            assert!(!store.finish_consult(&found, &post).unwrap());
            found.status = if status == ConsultStatus::Answered {
                ConsultStatus::FromRecord
            } else {
                ConsultStatus::Answered
            };
            assert!(!store.finish_consult(&found, &post).unwrap());
            assert_eq!(
                store.consult_for_run(RunId(2)).unwrap().unwrap().status,
                status
            );
            let count: i64 = store
                .conn()
                .unwrap()
                .query_row("SELECT count(*) FROM posts", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }
    }

    #[test]
    fn claiming_a_consult_reuses_its_association_and_skips_a_taken_question() {
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        store
            .conn()
            .unwrap()
            .execute("UPDATE consults SET answer_run=NULL", [])
            .unwrap();
        let first = store.claim_consult(RunId(2), MemberId(2)).unwrap().unwrap();
        assert_eq!(first.answer_run, Some(RunId(2)));
        assert_eq!(
            store.claim_consult(RunId(2), MemberId(2)).unwrap(),
            Some(first)
        );
        assert!(
            store
                .claim_consult(RunId(3), MemberId(2))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn consultation_dispatch_replay_and_folded_notices_keep_one_delivery() {
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        store
            .conn()
            .unwrap()
            .execute_batch(
                "UPDATE consults SET answer_run=NULL; UPDATE runs SET task=1 WHERE id=1;",
            )
            .unwrap();
        for _ in 0..2 {
            assert_eq!(
                store
                    .deliver_consult(1, ConsultDelivery::Expert, WakeDecision::Queue, false, 0)
                    .unwrap(),
                Receipt::Queued
            );
        }
        let mut result = store.consult_for_run(RunId(4)).unwrap().unwrap();
        result.status = ConsultStatus::Answered;
        store
            .finish_consult(
                &result,
                &Post {
                    author: "Member 2".into(),
                    text: "Synthetic answer".into(),
                    at: 0,
                    unsupported: false,
                },
            )
            .unwrap();
        for _ in 0..2 {
            assert_eq!(
                store
                    .deliver_consult(
                        1,
                        ConsultDelivery::Answer,
                        WakeDecision::Fold(RunId(1)),
                        false,
                        0
                    )
                    .unwrap(),
                Receipt::Folded
            );
        }
        assert!(store.pending_consult_notices().unwrap().is_empty());
        let count: i64 = store
            .conn()
            .unwrap()
            .query_row("SELECT count(*) FROM wakes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            count, 3,
            "one dispatch and one fold, without audit spam on retries"
        );
        store
            .conn()
            .unwrap()
            .execute("UPDATE runs SET status='succeeded' WHERE id=1", [])
            .unwrap();
        assert_eq!(store.pending_consult_notices().unwrap().len(), 1);
        for _ in 0..2 {
            assert_eq!(
                store
                    .deliver_consult(1, ConsultDelivery::Answer, WakeDecision::Queue, false, 0)
                    .unwrap(),
                Receipt::Queued
            );
        }
        assert!(store.pending_consult_notices().unwrap().is_empty());
        assert_eq!(
            store.consult_answers_for_run(RunId(5)).unwrap()[0].text,
            "Synthetic answer"
        );
        assert!(store.consult_answers_for_run(RunId(3)).unwrap().is_empty());
        let count: i64 = store
            .conn()
            .unwrap()
            .query_row("SELECT count(*) FROM wakes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 4);
    }

    #[test]
    fn recovery_adopts_an_unclaimed_legacy_expert_turn_instead_of_queuing_another() {
        let store = SqliteTeamStore::in_memory().unwrap();
        consult_fixture(&store);
        store
            .conn()
            .unwrap()
            .execute_batch(
                "UPDATE consults SET answer_run=NULL;
            UPDATE wakes SET member=2,reason='consulted' WHERE id=1;
            UPDATE runs SET status='queued' WHERE id=2;",
            )
            .unwrap();
        assert_eq!(
            store
                .deliver_consult(
                    1,
                    ConsultDelivery::Expert,
                    WakeDecision::Fold(RunId(2)),
                    false,
                    0
                )
                .unwrap(),
            Receipt::Queued
        );
        assert_eq!(store.consult_for_run(RunId(2)).unwrap().unwrap().id, 1);
        assert!(store.pending_consult_dispatches().unwrap().is_empty());
        let count: i64 = store
            .conn()
            .unwrap()
            .query_row("SELECT count(*) FROM wakes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            store
                .deliver_consult(1, ConsultDelivery::Expert, WakeDecision::Queue, false, 0)
                .unwrap(),
            Receipt::Queued
        );
    }

    #[test]
    fn two_connections_cannot_claim_or_complete_the_same_consult_twice() {
        let folder = std::path::Path::new("target/consult-recovery");
        std::fs::create_dir_all(folder).unwrap();
        let path = folder.join(format!("{}-claim.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let first = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
        consult_fixture(&first);
        first
            .conn()
            .unwrap()
            .execute("UPDATE consults SET answer_run=NULL", [])
            .unwrap();
        let second = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
        for store in [&first, &second] {
            store
                .conn()
                .unwrap()
                .busy_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }
        let barrier = std::sync::Barrier::new(2);
        let claimed = std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                barrier.wait();
                first.claim_consult(RunId(2), MemberId(2)).unwrap()
            });
            let b = scope.spawn(|| {
                barrier.wait();
                second.claim_consult(RunId(3), MemberId(2)).unwrap()
            });
            [a.join().unwrap(), b.join().unwrap()]
        });
        assert_eq!(claimed.iter().filter(|claim| claim.is_some()).count(), 1);
        let mut result = claimed.into_iter().flatten().next().unwrap();
        result.status = ConsultStatus::Answered;
        let post = Post {
            author: "Member 2".into(),
            text: "Synthetic answer".into(),
            at: 0,
            unsupported: false,
        };
        let accepted = std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                barrier.wait();
                first.finish_consult(&result, &post).unwrap()
            });
            let b = scope.spawn(|| {
                barrier.wait();
                second.finish_consult(&result, &post).unwrap()
            });
            [a.join().unwrap(), b.join().unwrap()]
        });
        assert_eq!(accepted.into_iter().filter(|accepted| *accepted).count(), 1);
        let deliveries = std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                barrier.wait();
                first
                    .deliver_consult(
                        result.id,
                        ConsultDelivery::Answer,
                        WakeDecision::Queue,
                        false,
                        0,
                    )
                    .unwrap()
            });
            let b = scope.spawn(|| {
                barrier.wait();
                second
                    .deliver_consult(
                        result.id,
                        ConsultDelivery::Answer,
                        WakeDecision::Queue,
                        false,
                        0,
                    )
                    .unwrap()
            });
            [a.join().unwrap(), b.join().unwrap()]
        });
        assert_eq!(deliveries, [Receipt::Queued, Receipt::Queued]);
        let runs: i64 = first
            .conn()
            .unwrap()
            .query_row("SELECT count(*) FROM runs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(runs, 4, "concurrent delivery reuses one queued result turn");
        assert_eq!(first.consult_answers_for_run(RunId(4)).unwrap().len(), 1);
        drop(first);
        drop(second);
        let reopened = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
        assert_eq!(
            reopened
                .consult_for_run(result.answer_run.unwrap())
                .unwrap()
                .unwrap()
                .status,
            ConsultStatus::Answered
        );
        let count: i64 = reopened
            .conn()
            .unwrap()
            .query_row("SELECT count(*) FROM posts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }

    /// The bundled SQLite is built with FTS5 (rusqlite's `bundled` feature compiles it in), so
    /// the conversations are searched through an index with no new dependency.
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
