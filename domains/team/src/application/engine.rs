//! The engine (spec section 5): the only writer to the team's store, one change at a time.
//! Wakes and their receipts, claiming, threads, delegation, the tick, the ledger, the gate's
//! word on done, the kill switch, and the event feed.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;

use super::ports::{
    Clock, DeskItem, NewRoutine, NewTask, Run, RunContext, StoreError, TeamStore, Wake,
    WorkerAdapter,
};
use crate::domain::chain::{self, Decision};
use crate::domain::effort::{self, Effort};
use crate::domain::lessons;
use crate::domain::money::{self, Micros};
use crate::domain::review;
use crate::domain::routines::{self, FireStep, MINUTE_MS};
use crate::domain::runs::{self, LONGEST_TURN_MS};
use crate::domain::wakes::{self, LiveTurn};
use crate::domain::work::{self, mentions};
use crate::domain::{
    Brain, CheckBackStatus, EventKind, Firing, GoalId, GoalStatus, Liveness, Member, MemberId,
    Millis, Priority, Receipt, Refusal, Role, Routine, RoutineId, RunId, RunStatus, Task, TaskId,
    TaskStatus, WakeDecision, WakeReason, WakeSituation,
};
use crate::domain::{docs, house};

/// How long open, assigned work may sit with nothing queued, running or held for it before its
/// member is woken once, and again before it goes on the owner's desk as stuck.
pub const STRANDED_AFTER_MS: Millis = 10 * 60 * 1000;

/// The most characters a goal's notes from the owner may run to.
pub const GOAL_NOTE_MOST: usize = 2_000;

/// How long an approval waits for the owner before it goes unrun (ADR 0108): a day.
pub const APPROVALS_LAST_MS: Millis = 24 * 60 * 60 * 1000;

/// How far back the CEO's briefing looks for the home lab's trouble and the board's desk.
pub const CEO_LOOKS_BACK_MS: Millis = 7 * 24 * 60 * 60 * 1000;

/// The most lines of what changed the CEO is told on a turn that is not its daily review: the
/// newest, and how many earlier ones there were.
pub const CEO_NEWS_MOST: usize = 20;

/// An alarm the event carries, as the CEO reads it: Endora's, fired into a routine, or passed
/// into the open task of one.
fn alarm_in(event: &crate::domain::Event) -> Option<&str> {
    match event.kind.as_str() {
        "routine.fired" if event.data["firing"] == "alarm" => event.data["why"].as_str(),
        "routine.posted" => event.data["said"].as_str(),
        _ => None,
    }
}

/// A desk item's first line, cut to 160 characters.
fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(160)
        .collect()
}

/// Where HQ keeps the house log (ADR 0111).
const HOUSE_LOG: &str = "house_log";
/// Set once HQ has seated its CEO, so it is never seated again.
const CEO_SEATED: &str = "ceo_seated";

/// Endora's public records and plans, as `(path, text)`, for the CEO's `read_doc`.
pub type Library = &'static [(&'static str, &'static str)];

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
    /// Routines whose time came, fired or skipped.
    pub routines: usize,
    /// Check-backs that woke their member.
    pub check_backs: usize,
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

/// What a routine's firing did (ADR 0107).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fired {
    /// It handed its member this new task.
    Created(TaskId),
    /// It passed an alarm into its open task.
    Posted(TaskId),
    /// It did nothing, and why.
    Skipped(String),
}

/// One line, at most `most` characters: what a member or an alarm says, as it is stored.
fn one_line(text: &str, most: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(most)
        .collect()
}

/// Where a member's question went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// On the owner's desk, as this item.
    Desk(crate::domain::DeskId),
    /// To this reviewer first, as a draft in the task's thread.
    Review(MemberId),
    /// Up the chain of command, to this member, in the task's thread (ADR 0111).
    Up(MemberId),
}

/// The engine.
pub struct Engine<S, W, C> {
    store: S,
    runner: W,
    clock: C,
    settings: Settings,
    lock: Mutex<()>,
    /// The containers in Endora's home lab, as the node last said: what a question to the
    /// owner is checked against. `None` until the node has said.
    lab: Mutex<Option<Vec<String>>>,
    /// Endora's public records and plans, which the CEO may read.
    docs: Library,
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
            lab: Mutex::new(None),
            docs: &[],
        }
    }

    /// The engine with Endora's public records and plans, for the CEO's `read_doc`.
    #[must_use]
    pub fn with_docs(mut self, docs: Library) -> Self {
        self.docs = docs;
        self
    }

    /// The containers Endora's home lab has now, as the node says them: what each question to
    /// the owner is checked against before it reaches the desk.
    pub fn set_lab_names(&self, names: Vec<String>) {
        *self
            .lab
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(names);
    }

    /// The containers the node last said the home lab has, if it has said.
    fn lab_names(&self) -> Option<Vec<String>> {
        self.lab
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
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

    /// The weights of the members with a paid brain, together: what the day's pool is shared
    /// among (ADR 0111).
    fn paid_weights(&self) -> Done<i64> {
        Ok(self
            .store
            .members()?
            .iter()
            .filter(|m| m.brain.cloud())
            .map(|m| i64::from(m.share_percent))
            .sum())
    }

    /// A member's budget for one day: its weight's part of the pool among the paid, and
    /// nothing for a member on the house's model, which costs nothing.
    fn per_day(&self, member: &Member, paid: i64) -> Micros {
        if !member.brain.cloud() {
            return 0;
        }
        money::member_a_day(self.settings.pool_per_month, member.share_percent, paid)
    }

    /// What a member could have reserved for a turn now, if anything. A member on the house's
    /// model reserves nothing and is never turned away for money (ADR 0111).
    fn affordable(&self, member: &Member) -> Done<Option<Micros>> {
        if !member.brain.cloud() {
            return Ok(Some(0));
        }
        let day = self.today();
        let member_left = self.per_day(member, self.paid_weights()?)
            - self.store.committed(Some(member.id), day)?;
        let pool_left =
            money::pool_a_day(self.settings.pool_per_month) - self.store.committed(None, day)?;
        Ok(money::reservation(
            member_left,
            pool_left,
            money::most_a_turn(member.role),
        ))
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

    /// The owner sets what a member thinks with (ADR 0109), by its name: `local`, or a Claude
    /// model the member's role may use. The next turn starts with it. The node asks HQ on
    /// every call, so a turn already running is held to it at once: switched to the cloud, it
    /// is given only what a cloud turn may see; switched home, the Anthropic relay refuses it.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; [`Refusal::NotABrain`] for
    /// one its role may not think with; a store failure.
    pub fn set_brain(&self, member: MemberId, brain: &str) -> Done<Brain> {
        let _held = self.held();
        let found = self.member(member)?;
        let brain = Brain::for_role(found.role, brain).map_err(EngineError::Refused)?;
        self.store.set_brain(member, brain)?;
        self.emit(
            EventKind::BrainSet,
            json!({ "member": member.0, "name": found.name, "brain": brain.name() }),
        )?;
        Ok(brain)
    }

    /// The owner renames a member: one to 40 characters on one line, and no other member's
    /// name, since members are named in threads by it. Returns the name as kept.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; [`Refusal::NotAName`] for
    /// an empty name, one too long, or another member's; a store failure.
    pub fn rename(&self, member: MemberId, name: &str) -> Done<String> {
        let _held = self.held();
        let found = self.member(member)?;
        let name = one_line(name, 41);
        let taken = self
            .store
            .members()?
            .iter()
            .any(|m| m.id != member && m.name.eq_ignore_ascii_case(&name));
        if name.is_empty() || name.chars().count() > 40 || taken {
            return Err(EngineError::Refused(Refusal::NotAName));
        }
        self.store.rename_member(member, &name)?;
        self.emit(
            EventKind::MemberRenamed,
            json!({ "member": member.0, "was": found.name, "name": name }),
        )?;
        Ok(name)
    }

    /// Seats the team's CEO (ADR 0111), once: a member named `name` in the CEO's role, with
    /// its weight in the pool and Claude Opus to think with, said in the feed. Nothing when the
    /// team has a CEO, or HQ has seated one before (the owner may have let it go since).
    ///
    /// # Errors
    /// A store failure.
    pub fn seat_ceo(&self, name: &str) -> Done<Option<MemberId>> {
        let _held = self.held();
        let has_one = self.store.members()?.iter().any(|m| m.role == Role::Ceo);
        if has_one || self.store.setting(CEO_SEATED)?.is_some() {
            return Ok(None);
        }
        let id = self
            .store
            .add_member(name, Role::Ceo, Role::Ceo.share(), 1, 3)?;
        self.store.set_brain(id, Brain::Opus)?;
        self.store.set_setting(CEO_SEATED, "1")?;
        self.emit(
            EventKind::MemberHired,
            json!({ "member": id.0, "name": name, "role": Role::Ceo.name(),
                    "brain": Brain::Opus.name(), "by": "endora" }),
        )?;
        Ok(Some(id))
    }

    // ---- the house log (ADR 0111) ---------------------------------------------------------

    /// The house log: empty when nothing is in it.
    ///
    /// # Errors
    /// A store failure.
    pub fn house_log(&self) -> Done<String> {
        Ok(self.store.setting(HOUSE_LOG)?.unwrap_or_default())
    }

    fn keep_house_log(&self, log: &str, by: &str, said: serde_json::Value) -> Done<()> {
        self.store.set_setting(HOUSE_LOG, log)?;
        let mut about = json!({ "by": by, "length": log.chars().count() });
        if let (Some(about), Some(said)) = (about.as_object_mut(), said.as_object()) {
            about.extend(said.clone());
        }
        self.emit(EventKind::HouseNoted, about)
    }

    /// The owner rewrites the house log on HQ's screen; empty clears it.
    ///
    /// # Errors
    /// [`Refusal::TooLong`] past [`house::HOUSE_LOG_MOST`]; a store failure.
    pub fn set_house_log(&self, log: &str) -> Done<()> {
        let _held = self.held();
        let log = house::rewritten(log).map_err(EngineError::Refused)?;
        self.keep_house_log(&log, "owner", json!({ "rewritten": true }))
    }

    /// A turn of the CEO's adds a dated line to the house log. Returns the line.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::OnlyTheCeo`] for anyone else; [`EngineError::Unknown`]
    /// for no entry; [`Refusal::TooLong`] past [`house::HOUSE_ENTRY_MOST`];
    /// [`Refusal::HouseLogFull`] when it has no room.
    pub fn note_house(&self, run: RunId, entry: &str) -> Done<String> {
        let _held = self.held();
        let (run, ceo) = self.the_ceos_turn(run)?;
        if entry.trim().is_empty() {
            return Err(EngineError::Unknown("entry for the house log"));
        }
        let line = house::line(entry, self.clock.now(), self.settings.offset_minutes)
            .map_err(EngineError::Refused)?;
        let log = house::appended(&self.house_log()?, &line).map_err(EngineError::Refused)?;
        self.keep_house_log(&log, &ceo.name, json!({ "entry": line }))?;
        if let Some(task) = run.task {
            self.mark_progress(ceo.id, task)?;
        }
        Ok(line)
    }

    /// A turn of the CEO's rewrites the house log whole, compactly.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::OnlyTheCeo`]; [`Refusal::TooLong`] past
    /// [`house::HOUSE_LOG_MOST`].
    pub fn rewrite_house(&self, run: RunId, log: &str) -> Done<usize> {
        let _held = self.held();
        let (run, ceo) = self.the_ceos_turn(run)?;
        let log = house::rewritten(log).map_err(EngineError::Refused)?;
        self.keep_house_log(&log, &ceo.name, json!({ "rewritten": true }))?;
        if let Some(task) = run.task {
            self.mark_progress(ceo.id, task)?;
        }
        Ok(log.chars().count())
    }

    /// A running turn of the CEO's, and the CEO.
    fn the_ceos_turn(&self, run: RunId) -> Done<(Run, Member)> {
        let run = self.running(run)?;
        let member = self.member(run.member)?;
        if member.role != Role::Ceo {
            return Err(EngineError::Refused(Refusal::OnlyTheCeo));
        }
        Ok((run, member))
    }

    /// A turn of the CEO's reads one of Endora's public records or plans, a part at a time
    /// ([`docs::DOC_PART`] characters), or the list of a folder's. They are public already;
    /// nothing else is read.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::OnlyTheCeo`]; [`Refusal::NotADoc`] for any other
    /// path; [`EngineError::Unknown`] for a document there is not.
    pub fn read_doc(&self, run: RunId, path: &str, part: usize) -> Done<String> {
        self.the_ceos_turn(run)?;
        match docs::asked(path).map_err(EngineError::Refused)? {
            docs::Asked::Folder(folder) => {
                let names: Vec<&str> = self
                    .docs
                    .iter()
                    .map(|(p, _)| *p)
                    .filter(|p| p.rsplit_once('/').is_some_and(|(f, _)| f == folder))
                    .collect();
                Ok(format!("In {folder}:\n{}", names.join("\n")))
            }
            docs::Asked::Doc(path) => {
                let (_, text) =
                    self.docs
                        .iter()
                        .find(|(p, _)| *p == path)
                        .ok_or(EngineError::Unknown(
                            "document: read_doc docs/adr or docs/plans to list them",
                        ))?;
                let part = part.max(1);
                let (said, parts) = docs::part(text, part);
                let next = if part < parts {
                    format!(" Read on with part {}.", part + 1)
                } else {
                    String::new()
                };
                Ok(format!("{path}, part {part} of {parts}.{next}\n\n{said}"))
            }
        }
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
        self.add_task_held_lock(by, task)
    }

    fn add_task_held_lock(&self, by: Author, task: NewTask) -> Done<TaskId> {
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

    /// Whether a member has a task of its own that is not over: handed to it, or taken.
    fn has_open_task(&self, member: MemberId) -> Done<bool> {
        Ok(self.store.tasks()?.iter().any(|t| {
            !t.status.finished() && (t.assignee == Some(member) || t.owner == Some(member))
        }))
    }

    fn situation(
        &self,
        member: &Member,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<WakeSituation> {
        let nothing_to_do =
            task.is_none() && reason.on_a_clock() && !self.has_open_task(member.id)?;
        let already_held = match task {
            Some(task) => self
                .store
                .held_wakes(task)?
                .iter()
                .any(|w| w.member == member.id),
            None => false,
        };
        let waits_for_review = match task {
            Some(task) if !reason.reaches_work_in_review() => {
                let task = self.task(task)?;
                task.status == TaskStatus::InReview
                    && task.owner.or(task.assignee) == Some(member.id)
            }
            _ => false,
        };
        Ok(WakeSituation {
            waits_for_review,
            not_for_the_ceo: member.role == Role::Ceo && !reason.reaches_the_ceo(),
            stopped: self.store.stopped()?,
            paused: member.paused,
            can_afford_a_turn: self.affordable(member)?.is_some(),
            live: self.live_turns(member.id, task)?,
            already_held,
            nothing_to_do,
        })
    }

    fn wake_held_lock(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<Receipt> {
        let who = self.member(member)?;
        // News is never kept waiting: a turn on its task held back by the back-off, or waiting
        // to try again, is brought forward to take it.
        if let (Some(task), true) = (task, reason.brings_news()) {
            self.bring_forward(task)?;
        }
        let decision = wakes::decide(member, &self.situation(&who, task, reason)?);
        let now = self.clock.now();
        let mut about = json!({
            "member": member.0, "task": task.map(|t| t.0), "reason": reason.name(),
        });
        match decision {
            WakeDecision::Queue => {
                let wake = self
                    .store
                    .add_wake(member, task, reason, Receipt::Queued, None, now)?;
                let (run, delayed) = self.queue_turn(member, task, wake, reason)?;
                self.store.set_wake(wake, Receipt::Queued, Some(run))?;
                if let Some(until) = delayed {
                    about["delayed_until"] = json!(until);
                }
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
                about["why"] = json!(why.name());
                self.emit(EventKind::WakeRefused, about)?;
                Ok(Receipt::Refused(why))
            }
        }
    }

    /// How many turns in a row on a task, newest first, moved nothing forward.
    fn empty_turns(&self, task: TaskId) -> Done<usize> {
        let mut ended: Vec<Run> = self
            .store
            .runs()?
            .into_iter()
            .filter(|r| r.task == Some(task) && !r.status.live() && r.started_at.is_some())
            .collect();
        ended.sort_by_key(|r| core::cmp::Reverse(r.id));
        Ok(ended.iter().take_while(|r| !r.moved_forward).count())
    }

    /// How long a wake that brings nothing new to `task` waits now (ADR 0107).
    fn back_off(&self, task: Option<TaskId>, reason: WakeReason) -> Done<Millis> {
        let Some(task) = task else {
            return Ok(0);
        };
        if reason.brings_news() {
            return Ok(0);
        }
        Ok(routines::back_off(self.empty_turns(task)?))
    }

    /// Queues a turn for a wake: at once, or, for a stale wake on a task whose turns keep
    /// moving nothing, waiting out its back-off. Returns the turn, and until when it waits.
    fn queue_turn(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        wake: crate::domain::WakeId,
        reason: WakeReason,
    ) -> Done<(RunId, Option<Millis>)> {
        let now = self.clock.now();
        let id = self.store.add_run(member, task, wake, now)?;
        let wait = self.back_off(task, reason)?;
        if wait == 0 {
            return Ok((id, None));
        }
        let mut run = self.store.run(id)?.ok_or(EngineError::Unknown("run"))?;
        run.status = RunStatus::RetryWaiting;
        run.retry_at = Some(now + wait);
        run.end_reason = "backing off: its last turns moved nothing".to_owned();
        self.store.save_run(&run)?;
        Ok((id, Some(now + wait)))
    }

    /// Brings a task's waiting turn forward, to start on the next tick.
    fn bring_forward(&self, task: TaskId) -> Done<()> {
        for mut run in self.store.runs()? {
            if run.task != Some(task) || run.status != RunStatus::RetryWaiting {
                continue;
            }
            run.status = RunStatus::Queued;
            run.retry_at = None;
            self.store.save_run(&run)?;
        }
        Ok(())
    }

    /// Releases the oldest wake held for a task, now that its turn has ended.
    fn release_held(&self, task: TaskId) -> Done<()> {
        let Some(oldest) = self.store.held_wakes(task)?.into_iter().next() else {
            return Ok(());
        };
        let who = self.member(oldest.member)?;
        let decision = wakes::decide(
            oldest.member,
            &self.situation(&who, Some(task), oldest.reason)?,
        );
        match decision {
            WakeDecision::Queue => {
                let (run, _) =
                    self.queue_turn(oldest.member, Some(task), oldest.id, oldest.reason)?;
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
    /// names the member (`@name`, as a wake reads it). The lead and the CEO may act on any task
    /// in a goal they lead; a team has one of each, which lead every goal.
    ///
    /// # Errors
    /// [`EngineError::Refused`] with [`Refusal::OutOfReach`] when it may not.
    fn reach(&self, run: &Run, task: TaskId, writing: bool) -> Done<()> {
        let target = self.task(task)?;
        let member = self.member(run.member)?;
        if member.role.leads() {
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

    /// A turn writes in a task's thread, within its reach for writing. A post that answers a
    /// question asked of the member up the chain wakes whoever asked it, as the owner's reply
    /// does (ADR 0111).
    ///
    /// # Errors
    /// [`Refusal::TurnOver`], [`Refusal::OutOfReach`], or a task that does not exist.
    pub fn post_in(&self, run: RunId, task: TaskId, text: &str) -> Done<()> {
        let _held = self.held();
        let run = self.running(run)?;
        self.reach(&run, task, true)?;
        let member = self.member(run.member)?;
        let askers = if text.trim_start().starts_with(chain::QUESTION_FOR) {
            Vec::new()
        } else {
            self.askers_of(&member, task)?
        };
        self.post_held_lock(Author::Member(member.id), task, text)?;
        let members = self.store.members()?;
        let named = mentions(text, &members);
        for asker in askers {
            if asker.id == member.id || named.iter().any(|m| m.id == asker.id) {
                continue;
            }
            self.wake_held_lock(asker.id, Some(task), WakeReason::QuestionAnswered)?;
        }
        Ok(())
    }

    /// The members whose questions up the chain to `member`, in `task`'s thread, it has not
    /// answered yet.
    fn askers_of(&self, member: &Member, task: TaskId) -> Done<Vec<Member>> {
        let thread = self.store.thread(task)?;
        let posts: Vec<(&str, &str)> = thread
            .iter()
            .map(|p| (p.author.as_str(), p.text.as_str()))
            .collect();
        let members = self.store.members()?;
        let mut askers: Vec<Member> = Vec::new();
        for (name, _) in chain::unanswered(&posts, &member.name) {
            let Some(asker) = members.iter().find(|m| m.name == name) else {
                continue;
            };
            if !askers.iter().any(|a| a.id == asker.id) {
                askers.push(asker.clone());
            }
        }
        Ok(askers)
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
        if by != Author::Owner {
            return self.post_held_lock(by, task, text);
        }
        // The owner's reply on an open task wakes whoever has it at once, named or not: it is
        // new, so the back-off never holds it.
        let found = self.task(task)?;
        if found.status.finished() {
            return Err(EngineError::Refused(Refusal::Finished));
        }
        self.post_held_lock(by, task, text)?;
        let Some(member) = found.owner.or(found.assignee) else {
            return Ok(());
        };
        let members = self.store.members()?;
        if mentions(text, &members).iter().any(|m| m.id == member) {
            return Ok(());
        }
        self.wake_held_lock(member, Some(task), WakeReason::OwnerReplied)?;
        Ok(())
    }

    /// The owner's notes for every member working towards a goal, at most
    /// [`GOAL_NOTE_MOST`] characters; empty clears them. Each briefing for a task under the
    /// goal carries them near its top.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a goal that does not exist; [`Refusal::TooLong`] for notes
    /// past the most; a store failure.
    pub fn set_goal_note(&self, goal: GoalId, note: &str) -> Done<()> {
        let _held = self.held();
        self.store.goal(goal)?.ok_or(EngineError::Unknown("goal"))?;
        let note = note.trim();
        if note.chars().count() > GOAL_NOTE_MOST {
            return Err(EngineError::Refused(Refusal::TooLong(GOAL_NOTE_MOST)));
        }
        self.store.set_goal_note(goal, note)?;
        Ok(())
    }

    /// A goal's notes from the owner: empty when there are none.
    ///
    /// # Errors
    /// A store failure.
    pub fn goal_note(&self, goal: GoalId) -> Done<String> {
        Ok(self.store.goal_note(goal)?)
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
        // A routine's check changes no code: its member closes it (ADR 0107).
        let check = to == TaskStatus::Done && self.routine_of(task)?.is_some();
        if check {
            work::may_close_check(current.status).map_err(EngineError::Refused)?;
        } else {
            work::may_set_status(current.status, to).map_err(EngineError::Refused)?;
        }
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
        if check {
            self.after_finishing(&current)?;
        }
        if to == TaskStatus::InReview {
            self.review_asked(&current)?;
        }
        Ok(())
    }

    /// The team's reviewer: the first member with the reviewer's role, if there is one.
    fn reviewer(&self) -> Done<Option<Member>> {
        Ok(self
            .store
            .members()?
            .into_iter()
            .filter(|m| m.role == Role::Reviewer)
            .min_by_key(|m| m.id))
    }

    /// Work was moved to review by its owner: the reviewer is woken to look at it. With no
    /// reviewer, or the reviewer's own work, it is on the owner's desk to read, with its
    /// thread, as it stands.
    fn review_asked(&self, task: &Task) -> Done<()> {
        let reviewer = self
            .reviewer()?
            .filter(|r| Some(r.id) != task.owner.or(task.assignee));
        if let Some(reviewer) = reviewer {
            self.wake_held_lock(reviewer.id, Some(task.id), WakeReason::ReviewRequested)?;
            return Ok(());
        }
        let thread: Vec<String> = self
            .store
            .thread(task.id)?
            .iter()
            .rev()
            .take(12)
            .rev()
            .map(|p| format!("- {}: {}", p.author, p.text))
            .collect();
        let desk = self.store.add_desk(
            "ready_to_read",
            Some(task.id),
            None,
            None,
            &format!(
                "Ready to read: {} (nobody on the team reviews)\n{}",
                task.title,
                thread.join("\n")
            ),
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": "ready_to_read", "task": task.id.0 }),
        )?;
        Ok(())
    }

    /// The reviewer's verdict on work in review, in its turn: `approve` closes work that needs
    /// no gate as done (work that needs the gate waits for it, as [`Self::approve`] says), and
    /// `send_back` returns it to its owner as `doing`, waking them. The notes go in the thread
    /// either way. Only the reviewer gives a verdict, and never on its own work.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`], [`Refusal::OutOfReach`]; [`Refusal::NotYours`] for a member who
    /// is not a reviewer; [`Refusal::NotAStep`] for a task not in review; [`Refusal::OwnWork`];
    /// [`Refusal::GateNotPassed`] for work that needs the gate.
    pub fn review_in(&self, run: RunId, task: TaskId, approve: bool, notes: &str) -> Done<()> {
        let _held = self.held();
        let run = self.running(run)?;
        self.reach(&run, task, true)?;
        let reviewer = self.member(run.member)?;
        if reviewer.role != Role::Reviewer {
            return Err(EngineError::Refused(Refusal::NotYours));
        }
        let current = self.task(task)?;
        let to = if approve {
            TaskStatus::Done
        } else {
            TaskStatus::Doing
        };
        if current.status != TaskStatus::InReview {
            return Err(EngineError::Refused(Refusal::NotAStep {
                from: current.status,
                to,
            }));
        }
        let worker = current.owner.or(current.assignee);
        if worker == Some(reviewer.id) {
            return Err(EngineError::Refused(Refusal::OwnWork));
        }
        if !approve {
            self.store.set_task_status(task, TaskStatus::Doing)?;
            self.post_held_lock(
                Author::Member(reviewer.id),
                task,
                &format!("Sent back: {notes}"),
            )?;
            self.emit_status(&current, TaskStatus::Doing)?;
            if let Some(owner) = worker {
                self.wake_held_lock(owner, Some(task), WakeReason::SentBack)?;
            }
            return Ok(());
        }
        let the_home_labs = self.the_home_labs(&current)?;
        let gated = review::needs_gate(self.routine_of(task)?.is_some(), the_home_labs);
        if gated {
            // Gated work keeps the gate's rule: done only once the gate has passed.
            self.approve_held_lock(reviewer.id, task)?;
            self.post_held_lock(
                Author::Member(reviewer.id),
                task,
                &format!("Approved: {notes}"),
            )?;
            return Ok(());
        }
        review::may_approve(false, false, worker, reviewer.id).map_err(EngineError::Refused)?;
        self.store.set_task_status(task, TaskStatus::Done)?;
        self.post_held_lock(
            Author::Member(reviewer.id),
            task,
            &format!("Approved: {notes}"),
        )?;
        self.emit_status(&current, TaskStatus::Done)?;
        self.after_finishing(&current)
    }

    /// Says a task moved to `to`, in the feed.
    fn emit_status(&self, current: &Task, to: TaskStatus) -> Done<()> {
        self.emit(
            EventKind::TaskChanged,
            json!({ "task": current.id.0, "goal": current.goal.0,
                    "owner": current.owner.map(|o| o.0), "status": to.name(),
                    "priority": current.priority.name() }),
        )
    }

    /// Whether a task is the home lab's: under a goal whose routine takes its alarms.
    fn the_home_labs(&self, task: &Task) -> Done<bool> {
        Ok(self
            .store
            .routines()?
            .iter()
            .any(|r| r.goal == task.goal && r.on_lab_trouble))
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
        self.approve_held_lock(reviewer, task)
    }

    fn approve_held_lock(&self, reviewer: MemberId, task: TaskId) -> Done<()> {
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

    /// What one model call of a turn sent, in input tokens, as the relay reported them: fresh,
    /// read from the prompt cache and written to it. The turn's record keeps the sums, and
    /// its end says how much of its input the cache gave (see [`runs::cached_share`]).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a run that does not exist; a store failure.
    pub fn note_tokens(&self, run: RunId, fresh: i64, read: i64, written: i64) -> Done<()> {
        let _held = self.held();
        self.store.run(run)?.ok_or(EngineError::Unknown("run"))?;
        self.store.add_tokens(run, fresh, read, written)?;
        Ok(())
    }

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
        let mut current = self.store.run(run)?.ok_or(EngineError::Unknown("run"))?;
        let most = money::most_a_turn(self.member(current.member)?.role);
        let amount = amount.clamp(0, most);
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
    /// How hard the member's Claude tries this turn, by its role and what the turn is for
    /// (see [`effort::effort_for`]): for the CEO, whether it weighs a decision.
    fn effort_of(&self, member: &Member, run: &Run) -> Done<Option<Effort>> {
        if member.role != Role::Ceo {
            return Ok(effort::effort_for(member.brain, member.role, false));
        }
        let drafted = match run.task {
            Some(task) => self.ceos_draft(&self.task(task)?)?.is_some(),
            None => false,
        };
        let reason = WakeReason::from_name(self.reason_for(run)?);
        let weighty = reason.map_or(drafted, |r| effort::weighs_a_decision(r, drafted));
        Ok(effort::effort_for(member.brain, member.role, weighty))
    }

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
        // The tokens as the relay reported them, which only it adds to.
        let cached = self.store.run(run.id)?.and_then(|r| {
            runs::cached_share(r.input_tokens, r.cache_read_tokens, r.cache_write_tokens)
        });
        self.emit(
            EventKind::RunEnded,
            json!({ "run": run.id.0, "member": run.member.0, "task": run.task.map(|t| t.0),
                    "result": status.name(), "why": why, "moved_forward": run.moved_forward,
                    "spent": run.spent, "cached": cached }),
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
        // A retry is a stale continuation: it backs off like one.
        let due = due.max(now + self.back_off(run.task, WakeReason::Retry)?);
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

    /// The lead's paragraph about the team: who is on it, with each one's role and brain, and
    /// that the lead plans and hands work on rather than doing it all. Live, a lead on a small
    /// model never delegated once; and a lead on a cloud brain may not take every step (the
    /// house's Confidential actions stay with local members, ADR 0109).
    fn team_for_the_lead(&self, lead: &Member) -> Done<String> {
        let others: Vec<String> = self
            .store
            .members()?
            .into_iter()
            .filter(|m| m.id != lead.id && !m.paused)
            .map(|m| format!("{} ({}, {})", m.name, m.role.name(), m.brain.label()))
            .collect();
        if others.is_empty() {
            return Ok(String::new());
        }
        Ok(format!(
            "Your team: {}. You lead: plan the work and hand its parts on with create_task \
             (fixes to the developer, look-ups to the researcher); the reviewer checks work \
             before it counts. When a step is one your tools refuse you, hand it to a member \
             who may take it, rather than leaving it undone.\n",
            others.join(", ")
        ))
    }

    /// What a member is told at the start of a turn.
    fn briefing(&self, member: &Member, run: &Run) -> Done<String> {
        let mut told = format!(
            "You are {}, the team's {}.\n",
            member.name,
            if member.role == Role::Ceo {
                "CEO"
            } else {
                member.role.name()
            }
        );
        if member.role == Role::Ceo {
            told.push_str(&self.ceo_context(member, run)?);
        }
        let log = self.house_log()?;
        if member.role == Role::Lead && !log.trim().is_empty() {
            told.push_str(&format!(
                "The house log, kept by the CEO (read it; it is not yours to change):\n{}\n",
                log.trim()
            ));
        }
        if let Some(above) = self.above(member)? {
            told.push_str(&format!(
                "Your questions (ask_owner) go to {}, the {}, in the task's thread, and they \
                 answer there: the owner's desk is the CEO's to use.\n",
                above.name,
                if above.role == Role::Ceo {
                    "CEO"
                } else {
                    above.role.name()
                }
            ));
        }
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
        let note = self.store.goal_note(task.goal)?;
        if !note.is_empty() {
            told.push_str(&format!("Notes from the owner for this goal: {note}\n"));
        }
        if member.role == Role::Lead {
            told.push_str(&self.team_for_the_lead(member)?);
        }
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
        for came_back in self.store.check_backs()?.iter().filter(|c| {
            c.member == member.id
                && c.task == task.id
                && c.status == CheckBackStatus::Fired
                && c.done_at.is_some_and(|at| at > since)
        }) {
            told.push_str(&format!(
                "You said you would check back: {}\n",
                came_back.why
            ));
        }
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
        if self.reason_for(run)? == WakeReason::QuestionAsked.name() {
            let thread = self.store.thread(task.id)?;
            let posts: Vec<(&str, &str)> = thread
                .iter()
                .map(|p| (p.author.as_str(), p.text.as_str()))
                .collect();
            for (asker, question) in chain::unanswered(&posts, &member.name) {
                told.push_str(&format!("{asker} asks you: {question}\n"));
            }
            told.push_str(if member.role == Role::Ceo {
                "Answer in the thread with post: whoever asked is woken when you do. Decide it \
                 yourself, or hand the work on with create_task; only what the board alone \
                 decides goes to them, as a decision with ask_owner.\n"
            } else {
                "Answer in the thread with post: whoever asked is woken when you do. Decide it, \
                 hand the work on with create_task, or, if it is beyond you, ask up the chain \
                 with ask_owner.\n"
            });
        }
        if self.reason_for(run)? == WakeReason::ReviewRequested.name() {
            // The newest of a draft for the owner and a move to review says which this is.
            let members = self.store.members()?;
            let asked = self.store.thread(task.id)?.into_iter().rev().find_map(|p| {
                if let Some(draft) = p.text.strip_prefix(review::DRAFT) {
                    let the_ceos = members
                        .iter()
                        .any(|m| m.role == Role::Ceo && m.name == p.author);
                    if the_ceos {
                        return Some(format!(
                            "{}, the CEO, wrote this decision for the board:\n{draft}\n{}",
                            p.author,
                            review::REVIEW_BOARD_DRAFT
                        ));
                    }
                    return Some(format!(
                        "{} wrote this draft for the owner: {draft}\n{}",
                        p.author,
                        review::REVIEW_DRAFT
                    ));
                }
                p.text.contains(" → in_review").then(|| {
                    format!(
                        "{} asks for your review.\n{}",
                        p.author,
                        review::REVIEW_WORK
                    )
                })
            });
            if let Some(asked) = asked {
                told.push_str(&format!("{asked}\n"));
            }
        }
        if member.role == Role::Ceo {
            told.push_str(
                "Your tools: my_turn, claim, post, set_status, create_task, ask_owner (a \
                 decision for the board), check_back, note_house, rewrite_house, read_doc.\n",
            );
            return Ok(told);
        }
        told.push_str(
            "Your tools: my_turn, claim, post, set_status, create_task, ask_owner, check_back, \
             run_gate, request_action",
        );
        told.push_str(if member.role == Role::Reviewer {
            ", review.\n"
        } else {
            ".\n"
        });
        Ok(told)
    }

    /// What the CEO is told first (ADR 0111). It opens with what changes rarely, always in one
    /// order: the house log, its team, the goals, and what only the board decides. Then, on
    /// its first turn and its daily review, the whole picture: the goals' notes and lessons,
    /// the open work, the home lab's trouble and what reached the board's desk in the last
    /// week. On any other turn, only what changed since its last turn began (see
    /// [`Self::ceo_news`]): what it read before is in the house log it keeps and the threads,
    /// and every turn's briefing is paid for again at each of its rounds. Nothing of the
    /// owner's brain, mail or chat: the CEO thinks on a cloud model.
    fn ceo_context(&self, ceo: &Member, run: &Run) -> Done<String> {
        let mut told = String::new();
        let log = self.house_log()?;
        told.push_str(
            "The house log, yours to keep (note_house adds a dated line, rewrite_house rewrites \
             it whole when it fills; the owner reads and edits it):\n",
        );
        told.push_str(if log.trim().is_empty() {
            "(empty)"
        } else {
            log.trim()
        });
        told.push('\n');
        let members = self.store.members()?;
        let name_of = |id: Option<MemberId>| {
            id.and_then(|id| members.iter().find(|m| m.id == id))
                .map_or_else(|| "nobody".to_owned(), |m| m.name.clone())
        };
        let team: Vec<String> = members
            .iter()
            .filter(|m| m.id != ceo.id)
            .map(|m| {
                format!(
                    "{} ({}, {}{})",
                    m.name,
                    m.role.name(),
                    m.brain.label(),
                    if m.paused { ", paused" } else { "" }
                )
            })
            .collect();
        told.push_str(&format!(
            "Your team: {}. The lead runs the work; you decide what it brings you.\n",
            if team.is_empty() {
                "nobody yet".to_owned()
            } else {
                team.join(", ")
            }
        ));
        told.push_str(chain::BOARD_ITEMS);
        told.push_str(
            "\nYou wake only for a question up the chain, the board's word, and your daily \
             review. Members' findings are in their tasks' threads: read those, never the house's \
             logs yourself. Endora's public records and plans are yours to read with read_doc.\n",
        );
        let last = self
            .store
            .runs()?
            .iter()
            .filter(|r| r.member == ceo.id && r.id != run.id)
            .filter_map(|r| r.started_at)
            .max();
        let review = match run.task {
            Some(task) => self.routine_of(task)?.is_some(),
            None => false,
        };
        let whole = review || last.is_none();
        told.push_str("Goals:\n");
        let goals = self.store.goals()?;
        let open_goals: Vec<_> = goals
            .iter()
            .filter(|g| matches!(g.status, GoalStatus::Active | GoalStatus::Proposed))
            .collect();
        if open_goals.is_empty() {
            told.push_str("- none\n");
        }
        for goal in open_goals {
            told.push_str(&format!(
                "- #{} {} ({})\n",
                goal.id,
                goal.title,
                goal.status.name()
            ));
            let notes = self.store.goal_note(goal.id)?;
            if whole && !notes.trim().is_empty() {
                told.push_str("  Its notes and lessons:\n");
                for line in notes.lines().filter(|l| !l.trim().is_empty()) {
                    told.push_str(&format!("  {}\n", line.trim()));
                }
            }
        }
        let day = |at: Millis| {
            let (d, m) = lessons::day_and_month(money::day_of(at, self.settings.offset_minutes));
            format!("{d} {m}")
        };
        if let (false, Some(last)) = (whole, last) {
            let news = self.ceo_news(last, &name_of, &day)?;
            if news.is_empty() {
                told.push_str(
                    "Nothing changed since your last turn: no task made or moved, no alarm, no \
                     word from the board, no lesson. Your daily review shows the whole.\n",
                );
                return Ok(told);
            }
            told.push_str("What changed since your last turn:\n");
            for line in news {
                told.push_str(&format!("{line}\n"));
            }
            return Ok(told);
        }
        let open: Vec<Task> = self
            .store
            .tasks()?
            .into_iter()
            .filter(|t| !t.status.finished())
            .collect();
        told.push_str("Open tasks:\n");
        if open.is_empty() {
            told.push_str("- none\n");
        }
        for task in open.iter().take(30) {
            told.push_str(&format!(
                "- #{} {} ({}, {})\n",
                task.id,
                task.title,
                task.status.name(),
                name_of(task.owner.or(task.assignee))
            ));
        }
        if open.len() > 30 {
            told.push_str(&format!("- and {} more\n", open.len() - 30));
        }
        let since = self.clock.now() - CEO_LOOKS_BACK_MS;
        let alarms: Vec<String> = self
            .store
            .events_after(0)?
            .into_iter()
            .filter(|e| e.at >= since)
            .filter_map(|e| Some(format!("- {}: {}", day(e.at), alarm_in(&e)?)))
            .collect();
        if alarms.is_empty() {
            told.push_str("Home-lab trouble in the last 7 days: no alarms.\n");
        } else {
            told.push_str("Home-lab trouble in the last 7 days (Endora's alarms):\n");
            for alarm in alarms.iter().rev().take(20).rev() {
                told.push_str(&format!("{alarm}\n"));
            }
        }
        let desk: Vec<DeskItem> = self
            .store
            .desk()?
            .into_iter()
            .filter(|d| d.at >= since)
            .collect();
        if desk.is_empty() {
            told.push_str("The board's desk in the last 7 days: nothing.\n");
            return Ok(told);
        }
        told.push_str("The board's desk in the last 7 days, and how the board decided:\n");
        for item in desk.iter().rev().take(15).rev() {
            let from = item
                .member
                .map(|m| format!(" from {}", name_of(Some(m))))
                .unwrap_or_default();
            let how = if item.status == "waiting" {
                "waiting for the board".to_owned()
            } else {
                format!("the board: {}", item.status.replace('_', " "))
            };
            told.push_str(&format!(
                "- {}, {}{from}: {} ({how})\n",
                day(item.at),
                item.kind.replace('_', " "),
                first_line(&item.text)
            ));
        }
        Ok(told)
    }

    /// What changed since `since` (the start of the CEO's last turn), oldest first, the
    /// newest [`CEO_NEWS_MOST`] of it: tasks made and moved, Endora's alarms, the board's
    /// word, and lessons noted. Empty when nothing did.
    fn ceo_news(
        &self,
        since: Millis,
        name_of: &dyn Fn(Option<MemberId>) -> String,
        day: &dyn Fn(Millis) -> String,
    ) -> Done<Vec<String>> {
        let tasks = self.store.tasks()?;
        let desk = self.store.desk()?;
        let title = |id: &serde_json::Value| {
            let id = id.as_i64().unwrap_or(0);
            let named = tasks.iter().find(|t| t.id.0 == id);
            format!("#{id} {}", named.map_or("", |t| t.title.as_str()))
        };
        let news: Vec<String> = self
            .store
            .events_after(0)?
            .into_iter()
            .filter(|e| e.at > since)
            .filter_map(|e| {
                let said = match e.kind.as_str() {
                    "task.created" => {
                        let id = e.data["task"].as_i64().unwrap_or(0);
                        let to = tasks
                            .iter()
                            .find(|t| t.id.0 == id)
                            .and_then(|t| t.owner.or(t.assignee));
                        format!("{}: created, for {}", title(&e.data["task"]), name_of(to))
                    }
                    "task.changed" => format!(
                        "{}: now {}",
                        title(&e.data["task"]),
                        e.data["status"].as_str().unwrap_or("?").replace('_', " ")
                    ),
                    "desk.decided" => {
                        let item = e.data["item"].as_i64().unwrap_or(0);
                        let text = desk
                            .iter()
                            .find(|d| d.id.0 == item)
                            .map_or(String::new(), |d| first_line(&d.text));
                        format!(
                            "the board, on a {}: {} ({})",
                            e.data["kind"].as_str().unwrap_or("?").replace('_', " "),
                            text,
                            e.data["decision"].as_str().unwrap_or("?").replace('_', " ")
                        )
                    }
                    "lesson.noted" => format!(
                        "a lesson for goal #{}: {}",
                        e.data["goal"],
                        e.data["lesson"].as_str().unwrap_or("")
                    ),
                    _ => alarm_in(&e)?.to_owned(),
                };
                Some(format!("- {}: {said}", day(e.at)))
            })
            .collect();
        let earlier = news.len().saturating_sub(CEO_NEWS_MOST);
        let mut kept: Vec<String> = Vec::new();
        if earlier > 0 {
            kept.push(format!("- and {earlier} earlier"));
        }
        kept.extend(news.into_iter().skip(earlier));
        Ok(kept)
    }

    // ---- the tick ----------------------------------------------------------------------------

    /// The engine's regular pass (every 30 seconds), in the spec's order: timers, routines and
    /// check-backs, ended and vanished turns, retries, starts, stuck work, finished waits. A
    /// late tick runs once.
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
        // Routines whose time came fire once, however late, and count on from now.
        for mut routine in self.store.routines()? {
            if !routine.enabled || routine.next_due_at > now {
                continue;
            }
            routine.next_due_at = routines::next_due(now, routine.every_ms);
            self.fire_held_lock(routine, &Firing::Schedule)?;
            report.routines += 1;
        }
        // Check-backs whose time came wake their member, while the task is still open.
        for back in self.store.check_backs()? {
            if back.status != CheckBackStatus::Pending || back.due_at > now {
                continue;
            }
            let about = json!({ "task": back.task.0, "member": back.member.0, "why": back.why });
            if self.task(back.task)?.status.finished() {
                self.store
                    .set_check_back(back.id, CheckBackStatus::Dropped, now)?;
                self.emit(EventKind::CheckBackDropped, about)?;
                continue;
            }
            self.store
                .set_check_back(back.id, CheckBackStatus::Fired, now)?;
            self.emit(EventKind::CheckBackFired, about)?;
            self.wake_held_lock(back.member, Some(back.task), WakeReason::CheckBack)?;
            report.check_backs += 1;
        }
        // Approvals nobody decided within a day go unrun, and the thread says so (ADR 0108).
        for item in self.store.desk()? {
            if item.kind != "approval"
                || item.status != "waiting"
                || now - item.at <= APPROVALS_LAST_MS
            {
                continue;
            }
            self.store.set_desk_status(item.id, "expired")?;
            self.emit(
                EventKind::DeskDecided,
                json!({ "item": item.id.0, "kind": item.kind, "decision": "expired" }),
            )?;
            self.settle_unrun(&item, "Nobody decided this within a day, so it was not run")?;
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

        // 5b. Open work handed to a member that nothing will bring it back to: no turn queued,
        //     running or waiting to retry, and no wake held for it, for ten minutes. One
        //     recovery wake for its member; still so ten minutes after, the owner's desk, once.
        //     Never handed to anyone else.
        let (stranded_nudged, stranded_stuck) = self.recover_stranded(&tasks, &live, now)?;
        report.nudged += stranded_nudged;
        report.stuck += stranded_stuck;

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
                effort: self.effort_of(&member, &run)?,
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
    /// While the team has a CEO at work, nobody's question but the CEO's reaches the desk
    /// (ADR 0111): it goes up the chain instead (see [`chain::above`]), posted in the task's
    /// thread for the one above, who is woken to answer there. The CEO's is a decision for the
    /// board ([`Self::ask_board`]); the reviewer's, about the CEO's decision it checked, passes
    /// that decision on to the board's desk.
    ///
    /// A question about the home lab, from anyone but the reviewer, goes to the reviewer
    /// first (see [`review::goes_to_review`]): the task moves to `in_review` with the draft in
    /// its thread, and the reviewer is woken to check it. Whatever reaches the desk lists the
    /// container-like names the home lab does not have (see [`review::checked`]).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a turn with no task; [`EngineError::Refused`] with
    /// [`Refusal::TurnOver`] for a turn that has ended, or [`Refusal::AlreadyAsked`] while its
    /// last question waits; a store failure.
    pub fn ask_owner(&self, run: RunId, text: &str) -> Done<Asked> {
        let _held = self.held();
        // Room for the question and the worker's line saying what the turn actually did.
        let text: String = text.trim().chars().take(1200).collect();
        let lab = self.lab_names();
        let turn = self.running(run)?;
        let task = self.task(
            turn.task
                .ok_or(EngineError::Unknown("task for this turn"))?,
        )?;
        let member = self.member(turn.member)?;
        // The CEO's word to the board is a decision, with its recommendation (ADR 0111).
        if member.role == Role::Ceo {
            let decision =
                Decision::read(&text).ok_or(EngineError::Refused(Refusal::NotADecision))?;
            return self.to_the_board(turn, &task, &member, &decision);
        }
        // The reviewer passing on the CEO's decision, checked: it reaches the board as the
        // CEO's, as the reviewer wrote it, or as the CEO did if the reviewer's lost its shape.
        if member.role == Role::Reviewer {
            if let Some((ceo, drafted)) = self.ceos_draft(&task)? {
                let decision = Decision::read(&text).unwrap_or(drafted);
                return self
                    .board_desk(turn, &task, &ceo, &decision, Some(&member))
                    .map(Asked::Desk);
            }
        }
        // Up the chain, while the team has a CEO at work; otherwise as before there was one.
        if let Some(above) = self.above(&member)? {
            return self.up_the_chain(turn, &task, &member, &above, &text);
        }
        if let Some(reviewer) = self.reviewer_for(&member, &task, &text, lab.as_deref())? {
            return self.to_review(turn, &task, &member, reviewer, &text);
        }
        let checked = review::checked(&text, lab.as_deref());
        self.ask_held_lock(run, "question", &|name| format!("{name} asks: {checked}"))
            .map(Asked::Desk)
    }

    /// The CEO puts a decision to the board, about its turn's task (ADR 0111): checked by the
    /// reviewer first, as every draft for the owner is, then on the board's desk with its
    /// recommendation. Its turn ends either way.
    ///
    /// # Errors
    /// [`Refusal::OnlyTheCeo`] for any other member, and what [`Self::ask_owner`] refuses.
    pub fn ask_board(&self, run: RunId, decision: &Decision) -> Done<Asked> {
        let _held = self.held();
        let (turn, ceo) = self.the_ceos_turn(run)?;
        let task = self.task(
            turn.task
                .ok_or(EngineError::Unknown("task for this turn"))?,
        )?;
        self.to_the_board(turn, &task, &ceo, decision)
    }

    fn to_the_board(
        &self,
        run: Run,
        task: &Task,
        ceo: &Member,
        decision: &Decision,
    ) -> Done<Asked> {
        let reviewer = self
            .reviewer()?
            .filter(|r| !r.paused && r.id != ceo.id && !task.status.finished());
        if let Some(reviewer) = reviewer {
            return self.to_review(run, task, ceo, reviewer.id, &decision.text());
        }
        self.board_desk(run, task, ceo, decision, None)
            .map(Asked::Desk)
    }

    /// The CEO's decision on the board's desk, from the CEO (so the board's word wakes it),
    /// saying who checked it; and the end of the turn that put it there.
    fn board_desk(
        &self,
        run: Run,
        task: &Task,
        ceo: &Member,
        decision: &Decision,
        checked_by: Option<&Member>,
    ) -> Done<crate::domain::DeskId> {
        if self.waiting_question(ceo.id, task.id)? {
            return Err(EngineError::Refused(Refusal::AlreadyAsked));
        }
        let mut text = decision.text();
        if let Some(reviewer) = checked_by {
            text.push_str(&format!("\nChecked by {}, the reviewer.", reviewer.name));
        }
        let desk = self.store.add_desk(
            "decision",
            Some(task.id),
            None,
            Some(ceo.id),
            &text,
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": "decision", "task": task.id.0 }),
        )?;
        let id = run.id;
        self.end(run, RunStatus::Succeeded, "put a decision to the board")?;
        self.runner.stop(id, self.settings.grace);
        Ok(desk)
    }

    /// The CEO's decision waiting in a task's review, if that is what the reviewer has before
    /// it: the newest of a draft for the owner and a move to review is a draft of the CEO's.
    fn ceos_draft(&self, task: &Task) -> Done<Option<(Member, Decision)>> {
        if task.status != TaskStatus::InReview {
            return Ok(None);
        }
        let newest = self
            .store
            .thread(task.id)?
            .into_iter()
            .rev()
            .find(|p| p.text.starts_with(review::DRAFT) || p.text.contains(" → in_review"));
        let Some(draft) = newest.filter(|p| p.text.starts_with(review::DRAFT)) else {
            return Ok(None);
        };
        let ceo = self
            .store
            .members()?
            .into_iter()
            .find(|m| m.role == Role::Ceo && m.name == draft.author);
        Ok(ceo.and_then(|ceo| Decision::read(&draft.text).map(|d| (ceo, d))))
    }

    /// Who `member` asks up the chain of command, if anyone: the lead or the CEO at work (not
    /// paused), by [`chain::above`]. Nobody while the team has no CEO at work.
    fn above(&self, member: &Member) -> Done<Option<Member>> {
        let members = self.store.members()?;
        let at_work = |role: Role| {
            members
                .iter()
                .filter(|m| m.role == role && !m.paused && m.id != member.id)
                .min_by_key(|m| m.id)
                .cloned()
        };
        let (lead, ceo) = (at_work(Role::Lead), at_work(Role::Ceo));
        Ok(
            match chain::above(member.role, lead.is_some(), ceo.is_some()) {
                Some(Role::Lead) => lead,
                Some(Role::Ceo) => ceo,
                _ => None,
            },
        )
    }

    /// A member's question, up the chain: posted in the task's thread for the one above, the
    /// asker's turn ended, and the one above woken to answer there. One question a member
    /// about a task waits for its answer at a time.
    fn up_the_chain(
        &self,
        run: Run,
        task: &Task,
        member: &Member,
        above: &Member,
        text: &str,
    ) -> Done<Asked> {
        let thread = self.store.thread(task.id)?;
        let posts: Vec<(&str, &str)> = thread
            .iter()
            .map(|p| (p.author.as_str(), p.text.as_str()))
            .collect();
        let waiting = chain::unanswered(&posts, &above.name)
            .iter()
            .any(|(asker, _)| *asker == member.name);
        if waiting {
            return Err(EngineError::Refused(Refusal::AlreadyAsked));
        }
        self.post_held_lock(
            Author::Member(member.id),
            task.id,
            &chain::question(&above.name, text),
        )?;
        let id = run.id;
        self.end(
            run,
            RunStatus::Succeeded,
            &format!("asked {}, the {}", above.name, above.role.name()),
        )?;
        self.wake_held_lock(above.id, Some(task.id), WakeReason::QuestionAsked)?;
        self.runner.stop(id, self.settings.grace);
        Ok(Asked::Up(above.id))
    }

    /// The reviewer a member's question goes to before the owner's desk, if it goes to one:
    /// the first member with the reviewer's role, for an open task of the home lab's or a
    /// question that speaks of containers, from anyone but that reviewer.
    fn reviewer_for(
        &self,
        member: &Member,
        task: &Task,
        text: &str,
        lab: Option<&[String]>,
    ) -> Done<Option<MemberId>> {
        if task.status.finished() {
            return Ok(None);
        }
        let reviewer = self.reviewer()?;
        let the_home_labs = self.the_home_labs(task)?;
        let speaks = review::names_containers(text, lab.unwrap_or_default());
        let goes = review::goes_to_review(
            member.role == Role::Reviewer,
            reviewer.is_some(),
            the_home_labs,
            speaks,
        );
        Ok(reviewer.filter(|_| goes).map(|r| r.id))
    }

    /// A member's question, as a draft for the reviewer: the task moves to `in_review`, the
    /// draft goes in its thread, the member's turn ends, and the reviewer is woken to check it.
    fn to_review(
        &self,
        run: Run,
        task: &Task,
        member: &Member,
        reviewer: MemberId,
        text: &str,
    ) -> Done<Asked> {
        if self.waiting_question(member.id, task.id)? {
            return Err(EngineError::Refused(Refusal::AlreadyAsked));
        }
        // The draft is the asker's work: a task nobody had taken becomes theirs, so the
        // reviewer, starting its turn on it, cannot take it and then review its own.
        if task.owner.is_none() {
            self.store.claim(task.id, member.id)?;
        }
        if task.status != TaskStatus::InReview {
            self.store.set_task_status(task.id, TaskStatus::InReview)?;
            self.emit_status(task, TaskStatus::InReview)?;
        }
        self.post_held_lock(
            Author::Member(member.id),
            task.id,
            &format!("{}{text}", review::DRAFT),
        )?;
        let id = run.id;
        self.end(
            run,
            RunStatus::Succeeded,
            "sent a draft for the owner to review",
        )?;
        self.wake_held_lock(reviewer, Some(task.id), WakeReason::ReviewRequested)?;
        self.runner.stop(id, self.settings.grace);
        Ok(Asked::Review(reviewer))
    }

    /// Whether a question or an approval from `member` about `task` still waits on the desk.
    fn waiting_question(&self, member: MemberId, task: TaskId) -> Done<bool> {
        Ok(self.store.desk()?.into_iter().any(|d| {
            matches!(d.kind.as_str(), "question" | "approval" | "decision")
                && d.status == "waiting"
                && d.member == Some(member)
                && d.task == Some(task)
        }))
    }

    /// The node asks the owner's approval for an action a member proposed through Endora's
    /// bridge (ADR 0108): an `approval` on the desk, saying exactly what would run, and the end
    /// of the member's turn, as [`Self::ask_owner`] ends it. It counts as the member's question
    /// about the task: one of either waits at a time. Nothing runs here; the node runs what it
    /// stored once the owner approves, and tells HQ what came of it
    /// ([`Self::approval_outcome`]).
    ///
    /// # Errors
    /// As [`Self::ask_owner`].
    pub fn ask_approval(&self, run: RunId, text: &str) -> Done<crate::domain::DeskId> {
        let _held = self.held();
        let text: String = text.trim().chars().take(1200).collect();
        self.ask_held_lock(run, "approval", &|_| text.clone())
    }

    /// A question or an approval on the desk from `run`'s member about its task, worded by
    /// `say` from the member's name, and the end of the turn.
    fn ask_held_lock(
        &self,
        run: RunId,
        kind: &str,
        say: &dyn Fn(&str) -> String,
    ) -> Done<crate::domain::DeskId> {
        let run = self.running(run)?;
        let task = run.task.ok_or(EngineError::Unknown("task for this turn"))?;
        let member = self.member(run.member)?;
        if self.waiting_question(member.id, task)? {
            return Err(EngineError::Refused(Refusal::AlreadyAsked));
        }
        let desk = self.store.add_desk(
            kind,
            Some(task),
            None,
            Some(member.id),
            &say(&member.name),
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": kind, "task": task.0 }),
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

    /// The owner decides an item on their desk, with words for the thread if they wrote any:
    /// a question answered and seen, or the board's word on the CEO's decision (approve,
    /// decline, or `answer` with words alone). Everything else as [`Self::decide`].
    ///
    /// # Errors
    /// As [`Self::decide`] and [`Self::answer`].
    pub fn decide_saying(
        &self,
        item: crate::domain::DeskId,
        decision: &str,
        words: &str,
    ) -> Done<()> {
        let _held = self.held();
        let found = self.waiting_on_desk(item)?;
        let words: String = words.trim().chars().take(4000).collect();
        if found.kind == "decision" {
            return self.board_decides(&found, decision, &words);
        }
        if decision == "seen" && !words.is_empty() && found.kind == "question" {
            let task = found.task.ok_or(EngineError::Unknown("task"))?;
            self.post_held_lock(Author::Owner, task, &words)?;
        }
        self.decide_held_lock(&found, decision)
    }

    /// The board's word on the CEO's decision (ADR 0111): in the task's thread as the owner's,
    /// with what they wrote, and the CEO woken to act on it.
    fn board_decides(&self, found: &DeskItem, verdict: &str, words: &str) -> Done<()> {
        let said = match verdict {
            "approve" => "The board approves",
            "decline" => "The board declines",
            "answer" if !words.is_empty() => "The board answers",
            _ => return Err(EngineError::Unknown("decision for this item")),
        };
        let task = found.task.ok_or(EngineError::Unknown("task"))?;
        let decision = Decision::read(&found.text).map_or_else(String::new, |d| d.decision);
        let mut text = format!("{said}: {decision}");
        if !words.is_empty() {
            text.push_str(&format!("\n{words}"));
        }
        self.post_held_lock(Author::Owner, task, &text)?;
        self.store.set_desk_status(found.id, verdict)?;
        self.emit(
            EventKind::DeskDecided,
            json!({ "item": found.id.0, "kind": found.kind, "decision": verdict }),
        )?;
        if let Some(ceo) = found.member {
            self.wake_held_lock(ceo, Some(task), WakeReason::ProposalDecided)?;
        }
        Ok(())
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
            // Approved: the node runs exactly what it stored, then tells what came of it, and
            // only that wakes the member (ADR 0108).
            ("approval", "approve") => {}
            ("approval", "decline") => {
                self.settle_unrun(found, "The owner declined this, and it was not run")?;
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

    /// The tick's step 5b: open work with a member and nothing that will bring it back, for
    /// [`STRANDED_AFTER_MS`]. `doing` work is step 5's. Work left `in_review` with no review
    /// coming wakes the reviewer instead of its owner (whose turns wait while it is reviewed).
    /// Returns how many were nudged and how many went on the desk.
    fn recover_stranded(
        &self,
        tasks: &[Task],
        live: &[Option<TaskId>],
        now: Millis,
    ) -> Done<(usize, usize)> {
        let (mut nudged, mut stuck) = (0, 0);
        let members = self.store.members()?;
        let runs = self.store.runs()?;
        let wakes = self.store.wakes()?;
        let desk = self.store.desk()?;
        for task in tasks {
            if !matches!(
                task.status,
                TaskStatus::Todo | TaskStatus::Blocked | TaskStatus::InReview
            ) || live.contains(&Some(task.id))
                || !self.waits_over(&task.waits_on)?
                || !self.store.held_wakes(task.id)?.is_empty()
            {
                continue;
            }
            let Some(member) = task.owner.or(task.assignee) else {
                continue;
            };
            if members.iter().any(|m| m.id == member && m.paused) {
                continue;
            }
            let on_the_desk = desk
                .iter()
                .any(|d| d.kind == "stuck" && d.status == "waiting" && d.task == Some(task.id));
            if on_the_desk {
                continue;
            }
            let last = runs
                .iter()
                .filter(|r| r.task == Some(task.id))
                .flat_map(|r| [Some(r.queued_at), r.started_at, r.ended_at])
                .flatten()
                .chain(
                    wakes
                        .iter()
                        .filter(|w| w.task == Some(task.id))
                        .map(|w| w.at),
                )
                .max()
                .unwrap_or(Millis::MIN);
            if now.saturating_sub(last) <= STRANDED_AFTER_MS {
                continue;
            }
            if !task.nudged {
                self.store.set_nudged(task.id, true)?;
                nudged += 1;
                if task.status == TaskStatus::InReview {
                    self.review_asked(task)?;
                    continue;
                }
                self.wake_held_lock(member, Some(task.id), WakeReason::AssignmentRecovery)?;
                continue;
            }
            self.store.set_task_status(task.id, TaskStatus::Blocked)?;
            let item = self.store.add_desk(
                "stuck",
                Some(task.id),
                None,
                None,
                &format!(
                    "Stuck: {}. It was handed on, woken once, and nobody came back to it.",
                    task.title
                ),
                now,
            )?;
            self.emit(
                EventKind::TaskChanged,
                json!({ "task": task.id.0, "goal": task.goal.0, "owner": task.owner.map(|o| o.0),
                        "status": TaskStatus::Blocked.name(), "priority": task.priority.name() }),
            )?;
            self.emit(
                EventKind::DeskAdded,
                json!({ "item": item.0, "kind": "stuck", "task": task.id.0 }),
            )?;
            stuck += 1;
        }
        Ok((nudged, stuck))
    }

    /// An approval that will not run: why, in the task's thread as Endora's, and the member
    /// who asked woken to read it.
    fn settle_unrun(&self, found: &DeskItem, why: &str) -> Done<()> {
        let Some(task) = found.task else {
            return Ok(());
        };
        self.post_held_lock(Author::Endora, task, &format!("{why}: {}", found.text))?;
        if let Some(member) = found.member {
            self.wake_held_lock(member, Some(task), WakeReason::ProposalDecided)?;
        }
        Ok(())
    }

    /// What came of an approved action, as the node that ran it says (ADR 0108): written in
    /// the task's thread as Endora's, and the member who asked woken to read it. Told once.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for an item that is not an approval, or one not approved;
    /// [`EngineError::Refused`] with [`Refusal::Finished`] once told.
    pub fn approval_outcome(&self, item: crate::domain::DeskId, text: &str) -> Done<()> {
        let _held = self.held();
        let found = self
            .store
            .desk()?
            .into_iter()
            .find(|d| d.id == item && d.kind == "approval")
            .ok_or(EngineError::Unknown("approval"))?;
        match found.status.as_str() {
            "approve" => {}
            "ran" => return Err(EngineError::Refused(Refusal::Finished)),
            _ => return Err(EngineError::Unknown("approved approval")),
        }
        self.store.set_desk_status(item, "ran")?;
        let text: String = text.trim().chars().take(4000).collect();
        if let Some(task) = found.task {
            self.post_held_lock(Author::Endora, task, &text)?;
            if let Some(member) = found.member {
                self.wake_held_lock(member, Some(task), WakeReason::ProposalDecided)?;
            }
        }
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
                "approval" => "decline",
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

    // ---- routines, alarms and check-backs (ADR 0107) -------------------------------------

    /// The routine whose last task this is, if any: the one task a routine has open.
    fn routine_of(&self, task: TaskId) -> Done<Option<Routine>> {
        Ok(self
            .store
            .routines()?
            .into_iter()
            .find(|r| r.last_task == Some(task)))
    }

    fn routine(&self, id: RoutineId) -> Done<Routine> {
        self.store
            .routines()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or(EngineError::Unknown("routine"))
    }

    /// Makes a routine: work that comes round again, handed to `assignee` as a task under
    /// `goal`. It first fires one period from now; the owner can run it at once.
    ///
    /// # Errors
    /// [`Refusal::Rhythm`] for a rhythm faster than every 15 minutes or slower than every 30
    /// days; [`EngineError::Unknown`] for a goal or member that does not exist, or no title.
    pub fn add_routine(&self, by: Author, routine: NewRoutine) -> Done<RoutineId> {
        let _held = self.held();
        routines::may_keep(routine.every_ms).map_err(EngineError::Refused)?;
        let title = one_line(&routine.title, 120);
        if title.is_empty() {
            return Err(EngineError::Unknown("routine title"));
        }
        self.store
            .goal(routine.goal)?
            .ok_or(EngineError::Unknown("goal"))?;
        self.member(routine.assignee)?;
        let routine = NewRoutine { title, ..routine };
        let next = routines::next_due(self.clock.now(), routine.every_ms);
        let id = self.store.add_routine(&routine, next)?;
        self.emit(
            EventKind::RoutineChanged,
            json!({ "routine": id.0, "title": routine.title, "enabled": true,
                    "rhythm": routines::rhythm(routine.every_ms),
                    "by": self.author_name(by)? }),
        )?;
        Ok(id)
    }

    /// Switches a routine on or off. Switched on, it counts from now: what it missed while
    /// off is skipped.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a routine that does not exist.
    pub fn switch_routine(&self, id: RoutineId, on: bool) -> Done<()> {
        let _held = self.held();
        let mut routine = self.routine(id)?;
        if on && !routine.enabled {
            routine.next_due_at = routines::next_due(self.clock.now(), routine.every_ms);
        }
        routine.enabled = on;
        self.store.save_routine(&routine)?;
        self.emit(
            EventKind::RoutineChanged,
            json!({ "routine": id.0, "title": routine.title, "enabled": on }),
        )
    }

    /// The owner runs a routine now, whatever its rhythm. Its next time is unchanged.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a routine that does not exist; a store failure.
    pub fn fire_routine(&self, id: RoutineId) -> Done<Fired> {
        let _held = self.held();
        let routine = self.routine(id)?;
        self.fire_held_lock(routine, &Firing::Now)
    }

    /// An alarm from Endora about the home lab, in its words: fired into the first routine
    /// that is on and takes the home lab's alarms. `None` when no routine takes them.
    ///
    /// # Errors
    /// A store failure.
    pub fn alarm(&self, said: &str) -> Done<Option<(RoutineId, Fired)>> {
        let _held = self.held();
        let said = one_line(said, 300);
        if said.is_empty() {
            return Err(EngineError::Unknown("alarm text"));
        }
        let Some(routine) = self
            .store
            .routines()?
            .into_iter()
            .find(|r| r.enabled && r.on_lab_trouble)
        else {
            return Ok(None);
        };
        let id = routine.id;
        let fired = self.fire_held_lock(routine, &Firing::Alarm(said))?;
        Ok(Some((id, fired)))
    }

    /// Fires a routine, through the engine's rules: never past the kill switch, to a paused
    /// member, beyond the budget or for a goal that is not active. A routine has one open task:
    /// on the clock it is skipped while that is open, and an alarm is passed into it instead.
    /// Every firing, skip or post is in the feed. The routine is saved as it is left.
    fn fire_held_lock(&self, mut routine: Routine, firing: &Firing) -> Done<Fired> {
        let now = self.clock.now();
        let why = firing.why(routine.every_ms);
        let skipped = self.why_not_fire(&routine)?;
        let open = match routine.last_task {
            Some(task) => Some(self.task(task)?).filter(|t| !t.status.finished()),
            None => None,
        };
        let step = routines::step(firing, open.is_some());
        let skipped = skipped.or_else(|| match (&open, step) {
            (Some(task), FireStep::Skip) => {
                Some(format!("its last task, #{}, is still open", task.id))
            }
            _ => None,
        });
        if let Some(reason) = skipped {
            self.store.save_routine(&routine)?;
            self.emit(
                EventKind::RoutineSkipped,
                json!({ "routine": routine.id.0, "title": routine.title, "firing": firing.name(),
                        "why": reason }),
            )?;
            return Ok(Fired::Skipped(reason));
        }
        if let (Some(task), Firing::Alarm(said)) = (open, firing) {
            let text = format!("Alarm: {said}");
            self.post_held_lock(Author::Endora, task.id, &text)?;
            let to = task.owner.or(task.assignee).unwrap_or(routine.assignee);
            self.wake_held_lock(to, Some(task.id), WakeReason::Alarm)?;
            routine.last_fired_at = Some(now);
            self.store.save_routine(&routine)?;
            self.emit(
                EventKind::RoutinePosted,
                json!({ "routine": routine.id.0, "title": routine.title, "task": task.id.0,
                        "said": said }),
            )?;
            return Ok(Fired::Posted(task.id));
        }
        let priority = match firing {
            Firing::Alarm(_) => Priority::High,
            _ => Priority::Normal,
        };
        let description = if routine.description.trim().is_empty() {
            why.clone()
        } else {
            format!("{}\n\nWhy now: {why}", routine.description.trim())
        };
        let task = self.add_task_held_lock(
            Author::Endora,
            NewTask {
                goal: routine.goal,
                parent: None,
                title: routine.title.clone(),
                description,
                assignee: Some(routine.assignee),
                priority,
                waits_on: Vec::new(),
            },
        )?;
        routine.last_task = Some(task);
        routine.last_fired_at = Some(now);
        self.store.save_routine(&routine)?;
        self.emit(
            EventKind::RoutineFired,
            json!({ "routine": routine.id.0, "title": routine.title, "task": task.0,
                    "firing": firing.name(), "why": why }),
        )?;
        Ok(Fired::Created(task))
    }

    /// Why a routine may not fire now, if it may not: the engine's rules, said in words.
    fn why_not_fire(&self, routine: &Routine) -> Done<Option<String>> {
        if self.store.stopped()? {
            return Ok(Some("the kill switch is on".to_owned()));
        }
        let active = self
            .store
            .goal(routine.goal)?
            .is_some_and(|g| g.status == GoalStatus::Active);
        if !active {
            return Ok(Some("its goal is not active".to_owned()));
        }
        let member = self.member(routine.assignee)?;
        if member.paused {
            return Ok(Some(format!("{} is paused", member.name)));
        }
        if self.affordable(&member)?.is_none() {
            return Ok(Some(format!("{}'s budget for today is spent", member.name)));
        }
        Ok(None)
    }

    /// A turn asks to come back to its task after `minutes` (held to 5–720), saying why: one
    /// wake for its member, then, through the normal wake path, if the task is still open. A
    /// newer one from the same member replaces its older one, and a task holds at most three.
    /// Returns when it is due.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`] for a turn that has ended; [`EngineError::Unknown`] for a turn
    /// with no task or no reason; [`Refusal::Finished`] for a task that is over.
    pub fn check_back(&self, run: RunId, minutes: i64, why: &str) -> Done<Millis> {
        let _held = self.held();
        let run = self.running(run)?;
        let task = run.task.ok_or(EngineError::Unknown("task for this turn"))?;
        if self.task(task)?.status.finished() {
            return Err(EngineError::Refused(Refusal::Finished));
        }
        let why = one_line(why, 300);
        if why.is_empty() {
            return Err(EngineError::Unknown("reason to check back"));
        }
        let now = self.clock.now();
        let pending: Vec<_> = self
            .store
            .check_backs()?
            .into_iter()
            .filter(|c| c.task == task && c.status == CheckBackStatus::Pending)
            .collect();
        for replaced in routines::replaced_by_new(&pending, run.member) {
            self.store
                .set_check_back(replaced, CheckBackStatus::Replaced, now)?;
        }
        let minutes = routines::check_back_minutes(minutes);
        let due = now + minutes * MINUTE_MS;
        self.store
            .add_check_back(run.member, task, &why, due, now)?;
        self.emit(
            EventKind::CheckBackSet,
            json!({ "task": task.0, "member": run.member.0, "minutes": minutes, "due_at": due,
                    "why": why }),
        )?;
        Ok(due)
    }

    /// A turn of the lead's or the reviewer's notes a lesson the team will need again: one
    /// line at the end of its task's goal's notes for the team, which every briefing under the
    /// goal carries, as `- <lesson> (<member>, <day> <month>)`. Each is an event in the feed
    /// and a notice on the owner's desk, so the owner sees, and can edit, what the team
    /// learned. Returns the line.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`] for a turn that has ended; [`Refusal::NotALessonKeeper`] for a
    /// member that is neither; [`EngineError::Unknown`] for a turn with no task, or no lesson;
    /// [`Refusal::TooLong`] for a lesson past [`lessons::LESSON_MOST`]; [`Refusal::NotesFull`]
    /// when the notes have no room for it.
    pub fn note_lesson(&self, run: RunId, lesson: &str) -> Done<String> {
        let _held = self.held();
        let run = self.running(run)?;
        let member = self.member(run.member)?;
        if !lessons::keeps_lessons(member.role) {
            return Err(EngineError::Refused(Refusal::NotALessonKeeper));
        }
        let task = run.task.ok_or(EngineError::Unknown("task for this turn"))?;
        let goal = self.task(task)?.goal;
        if lesson.trim().is_empty() {
            return Err(EngineError::Unknown("lesson"));
        }
        let now = self.clock.now();
        let line = lessons::line(lesson, &member.name, now, self.settings.offset_minutes)
            .map_err(EngineError::Refused)?;
        let notes = lessons::appended(&self.store.goal_note(goal)?, &line, GOAL_NOTE_MOST)
            .map_err(EngineError::Refused)?;
        self.store.set_goal_note(goal, &notes)?;
        let said = line
            .trim_start_matches("- ")
            .rsplit_once(" (")
            .map_or(line.as_str(), |(l, _)| l);
        self.emit(
            EventKind::LessonNoted,
            json!({ "goal": goal.0, "task": task.0, "member": member.id.0, "lesson": said }),
        )?;
        let text = format!("{} noted: {said}", member.name);
        let desk = self
            .store
            .add_desk("notice", Some(task), None, Some(member.id), &text, now)?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": "notice", "task": task.0 }),
        )?;
        Ok(line)
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
        let mut notes = std::collections::HashMap::new();
        for g in &goals {
            notes.insert(g.id.0, self.store.goal_note(g.id)?);
        }
        let mut members = Vec::new();
        let paid = self.paid_weights()?;
        for m in self.store.members()? {
            let committed = self.store.committed(Some(m.id), day)?;
            let spent = self.store.spent(Some(m.id), day)?;
            members.push(json!({
                "id": m.id.0, "name": m.name, "role": m.role.name(), "look": m.look,
                "paused": m.paused, "slots": m.slots,
                // What it thinks with, and what the owner may choose for its role (ADR 0109).
                "brain": m.brain.name(),
                "brains": Brain::choices(m.role).into_iter()
                    .map(|b| json!({ "name": b.name(), "label": b.label() }))
                    .collect::<Vec<_>>(),
                // A member on the house's model costs nothing; the paid share the pool.
                "per_day": self.per_day(&m, paid),
                "committed": committed, "spent": spent, "held": (committed - spent).max(0),
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
        let routine_list = self.store.routines()?;
        let mut task_rows = Vec::new();
        for t in &tasks {
            task_rows.push(json!({
                "id": t.id.0, "goal": t.goal.0, "parent": t.parent.map(|p| p.0), "title": t.title,
                "description": t.description, "owner": t.owner.map(|o| o.0),
                "assignee": t.assignee.map(|a| a.0), "status": t.status.name(),
                "priority": t.priority.name(), "depth": t.depth,
                "waits_on": t.waits_on.iter().map(|w| w.0).collect::<Vec<_>>(),
                "gate": self.store.gate(t.id)?, "why": why(t),
                "routine": routine_list.iter().find(|r| r.last_task == Some(t.id)).map(|r| r.id.0),
                "thread": self.store.thread(t.id)?.iter()
                    .map(|p| json!({ "author": p.author, "text": p.text, "at": p.at }))
                    .collect::<Vec<_>>(),
            }));
        }
        // Spent is what turns reported using; held is what they have reserved and not yet
        // spent. Committed, the two together, is what the budget counts.
        let pool_committed = self.store.committed(None, day)?;
        let pool_spent = self.store.spent(None, day)?;
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
            "pool": { "per_day": money::pool_a_day(pool), "committed": pool_committed,
                      "spent": pool_spent, "held": (pool_committed - pool_spent).max(0) },
            "members": members,
            "goals": goals.iter().map(|g| json!({
                "id": g.id.0, "title": g.title, "reason": g.reason, "status": g.status.name(),
                "parent": g.parent.map(|p| p.0), "note": notes.get(&g.id.0).cloned().unwrap_or_default(),
            })).collect::<Vec<_>>(),
            "tasks": task_rows,
            "runs": runs.iter().map(|r| json!({
                "id": r.id.0, "member": r.member.0, "task": r.task.map(|t| t.0),
                "status": r.status.name(), "started_at": r.started_at, "reserved": r.reserved,
                "spent": r.spent,
            })).collect::<Vec<_>>(),
            "routines": routine_list.iter().map(|r| json!({
                "id": r.id.0, "title": r.title, "description": r.description, "goal": r.goal.0,
                "assignee": r.assignee.0, "every_ms": r.every_ms,
                "rhythm": routines::rhythm(r.every_ms), "next_due_at": r.next_due_at,
                "enabled": r.enabled, "on_lab_trouble": r.on_lab_trouble,
                "last_fired_at": r.last_fired_at, "last_task": r.last_task.map(|t| t.0),
                "last_task_open": r.last_task
                    .and_then(|id| tasks.iter().find(|t| t.id == id))
                    .is_some_and(|t| !t.status.finished()),
            })).collect::<Vec<_>>(),
            "desk": self.store.desk()?.iter().filter(|d| d.status == "waiting").map(|d| {
                let mut item = json!({
                    "id": d.id.0, "kind": d.kind, "task": d.task.map(|t| t.0),
                    "goal": d.goal.map(|g| g.0), "text": d.text, "at": d.at,
                });
                // The CEO's decision for the board, in its three parts (ADR 0111).
                if let Some(decided) = (d.kind == "decision").then(|| Decision::read(&d.text)).flatten() {
                    item["decision"] = json!({ "decision": decided.decision,
                        "recommendation": decided.recommendation, "why": decided.why });
                }
                item
            }).collect::<Vec<_>>(),
            "house_log": self.store.setting(HOUSE_LOG)?.unwrap_or_default(),
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
