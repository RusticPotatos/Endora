//! Persistent conversation intent; implementations use bounded, indexed reads.

use super::ports::Stored;
use crate::domain::assistance::{NewRequest, Request, SourceRef};
use crate::domain::conversation::{Conversation, Information, Message, Operation};
use crate::domain::{MemberId, Millis, ProjectId, RunId, TaskId};

/// Atomic message/request acceptance result, including a deterministic policy refusal.
pub type Accepted = Result<i64, &'static str>;

/// The store boundary for owner-authorized conversations and bounded assistance.
pub trait ConversationStore {
    /// Trusted payer of an assistance/result run, retained in its reservation after deletion.
    /// # Errors
    /// A store failure.
    fn assistance_charging_project(&self, run: RunId) -> Stored<Option<ProjectId>>;
    /// One retained request, by primary key.
    /// # Errors
    /// A store failure.
    fn assistance_request(&self, id: i64) -> Stored<Option<Request>>;
    /// Cancel an open request conditionally; an already accepted answer wins.
    /// # Errors
    /// A store failure.
    fn cancel_assistance_request(
        &self,
        id: i64,
        actor: Option<RunId>,
        now: Millis,
    ) -> Stored<Accepted>;
    /// Owner kill switch cancels active requests and undelivered results.
    /// # Errors
    /// A store failure.
    fn cancel_assistance(&self) -> Stored<()>;
    /// Mark a terminal expert turn that supplied no answer.
    /// # Errors
    /// A store failure.
    fn close_unanswered_assistance(&self, run: RunId, completed: bool) -> Stored<()>;
    /// Grant one immutable original-source revision to an active owner scope, at most five.
    /// # Errors
    /// A store failure.
    fn grant_assistance_source(
        &self,
        conversation: i64,
        source: SourceRef,
        at: Millis,
    ) -> Stored<bool>;
    /// Check a live grant against the current original-source revision.
    /// # Errors
    /// A store failure.
    fn assistance_source_granted(
        &self,
        conversation: i64,
        source: SourceRef,
        at: Millis,
    ) -> Stored<bool>;
    /// Owner's at most five active source grants.
    /// # Errors
    /// A store failure.
    fn assistance_grants(&self, conversation: i64, at: Millis) -> Stored<Vec<(SourceRef, Millis)>>;
    /// Owner revokes a source immediately.
    /// # Errors
    /// A store failure.
    fn revoke_assistance_source(&self, conversation: i64, source: SourceRef) -> Stored<()>;
    /// Mark that the turn's briefing was classified under this owner switch revision.
    /// # Errors
    /// A store failure.
    fn mark_conversation_ready(&self, run: RunId, revision: i64) -> Stored<()>;
    /// Whether the turn's classified briefing matches the current switch revision.
    /// # Errors
    /// A store failure.
    fn conversation_ready(&self, run: RunId, revision: i64) -> Stored<bool>;
    /// Create an owner-authorized scope.
    /// # Errors
    /// A store failure.
    fn add_conversation(
        &self,
        project: ProjectId,
        members: [MemberId; 2],
        at: Millis,
    ) -> Stored<i64>;
    /// Look up one non-deleted scope.
    /// # Errors
    /// A store failure.
    fn conversation(&self, id: i64) -> Stored<Option<Conversation>>;
    /// At most two scopes for a fixed pair, to distinguish unique from ambiguous routing.
    /// No history bodies or archive-wide directory scan.
    /// # Errors
    /// A store failure.
    fn conversation_pair(&self, a: MemberId, b: MemberId) -> Stored<Vec<Conversation>>;
    /// Metadata-only existence check for an active project scope, independent of
    /// history pages. A project restricts task-linked discovery, never grants it.
    /// # Errors
    /// A store failure.
    fn has_active_conversation(&self, member: MemberId, project: Option<ProjectId>)
    -> Stored<bool>;
    /// Read at most PAGE+1 summaries after an ID, optionally for one participant.
    /// # Errors
    /// A store failure.
    fn conversation_page(&self, member: Option<MemberId>, after: i64) -> Stored<Vec<Conversation>>;
    /// Atomically deduplicate, check rate limits, insert, audit and update summary.
    /// # Errors
    /// A store failure; user conflicts and quotas are in the accepted result.
    fn accept_information(&self, message: &Information) -> Stored<Accepted>;
    /// Read at most PAGE+1 messages, using an indexed keyset cursor.
    /// # Errors
    /// A store failure.
    fn message_page(&self, conversation: i64, after: i64) -> Stored<Vec<Message>>;
    /// Record an operation without message content or unauthorized locators.
    /// # Errors
    /// A store failure.
    fn conversation_operation(
        &self,
        run: RunId,
        conversation: Option<i64>,
        operation: &str,
        reason: &str,
        at: Millis,
    ) -> Stored<()>;
    /// Read at most PAGE+1 content-free outcomes for the owner, after a stable ID.
    /// # Errors
    /// A store failure.
    fn conversation_operations_page(&self, after: i64) -> Stored<Vec<Operation>>;
    /// Immediately deny reads; later purging is bounded.
    /// # Errors
    /// A store failure.
    fn delete_conversation(&self, id: i64, at: Millis) -> Stored<bool>;
    /// Remove at most PURGE_BATCH messages/operation rows from one deleted scope.
    /// # Errors
    /// A store failure.
    fn purge_conversation_batch(&self) -> Stored<usize>;
    /// Atomically accept one bounded request and its delivery intent, or replay its key.
    /// Recheck the requester's trusted task/project and preserve the original task on replay.
    /// # Errors
    /// A storage failure; conflicts and quotas are returned as policy refusals.
    fn accept_assistance(&self, request: &NewRequest) -> Stored<Accepted>;
    /// Due delivery intents, bounded and ordered by stable ID.
    /// # Errors
    /// A store failure.
    fn due_assistance(&self, now: Millis) -> Stored<Vec<Request>>;
    /// Atomically associate exactly one queued expert turn with a still-due request.
    /// # Errors
    /// A store failure.
    fn queue_assistance(&self, id: i64, now: Millis) -> Stored<Option<RunId>>;
    /// Read the authoritative request linked to an expert turn.
    /// # Errors
    /// A store failure.
    fn assistance_for_run(&self, run: RunId) -> Stored<Option<Request>>;
    /// Accept one terminal answer and its notification intent, idempotently.
    /// # Errors
    /// A store failure; stale, late and conflicting answers are policy refusals.
    fn accept_assistance_answer(
        &self,
        run: RunId,
        answer: &str,
        evidence: &[SourceRef],
        now: Millis,
    ) -> Stored<Accepted>;
    /// Owner's bounded request/result page within one scope.
    /// # Errors
    /// A store failure.
    fn assistance_page(&self, conversation: i64, after: i64) -> Stored<Vec<Request>>;
    /// Atomically retain one finding from its live associated requester-result
    /// turn. Exact retries reuse it; conflicting replacements refuse.
    /// # Errors
    /// A store failure; policy refusals are returned in Accepted.
    fn record_assistance_finding(
        &self,
        run: RunId,
        text: &str,
        evidence: &[SourceRef],
        now: Millis,
    ) -> Stored<Accepted>;
    /// Read one retained finding by its exact request ID.
    /// # Errors
    /// A store failure.
    fn assistance_finding(
        &self,
        request: i64,
    ) -> Stored<Option<crate::domain::assistance::Finding>>;
    /// Expire at most fifty due open requests, returning associated expert turns to stop.
    /// # Errors
    /// A store failure.
    fn expire_assistance(&self, now: Millis) -> Stored<Vec<RunId>>;
    /// At most fifty linked live turns whose request was cancelled or expired.
    /// # Errors
    /// A store failure.
    fn inactive_assistance_runs(&self) -> Stored<Vec<RunId>>;
    /// Answered results still needing a requester wake, bounded by stable ID.
    /// # Errors
    /// A store failure.
    fn pending_assistance_notices(&self) -> Stored<Vec<Request>>;
    /// Atomically queue one result wake and mark its intent delivered.
    /// # Errors
    /// A store failure.
    fn queue_assistance_notice(&self, id: i64, now: Millis) -> Stored<Option<RunId>>;
    /// The result assigned to one requester notification turn.
    /// # Errors
    /// A store failure.
    fn assistance_notice_for_run(&self, run: RunId) -> Stored<Option<Request>>;
    /// Whether a member awaits accepted help for this task or its live result
    /// verification. Callers supply the current owner; this holds only recovery.
    /// # Errors
    /// A store failure.
    fn task_awaits_assistance(&self, task: TaskId, member: MemberId) -> Stored<bool>;
}
