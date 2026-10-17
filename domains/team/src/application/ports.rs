//! What the engine needs from the world: a store, runners, and a clock.

use core::fmt;
use std::time::Duration;

use crate::domain::{CheckBack, CheckBackStatus, Goal, Liveness, Routine, RoutineId};
use crate::domain::{
    Event, GoalId, GoalStatus, Member, MemberId, Micros, Millis, Priority, Receipt, Role, RunId,
    RunStatus, Task, TaskId, TaskStatus, WakeId, WakeReason,
};

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
    /// The member its tasks are handed to.
    pub assignee: MemberId,
    /// How often it fires.
    pub every_ms: Millis,
    /// Whether it takes the home lab's alarms.
    pub on_lab_trouble: bool,
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
    /// What it has reported spending, across attempts.
    pub spent: Micros,
    /// What the member was told at the start of its latest attempt.
    pub told: String,
    /// Why it ended, in a few words.
    pub end_reason: String,
    /// Whether it moved the work forward: posted, or changed its task.
    pub moved_forward: bool,
    /// The fresh input tokens its model calls sent, as the relay reported them.
    pub input_tokens: i64,
    /// The input tokens they read from the prompt cache.
    pub cache_read_tokens: i64,
    /// The input tokens they wrote to the prompt cache.
    pub cache_write_tokens: i64,
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
pub trait TeamStore {
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
    /// Sets what a member thinks with.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn set_brain(&self, member: MemberId, brain: crate::domain::Brain) -> Stored<()>;
    /// Renames a member.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn rename_member(&self, member: MemberId, name: &str) -> Stored<()>;

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
    fn post(&self, task: TaskId, author: &str, text: &str, at: Millis) -> Stored<()>;
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

    /// Adds a model call's input tokens to a turn's (fresh, read from the cache, written to
    /// it). Only this adds to them; [`Self::save_run`] leaves them as they are.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_tokens(&self, run: RunId, fresh: i64, read: i64, written: i64) -> Stored<()>;

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
    /// What `member` has really spent on `day`: the use its turns reported, not what they
    /// hold. `None` for the whole pool.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn spent(&self, member: Option<MemberId>, day: i64) -> Stored<Micros>;

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
    /// Deletes all but the newest `keep` events: the feed is for display, and a reader that
    /// fell that far behind starts again from a snapshot. Endora's audit log is the record.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn prune_events(&self, keep: i64) -> Stored<usize>;
    /// The events after `id`, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn events_after(&self, id: i64) -> Stored<Vec<Event>>;

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
    /// Saves a routine's changes: on or off, when it is next due, when it last fired, and
    /// its last task.
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
