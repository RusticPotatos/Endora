//! Conversation persistence: indexed pages, atomic retries, short deletion passes.

use rusqlite::{OptionalExtension, params};

use super::{SqliteTeamStore, backend};
use crate::application::conversation_ports::{Accepted, ConversationStore};
use crate::application::ports::Stored;
use crate::domain::assistance::{self, NewRequest, Request, SourceRef};
use crate::domain::conversation::{self, Conversation, Information, Message, Operation};
use crate::domain::{MemberId, Millis, ProjectId, Ref, RunId, TaskId};

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS conversation_ready_runs (
 run INTEGER PRIMARY KEY REFERENCES runs(id), revision INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS conversations (
 id INTEGER PRIMARY KEY AUTOINCREMENT, project INTEGER NOT NULL REFERENCES projects(id),
 member1 INTEGER NOT NULL REFERENCES members(id), member2 INTEGER NOT NULL REFERENCES members(id),
 at_ms INTEGER NOT NULL, latest INTEGER, message_count INTEGER NOT NULL DEFAULT 0,
 deleted_at INTEGER, CHECK(member1 != member2)
) STRICT;
CREATE INDEX IF NOT EXISTS conversations_active ON conversations(id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS conversations_deleted ON conversations(id) WHERE deleted_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS conversations_member1 ON conversations(member1,id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS conversations_member2 ON conversations(member2,id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS conversations_pair ON conversations(member1,member2,id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS conversations_member1_project ON conversations(member1,project) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS conversations_member2_project ON conversations(member2,project) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS conversations_active_projects ON projects(id) WHERE status='active';
CREATE TABLE IF NOT EXISTS conversation_messages (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 conversation INTEGER NOT NULL REFERENCES conversations(id),
 member INTEGER NOT NULL REFERENCES members(id), run INTEGER NOT NULL REFERENCES runs(id),
 client_key TEXT NOT NULL, text TEXT NOT NULL, evidence TEXT NOT NULL,
 accepted_day INTEGER NOT NULL, at_ms INTEGER NOT NULL,
 UNIQUE(conversation,member,client_key)
) STRICT;
CREATE INDEX IF NOT EXISTS messages_page ON conversation_messages(conversation,id);
-- Admission accounting contains no body, references or conversation link. Deleting history
-- cannot reset an allowance. Point lookups never count the growing message archive.
CREATE TABLE IF NOT EXISTS conversation_turn_usage (
 run INTEGER PRIMARY KEY REFERENCES runs(id), accepted INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS conversation_day_usage (
 member INTEGER NOT NULL REFERENCES members(id), accepted_day INTEGER NOT NULL,
 accepted INTEGER NOT NULL, PRIMARY KEY(member,accepted_day)
) STRICT;
CREATE TRIGGER IF NOT EXISTS messages_are_immutable BEFORE UPDATE ON conversation_messages
 BEGIN SELECT RAISE(ABORT, 'a message is never rewritten'); END;
CREATE TABLE IF NOT EXISTS conversation_operations (
 id INTEGER PRIMARY KEY AUTOINCREMENT, conversation INTEGER REFERENCES conversations(id),
 run INTEGER NOT NULL REFERENCES runs(id), operation TEXT NOT NULL, reason TEXT NOT NULL, at_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS conversation_operations_scope ON conversation_operations(conversation,id);
";

const COLUMNS: &str = "id,project,member1,member2,at_ms,latest,message_count";
const PAIR_SCOPES: &str = "SELECT * FROM (
 SELECT * FROM (SELECT id,project,member1,member2,at_ms,latest,message_count FROM conversations WHERE member1=?1 AND member2=?2 AND deleted_at IS NULL ORDER BY id LIMIT 2)
 UNION ALL
 SELECT * FROM (SELECT id,project,member1,member2,at_ms,latest,message_count FROM conversations WHERE member1=?2 AND member2=?1 AND deleted_at IS NULL ORDER BY id LIMIT 2)
 ) ORDER BY id LIMIT 2";
#[cfg(test)]
impl SqliteTeamStore {
    pub(crate) fn conversation_audit_for_test(
        &self,
        run: RunId,
    ) -> Vec<(Option<i64>, String, String)> {
        let conn = self.conn().unwrap();
        conn.prepare("SELECT conversation,operation,reason FROM conversation_operations WHERE run=?1 ORDER BY id LIMIT 100")
            .unwrap().query_map([run.0], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap()
    }
}

pub(super) const MESSAGES_PAGE: &str = "SELECT id,conversation,member,run,text,evidence,at_ms
 FROM conversation_messages WHERE conversation=?1 AND id>?2 ORDER BY id LIMIT 51";

fn scope(row: &rusqlite::Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get(0)?,
        project: ProjectId(row.get(1)?),
        participants: [MemberId(row.get(2)?), MemberId(row.get(3)?)],
        at: row.get(4)?,
        latest: row.get(5)?,
        messages: row.get(6)?,
    })
}

const REQUEST_COLUMNS: &str = "id,conversation,project,requester,requester_run,expert,objective,evidence,status,deadline_at,expert_run,answer,answer_evidence,notified,notice_run,notice_cancelled";
const DUE_REQUESTS: &str = " FROM assistance_requests WHERE status='queued' AND expert_run IS NULL AND deadline_at>?1 ORDER BY deadline_at,id LIMIT 50";
const ACTIVE_MEMBER_SCOPE: &str = "SELECT EXISTS(SELECT 1 FROM projects p WHERE p.status='active' AND
    (EXISTS(SELECT 1 FROM conversations c WHERE c.member1=?1 AND c.project=p.id AND c.deleted_at IS NULL)
    OR EXISTS(SELECT 1 FROM conversations c WHERE c.member2=?1 AND c.project=p.id AND c.deleted_at IS NULL)))";
const ACTIVE_TASK_SCOPE: &str = "SELECT EXISTS(SELECT 1 FROM projects p WHERE p.id=?2 AND p.status='active' AND
    (EXISTS(SELECT 1 FROM conversations c WHERE c.member1=?1 AND c.project=p.id AND c.deleted_at IS NULL)
    OR EXISTS(SELECT 1 FROM conversations c WHERE c.member2=?1 AND c.project=p.id AND c.deleted_at IS NULL)))";
const TASK_AWAITS_ASSISTANCE: &str = "SELECT EXISTS(
    SELECT 1 FROM assistance_requests a JOIN runs s ON s.id=a.requester_run
    WHERE a.requester=?1 AND s.member=?1 AND s.task=?2 AND a.notice_cancelled=0
    AND (a.status IN ('queued','processing') OR (a.status='answered' AND a.notified=0)))
    OR EXISTS(SELECT 1 FROM runs n JOIN assistance_requests a ON a.notice_run=n.id
    JOIN runs s ON s.id=a.requester_run
    WHERE n.member=?1 AND n.status IN ('queued','running','retry_waiting')
    AND a.requester=?1 AND s.member=?1 AND s.task=?2
    AND a.status='answered' AND a.notice_cancelled=0)";
const PENDING_NOTICES: &str = " FROM assistance_requests WHERE status='answered' AND notified=0 AND notice_cancelled=0 ORDER BY id LIMIT 50";
const REQUEST_PAGE: &str =
    " FROM assistance_requests WHERE conversation=?1 AND id>?2 ORDER BY id LIMIT 51";
const FINDING_BY_REQUEST: &str =
    "SELECT run,text,evidence,at_ms FROM assistance_findings WHERE request=?1";

fn request(row: &rusqlite::Row<'_>) -> rusqlite::Result<Request> {
    Ok(Request {
        id: row.get(0)?,
        conversation: row.get(1)?,
        project: ProjectId(row.get(2)?),
        requester: MemberId(row.get(3)?),
        requester_run: RunId(row.get(4)?),
        expert: MemberId(row.get(5)?),
        objective: row.get(6)?,
        evidence: SourceRef::read(&row.get::<_, String>(7)?),
        status: row.get(8)?,
        deadline_at: row.get(9)?,
        expert_run: row.get::<_, Option<i64>>(10)?.map(RunId),
        answer: row.get(11)?,
        answer_evidence: SourceRef::read(&row.get::<_, String>(12)?),
        notified: row.get(13)?,
        notice_run: row.get::<_, Option<i64>>(14)?.map(RunId),
        notice_cancelled: row.get(15)?,
    })
}

fn source_key(source: SourceRef) -> (&'static str, i64) {
    match source {
        SourceRef::Document(id) => ("d", id),
        SourceRef::Receipt(id) => ("r", id),
    }
}

fn source_revision(
    conn: &rusqlite::Connection,
    conversation: i64,
    source: SourceRef,
) -> Stored<Option<i64>> {
    let (_, id) = source_key(source);
    let sql=match source {
        SourceRef::Document(_)=>"SELECT d.at_ms FROM documents d JOIN tasks t ON t.id=d.task
            JOIN conversations c ON c.project=t.project WHERE c.id=?1 AND c.deleted_at IS NULL AND d.id=?2",
        SourceRef::Receipt(_)=>"SELECT d.at_ms FROM call_receipts d JOIN runs s ON s.id=d.run
            LEFT JOIN tasks t ON t.id=s.task LEFT JOIN assistance_requests a ON a.expert_run=s.id
            LEFT JOIN assistance_requests b ON b.notice_run=s.id
            LEFT JOIN consults lc ON lc.answer_run=s.id LEFT JOIN consult_projects lp ON lp.consult=lc.id
            JOIN conversations c ON c.project=COALESCE(t.project,a.project,b.project,lp.project)
            WHERE c.id=?1 AND c.deleted_at IS NULL AND d.id=?2",
    };
    conn.query_row(sql, params![conversation, id], |r| r.get(0))
        .optional()
        .map_err(backend)
}

impl ConversationStore for SqliteTeamStore {
    fn assistance_finding(&self, request: i64) -> Stored<Option<assistance::Finding>> {
        self.conn()?
            .query_row(FINDING_BY_REQUEST, [request], |r| {
                Ok(assistance::Finding {
                    run: RunId(r.get(0)?),
                    text: r.get(1)?,
                    evidence: SourceRef::read(&r.get::<_, String>(2)?),
                    at: r.get(3)?,
                })
            })
            .optional()
            .map_err(backend)
    }

    fn record_assistance_finding(
        &self,
        run: RunId,
        text: &str,
        evidence: &[SourceRef],
        now: Millis,
    ) -> Stored<Accepted> {
        let clean = match conversation::words("finding_01", text, &[]) {
            Ok(clean) if clean == text => clean,
            _ => return Ok(Err("a bounded canonical finding is required")),
        };
        if evidence.len() > assistance::SOURCES_MOST {
            return Ok(Err("too many finding sources"));
        }
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let enabled: Option<String> = tx
            .query_row(
                "SELECT value FROM settings WHERE key='conversations_enabled'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(revision) = enabled
            .as_deref()
            .and_then(|s| s.strip_prefix("1:"))
            .and_then(|s| s.parse::<i64>().ok())
        else {
            return Ok(Err("assistance is disabled"));
        };
        let sql = format!("SELECT a.id,a.conversation,a.evidence FROM assistance_requests a
            JOIN runs r ON r.id=a.notice_run AND r.member=a.requester
            JOIN runs s ON s.id=a.requester_run AND s.member=a.requester
            JOIN members m ON m.id=r.member JOIN conversations c ON c.id=a.conversation
            JOIN projects p ON p.id=c.project
            JOIN conversation_ready_runs ready ON ready.run=r.id AND ready.revision=?2
            WHERE r.id=?1 AND r.status='running' AND a.status='answered' AND a.notice_cancelled=0
            AND c.deleted_at IS NULL AND c.project=a.project AND p.status='active' AND m.paused=0 AND m.furloughed_at IS NULL AND m.role!='ceo'
            AND (c.member1=m.id OR c.member2=m.id)
            AND NOT EXISTS(SELECT 1 FROM settings WHERE key='stopped' AND value='1') AND NOT {}", super::record::read_secrets("r.id"));
        let associated: Option<(i64, i64, String)> = tx
            .query_row(&sql, params![run.0, revision], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()
            .map_err(backend)?;
        let Some((id, conversation, original_sources)) = associated else {
            return Ok(Err(
                "no active requester-result turn belongs to this finding",
            ));
        };
        let kept = SourceRef::kept(evidence);
        let prior: Option<(i64, String, String)> = tx
            .query_row(
                "SELECT run,text,evidence FROM assistance_findings WHERE request=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some((prior_run, prior_text, prior_sources)) = prior {
            return Ok(
                if prior_run == run.0 && prior_text == clean && prior_sources == kept {
                    Ok(id)
                } else {
                    Err("the request already has a different finding")
                },
            );
        }
        let originals = SourceRef::read(&original_sources);
        for source in evidence {
            if !originals.contains(source) {
                return Ok(Err("finding sources must come from this request"));
            }
            let Some(revision) = source_revision(&tx, conversation, *source)? else {
                return Ok(Err("original evidence unavailable"));
            };
            let (kind, resource) = source_key(*source);
            let granted: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM assistance_source_grants WHERE conversation=?1 AND kind=?2 AND resource=?3 AND revision=?4 AND expires_at>?5)", params![conversation,kind,resource,revision,now], |r| r.get(0)).map_err(backend)?;
            if !granted {
                return Ok(Err("original evidence grant unavailable"));
            }
            let origin_sql = match source {
                SourceRef::Document(_) => "SELECT run FROM documents WHERE id=?1",
                SourceRef::Receipt(_) => {
                    "SELECT run FROM call_receipts WHERE id=?1 AND confidential=0"
                }
            };
            let origin = tx
                .query_row(origin_sql, [resource], |r| r.get::<_, Option<i64>>(0))
                .optional()
                .map_err(backend)?
                .flatten();
            let Some(origin) = origin else {
                return Ok(Err("original evidence source unavailable"));
            };
            let private: bool = tx
                .query_row(
                    &format!(
                        "SELECT NOT EXISTS(SELECT 1 FROM runs WHERE id=?1) OR {}",
                        super::record::read_secrets("?1")
                    ),
                    [origin],
                    |r| r.get(0),
                )
                .map_err(backend)?;
            if private {
                return Ok(Err("original evidence source is not Internal"));
            }
        }
        tx.execute("INSERT INTO assistance_findings(request,run,text,evidence,at_ms) VALUES(?1,?2,?3,?4,?5)", params![id,run.0,clean,kept,now]).map_err(backend)?;
        tx.execute("INSERT INTO conversation_operations(conversation,run,operation,reason,at_ms) VALUES(?1,?2,'finding','accepted',?3)", params![conversation,run.0,now]).map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(Ok(id))
    }

    fn has_active_conversation(
        &self,
        member: MemberId,
        project: Option<ProjectId>,
    ) -> Stored<bool> {
        let conn = self.conn()?;
        if let Some(project) = project {
            return conn
                .query_row(ACTIVE_TASK_SCOPE, params![member.0, project.0], |r| {
                    r.get(0)
                })
                .map_err(backend);
        }
        conn.query_row(ACTIVE_MEMBER_SCOPE, [member.0], |r| r.get(0))
            .map_err(backend)
    }

    fn task_awaits_assistance(&self, task: TaskId, member: MemberId) -> Stored<bool> {
        self.conn()?
            .query_row(TASK_AWAITS_ASSISTANCE, params![member.0, task.0], |row| {
                row.get(0)
            })
            .map_err(backend)
    }

    fn assistance_charging_project(&self, run: RunId) -> Stored<Option<ProjectId>> {
        self.conn()?
            .query_row(
                &format!(
                    "SELECT COALESCE(
            (SELECT project FROM assistance_requests WHERE expert_run=?1),
            (SELECT project FROM assistance_requests WHERE notice_run=?1),
            {},
            (SELECT project FROM ledger WHERE run=?1 AND kind='reserve' ORDER BY id DESC LIMIT 1))",
                    super::history::CONSULT_PROJECT_FOR_RUN
                ),
                [run.0],
                |r| r.get::<_, Option<i64>>(0),
            )
            .map(|value| value.map(ProjectId))
            .map_err(backend)
    }
    fn assistance_request(&self, id: i64) -> Stored<Option<Request>> {
        self.conn()?
            .query_row(
                &format!("SELECT {REQUEST_COLUMNS} FROM assistance_requests WHERE id=?1"),
                [id],
                request,
            )
            .optional()
            .map_err(backend)
    }

    fn cancel_assistance_request(
        &self,
        id: i64,
        actor: Option<RunId>,
        now: Millis,
    ) -> Stored<Accepted> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let existing: Option<(i64, String)> = tx
            .query_row(
                "SELECT conversation,status FROM assistance_requests WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((conversation, status)) = existing else {
            return Ok(Err("request unavailable"));
        };
        if status == "cancelled" {
            return Ok(Ok(id));
        }
        if !matches!(status.as_str(), "queued" | "processing") {
            return Ok(Err("a terminal request cannot be cancelled"));
        }
        tx.execute("UPDATE assistance_requests SET status='cancelled',notice_cancelled=1 WHERE id=?1 AND status IN ('queued','processing')",[id]).map_err(backend)?;
        if let Some(actor) = actor {
            tx.execute("INSERT INTO conversation_operations(conversation,run,operation,reason,at_ms) VALUES(?1,?2,'cancel','accepted',?3)",params![conversation,actor.0,now]).map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(Ok(id))
    }
    fn cancel_assistance(&self) -> Stored<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(backend)?;
        tx.execute("UPDATE assistance_requests SET status='cancelled' WHERE status IN ('queued','processing')",[]).map_err(backend)?;
        tx.execute("UPDATE assistance_requests SET notice_cancelled=1 WHERE id IN (
            SELECT id FROM assistance_requests WHERE status='answered' AND notified=0 AND notice_cancelled=0
            UNION SELECT a.id FROM runs r JOIN assistance_requests a ON a.notice_run=r.id WHERE r.status IN ('queued','running','retry_waiting')
            UNION SELECT a.id FROM runs r JOIN assistance_requests a ON a.expert_run=r.id WHERE a.status='answered' AND r.status IN ('queued','running','retry_waiting'))",[]).map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(())
    }

    fn close_unanswered_assistance(&self, run: RunId, completed: bool) -> Stored<()> {
        self.conn()?.execute("UPDATE assistance_requests SET status=?2 WHERE expert_run=?1 AND status='processing'",
            params![run.0,if completed {"declined"} else {"failed"}]).map_err(backend)?;
        Ok(())
    }
    fn grant_assistance_source(
        &self,
        conversation: i64,
        source: SourceRef,
        at: Millis,
    ) -> Stored<bool> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let (kind, resource) = source_key(source);
        let candidate = source_revision(&tx, conversation, source)?;
        let Some(revision) = candidate else {
            return Ok(false);
        };
        let already: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM assistance_source_grants WHERE conversation=?1 AND kind=?2 AND resource=?3 AND expires_at>?4)",params![conversation,kind,resource,at],|r|r.get(0)).map_err(backend)?;
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM assistance_source_grants WHERE conversation=?1 AND expires_at>?2",
            params![conversation,at], |r|r.get(0),
        ).map_err(backend)?;
        if count >= i64::try_from(assistance::SOURCES_MOST).map_err(backend)? && !already {
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM assistance_source_grants WHERE conversation=?1 AND expires_at<=?2",
            params![conversation, at],
        )
        .map_err(backend)?;
        tx.execute("INSERT INTO assistance_source_grants(conversation,kind,resource,revision,granted_at,expires_at)
            VALUES(?1,?2,?3,?4,?5,?6)
            ON CONFLICT(conversation,kind,resource) DO UPDATE SET revision=excluded.revision,
            granted_at=excluded.granted_at, expires_at=excluded.expires_at",
            params![conversation,kind,resource,revision,at,at+assistance::GRANT_MS]).map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(true)
    }

    fn assistance_source_granted(
        &self,
        conversation: i64,
        source: SourceRef,
        at: Millis,
    ) -> Stored<bool> {
        let conn = self.conn()?;
        let Some(revision) = source_revision(&conn, conversation, source)? else {
            return Ok(false);
        };
        let (kind, resource) = source_key(source);
        conn.query_row("SELECT EXISTS(SELECT 1 FROM assistance_source_grants WHERE conversation=?1 AND kind=?2 AND resource=?3 AND revision=?4 AND expires_at>?5)",
            params![conversation,kind,resource,revision,at],|r|r.get(0)).map_err(backend)
    }

    fn assistance_grants(&self, conversation: i64, at: Millis) -> Stored<Vec<(SourceRef, Millis)>> {
        let conn = self.conn()?;
        let mut stmt=conn.prepare("SELECT kind,resource,expires_at FROM assistance_source_grants WHERE conversation=?1 AND expires_at>?2 ORDER BY kind,resource LIMIT 5").map_err(backend)?;
        stmt.query_map(params![conversation, at], |r| {
            let kind: String = r.get(0)?;
            let id = r.get(1)?;
            Ok((
                if kind == "d" {
                    SourceRef::Document(id)
                } else {
                    SourceRef::Receipt(id)
                },
                r.get(2)?,
            ))
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn revoke_assistance_source(&self, conversation: i64, source: SourceRef) -> Stored<()> {
        let (kind, resource) = source_key(source);
        self.conn()?.execute("DELETE FROM assistance_source_grants WHERE conversation=?1 AND kind=?2 AND resource=?3",params![conversation,kind,resource]).map_err(backend)?;
        Ok(())
    }
    fn inactive_assistance_runs(&self) -> Stored<Vec<RunId>> {
        let conn = self.conn()?;
        let mut stmt=conn.prepare("SELECT r.id FROM runs r WHERE r.status IN ('queued','running','retry_waiting') AND (
            EXISTS(SELECT 1 FROM assistance_requests a WHERE a.expert_run=r.id AND (a.status IN ('cancelled','expired') OR a.notice_cancelled=1))
            OR EXISTS(SELECT 1 FROM assistance_requests a WHERE a.notice_run=r.id AND a.notice_cancelled=1)
        ) ORDER BY r.id LIMIT 50").map_err(backend)?;
        stmt.query_map([], |r| r.get::<_, i64>(0).map(RunId))
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn pending_assistance_notices(&self) -> Stored<Vec<Request>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("SELECT {REQUEST_COLUMNS}{PENDING_NOTICES}"))
            .map_err(backend)?;
        stmt.query_map([], request)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn queue_assistance_notice(&self, id: i64, now: Millis) -> Stored<Option<RunId>> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let requester: Option<i64>=tx.query_row("SELECT requester FROM assistance_requests WHERE id=?1 AND status='answered' AND notified=0 AND notice_cancelled=0 AND notice_run IS NULL",
            [id],|r|r.get(0)).optional().map_err(backend)?;
        let Some(requester) = requester else {
            return Ok(None);
        };
        let busy: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM runs WHERE member=?1 AND status IN ('queued','running','retry_waiting'))",
            [requester],|r|r.get(0)).map_err(backend)?;
        if busy {
            return Ok(None);
        };
        let (_, run) = super::assistance::queue_turn(
            &tx,
            MemberId(requester),
            None,
            crate::domain::WakeReason::QuestionAnswered,
            false,
            now,
        )?;
        tx.execute(
            "UPDATE assistance_requests SET notified=1,notice_run=?2 WHERE id=?1",
            params![id, run.0],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(Some(run))
    }

    fn assistance_notice_for_run(&self, run: RunId) -> Stored<Option<Request>> {
        self.conn()?
            .query_row(
                &format!("SELECT {REQUEST_COLUMNS} FROM assistance_requests WHERE notice_run=?1"),
                [run.0],
                request,
            )
            .optional()
            .map_err(backend)
    }

    fn expire_assistance(&self, now: Millis) -> Stored<Vec<RunId>> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        tx.execute("UPDATE assistance_requests SET status=CASE
            WHEN (SELECT status FROM runs WHERE id=expert_run)='succeeded' THEN 'declined' ELSE 'failed' END
            WHERE id IN (SELECT a.id FROM assistance_requests a JOIN runs r ON r.id=a.expert_run
                WHERE a.status='processing' AND r.status NOT IN ('queued','running','retry_waiting') ORDER BY a.id LIMIT 50)",[]).map_err(backend)?;
        let due: Vec<(i64, Option<i64>)> = {
            let mut stmt=tx.prepare("SELECT id,expert_run FROM assistance_requests WHERE status IN ('queued','processing') AND deadline_at<=?1 ORDER BY deadline_at,id LIMIT 50").map_err(backend)?;
            stmt.query_map([now], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(backend)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(backend)?
        };
        for (id, _) in &due {
            tx.execute("UPDATE assistance_requests SET status='expired' WHERE id=?1 AND status IN ('queued','processing')",[id]).map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(due
            .into_iter()
            .filter_map(|(_, run)| run.map(RunId))
            .collect())
    }

    fn assistance_page(&self, conversation: i64, after: i64) -> Stored<Vec<Request>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("SELECT {REQUEST_COLUMNS}{REQUEST_PAGE}"))
            .map_err(backend)?;
        stmt.query_map(params![conversation, after], request)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn accept_assistance_answer(
        &self,
        run: RunId,
        answer: &str,
        evidence: &[SourceRef],
        now: Millis,
    ) -> Stored<Accepted> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let existing: Option<(i64,i64,String,Option<String>,String,String)> = tx.query_row(
            "SELECT id,conversation,status,answer,answer_evidence,objective FROM assistance_requests WHERE expert_run=?1",
            [run.0],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))
        ).optional().map_err(backend)?;
        let Some((id, conversation, status, prior, prior_refs, _)) = existing else {
            return Ok(Err("no assistance request belongs to this turn"));
        };
        let refs = SourceRef::kept(evidence);
        if status == "answered" {
            if prior.as_deref() == Some(answer) && prior_refs == refs {
                return Ok(Ok(id));
            }
            return Ok(Err("the request already has a different answer"));
        }
        let changed=tx.execute("UPDATE assistance_requests SET status='answered',answer=?2,answer_evidence=?3,answer_run=?4,notified=0 WHERE id=?1 AND status='processing' AND deadline_at>?5",
            params![id,answer,refs,run.0,now]).map_err(backend)?;
        if changed == 0 {
            return Ok(Err("the request is no longer open"));
        }
        tx.execute("INSERT INTO conversation_operations(conversation,run,operation,reason,at_ms) VALUES(?1,?2,'answer','accepted',?3)",
            params![conversation,run.0,now]).map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(Ok(id))
    }

    fn due_assistance(&self, now: Millis) -> Stored<Vec<Request>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("SELECT {REQUEST_COLUMNS}{DUE_REQUESTS}"))
            .map_err(backend)?;
        stmt.query_map([now], request)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn queue_assistance(&self, id: i64, now: Millis) -> Stored<Option<RunId>> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let expert: Option<i64>=tx.query_row(
            "SELECT expert FROM assistance_requests WHERE id=?1 AND status='queued' AND expert_run IS NULL AND deadline_at>?2",
            params![id,now],|r|r.get(0)).optional().map_err(backend)?;
        let Some(expert) = expert else {
            return Ok(None);
        };
        let (_, run) = super::assistance::queue_turn(
            &tx,
            MemberId(expert),
            None,
            crate::domain::WakeReason::Consulted,
            false,
            now,
        )?;
        tx.execute(
            "UPDATE assistance_requests SET status='processing',expert_run=?2 WHERE id=?1",
            params![id, run.0],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(Some(run))
    }

    fn assistance_for_run(&self, run: RunId) -> Stored<Option<Request>> {
        self.conn()?
            .query_row(
                &format!("SELECT {REQUEST_COLUMNS} FROM assistance_requests WHERE expert_run=?1"),
                [run.0],
                request,
            )
            .optional()
            .map_err(backend)
    }

    fn accept_assistance(&self, request: &NewRequest) -> Stored<Accepted> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let active: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversations c
                JOIN runs r ON r.id=?5 AND r.member=?3
                JOIN projects p ON p.id=c.project AND p.status='active'
                LEFT JOIN tasks t ON t.id=r.task
                WHERE c.id=?1 AND c.project=?2 AND c.deleted_at IS NULL
                AND (c.member1=?3 OR c.member2=?3) AND (c.member1=?4 OR c.member2=?4)
                AND (r.task IS NULL OR t.project=c.project))",
                params![
                    request.conversation,
                    request.project.0,
                    request.requester.0,
                    request.expert.0,
                    request.requester_run.0
                ],
                |r| r.get(0),
            )
            .map_err(backend)?;
        if !active {
            return Ok(Err("conversation unavailable"));
        }
        let evidence = SourceRef::kept(&request.evidence);
        let task: Option<i64> = tx
            .query_row(
                "SELECT task FROM runs WHERE id=?1",
                [request.requester_run.0],
                |r| r.get(0),
            )
            .map_err(backend)?;
        let prior: Option<(i64,i64,String,String,Option<i64>)> = tx.query_row(
            "SELECT id,expert,objective,evidence,(SELECT task FROM runs WHERE id=assistance_requests.requester_run) FROM assistance_requests WHERE conversation=?1 AND requester=?2 AND client_key=?3",
            params![request.conversation,request.requester.0,request.key],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))
        ).optional().map_err(backend)?;
        if let Some((id, expert, objective, refs, original_task)) = prior {
            if expert == request.expert.0
                && objective == request.objective
                && refs == evidence
                && original_task == task
            {
                tx.execute("INSERT INTO conversation_operations(conversation,run,operation,reason,at_ms) VALUES(?1,?2,'request','replayed',?3)",
                    params![request.conversation,request.requester_run.0,request.at]).map_err(backend)?;
                tx.commit().map_err(backend)?;
                return Ok(Ok(id));
            }
            return Ok(Err("request key already belongs to different content"));
        }
        let open: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM assistance_requests WHERE conversation=?1 AND requester=?2 AND expert=?3 AND status IN ('queued','processing'))",
            params![request.conversation,request.requester.0,request.expert.0],|r|r.get(0)).map_err(backend)?;
        if open {
            return Ok(Err("one open request is allowed for this pair"));
        }
        let (turn, day) = super::assistance::consultation_usage(
            &tx,
            request.requester_run,
            request.requester,
            request.day,
            request.day_start,
        )?;
        if turn >= assistance::PER_TURN || day >= assistance::PER_DAY {
            return Ok(Err(
                "assistance limit reached: two per turn and six per day",
            ));
        }
        tx.execute("INSERT INTO assistance_requests(conversation,project,requester,requester_run,expert,client_key,objective,evidence,accepted_day,accepted_at,deadline_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![request.conversation,request.project.0,request.requester.0,request.requester_run.0,request.expert.0,request.key,request.objective,evidence,request.day,request.at,request.at.saturating_add(assistance::DEADLINE_MS)]).map_err(backend)?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO assistance_turn_usage(requester_run,accepted) VALUES(?1,1) ON CONFLICT(requester_run) DO UPDATE SET accepted=accepted+1",[request.requester_run.0]).map_err(backend)?;
        tx.execute("INSERT INTO assistance_day_usage(requester,accepted_day,accepted) VALUES(?1,?2,1) ON CONFLICT(requester,accepted_day) DO UPDATE SET accepted=accepted+1",
            params![request.requester.0,request.day]).map_err(backend)?;
        tx.execute("INSERT INTO conversation_operations(conversation,run,operation,reason,at_ms) VALUES(?1,?2,'request','accepted',?3)",
            params![request.conversation,request.requester_run.0,request.at]).map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(Ok(id))
    }

    fn mark_conversation_ready(&self, run: RunId, revision: i64) -> Stored<()> {
        self.conn()?.execute("INSERT INTO conversation_ready_runs(run,revision) VALUES(?1,?2) ON CONFLICT(run) DO UPDATE SET revision=excluded.revision",params![run.0,revision]).map_err(backend)?;
        Ok(())
    }

    fn conversation_ready(&self, run: RunId, revision: i64) -> Stored<bool> {
        self.conn()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversation_ready_runs WHERE run=?1 AND revision=?2)",
                params![run.0, revision],
                |r| r.get(0),
            )
            .map_err(backend)
    }

    fn conversation_operations_page(&self, after: i64) -> Stored<Vec<Operation>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT o.id,o.conversation,r.member,o.run,o.operation,o.reason,o.at_ms
            FROM conversation_operations o JOIN runs r ON r.id=o.run WHERE o.id>?1 ORDER BY o.id LIMIT 51").map_err(backend)?;
        stmt.query_map([after], |r| {
            Ok(Operation {
                id: r.get(0)?,
                conversation: r.get(1)?,
                member: MemberId(r.get(2)?),
                run: RunId(r.get(3)?),
                operation: r.get(4)?,
                reason: r.get(5)?,
                at: r.get(6)?,
            })
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn add_conversation(
        &self,
        project: ProjectId,
        members: [MemberId; 2],
        at: Millis,
    ) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO conversations(project,member1,member2,at_ms) VALUES(?1,?2,?3,?4)",
            params![project.0, members[0].0, members[1].0, at],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn conversation(&self, id: i64) -> Stored<Option<Conversation>> {
        self.conn()?
            .query_row(
                &format!("SELECT {COLUMNS} FROM conversations WHERE id=?1 AND deleted_at IS NULL"),
                [id],
                scope,
            )
            .optional()
            .map_err(backend)
    }

    fn conversation_pair(&self, a: MemberId, b: MemberId) -> Stored<Vec<Conversation>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(PAIR_SCOPES).map_err(backend)?;
        stmt.query_map(params![a.0, b.0], scope)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn conversation_page(&self, member: Option<MemberId>, after: i64) -> Stored<Vec<Conversation>> {
        let conn = self.conn()?;
        if let Some(member) = member {
            // Bound both index branches before UNION and sorting, even for a prolific member.
            let sql = format!("SELECT * FROM (
                SELECT * FROM (SELECT {COLUMNS} FROM conversations WHERE member1=?1 AND deleted_at IS NULL AND id>?2 ORDER BY id LIMIT 51)
                UNION ALL
                SELECT * FROM (SELECT {COLUMNS} FROM conversations WHERE member2=?1 AND deleted_at IS NULL AND id>?2 ORDER BY id LIMIT 51)
                ) ORDER BY id LIMIT 51");
            let mut stmt = conn.prepare(&sql).map_err(backend)?;
            return stmt
                .query_map(params![member.0, after], scope)
                .map_err(backend)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(backend);
        }
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM conversations WHERE deleted_at IS NULL AND id>?1 ORDER BY id LIMIT 51")).map_err(backend)?;
        stmt.query_map([after], scope)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn accept_information(&self, message: &Information) -> Stored<Accepted> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM conversations WHERE id=?1 AND deleted_at IS NULL AND (member1=?2 OR member2=?2))",
            params![message.conversation,message.member.0],|r|r.get(0)).map_err(backend)?;
        if !exists {
            return Ok(Err("conversation unavailable"));
        }
        let evidence = Ref::kept(&message.evidence);
        let prior: Option<(i64,String,String)> = tx.query_row("SELECT id,text,evidence FROM conversation_messages WHERE conversation=?1 AND member=?2 AND client_key=?3",
            params![message.conversation,message.member.0,message.key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(backend)?;
        if let Some((id, text, refs)) = prior {
            if text == message.text && refs == evidence {
                tx.execute("INSERT INTO conversation_operations(conversation,run,operation,reason,at_ms) VALUES(?1,?2,'send','replayed',?3)",
                    params![message.conversation,message.run.0,message.at]).map_err(backend)?;
                tx.commit().map_err(backend)?;
                return Ok(Ok(id));
            }
            return Ok(Err("message key already belongs to different content"));
        }
        let turn: i64 = tx
            .query_row(
                "SELECT COALESCE((SELECT accepted FROM conversation_turn_usage WHERE run=?1),0)",
                [message.run.0],
                |r| r.get(0),
            )
            .map_err(backend)?;
        let day: i64 = tx.query_row("SELECT COALESCE((SELECT accepted FROM conversation_day_usage WHERE member=?1 AND accepted_day=?2),0)",
            params![message.member.0,message.day],|r|r.get(0)).map_err(backend)?;
        if usize::try_from(turn).map_err(backend)? >= conversation::PER_TURN
            || usize::try_from(day).map_err(backend)? >= conversation::PER_DAY
        {
            return Ok(Err(
                "information message limit reached: twelve per turn and sixty per day",
            ));
        }
        tx.execute("INSERT INTO conversation_turn_usage(run,accepted) VALUES(?1,1) ON CONFLICT(run) DO UPDATE SET accepted=accepted+1",[message.run.0]).map_err(backend)?;
        tx.execute("INSERT INTO conversation_day_usage(member,accepted_day,accepted) VALUES(?1,?2,1) ON CONFLICT(member,accepted_day) DO UPDATE SET accepted=accepted+1",params![message.member.0,message.day]).map_err(backend)?;
        tx.execute("INSERT INTO conversation_messages(conversation,member,run,client_key,text,evidence,accepted_day,at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![message.conversation,message.member.0,message.run.0,message.key,message.text,evidence,message.day,message.at]).map_err(backend)?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE conversations SET latest=?2,message_count=message_count+1 WHERE id=?1",
            params![message.conversation, id],
        )
        .map_err(backend)?;
        tx.execute("INSERT INTO conversation_operations(conversation,run,operation,reason,at_ms) VALUES(?1,?2,'send','accepted',?3)",
            params![message.conversation,message.run.0,message.at]).map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(Ok(id))
    }

    fn message_page(&self, conversation: i64, after: i64) -> Stored<Vec<Message>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(MESSAGES_PAGE).map_err(backend)?;
        stmt.query_map(params![conversation, after], |r| {
            Ok(Message {
                id: r.get(0)?,
                conversation: r.get(1)?,
                member: MemberId(r.get(2)?),
                run: RunId(r.get(3)?),
                text: r.get(4)?,
                evidence: Ref::read(&r.get::<_, String>(5)?),
                at: r.get(6)?,
            })
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn conversation_operation(
        &self,
        run: RunId,
        conversation: Option<i64>,
        operation: &str,
        reason: &str,
        at: Millis,
    ) -> Stored<()> {
        self.conn()?.execute("INSERT INTO conversation_operations(conversation,run,operation,reason,at_ms) VALUES(?1,?2,?3,?4,?5)",
            params![conversation,run.0,operation,reason,at]).map_err(backend)?;
        Ok(())
    }

    fn delete_conversation(&self, id: i64, at: Millis) -> Stored<bool> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let changed = tx
            .execute(
                "UPDATE conversations SET deleted_at=?2 WHERE id=?1 AND deleted_at IS NULL",
                params![id, at],
            )
            .map_err(backend)?;
        if changed == 0 {
            return Ok(false);
        }
        tx.execute(
            "UPDATE assistance_requests SET status=CASE WHEN status IN ('queued','processing') THEN 'cancelled' ELSE status END,notice_cancelled=1 WHERE conversation=?1",
            [id],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(true)
    }

    fn purge_conversation_batch(&self) -> Stored<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(backend)?;
        let id: Option<i64> = tx
            .query_row(
                "SELECT id FROM conversations WHERE deleted_at IS NOT NULL ORDER BY id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(id) = id else {
            return Ok(0);
        };
        let messages = tx.execute("DELETE FROM conversation_messages WHERE id IN (SELECT id FROM conversation_messages WHERE conversation=?1 ORDER BY id LIMIT ?2)",
            params![id,i64::try_from(conversation::PURGE_BATCH).map_err(backend)?]).map_err(backend)?;
        let operations = tx.execute("DELETE FROM conversation_operations WHERE id IN (SELECT id FROM conversation_operations WHERE conversation=?1 ORDER BY id LIMIT ?2)",
            params![id,i64::try_from(conversation::PURGE_BATCH-messages).map_err(backend)?]).map_err(backend)?;
        let grants=tx.execute("DELETE FROM assistance_source_grants WHERE rowid IN (SELECT rowid FROM assistance_source_grants WHERE conversation=?1 LIMIT ?2)",
            params![id,i64::try_from(conversation::PURGE_BATCH-messages-operations).map_err(backend)?]).map_err(backend)?;
        let findings=tx.execute("DELETE FROM assistance_findings WHERE request IN
            (SELECT f.request FROM assistance_findings f JOIN assistance_requests a ON a.id=f.request
             WHERE a.conversation=?1 ORDER BY f.request LIMIT ?2)",
            params![id,i64::try_from(conversation::PURGE_BATCH-messages-operations-grants).map_err(backend)?]).map_err(backend)?;
        let requests=tx.execute("DELETE FROM assistance_requests WHERE id IN (SELECT a.id FROM assistance_requests a WHERE a.conversation=?1
            AND NOT EXISTS(SELECT 1 FROM runs r WHERE r.id IN (a.expert_run,a.notice_run) AND r.status IN ('queued','running','retry_waiting'))
            ORDER BY a.id LIMIT ?2)",
            params![id,i64::try_from(conversation::PURGE_BATCH-messages-operations-grants-findings).map_err(backend)?]).map_err(backend)?;
        let remaining: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM conversation_messages WHERE conversation=?1) OR EXISTS(SELECT 1 FROM conversation_operations WHERE conversation=?1) OR EXISTS(SELECT 1 FROM assistance_requests WHERE conversation=?1) OR EXISTS(SELECT 1 FROM assistance_source_grants WHERE conversation=?1)",
            [id],|r|r.get(0)).map_err(backend)?;
        if !remaining {
            tx.execute("DELETE FROM conversations WHERE id=?1", [id])
                .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(messages + operations + grants + findings + requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::history_ports::HistoryStore;
    use crate::application::ports::TeamStore;
    use rusqlite::StatementStatus;

    fn fixture(store: &SqliteTeamStore) {
        store.conn().unwrap().execute_batch("
            INSERT INTO members(id,name,role,share_percent,slots) VALUES(1,'Member 1','lead',25,2),(2,'Member 2','developer',40,2);
            INSERT INTO goals(id,title,reason,status) VALUES(1,'Fixture','','active');
            INSERT INTO projects(id,goal,title,reason,lead,status,created_by,created_at,updated_at) VALUES(1,1,'Fixture','',1,'active','owner',0,0);
            INSERT INTO wakes(id,member,reason,receipt,at_ms) VALUES(1,2,'downtime','queued',0);
            INSERT INTO runs(id,member,wake,status,queued_at) VALUES(1,2,1,'running',0);
        ").unwrap();
    }

    #[test]
    fn concurrent_result_reports_keep_one_finding_and_one_audit_row() {
        use std::sync::{Arc, Barrier};
        let path =
            std::env::temp_dir().join(format!("endora-finding-race-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
        finding_fixture(&store);
        let barrier = Arc::new(Barrier::new(2));
        let mut writers = Vec::new();
        for finding in ["First limited finding.", "Second limited finding."] {
            let other = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
            let barrier = barrier.clone();
            writers.push(std::thread::spawn(move || {
                barrier.wait();
                other
                    .record_assistance_finding(RunId(2), finding, &[SourceRef::Document(1)], 50)
                    .unwrap()
            }));
        }
        let accepted: Vec<_> = writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .collect();
        assert_eq!(accepted.iter().filter(|result| result.is_ok()).count(), 1);
        let finding = store.assistance_finding(1).unwrap().unwrap();
        assert!(
            ["First limited finding.", "Second limited finding."].contains(&finding.text.as_str())
        );
        assert_eq!(
            store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM conversation_operations WHERE operation='finding'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn retained_finding_lookup_does_not_scan_the_archive() {
        let store = SqliteTeamStore::in_memory().unwrap();
        finding_fixture(&store);
        let conn = store.conn().unwrap();
        let mut steps = Vec::new();
        for size in [1_000, 100_000] {
            let previous = if size == 1_000 { 999 } else { 1999 };
            conn.execute("WITH RECURSIVE n(id) AS (VALUES(?1+1) UNION ALL SELECT id+1 FROM n WHERE id<999+?2)
                INSERT INTO runs(id,member,wake,status,queued_at) SELECT id,2,1,'succeeded',0 FROM n", params![previous,size]).unwrap();
            conn.execute("INSERT INTO assistance_requests(id,conversation,project,requester,requester_run,expert,client_key,objective,evidence,accepted_day,accepted_at,deadline_at,status)
                SELECT id,1,1,2,1,1,'archive_'||id,'Fixture','',0,0,1000,'answered' FROM runs WHERE id>?1 AND id>=1000", [previous]).unwrap();
            conn.execute(
                "INSERT INTO assistance_findings(request,run,text,evidence,at_ms)
                SELECT id,id,'Retained finding','',0 FROM runs WHERE id>?1 AND id>=1000",
                [previous],
            )
            .unwrap();
            let mut lookup = conn.prepare(FINDING_BY_REQUEST).unwrap();
            assert!(
                lookup
                    .query_row([9999999], |r| r.get::<_, i64>(0))
                    .optional()
                    .unwrap()
                    .is_none()
            );
            steps.push(lookup.get_status(StatementStatus::VmStep));
        }
        assert_eq!(steps[0], steps[1]);
        assert!(steps[1] < 100);
        conn.execute_batch("CREATE TABLE unindexed_findings AS SELECT * FROM assistance_findings;
            DROP TABLE assistance_findings; ALTER TABLE unindexed_findings RENAME TO assistance_findings;").unwrap();
        let mut lookup = conn.prepare(FINDING_BY_REQUEST).unwrap();
        assert!(
            lookup
                .query_row([9999999], |r| r.get::<_, i64>(0))
                .optional()
                .unwrap()
                .is_none()
        );
        assert!(lookup.get_status(StatementStatus::VmStep) > 100000);
    }

    #[test]
    fn retained_findings_revalidate_current_authority_and_original_sources() {
        for invalid in [
            "none",
            "ordinary_turn",
            "private_turn",
            "private_source",
            "revoked_source",
            "expired_source",
            "stale_source",
            "wrong_source",
            "stopped",
            "disabled",
            "revision",
            "paused_member",
            "deleted_scope",
            "inactive_project",
            "cancelled_notice",
            "wrong_member",
            "wrong_origin",
            "wrong_project",
        ] {
            let store = SqliteTeamStore::in_memory().unwrap();
            finding_fixture(&store);
            let mut run = RunId(2);
            let mut sources = vec![SourceRef::Document(1)];
            let change = match invalid {
                "ordinary_turn" => {
                    run = RunId(1);
                    "SELECT 1;"
                }
                "private_turn" => "INSERT INTO run_confidential_inputs(run) VALUES(2);",
                "private_source" => "INSERT INTO run_confidential_inputs(run) VALUES(1);",
                "revoked_source" => "DELETE FROM assistance_source_grants;",
                "expired_source" => "UPDATE assistance_source_grants SET expires_at=49;",
                "stale_source" => "UPDATE assistance_source_grants SET revision=41;",
                "wrong_source" => {
                    sources = vec![SourceRef::Document(2)];
                    "SELECT 1;"
                }
                "stopped" => "INSERT INTO settings(key,value) VALUES('stopped','1');",
                "disabled" => "UPDATE settings SET value='0:2' WHERE key='conversations_enabled';",
                "revision" => "UPDATE settings SET value='1:2' WHERE key='conversations_enabled';",
                "paused_member" => "UPDATE members SET paused=1 WHERE id=2;",
                "deleted_scope" => "UPDATE conversations SET deleted_at=1;",
                "inactive_project" => "UPDATE projects SET status='paused';",
                "cancelled_notice" => "UPDATE assistance_requests SET notice_cancelled=1;",
                "wrong_member" => "UPDATE runs SET member=1 WHERE id=2;",
                "wrong_origin" => "UPDATE runs SET member=1 WHERE id=1;",
                "wrong_project" => {
                    "INSERT INTO projects(id,goal,title,reason,lead,status,created_by,created_at,updated_at) VALUES(2,1,'Other fixture','',1,'active','owner',0,0); UPDATE assistance_requests SET project=2;"
                }
                _ => "SELECT 1;",
            };
            store.conn().unwrap().execute_batch(change).unwrap();
            let recorded = store
                .record_assistance_finding(
                    run,
                    "The original supports this limited finding.",
                    &sources,
                    50,
                )
                .unwrap();
            assert_eq!(
                recorded.is_ok(),
                invalid == "none",
                "{invalid}: {recorded:?}"
            );
            assert_eq!(
                store.assistance_finding(1).unwrap().is_some(),
                invalid == "none"
            );
            assert_eq!(
                store
                    .conn()
                    .unwrap()
                    .query_row("SELECT count(*) FROM wakes", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                2
            );
            assert_eq!(
                store
                    .conn()
                    .unwrap()
                    .query_row("SELECT status FROM tasks WHERE id=1", [], |r| r
                        .get::<_, String>(0))
                    .unwrap(),
                "doing"
            );
        }
    }

    fn finding_fixture(store: &SqliteTeamStore) {
        fixture(store);
        store.conn().unwrap().execute_batch("INSERT INTO settings(key,value) VALUES('conversations_enabled','1:1');
            INSERT INTO tasks(id,goal,title,description,status,priority,depth,project,owner)
            VALUES(1,1,'Fixture','','doing','normal',0,1,2);
            UPDATE runs SET task=1 WHERE id=1;
            INSERT INTO wakes(id,member,reason,receipt,at_ms) VALUES(2,2,'question_answered','started',42);
            INSERT INTO runs(id,member,wake,status,queued_at) VALUES(2,2,2,'running',42);
            INSERT INTO conversation_ready_runs(run,revision) VALUES(2,1);
            INSERT INTO conversations(id,project,member1,member2,at_ms) VALUES(1,1,1,2,42);
            INSERT INTO documents(id,task,run,title,text,at_ms) VALUES(1,1,1,'Fixture','Original evidence',42);
            INSERT INTO assistance_source_grants(conversation,kind,resource,revision,granted_at,expires_at)
            VALUES(1,'d',1,42,42,1000);
            INSERT INTO assistance_requests(id,conversation,project,requester,requester_run,expert,client_key,
                objective,evidence,accepted_day,accepted_at,deadline_at,status,answer,notified,notice_run)
            VALUES(1,1,1,2,1,1,'fixture_request','Fixture objective','d1',0,42,1000,'answered','Peer testimony',1,2);").unwrap();
    }

    #[test]
    fn finding_retries_are_immutable_and_do_not_regrant_an_original() {
        let store = SqliteTeamStore::in_memory().unwrap();
        finding_fixture(&store);
        let sources = [SourceRef::Document(1)];
        let finding = "A limited original-based finding.";
        assert_eq!(
            store
                .record_assistance_finding(RunId(2), finding, &sources, 50)
                .unwrap(),
            Ok(1)
        );
        store
            .conn()
            .unwrap()
            .execute_batch("DELETE FROM assistance_source_grants;")
            .unwrap();
        assert_eq!(
            store
                .record_assistance_finding(RunId(2), finding, &sources, 60)
                .unwrap(),
            Ok(1)
        );
        assert!(
            store
                .record_assistance_finding(RunId(2), "A different finding.", &sources, 60)
                .unwrap()
                .is_err()
        );
        assert_eq!(store.assistance_finding(1).unwrap().unwrap().at, 50);
        assert_eq!(store.assistance_finding(1).unwrap().unwrap().text, finding);
        assert_eq!(
            store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM conversation_operations WHERE operation='finding'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert!(!store.assistance_source_granted(1, sources[0], 60).unwrap());
    }

    #[test]
    fn scope_discovery_checks_membership_project_status_and_deletion() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        assert!(!store.has_active_conversation(MemberId(2), None).unwrap());
        let id = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        for member in [MemberId(1), MemberId(2)] {
            assert!(store.has_active_conversation(member, None).unwrap());
            assert!(
                store
                    .has_active_conversation(member, Some(ProjectId(1)))
                    .unwrap()
            );
            assert!(
                !store
                    .has_active_conversation(member, Some(ProjectId(2)))
                    .unwrap()
            );
        }
        assert!(!store.has_active_conversation(MemberId(3), None).unwrap());
        store
            .conn()
            .unwrap()
            .execute_batch("UPDATE projects SET status='paused';")
            .unwrap();
        assert!(!store.has_active_conversation(MemberId(2), None).unwrap());
        assert!(
            !store
                .has_active_conversation(MemberId(2), Some(ProjectId(1)))
                .unwrap()
        );
        store
            .conn()
            .unwrap()
            .execute_batch("UPDATE projects SET status='active';")
            .unwrap();
        store
            .conn()
            .unwrap()
            .execute("UPDATE conversations SET deleted_at=1 WHERE id=?1", [id])
            .unwrap();
        assert!(!store.has_active_conversation(MemberId(2), None).unwrap());
        assert!(
            !store
                .has_active_conversation(MemberId(2), Some(ProjectId(1)))
                .unwrap()
        );
    }

    #[test]
    fn scope_discovery_does_not_scan_inactive_project_or_deleted_conversation_archives() {
        use rusqlite::StatementStatus;
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        let conn = store.conn().unwrap();
        let mut steps = Vec::new();
        for size in [1_000, 100_000] {
            conn.execute_batch("DELETE FROM conversations; DELETE FROM projects WHERE id>=1000;")
                .unwrap();
            conn.execute("WITH RECURSIVE n(id) AS (VALUES(1000) UNION ALL SELECT id+1 FROM n WHERE id<999+?1)
                INSERT INTO projects(id,goal,title,reason,lead,status,created_by,created_at,updated_at)
                SELECT id,1,'Fixture','',1,'paused','owner',0,0 FROM n", [size]).unwrap();
            conn.execute(
                "INSERT INTO conversations(project,member1,member2,at_ms)
                SELECT id,1,2,0 FROM projects WHERE id>=1000",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO conversations(project,member1,member2,at_ms,deleted_at)
                SELECT 1,1,2,0,1 FROM projects WHERE id>=1000",
                [],
            )
            .unwrap();
            let mut sample = Vec::new();
            for sql in [ACTIVE_MEMBER_SCOPE, ACTIVE_TASK_SCOPE] {
                let mut query = conn.prepare(sql).unwrap();
                let mut params = vec![2];
                if sql == ACTIVE_TASK_SCOPE {
                    params.push(1);
                }
                assert!(
                    !query
                        .query_row(rusqlite::params_from_iter(params), |r| r.get::<_, bool>(0))
                        .unwrap()
                );
                sample.push(query.get_status(StatementStatus::VmStep));
            }
            steps.push(sample);
        }
        assert_eq!(steps[0], steps[1]);
        assert!(steps[1].iter().all(|steps| *steps < 1000));
        conn.execute_batch("DROP INDEX conversations_member1_project; DROP INDEX conversations_member2_project; DROP INDEX conversations_active_projects;").unwrap();
        let mut unindexed = conn.prepare(ACTIVE_MEMBER_SCOPE).unwrap();
        assert!(!unindexed.query_row([2], |r| r.get::<_, bool>(0)).unwrap());
        assert!(
            unindexed.get_status(StatementStatus::VmStep) > 100_000,
            "negative control must scan the retained archive"
        );
    }

    #[test]
    fn task_help_wait_tracks_only_the_original_task_and_unfinished_delivery() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        store.conn().unwrap().execute_batch("INSERT INTO tasks(id,goal,title,description,status,priority,depth,project,owner) VALUES(1,1,'Fixture','','doing','normal',0,1,2),(2,1,'Other fixture','','doing','normal',0,1,2);
            UPDATE runs SET task=1 WHERE id=1;
            INSERT INTO conversations(id,project,member1,member2,at_ms) VALUES(1,1,1,2,0);
            INSERT INTO assistance_requests(id,conversation,project,requester,requester_run,expert,client_key,objective,evidence,accepted_day,accepted_at,deadline_at)
            VALUES(1,1,1,2,1,1,'fixture-key','Fixture','',0,0,1000);").unwrap();
        for status in ["queued", "processing", "answered"] {
            store
                .conn()
                .unwrap()
                .execute("UPDATE assistance_requests SET status=?1", [status])
                .unwrap();
            assert!(
                store
                    .task_awaits_assistance(TaskId(1), MemberId(2))
                    .unwrap(),
                "{status}"
            );
            assert!(
                !store
                    .task_awaits_assistance(TaskId(2), MemberId(2))
                    .unwrap()
            );
            assert!(
                !store
                    .task_awaits_assistance(TaskId(1), MemberId(1))
                    .unwrap()
            );
        }
        for status in ["cancelled", "expired", "failed", "declined"] {
            store
                .conn()
                .unwrap()
                .execute("UPDATE assistance_requests SET status=?1", [status])
                .unwrap();
            assert!(
                !store
                    .task_awaits_assistance(TaskId(1), MemberId(2))
                    .unwrap(),
                "{status}"
            );
        }
        store.conn().unwrap().execute_batch("INSERT INTO wakes(id,member,reason,receipt,at_ms) VALUES(3,2,'question_answered','queued',0);
            INSERT INTO runs(id,member,wake,status,queued_at) VALUES(3,2,3,'queued',0);
            UPDATE assistance_requests SET status='answered',notified=1,notice_run=3;").unwrap();
        for status in ["queued", "running", "retry_waiting"] {
            store
                .conn()
                .unwrap()
                .execute("UPDATE runs SET status=?1 WHERE id=3", [status])
                .unwrap();
            assert!(
                store
                    .task_awaits_assistance(TaskId(1), MemberId(2))
                    .unwrap(),
                "{status}"
            );
        }
        store
            .conn()
            .unwrap()
            .execute_batch("UPDATE runs SET status='succeeded' WHERE id=3;")
            .unwrap();
        assert!(
            !store
                .task_awaits_assistance(TaskId(1), MemberId(2))
                .unwrap()
        );
        store.conn().unwrap().execute_batch("UPDATE runs SET status='running' WHERE id=3; UPDATE assistance_requests SET notice_cancelled=1;").unwrap();
        assert!(
            !store
                .task_awaits_assistance(TaskId(1), MemberId(2))
                .unwrap()
        );
        store.conn().unwrap().execute_batch("UPDATE assistance_requests SET notice_cancelled=0,notified=0,notice_run=NULL,status='queued'; UPDATE runs SET task=NULL WHERE id=1;").unwrap();
        assert!(
            !store
                .task_awaits_assistance(TaskId(1), MemberId(2))
                .unwrap(),
            "scope alone cannot invent a task association"
        );
    }

    #[test]
    fn task_help_wait_does_not_scan_completed_request_or_run_archives() {
        use rusqlite::StatementStatus;
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        let conn = store.conn().unwrap();
        conn.execute_batch("INSERT INTO tasks(id,goal,title,description,status,priority,depth,project,owner) VALUES(1,1,'Fixture','','doing','normal',0,1,2),(2,1,'Other fixture','','doing','normal',0,1,2);
            UPDATE runs SET task=1 WHERE id=1;
            INSERT INTO conversations(id,project,member1,member2,at_ms) VALUES(1,1,1,2,0);
            INSERT INTO assistance_requests(id,conversation,project,requester,requester_run,expert,client_key,objective,evidence,accepted_day,accepted_at,deadline_at)
            VALUES(1,1,1,2,1,1,'fixture-key','Fixture','',0,0,1000);").unwrap();
        let mut steps = Vec::new();
        for size in [1_000, 100_000] {
            conn.execute("DELETE FROM assistance_requests WHERE id>=1000", [])
                .unwrap();
            conn.execute("DELETE FROM runs WHERE id>=1000", []).unwrap();
            conn.execute("WITH RECURSIVE n(id) AS (VALUES(1000) UNION ALL SELECT id+1 FROM n WHERE id<999+?1)
                INSERT INTO runs(id,member,wake,status,queued_at) SELECT id,2,1,'succeeded',0 FROM n", [size]).unwrap();
            conn.execute("WITH RECURSIVE n(id) AS (VALUES(1000) UNION ALL SELECT id+1 FROM n WHERE id<999+?1)
                INSERT INTO assistance_requests(id,conversation,project,requester,requester_run,expert,client_key,objective,evidence,accepted_day,accepted_at,deadline_at,status,notified,notice_run)
                SELECT id,1,1,2,1,1,'archive_'||id,'Fixture','',0,0,1000,'answered',1,id FROM n", [size]).unwrap();
            let mut query = conn.prepare(TASK_AWAITS_ASSISTANCE).unwrap();
            assert!(!query.query_row([2, 2], |r| r.get::<_, bool>(0)).unwrap());
            steps.push(query.get_status(StatementStatus::VmStep));
        }
        assert_eq!(steps[0], steps[1]);
        assert!(steps[1] < 1_000);
        conn.execute_batch("DROP INDEX assistance_waiting_task; DROP INDEX assistance_unnotified; DROP INDEX assistance_pending; DROP INDEX assistance_live_member_run;").unwrap();
        let mut unindexed = conn.prepare(TASK_AWAITS_ASSISTANCE).unwrap();
        assert!(
            !unindexed
                .query_row([2, 2], |r| r.get::<_, bool>(0))
                .unwrap()
        );
        assert!(
            unindexed.get_status(StatementStatus::VmStep) > 100_000,
            "negative control must scan the archive"
        );
    }

    #[test]
    fn task_project_changes_cannot_race_assistance_admission() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        store.conn().unwrap().execute_batch("INSERT INTO tasks(id,goal,title,description,status,priority,depth,project) VALUES(1,1,'Fixture','','ready','normal',0,1);
            UPDATE runs SET task=1 WHERE id=1;").unwrap();
        let conversation = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        let request = NewRequest {
            conversation,
            project: ProjectId(1),
            requester: MemberId(2),
            requester_run: RunId(1),
            expert: MemberId(1),
            key: "task_scope_race_1".into(),
            objective: "Fixture outcome?".into(),
            evidence: vec![],
            day: 0,
            day_start: 0,
            at: 0,
        };
        assert_eq!(
            store
                .task(crate::domain::TaskId(1))
                .unwrap()
                .unwrap()
                .project,
            Some(request.project)
        );
        // Simulate an edit after application preflight, before the admission transaction.
        store
            .conn()
            .unwrap()
            .execute("UPDATE tasks SET project=NULL WHERE id=1", [])
            .unwrap();
        assert!(store.accept_assistance(&request).unwrap().is_err());
        assert!(store.assistance_page(conversation, 0).unwrap().is_empty());
        store
            .conn()
            .unwrap()
            .execute("UPDATE tasks SET project=1 WHERE id=1", [])
            .unwrap();
        assert!(
            store.accept_assistance(&request).unwrap().is_ok(),
            "a refusal must not consume admission or the retry key"
        );
    }

    #[test]
    fn task_assistance_replay_cannot_attach_a_request_to_another_task() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        store.conn().unwrap().execute_batch("INSERT INTO tasks(id,goal,title,description,status,priority,depth,project) VALUES(1,1,'First','','ready','normal',0,1),(2,1,'Other','','ready','normal',0,1);
            UPDATE runs SET task=1 WHERE id=1;
            INSERT INTO wakes(id,member,task,reason,receipt,at_ms) VALUES(2,2,2,'assigned','queued',0),(3,2,1,'assigned','queued',0);
            INSERT INTO runs(id,member,task,wake,status,queued_at) VALUES(2,2,2,2,'running',0),(3,2,1,3,'running',0);").unwrap();
        let conversation = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        let mut request = NewRequest {
            conversation,
            project: ProjectId(1),
            requester: MemberId(2),
            requester_run: RunId(1),
            expert: MemberId(1),
            key: "task_scope_replay_1".into(),
            objective: "Fixture outcome?".into(),
            evidence: vec![],
            day: 0,
            day_start: 0,
            at: 0,
        };
        let accepted = store.accept_assistance(&request).unwrap().unwrap();
        request.requester_run = RunId(2);
        assert!(
            store.accept_assistance(&request).unwrap().is_err(),
            "the same question and key cannot bind a different task to the old request"
        );
        request.requester_run = RunId(3);
        assert_eq!(
            store.accept_assistance(&request).unwrap().unwrap(),
            accepted,
            "a later turn on the original task may retry its accepted request"
        );
        assert_eq!(
            store
                .assistance_request(accepted)
                .unwrap()
                .unwrap()
                .requester_run,
            RunId(1)
        );
    }

    #[test]
    fn canonical_requests_share_the_last_turn_allowance_with_retained_legacy_usage() {
        use crate::domain::consult::{Consult, Status};
        let folder = std::path::Path::new("target/consult-recovery");
        std::fs::create_dir_all(folder).unwrap();
        let path = folder.join(format!("{}-admission.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let first = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
        fixture(&first);
        first.conn().unwrap().execute_batch("INSERT INTO tasks(id,goal,title,description,status,priority,depth,project) VALUES(1,1,'Fixture','','ready','normal',0,1);").unwrap();
        let conversation = first
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        let intent = Consult {
            id: 0,
            asker: MemberId(2),
            asker_run: RunId(1),
            expert: MemberId(1),
            topic: Some(crate::domain::TaskId(1)),
            thread: crate::domain::TaskId(1),
            question: "Earlier fixture?".into(),
            status: Status::Answered,
            answer_run: None,
            why: String::new(),
            asker_woken: true,
            at: 0,
        };
        first.seed_legacy_consultation(&intent).unwrap();
        first.conn().unwrap().execute("INSERT INTO members(id,name,role,share_percent,slots) VALUES(3,'Member 3','developer',30,2)", []).unwrap();
        let other_conversation = first
            .add_conversation(ProjectId(1), [MemberId(2), MemberId(3)], 0)
            .unwrap();
        let request = NewRequest {
            conversation,
            project: ProjectId(1),
            requester: MemberId(2),
            requester_run: RunId(1),
            expert: MemberId(1),
            key: "shared_race_1".into(),
            objective: "Other fixture?".into(),
            evidence: vec![],
            day: 0,
            day_start: 0,
            at: 0,
        };
        let other_request = NewRequest {
            conversation: other_conversation,
            expert: MemberId(3),
            key: "shared_race_2".into(),
            project: request.project,
            requester: request.requester,
            requester_run: request.requester_run,
            objective: request.objective.clone(),
            evidence: request.evidence.clone(),
            day: request.day,
            day_start: request.day_start,
            at: request.at,
        };
        let second = SqliteTeamStore::open(path.to_str().unwrap()).unwrap();
        for store in [&first, &second] {
            store
                .conn()
                .unwrap()
                .busy_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }
        let barrier = std::sync::Barrier::new(2);
        let accepted = std::thread::scope(|scope| {
            let first_request = scope.spawn(|| {
                barrier.wait();
                first.accept_assistance(&request).unwrap().is_ok()
            });
            let conversation = scope.spawn(|| {
                barrier.wait();
                second.accept_assistance(&other_request).unwrap().is_ok()
            });
            [first_request.join().unwrap(), conversation.join().unwrap()]
        });
        assert_eq!(accepted.into_iter().filter(|accepted| *accepted).count(), 1);
        if accepted[0] {
            first.accept_assistance(&request).unwrap().unwrap();
        }
        if accepted[1] {
            second.accept_assistance(&other_request).unwrap().unwrap();
        }
        assert_eq!(
            super::super::assistance::consultation_usage(
                &first.conn().unwrap(),
                RunId(1),
                MemberId(2),
                0,
                0
            )
            .unwrap(),
            (2, 2)
        );
        drop(first);
        drop(second);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn consult_pair_lookup_is_bounded_as_unrelated_conversations_grow() {
        let mut observed = Vec::new();
        for size in [1_000, 100_000] {
            let store = SqliteTeamStore::in_memory().unwrap();
            fixture(&store);
            store.conn().unwrap().execute("INSERT INTO members(id,name,role,share_percent,slots) VALUES(3,'Member 3','developer',30,2)", []).unwrap();
            let selected = store
                .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
                .unwrap();
            store
                .conn()
                .unwrap()
                .execute_batch(&format!(
                    "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<{size})
                INSERT INTO conversations(project,member1,member2,at_ms)
                SELECT 1, CASE x%4 WHEN 0 THEN 1 WHEN 1 THEN 2 ELSE 3 END,
                          CASE x%4 WHEN 0 THEN 3 WHEN 1 THEN 3 WHEN 2 THEN 1 ELSE 2 END, 0 FROM n"
                ))
                .unwrap();
            let measure = || {
                let conn = store.conn().unwrap();
                let mut stmt = conn.prepare(PAIR_SCOPES).unwrap();
                let ids = stmt
                    .query_map([1, 2], |row| row.get::<_, i64>(0))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                assert_eq!(ids, [selected]);
                stmt.get_status(StatementStatus::VmStep)
            };
            observed.push(measure());
            assert_eq!(
                store.conversation_pair(MemberId(2), MemberId(1)).unwrap()[0].id,
                selected
            );
            if size == 100_000 {
                store
                    .conn()
                    .unwrap()
                    .execute_batch("DROP INDEX conversations_pair")
                    .unwrap();
                assert!(
                    measure() > 10_000,
                    "negative control must detect the missing composite index"
                );
            }
        }
        assert_eq!(observed[0], observed[1]);
        assert!(observed[1] < 1_000, "{observed:?}");
    }

    fn information(conversation: i64) -> Information {
        Information {
            conversation,
            member: MemberId(2),
            run: RunId(1),
            key: "message_key_1".into(),
            text: "Fixture evidence".into(),
            evidence: Vec::new(),
            day: 42,
            at: 42,
        }
    }

    #[test]
    fn assistance_limits_count_new_requests_even_after_cancellation_and_allow_idempotent_replay() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        let conversation = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        for run in 1..=4 {
            if run > 1 {
                store.conn().unwrap().execute("INSERT INTO runs(id,member,wake,status,queued_at) VALUES(?1,2,1,'running',0)",[run]).unwrap();
            }
            for n in 0..3 {
                let intent = NewRequest {
                    conversation,
                    project: ProjectId(1),
                    requester: MemberId(2),
                    requester_run: RunId(run),
                    expert: MemberId(1),
                    key: format!("request_{run}_{n}"),
                    objective: "Synthetic question?".into(),
                    evidence: Vec::new(),
                    day: 42,
                    day_start: 0,
                    at: 42,
                };
                let accepted = store.accept_assistance(&intent).unwrap();
                if run <= 3 && n < 2 {
                    let id = accepted.unwrap();
                    store
                        .cancel_assistance_request(id, Some(RunId(run)), 43)
                        .unwrap()
                        .unwrap();
                    assert_eq!(store.accept_assistance(&intent).unwrap().unwrap(), id);
                    continue;
                }
                assert!(
                    accepted.is_err(),
                    "new requests beyond two/turn or six/day must be refused"
                );
            }
        }
        assert_eq!(store.assistance_page(conversation, 0).unwrap().len(), 6);
    }

    #[test]
    fn original_grants_are_immutable_bounded_revocable_and_scoped_during_purge() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        store.conn().unwrap().execute_batch("INSERT INTO tasks(id,goal,title,description,status,priority,depth,project) VALUES(1,1,'Fixture','','ready','normal',0,1);
            UPDATE runs SET task=1 WHERE id=1;
            INSERT INTO documents(id,task,run,title,text,at_ms) VALUES(1,1,1,'Fixture','Original',42),(2,1,1,'Fixture','Original',42),(3,1,1,'Fixture','Original',42),(4,1,1,'Fixture','Original',42);
            INSERT INTO call_receipts(id,run,member,tool,args,excerpt,ok,act,confidential,at_ms) VALUES(1,1,2,'fixture_check','','Original',1,0,0,42),(2,1,2,'fixture_check','','Original',1,0,0,42);").unwrap();
        let scope = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        for id in 1..=4 {
            assert!(
                store
                    .grant_assistance_source(scope, SourceRef::Document(id), 42)
                    .unwrap()
            );
        }
        assert!(
            store
                .grant_assistance_source(scope, SourceRef::Receipt(1), 42)
                .unwrap()
        );
        assert!(
            !store
                .grant_assistance_source(scope, SourceRef::Receipt(2), 42)
                .unwrap()
        );
        assert!(
            store
                .grant_assistance_source(scope, SourceRef::Document(1), 42)
                .unwrap(),
            "renewal does not consume a sixth slot"
        );
        assert_eq!(store.assistance_grants(scope, 42).unwrap().len(), 5);
        assert!(
            store
                .conn()
                .unwrap()
                .execute("UPDATE documents SET text='Rewritten' WHERE id=1", [])
                .is_err()
        );
        let other = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        assert!(
            store
                .grant_assistance_source(other, SourceRef::Document(1), 42)
                .unwrap()
        );
        store.delete_conversation(scope, 43).unwrap();
        store.purge_conversation_batch().unwrap();
        assert!(
            store
                .assistance_source_granted(other, SourceRef::Document(1), 44)
                .unwrap(),
            "purge must not revoke another conversation's grant to the same original"
        );
        store
            .revoke_assistance_source(other, SourceRef::Document(1))
            .unwrap();
        assert!(
            !store
                .assistance_source_granted(other, SourceRef::Document(1), 44)
                .unwrap()
        );
        store
            .grant_assistance_source(other, SourceRef::Receipt(2), 42)
            .unwrap();
        assert!(
            !store
                .assistance_source_granted(other, SourceRef::Receipt(2), 42 + assistance::GRANT_MS)
                .unwrap()
        );
    }

    #[test]
    fn recovery_closes_an_expert_that_ended_before_its_request_was_updated() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        let conversation = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        let request = NewRequest {
            conversation,
            project: ProjectId(1),
            requester: MemberId(2),
            requester_run: RunId(1),
            expert: MemberId(1),
            key: "request_key_1".into(),
            objective: "Synthetic question?".into(),
            evidence: Vec::new(),
            day: 42,
            day_start: 0,
            at: 42,
        };
        let id = store.accept_assistance(&request).unwrap().unwrap();
        let expert = store.queue_assistance(id, 43).unwrap().unwrap();
        store
            .conn()
            .unwrap()
            .execute(
                "UPDATE runs SET status='succeeded',ended_at=44 WHERE id=?1",
                [expert.0],
            )
            .unwrap();
        store.expire_assistance(45).unwrap();
        assert_eq!(
            store.assistance_for_run(expert).unwrap().unwrap().status,
            "declined"
        );
        assert!(
            store.queue_assistance(id, 46).unwrap().is_none(),
            "reconciliation never reopens a terminal run"
        );
    }

    #[test]
    fn assistance_reopens_claims_once_and_charges_expert_and_result_to_the_fixed_project() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let folder = std::path::Path::new("target/conversation-tests");
        std::fs::create_dir_all(folder).unwrap();
        let path = folder.join(format!(
            "assistance-{}-{}.db",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let path = path.to_str().unwrap();
        let id;
        let conversation;
        {
            let store = SqliteTeamStore::open(path).unwrap();
            fixture(&store);
            conversation = store
                .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
                .unwrap();
            let intent = NewRequest {
                conversation,
                project: ProjectId(1),
                requester: MemberId(2),
                requester_run: RunId(1),
                expert: MemberId(1),
                key: "request_key_1".into(),
                objective: "Synthetic question?".into(),
                evidence: Vec::new(),
                day: 42,
                day_start: 0,
                at: 42,
            };
            id = store.accept_assistance(&intent).unwrap().unwrap();
            assert_eq!(store.accept_assistance(&intent).unwrap().unwrap(), id);
        }
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = [
            SqliteTeamStore::open(path).unwrap(),
            SqliteTeamStore::open(path).unwrap(),
        ]
        .into_iter()
        .map(|store| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.queue_assistance(id, 43).unwrap()
            })
        })
        .collect();
        let runs: Vec<_> = handles
            .into_iter()
            .filter_map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(
            runs.len(),
            1,
            "independent concurrent delivery claims must associate one run"
        );
        let expert = runs[0];
        let store = SqliteTeamStore::open(path).unwrap();
        assert_eq!(store.assistance_for_run(expert).unwrap().unwrap().id, id);
        assert_eq!(
            store
                .accept_assistance_answer(expert, "Synthetic answer", &[], 44)
                .unwrap()
                .unwrap(),
            id
        );
        drop(store);
        let store = SqliteTeamStore::open(path).unwrap();
        assert_eq!(
            store
                .accept_assistance_answer(expert, "Synthetic answer", &[], 45)
                .unwrap()
                .unwrap(),
            id
        );
        assert!(
            store
                .accept_assistance_answer(expert, "Different answer", &[], 45)
                .unwrap()
                .is_err()
        );
        assert!(
            store.queue_assistance_notice(id, 46).unwrap().is_none(),
            "a running requester is not interrupted"
        );
        store
            .conn()
            .unwrap()
            .execute("UPDATE runs SET status='succeeded' WHERE id=1", [])
            .unwrap();
        let notice = store.queue_assistance_notice(id, 47).unwrap().unwrap();
        assert!(store.queue_assistance_notice(id, 47).unwrap().is_none());
        for (run, member, amount) in [(expert, MemberId(1), 20), (notice, MemberId(2), 10)] {
            store.reserve(run, member, 42, 100, 48).unwrap();
            store.record_use(run, member, 42, amount, 49).unwrap();
            store.settle(run, member, 42, 50).unwrap();
        }
        store.record_use(expert, MemberId(1), 42, 5, 51).unwrap();
        store.settle(expert, MemberId(1), 42, 52).unwrap();
        assert_eq!(store.project_spent(ProjectId(1), 0, 100).unwrap(), 35);
        let unscoped: i64 = store
            .conn()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM ledger WHERE project IS NULL OR project!=1 OR owner!=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            unscoped, 0,
            "every reserve, use, charge, release and late charge keeps the trusted project"
        );
        let intent = NewRequest {
            conversation,
            project: ProjectId(1),
            requester: MemberId(2),
            requester_run: RunId(1),
            expert: MemberId(1),
            key: "request_key_2".into(),
            objective: "Another synthetic question?".into(),
            evidence: Vec::new(),
            day: 42,
            day_start: 0,
            at: 60,
        };
        let racing = store.accept_assistance(&intent).unwrap().unwrap();
        let racing_run = store.queue_assistance(racing, 61).unwrap().unwrap();
        drop(store);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = [true, false]
            .into_iter()
            .map(|cancel| {
                let store = SqliteTeamStore::open(path).unwrap();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    if cancel {
                        return store
                            .cancel_assistance_request(racing, None, 62)
                            .unwrap()
                            .is_ok();
                    }
                    store
                        .accept_assistance_answer(racing_run, "Racing answer", &[], 62)
                        .unwrap()
                        .is_ok()
                })
            })
            .collect();
        let winners = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|accepted| *accepted)
            .count();
        assert_eq!(
            winners, 1,
            "answer and cancellation have one transactional winner"
        );
        let store = SqliteTeamStore::open(path).unwrap();
        let outcome = store.assistance_request(racing).unwrap().unwrap();
        assert!(matches!(outcome.status.as_str(), "answered" | "cancelled"));
        assert_eq!(
            store.pending_assistance_notices().unwrap().len(),
            usize::from(outcome.status == "answered"),
            "only an answer that won may create a result-delivery intent"
        );
        store
            .conn()
            .unwrap()
            .execute("UPDATE runs SET status='succeeded'", [])
            .unwrap();
        store.delete_conversation(conversation, 63).unwrap();
        store.purge_conversation_batch().unwrap();
        assert!(store.assistance_for_run(expert).unwrap().is_none());
        assert_eq!(
            store.assistance_charging_project(expert).unwrap(),
            Some(ProjectId(1)),
            "owner deletion cannot erase the payer of an already charged run"
        );
        assert_eq!(
            store.assistance_charging_project(notice).unwrap(),
            Some(ProjectId(1))
        );
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_message_and_its_retry_survive_an_independent_store_reopen() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let folder = std::path::Path::new("target/conversation-tests");
        std::fs::create_dir_all(folder).unwrap();
        let path = folder.join(format!(
            "{}-{}.db",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let path = path.to_str().unwrap();
        let id;
        let message;
        {
            let store = SqliteTeamStore::open(path).unwrap();
            fixture(&store);
            id = store
                .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
                .unwrap();
            message = store.accept_information(&information(id)).unwrap().unwrap();
            store.mark_confidential_input(RunId(1)).unwrap();
        }
        {
            let store = SqliteTeamStore::open(path).unwrap();
            assert!(store.run_read_confidential(RunId(1)).unwrap());
            assert_eq!(
                store.accept_information(&information(id)).unwrap().unwrap(),
                message
            );
            assert_eq!(store.message_page(id, 0).unwrap().len(), 1);
            assert_eq!(store.conversation(id).unwrap().unwrap().messages, 1);
            assert!(
                store
                    .conn()
                    .unwrap()
                    .execute("UPDATE conversation_messages SET text='changed'", [])
                    .is_err()
            );
        }
        // Separate connections accept the same previously unseen key concurrently: exactly
        // one immutable message and one summary increment survive both successful responses.
        let left = SqliteTeamStore::open(path).unwrap();
        let right = SqliteTeamStore::open(path).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = [left, right]
            .into_iter()
            .map(|store| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut next = information(id);
                    next.key = "concurrent_key_1".into();
                    barrier.wait();
                    store.accept_information(&next).unwrap().unwrap()
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results[0], results[1]);
        let store = SqliteTeamStore::open(path).unwrap();
        assert_eq!(store.message_page(id, 0).unwrap().len(), 2);
        assert_eq!(store.conversation(id).unwrap().unwrap().messages, 2);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn daily_limit_applies_across_runs_and_does_not_charge_a_retry() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        let id = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        for run in 1..=6 {
            if run > 1 {
                store.conn().unwrap().execute("INSERT INTO runs(id,member,wake,status,queued_at) VALUES(?1,2,1,'running',0)",[run]).unwrap();
            }
            for n in 0..12 {
                let mut next = information(id);
                next.run = RunId(run);
                next.key = format!("day_key_{run}_{n}");
                let accepted = store.accept_information(&next).unwrap();
                if run <= 5 {
                    assert!(accepted.is_ok());
                } else {
                    assert!(accepted.is_err());
                }
            }
        }
        let mut retry = information(id);
        retry.key = "day_key_1_0".into();
        assert!(store.accept_information(&retry).unwrap().is_ok());
        store.delete_conversation(id, 42).unwrap();
        while store.purge_conversation_batch().unwrap() > 0 {}
        retry.conversation = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 42)
            .unwrap();
        retry.run = RunId(6);
        retry.key = "after_deletion_key".into();
        assert!(
            store.accept_information(&retry).unwrap().is_err(),
            "owner deletion must not reset that member's admission limit"
        );
        retry.key = "following_day_key".into();
        retry.run = RunId(6);
        retry.day += 1;
        assert!(store.accept_information(&retry).unwrap().is_ok());
    }

    fn page_work(store: &SqliteTeamStore, conversation: i64) -> i32 {
        let conn = store.conn().unwrap();
        let mut stmt = conn.prepare(MESSAGES_PAGE).unwrap();
        let rows = stmt
            .query_map(params![conversation, 0], |r| r.get::<_, i64>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(rows.len(), 51);
        stmt.get_status(StatementStatus::VmStep)
    }

    fn request_query_work(store: &SqliteTeamStore, suffix: &str, parameters: &[i64]) -> i32 {
        let conn = store.conn().unwrap();
        let mut stmt = conn
            .prepare(&format!("SELECT {REQUEST_COLUMNS}{suffix}"))
            .unwrap();
        let _ = stmt
            .query_map(rusqlite::params_from_iter(parameters), |r| {
                r.get::<_, i64>(0)
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        stmt.get_status(StatementStatus::VmStep)
    }

    #[test]
    fn retained_assistance_does_not_expand_delivery_notification_or_page_work() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        let scope = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        let archive = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        let insert="INSERT INTO assistance_requests(conversation,project,requester,requester_run,expert,client_key,objective,evidence,accepted_day,accepted_at,deadline_at,status,notified)
            WITH RECURSIVE n(x) AS (VALUES(?2) UNION ALL SELECT x+1 FROM n WHERE x<?3)
            SELECT CASE WHEN x%2=0 THEN ?1 ELSE ?4 END,1,2,1,1,printf('archive_%d',x),'Old fixture','',0,0,1000000,'answered',1 FROM n";
        store
            .conn()
            .unwrap()
            .execute(insert, params![archive, 1, 1000, scope])
            .unwrap();
        store.conn().unwrap().execute("INSERT INTO assistance_requests(conversation,project,requester,requester_run,expert,client_key,objective,evidence,accepted_day,accepted_at,deadline_at,status,notified)
            VALUES(?1,1,2,1,1,'current_request_1','Current fixture','',42,42,1000000,'queued',0),
                  (?1,1,2,1,1,'current_request_2','Current fixture','',42,42,1000000,'answered',0)",[scope]).unwrap();
        let queries = [
            (DUE_REQUESTS, vec![42]),
            (PENDING_NOTICES, vec![]),
            (REQUEST_PAGE, vec![scope, 0]),
        ];
        let before: Vec<_> = queries
            .iter()
            .map(|(suffix, parameters)| request_query_work(&store, suffix, parameters))
            .collect();
        store
            .conn()
            .unwrap()
            .execute(insert, params![archive, 1001, 100000, scope])
            .unwrap();
        for ((suffix, parameters), small) in queries.iter().zip(before) {
            let large = request_query_work(&store, suffix, parameters);
            eprintln!("assistance SQL work: {small} -> {large}: {suffix}");
            assert!(
                large < 5000 && large <= small + 100,
                "retained assistance expanded routine work: {small} -> {large}: {suffix}"
            );
        }
        store
            .conn()
            .unwrap()
            .execute("DROP INDEX assistance_pending", [])
            .unwrap();
        store
            .conn()
            .unwrap()
            .execute("DROP INDEX assistance_unnotified", [])
            .unwrap();
        assert!(
            request_query_work(&store, DUE_REQUESTS, &[42]) > 5000,
            "the archive-work check must catch a missing delivery index"
        );
        assert!(
            request_query_work(&store, PENDING_NOTICES, &[]) > 5000,
            "the archive-work check must catch a missing result-notification index"
        );
    }

    #[test]
    fn historical_archive_keeps_real_message_pages_bounded_and_a_missing_index_is_detected() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        let current = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        let old = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        {
            let mut conn = store.conn().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute("INSERT INTO conversation_messages(conversation,member,run,client_key,text,evidence,accepted_day,at_ms)
                WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000)
                SELECT CASE WHEN x%2=0 THEN ?2 ELSE ?1 END,2,1,printf('old_%d',x),'Old fixture','',0,0 FROM n",params![old,current]).unwrap();
            tx.execute("INSERT INTO conversation_messages(conversation,member,run,client_key,text,evidence,accepted_day,at_ms)
                WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<51)
                SELECT ?1,2,1,printf('current_%d',x),'Current fixture','',42,42 FROM n",[current]).unwrap();
            tx.commit().unwrap();
        }
        let small = page_work(&store, current);
        {
            let mut conn = store.conn().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute("INSERT INTO conversation_messages(conversation,member,run,client_key,text,evidence,accepted_day,at_ms)
                WITH RECURSIVE n(x) AS (VALUES(1001) UNION ALL SELECT x+1 FROM n WHERE x<100000)
                SELECT CASE WHEN x%2=0 THEN ?2 ELSE ?1 END,2,1,printf('old_%d',x),'Old fixture','',0,0 FROM n",params![old,current]).unwrap();
            tx.commit().unwrap();
        }
        let bounded = page_work(&store, current);
        assert!(bounded < 5000, "indexed page used {bounded} VM steps");
        assert!(
            bounded <= small + 100,
            "archive growth expanded the page's SQL work: {small} -> {bounded}"
        );
        assert_eq!(
            store.message_page(old, 0).unwrap().len(),
            51,
            "same-conversation history also stays paginated"
        );
        assert!(store.delete_conversation(old, 99).unwrap());
        let removed = store.purge_conversation_batch().unwrap();
        assert_eq!(removed, conversation::PURGE_BATCH);
        assert_eq!(store.message_page(current, 0).unwrap().len(), 51);
        store
            .conn()
            .unwrap()
            .execute("DROP INDEX messages_page", [])
            .unwrap();
        let broken = page_work(&store, current);
        eprintln!(
            "message page VM steps: 1000 old rows {small}, 100000 old rows {bounded}, missing index {broken}"
        );
        assert!(
            broken >= 5000,
            "broken unindexed query escaped the cost checker: {broken}"
        );
    }
}
