//! What the engine needs from the store for the team's history (ADR 0129): the conversations of
//! turns, kept for a while. Only [`HistoryStore::forget_transcripts`] ever removes anything, and
//! only what the retention rule names.

use super::ports::{Post, Stored};
use crate::domain::consult::Consult;
use crate::domain::history::HistoryRefusal;
use crate::domain::history::{Kept, Transcript};
use crate::domain::record::{Passage, Query};
use crate::domain::{MemberId, Millis, Receipt, RunId, WakeDecision};

/// The two trusted delivery stages of a task-thread consultation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsultDelivery {
    /// Ask the expert to answer.
    Expert,
    /// Bring the accepted answer to the requester.
    Answer,
}

/// Application-selected effects of a member's accepted answer. Fallback posts have
/// no member or knowledge touches. IDs are resolved from the existing world model.
#[derive(Debug, Default)]
pub struct ConsultPostEffects {
    /// The authenticated answering member, if this is an expert answer.
    pub member: Option<MemberId>,
    /// Story IDs cited by the cleaned answer.
    pub stories: Vec<i64>,
    /// Section IDs cited by the cleaned answer.
    pub sections: Vec<i64>,
}

impl ConsultDelivery {
    /// Stable persistence discriminator, selected by the application, never model input.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Expert => "expert",
            Self::Answer => "answer",
        }
    }
}

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
    /// # Errors
    /// A store failure.
    fn input_origin(
        &self,
        input: crate::domain::history::InputRef,
    ) -> Stored<crate::domain::history::InputOrigin>;
    /// Persist that HQ supplied Confidential history to this turn, before returning it.
    /// # Errors
    /// A store failure.
    fn mark_confidential_input(&self, run: RunId) -> Stored<()>;

    /// Whether this turn read Confidential tool results or inherited Confidential history.
    /// A missing source run also fails closed; existence and tier use one snapshot.
    /// # Errors
    /// A store failure.
    fn run_read_confidential(&self, run: RunId) -> Stored<bool>;

    /// Keeps a turn's conversation. A turn has one.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails, or the turn already has one.
    fn add_transcript(&self, transcript: &Transcript) -> Stored<()>;

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

    /// Keeps a consult asked: its id is returned.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn add_consult(&self, consult: &Consult) -> Stored<i64>;

    /// Admit a task-thread consultation and snapshot its thread's optional project.
    /// Exact retries in the same originating turn reuse the stored request before quotas.
    /// The boolean distinguishes a new admission from a replay.
    /// # Errors
    /// A store failure, or a shared turn/day allowance refusal.
    fn accept_consult(
        &self,
        consult: &Consult,
        day: i64,
        day_start: Millis,
    ) -> Stored<Result<(Consult, bool), HistoryRefusal>>;

    /// Every consult, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn consults(&self) -> Stored<Vec<Consult>>;

    /// Legacy-only counts saturated at the admission limits, without loading questions.
    /// Admission itself uses the combined origin count in `accept_consult`.
    /// The day interval is supplied by the application in the owner's timezone.
    /// # Errors
    /// A store failure.
    fn consult_counts(
        &self,
        run: RunId,
        asker: MemberId,
        start: Millis,
        end: Millis,
    ) -> Stored<(usize, usize)>;

    /// The first consult assigned to this run, using its durable association.
    /// # Errors
    /// A store failure.
    fn consult_for_run(&self, run: RunId) -> Stored<Option<Consult>>;

    /// Return this run's existing association, or atomically assign the oldest queued
    /// consult for its expert. A concurrent claimant cannot take the same question.
    /// # Errors
    /// A store failure.
    fn claim_consult(&self, run: RunId, expert: MemberId) -> Stored<Option<Consult>>;

    /// At most fifty terminal answers still awaiting their asker's wake.
    /// # Errors
    /// A store failure.
    fn unread_consults(&self, asker: MemberId) -> Stored<Vec<Consult>>;

    /// Keeps a consult's new standing: its status, the turn that answered, why, whether its
    /// asker has been woken.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn save_consult(&self, consult: &Consult) -> Stored<()>;

    /// Conditionally complete a queued consult and append its answer to the task thread
    /// in one transaction. A stale completion writes nothing.
    /// # Errors
    /// A store failure rolls back both writes.
    fn finish_consult(&self, consult: &Consult, post: &Post) -> Stored<bool> {
        self.finish_consult_with_effects(consult, post, &ConsultPostEffects::default())
    }

    /// Complete the answer, its feed event and selected knowledge-use markers atomically.
    /// # Errors
    /// A store failure rolls back all completion effects.
    fn finish_consult_with_effects(
        &self,
        consult: &Consult,
        post: &Post,
        effects: &ConsultPostEffects,
    ) -> Stored<bool>;

    /// At most fifty requests whose expert dispatch is still pending.
    /// # Errors
    /// A store failure.
    fn pending_consult_dispatches(&self) -> Stored<Vec<Consult>>;

    /// At most fifty answers needing delivery, excluding a fold whose turn is still live.
    /// # Errors
    /// A store failure.
    fn pending_consult_notices(&self) -> Stored<Vec<Consult>>;

    /// At most fifty assigned questions whose expert turn ended before fallback completed.
    /// # Errors
    /// A store failure.
    fn ended_consults(&self) -> Stored<Vec<Consult>>;

    /// Atomically persist the policy-approved wake, any queued run and its consultation
    /// association. Replays reuse the prior outcome; folded answer notices wait for that
    /// turn to end before a new delivery decision is admitted.
    /// # Errors
    /// A store failure rolls back the entire delivery.
    fn deliver_consult(
        &self,
        id: i64,
        stage: ConsultDelivery,
        decision: WakeDecision,
        by_owner: bool,
        now: Millis,
    ) -> Stored<Receipt>;

    /// Exact accepted answer posts assigned to this result turn, independent of timestamps.
    /// # Errors
    /// A store failure.
    fn consult_answers_for_run(&self, run: RunId) -> Stored<Vec<Post>>;
}
