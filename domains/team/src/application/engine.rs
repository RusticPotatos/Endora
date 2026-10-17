//! The engine (spec section 5): the only writer to the team's store, one change at a time.
//! Wakes and their receipts, claiming, threads, delegation, the tick, the ledger, the gate's
//! word on done, the kill switch, and the event feed.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;

use super::ports::{
    Clock, DeskItem, NewTask, Run, RunContext, StoreError, TeamStore, Wake, WorkerAdapter,
};
use crate::domain::money::{self, Micros};
use crate::domain::runs::{self, LONGEST_TURN_MS};
use crate::domain::wakes::{self, LiveTurn};
use crate::domain::work::{self, mentions};
use crate::domain::{
    EventKind, GoalId, GoalStatus, Liveness, Member, MemberId, Millis, Receipt, Refusal, Role,
    RunId, RunStatus, Task, TaskId, TaskStatus, WakeDecision, WakeReason, WakeSituation,
};

/// How the engine is set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The team pool for a month.
    pub pool_per_month: Micros,
    /// The owner's distance from UTC, for where a day starts.
    pub offset_minutes: i64,
    /// How long a turn asked to stop gets before it is ended.
    pub grace: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            pool_per_month: money::POOL_PER_MONTH,
            offset_minutes: 0,
            grace: Duration::from_secs(10),
        }
    }
}

/// Who did something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Author {
    /// The person Endora works for.
    Owner,
    /// Endora itself.
    Endora,
    /// A member of the team.
    Member(MemberId),
}

/// Why the engine did not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The store failed.
    Store(StoreError),
    /// The rules refuse it, and why.
    Refused(Refusal),
    /// Something it names does not exist.
    Unknown(&'static str),
}

impl From<StoreError> for EngineError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl core::fmt::Display for EngineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Store(e) => e.fmt(f),
            Self::Refused(r) => r.fmt(f),
            Self::Unknown(what) => write!(f, "no such {what}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// An engine result.
pub type Done<T> = Result<T, EngineError>;

/// What one tick did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickReport {
    /// Wakes raised by timers.
    pub timers: usize,
    /// Turns that ended.
    pub ended: usize,
    /// Turns set to try again.
    pub retrying: usize,
    /// Turns queued again after waiting to retry.
    pub requeued: usize,
    /// Turns started.
    pub started: usize,
    /// Stuck tasks nudged once.
    pub nudged: usize,
    /// Stuck tasks put on the owner's desk.
    pub stuck: usize,
    /// Tasks woken because what they waited on finished.
    pub woken: usize,
}

/// The engine.
pub struct Engine<S, W, C> {
    store: S,
    runner: W,
    clock: C,
    settings: Settings,
    lock: Mutex<()>,
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// An engine over a store, a runner and a clock.
    pub fn new(store: S, runner: W, clock: C, settings: Settings) -> Self {
        Self {
            store,
            runner,
            clock,
            settings,
            lock: Mutex::new(()),
        }
    }

    /// The store, for reading.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// The runner.
    pub fn runner(&self) -> &W {
        &self.runner
    }

    fn held(&self) -> std::sync::MutexGuard<'_, ()> {
        // A panic while holding the lock leaves nothing half-written that the store has not
        // already refused, so the lock is taken back rather than poisoning the engine.
        self.lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn emit(&self, kind: EventKind, data: serde_json::Value) -> Done<()> {
        self.store.emit(self.clock.now(), kind.name(), &data)?;
        Ok(())
    }

    fn today(&self) -> i64 {
        money::day_of(self.clock.now(), self.settings.offset_minutes)
    }

    fn member(&self, id: MemberId) -> Done<Member> {
        self.store
            .members()?
            .into_iter()
            .find(|m| m.id == id)
            .ok_or(EngineError::Unknown("member"))
    }

    fn task(&self, id: TaskId) -> Done<Task> {
        self.store.task(id)?.ok_or(EngineError::Unknown("task"))
    }

    /// What a member could have reserved for a turn now, if anything.
    fn affordable(&self, member: &Member) -> Done<Option<Micros>> {
        let day = self.today();
        let member_left = money::member_a_day(self.settings.pool_per_month, member.share_percent)
            - self.store.committed(Some(member.id), day)?;
        let pool_left =
            money::pool_a_day(self.settings.pool_per_month) - self.store.committed(None, day)?;
        Ok(money::reservation(member_left, pool_left))
    }

    // ---- the team and the work ---------------------------------------------------------

    /// Adds a member. (Hiring goes through the owner's desk once HQ is a service.)
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn hire(
        &self,
        name: &str,
        role: Role,
        share_percent: u8,
        slots: u8,
        look: u8,
    ) -> Done<MemberId> {
        let _held = self.held();
        Ok(self
            .store
            .add_member(name, role, share_percent, slots, look)?)
    }

    /// Pauses or resumes a member: a paused member's wakes are turned away.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn pause(&self, member: MemberId, paused: bool) -> Done<()> {
        let _held = self.held();
        Ok(self.store.set_paused(member, paused)?)
    }

    /// Sets a goal. The owner's is active at once; one Endora proposes waits on the desk.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn set_goal(&self, title: &str, reason: &str, by: Author) -> Done<GoalId> {
        let _held = self.held();
        let proposed = by != Author::Owner;
        let status = if proposed {
            GoalStatus::Proposed
        } else {
            GoalStatus::Active
        };
        let goal = self.store.add_goal(title, reason, status, None)?;
        if proposed {
            let desk = self.store.add_desk(
                "goal",
                None,
                Some(goal),
                None,
                &format!("A goal: {title}"),
                self.clock.now(),
            )?;
            self.emit(
                EventKind::GoalProposed,
                json!({ "goal": goal.0, "title": title }),
            )?;
            self.emit(
                EventKind::DeskAdded,
                json!({ "item": desk.0, "kind": "goal" }),
            )?;
            return Ok(goal);
        }
        self.emit(
            EventKind::GoalChanged,
            json!({ "goal": goal.0, "title": title, "status": status.name() }),
        )?;
        Ok(goal)
    }

    /// Creates a task. A member hands work only to members below it; the task records how
    /// many hands it passed through. Its assignee is woken, unless it waits on something.
    ///
    /// # Errors
    /// [`EngineError::Refused`] if a member hands work up or across;
    /// [`EngineError::Unknown`] for a goal, task or member that does not exist.
    pub fn add_task(&self, by: Author, task: NewTask) -> Done<TaskId> {
        let _held = self.held();
        self.store
            .goal(task.goal)?
            .ok_or(EngineError::Unknown("goal"))?;
        let parent_depth = match task.parent {
            Some(parent) => Some(self.task(parent)?.depth),
            None => None,
        };
        if let (Author::Member(from), Some(to)) = (by, task.assignee) {
            if from != to {
                let (from, to) = (self.member(from)?, self.member(to)?);
                work::may_delegate(from.role, to.role).map_err(EngineError::Refused)?;
            }
        }
        if let Some(to) = task.assignee {
            self.member(to)?;
        }
        let depth = match (parent_depth, by) {
            (Some(depth), _) => depth.saturating_add(1),
            (None, Author::Member(_)) => 1,
            (None, _) => 0,
        };
        let id = self.store.add_task(&task, depth)?;
        self.emit(
            EventKind::TaskCreated,
            json!({
                "task": id.0, "goal": task.goal.0, "parent": task.parent.map(|p| p.0),
                "owner": null, "status": TaskStatus::Todo.name(), "priority": task.priority.name(),
            }),
        )?;
        if let Some(to) = task.assignee {
            if self.waits_over(&task.waits_on)? {
                self.wake_held_lock(to, Some(id), WakeReason::Assigned)?;
            }
        }
        Ok(id)
    }

    fn waits_over(&self, waits: &[TaskId]) -> Done<bool> {
        for wait in waits {
            if !self.task(*wait)?.status.finished() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    // ---- wakes ---------------------------------------------------------------------------

    /// Asks `member` to take a turn, about `task` if any. Every wake leaves a receipt.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails; [`EngineError::Unknown`] for a member that
    /// does not exist.
    pub fn wake(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<Receipt> {
        let _held = self.held();
        self.wake_held_lock(member, task, reason)
    }

    fn live_turns(&self, member: MemberId, task: Option<TaskId>) -> Done<Vec<LiveTurn>> {
        Ok(self
            .store
            .runs()?
            .into_iter()
            .filter(|run| run.status.live())
            .filter(|run| match task {
                Some(_) => run.task == task,
                // A wake about no task joins only the member's own turn about no task.
                None => run.task.is_none() && run.member == member,
            })
            .map(|run| LiveTurn {
                run: run.id,
                member: run.member,
            })
            .collect())
    }

    fn situation(&self, member: &Member, task: Option<TaskId>) -> Done<WakeSituation> {
        let already_held = match task {
            Some(task) => self
                .store
                .held_wakes(task)?
                .iter()
                .any(|w| w.member == member.id),
            None => false,
        };
        Ok(WakeSituation {
            stopped: self.store.stopped()?,
            paused: member.paused,
            can_afford_a_turn: self.affordable(member)?.is_some(),
            live: self.live_turns(member.id, task)?,
            already_held,
        })
    }

    fn wake_held_lock(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<Receipt> {
        let who = self.member(member)?;
        let decision = wakes::decide(member, &self.situation(&who, task)?);
        let now = self.clock.now();
        let about = json!({
            "member": member.0, "task": task.map(|t| t.0), "reason": reason.name(),
        });
        match decision {
            WakeDecision::Queue => {
                let wake = self
                    .store
                    .add_wake(member, task, reason, Receipt::Queued, None, now)?;
                let run = self.store.add_run(member, task, wake, now)?;
                self.store.set_wake(wake, Receipt::Queued, Some(run))?;
                self.emit(EventKind::WakeQueued, about)?;
                Ok(Receipt::Queued)
            }
            WakeDecision::Fold(run) => {
                self.store
                    .add_wake(member, task, reason, Receipt::Folded, Some(run), now)?;
                self.emit(EventKind::WakeFolded, about)?;
                Ok(Receipt::Folded)
            }
            WakeDecision::Hold => {
                self.store
                    .add_wake(member, task, reason, Receipt::Held, None, now)?;
                self.emit(EventKind::WakeHeld, about)?;
                Ok(Receipt::Held)
            }
            WakeDecision::Refuse(why) => {
                self.store
                    .add_wake(member, task, reason, Receipt::Refused(why), None, now)?;
                let mut about = about;
                about["why"] = json!(why.name());
                self.emit(EventKind::WakeRefused, about)?;
                Ok(Receipt::Refused(why))
            }
        }
    }

    /// Releases the oldest wake held for a task, now that its turn has ended.
    fn release_held(&self, task: TaskId) -> Done<()> {
        let Some(oldest) = self.store.held_wakes(task)?.into_iter().next() else {
            return Ok(());
        };
        let who = self.member(oldest.member)?;
        let decision = wakes::decide(oldest.member, &self.situation(&who, Some(task))?);
        match decision {
            WakeDecision::Queue => {
                let run =
                    self.store
                        .add_run(oldest.member, Some(task), oldest.id, self.clock.now())?;
                self.store.set_wake(oldest.id, Receipt::Queued, Some(run))?;
                self.emit(
                    EventKind::WakeQueued,
                    json!({ "member": oldest.member.0, "task": task.0, "reason": oldest.reason.name() }),
                )?;
            }
            WakeDecision::Fold(run) => {
                self.store.set_wake(oldest.id, Receipt::Folded, Some(run))?
            }
            // Still held behind another turn; it waits on.
            WakeDecision::Hold | WakeDecision::Refuse(wakes::Refused::Duplicate) => {}
            WakeDecision::Refuse(why) => {
                self.store
                    .set_wake(oldest.id, Receipt::Refused(why), None)?;
            }
        }
        Ok(())
    }

    // ---- what members do -------------------------------------------------------------------

    fn mark_progress(&self, member: MemberId, task: TaskId) -> Done<()> {
        for mut run in self.store.runs()? {
            if run.member == member && run.task == Some(task) && run.status == RunStatus::Running {
                run.moved_forward = true;
                self.store.save_run(&run)?;
            }
        }
        Ok(())
    }

    // ---- what a turn may touch --------------------------------------------------------------

    /// A turn, while it still runs.
    fn running(&self, run: RunId) -> Done<Run> {
        let run = self.store.run(run)?.ok_or(EngineError::Unknown("run"))?;
        if run.status != RunStatus::Running {
            return Err(EngineError::Refused(Refusal::TurnOver));
        }
        Ok(run)
    }

    /// Whether a turn may act on `task`: its own task and the sub-tasks under it, at any
    /// depth. Writing reaches further: the task its own hangs from, and any task whose thread
    /// names the member (`@name`, as a wake reads it). The lead may act on any task in a goal
    /// it leads; a team has one lead, which leads every goal.
    ///
    /// # Errors
    /// [`EngineError::Refused`] with [`Refusal::OutOfReach`] when it may not.
    fn reach(&self, run: &Run, task: TaskId, writing: bool) -> Done<()> {
        let target = self.task(task)?;
        let member = self.member(run.member)?;
        if member.role == Role::Lead {
            return Ok(());
        }
        if let Some(own) = run.task {
            let mut at = Some(target.id);
            while let Some(id) = at {
                if id == own {
                    return Ok(());
                }
                at = self.task(id)?.parent;
            }
            if writing && self.task(own)?.parent == Some(target.id) {
                return Ok(());
            }
        }
        if writing {
            let members = self.store.members()?;
            let named = self.store.thread(task)?.iter().any(|p| {
                p.author != member.name
                    && mentions(&p.text, &members)
                        .iter()
                        .any(|m| m.id == member.id)
            });
            if named {
                return Ok(());
            }
        }
        Err(EngineError::Refused(Refusal::OutOfReach))
    }

    /// A turn takes a task, within its reach (see [`Self::claim`] for the rules).
    ///
    /// # Errors
    /// [`Refusal::TurnOver`] for a turn that has ended, [`Refusal::OutOfReach`] for a task
    /// outside it, and what [`Self::claim`] refuses.
    pub fn claim_in(&self, run: RunId, task: TaskId) -> Done<()> {
        let _held = self.held();
        let run = self.running(run)?;
        self.reach(&run, task, false)?;
        self.claim_held_lock(run.member, task)
    }

    /// A turn moves a task on, within its reach (see [`Self::set_status`] for the rules).
    ///
    /// # Errors
    /// [`Refusal::TurnOver`], [`Refusal::OutOfReach`], and what [`Self::set_status`] refuses.
    pub fn set_status_in(
        &self,
        run: RunId,
        task: TaskId,
        to: TaskStatus,
        reason: &str,
    ) -> Done<()> {
        let _held = self.held();
        let run = self.running(run)?;
        self.reach(&run, task, false)?;
        self.set_status_held_lock(run.member, task, to, reason)
    }

    /// A turn writes in a task's thread, within its reach for writing.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`], [`Refusal::OutOfReach`], or a task that does not exist.
    pub fn post_in(&self, run: RunId, task: TaskId, text: &str) -> Done<()> {
        let _held = self.held();
        let run = self.running(run)?;
        self.reach(&run, task, true)?;
        self.post_held_lock(Author::Member(run.member), task, text)
    }

    /// Takes a task for `member`, outside any turn: the rules of claiming on their own, for
    /// the tests. A turn claims through [`Self::claim_in`].
    #[cfg(test)]
    pub(crate) fn claim(&self, member: MemberId, task: TaskId) -> Done<()> {
        let _held = self.held();
        self.claim_held_lock(member, task)
    }

    /// Takes a task for `member`: one atomic step that succeeds for exactly one member.
    ///
    /// # Errors
    /// [`EngineError::Refused`] with [`Refusal::TakenBy`] if another member has it, or
    /// [`Refusal::Finished`] if it is over.
    fn claim_held_lock(&self, member: MemberId, task: TaskId) -> Done<()> {
        let current = self.task(task)?;
        if current.status.finished() {
            return Err(EngineError::Refused(Refusal::Finished));
        }
        if let Some(other) = self.store.claim(task, member)? {
            return Err(EngineError::Refused(Refusal::TakenBy(other)));
        }
        if current.status == TaskStatus::Todo {
            self.store.set_task_status(task, TaskStatus::Doing)?;
        }
        self.mark_progress(member, task)?;
        self.emit(
            EventKind::TaskClaimed,
            json!({ "task": task.0, "member": member.0 }),
        )?;
        Ok(())
    }

    /// Writes to a task's thread, as the owner or Endora. Anyone named in it is woken. A
    /// member writes only in its turn, through [`Self::post_in`].
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that does not exist; [`Refusal::OutOfReach`] for a
    /// member.
    pub fn post(&self, by: Author, task: TaskId, text: &str) -> Done<()> {
        let _held = self.held();
        if let Author::Member(_) = by {
            return Err(EngineError::Refused(Refusal::OutOfReach));
        }
        self.post_held_lock(by, task, text)
    }

    /// Writes as `member`, outside any turn: the thread's rules on their own, for the tests.
    #[cfg(test)]
    pub(crate) fn post_as(&self, member: MemberId, task: TaskId, text: &str) -> Done<()> {
        let _held = self.held();
        self.post_held_lock(Author::Member(member), task, text)
    }

    fn author_name(&self, by: Author) -> Done<String> {
        Ok(match by {
            Author::Owner => "owner".to_owned(),
            Author::Endora => "endora".to_owned(),
            Author::Member(id) => self.member(id)?.name,
        })
    }

    fn post_held_lock(&self, by: Author, task: TaskId, text: &str) -> Done<()> {
        self.task(task)?;
        let author = self.author_name(by)?;
        self.store.post(task, &author, text, self.clock.now())?;
        let preview: String = text.chars().take(80).collect();
        self.emit(
            EventKind::ThreadPosted,
            json!({ "task": task.0, "author": author, "preview": preview }),
        )?;
        if let Author::Member(member) = by {
            self.mark_progress(member, task)?;
        }
        let members = self.store.members()?;
        for named in mentions(text, &members) {
            if by == Author::Member(named.id) {
                continue;
            }
            self.wake_held_lock(named.id, Some(task), WakeReason::Mentioned)?;
        }
        Ok(())
    }

    /// Moves a task on for `member`, outside any turn: the rules on their own, for the tests.
    /// A turn moves its work through [`Self::set_status_in`].
    #[cfg(test)]
    pub(crate) fn set_status(
        &self,
        member: MemberId,
        task: TaskId,
        to: TaskStatus,
        reason: &str,
    ) -> Done<()> {
        let _held = self.held();
        self.set_status_held_lock(member, task, to, reason)
    }

    /// Moves a member's task on, with a reason. Handing it back (`todo`) lets it go and wakes
    /// the lead; `done` and `dropped` are never a member's to set.
    ///
    /// # Errors
    /// [`EngineError::Refused`] if the task is not the member's or the step is not allowed.
    fn set_status_held_lock(
        &self,
        member: MemberId,
        task: TaskId,
        to: TaskStatus,
        reason: &str,
    ) -> Done<()> {
        let current = self.task(task)?;
        if current.owner != Some(member) {
            return Err(EngineError::Refused(Refusal::NotYours));
        }
        work::may_set_status(current.status, to).map_err(EngineError::Refused)?;
        self.store.set_task_status(task, to)?;
        self.mark_progress(member, task)?;
        let said = if to == TaskStatus::Todo {
            format!("Handed back: {reason}")
        } else {
            format!("{} → {}: {reason}", current.status.name(), to.name())
        };
        self.post_held_lock(Author::Member(member), task, &said)?;
        if to == TaskStatus::Todo {
            self.store.release(task)?;
            self.emit(
                EventKind::TaskReleased,
                json!({ "task": task.0, "member": member.0 }),
            )?;
            for lead in self
                .store
                .members()?
                .iter()
                .filter(|m| m.role == Role::Lead)
            {
                if lead.id != member {
                    self.wake_held_lock(lead.id, Some(task), WakeReason::Assigned)?;
                }
            }
        }
        self.emit(
            EventKind::TaskChanged,
            json!({ "task": task.0, "goal": current.goal.0, "owner": current.owner.map(|o| o.0),
                    "status": to.name(), "priority": current.priority.name() }),
        )?;
        Ok(())
    }

    /// The gate's word on a task: tests, lint and build on a runner.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that does not exist.
    pub fn record_gate(&self, task: TaskId, passed: bool, what_failed: &str) -> Done<()> {
        let _held = self.held();
        self.task(task)?;
        self.store.set_gate(task, passed)?;
        self.emit(
            EventKind::GateResult,
            json!({ "task": task.0, "passed": passed, "failed": what_failed }),
        )?;
        Ok(())
    }

    /// A member approves work under review. It is done only when the gate passed and the
    /// reviewer is not the one who did it. Then a whole piece of work (one that is not part of
    /// another) is on the owner's desk: ready to merge, or ready to read when a researcher did
    /// it. A part goes to whoever owns the whole, not to the owner's desk.
    ///
    /// # Errors
    /// [`EngineError::Refused`] if it is the reviewer's own work, the gate has not passed, or
    /// the task is not under review.
    pub fn approve(&self, reviewer: MemberId, task: TaskId) -> Done<()> {
        let _held = self.held();
        self.member(reviewer)?;
        let current = self.task(task)?;
        if current.status != TaskStatus::InReview {
            return Err(EngineError::Refused(Refusal::NotAStep {
                from: current.status,
                to: TaskStatus::Done,
            }));
        }
        let gate = self.store.gate(task)?.unwrap_or(false);
        work::is_done(gate, current.owner, reviewer).map_err(EngineError::Refused)?;
        self.store.set_task_status(task, TaskStatus::Done)?;
        self.emit(
            EventKind::TaskChanged,
            json!({ "task": task.0, "goal": current.goal.0, "owner": current.owner.map(|o| o.0),
                    "status": TaskStatus::Done.name(), "priority": current.priority.name() }),
        )?;
        if current.parent.is_none() {
            let researched = match current.owner {
                Some(worker) => self.member(worker)?.role == Role::Researcher,
                None => false,
            };
            let (kind, says) = if researched {
                ("ready_to_read", "Ready to read")
            } else {
                ("ready_to_merge", "Ready to merge")
            };
            let desk = self.store.add_desk(
                kind,
                Some(task),
                None,
                None,
                &format!("{says}: {}", current.title),
                self.clock.now(),
            )?;
            self.emit(
                EventKind::DeskAdded,
                json!({ "item": desk.0, "kind": kind, "task": task.0 }),
            )?;
        }
        self.after_finishing(&current)?;
        Ok(())
    }

    /// The owner drops a task.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that does not exist.
    pub fn drop_task(&self, task: TaskId) -> Done<()> {
        let _held = self.held();
        let current = self.task(task)?;
        self.store.set_task_status(task, TaskStatus::Dropped)?;
        self.emit(
            EventKind::TaskChanged,
            json!({ "task": task.0, "goal": current.goal.0, "owner": current.owner.map(|o| o.0),
                    "status": TaskStatus::Dropped.name(), "priority": current.priority.name() }),
        )?;
        self.after_finishing(&current)
    }

    /// When the last sub-task of a task finishes, its owner is woken.
    fn after_finishing(&self, finished: &Task) -> Done<()> {
        let Some(parent) = finished.parent else {
            return Ok(());
        };
        let siblings = self.store.tasks()?;
        let open = siblings
            .iter()
            .any(|t| t.parent == Some(parent) && !t.status.finished());
        if open {
            return Ok(());
        }
        let parent = self.task(parent)?;
        if let Some(owner) = parent.owner.or(parent.assignee) {
            self.wake_held_lock(owner, Some(parent.id), WakeReason::SubtasksFinished)?;
        }
        Ok(())
    }

    // ---- money -----------------------------------------------------------------------------

    /// A turn reports what one model call cost. Crossing what it reserved stops it now, not
    /// afterwards. Returns whether it may go on.
    ///
    /// One report is held to what a whole turn may spend, and below zero is none: no call
    /// costs more than a turn reserves, and a number past that would be one the ledger could
    /// not add up. A charge for a turn that has already ended (a call still running when it
    /// was stopped) is still counted, settled on the day the turn was booked.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a run that does not exist.
    pub fn report_usage(&self, run: RunId, amount: Micros) -> Done<bool> {
        let _held = self.held();
        let amount = amount.clamp(0, money::MOST_A_TURN);
        let mut current = self.store.run(run)?.ok_or(EngineError::Unknown("run"))?;
        if current.status != RunStatus::Running {
            self.charge_late(current, amount)?;
            return Ok(false);
        }
        let now = self.clock.now();
        self.store
            .record_use(run, current.member, self.today(), amount, now)?;
        current.spent = current.spent.saturating_add(amount);
        self.store.save_run(&current)?;
        self.emit(
            EventKind::BudgetChanged,
            json!({ "member": current.member.0, "run": run.0, "spent": current.spent }),
        )?;
        if !money::crossed(current.spent, current.reserved) {
            return Ok(true);
        }
        self.emit(
            EventKind::BudgetCrossed,
            json!({ "member": current.member.0, "run": run.0, "spent": current.spent,
                    "limit": current.reserved }),
        )?;
        self.runner.stop(run, self.settings.grace);
        self.end(current, RunStatus::Stopped, "spent what it could")?;
        Ok(false)
    }

    /// Counts a charge that reached HQ after its turn stopped running. Its reservation is
    /// already released, so settling again charges only this use. A turn that never started
    /// made no call, and has nothing to count.
    fn charge_late(&self, mut run: Run, amount: Micros) -> Done<()> {
        if run.started_at.is_none() || amount == 0 {
            return Ok(());
        }
        let now = self.clock.now();
        self.store
            .record_use(run.id, run.member, self.today(), amount, now)?;
        self.store.settle(run.id, run.member, self.today(), now)?;
        run.spent = run.spent.saturating_add(amount);
        self.store.save_run(&run)?;
        self.emit(
            EventKind::BudgetChanged,
            json!({ "member": run.member.0, "run": run.id.0, "spent": run.spent, "late": true }),
        )
    }

    // ---- the kill switch -----------------------------------------------------------------

    /// Stops every turn at once; every wake is turned away until the owner resumes.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn stop_all(&self, why: &str) -> Done<()> {
        let _held = self.held();
        self.store.set_stopped(true)?;
        for run in self.store.runs()? {
            if !run.status.live() {
                continue;
            }
            if run.status == RunStatus::Running {
                self.runner.stop(run.id, self.settings.grace);
            }
            self.end(run, RunStatus::Stopped, "the kill switch")?;
        }
        self.emit(EventKind::Stopped, json!({ "by": "owner", "why": why }))
    }

    /// Lets the team work again. Only the owner does this.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn resume(&self) -> Done<()> {
        let _held = self.held();
        self.store.set_stopped(false)?;
        self.emit(EventKind::Resumed, json!({ "by": "owner" }))
    }

    // ---- runs --------------------------------------------------------------------------------

    /// Why a turn was started: the reason of the wake that queued it.
    fn reason_for(&self, run: &Run) -> Done<&'static str> {
        Ok(self
            .store
            .wakes()?
            .iter()
            .find(|w| w.id == run.wake)
            .map_or("", |w| w.reason.name()))
    }

    /// Ends a turn: settles its money, closes its wake, and lets the next held wake on its task
    /// go. A turn that moved its task forward earns the task a fresh recovery nudge.
    fn end(&self, mut run: Run, status: RunStatus, why: &str) -> Done<()> {
        let now = self.clock.now();
        if run.started_at.is_some() {
            self.store.settle(run.id, run.member, self.today(), now)?;
        }
        run.status = status;
        run.ended_at = Some(now);
        run.end_reason = why.to_owned();
        run.reserved = 0;
        self.store.save_run(&run)?;
        self.store
            .set_wake(run.wake, Receipt::Finished, Some(run.id))?;
        self.emit(
            EventKind::RunEnded,
            json!({ "run": run.id.0, "member": run.member.0, "task": run.task.map(|t| t.0),
                    "result": status.name(), "why": why, "moved_forward": run.moved_forward,
                    "spent": run.spent }),
        )?;
        if let Some(task) = run.task {
            if run.moved_forward {
                self.store.set_nudged(task, false)?;
            }
            self.release_held(task)?;
        }
        Ok(())
    }

    /// A failed turn: tried again a minute later, at most twice; then failed for good.
    fn fail(&self, mut run: Run, why: &str) -> Done<bool> {
        let now = self.clock.now();
        let Some(due) = runs::retry_after_failure(run.attempts, now) else {
            self.end(run, RunStatus::Failed, why)?;
            return Ok(false);
        };
        if run.started_at.is_some() {
            self.store.settle(run.id, run.member, self.today(), now)?;
        }
        run.status = RunStatus::RetryWaiting;
        run.retry_at = Some(due);
        run.reserved = 0;
        run.end_reason = why.to_owned();
        self.store.save_run(&run)?;
        self.emit(
            EventKind::RunEnded,
            json!({ "run": run.id.0, "member": run.member.0, "task": run.task.map(|t| t.0),
                    "result": RunStatus::RetryWaiting.name(), "why": why }),
        )?;
        Ok(true)
    }

    /// What a member is told at the start of a turn.
    fn briefing(&self, member: &Member, run: &Run) -> Done<String> {
        let mut told = format!(
            "You are {}, the team's {}.\n",
            member.name,
            member.role.name()
        );
        let Some(task) = run.task else {
            told.push_str("This turn is about no task: look over the team's work.\n");
            return Ok(told);
        };
        let task = self.task(task)?;
        told.push_str(&format!(
            "Your task: {} (#{}, {}, {})\n{}\n",
            task.title,
            task.id,
            task.status.name(),
            task.priority.name(),
            task.description
        ));
        let mut why = vec![task.title.clone()];
        let mut parent = task.parent;
        while let Some(id) = parent {
            let up = self.task(id)?;
            why.push(up.title.clone());
            parent = up.parent;
        }
        if let Some(goal) = self.store.goal(task.goal)? {
            why.push(format!("goal: {}", goal.title));
        }
        told.push_str(&format!("Why it exists: {}\n", why.join(" ← ")));
        if !task.waits_on.is_empty() {
            let waits: Vec<String> = task.waits_on.iter().map(|t| format!("#{t}")).collect();
            told.push_str(&format!("It waits on: {}\n", waits.join(", ")));
        }
        let since = self
            .store
            .runs()?
            .iter()
            .filter(|r| r.member == member.id && r.task == run.task && r.id != run.id)
            .filter_map(|r| r.started_at)
            .max()
            .unwrap_or(Millis::MIN);
        let new: Vec<String> = self
            .store
            .thread(task.id)?
            .into_iter()
            .filter(|p| p.at > since)
            .map(|p| format!("{}: {}", p.author, p.text))
            .collect();
        if new.is_empty() {
            told.push_str("Nothing new in the thread since your last turn.\n");
        } else {
            told.push_str("New in the thread since your last turn:\n");
            for line in new {
                told.push_str(&format!("- {line}\n"));
            }
        }
        told.push_str(
            "Your tools: my_turn, claim, post, set_status, create_task, ask_owner, run_gate, \
             request_action.\n",
        );
        Ok(told)
    }

    // ---- the tick ----------------------------------------------------------------------------

    /// The engine's regular pass (every 30 seconds), in the spec's order: timers, ended and
    /// vanished turns, retries, starts, stuck work, finished waits. A late tick runs once.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn tick(&self) -> Done<TickReport> {
        let _held = self.held();
        let mut report = TickReport::default();
        if self.store.stopped()? {
            return Ok(report);
        }
        let now = self.clock.now();

        // 1. Timers that are due raise their wakes; a late one fires once.
        for timer in self.store.timers()? {
            if timer.due_at <= now {
                self.wake_held_lock(timer.member, None, timer.reason)?;
                self.store.set_timer_due(timer.id, now + timer.every_ms)?;
                report.timers += 1;
            }
        }

        // 2. Running turns: ended, failed, vanished, or too long.
        for run in self.store.runs()? {
            if run.status != RunStatus::Running {
                continue;
            }
            if run
                .started_at
                .is_some_and(|started| now - started > LONGEST_TURN_MS)
            {
                self.runner.stop(run.id, self.settings.grace);
                self.end(run, RunStatus::TimedOut, "took longer than a turn may")?;
                report.ended += 1;
                continue;
            }
            match self.runner.status(run.id) {
                Liveness::Running => {}
                Liveness::Finished { succeeded: true } => {
                    self.end(run, RunStatus::Succeeded, "ended its turn")?;
                    report.ended += 1;
                }
                Liveness::Finished { succeeded: false } => {
                    if self.fail(run, "the turn failed")? {
                        report.retrying += 1;
                    } else {
                        report.ended += 1;
                    }
                }
                Liveness::Gone => {
                    if self.fail(run, "its container is gone")? {
                        report.retrying += 1;
                    } else {
                        report.ended += 1;
                    }
                }
            }
        }

        // 3. Retries whose time has come are queued again.
        for mut run in self.store.runs()? {
            if run.status == RunStatus::RetryWaiting && run.retry_at.is_some_and(|due| due <= now) {
                run.status = RunStatus::Queued;
                run.retry_at = None;
                self.store.save_run(&run)?;
                report.requeued += 1;
            }
        }

        // 4. Queued turns start where a member has a free slot, budget can be reserved, and a
        //    runner has room: urgent work first, then the oldest.
        report.started = self.start_queued(now)?;

        // 5. Doing, with no turn and nothing to wait for: one nudge, then the owner's desk.
        let tasks = self.store.tasks()?;
        let live: Vec<Option<TaskId>> = self
            .store
            .runs()?
            .iter()
            .filter(|r| r.status.live())
            .map(|r| r.task)
            .collect();
        for task in &tasks {
            if task.status != TaskStatus::Doing || live.contains(&Some(task.id)) {
                continue;
            }
            if !self.waits_over(&task.waits_on)? {
                continue;
            }
            let Some(owner) = task.owner else {
                continue;
            };
            if !task.nudged {
                self.store.set_nudged(task.id, true)?;
                self.wake_held_lock(owner, Some(task.id), WakeReason::Recovery)?;
                report.nudged += 1;
                continue;
            }
            self.store.set_task_status(task.id, TaskStatus::Blocked)?;
            let desk = self.store.add_desk(
                "stuck",
                Some(task.id),
                None,
                None,
                &format!(
                    "Stuck: {}. It was nudged once and did not move.",
                    task.title
                ),
                now,
            )?;
            self.emit(
                EventKind::TaskChanged,
                json!({ "task": task.id.0, "goal": task.goal.0, "owner": owner.0,
                        "status": TaskStatus::Blocked.name(), "priority": task.priority.name() }),
            )?;
            self.emit(
                EventKind::DeskAdded,
                json!({ "item": desk.0, "kind": "stuck", "task": task.id.0 }),
            )?;
            report.stuck += 1;
        }

        // 6. Tasks whose waits have all finished are woken, once.
        for task in &tasks {
            if task.waits_on.is_empty() || task.status.finished() {
                continue;
            }
            let Some(member) = task.owner.or(task.assignee) else {
                continue;
            };
            if self.store.woken_after_waits(task.id)? || !self.waits_over(&task.waits_on)? {
                continue;
            }
            self.store.set_woken_after_waits(task.id, true)?;
            self.wake_held_lock(member, Some(task.id), WakeReason::WaitFinished)?;
            report.woken += 1;
        }

        self.emit(
            EventKind::Tick,
            json!({ "started": report.started, "ended": report.ended, "stuck": report.stuck }),
        )?;
        Ok(report)
    }

    fn start_queued(&self, now: Millis) -> Done<usize> {
        let tasks = self.store.tasks()?;
        let priority = |run: &Run| {
            run.task
                .and_then(|t| tasks.iter().find(|task| task.id == t))
                .map_or(crate::domain::Priority::Normal, |t| t.priority)
        };
        let mut queued: Vec<Run> = self
            .store
            .runs()?
            .into_iter()
            .filter(|r| r.status == RunStatus::Queued)
            .collect();
        queued.sort_by_key(|r| (priority(r), r.queued_at, r.id));
        let mut started = 0;
        for mut run in queued {
            // A turn queued for work that has since finished (dropped, or done) is ended, not
            // started: nothing is left for it to do.
            let over = run
                .task
                .and_then(|t| tasks.iter().find(|task| task.id == t))
                .is_some_and(|t| t.status.finished());
            if over {
                self.end(run, RunStatus::Stopped, "its task is over")?;
                continue;
            }
            let running: Vec<Run> = self
                .store
                .runs()?
                .into_iter()
                .filter(|r| r.status == RunStatus::Running)
                .collect();
            if running.len() >= self.runner.capacity() {
                break;
            }
            let member = self.member(run.member)?;
            let busy = running.iter().filter(|r| r.member == member.id).count();
            if busy >= usize::from(member.slots) || member.paused {
                continue;
            }
            let Some(amount) = self.affordable(&member)? else {
                continue;
            };
            run.told = self.briefing(&member, &run)?;
            self.store
                .reserve(run.id, member.id, self.today(), amount, now)?;
            run.status = RunStatus::Running;
            run.attempts = run.attempts.saturating_add(1);
            run.started_at = Some(now);
            run.reserved = amount;
            self.store.save_run(&run)?;
            let context = RunContext {
                run: run.id,
                member: member.id,
                task: run.task,
                told: run.told.clone(),
                token: self.store.issue_token(run.id)?,
            };
            if let Err(why) = self.runner.start(&context) {
                self.fail(run, &format!("could not start: {}", why.0))?;
                continue;
            }
            self.store
                .set_wake(run.wake, Receipt::Started, Some(run.id))?;
            self.emit(
                EventKind::RunStarted,
                json!({ "run": run.id.0, "member": member.id.0, "task": run.task.map(|t| t.0),
                        "reserved": amount, "reason": self.reason_for(&run)? }),
            )?;
            started += 1;
        }
        Ok(started)
    }

    // ---- the team protocol (spec section 6) ------------------------------------------------

    /// The turn a token was issued for, while it runs: its run and its member. A token for a
    /// turn that has ended opens nothing.
    ///
    /// # Errors
    /// A store failure.
    pub fn turn_of(&self, token: &str) -> Done<Option<(Run, Member)>> {
        let Some(id) = self.store.run_of_token(token)? else {
            return Ok(None);
        };
        let Some(run) = self
            .store
            .run(id)?
            .filter(|r| r.status == RunStatus::Running)
        else {
            return Ok(None);
        };
        let member = self.member(run.member)?;
        Ok(Some((run, member)))
    }

    /// A member asks the owner something about its task: a question on the desk, and the end
    /// of its turn, which can do nothing useful until the owner has seen it. Seeing it wakes
    /// that member again. One question a member about a task waits at a time, so a turn
    /// cannot fill the owner's desk.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a turn with no task; [`EngineError::Refused`] with
    /// [`Refusal::TurnOver`] for a turn that has ended, or [`Refusal::AlreadyAsked`] while its
    /// last question waits; a store failure.
    pub fn ask_owner(&self, run: RunId, text: &str) -> Done<crate::domain::DeskId> {
        let _held = self.held();
        let run = self.running(run)?;
        let task = run.task.ok_or(EngineError::Unknown("task for this turn"))?;
        let member = self.member(run.member)?;
        let waiting = self.store.desk()?.into_iter().any(|d| {
            d.kind == "question"
                && d.status == "waiting"
                && d.member == Some(member.id)
                && d.task == Some(task)
        });
        if waiting {
            return Err(EngineError::Refused(Refusal::AlreadyAsked));
        }
        let text: String = text.trim().chars().take(600).collect();
        let desk = self.store.add_desk(
            "question",
            Some(task),
            None,
            Some(member.id),
            &format!("{} asks: {text}", member.name),
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": "question", "task": task.0 }),
        )?;
        // The worker ends its turn on reading that it has; its container is let go too, so a
        // worker that does not cannot linger.
        let id = run.id;
        self.end(run, RunStatus::Succeeded, "asked the owner")?;
        self.runner.stop(id, self.settings.grace);
        Ok(desk)
    }

    // ---- the owner's desk ---------------------------------------------------------------

    /// The owner decides an item on their desk. A proposed goal is `approve`d (it becomes
    /// active) or `decline`d (dropped); stuck work is tried again (`retry`: back to its
    /// assignee, with a fresh nudge) or `drop`ped; finished work is `merged` or left
    /// `not_now`. Each item is decided once.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for an item that does not exist or a decision it does not
    /// take; [`EngineError::Refused`] with [`Refusal::Finished`] if it is already decided.
    pub fn decide(&self, item: crate::domain::DeskId, decision: &str) -> Done<()> {
        let _held = self.held();
        let found = self.waiting_on_desk(item)?;
        self.decide_held_lock(&found, decision)
    }

    /// The owner answers a member's question: the answer goes in the task's thread, as the
    /// owner's, and then the question is seen, which wakes the member who asked to read it.
    ///
    /// # Errors
    /// As [`Self::decide`]; [`EngineError::Unknown`] for an item that is not a question or an
    /// answer with no words.
    pub fn answer(&self, item: crate::domain::DeskId, text: &str) -> Done<()> {
        let _held = self.held();
        let found = self.waiting_on_desk(item)?;
        let text = text.trim();
        if found.kind != "question" || text.is_empty() {
            return Err(EngineError::Unknown("answer for this item"));
        }
        let task = found.task.ok_or(EngineError::Unknown("task"))?;
        self.post_held_lock(Author::Owner, task, text)?;
        self.decide_held_lock(&found, "seen")
    }

    /// A desk item still waiting for the owner.
    fn waiting_on_desk(&self, item: crate::domain::DeskId) -> Done<DeskItem> {
        let found = self
            .store
            .desk()?
            .into_iter()
            .find(|d| d.id == item)
            .ok_or(EngineError::Unknown("desk item"))?;
        if found.status != "waiting" {
            return Err(EngineError::Refused(Refusal::Finished));
        }
        Ok(found)
    }

    fn decide_held_lock(&self, found: &DeskItem, decision: &str) -> Done<()> {
        let item = found.id;
        match (found.kind.as_str(), decision) {
            ("goal", "approve" | "decline") => {
                let goal = found
                    .goal
                    .and_then(|id| self.store.goal(id).transpose())
                    .transpose()?
                    .ok_or(EngineError::Unknown("goal"))?;
                let status = if decision == "approve" {
                    GoalStatus::Active
                } else {
                    GoalStatus::Dropped
                };
                self.store.set_goal_status(goal.id, status)?;
                self.emit(
                    EventKind::GoalChanged,
                    json!({ "goal": goal.id.0, "title": goal.title, "status": status.name() }),
                )?;
            }
            ("stuck", "retry") => {
                let task = self.task(found.task.ok_or(EngineError::Unknown("task"))?)?;
                let to = task.owner.or(task.assignee);
                self.store.release(task.id)?;
                self.store.set_task_status(task.id, TaskStatus::Todo)?;
                self.store.set_nudged(task.id, false)?;
                if let Some(member) = to {
                    self.wake_held_lock(member, Some(task.id), WakeReason::ProposalDecided)?;
                }
            }
            ("stuck", "drop") => {
                let task = self.task(found.task.ok_or(EngineError::Unknown("task"))?)?;
                self.store.set_task_status(task.id, TaskStatus::Dropped)?;
                self.after_finishing(&task)?;
            }
            ("question", "seen") => {
                // The member who asked carries on, reading the thread for any answer. A
                // question from before the desk knew who asked goes to whoever has the task.
                let task = self.task(found.task.ok_or(EngineError::Unknown("task"))?)?;
                if let Some(member) = found.member.or(task.owner).or(task.assignee) {
                    self.wake_held_lock(member, Some(task.id), WakeReason::ProposalDecided)?;
                }
            }
            ("ready_to_merge", "merged" | "not_now")
            | ("ready_to_read", "read" | "not_now")
            | ("question", "dismiss")
            | ("notice", "seen" | "dismiss") => {}
            _ => return Err(EngineError::Unknown("decision for this item")),
        }
        self.store.set_desk_status(item, decision)?;
        self.emit(
            EventKind::DeskDecided,
            json!({ "item": item.0, "kind": found.kind, "decision": decision }),
        )?;
        Ok(())
    }

    /// Tells the owner something done that needs no decision: a notice on the desk, only seen
    /// or dismissed. From a turn (`run`), it is from that turn's member and about its task:
    /// how Endora reports a safe fix a member made through a contract (ADR 0106).
    ///
    /// # Errors
    /// A store failure.
    pub fn notice(&self, run: Option<RunId>, text: &str) -> Done<crate::domain::DeskId> {
        let _held = self.held();
        let run = match run {
            Some(id) => self.store.run(id)?,
            None => None,
        };
        let text: String = text.trim().chars().take(600).collect();
        let task = run.as_ref().and_then(|r| r.task);
        let desk = self.store.add_desk(
            "notice",
            task,
            None,
            run.as_ref().map(|r| r.member),
            &text,
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": "notice", "task": task.map(|t| t.0) }),
        )?;
        Ok(desk)
    }

    /// Retires a goal: it is dropped, and so is every task under it not yet finished; their
    /// turns end, queued or running, and what waits on the desk about them is put away, so
    /// none of it becomes a turn again. Returns how many tasks it dropped. How HQ puts away the
    /// demo's work when it starts with the demo off.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a goal that does not exist; a store failure.
    pub fn retire_goal(&self, goal: GoalId, why: &str) -> Done<usize> {
        let _held = self.held();
        let found = self.store.goal(goal)?.ok_or(EngineError::Unknown("goal"))?;
        self.store.set_goal_status(goal, GoalStatus::Dropped)?;
        self.emit(
            EventKind::GoalChanged,
            json!({ "goal": goal.0, "title": found.title, "status": GoalStatus::Dropped.name() }),
        )?;
        let open: Vec<Task> = self
            .store
            .tasks()?
            .into_iter()
            .filter(|t| t.goal == goal && !t.status.finished())
            .collect();
        for task in &open {
            self.store.set_task_status(task.id, TaskStatus::Dropped)?;
            self.emit(
                EventKind::TaskChanged,
                json!({ "task": task.id.0, "goal": goal.0, "owner": task.owner.map(|o| o.0),
                        "status": TaskStatus::Dropped.name(), "priority": task.priority.name() }),
            )?;
        }
        let ids: Vec<TaskId> = open.iter().map(|t| t.id).collect();
        for run in self.store.runs()? {
            if !run.status.live() || !run.task.is_some_and(|t| ids.contains(&t)) {
                continue;
            }
            if run.status == RunStatus::Running {
                self.runner.stop(run.id, self.settings.grace);
            }
            self.end(run, RunStatus::Stopped, why)?;
        }
        for item in self.store.desk()? {
            let about = item.goal == Some(goal) || item.task.is_some_and(|t| ids.contains(&t));
            if item.status != "waiting" || !about {
                continue;
            }
            // Put away with the word the owner would have used to let it go.
            let decision = match item.kind.as_str() {
                "goal" => "decline",
                "stuck" => "drop",
                "ready_to_merge" | "ready_to_read" => "not_now",
                _ => "dismiss",
            };
            self.store.set_desk_status(item.id, decision)?;
            self.emit(
                EventKind::DeskDecided,
                json!({ "item": item.id.0, "kind": item.kind, "decision": decision }),
            )?;
        }
        Ok(open.len())
    }

    // ---- what the office shows ------------------------------------------------------------

    /// Everything on screen now (spec section 12), and the id of the last event in it: the
    /// event stream resumes after that id, so nothing is shown twice or missed.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn snapshot(&self) -> Done<serde_json::Value> {
        let _held = self.held();
        let day = self.today();
        let pool = self.settings.pool_per_month;
        let runs: Vec<Run> = self
            .store
            .runs()?
            .into_iter()
            .filter(|r| r.status.live())
            .collect();
        let tasks = self.store.tasks()?;
        let goals = self.store.goals()?;
        let mut members = Vec::new();
        for m in self.store.members()? {
            members.push(json!({
                "id": m.id.0, "name": m.name, "role": m.role.name(), "look": m.look,
                "paused": m.paused, "slots": m.slots,
                "per_day": money::member_a_day(pool, m.share_percent),
                "committed": self.store.committed(Some(m.id), day)?,
                "running": runs.iter().filter(|r| r.member == m.id && r.status == RunStatus::Running)
                    .map(|r| json!({ "run": r.id.0, "task": r.task.map(|t| t.0) }))
                    .collect::<Vec<_>>(),
            }));
        }
        let why = |task: &Task| {
            let mut chain = vec![task.title.clone()];
            let mut parent = task.parent;
            while let Some(id) = parent {
                let Some(up) = tasks.iter().find(|t| t.id == id) else {
                    break;
                };
                chain.push(up.title.clone());
                parent = up.parent;
            }
            if let Some(goal) = goals.iter().find(|g| g.id == task.goal) {
                chain.push(format!("Goal: {}", goal.title));
            }
            chain
        };
        let mut task_rows = Vec::new();
        for t in &tasks {
            task_rows.push(json!({
                "id": t.id.0, "goal": t.goal.0, "parent": t.parent.map(|p| p.0), "title": t.title,
                "description": t.description, "owner": t.owner.map(|o| o.0),
                "assignee": t.assignee.map(|a| a.0), "status": t.status.name(),
                "priority": t.priority.name(), "depth": t.depth,
                "waits_on": t.waits_on.iter().map(|w| w.0).collect::<Vec<_>>(),
                "gate": self.store.gate(t.id)?, "why": why(t),
                "thread": self.store.thread(t.id)?.iter()
                    .map(|p| json!({ "author": p.author, "text": p.text, "at": p.at }))
                    .collect::<Vec<_>>(),
            }));
        }
        let (ticks, last_tick_at) = self.store.count_events(EventKind::Tick.name())?;
        let (folded, _) = self.store.count_events(EventKind::WakeFolded.name())?;
        let held = self
            .store
            .wakes()?
            .iter()
            .filter(|w| w.receipt == crate::domain::Receipt::Held)
            .count();
        let queued = runs
            .iter()
            .filter(|r| r.status == RunStatus::Queued)
            .count();
        let running = runs
            .iter()
            .filter(|r| r.status == RunStatus::Running)
            .count();
        Ok(json!({
            "stopped": self.store.stopped()?,
            "last_event": self.store.last_event()?,
            "counts": { "ticks": ticks, "running": running, "queued": queued, "held": held,
                        "folded": folded, "open_tasks": tasks.iter().filter(|t| !t.status.finished()).count() },
            "last_tick_at": last_tick_at,
            "pool": { "per_day": money::pool_a_day(pool), "committed": self.store.committed(None, day)? },
            "members": members,
            "goals": goals.iter().map(|g| json!({
                "id": g.id.0, "title": g.title, "reason": g.reason, "status": g.status.name(),
                "parent": g.parent.map(|p| p.0),
            })).collect::<Vec<_>>(),
            "tasks": task_rows,
            "runs": runs.iter().map(|r| json!({
                "id": r.id.0, "member": r.member.0, "task": r.task.map(|t| t.0),
                "status": r.status.name(), "started_at": r.started_at, "reserved": r.reserved,
                "spent": r.spent,
            })).collect::<Vec<_>>(),
            "desk": self.store.desk()?.iter().filter(|d| d.status == "waiting").map(|d| json!({
                "id": d.id.0, "kind": d.kind, "task": d.task.map(|t| t.0), "goal": d.goal.map(|g| g.0),
                "text": d.text, "at": d.at,
            })).collect::<Vec<_>>(),
        }))
    }

    /// Keeps the newest `keep` events and deletes the rest. Returns how many went.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn prune_events(&self, keep: i64) -> Done<usize> {
        let _held = self.held();
        Ok(self.store.prune_events(keep)?)
    }

    /// The events after `id`, for the stream.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn events_after(&self, id: i64) -> Done<Vec<crate::domain::Event>> {
        Ok(self.store.events_after(id)?)
    }

    /// Every wake, with its receipt.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn wakes(&self) -> Done<Vec<Wake>> {
        Ok(self.store.wakes()?)
    }
}
