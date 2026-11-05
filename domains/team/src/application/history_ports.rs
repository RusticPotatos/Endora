//! What the engine needs from the store for the team's history (ADR 0129): the conversations of
//! turns, kept for a while. Only [`HistoryStore::forget_transcripts`] ever removes anything, and
//! only what the retention rule names.

use super::ports::{Post, Stored};
use crate::domain::consult::Consult;
use crate::domain::history::{Kept, Transcript};
use crate::domain::record::{Passage, Query};
use crate::domain::{MemberId, Millis, RunId};

/// Task content and independently authored components from one database snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInputs {
    /// Task content and structural links from the same snapshot as its provenance.
    pub task: crate::domain::Task,
    /// Source of title/body and goal/parent links.
    pub body_origin: crate::domain::history::InputOrigin,
    /// Source of acceptance criteria, independent of the body.
    pub criteria_origin: crate::domain::history::InputOrigin,
}

/// Goal content paired with source identity in one database snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalInput {
    /// Current goal content.
    pub goal: crate::domain::Goal,
    /// Source of that content.
    pub origin: crate::domain::history::InputOrigin,
}

/// One task post with a stable keyset cursor and atomically read provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostInput {
    /// Original post ID, never guessed from text or timestamps.
    pub id: i64,
    /// Retained content and presentation author.
    pub post: Post,
    /// Original evidence references; these do not grant access.
    pub evidence: Vec<crate::domain::Ref>,
    /// Source of this exact post content.
    pub origin: crate::domain::history::InputOrigin,
}

/// Retained task consultation with original IDs and exact associations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyConsultation {
    /// Stable original row ID; distinct from canonical conversation IDs.
    pub id: i64,
    /// Recorded requester and source turn, never inferred from names.
    pub requester: MemberId,
    /// Original requesting turn.
    pub requester_run: RunId,
    /// Recorded intended expert.
    pub expert: MemberId,
    /// Exact associated expert turn, if retained.
    pub expert_run: Option<RunId>,
    /// Task whose thread originally held the answer.
    pub thread: crate::domain::TaskId,
    /// Optional original question topic, not an authority grant.
    pub topic: Option<crate::domain::TaskId>,
    /// Recorded project snapshot only; absent associations remain unscoped.
    pub project: Option<crate::domain::ProjectId>,
    /// Original retained question.
    pub question: String,
    /// Original recorded state, retained as testimony rather than a live workflow.
    pub outcome: String,
    /// Original outcome explanation.
    pub reason: String,
    /// Original creation time.
    pub at: Millis,
    /// Exact answer-post association, even when its body is unavailable.
    pub answer_post: Option<i64>,
    /// Answer content only when the exact linked post belongs to this thread.
    pub answer: Option<PostInput>,
}

/// One bounded pass preparing legacy peer work for the ADR 0138 cutover.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LegacyRetirementBatch {
    /// Requests explicitly cancelled in this transaction (at most fifty).
    pub cancelled: usize,
    /// Still-live taskless legacy expert turns to stop and settle (at most fifty).
    /// These remain discoverable until the application finishes them, including
    /// after a restart between cancellation and accounting settlement.
    pub runs: Vec<RunId>,
}

/// The history's part of HQ's store.
pub trait HistoryStore {
    /// Cancel at most fifty queued legacy requests and identify at most fifty
    /// live legacy expert turns, preserving all historical links and content.
    /// The application must first fence legacy admission/dispatch, then stop and
    /// settle returned turns before declaring cutover complete. This operation
    /// creates no answer, notification, permission, charge or replacement work.
    /// # Errors
    /// A store failure; a failed transaction leaves cancellation unchanged.
    fn retire_legacy_consultations(&self) -> Stored<LegacyRetirementBatch>;

    /// At most 51 legacy rows after an ID: fifty visible records and one lookahead.
    /// Historical inspection cannot create a conversation, grant or wake.
    /// # Errors
    /// A store failure or corrupt retained record.
    fn legacy_task_consultations(
        &self,
        task: crate::domain::TaskId,
        after: i64,
    ) -> Stored<Vec<LegacyConsultation>>;
    /// Task content and provenance in the same snapshot, including criteria.
    /// # Errors
    /// A store failure or corrupt record.
    fn task_inputs(&self, task: crate::domain::TaskId) -> Stored<Option<TaskInputs>>;
    /// Goal content and provenance in the same snapshot.
    /// # Errors
    /// A store failure or corrupt record.
    fn goal_input(&self, goal: crate::domain::GoalId) -> Stored<Option<GoalInput>>;
    /// At most fifty posts after a stable ID, with content and source read together.
    /// # Errors
    /// A store failure or corrupt record.
    fn authored_posts(
        &self,
        task: crate::domain::TaskId,
        after: i64,
        most: usize,
    ) -> Stored<Vec<PostInput>>;
    /// The newest posts, bounded to fifty and returned in original ID order.
    /// Content and provenance share a snapshot; older history is not loaded.
    /// # Errors
    /// A store failure or corrupt record.
    fn latest_authored_posts(
        &self,
        task: crate::domain::TaskId,
        most: usize,
    ) -> Stored<Vec<PostInput>>;
    /// Reliable source identity, or Unknown for legacy/missing/invalidated input.
    /// This metadata lookup is for audit; a classifier must read content and origin
    /// together in one snapshot, not pair it with an earlier independent content read.
    fn input_origin(
        &self,
        input: crate::domain::history::InputRef,
    ) -> Stored<crate::domain::history::InputOrigin>;
    /// Persist that HQ supplied Confidential history to this turn, before returning it.
    fn mark_confidential_input(&self, run: RunId) -> Stored<()>;

    /// Whether this turn read Confidential tool results or inherited Confidential history.
    /// A missing source run also fails closed; existence and tier use one snapshot.
    fn run_read_confidential(&self, run: RunId) -> Stored<bool>;

    /// Keeps a turn's conversation. A turn has one.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails, or the turn already has one.
    fn add_transcript(&self, transcript: &Transcript) -> Stored<()>;

    /// Puts `transcript` in place of the one its turn has: a turn tried again keeps each
    /// attempt's conversation in one.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn replace_transcript(&self, transcript: &Transcript) -> Stored<()>;

    /// A turn's conversation, if HQ kept it and still has it.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn transcript(&self, run: RunId) -> Stored<Option<Transcript>>;

    /// What is kept, without the conversations, oldest first: what the retention rule reads.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn transcripts_kept(&self) -> Stored<Vec<Kept>>;

    /// Lets go of these conversations. Returns how many were kept and are gone.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn forget_transcripts(&self, runs: &[RunId]) -> Stored<usize>;

    /// The passages of HQ's record that hold any of a query's words, newest first from each
    /// kind of record (posts, handover notes, pages and timelines, stories, notebooks, turns'
    /// conversations). Nothing is chosen, ranked or hidden here: that is the domain's.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn search_record(&self, query: &Query) -> Stored<Vec<Passage>>;

    /// Import retained test history without any admission or execution effects.
    /// # Errors
    /// A fixture store failure.
    #[cfg(test)]
    fn seed_legacy_consultation(&self, consult: &Consult) -> Stored<i64>;

    /// Retained legacy consultations for the owner's historical outcome report.
    /// # Errors
    /// A store failure or corrupt retained record.
    fn consults(&self) -> Stored<Vec<Consult>>;
}
