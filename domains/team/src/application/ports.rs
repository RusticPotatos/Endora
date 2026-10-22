//! What the engine needs from the world: a store, runners, and a clock.

use core::fmt;
use std::time::Duration;

use crate::domain::office;
use crate::domain::{Branch, Check, Document};
use crate::domain::{CallReceipt, Criterion, Label, Ref};
use crate::domain::{
    CheckBack, CheckBackStatus, Exit, Goal, Liveness, Project, Routine, RoutineId,
};
use crate::domain::{
    Event, GoalId, GoalStatus, Member, MemberId, Micros, Millis, Priority, Receipt, Role, RunId,
    RunStatus, Task, TaskId, TaskStatus, WakeId, WakeReason,
};
use crate::domain::{Evidence, ProjectId, ProjectStatus};
use crate::domain::{chess, games};

/// A store that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the team's store failed: {}", self.0)
    }
}

impl std::error::Error for StoreError {}

/// A store result.
pub type Stored<T> = Result<T, StoreError>;

/// A task to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    /// The goal it is for.
    pub goal: GoalId,
    /// The project of that goal it goes in, if any; a sub-task goes in its parent's.
    pub project: Option<ProjectId>,
    /// The task it is part of, if any.
    pub parent: Option<TaskId>,
    /// What it is.
    pub title: String,
    /// What is wanted.
    pub description: String,
    /// The member it is handed to, if any.
    pub assignee: Option<MemberId>,
    /// How soon it matters.
    pub priority: Priority,
    /// The tasks it waits on.
    pub waits_on: Vec<TaskId>,
}

/// A routine to create (ADR 0107).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRoutine {
    /// The title each of its tasks gets.
    pub title: String,
    /// What each of its tasks asks for.
    pub description: String,
    /// The goal its tasks serve.
    pub goal: GoalId,
    /// The project of that goal its tasks go in, if any.
    pub project: Option<ProjectId>,
    /// The member its tasks are handed to.
    pub assignee: MemberId,
    /// How often it fires.
    pub every_ms: Millis,
    /// Whether it takes the home lab's alarms.
    pub on_lab_trouble: bool,
}

/// A project to create (ADR 0113).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProject {
    /// The goal it serves.
    pub goal: GoalId,
    /// What it is.
    pub title: String,
    /// Why it is worth doing.
    pub reason: String,
    /// Who leads it.
    pub lead: MemberId,
    /// Where it starts.
    pub status: ProjectStatus,
    /// Its cap, if the owner set one.
    pub cap: Option<Micros>,
    /// A cap asked for, waiting on the owner's desk.
    pub asked_cap: Option<Micros>,
    /// Who made it.
    pub created_by: String,
    /// The member who proposed it, if any.
    pub proposer: Option<MemberId>,
    /// The task the proposal was made in, if any.
    pub proposed_in: Option<TaskId>,
    /// What the proposal pointed at.
    pub evidence: Vec<Evidence>,
}

/// One post in a task's thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    /// Who wrote it: `owner`, `endora`, or a member's name.
    pub author: String,
    /// What it says.
    pub text: String,
    /// When.
    pub at: Millis,
    /// Whether a member wrote it stating nothing from the record, in a turn that took no
    /// reading (ADR 0114): shown as unsupported to the tester and the lead.
    pub unsupported: bool,
}

/// Evidence cited for one criterion of a task, at one stage of its checking (ADR 0114).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cited {
    /// The task.
    pub task: TaskId,
    /// The turn that cited it.
    pub run: RunId,
    /// The member whose turn it was.
    pub member: MemberId,
    /// `submitted` (the member's), `tested` (the tester's pass), `accepted` (the lead's), or
    /// `closed` (a routine's check closed by its member).
    pub stage: String,
    /// Which criterion, from 1; 0 for work with none.
    pub criterion: usize,
    /// What it cites.
    pub refs: Vec<Ref>,
    /// When.
    pub at: Millis,
}

/// One action in a scenario, as its log keeps it (the Holodeck plan's phase 5): the accepted
/// ones, replayed in order from the seed, are the world's state; the refused ones are kept as
/// what was tried. The log is append-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioAction {
    /// Its place in the log (ignored when added).
    pub id: i64,
    /// The scenario's task.
    pub task: TaskId,
    /// The turn that acted.
    pub run: RunId,
    /// The member whose turn it was.
    pub member: MemberId,
    /// The action, as the scenario keeps it (`inspect`, `replace_part bulb`).
    pub action: String,
    /// Whether the world accepted it.
    pub ok: bool,
    /// What the world said: what was seen, or why it was refused.
    pub said: String,
    /// When.
    pub at: Millis,
}

/// One wake, as its receipt keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wake {
    /// Its id.
    pub id: WakeId,
    /// Whose turn it asks for.
    pub member: MemberId,
    /// What it is about, if a task.
    pub task: Option<TaskId>,
    /// Why.
    pub reason: WakeReason,
    /// What became of it.
    pub receipt: Receipt,
    /// The turn it started or joined, if any.
    pub run: Option<RunId>,
    /// When it was raised.
    pub at: Millis,
    /// Whether the owner's own request raised it (ADR 0113, amended): their task, reply,
    /// answer or "Run now". The turn it starts is then the pool's, not the member's share's.
    pub by_owner: bool,
}

/// One turn, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// Its id.
    pub id: RunId,
    /// Whose turn.
    pub member: MemberId,
    /// The task, if it is about one.
    pub task: Option<TaskId>,
    /// The wake that started it.
    pub wake: WakeId,
    /// Where it is.
    pub status: RunStatus,
    /// How many times it has started.
    pub attempts: u8,
    /// When it was queued.
    pub queued_at: Millis,
    /// When it last started, if it has.
    pub started_at: Option<Millis>,
    /// When it may be tried again, while it waits to.
    pub retry_at: Option<Millis>,
    /// When it ended, if it has.
    pub ended_at: Option<Millis>,
    /// What it has reserved from the budget for its current attempt.
    pub reserved: Micros,
    /// How many times the platform interrupted its turn (HQ or the runner restarted under it,
    /// or its container vanished): the first is tried again without counting against its
    /// retries ([`crate::domain::runs::MOST_QUIET`]).
    pub interrupted: u8,
    /// What it has reported spending, across attempts.
    pub spent: Micros,
    /// What the member was told at the start of its latest attempt.
    pub told: String,
    /// Why it ended, in a few words.
    pub end_reason: String,
    /// Whether it changed something through HQ: moved a task, made one, changed a project
    /// (ADR 0114; before it, also whether it posted).
    pub moved_forward: bool,
    /// Whether it posted in a thread.
    pub posted: bool,
    /// Whether it left its task blocked or put a blocker to someone (a question, an approval).
    pub blocked: bool,
    /// What it came to, once it ended (ADR 0114); `None` before, and for turns before labels.
    pub label: Option<Label>,
    /// The fresh input tokens its model calls sent, as the relay reported them.
    pub input_tokens: i64,
    /// The input tokens they read from the prompt cache.
    pub cache_read_tokens: i64,
    /// The input tokens they wrote to the prompt cache.
    pub cache_write_tokens: i64,
    /// The output tokens its model calls produced, as the relay reported them.
    pub output_tokens: i64,
    /// Whether it exists because the owner asked (ADR 0113, amended): what it spends is
    /// counted against the pool's day only, never the member's share.
    pub asked_by_owner: bool,
    /// The brain its latest attempt runs or ran on (ADR 0131), decided when it started; `None`
    /// before it has, and for turns before turns had a kind, which ran on their member's
    /// brain.
    pub brain: Option<crate::domain::Brain>,
}

/// One item on the owner's desk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskItem {
    /// Its id.
    pub id: crate::domain::DeskId,
    /// What kind of decision it is: `stuck`, `ready_to_merge`, …
    pub kind: String,
    /// The task it is about, if any.
    pub task: Option<TaskId>,
    /// The goal it is about, if any.
    pub goal: Option<GoalId>,
    /// The project it is about, if any (ADR 0113).
    pub project: Option<ProjectId>,
    /// The member it is from, if any: who asked, for a question.
    pub member: Option<MemberId>,
    /// What the owner is shown.
    pub text: String,
    /// `waiting`, or how the owner decided.
    pub status: String,
    /// When it landed.
    pub at: Millis,
}

/// A regular wake: the lead's look over the team's work, for one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timer {
    /// Its id.
    pub id: i64,
    /// Whose turn it raises.
    pub member: MemberId,
    /// Why.
    pub reason: WakeReason,
    /// When it is next due.
    pub due_at: Millis,
    /// How often.
    pub every_ms: Millis,
}

/// HQ's own store (spec section 14). Every change the engine makes goes through it, and the
/// engine is its only writer, one change at a time.
pub trait TeamStore:
    super::world_ports::WorldStore
    + super::guard_ports::GuardStore
    + super::map_ports::MapStore
    + super::history_ports::HistoryStore
    + super::conversation_ports::ConversationStore
{
    /// Adds a member.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_member(
        &self,
        name: &str,
        role: Role,
        share_percent: u8,
        slots: u8,
        look: u8,
    ) -> Stored<MemberId>;
    /// Every member, in the order they joined.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn members(&self) -> Stored<Vec<Member>>;
    /// Pauses or resumes a member.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_paused(&self, member: MemberId, paused: bool) -> Stored<()>;
    /// Puts a member on furlough from `at` (ADR 0118). Nothing else about it changes: members
    /// are never deleted.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn furlough_member(&self, member: MemberId, at: Millis) -> Stored<()>;
    /// Ends a member's furlough at `at`.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn recall_member(&self, member: MemberId, at: Millis) -> Stored<()>;
    /// Adds a note to a member's notebook (ADR 0118), written at `at` (and last used then).
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_note(
        &self,
        member: MemberId,
        text: &str,
        evidence: &[Ref],
        from_owner: bool,
        run: Option<RunId>,
        at: Millis,
    ) -> Stored<i64>;
    /// A member's notes, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn notes(&self, member: MemberId) -> Stored<Vec<crate::domain::Note>>;
    /// Rewrites a note's words.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_note_text(&self, note: i64, text: &str) -> Stored<()>;
    /// Says a note was used at `at`.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn touch_note(&self, note: i64, at: Millis) -> Stored<()>;
    /// Removes a note from its notebook.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn delete_note(&self, note: i64) -> Stored<()>;
    /// Sets a brain of a member's (ADR 0131), the one named going to the slot it belongs to: a
    /// Claude is the brain it judges with, a house model the brain it works with. The other
    /// slot is left as it is.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_brain(&self, member: MemberId, brain: crate::domain::Brain) -> Stored<()>;
    /// Sets what a member does its work with (ADR 0131): the Mac or the house's own model.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_work_brain(&self, member: MemberId, brain: crate::domain::Brain) -> Stored<()>;
    /// What runs a member's turns, as the owner set it (ADR 0117): the worker for a member
    /// never set, or one the store does not know.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn runtime_of(&self, member: MemberId) -> Stored<crate::domain::Runtime>;
    /// Sets what runs a member's turns.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_runtime(&self, member: MemberId, runtime: crate::domain::Runtime) -> Stored<()>;
    /// Renames a member.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn rename_member(&self, member: MemberId, name: &str) -> Stored<()>;
    /// Sets whom a member reports to (ADR 0113).
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_reports_to(&self, member: MemberId, to: Option<MemberId>) -> Stored<()>;

    /// Adds a goal.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_goal(
        &self,
        title: &str,
        reason: &str,
        status: GoalStatus,
        parent: Option<GoalId>,
    ) -> Stored<GoalId>;
    /// A goal.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn goal(&self, id: GoalId) -> Stored<Option<Goal>>;
    /// Every goal, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn goals(&self) -> Stored<Vec<Goal>>;
    /// Sets a goal's status.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_goal_status(&self, id: GoalId, status: GoalStatus) -> Stored<()>;
    /// A goal's notes from the owner: empty when there are none.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn goal_note(&self, id: GoalId) -> Stored<String>;
    /// Sets a goal's notes from the owner; empty clears them.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_goal_note(&self, id: GoalId, note: &str) -> Stored<()>;

    /// Adds a task, at `depth` hands from its goal.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_task(&self, task: &NewTask, depth: u8) -> Stored<TaskId>;
    /// A task.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn task(&self, id: TaskId) -> Stored<Option<Task>>;
    /// Every task, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn tasks(&self) -> Stored<Vec<Task>>;
    /// Takes a task for `member`, in one step that succeeds for exactly one member: `Ok(None)`
    /// if it is now theirs, `Ok(Some(other))` if another member has it.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn claim(&self, task: TaskId, member: MemberId) -> Stored<Option<MemberId>>;
    /// Lets a task go: it has no owner.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn release(&self, task: TaskId) -> Stored<()>;
    /// Sets a task's status.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_task_status(&self, task: TaskId, status: TaskStatus) -> Stored<()>;
    /// Puts a task in a project, or in none.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_task_project(&self, task: TaskId, project: Option<ProjectId>) -> Stored<()>;
    /// Records whether a task has had its one recovery nudge.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_nudged(&self, task: TaskId, nudged: bool) -> Stored<()>;
    /// Records whether the task's waiter has been woken now its waits are over.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_woken_after_waits(&self, task: TaskId, woken: bool) -> Stored<()>;
    /// Whether the task's waiter has been woken now its waits are over.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn woken_after_waits(&self, task: TaskId) -> Stored<bool>;
    /// Records the gate's latest result for a task.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_gate(&self, task: TaskId, passed: bool) -> Stored<()>;
    /// The gate's latest result for a task, if it has run.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn gate(&self, task: TaskId) -> Stored<Option<bool>>;

    /// Adds a post to a task's thread.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn post(&self, task: TaskId, author: &str, text: &str, at: Millis) -> Stored<()> {
        self.post_marked(task, author, text, at, &[], false)
    }
    /// Adds a post to a task's thread with the evidence it cites, and whether it is
    /// unsupported (ADR 0114).
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn post_marked(
        &self,
        task: TaskId,
        author: &str,
        text: &str,
        at: Millis,
        evidence: &[Ref],
        unsupported: bool,
    ) -> Stored<()>;
    /// Sets what done means for a task.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_criteria(&self, task: TaskId, criteria: &[Criterion]) -> Stored<()>;
    /// Hands a task to a member, or to nobody.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_assignee(&self, task: TaskId, member: Option<MemberId>) -> Stored<()>;
    /// Records a receipt the node wrote (its `id` is ignored) and returns its id.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_call_receipt(&self, receipt: &CallReceipt) -> Stored<i64>;
    /// A receipt, by its id.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn call_receipt(&self, id: i64) -> Stored<Option<CallReceipt>>;
    /// A turn's receipts, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn call_receipts_of(&self, run: RunId) -> Stored<Vec<CallReceipt>>;
    /// Adds an action to a scenario's log (its `id` is ignored) and returns its id.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_scenario_action(&self, action: &ScenarioAction) -> Stored<i64>;
    /// A scenario's log, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn scenario_actions(&self, task: TaskId) -> Stored<Vec<ScenarioAction>>;
    /// Records evidence cited for a criterion.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_cited(&self, cited: &Cited) -> Stored<()>;
    /// The evidence cited on a task, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn cited(&self, task: TaskId) -> Stored<Vec<Cited>>;
    /// A task's thread, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn thread(&self, task: TaskId) -> Stored<Vec<Post>>;

    /// Records a wake with its receipt.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_wake(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        reason: WakeReason,
        receipt: Receipt,
        run: Option<RunId>,
        at: Millis,
    ) -> Stored<WakeId>;
    /// Updates a wake's receipt.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_wake(&self, id: WakeId, receipt: Receipt, run: Option<RunId>) -> Stored<()>;
    /// Marks a wake as raised by the owner's own request.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_wake_by_owner(&self, id: WakeId) -> Stored<()>;
    /// The wakes held for a task, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn held_wakes(&self, task: TaskId) -> Stored<Vec<Wake>>;
    /// Every wake, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn wakes(&self) -> Stored<Vec<Wake>>;

    /// Queues a turn for a wake.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_run(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        wake: WakeId,
        at: Millis,
    ) -> Stored<RunId>;
    /// A turn.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn run(&self, id: RunId) -> Stored<Option<Run>>;
    /// Every turn, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn runs(&self) -> Stored<Vec<Run>>;
    /// Saves a turn's changes.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn save_run(&self, run: &Run) -> Stored<()>;

    /// Adds a model call's tokens to a turn's: input (fresh, read from the cache, written to
    /// it) and output. Only this adds to them; [`Self::save_run`] leaves them as they are.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_tokens(
        &self,
        run: RunId,
        fresh: i64,
        read: i64,
        written: i64,
        output: i64,
    ) -> Stored<()>;

    /// Issues a fresh token for a turn, replacing any it had: what its worker presents to the
    /// team protocol, good for this run only (spec section 3, invariant 3).
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn issue_token(&self, run: RunId) -> Stored<String>;

    /// The run a token was issued for, if any.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn run_of_token(&self, token: &str) -> Stored<Option<RunId>>;

    /// Reserves `amount` for a turn of `member` on `day`, before it starts.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn reserve(
        &self,
        run: RunId,
        member: MemberId,
        day: i64,
        amount: Micros,
        at: Millis,
    ) -> Stored<()>;
    /// Records use reported while a turn runs.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn record_use(
        &self,
        run: RunId,
        member: MemberId,
        day: i64,
        amount: Micros,
        at: Millis,
    ) -> Stored<()>;
    /// Settles a turn: charges what it spent and releases what it reserved.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn settle(&self, run: RunId, member: MemberId, day: i64, at: Millis) -> Stored<()>;
    /// What `member` has committed on `day`: charged, plus reserved and not yet released.
    /// `None` for the whole pool.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn committed(&self, member: Option<MemberId>, day: i64) -> Stored<Micros>;
    /// What `member` has committed on `day` against its own share: as [`Self::committed`],
    /// less its turns the owner asked for, which are the pool's alone (ADR 0113, amended).
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn share_committed(&self, member: MemberId, day: i64) -> Stored<Micros>;
    /// What `member` has really spent on `day`: the use its turns reported, not what they
    /// hold. `None` for the whole pool.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn spent(&self, member: Option<MemberId>, day: i64) -> Stored<Micros>;
    /// What turns on a project's tasks committed from `from` until `until` (charged, plus
    /// reserved and not yet released), by the moments the ledger's rows were written. A
    /// ledger row is a project's when its turn's task was in the project as it was written.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn project_committed(&self, project: ProjectId, from: Millis, until: Millis) -> Stored<Micros>;
    /// What turns on a project's tasks really spent from `from` until `until`.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn project_spent(&self, project: ProjectId, from: Millis, until: Millis) -> Stored<Micros>;

    /// Adds a project.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_project(&self, project: &NewProject, at: Millis) -> Stored<ProjectId>;
    /// Every project, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn projects(&self) -> Stored<Vec<Project>>;
    /// Saves a project's changes: its lead, status, pause, cap and asked cap, and when.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn save_project(&self, project: &Project) -> Stored<()>;
    /// Says what a desk item is about, when that is a project.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_desk_project(&self, id: crate::domain::DeskId, project: ProjectId) -> Stored<()>;

    /// Appends an event to the feed and returns its id.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn emit(&self, at: Millis, kind: &str, data: &serde_json::Value) -> Stored<i64>;
    /// The id of the latest event, 0 when there is none.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn last_event(&self) -> Stored<i64>;
    /// How many events of a kind there are, and when the latest was.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn count_events(&self, kind: &str) -> Stored<(i64, Option<Millis>)>;
    /// Deletes all but the newest `keep` events that are not ticks, and every tick not among the
    /// newest `keep` of all: the feed is for display, and a reader that fell that far behind
    /// starts again from a snapshot. The ticks do not count, or the heartbeat pushes out the
    /// week the guards read. Endora's audit log is the record.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn prune_events(&self, keep: i64) -> Stored<usize>;
    /// The event with this id, if the feed still has it.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn event(&self, id: i64) -> Stored<Option<Event>>;
    /// Whether the feed still has the event with this id.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn has_event(&self, id: i64) -> Stored<bool>;
    /// The events after `id`, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn events_after(&self, id: i64) -> Stored<Vec<Event>>;
    /// The latest `limit` events with an id below `id`, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn events_before(&self, id: i64, limit: usize) -> Stored<Vec<Event>>;

    /// Puts something on the owner's desk, about a task or a goal, from a member or not.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_desk(
        &self,
        kind: &str,
        task: Option<TaskId>,
        goal: Option<GoalId>,
        from: Option<MemberId>,
        text: &str,
        at: Millis,
    ) -> Stored<crate::domain::DeskId>;
    /// Records how the owner decided a desk item.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_desk_status(&self, id: crate::domain::DeskId, status: &str) -> Stored<()>;
    /// Rewords a desk item that still waits: a finding seen again says what it fired on now
    /// (ADR 0132). An item the owner has decided keeps the words it was decided on.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_desk_text(&self, id: crate::domain::DeskId, text: &str) -> Stored<()>;
    /// Everything on the desk, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn desk(&self) -> Stored<Vec<DeskItem>>;

    /// Whether the kill switch is on.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn stopped(&self) -> Stored<bool>;
    /// Pulls or releases the kill switch.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_stopped(&self, stopped: bool) -> Stored<()>;
    /// One of HQ's own settings, by its key: the house log, and what was done once.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn setting(&self, key: &str) -> Stored<Option<String>>;
    /// Sets one of HQ's own settings.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_setting(&self, key: &str, value: &str) -> Stored<()>;

    /// Adds a timer.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_timer(
        &self,
        member: MemberId,
        reason: WakeReason,
        due_at: Millis,
        every_ms: Millis,
    ) -> Stored<i64>;
    /// Every timer.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn timers(&self) -> Stored<Vec<Timer>>;
    /// Moves a timer to its next time.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_timer_due(&self, id: i64, due_at: Millis) -> Stored<()>;

    /// Adds a routine, on, first due at `next_due_at`.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_routine(&self, routine: &NewRoutine, next_due_at: Millis) -> Stored<RoutineId>;
    /// Every routine, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn routines(&self) -> Stored<Vec<Routine>>;
    /// Saves a routine's changes: on or off, when it is next due, when it last fired, its
    /// last task, and its project.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn save_routine(&self, routine: &Routine) -> Stored<()>;

    /// Adds a pending check-back.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_check_back(
        &self,
        member: MemberId,
        task: TaskId,
        why: &str,
        due_at: Millis,
        at: Millis,
    ) -> Stored<i64>;
    /// Every check-back, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn check_backs(&self) -> Stored<Vec<CheckBack>>;
    /// Records what became of a check-back, and when.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_check_back(&self, id: i64, status: CheckBackStatus, at: Millis) -> Stored<()>;

    /// Adds a document to a task (ADR 0115; its `id` is ignored) and returns its id.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_document(&self, document: &Document) -> Stored<i64>;
    /// A document, by its id.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn document(&self, id: i64) -> Stored<Option<Document>>;
    /// A task's documents, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn documents_of(&self, task: TaskId) -> Stored<Vec<Document>>;
    /// Records a submitted branch (its `id` is ignored) and returns its id.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_branch(&self, branch: &Branch) -> Stored<i64>;
    /// Every submitted branch, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn branches(&self) -> Stored<Vec<Branch>>;
    /// Saves a branch's status.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn save_branch(&self, branch: &Branch) -> Stored<()>;
    /// Queues a check of a branch (its `id` and `token` are ignored): returns its id and the
    /// job's own token, made by the store.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_check(&self, check: &Check) -> Stored<(i64, String)>;
    /// Every check, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn checks(&self) -> Stored<Vec<Check>>;
    /// Saves a check's status, result and times.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn save_check(&self, check: &Check) -> Stored<()>;

    /// Every post in a task's thread with the evidence it cited, oldest first: what Replay
    /// reads back.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn thread_cited(&self, task: TaskId) -> Stored<Vec<ThreadPost>>;
    /// A turn's rows of the ledger, oldest first: what Replay says it held, used, was charged
    /// and released.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn ledger_of(&self, run: RunId) -> Stored<Vec<LedgerRow>>;

    // ---- roles as an enforced RACI (ADR 0120) ------------------------------------------

    /// Every goal's strategy that has been written.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn strategies(&self) -> Stored<Vec<crate::domain::areas::Strategy>>;

    /// Keeps a goal's strategy, replacing what it had.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn save_strategy(&self, strategy: &crate::domain::areas::Strategy) -> Stored<()>;

    /// Keeps a lead's area report; returns its id.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_area_report(&self, report: &crate::domain::areas::AreaReport) -> Stored<i64>;

    /// Every area report, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn area_reports(&self) -> Stored<Vec<crate::domain::areas::AreaReport>>;

    /// Marks a task as a lead's report (`report` or `patterns`): not work on the board.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn mark_report_task(&self, task: TaskId, lead: MemberId, kind: &str) -> Stored<()>;

    /// Every task marked as a lead's report: the task, its lead, and its kind.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn report_tasks(&self) -> Stored<Vec<(TaskId, MemberId, String)>>;

    // ---- the office (ADR 0119) ----------------------------------------------------------

    /// Every item in the office, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn office_items(&self) -> Stored<Vec<office::Item>>;
    /// Puts a new item in the office; returns its number.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_office_item(
        &self,
        kind: office::Kind,
        x: i64,
        y: i64,
        by: &str,
        at: Millis,
    ) -> Stored<i64>;
    /// Moves an item.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn move_office_item(&self, id: i64, x: i64, y: i64) -> Stored<()>;
    /// Takes an item away.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn remove_office_item(&self, id: i64) -> Stored<()>;
    /// Makes the office exactly these items, numbers kept: for an undo and a reset.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn replace_office_items(&self, items: &[office::Item]) -> Stored<()>;
    /// Keeps a proposal, open; returns its number.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_office_proposal(
        &self,
        by: &str,
        changes: &[office::Change],
        why: &str,
        at: Millis,
    ) -> Stored<i64>;
    /// Every proposal with its votes, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn office_proposals(&self) -> Stored<Vec<office::Proposal>>;
    /// Closes a proposal, or reopens none: its status and how it ended.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_office_proposal(
        &self,
        id: i64,
        status: office::Status,
        outcome: &str,
        at: Millis,
    ) -> Stored<()>;
    /// Records a vote; `false` when the member had voted already (the vote is kept as it was).
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_office_vote(&self, proposal: i64, vote: &office::Vote) -> Stored<bool>;
    /// Keeps what the office was before a change, for the owner's undo.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn push_office_history(&self, what: &str, before: &[office::Item], at: Millis) -> Stored<i64>;
    /// Takes the newest of those back: what the change was, and the office before it.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn pop_office_history(&self) -> Stored<Option<(String, Vec<office::Item>)>>;
    /// How many changes the owner could undo.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn office_history_len(&self) -> Stored<i64>;

    // ---- games in the office (ADR 0121) ---------------------------------------------------

    /// Keeps a ping-pong or pool match that was played (the record's own number is ignored);
    /// returns the number it was given.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_game(&self, record: &games::Record) -> Stored<i64>;
    /// Every match played, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn games(&self) -> Stored<Vec<games::Record>>;
    /// Starts a game of chess, white to move; returns its number.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_chess_game(&self, white: MemberId, black: MemberId, at: Millis) -> Stored<i64>;
    /// Every game of chess, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn chess_games(&self) -> Stored<Vec<chess::Game>>;
    /// Keeps a game of chess as it now stands: its moves, status and last move.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn save_chess_game(&self, game: &chess::Game) -> Stored<()>;
}

/// A post in a task's thread with what it cited, as Replay reads it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadPost {
    /// The task whose thread it is in.
    pub task: TaskId,
    /// The post.
    pub post: Post,
    /// What it cited (ADR 0114).
    pub evidence: Vec<Ref>,
}

/// One row of the ledger, as Replay reads it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRow {
    /// The turn it is for.
    pub run: RunId,
    /// `reserve`, `use`, `charge` or `release`.
    pub kind: String,
    /// How much.
    pub amount: Micros,
    /// When.
    pub at: Millis,
}

/// What a member's turn is started with: the run, the member, the task and what it is told.
/// A real runner also gets the protocol endpoint and a token for this run only (phase 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunContext {
    /// The turn.
    pub run: RunId,
    /// Whose.
    pub member: MemberId,
    /// The task, if any.
    pub task: Option<TaskId>,
    /// What the member is told: its role, the task and its chain of why, what is new, the
    /// tasks it waits on.
    pub told: String,
    /// The run's own token for the team protocol, which ends with the run.
    pub token: String,
    /// How hard the member's Claude tries this turn; `None` for the house's model, or for a
    /// role left to its model's own.
    pub effort: Option<crate::domain::Effort>,
}

/// Why a turn could not be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError(pub String);

/// A runner that takes members' turns, each in a fresh container (spec section 6).
pub trait WorkerAdapter {
    /// Starts a turn.
    ///
    /// # Errors
    /// [`StartError`] if it could not start.
    fn start(&self, run: &RunContext) -> Result<(), StartError>;
    /// Whether the turn is still alive, ended, or gone.
    fn status(&self, run: RunId) -> Liveness;
    /// Why a turn that finished badly did, in the runner's words, when it said.
    fn why_ended(&self, _run: RunId) -> Option<String> {
        None
    }
    /// How a turn that finished badly ended, when the runner said: the exit code of its worker,
    /// or that its container was gone before the runner saw it end. What tells the platform's
    /// interruption of a turn from its worker's failure.
    fn exit_of(&self, _run: RunId) -> Exit {
        Exit::Unsaid
    }
    /// Asks the turn to stop, then ends it after the grace period.
    fn stop(&self, run: RunId, grace: Duration);
    /// How many turns it can run at once.
    fn capacity(&self) -> usize;
}

/// The time now.
pub trait Clock {
    /// Milliseconds since the Unix epoch.
    fn now(&self) -> Millis;
}
