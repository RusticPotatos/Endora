//! Durable request and delivery intent, kept separately from message bodies.

use super::backend;
use crate::application::ports::Stored;
use crate::domain::{MemberId, Millis, RunId, TaskId, WakeId, WakeReason};
use rusqlite::{Connection, Transaction, params};

/// Both authenticated request origins consume the same allowance. Legacy rows are
/// retained, while conversation usage survives cancellation and history deletion.
pub(super) fn consultation_usage(
    conn: &Connection,
    run: RunId,
    member: MemberId,
    day: i64,
    start: Millis,
) -> Stored<(usize, usize)> {
    let legacy_turn: u8 = conn
        .query_row(super::history::CONSULT_TURN_COUNT, [run.0], |row| {
            row.get(0)
        })
        .map_err(backend)?;
    let legacy_day: u8 = conn
        .query_row(
            super::history::CONSULT_DAY_COUNT,
            params![
                member.0,
                start,
                start.saturating_add(crate::domain::money::DAY_MS)
            ],
            |row| row.get(0),
        )
        .map_err(backend)?;
    let turn:i64=conn.query_row("SELECT COALESCE((SELECT accepted FROM assistance_turn_usage WHERE requester_run=?1),0)",[run.0],|row|row.get(0)).map_err(backend)?;
    let daily:i64=conn.query_row("SELECT COALESCE((SELECT accepted FROM assistance_day_usage WHERE requester=?1 AND accepted_day=?2),0)",params![member.0,day],|row|row.get(0)).map_err(backend)?;
    Ok((
        usize::from(legacy_turn).saturating_add(usize::try_from(turn).map_err(backend)?),
        usize::from(legacy_day).saturating_add(usize::try_from(daily).map_err(backend)?),
    ))
}

/// Shared transactional queue write for both assistance origins and answer delivery.
/// Application policy authorizes the call; the caller commits the association in this tx.
pub(super) fn queue_turn(
    tx: &Transaction<'_>,
    member: MemberId,
    task: Option<TaskId>,
    reason: WakeReason,
    by_owner: bool,
    now: Millis,
) -> Stored<(WakeId, RunId)> {
    tx.execute("INSERT INTO wakes(member,task,reason,receipt,at_ms,by_owner) VALUES(?1,?2,?3,'queued',?4,?5)",
        params![member.0,task.map(|id|id.0),reason.name(),now,by_owner]).map_err(backend)?;
    let wake = WakeId(tx.last_insert_rowid());
    tx.execute("INSERT INTO runs(member,task,wake,status,queued_at,asked_by_owner) VALUES(?1,?2,?3,'queued',?4,?5)",
        params![member.0,task.map(|id|id.0),wake.0,now,by_owner]).map_err(backend)?;
    let run = RunId(tx.last_insert_rowid());
    tx.execute(
        "UPDATE wakes SET run=?2 WHERE id=?1",
        params![wake.0, run.0],
    )
    .map_err(backend)?;
    Ok((wake, run))
}

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS assistance_source_grants (
 conversation INTEGER NOT NULL REFERENCES conversations(id),
 kind TEXT NOT NULL CHECK(kind IN ('d','r')), resource INTEGER NOT NULL,
 revision INTEGER NOT NULL, granted_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
 PRIMARY KEY(conversation,kind,resource)
) STRICT;
CREATE TRIGGER IF NOT EXISTS documents_are_immutable BEFORE UPDATE ON documents
 BEGIN SELECT RAISE(ABORT, 'an original document is never rewritten'); END;
CREATE TABLE IF NOT EXISTS assistance_requests (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 conversation INTEGER NOT NULL REFERENCES conversations(id),
 project INTEGER NOT NULL REFERENCES projects(id),
 requester INTEGER NOT NULL REFERENCES members(id),
 requester_run INTEGER NOT NULL REFERENCES runs(id),
 expert INTEGER NOT NULL REFERENCES members(id),
 client_key TEXT NOT NULL, objective TEXT NOT NULL, evidence TEXT NOT NULL,
 accepted_day INTEGER NOT NULL, accepted_at INTEGER NOT NULL, deadline_at INTEGER NOT NULL,
 status TEXT NOT NULL DEFAULT 'queued',
 expert_run INTEGER REFERENCES runs(id), answer TEXT, answer_evidence TEXT NOT NULL DEFAULT '', answer_run INTEGER REFERENCES runs(id),
 notified INTEGER NOT NULL DEFAULT 0, notice_run INTEGER REFERENCES runs(id), notice_cancelled INTEGER NOT NULL DEFAULT 0,
 UNIQUE(conversation,requester,client_key),
 CHECK(requester != expert),
 CHECK(status IN ('queued','processing','answered','declined','cancelled','expired','failed'))
) STRICT;
CREATE UNIQUE INDEX IF NOT EXISTS assistance_one_open_pair
 ON assistance_requests(conversation,requester,expert)
 WHERE status IN ('queued','processing');
CREATE INDEX IF NOT EXISTS assistance_pending
 ON assistance_requests(status,deadline_at,id);
CREATE INDEX IF NOT EXISTS assistance_conversation_page
 ON assistance_requests(conversation,id);
CREATE UNIQUE INDEX IF NOT EXISTS assistance_expert_run
 ON assistance_requests(expert_run) WHERE expert_run IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS assistance_notice_run
 ON assistance_requests(notice_run) WHERE notice_run IS NOT NULL;
CREATE INDEX IF NOT EXISTS assistance_unnotified
 ON assistance_requests(status,notified,notice_cancelled,id);
CREATE INDEX IF NOT EXISTS assistance_waiting_task
 ON assistance_requests(requester,requester_run,id)
 WHERE notice_cancelled=0 AND (status IN ('queued','processing') OR (status='answered' AND notified=0));
CREATE INDEX IF NOT EXISTS assistance_live_member_run
 ON runs(member,status) WHERE status IN ('queued','running','retry_waiting');
CREATE TABLE IF NOT EXISTS assistance_turn_usage (
 requester_run INTEGER PRIMARY KEY REFERENCES runs(id), accepted INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS assistance_day_usage (
 requester INTEGER NOT NULL REFERENCES members(id), accepted_day INTEGER NOT NULL,
 accepted INTEGER NOT NULL, PRIMARY KEY(requester,accepted_day)
) STRICT;
";
