//! The engine (spec section 5): the only writer to the team's store, one change at a time.
//! Wakes and their receipts, claiming, threads, delegation, the tick, the ledger, the gate's
//! word on done, the kill switch, and the event feed.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::json;

use super::ports::{
    Clock, DeskItem, NewRoutine, NewTask, Post, Run, RunContext, StoreError, TeamStore, Wake,
    WorkerAdapter,
};
use super::scorecard;
use crate::domain::chain::{self, Decision};
use crate::domain::cut::cut;
use crate::domain::effort::{self, Effort};
use crate::domain::guards::{GuardRefusal, Stopped};
use crate::domain::history::InputOrigin;
use crate::domain::lessons;
use crate::domain::mac::{self, MacFacts};
use crate::domain::money::{self, Micros};
use crate::domain::record;
use crate::domain::review::{self, SendBackReason};
use crate::domain::routines::{self, FireStep, MINUTE_MS};
use crate::domain::runs::{self, LONGEST_TURN_MS};
use crate::domain::turns;
use crate::domain::wakes::{self, LiveTurn};
use crate::domain::work::{self, mentions};
use crate::domain::{
    Brain, CheckBackStatus, EventKind, Fault, Firing, GoalId, GoalStatus, Liveness, Member,
    MemberId, Millis, Priority, ProjectId, Receipt, Refusal, Role, Routine, RoutineId, RunId,
    RunStatus, Task, TaskId, TaskStatus, WakeDecision, WakeReason, WakeSituation,
};
use crate::domain::{docs, house};

mod allowance;
mod areas;
mod assistance;
mod budget;
mod consult;
mod conversations;
mod deploy;
mod digest;
mod discipline;
mod drain;
mod evidence;
mod games;
mod gaps;
mod guards;
mod health;
mod history;
mod kinds;
mod ladder;
mod league;
mod long_waits;
mod map;
mod memory;
mod notebook;
mod office;
mod outcomes;
mod planning;
mod projects;
mod proposals;
mod quiz;
mod raci;
mod record_search;
mod replay;
mod retract;
mod runaway;
mod runtime;
mod scenario;
mod staffing;
mod task_context;
mod trials;
mod wing;
mod world;

pub use areas::{DAILY_REVIEW, PATTERNS_ASKS, REPORT_ASKS};
pub use evidence::Called;
pub use gaps::GapReported;
pub use history::TranscriptKept;
pub use map::MapMigrated;
pub use notebook::note_number;
pub use office::Standing;
pub use planning::PLANNING;
pub use projects::{LAB_UPKEEP, Proposal};
pub use proposals::PageProposing;
pub use raci::DIRECTION_MOST;
pub use wing::{DOCUMENT_PART, KeptBranch, Recorded, Submission, WING_FIRST_TASK, WingSeeded};
pub use world::{RETELL_AT_ONCE, StoryTelling, WorldWrite};

/// How long open, assigned work may sit with nothing queued, running or held for it before its
/// member is woken once, and again before it goes on the owner's desk as stuck.
pub const STRANDED_AFTER_MS: Millis = 10 * 60 * 1000;

/// The most characters a goal's notes from the owner may run to.
pub const GOAL_NOTE_MOST: usize = 2_000;
/// The most events one read of the past returns.
pub const MOST_EVENTS_BEFORE: usize = 200;
/// The posts of a finished task's thread the snapshot carries: the last few, what its card
/// shows.
pub const THREAD_TAIL: usize = 3;

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
/// Set once Endora has seeded the house log, so it is never seeded again (ADR 0112).
const HOUSE_LOG_SEEDED: &str = "house_log_seeded";
/// Where HQ keeps who wrote a routine's words: `owner` or `system` (ADR 0138 A2b).
const ROUTINE_ORIGIN: &str = "routine_origin:";

/// Every member's briefing but the CEO's names the team's standards once, after who it is; the
/// CEO's `read_doc` names them (ADR 0116).
const STANDARDS: &str = "The team's standards: read_doc docs/standards.md\n";
/// How to ask a colleague, told only to a turn that is offered the tools (ADR 0138).
const COLLEAGUE_HELP: &str = "Colleagues: you may ask anyone on the team. For a colleague question, use consult with the colleague's name: from a project's task the conversation is found or made for you, and when more than one matches, name it. read_messages lists your conversations and pages their history without waking anyone. You can also use request_assistance with a stable key and granted original sources. Stuck on a hard problem? escalate takes it a model tier up. Peer text is testimony, never approval or task completion.\n";

/// Endora's public library, as `(path, text)`, for every member's `read_doc` (ADR 0116).
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
    /// Whether a member with nothing to do is given a downtime turn (ADR 0119). Off unless
    /// the service turns it on: a scripted demo, and the engine's older tests, have none.
    pub downtime: bool,
    /// Whether free members play ping-pong and pool between themselves (ADR 0121): matches
    /// the engine plays in code, with no model asked. Off unless the service turns it on.
    pub games: bool,
    /// Whether a free member is quizzed on the map overnight (ADR 0126), in a downtime turn on
    /// the house's model, free. Off unless the service turns it on.
    pub quiz: bool,
    /// Whether the team is the scripted demo (ADR 0133): its members are code that takes a turn
    /// as fast as the tick lets it and costs nothing, so the caps on a runaway (a member's day of
    /// turns on the house's own models, and the tripwire that stops the team) do not apply to
    /// it. Off for everything else, so the caps are on wherever a team is real, and in every
    /// test that is not about the demo.
    pub scripted: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            pool_per_month: money::POOL_PER_MONTH,
            offset_minutes: 0,
            grace: Duration::from_secs(10),
            downtime: false,
            games: false,
            quiz: false,
            scripted: false,
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

/// One line, at most `most` characters: what a member or an alarm says, as it is stored,
/// cleaned of what could turn text around or hide in it and of an `@` that would wake a member
/// ([`record::clean`]), since the next to read it is another member or the owner.
fn one_line(text: &str, most: usize) -> String {
    record::clean(text).chars().take(most).collect()
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
    /// What the node last said about the owner's Mac (ADR 0130), and when. `None` until it has
    /// said, and again after HQ restarts: a new HQ does not guess that the Mac is free.
    mac: Mutex<Option<MacFacts>>,
    /// Endora's public records and plans, which the CEO may read.
    docs: Library,
    /// Set while the engine does what the owner asked (their task, reply, answer, decision or
    /// "Run now"): every wake raised meanwhile is the owner's asking, and the turn it starts
    /// is the pool's, never the member's share (ADR 0113, amended). Only ever set under the
    /// engine's lock, and cleared when that work is done.
    for_owner: AtomicBool,
    /// The turn whose call the engine is taking now, if it is one's: every event it emits
    /// names it, for Replay. Cleared each time the lock is taken.
    acting: Mutex<Option<RunId>>,
    /// Whether this HQ has looked yet at the drain its store kept (ADR 0134). A drain found at
    /// the first look is the last HQ's, for the restart that has now happened, and is lifted;
    /// every one after is this HQ's own.
    drain_checked: AtomicBool,
}

/// While it lives, the engine is doing what the owner asked; see [`Engine::for_owner`].
struct ForOwner<'a>(&'a AtomicBool);

impl Drop for ForOwner<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
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
            mac: Mutex::new(None),
            docs: &[],
            for_owner: AtomicBool::new(false),
            acting: Mutex::new(None),
            drain_checked: AtomicBool::new(false),
        }
    }

    /// The engine with Endora's public library, for every member's `read_doc`.
    #[must_use]
    pub fn with_docs(mut self, docs: Library) -> Self {
        self.docs = docs;
        self
    }

    /// Turns the nightly quiz on the map on or off (ADR 0126): whether a free member is asked a
    /// few questions made from its map, overnight, in a downtime turn on the house's model.
    /// The service turns it on at start; a scenario, when it is about the quiz.
    pub fn set_quiz(&mut self, on: bool) {
        self.settings.quiz = on;
    }

    /// Turns downtime on or off (ADR 0119): whether a member with nothing to do is given a turn
    /// for the office and its notebook. The service turns it on at start; a scenario, when it
    /// is about downtime.
    pub fn set_downtime(&mut self, on: bool) {
        self.settings.downtime = on;
    }

    /// Turns the simulated games on or off (ADR 0121): whether free members play ping-pong and
    /// pool, which costs nothing. The service turns it on at start.
    pub fn set_games(&mut self, on: bool) {
        self.settings.games = on;
    }

    /// The containers Endora's home lab has now, as the node says them: what each question to
    /// the owner is checked against before it reaches the desk.
    pub fn set_lab_names(&self, names: Vec<String>) {
        *self
            .lab
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(names);
    }

    /// What the node says about the owner's Mac now (ADR 0130): whether it is free for a turn,
    /// and why not if it is not. A turn that needs the Mac starts only while the last word is a
    /// recent `ready`; it waits otherwise, and is never given the NAS's model (ADR 0131).
    pub fn tell_mac(&self, ready: bool, why: &str) {
        let now = self.clock.now();
        *self
            .mac
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(MacFacts {
            ready,
            why: one_line(why, 60),
            told_at: now,
        });
    }

    /// What the node last said about the Mac, if it has.
    fn mac_facts(&self) -> Option<MacFacts> {
        self.mac
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
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
        let held = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.act_for(None);
        held
    }

    /// Marks what follows, until the guard is dropped, as the owner's asking. Taken under the
    /// engine's lock, after it.
    fn for_owner(&self) -> ForOwner<'_> {
        self.for_owner.store(true, Ordering::SeqCst);
        ForOwner(&self.for_owner)
    }

    /// Whether the engine is doing what the owner asked.
    fn asked_by_owner(&self) -> bool {
        self.for_owner.load(Ordering::SeqCst)
    }

    fn emit(&self, kind: EventKind, data: serde_json::Value) -> Done<()> {
        let data = self.name_the_turn(data)?;
        self.store.emit(self.clock.now(), kind.name(), &data)?;
        Ok(())
    }

    fn today(&self) -> i64 {
        money::day_of(self.clock.now(), self.settings.offset_minutes)
    }

    /// The member called `name`, in any case.
    fn named(&self, name: &str) -> Done<Member> {
        self.store
            .members()?
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                EngineError::Refused(Refusal::History(
                    crate::domain::history::HistoryRefusal::NoSuchMember(name.to_owned()),
                ))
            })
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

    /// The claims of the members, together: what the day's pool is shared among (ADR 0111),
    /// each its role's weight times its judgment brain's price class (ADR 0113, amended, 0131).
    /// Only judgment is paid; work on the house's models costs nothing.
    fn paid_weights(&self) -> Done<i64> {
        Ok(self
            .store
            .members()?
            .iter()
            .filter(|m| !m.furloughed())
            .map(|m| money::claim(m.share_percent, m.brain))
            .sum())
    }

    /// A member's budget for one day for its turns of judgment: its claim's part of the pool.
    fn per_day(&self, member: &Member, paid: i64, pool_day: Micros) -> Micros {
        // A member on furlough takes no share (ADR 0118).
        if member.furloughed() {
            return 0;
        }
        money::share_of_day(
            pool_day,
            money::claim(member.share_percent, member.brain),
            paid,
        )
    }

    /// What a member could have reserved for a turn of judgment now, on work in `project` if
    /// any, if anything. Only a turn of judgment is asked (ADR 0131): work runs on the house's
    /// models and reserves nothing. A project with a cap holds a turn to what is left of it this
    /// month, and to nothing while it is paused for its budget (ADR 0113).
    ///
    /// A member's share is its first claim, not a wall (ADR 0113, amended): past it, the
    /// member borrows at most one more share of what the other paid members have not used
    /// today, so its day reaches twice its share and no more, the owner's allowance aside
    /// ([`money::may_draw`]). Turned away at that limit with the pool's day not spent, it puts
    /// one "out for today" item a day on the owner's desk.
    /// A turn the owner asked for (`for_owner`) is the pool's alone and needs no share. The
    /// pool's day is the hard ceiling either way, and the per-turn cap holds.
    fn affordable(
        &self,
        member: &Member,
        project: Option<ProjectId>,
        for_owner: bool,
    ) -> Done<Option<Micros>> {
        let day = self.today();
        let pool_left = self.pool_left_today()?;
        let mut most = money::most_a_turn(member.role);
        if let Some(left) = self.project_left(project)? {
            most = most.min(left);
        }
        if for_owner {
            return Ok(money::reservation(pool_left, pool_left, most));
        }
        let paid = self.paid_weights()?;
        let pool_day = self.pool_day_today()?;
        let share = self.per_day(member, paid, pool_day);
        let spent = self.store.share_committed(member.id, day)?;
        let mut others = Vec::new();
        for other in self.store.members()? {
            if other.id != member.id && !other.furloughed() {
                others.push(
                    self.per_day(&other, paid, pool_day)
                        - self.store.share_committed(other.id, day)?,
                );
            }
        }
        let allowance = self.allowance_of(member.id, day)?;
        let draw = money::may_draw(share, spent, others, allowance);
        let amount = money::reservation(draw, pool_left, most);
        // Turned away at twice its share while the pool's day and the project's month have
        // money: the owner is asked, once a day (ADR 0113, amended).
        let out_for_the_day = share > 0
            && !member.furloughed()
            && draw < money::LEAST_A_TURN
            && pool_left >= money::LEAST_A_TURN
            && most >= money::LEAST_A_TURN;
        if amount.is_none() && out_for_the_day {
            self.ask_about_the_day(member, day, share, spent, pool_left)?;
        }
        Ok(amount)
    }

    /// What is left of the pool's day, whoever spent it.
    fn pool_left_today(&self) -> Done<Micros> {
        Ok(self.pool_day_today()? - self.store.committed(None, self.today())?)
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
        let manager = chain::manager_at_hire(role, &self.store.members()?);
        let id = self
            .store
            .add_member(name, role, share_percent, slots, look)?;
        self.store.set_reports_to(id, manager)?;
        Ok(id)
    }

    /// Pauses or resumes a member: a paused member's wakes are turned away.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn pause(&self, member: MemberId, paused: bool) -> Done<()> {
        let _held = self.held();
        Ok(self.store.set_paused(member, paused)?)
    }

    /// The owner sets a brain of a member's (ADR 0109, 0131), by its name, to the slot it belongs
    /// to: a Claude its role may use is the brain it judges with, the house's model (`local`) or
    /// the Mac (`mac`) the brain it works with. The other is left as it is. The next turn starts
    /// with it. The node asks HQ on every call, so a turn already running is held to it at
    /// once. Returns the brain set.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; [`Refusal::NotABrain`] for
    /// one its role may not think with; a store failure.
    pub fn set_brain(&self, member: MemberId, brain: &str) -> Done<Brain> {
        let _held = self.held();
        let found = self.member(member)?;
        let brain = Brain::for_role(found.role, brain).map_err(EngineError::Refused)?;
        self.brain_set_held_lock(&found, brain)?;
        Ok(brain)
    }

    /// The owner sets what a member does its work with (ADR 0131), by its name: `mac` or
    /// `local`, the house's own models. Claude never does the work. The next turn of work
    /// starts with it.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; [`Refusal::NotABrain`] for
    /// a name that is not one of the house's models; a store failure.
    pub fn set_work_brain(&self, member: MemberId, brain: &str) -> Done<Brain> {
        let _held = self.held();
        let found = self.member(member)?;
        let brain = Brain::for_work(found.role, brain).map_err(EngineError::Refused)?;
        self.brain_set_held_lock(&found, brain)?;
        Ok(brain)
    }

    /// The owner sets the Claude a member judges with (ADR 0131), by its name. Every member
    /// has one: Haiku until the owner says otherwise (the CEO, Sonnet). The next turn of
    /// judgment starts with it.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; [`Refusal::NotABrain`] for
    /// a name that is not a Claude its role may use; a store failure.
    pub fn set_judgment_brain(&self, member: MemberId, brain: &str) -> Done<Brain> {
        let _held = self.held();
        let found = self.member(member)?;
        let brain = Brain::for_judgment(found.role, brain).map_err(EngineError::Refused)?;
        self.brain_set_held_lock(&found, brain)?;
        Ok(brain)
    }

    /// Records a brain set (the lock held): in the slot it belongs to, and said in the feed.
    fn brain_set_held_lock(&self, found: &Member, brain: Brain) -> Done<()> {
        self.store.set_brain(found.id, brain)?;
        self.emit(
            EventKind::BrainSet,
            json!({ "member": found.id.0, "name": found.name, "brain": brain.name(),
                    "slot": if brain.house() { "work" } else { "judgment" } }),
        )
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

    /// Endora seeds the house log with how this lab is built ([`house::lab_built`]), once, and
    /// only when it is empty: what the CEO or the owner wrote is never overwritten, and a log
    /// the owner cleared later stays clear. Said in the feed as Endora's. Returns whether it
    /// was seeded now.
    ///
    /// # Errors
    /// A store failure.
    pub fn seed_house_log(&self) -> Done<bool> {
        let _held = self.held();
        if self.store.setting(HOUSE_LOG_SEEDED)?.is_some() {
            return Ok(false);
        }
        self.store.set_setting(HOUSE_LOG_SEEDED, "1")?;
        if !self.house_log()?.trim().is_empty() {
            return Ok(false);
        }
        self.keep_house_log(
            &house::lab_built(),
            "Endora",
            json!({ "entry": endora_kernel::lab::LAB_BUILT_TITLE, "seeded": true }),
        )?;
        Ok(true)
    }

    /// The owner rewrites the house log on HQ's screen; empty clears it. Once the house log is
    /// the house page's timeline (ADR 0122), it is kept read-only for a release: refused.
    ///
    /// # Errors
    /// [`Refusal::TooLong`] past [`house::HOUSE_LOG_MOST`];
    /// [`crate::domain::world::WorldRefusal::HouseLogRetired`] once moved; a store failure.
    pub fn set_house_log(&self, log: &str) -> Done<()> {
        let _held = self.held();
        if self.world_migrated()? {
            return Err(EngineError::Refused(Refusal::World(
                crate::domain::world::WorldRefusal::HouseLogRetired,
            )));
        }
        let log = house::rewritten(log).map_err(EngineError::Refused)?;
        self.keep_house_log(&log, "owner", json!({ "rewritten": true }))
    }

    /// A turn of the CEO's adds a dated line to the house page's timeline (ADR 0122; before
    /// it, to the house log). Returns the line.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::OnlyTheCeo`] for anyone else; [`EngineError::Unknown`]
    /// for no entry; [`Refusal::TooLong`] past [`house::HOUSE_ENTRY_MOST`].
    pub fn note_house(&self, run: RunId, entry: &str) -> Done<String> {
        let _held = self.held();
        let (run, ceo) = self.the_ceos_turn(run)?;
        if entry.trim().is_empty() {
            return Err(EngineError::Unknown("entry for the house page's timeline"));
        }
        let line = self.note_house_line(&run, &ceo, entry)?;
        if let Some(task) = run.task {
            self.mark_progress(ceo.id, task)?;
        }
        Ok(line)
    }

    /// A turn of the CEO's once rewrote the house log whole. The house log is the house page
    /// now (ADR 0122): refused, saying what to use instead, for one release.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::OnlyTheCeo`];
    /// [`crate::domain::world::WorldRefusal::HouseLogRetired`].
    pub fn rewrite_house(&self, run: RunId, _log: &str) -> Done<usize> {
        let _held = self.held();
        self.the_ceos_turn(run)?;
        Err(EngineError::Refused(Refusal::World(
            crate::domain::world::WorldRefusal::HouseLogRetired,
        )))
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

    /// A member's turn reads one of Endora's public library (its records, plans, drafts and
    /// the team's standards), a part at a time ([`docs::DOC_PART`] characters), or the list of
    /// a folder's. They are public already; nothing else is read.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::NotADoc`] for any other path;
    /// [`EngineError::Unknown`] for a document there is not.
    pub fn read_doc(&self, run: RunId, path: &str, part: usize) -> Done<String> {
        self.running(run)?;
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
                            "document: read_doc docs/adr, docs/plans or docs/drafts to list them",
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
        if by != Author::Owner {
            return self.propose_goal_held_lock(title, reason, None, &format!("A goal: {title}"));
        }
        self.set_goal_authored_held_lock(title, reason, InputOrigin::Owner)
    }

    fn set_generated_goal(&self, title: &str, reason: &str) -> Done<GoalId> {
        let _held = self.held();
        self.set_goal_authored_held_lock(title, reason, InputOrigin::Unknown)
    }

    fn set_goal_authored_held_lock(
        &self,
        title: &str,
        reason: &str,
        origin: InputOrigin,
    ) -> Done<GoalId> {
        let status = GoalStatus::Active;
        let goal = self
            .store
            .add_goal_from(title, reason, status, None, origin)?;
        self.emit(
            EventKind::GoalChanged,
            json!({ "goal": goal.0, "title": title, "status": status.name() }),
        )?;
        Ok(goal)
    }

    /// A proposed goal, waiting on the owner's desk as `text`, from `from` if a member.
    fn propose_goal_held_lock(
        &self,
        title: &str,
        reason: &str,
        from: Option<MemberId>,
        text: &str,
    ) -> Done<GoalId> {
        let goal = self
            .store
            .add_goal(title, reason, GoalStatus::Proposed, None)?;
        let desk = self
            .store
            .add_desk("goal", None, Some(goal), from, text, self.clock.now())?;
        self.emit(
            EventKind::GoalProposed,
            json!({ "goal": goal.0, "title": title }),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": "goal" }),
        )?;
        Ok(goal)
    }

    /// Creates a task. A member hands work only to members below it; the task records how
    /// many hands it passed through. Its assignee is woken, unless it waits on something. It
    /// says nothing of what done means: the owner's front door is [`Self::add_task_with`], which
    /// asks for it (ADR 0124, amended), and this is what that, a routine and a started project
    /// make their tasks with.
    ///
    /// # Errors
    /// [`EngineError::Refused`] if a member hands work up or across;
    /// [`EngineError::Unknown`] for a goal, task or member that does not exist.
    pub fn add_task(&self, by: Author, task: NewTask) -> Done<TaskId> {
        let _held = self.held();
        let _asked = (by == Author::Owner).then(|| self.for_owner());
        let origin = if by == Author::Owner {
            InputOrigin::Owner
        } else {
            InputOrigin::Unknown
        };
        self.add_task_authored_held_lock(by, task, origin)
    }

    fn add_task_held_lock(&self, by: Author, task: NewTask) -> Done<TaskId> {
        // Generated work may spend the owner's budget; that does not establish
        // that the owner authored its title, body or borrowed context.
        self.add_task_authored_held_lock(by, task, InputOrigin::Unknown)
    }

    fn add_task_authored_held_lock(
        &self,
        by: Author,
        task: NewTask,
        origin: InputOrigin,
    ) -> Done<TaskId> {
        self.store
            .goal(task.goal)?
            .ok_or(EngineError::Unknown("goal"))?;
        // A task waits only on work still to be done: waiting on what is done has nothing to
        // wait for, and on what was dropped waits for what will not happen (ADR 0124, amended).
        for wait in &task.waits_on {
            let found = self.task(*wait)?;
            if found.status.finished() {
                return Err(EngineError::Refused(Refusal::Guard(
                    GuardRefusal::WaitsOnFinished {
                        task: *wait,
                        status: found.status,
                    },
                )));
            }
        }
        let (parent_depth, project) = match task.parent {
            // A sub-task goes in its parent's project, whatever it was asked to go in.
            Some(parent) => {
                let parent = self.task(parent)?;
                (Some(parent.depth), parent.project)
            }
            None => (None, task.project),
        };
        if let Some(project) = project {
            let found = self.project(project)?;
            crate::domain::projects::may_hold(found.goal, found.status, task.goal)
                .map_err(EngineError::Refused)?;
        }
        let task = NewTask { project, ..task };
        if let (Author::Member(from), Some(to)) = (by, task.assignee) {
            if from != to {
                let (from, to) = (self.member(from)?, self.member(to)?);
                work::may_delegate(from.role, to.role).map_err(EngineError::Refused)?;
            }
        }
        if let Some(to) = task.assignee {
            let to = self.member(to)?;
            if to.furloughed() {
                return Err(EngineError::Refused(Refusal::Furloughed(to.name)));
            }
        }
        if let Author::Member(from) = by {
            self.not_covered(from, &task)?;
        }
        let depth = match (parent_depth, by) {
            (Some(depth), _) => depth.saturating_add(1),
            (None, Author::Member(_)) => 1,
            (None, _) => 0,
        };
        let id = self.store.add_task_from(&task, depth, origin)?;
        self.emit(
            EventKind::TaskCreated,
            json!({
                "task": id.0, "goal": task.goal.0, "parent": task.parent.map(|p| p.0),
                "project": task.project.map(|p| p.0),
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

    /// Refuses a member's new task when an open task under the same goal is already about the
    /// same home-lab containers ([`review::about`], against the names the node last said): one
    /// task a problem. Live, the lead handed the developer the same three exited containers
    /// twice, and each task drew its own turns. The member's own task above the new one, and
    /// everything above that, is the work being split, not a second task: so a member on its
    /// own sub-task of the alarm's task may split it further, while a sub-task made on
    /// another member's task about the same containers is the duplicate it looks like.
    /// Nothing is judged before the node has said what the home lab has.
    fn not_covered(&self, from: MemberId, task: &NewTask) -> Done<()> {
        let Some(lab) = self.lab_names() else {
            return Ok(());
        };
        let about = review::about(&task.title, &task.description, &lab);
        if about.is_empty() {
            return Ok(());
        }
        let mut own = Vec::new();
        let mut mine = false;
        let mut up = task.parent;
        while let Some(id) = up {
            let above = self.task(id)?;
            mine = mine || above.owner == Some(from) || above.assignee == Some(from);
            if mine {
                own.push(id);
            }
            up = above.parent;
        }
        for open in self.store.tasks()? {
            if open.goal != task.goal || open.status.finished() || own.contains(&open.id) {
                continue;
            }
            let theirs = review::about(&open.title, &open.description, &lab);
            let names: Vec<String> = about
                .iter()
                .filter(|n| theirs.contains(n))
                .cloned()
                .collect();
            if !names.is_empty() {
                return Err(EngineError::Refused(Refusal::Covered {
                    task: open.id,
                    names,
                }));
            }
        }
        Ok(())
    }

    /// Whether every task in `waits` is done. A dropped one is not: the work waited for never
    /// happened, so whoever waits must decide what now (see [`Self::dropped_wait`]).
    fn waits_over(&self, waits: &[TaskId]) -> Done<bool> {
        for wait in waits {
            if self.task(*wait)?.status != TaskStatus::Done {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// What a task waits on that is still open: the tasks it waits for, and its sub-tasks.
    fn waiting_on(&self, task: &Task) -> Done<Vec<TaskId>> {
        let mut open = Vec::new();
        for wait in &task.waits_on {
            if !self.task(*wait)?.status.finished() {
                open.push(*wait);
            }
        }
        for sub in self.store.tasks()? {
            if sub.parent == Some(task.id) && !sub.status.finished() {
                open.push(sub.id);
            }
        }
        Ok(open)
    }

    /// The first task in `waits` that was dropped, if any: a wait that will never finish.
    fn dropped_wait(&self, waits: &[TaskId]) -> Done<Option<Task>> {
        for wait in waits {
            let task = self.task(*wait)?;
            if task.status == TaskStatus::Dropped {
                return Ok(Some(task));
            }
        }
        Ok(None)
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

    /// The live turns a wake about `task` meets. A wake of judgment (`judgment`) may join any
    /// of them but a work turn already running on the house's models: it would be judged by a
    /// brain that must not judge (ADR 0131).
    fn live_turns(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        judgment: bool,
    ) -> Done<Vec<LiveTurn>> {
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
                foldable: !(judgment
                    && run.status == RunStatus::Running
                    && run.brain.is_some_and(Brain::house)),
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
        let project = self.project_of(task)?;
        Ok(WakeSituation {
            waits_for_review,
            not_for_the_ceo: member.role == Role::Ceo && !reason.reaches_the_ceo(),
            project_paused: self.held_by_owner(project)?,
            stopped: self.store.stopped()?,
            paused: member.paused,
            furloughed: member.furloughed(),
            // Whether the money is there is not asked of a wake (ADR 0131): the turn is queued,
            // and waits for budget where turns start.
            about_a_task: task.is_some(),
            live: self.live_turns(
                member.id,
                task,
                self.need_of_wake(member, task, reason)?.kind == turns::TurnKind::Judgment,
            )?,
            already_held,
            nothing_to_do,
            // A wake that brings nothing new, to a member its last turns showed nothing for,
            // again and again (ADR 0124): parked, not woken once more.
            woken_for_nothing: match task {
                Some(task) if !reason.brings_news() => self.woken_for_nothing(member.id, task)?,
                _ => false,
            },
            // Talk on a task that has had its budget of it, with nothing changing, is held while
            // the guard is on and its lead can decide it (ADR 0133).
            coordination_held: match task {
                Some(task) if reason.is_talk() => self.talk_is_held(task)?,
                _ => false,
            },
        })
    }

    fn decide_wake(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<(WakeDecision, bool)> {
        let who = self.member(member)?;
        // News is never kept waiting: a turn on its task held back by the back-off, or waiting
        // to try again, is brought forward to take it.
        if let (Some(task), true) = (task, reason.brings_news()) {
            self.bring_forward(task)?;
        }
        let situation = self.situation(&who, task, reason)?;
        let decision = match task {
            // The check after a deploy has one turn (ADR 0115, amended): once its verdict is
            // given, nothing wakes it again.
            Some(task) if self.deploy_check_given(task)? => {
                WakeDecision::Refuse(wakes::Refused::Checked)
            }
            _ => wakes::decide(member, &situation),
        };
        Ok((decision, situation.coordination_held))
    }

    fn wake_held_lock(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<Receipt> {
        if reason == WakeReason::Consulted {
            return Err(EngineError::Refused(Refusal::Conversation(
                "legacy consultation wakes are retired; use authorized assistance",
            )));
        }
        let (decision, coordination_held) = self.decide_wake(member, task, reason)?;
        let now = self.clock.now();
        let mut about = json!({
            "member": member.0, "task": task.map(|t| t.0), "reason": reason.name(),
        });
        match decision {
            WakeDecision::Queue => {
                let wake = self
                    .store
                    .add_wake(member, task, reason, Receipt::Queued, None, now)?;
                self.mark_owners(wake, None)?;
                let (run, delayed) =
                    self.queue_turn(member, task, wake, reason, self.asked_by_owner())?;
                self.store.set_wake(wake, Receipt::Queued, Some(run))?;
                if let Some(until) = delayed {
                    about["delayed_until"] = json!(until);
                }
                self.emit(EventKind::WakeQueued, about)?;
                Ok(Receipt::Queued)
            }
            WakeDecision::Fold(run) => {
                let wake =
                    self.store
                        .add_wake(member, task, reason, Receipt::Folded, Some(run), now)?;
                self.mark_owners(wake, Some(run))?;
                self.emit(EventKind::WakeFolded, about)?;
                Ok(Receipt::Folded)
            }
            WakeDecision::Hold => {
                let wake = self
                    .store
                    .add_wake(member, task, reason, Receipt::Held, None, now)?;
                self.mark_owners(wake, None)?;
                if coordination_held {
                    about["why"] = json!("coordination_budget");
                }
                self.emit(EventKind::WakeHeld, about)?;
                // Held for the budget of talk: the task's lead is asked what to do, once.
                if let (true, Some(task)) = (coordination_held, task) {
                    self.coordination_held_notice(task)?;
                }
                Ok(Receipt::Held)
            }
            WakeDecision::Refuse(why) => {
                self.store
                    .add_wake(member, task, reason, Receipt::Refused(why), None, now)?;
                about["why"] = json!(why.name());
                self.emit(EventKind::WakeRefused, about)?;
                if let (wakes::Refused::Redundant, Some(task)) = (why, task) {
                    self.found_redundant(member, task)?;
                }
                Ok(Receipt::Refused(why))
            }
        }
    }

    /// Marks a wake the owner's asking, while the engine does what they asked; and the turn it
    /// folds into, if that has not started yet (its reservation is still to be made).
    fn mark_owners(&self, wake: crate::domain::WakeId, folded: Option<RunId>) -> Done<()> {
        if !self.asked_by_owner() {
            return Ok(());
        }
        self.store.set_wake_by_owner(wake)?;
        let Some(run) = folded else {
            return Ok(());
        };
        let Some(mut run) = self.store.run(run)? else {
            return Ok(());
        };
        if run.started_at.is_none() && !run.asked_by_owner {
            run.asked_by_owner = true;
            self.store.save_run(&run)?;
        }
        Ok(())
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
        // A turn's label says whether it did something (ADR 0114). A turn with none (from
        // before labels, or that never settled) cannot be told, so the count stops there
        // rather than guess from whether it moved its task (ADR 0124).
        Ok(ended
            .iter()
            .take_while(|r| r.label.is_some_and(|l| !l.productive()))
            .count())
    }

    /// How long a wake that brings nothing new to `task` waits now (ADR 0107): its back-off,
    /// and at least until [`wakes::AGAIN_AFTER_MS`] after `member`'s last turn on the task ended.
    fn back_off(&self, member: MemberId, task: Option<TaskId>, reason: WakeReason) -> Done<Millis> {
        let Some(task) = task else {
            return Ok(0);
        };
        if reason.brings_news() {
            return Ok(0);
        }
        let last_ended = self
            .store
            .runs()?
            .iter()
            .filter(|r| r.member == member && r.task == Some(task))
            .filter_map(|r| r.ended_at)
            .max();
        let gap = last_ended.map_or(0, |at| {
            (at + wakes::AGAIN_AFTER_MS - self.clock.now()).max(0)
        });
        Ok(routines::back_off(self.empty_turns(task)?).max(gap))
    }

    /// Queues a turn for a wake: at once, or, for a stale wake on a task whose turns keep
    /// moving nothing, waiting out its back-off. Returns the turn, and until when it waits. A
    /// wake the owner's asking raised starts a turn the owner asked for.
    fn queue_turn(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        wake: crate::domain::WakeId,
        reason: WakeReason,
        by_owner: bool,
    ) -> Done<(RunId, Option<Millis>)> {
        let now = self.clock.now();
        let id = self.store.add_run(member, task, wake, now)?;
        let mut run = self.store.run(id)?.ok_or(EngineError::Unknown("run"))?;
        if by_owner {
            run.asked_by_owner = true;
            self.store.save_run(&run)?;
        }
        let wait = self.back_off(member, task, reason)?;
        if wait == 0 {
            return Ok((id, None));
        }
        run.status = RunStatus::RetryWaiting;
        run.retry_at = Some(now + wait);
        run.end_reason =
            "backing off: its last turns moved nothing, or it took one on this moments ago"
                .to_owned();
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
                let (run, _) = self.queue_turn(
                    oldest.member,
                    Some(task),
                    oldest.id,
                    oldest.reason,
                    oldest.by_owner,
                )?;
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
        self.act_for(Some(run.id));
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
        self.ceo_keeps_out(run.member, &self.task(task)?)?;
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
        self.ceo_keeps_out(run.member, &self.task(task)?)?;
        self.set_status_held_lock(run.member, task, to, reason)
    }

    /// A turn writes in a task's thread, within its reach for writing. A post that answers a
    /// question asked of the member up the chain wakes whoever asked it, as the owner's reply
    /// does (ADR 0111).
    ///
    /// # Errors
    /// [`Refusal::TurnOver`], [`Refusal::OutOfReach`], or a task that does not exist.
    pub fn post_in(&self, run: RunId, task: TaskId, text: &str) -> Done<()> {
        self.post_in_marked(run, task, text, true, &[])
    }

    /// A turn's last word, which the worker posts for the model when it stops: in the thread
    /// like any post. It is what the model said, not something it did: a turn whose only word
    /// is this is talk only (ADR 0114), never one that moved its task forward.
    ///
    /// # Errors
    /// As [`Self::post_in`].
    pub fn last_word_in(&self, run: RunId, task: TaskId, text: &str) -> Done<()> {
        self.post_in_marked(run, task, text, false, &[])
    }

    fn post_in_marked(
        &self,
        run: RunId,
        task: TaskId,
        text: &str,
        answers: bool,
        cited: &[crate::domain::Ref],
    ) -> Done<()> {
        let _held = self.held();
        let run = self.running(run)?;
        self.reach(&run, task, true)?;
        let member = self.member(run.member)?;
        let askers = if !answers || text.trim_start().starts_with(chain::QUESTION_FOR) {
            Vec::new()
        } else {
            self.askers_of(&member, task)?
        };
        // Unsupported only when it states something of the member's own, cites nothing, and
        // its turn has no receipt (ADR 0114, amended). The worker's last word (`answers`
        // false) is what the model said as it stopped, never tagged.
        let unsupported = answers && cited.is_empty() && !self.has_receipts(run.id)?;
        self.post_authored_held_lock_marked(
            Author::Member(member.id),
            task,
            text,
            cited,
            unsupported,
            InputOrigin::Run(run.id),
        )?;
        for asker in askers {
            if asker.id == member.id {
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
        // Taking a task is progress; claiming one the member already has is not. The worker
        // claims before every turn, and a turn that did nothing else must count as empty, or
        // the empty-turn back-off never holds a real turn back.
        if current.owner != Some(member) || current.status == TaskStatus::Todo {
            self.mark_progress(member, task)?;
        }
        self.emit(
            EventKind::TaskClaimed,
            json!({ "task": task.0, "member": member.0 }),
        )?;
        Ok(())
    }

    /// Writes to a task's thread, as the owner or Endora. An explicit owner post may
    /// wake named members. A member writes only in its turn, through [`Self::post_in`].
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
        let _asked = self.for_owner();
        let found = self.task(task)?;
        if found.status.finished() {
            return Err(EngineError::Refused(Refusal::Finished));
        }
        self.post_authored_held_lock_marked(by, task, text, &[], false, InputOrigin::Owner)?;
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
        self.post_held_lock_marked(by, task, text, &[], false)
    }

    /// A post HQ writes for a member in that member's own turn, of the member's own words with
    /// names, ids and fixed wording around them: its source is that turn (ADR 0138). Anywhere
    /// else (no turn acting, or another member's) it stays Unknown, as every post did before.
    /// Only for text with nothing borrowed from another run, the owner or outside.
    fn post_own_held_lock(
        &self,
        member: MemberId,
        task: TaskId,
        text: &str,
        cited: &[crate::domain::Ref],
    ) -> Done<()> {
        let origin = match self.acting_run() {
            Some(run)
                if self
                    .store
                    .run(run)?
                    .is_some_and(|turn| turn.member == member) =>
            {
                InputOrigin::Run(run)
            }
            _ => InputOrigin::Unknown,
        };
        self.post_authored_held_lock_marked(
            Author::Member(member),
            task,
            text,
            cited,
            false,
            origin,
        )
    }

    /// HQ's own words in a thread: a template filled only with ids, names, statuses and tool
    /// names, never text from a run, the owner or outside, so it carries no tier (ADR 0138).
    fn post_system_held_lock(&self, task: TaskId, text: &str) -> Done<()> {
        self.post_authored_held_lock_marked(
            Author::Endora,
            task,
            text,
            &[],
            false,
            InputOrigin::System,
        )
    }

    /// Writes to a thread; a member's post marks its turn as one that posted (talk, unless it
    /// did more), never as one that moved its task forward (ADR 0114).
    fn post_held_lock_marked(
        &self,
        by: Author,
        task: TaskId,
        text: &str,
        cited: &[crate::domain::Ref],
        unsupported: bool,
    ) -> Done<()> {
        // A generated approval/report can contain borrowed model text even when
        // its display actor is the owner. Only an explicit authored path has proof.
        self.post_authored_held_lock_marked(
            by,
            task,
            text,
            cited,
            unsupported,
            InputOrigin::Unknown,
        )
    }

    fn post_authored_held_lock_marked(
        &self,
        by: Author,
        task: TaskId,
        text: &str,
        cited: &[crate::domain::Ref],
        unsupported: bool,
        origin: InputOrigin,
    ) -> Done<()> {
        self.task(task)?;
        let author = self.author_name(by)?;
        let at = self.clock.now();
        self.store.post_from(
            task,
            &Post {
                author,
                text: text.to_owned(),
                at,
                unsupported,
            },
            cited,
            origin,
        )?;
        self.after_thread_post(by, task, text, unsupported, origin)
    }

    /// Reporting effects after a persisted task post. Only explicit owner-authored
    /// text dispatches mentions; model prose is not a second peer-help scheduler.
    fn after_thread_post(
        &self,
        by: Author,
        task: TaskId,
        text: &str,
        unsupported: bool,
        origin: InputOrigin,
    ) -> Done<()> {
        let author = self.author_name(by)?;
        // A short line for a bubble. The feed shows the whole post: `post` is its place in the
        // task's thread (posts are only ever added), and `cut` says the preview is not all of it.
        let place = self.store.thread(task)?.len().saturating_sub(1);
        let mut preview: String = text.chars().take(80).collect();
        let cut = text.chars().count() > 80;
        if cut {
            preview.push('…');
        }
        self.emit(
            EventKind::ThreadPosted,
            json!({ "task": task.0, "author": author, "preview": preview, "cut": cut,
                    "post": place, "unsupported": unsupported }),
        )?;
        if let Author::Member(member) = by {
            self.mark_turn(member, task, &|run| run.posted = true)?;
            // The stories and sections it names were used (ADR 0122).
            self.world_used(text)?;
        }
        if by != Author::Owner || origin != InputOrigin::Owner {
            return Ok(());
        }
        let members = self.store.members()?;
        for named in mentions(text, &members) {
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
        // Its holder moves it, or anyone above the holder in the chain of command (ADR 0114),
        // saying so.
        let on_behalf = if current.owner == Some(member) {
            None
        } else if self.above_in_chain(member, current.owner.or(current.assignee))? {
            current.owner.or(current.assignee)
        } else {
            return Err(EngineError::Refused(Refusal::NotYours));
        };
        // A routine's check changes no code: its member closes it (ADR 0107).
        let check = to == TaskStatus::Done && self.is_a_check(task)?;
        if check {
            work::may_close_check(current.status).map_err(EngineError::Refused)?;
        } else {
            work::may_set_status(current.status, to).map_err(EngineError::Refused)?;
        }
        self.store.set_task_status(task, to)?;
        if to == TaskStatus::Blocked {
            self.mark_turn(member, task, &|run| run.blocked = true)?;
        } else {
            self.mark_progress(member, task)?;
        }
        let mut said = if to == TaskStatus::Todo {
            format!("Handed back: {reason}")
        } else {
            format!("{} → {}: {reason}", current.status.name(), to.name())
        };
        if let Some(holder) = on_behalf {
            said.push_str(&format!(
                " (for {}, as one above them)",
                self.member(holder)?.name
            ));
        }
        self.post_own_held_lock(member, task, &said, &[])?;
        if to == TaskStatus::Todo {
            self.store.release(task)?;
            self.emit(
                EventKind::TaskReleased,
                json!({ "task": task.0, "member": member.0 }),
            )?;
            for lead in self.leads_to_wake(&current, member)? {
                self.wake_held_lock(lead, Some(task), WakeReason::Assigned)?;
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
        // Sent back three times: a huddle of its area decides it (ADR 0120, amending 0114's
        // owner's round); the owner's only with no lead to call one.
        let rung = self.rung_of(task.id)?;
        // The carousel's huddle is deciding it (ADR 0124): no round before its decision.
        let carousel = self
            .store
            .findings()?
            .iter()
            .any(|f| f.key == format!("carousel:{}", task.id.0) && f.cleared_at.is_none());
        if carousel && matches!(rung, Some((crate::domain::ladder::Rung::Huddle, _))) {
            self.post_held_lock(
                Author::Endora,
                task.id,
                "Submitted while its huddle decides it: it waits for the lead's decision, not \
                 another round.",
            )?;
            return Ok(());
        }
        let decided = matches!(rung, Some((crate::domain::ladder::Rung::Decided, _)));
        if !decided && crate::domain::evidence::to_the_owner(self.sent_back(task.id)?) {
            if let Some(lead) = self.area_lead(task)? {
                let rounds = self.sent_back(task.id)?;
                return self.call_huddle(task, &lead, &format!("sent back {rounds} times"));
            }
            return self.rounds_to_the_owner(task);
        }
        match self.checkers_of(task)? {
            crate::domain::evidence::Checkers::Both { tester, .. }
            | crate::domain::evidence::Checkers::TesterAlone(tester) => {
                self.wake_held_lock(tester, Some(task.id), WakeReason::ReviewRequested)?;
                return Ok(());
            }
            crate::domain::evidence::Checkers::LeadAlone(lead) => {
                let name = self.member(lead)?.name;
                self.post_held_lock(
                    Author::Endora,
                    task.id,
                    &format!(
                        "No tester but the one who did this work: {name}, its lead, accepts it \
                         alone."
                    ),
                )?;
                self.wake_held_lock(lead, Some(task.id), WakeReason::AcceptanceAsked)?;
                return Ok(());
            }
            crate::domain::evidence::Checkers::Owner => {}
        }
        let members = self.store.members()?;
        let thread: Vec<String> = self
            .store
            .thread(task.id)?
            .iter()
            .rev()
            .take(12)
            .rev()
            .map(|p| Self::post_line(&members, &p.author, &p.text, false))
            .map(|line| format!("- {line}"))
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
    /// The tester's tool reaches it for a send-back alone ([`Self::test_in`]): a pass is judged
    /// there, on evidence, and never closes a draft for the owner (ADR 0114, amended).
    ///
    /// A send-back may name its `reason` when it is one HQ counts (ADR 0132); a pass has none.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`], [`Refusal::OutOfReach`]; [`Refusal::NotYours`] for a member who
    /// is not a reviewer; [`Refusal::NotAStep`] for a task not in review; [`Refusal::OwnWork`];
    /// [`Refusal::GateNotPassed`] for work that needs the gate.
    pub fn review_in(
        &self,
        run: RunId,
        task: TaskId,
        approve: bool,
        notes: &str,
        reason: Option<SendBackReason>,
    ) -> Done<()> {
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
        // The tester's verdict is her deliverable, whichever way it goes (ADR 0114).
        self.mark_progress(reviewer.id, task)?;
        if !approve {
            self.store.set_task_status(task, TaskStatus::Doing)?;
            self.post_own_held_lock(reviewer.id, task, &format!("Sent back: {notes}"), &[])?;
            self.emit_status(&current, TaskStatus::Doing)?;
            self.emit_sent_back(task, reviewer.id, "tester", reason)?;
            if let Some(owner) = worker {
                self.wake_held_lock(owner, Some(task), WakeReason::SentBack)?;
            }
            // Bounced with nothing new between: a huddle, not another round (ADR 0124).
            return self.after_sent_back(task);
        }
        let the_home_labs = self.the_home_labs(&current)?;
        let gated = review::needs_gate(self.routine_of(task)?.is_some(), the_home_labs);
        if gated {
            // Gated work keeps the gate's rule: done only once the gate has passed.
            self.approve_held_lock(reviewer.id, task)?;
            self.post_own_held_lock(reviewer.id, task, &format!("Approved: {notes}"), &[])?;
            return Ok(());
        }
        review::may_approve(false, false, worker, reviewer.id).map_err(EngineError::Refused)?;
        self.store.set_task_status(task, TaskStatus::Done)?;
        self.post_own_held_lock(reviewer.id, task, &format!("Approved: {notes}"), &[])?;
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

    /// Says in the feed that `by`, a `tester` or a `lead` (`via`), sent a task back from review,
    /// and why if the reason is one HQ counts (ADR 0132). Every send-back is told, with the
    /// reason null when none was named, so the feed can count them all.
    fn emit_sent_back(
        &self,
        task: TaskId,
        by: MemberId,
        via: &str,
        reason: Option<SendBackReason>,
    ) -> Done<()> {
        self.emit(
            EventKind::SentBack,
            json!({ "task": task.0, "by": by.0, "via": via,
                    "reason": reason.map(SendBackReason::name) }),
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

    /// The owner drops a task, giving no reason: work that will not be done (ADR 0124; see
    /// [`Self::drop_task_because`] to say why).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that does not exist.
    pub fn drop_task(&self, task: TaskId) -> Done<()> {
        use crate::domain::closure::{Closure, DropWhy};
        self.drop_task_because(task, Closure::Dropped(DropWhy::WontDo), None)
    }

    /// When work closes it carries a reason, and what waited on a dropped task is told (ADR
    /// 0124); when the last sub-task of a task finishes, its owner is woken.
    fn after_finishing(&self, finished: &Task) -> Done<()> {
        self.after_closing(finished.id)?;
        // Work done with a branch submitted: the push is the owner's to approve (ADR 0115).
        if self
            .store
            .task(finished.id)?
            .is_some_and(|t| t.status == TaskStatus::Done)
        {
            self.offer_push(finished.id)?;
        }
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

    /// What one model call of a turn used, in tokens, as the relay reported them: input
    /// (fresh, read from the prompt cache and written to it) and output. The turn's record
    /// keeps the sums, and its end says them, and how much of its input the cache gave (see
    /// [`runs::cached_share`]).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a run that does not exist; a store failure.
    pub fn note_tokens(
        &self,
        run: RunId,
        fresh: i64,
        read: i64,
        written: i64,
        output: i64,
    ) -> Done<()> {
        let _held = self.held();
        self.store.run(run)?.ok_or(EngineError::Unknown("run"))?;
        self.store.add_tokens(run, fresh, read, written, output)?;
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
        self.watch_budget(self.project_of_run(&current)?)?;
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
        )?;
        self.watch_budget(self.project_of_run(&run)?)
    }

    // ---- the kill switch -----------------------------------------------------------------

    /// Stops every turn at once; every wake is turned away until the owner resumes.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn stop_all(&self, why: &str) -> Done<()> {
        let _held = self.held();
        self.stop_all_held_lock("owner", why, json!({}))
    }

    /// Stops every turn at once and turns every wake away, as `by` (the owner, or the runaway
    /// tripwire, ADR 0133) and for `why`; `evidence`, an object, is told in the feed with it. The
    /// lock is held.
    fn stop_all_held_lock(&self, by: &str, why: &str, evidence: serde_json::Value) -> Done<()> {
        self.store.set_stopped(true)?;
        self.store.cancel_assistance()?;
        self.store.set_setting(runaway::STOPPED_BY, by)?;
        let reason = if by == "owner" {
            "the kill switch"
        } else {
            "the runaway tripwire"
        };
        for run in self.store.runs()? {
            if !run.status.live() {
                continue;
            }
            if run.status == RunStatus::Running {
                self.runner.stop(run.id, self.settings.grace);
            }
            self.end(run, RunStatus::Stopped, reason)?;
        }
        let mut told = json!({ "by": by, "why": why });
        if let (Some(told), Some(more)) = (told.as_object_mut(), evidence.as_object()) {
            told.extend(more.clone());
        }
        self.emit(EventKind::Stopped, told)
    }

    /// Lets the team work again. Only the owner does this.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn resume(&self) -> Done<()> {
        let _held = self.held();
        self.resume_held_lock()
    }

    // ---- runs --------------------------------------------------------------------------------

    /// Why a turn was started: the reason of the wake that queued it.
    /// How hard the member's Claude tries this turn, by its role and what the turn is for
    /// (see [`effort::effort_for`]): for the CEO, what the turn is for.
    fn effort_of(&self, member: &Member, run: &Run) -> Done<Option<Effort>> {
        if member.role != Role::Ceo {
            return Ok(effort::effort_for(
                member.brain,
                member.role,
                effort::Weight::Ordinary,
            ));
        }
        let (drafted, review) = match run.task {
            Some(task) => (
                self.ceos_draft(&self.task(task)?)?.is_some(),
                self.routine_of(task)?.is_some(),
            ),
            None => (false, false),
        };
        let proposals = self.proposals_waiting()? > 0;
        let weight = WakeReason::from_name(self.reason_for(run)?)
            .map_or(effort::Weight::Ordinary, |r| {
                effort::ceo_weight(r, review, proposals, drafted)
            });
        Ok(effort::effort_for(member.brain, member.role, weight))
    }

    fn reason_for(&self, run: &Run) -> Done<&'static str> {
        Ok(self.store.wake(run.wake)?.map_or("", |w| w.reason.name()))
    }

    /// Ends a turn: settles its money, closes its wake, and lets the next held wake on its task
    /// go. A turn that moved its task forward earns the task a fresh recovery nudge.
    fn end(&self, mut run: Run, status: RunStatus, why: &str) -> Done<()> {
        let now = self.clock.now();
        if run.started_at.is_some() {
            self.store.settle(run.id, run.member, self.today(), now)?;
        }
        let label = self.settle_label(&mut run)?;
        run.status = status;
        run.ended_at = Some(now);
        run.end_reason = why.to_owned();
        run.reserved = 0;
        self.store.save_run(&run)?;
        if self.store.assistance_for_run(run.id)?.is_some() {
            self.store
                .close_unanswered_assistance(run.id, status == RunStatus::Succeeded)?;
        }
        self.store
            .set_wake(run.wake, Receipt::Finished, Some(run.id))?;
        // The tokens as the relay reported them, which only it adds to: all the input sent
        // and the output, for a turn that sent any, and the share the cache gave.
        let kept = self.store.run(run.id)?;
        let cached = kept.as_ref().and_then(|r| {
            runs::cached_share(r.input_tokens, r.cache_read_tokens, r.cache_write_tokens)
        });
        let tokens = kept
            .as_ref()
            .map(|r| {
                (
                    r.input_tokens + r.cache_read_tokens + r.cache_write_tokens,
                    r.output_tokens,
                )
            })
            .filter(|(input, output)| input + output > 0);
        self.emit(
            EventKind::RunEnded,
            json!({ "run": run.id.0, "member": run.member.0, "task": run.task.map(|t| t.0),
                    "result": status.name(), "why": why, "moved_forward": run.moved_forward,
                    "label": label.map(crate::domain::Label::name),
                    "spent": run.spent, "cached": cached,
                    "input": tokens.map(|t| t.0), "output": tokens.map(|t| t.1) }),
        )?;
        if let Some(task) = run.task {
            // A turn that showed something earns its task a fresh recovery nudge; a reading the
            // turn before had already taken shows nothing, so it earns none.
            let repeated = self
                .shown_nothing(run.member, task)?
                .last()
                .is_some_and(|(id, nothing)| *id == run.id && *nothing);
            if label.is_some_and(crate::domain::Label::productive) && !repeated {
                self.store.set_nudged(task, false)?;
            }
            self.release_held(task)?;
            // A huddle hears each member once (ADR 0120).
            self.after_huddle_turn(&run)?;
        }
        if status == RunStatus::Succeeded && label.is_some() {
            // Enough evidence, re-checked three times: HQ submits it (ADR 0124).
            self.submit_when_enough(&run)?;
            self.after_nothing_shown(&run)?;
        }
        // A deploy check's one turn is over: its verdict, once (ADR 0115, amended).
        self.after_a_deploy_check(&run)?;
        Ok(())
    }

    /// Ends a turn that put a blocker to someone: a question, a draft, a decision, an approval.
    fn end_blocked(&self, mut run: Run, status: RunStatus, why: &str) -> Done<()> {
        run.blocked = true;
        self.end(run, status, why)
    }

    /// A failed turn: tried again a minute later, at most twice; then failed for good. A turn the
    /// platform cut short (HQ or the runner restarted under it, its container vanished) did
    /// nothing wrong: the first time, its start does not count against its retries, and it says
    /// it was interrupted (ADR 0124, amended).
    fn fail(&self, mut run: Run, why: &str, fault: Fault) -> Done<bool> {
        let now = self.clock.now();
        let why = match fault {
            Fault::Worker => why.to_owned(),
            Fault::Platform => {
                run.interrupted = run.interrupted.saturating_add(1);
                format!("interrupted: {why}")
            }
        };
        let why = why.as_str();
        if let Some(status) = self.assistance_terminal_for(run.id)? {
            self.end(
                run,
                status,
                &format!("assistance ended before worker recovery: {why}"),
            )?;
            return Ok(false);
        }
        let assistance_delivery = self.store.assistance_for_run(run.id)?.is_some()
            || self.store.assistance_notice_for_run(run.id)?.is_some();
        // A turn on the owner's Mac that failed because the Mac went away (the owner came back,
        // the laptop left its charger) did nothing wrong: it is tried again when the Mac is
        // free, and the failure is not counted against it (ADR 0130).
        let on_the_mac = run.brain == Some(Brain::Mac);
        if !assistance_delivery
            && on_the_mac
            && mac::wait_for(self.mac_facts().as_ref(), now, false).is_some()
        {
            run.attempts = run.attempts.saturating_sub(1);
        }
        let own = if assistance_delivery {
            run.attempts
        } else {
            runs::own_attempts(run.attempts, run.interrupted)
        };
        let Some(due) = runs::retry_after_failure(own, now) else {
            self.end(run, RunStatus::Failed, why)?;
            return Ok(false);
        };
        // A retry is a stale continuation: it backs off like one.
        let due = due.max(now + self.back_off(run.member, run.task, WakeReason::Retry)?);
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
                    "result": RunStatus::RetryWaiting.name(), "why": why,
                    "interrupted": fault == Fault::Platform }),
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
            .filter(|m| m.id != lead.id && !m.paused && !m.furloughed())
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

    /// What a member is told at the start of a turn: who it is, then what of the team's world
    /// its situation needs within its budget (ADR 0122), then its work; and at the very end,
    /// what of the world was left out and how to read it. The run records its situation.
    fn briefing(&self, member: &Member, run: &Run) -> Done<String> {
        self.prepare_conversation_turn(run.id)?;
        if let Some(request) = self.store.assistance_for_run(run.id)? {
            self.assistance_reached(run.id, &[request.requester_run])?;
            return self.assistance_briefing(member, &request);
        }
        if let Some(request) = self.store.assistance_notice_for_run(run.id)? {
            let mut sources = vec![request.requester_run];
            sources.extend(request.expert_run);
            self.assistance_reached(run.id, &sources)?;
            return self.assistance_notice_briefing(member, &request);
        }
        let mut told = if self.canonical_task_context_applies(member, run)? {
            self.canonical_task_briefing(member, run)?
        } else {
            let (_, memory) = self.memory(member, run)?;
            let mut told = self.briefing_body(member, run, &memory.text)?;
            told.push_str(&memory.omitted);
            told
        };
        // Decided last, once the briefing has recorded what it showed the turn: a turn is told
        // of colleague help only when it is offered the tools (downtime lists them itself).
        if self.reason_for(run)? != WakeReason::Downtime.name()
            && self.colleague_help_said(member, run)?
        {
            told.push_str(COLLEAGUE_HELP);
        }
        Ok(told)
    }

    fn colleague_help_said(&self, member: &Member, run: &Run) -> Done<bool> {
        Ok(!self.in_quiz(run.id)? && self.may_ask_colleagues(member, run)?)
    }

    /// The briefing, around what it carries of the world (`memory`).
    fn briefing_body(&self, member: &Member, run: &Run, memory: &str) -> Done<String> {
        let mut told = format!(
            "You are {}, the team's {}. Today is {}.\n",
            member.name,
            match member.role {
                Role::Ceo => "CEO",
                // The reviewer's role is shown as the tester (ADR 0114).
                Role::Reviewer => "tester",
                role => role.name(),
            },
            self.said_today()
        );
        // What it knows, first: its situation's pieces of the world, its stories and its own
        // notebook (ADR 0118, 0122).
        told.push_str(memory);
        // Downtime has no task to brief: the office, the open proposals and what may be done
        // (ADR 0119).
        if self.reason_for(run)? == WakeReason::Downtime.name() {
            told.push_str(&self.downtime_said(member, run.id)?);
            // Everything this turn is shown has been recorded by now, so the offer is known.
            let colleague_communication = self.colleague_help_said(member, run)?;
            if colleague_communication {
                told.push_str(COLLEAGUE_HELP);
            }
            // The quiz's answer is offered in the quiz's turn alone, so only that one is told of it.
            told.push_str(
                "Your tools at HQ: my_turn, office, chess, note, forget, read_notebook, read_doc, \
                 read_world",
            );
            if colleague_communication {
                told.push_str(
                    ", read_messages, send_message, consult, request_assistance, cancel_assistance",
                );
            }
            told.push_str(if self.in_quiz(run.id)? {
                ", answer_quiz.\n"
            } else {
                ".\n"
            });
            return Ok(told);
        }
        // The CEO's quiet briefing is held under half its whole one: its read_doc names the
        // standards instead.
        if member.role != Role::Ceo {
            told.push_str(STANDARDS);
        }
        if member.role == Role::Ceo {
            // The CEO's context reads across the house without lineage yet (ADR 0138 A2e).
            self.unclassified_message_input(run.id)?;
            told.push_str(&self.ceo_context(member, run)?);
        }
        if member.role == Role::Reviewer {
            told.push_str(&format!("{}\n", review::TESTER));
        }
        told.push_str(self.wing_rules(member)?);
        let on = match run.task {
            Some(task) => Some(self.task(task)?),
            None => None,
        };
        if let Some(above) = self.asked_of(member, on.as_ref())? {
            told.push_str(&format!(
                "Your questions (ask_owner) go to {}, the {}, in the task's thread, and they \
                 answer there: the owner's desk is the CEO's to use. They are for decisions \
                 only: for help, ask any colleague with consult, or take a hard problem a model \
                 tier up with escalate.\n",
                above.name,
                if above.role == Role::Ceo {
                    "CEO"
                } else {
                    above.role.name()
                }
            ));
        }
        if member.role == Role::Lead {
            // A project with no proposer was made by the owner's route or by HQ; a member's
            // proposal has no recorded run, so its title fails closed (ADR 0138 A2c).
            let proposed =
                self.store.projects()?.iter().any(|p| {
                    p.lead == member.id && p.status.holds_its_cap() && p.proposer.is_some()
                });
            if proposed {
                self.unclassified_message_input(run.id)?;
            }
            told.push_str(&self.projects_led(member)?);
        }
        let Some(task) = run.task else {
            told.push_str("This turn is about no task: look over the team's work.\n");
            return Ok(told);
        };
        // Each section is classified by the lineage of what it shows, and what has none fails
        // closed (ADR 0138 A2c): the task's body and criteria, its parents and goal, and the
        // thread's posts by their recorded origins.
        let input = self
            .store
            .task_inputs(task)?
            .ok_or(EngineError::Unknown("task"))?;
        self.inherit_authored_input(run.id, input.body_origin)?;
        if !input.task.criteria.is_empty() {
            self.inherit_authored_input(run.id, input.criteria_origin)?;
        }
        let task = input.task;
        // The CEO's review and planning say what they ask as HQ asks it now, whatever their
        // routine was made with (ADR 0120).
        let description = match self.routine_of(task.id)? {
            Some(_) if member.role == Role::Ceo && self.is_planning(task.id)? => {
                planning::PLANNING.to_owned()
            }
            Some(r) if member.role == Role::Ceo && r.assignee == member.id => {
                areas::DAILY_REVIEW.to_owned()
            }
            _ => task.description.clone(),
        };
        told.push_str(&format!(
            "Your task: {} (#{}, {}, {})\n{}\n",
            task.title,
            task.id,
            task.status.name(),
            task.priority.name(),
            description
        ));
        if member.role == Role::Lead {
            told.push_str(&self.area_picture(member, run)?);
        }
        told.push_str(&self.criteria_said(&task));
        if let Some(nudge) = self.nudge_for(member.id, run)? {
            told.push_str(&format!("{nudge}\n"));
        }
        // Evidence already enough is submitted, not polished (ADR 0124).
        told.push_str(&self.enough_said_for(member.id, run)?);
        let note = self.goal_note_briefed(task.goal)?;
        if !note.is_empty() {
            self.unclassified_message_input(run.id)?;
            told.push_str(&format!("Notes from the owner for this goal: {note}\n"));
        }
        // What the goal chose not to do now (ADR 0120's strategy), in every briefing on it: the
        // premise each turn works in is bounded in its own words (ADR 0124).
        let not_now = self.strategy_of(task.goal)?.not_now;
        if !not_now.trim().is_empty() {
            self.not_now_reached(run.id, task.goal)?;
            told.push_str(&format!(
                "Not now, for this goal: {}. Work outside the goal's bets waits.\n",
                one_line(&not_now, 600)
            ));
        }
        if member.role == Role::Lead {
            told.push_str(&self.team_for_the_lead(member)?);
        }
        let mut why = vec![task.title.clone()];
        let mut parent = task.parent;
        let mut first_parent = None;
        while let Some(id) = parent {
            let up = self
                .store
                .task_inputs(id)?
                .ok_or(EngineError::Unknown("parent task"))?;
            self.inherit_authored_input(run.id, up.body_origin)?;
            why.push(up.task.title.clone());
            parent = up.task.parent;
            first_parent.get_or_insert(up);
        }
        if let Some(goal) = self.store.goal_input(task.goal)? {
            self.inherit_authored_input(run.id, goal.origin)?;
            why.push(format!("goal: {}", goal.goal.title));
        }
        told.push_str(&format!("Why it exists: {}\n", why.join(" ← ")));
        let serves = self.serves_said(&task)?;
        if let (false, Some(above)) = (serves.is_empty(), &first_parent) {
            self.inherit_authored_input(run.id, above.criteria_origin)?;
        }
        told.push_str(&serves);
        let documents = self.documents_said(task.id)?;
        if !documents.is_empty() {
            self.documents_reached(run.id, task.id)?;
        }
        told.push_str(&documents);
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
            self.unclassified_message_input(run.id)?;
            told.push_str(&format!(
                "You said you would check back: {}\n",
                came_back.why
            ));
        }
        let members = self.store.members()?;
        // The thread with each post's recorded origin, read together.
        let thread = self.all_authored_posts(task.id)?;
        let mut new = Vec::new();
        for input in thread.iter().filter(|p| p.post.at > since) {
            self.inherit_authored_input(run.id, input.origin)?;
            let p = &input.post;
            new.push(Self::post_line(&members, &p.author, &p.text, p.unsupported));
        }
        if new.is_empty() {
            told.push_str("Nothing new in the thread since your last turn.\n");
        } else {
            told.push_str("New in the thread since your last turn:\n");
            for line in new {
                told.push_str(&format!("- {line}\n"));
            }
        }
        if self.reason_for(run)? == WakeReason::QuestionAsked.name() {
            // The unanswered questions are read from the whole thread.
            for input in &thread {
                self.inherit_authored_input(run.id, input.origin)?;
            }
            let posts: Vec<(&str, &str)> = thread
                .iter()
                .map(|p| (p.post.author.as_str(), p.post.text.as_str()))
                .collect();
            for (asker, question) in chain::unanswered(&posts, &member.name) {
                told.push_str(&format!("{asker} asks you: {}\n", record::clean(&question)));
            }
            told.push_str(if member.role == Role::Ceo {
                "Answer in the thread with post: whoever asked is woken when you do. Give your \
                 decision or your direction there; the lead acts on it in its area. Only what the \
                 board alone decides goes to them, as a decision with ask_owner.\n"
            } else {
                "Answer in the thread with post: whoever asked is woken when you do. Decide it, \
                 hand the work on with create_task, or, if it is beyond you, ask up the chain \
                 with ask_owner.\n"
            });
        }
        // ADR 0139: this member's own turn on the house's model was stuck and took the problem a
        // model tier up; its last word in the thread says why.
        if self.reason_for(run)? == WakeReason::Escalated.name() {
            told.push_str(
                "This task went up a model tier: your work on the house's model was stuck, and \
                 why is in its thread. Now on your Claude, work out what it could not: fix the \
                 approach, then carry the task on as usual. A decision that is not yours still \
                 goes to whoever holds it, with ask_owner.\n",
            );
        }
        if self.reason_for(run)? == WakeReason::ReviewRequested.name() {
            // The newest of a draft for the owner and a move to review says which this is, read
            // from the whole thread; the receipts a draft cites have no lineage here yet.
            for input in &thread {
                self.inherit_authored_input(run.id, input.origin)?;
            }
            let members = self.store.members()?;
            let mut receipts_cited = false;
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
                    let cited = self
                        .draft_receipts(draft, member.brain.cloud())
                        .unwrap_or_default();
                    if !cited.trim().is_empty() {
                        receipts_cited = true;
                    }
                    return Some(format!(
                        "{} wrote this draft for the owner: {draft}\n{}\n{cited}",
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
            if receipts_cited {
                self.unclassified_message_input(run.id)?;
            }
            if let Some(asked) = asked {
                told.push_str(&format!("{}\n", asked.trim_end()));
            }
        }
        // Checks' results, a CEO's direction and the ladder's views and decisions have no
        // lineage yet: shown, they fail closed.
        let mut untraced = self.checking_said(member, run, &task)?;
        if self.reason_for(run)? == WakeReason::Directed.name() {
            untraced.push_str(&self.direction_said(member, task.id)?);
        }
        untraced.push_str(&self.ladder_said(member, run, &task)?);
        if !untraced.is_empty() {
            self.unclassified_message_input(run.id)?;
        }
        told.push_str(&untraced);
        if member.role == Role::Ceo {
            if let Some(lead) = self.area_lead(&task)? {
                told.push_str(&format!(
                    "This task is in {}'s area: you read, talk and direct (direct); {} decides \
                     its work.\n",
                    lead.name, lead.name
                ));
            }
        }
        // HQ's own tools, never as all a member has: live, a lead on Claude offered the home
        // lab's read took this line for its whole kit and said it could not read the lab.
        let beside = "Endora's own tools on the home lab (homelab_status and the like) come \
                      beside them: what your tool list offers, you have.\n";
        if member.role == Role::Ceo {
            told.push_str(
                "Your tools at HQ: my_turn, claim, post, set_status, create_task (to a lead), \
                 direct, set_strategy, decide_project, propose_goal, ask_owner (for the board), \
                 check_back, note_house, write_world, decide_proposal, verify_world, \
                 retire_world, propose_canon, tell_story, read_world, propose_world, read_doc, \
                 hand_to (to a lead), \
                 read_document, note, forget, read_notebook, office, judge_finding.\n",
            );
            return Ok(told);
        }
        // The tester is not offered create_task: she passes or sends back (ADR 0114).
        told.push_str(if member.role == Role::Reviewer {
            "Your tools at HQ: my_turn, claim, post, set_status, ask_owner, \
             check_back"
        } else {
            "Your tools at HQ: my_turn, claim, post, set_status, create_task, ask_owner, \
             check_back"
        });
        told.push_str(match member.role {
            Role::Reviewer => {
                ", review, read_document, read_doc, read_world, tell_story, propose_world, office. "
            }
            Role::Lead => {
                ", propose_project, accept, hand_to, request_hire, request_staff, report_area, decide_stuck, narrow_criteria, judge_finding, read_document, read_doc, read_world, tell_story, propose_world, decide_proposal, write_world, verify_world, retire_world, propose_canon, office. "
            }
            _ => ", read_document, read_doc, read_world, tell_story, propose_world, office. ",
        });
        told.push_str(beside);
        told.push_str(notebook::TOOLS);
        Ok(told)
    }

    /// What the CEO is told first (ADR 0111). It opens with what changes rarely, always in one
    /// order: (after the house page, ADR 0122) its team, the goals, and what only the board
    /// decides. Then, on
    /// its first turn and its daily review, the whole picture: the goals' notes and lessons,
    /// the open work, the home lab's trouble and what reached the board's desk in the last
    /// week. On any other turn, only what changed since its last turn began (see
    /// [`Self::ceo_news`]): what it read before is in the house page it keeps and the threads,
    /// and every turn's briefing is paid for again at each of its rounds. Nothing of the
    /// owner's brain, mail or chat: the CEO thinks on a cloud model.
    fn ceo_context(&self, ceo: &Member, run: &Run) -> Done<String> {
        // The house page, which it keeps, comes before this with the rest of what it knows
        // (ADR 0122).
        let mut told = String::new();
        let members = self.store.members()?;
        let name_of = |id: Option<MemberId>| {
            id.and_then(|id| members.iter().find(|m| m.id == id))
                .map_or_else(|| "nobody".to_owned(), |m| m.name.clone())
        };
        let team: Vec<String> = members
            .iter()
            .filter(|m| m.id != ceo.id && !m.furloughed())
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
            "Your team: {}. Each lead runs and decides its area; you set direction, decide \
             what leads propose, and direct a lead (direct).\n",
            if team.is_empty() {
                "nobody yet".to_owned()
            } else {
                team.join(", ")
            }
        ));
        told.push_str(chain::BOARD_ITEMS);
        told.push_str(
            "\nYou wake only for a question up the chain, the board's word, your review and \
             planning, never for the lab's alarms. Read what you choose (threads, the lab, \
             read_doc); acting on the lab is the areas'.\n",
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
        let planning = match run.task {
            Some(task) => self.is_planning(task)?,
            None => false,
        };
        let whole = review || last.is_none();
        told.push_str(&self.projects_past_warning()?);
        if planning {
            told.push_str(&self.planning_picture()?);
            return Ok(told);
        }
        let day = |at: Millis| {
            let (d, m) = lessons::day_and_month(money::day_of(at, self.settings.offset_minutes));
            format!("{d} {m}")
        };
        if let (false, Some(last)) = (whole, last) {
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
            }
            let waiting = self.proposals_waiting()?;
            if waiting > 0 {
                told.push_str(&format!(
                    "Project proposals waiting for your decision: {waiting} (decide_project; \
                     your daily review lists them).\n"
                ));
            }
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
        // The whole, on its first turn and its daily review (ADR 0120): what it steers by
        // (each goal's strategy, the leads' reports, each area's measures, the proposals and
        // the board's word), never the areas' work item by item; it reads that when it
        // chooses to.
        told.push_str(&self.strategies_said(last)?);
        told.push_str(&self.reports_said()?);
        told.push_str(&self.areas_said(7)?);
        told.push_str(&self.proposals_said()?);
        let since = self.clock.now() - CEO_LOOKS_BACK_MS;
        let desk: Vec<DeskItem> = self
            .store
            .desk()?
            .into_iter()
            .filter(|d| d.at >= since)
            .filter(|d| {
                !matches!(
                    d.kind.as_str(),
                    "notice" | "approval" | "stuck" | "question" | "push" | "rounds"
                )
            })
            .collect();
        if desk.is_empty() {
            told.push_str("The board's decisions in the last 7 days: none.\n");
            return Ok(told);
        }
        told.push_str("The board's decisions in the last 7 days:\n");
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
                    "story.told" => format!(
                        "story:{} was {}, by {}",
                        e.data["handle"].as_str().unwrap_or("?"),
                        if e.data["retold"] == true {
                            "retold"
                        } else {
                            "told"
                        },
                        e.data["by"].as_str().unwrap_or("?")
                    ),
                    // The owner's canon, made or demoted (ADR 0122).
                    "world.changed"
                        if matches!(e.data["what"].as_str(), Some("promoted" | "demoted")) =>
                    {
                        format!(
                            "the owner {} {}{}",
                            if e.data["what"] == "promoted" {
                                "made canon"
                            } else {
                                "demoted from canon"
                            },
                            e.data["address"].as_str().unwrap_or("?"),
                            e.data["why"]
                                .as_str()
                                .filter(|w| !w.is_empty())
                                .map(|w| format!(": {w}"))
                                .unwrap_or_default()
                        )
                    }
                    "project.budget" => format!(
                        "project #{} {}: {}",
                        e.data["project"],
                        e.data["title"].as_str().unwrap_or(""),
                        e.data["said"].as_str().unwrap_or("")
                    ),
                    "project.changed" => format!(
                        "project #{} {}: {}",
                        e.data["project"],
                        e.data["title"].as_str().unwrap_or(""),
                        e.data["status"].as_str().unwrap_or("?")
                    ),
                    // The lab's alarms are the areas' to answer (ADR 0120): the CEO hears of
                    // its areas through their leads' reports and measures.
                    "area.reported" => format!(
                        "{} reported on their area (your review reads it)",
                        name_of(e.data["lead"].as_i64().map(MemberId))
                    ),
                    "goal.strategy" => format!(
                        "goal #{}'s strategy was written, by {}",
                        e.data["goal"],
                        e.data["by"].as_str().unwrap_or("?")
                    ),
                    _ => return None,
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
        let now = self.clock.now();
        // A drain before a restart (ADR 0134): the one the last HQ kept is lifted now that HQ is
        // up again, before its turns are looked at, so the ones the restart cut are counted; and
        // one nobody ended outlasts its age.
        self.drain_tick_held_lock(now)?;
        self.expire_assistance_tick(now)?;
        // Owner-requested deletion stays bounded and also runs while the team is stopped.
        self.store.purge_conversation_batch()?;
        if self.store.stopped()? {
            return Ok(report);
        }

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
        // Once a day, on the day's first tick: members with nothing to do are offered to the
        // owner for furlough (ADR 0118).
        self.idle_look()?;
        // A huddle whose views are late goes to its lead to decide (ADR 0120).
        self.huddles_due()?;
        // Check-backs whose time came wake their member, while the task is still open.
        for back in self.store.check_backs()? {
            if back.status != CheckBackStatus::Pending || back.due_at > now {
                continue;
            }
            let about = json!({ "task": back.task.0, "member": back.member.0, "why": back.why });
            let task = self.task(back.task)?;
            if task.status.finished() {
                self.store
                    .set_check_back(back.id, CheckBackStatus::Dropped, now)?;
                self.emit(EventKind::CheckBackDropped, about)?;
                continue;
            }
            // Waiting is not a turn (ADR 0114): a blocked task's wait ending is its wake.
            let waits = self.waiting_on(&task)?;
            if task.status == TaskStatus::Blocked && !waits.is_empty() {
                self.store
                    .set_check_back(back.id, CheckBackStatus::Replaced, now)?;
                let on: Vec<String> = waits.iter().map(|t| format!("#{t}")).collect();
                self.post_held_lock(
                    Author::Endora,
                    back.task,
                    &format!(
                        "The check-back ({}) is replaced by the wait on {}: you are woken when \
                         it finishes, not before.",
                        back.why,
                        on.join(", ")
                    ),
                )?;
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
                    // The runner's words when it gave them: the worker's exit code's meaning
                    // and its last words, which would otherwise be lost with its container.
                    let why = self
                        .runner
                        .why_ended(run.id)
                        .unwrap_or_else(|| "the turn failed".to_owned());
                    // Whose it was: a container the runner found gone, or killed from outside
                    // before its full time, was cut short by the platform (ADR 0124, amended).
                    let ran = run.started_at.map_or(0, |started| now - started);
                    let fault = runs::fault_of(self.runner.exit_of(run.id), ran);
                    if self.fail(run, &why, fault)? {
                        report.retrying += 1;
                    } else {
                        report.ended += 1;
                    }
                }
                // HQ lost the turn: it restarted, and with it the memory of what the runner
                // was handed. Nothing its member did.
                Liveness::Gone => {
                    if self.fail(run, "its container is gone", Fault::Platform)? {
                        report.retrying += 1;
                    } else {
                        report.ended += 1;
                    }
                }
            }
        }

        // A project paused for its budget goes on once the month turns (ADR 0113).
        self.lift_budget_pauses()?;

        // 3. Retries whose time has come are queued again.
        for mut run in self.store.runs()? {
            if run.status == RunStatus::RetryWaiting && run.retry_at.is_some_and(|due| due <= now) {
                run.status = RunStatus::Queued;
                run.retry_at = None;
                self.store.save_run(&run)?;
                report.requeued += 1;
            }
        }

        // Explicit assistance is a durable intent; a bounded set may enter the normal queue.
        self.assistance_tick(now)?;
        self.assistance_notice_tick(now)?;
        // 3b. The office (ADR 0119): proposals whose time came carry or are dropped, and one
        //     member with nothing to do is given a downtime turn.
        self.office_tick()?;
        // The team's map (ADR 0126): a lead hired, a project or goal made since the last look
        // gets its page.
        self.migrate_map_held()?;
        // The team's history (ADR 0129): what is past its thirty days, or beyond the last five
        // of a member on a task, is let go; handover notes and posts are not touched.
        self.history_tick(now)?;
        // The nightly quiz on the map (ADR 0126) takes a free member's downtime first.
        if self.settings.quiz {
            self.quiz_look(now)?;
        }
        if self.settings.downtime {
            self.downtime_look(now)?;
        }
        // Games (ADR 0121): free members play a match in code, and a game of chess nobody
        // has moved in a week is adjourned. No model is asked.
        self.games_tick(now)?;
        // Scenarios (ADR 0135): a world that has ended with its tasks open is closed.
        self.close_ended_scenarios()?;
        // The owner's re-run of the Holodeck: the next scenario starts once none is running.
        self.holodeck_rerun_tick()?;
        // The model league: the next pair of brains sits down to chess once nothing is played.
        self.league_tick(now)?;
        // 3c. A wake held for a task with no turn live on it has nothing to wait behind (ADR
        //     0133): it is asked again, and goes once the task's budget of talk is in hand.
        self.release_held_talk()?;

        // 4. Queued turns start where a member has a free slot, budget can be reserved, and a
        //    runner has room: urgent work first, then the oldest. The tripwire may stop the team
        //    here (ADR 0133): then nothing else in this tick is done, as when the owner stops it.
        report.started = self.start_queued(now)?;
        // Work that waits long for the Mac is said in the owner's digest (Phase 0 audit, P5).
        self.long_mac_waits(now)?;
        if self.store.stopped()? {
            return Ok(report);
        }

        // 5. Doing, with no turn: parked work (ADR 0124) gets one nudge, then climbs the
        //    ladder; work held on purpose is left alone, and work HQ cannot tell about gets
        //    one look from its area's lead.
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
            let Some(owner) = task.owner else {
                continue;
            };
            // An accepted canonical question is deliberate work in progress.
            // Recovery must not occupy the requester ahead of its result turn.
            if self.store.task_awaits_assistance(task.id, owner)? {
                if task.nudged {
                    self.store.set_nudged(task.id, false)?;
                }
                continue;
            }
            // A scenario's world wakes its players (ADR 0135): waiting for it is no stall.
            if self.in_a_scenario(task.id)? {
                continue;
            }
            // Its member said when it comes back, or it waits on open work: that is its plan,
            // not a stall (live, #667 was woken every ten seconds while its check-back waited).
            let Some(found) = self.diagnosis(task.id)? else {
                continue;
            };
            match found.stopped {
                Stopped::Working | Stopped::Held => continue,
                Stopped::Indeterminate => {
                    self.look_once(task, found)?;
                    continue;
                }
                Stopped::Parked => {}
            }
            if !task.nudged {
                self.store.set_nudged(task.id, true)?;
                if self.nudge_once(task, owner, WakeReason::Recovery)? {
                    report.nudged += 1;
                }
                continue;
            }
            // In a lead's area it climbs the ladder (ADR 0120), once it has sat as long as
            // stranded work does: a lead woken to decide is a turn, never a reflex.
            if self.area_lead(task)?.is_some() && !self.idle_for(task.id, STRANDED_AFTER_MS)? {
                continue;
            }
            self.store.set_task_status(task.id, TaskStatus::Blocked)?;
            self.emit(
                EventKind::TaskChanged,
                json!({ "task": task.id.0, "goal": task.goal.0, "owner": owner.0,
                        "status": TaskStatus::Blocked.name(), "priority": task.priority.name() }),
            )?;
            report.stuck += 1;
            self.found_stuck(task, found.why)?;
            // Stuck work climbs the ladder (ADR 0120); the owner's desk only with no lead.
            if !self.climb_stuck(task, "it was nudged once and did not move")? {
                self.stuck_on_the_desk(task, "It was nudged once and did not move.")?;
            }
        }

        // 5b. Open work handed to a member that nothing will bring it back to: no turn queued,
        //     running or waiting to retry, and no wake held for it, for ten minutes. One
        //     recovery wake for its member; still so ten minutes after, the owner's desk, once.
        //     Never handed to anyone else.
        let (stranded_nudged, stranded_stuck) = self.recover_stranded(&tasks, &live, now)?;
        report.nudged += stranded_nudged;
        report.stuck += stranded_stuck;
        // 5c. The guards (ADR 0124): what is stuck and has moved again closes itself, and every
        //     few minutes each project's process health is looked at.
        self.guards_tick()?;
        self.health_tick()?;

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
        // A drain before a restart (ADR 0134) starts nothing: the queue waits, and says why.
        if self.hold_for_restart_held_lock()? {
            return Ok(0);
        }
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
        // What deciding a turn's kind reads, once for the tick (ADR 0131).
        let wakes = self.store.wakes()?;
        let known = self.known_for_kinds()?;
        // What the caps on a runaway read, once for the tick and kept up as turns start (ADR
        // 0133). A scripted team, the demo, has none.
        let mut starts = self.starts_read(now)?;
        let mut started = 0;
        for mut run in queued {
            if let Some(status) = self.assistance_terminal_for(run.id)? {
                self.end(run, status, "assistance request already ended")?;
                continue;
            }
            if self.retired_peer_turn(run.id)? {
                self.end(run, RunStatus::Stopped, "legacy peer help retired")?;
                continue;
            }
            if (self.store.assistance_for_run(run.id)?.is_some()
                || self.store.assistance_notice_for_run(run.id)?.is_some())
                && !self.conversations_enabled()?
            {
                continue;
            }
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
            if member.furloughed() {
                // Furloughed after its wake queued a turn: there is nothing for it to do.
                self.end(run, RunStatus::Stopped, "its member is on furlough")?;
                continue;
            }
            let downtime = self.reason_for(&run)? == WakeReason::Downtime.name();
            if downtime && member.paused {
                // Paused after its wake queued a turn: downtime is not kept waiting.
                self.end(run, RunStatus::Stopped, "its member was paused")?;
                continue;
            }
            if busy >= usize::from(member.slots) || member.paused {
                continue;
            }
            let project = self.project_of_run(&run)?;
            if self.held_by_owner(project)? {
                continue;
            }
            // What the turn needs is decided here, from why it was asked for (ADR 0131), and
            // never by the member: judgment runs only on the member's Claude, work on the best
            // free work brain it is allowed on. A turn that has nowhere it may run waits where
            // it is, and says why. It is never given another brain; an alarm out of budget is
            // the one exception, and acts at once.
            // A side of a league game is pinned to its brain, whoever plays it, and a Claude
            // side is paid from the league's capped project alone.
            let pinned = self.league_brain_of(run.task)?;
            // A scenario's turn is bounded by its world, not its player's day (ADR 0133, amended).
            let in_a_world = match run.task {
                Some(task) => self.in_a_scenario(task)?,
                None => false,
            };
            let need = match pinned {
                Some(brain) => turns::Need::pinned(brain),
                None => self.need_of_run(&run, &member, &wakes, &known),
            };
            // Only judgment is paid for; work on the house's models reserves nothing.
            let asked = match (need.kind, pinned) {
                (turns::TurnKind::Work, _) => Some(0),
                (turns::TurnKind::Judgment, Some(_)) => match self.project_left(project)? {
                    Some(_) => self.affordable(&member, project, true)?,
                    None => None,
                },
                (turns::TurnKind::Judgment, None) => {
                    self.affordable(&member, project, run.asked_by_owner)?
                }
            };
            let on_the_mac = running.iter().any(|r| r.brain == Some(Brain::Mac));
            let facts = turns::Facts {
                work_brain: member.work_brain,
                judgment_brain: pinned
                    .filter(|b| b.cloud())
                    .unwrap_or_else(|| member.judgment_brain()),
                mac_waits: mac::wait_for(self.mac_facts().as_ref(), now, on_the_mac),
                can_afford: asked.is_some(),
                free_turns_today: if in_a_world {
                    0
                } else {
                    starts.free_turns_of(&run)
                },
            };
            let (brain, amount, fell_back) = match turns::place(need, &facts) {
                turns::Placed::On(brain) => (brain, asked.unwrap_or(0), false),
                turns::Placed::Fallback(brain) => (brain, 0, true),
                turns::Placed::Waits(wait) => {
                    let said = wait.words();
                    if run.end_reason != said {
                        run.end_reason = said;
                        self.store.save_run(&run)?;
                    }
                    continue;
                }
            };
            // The tripwire (ADR 0133): the team has started as many turns as it may in ten
            // minutes, and this would be one more. HQ stops the team itself, as the owner's
            // switch does, and this turn never starts.
            if let Some(ran) = starts.runaway(now) {
                self.trip(&ran)?;
                return Ok(started);
            }
            // The turn is briefed, told how hard to try and answered by the brain it runs on.
            let on = Member {
                brain,
                ..member.clone()
            };
            run.brain = Some(brain);
            run.told = self.briefing(&on, &run)?;
            self.store
                .reserve(run.id, member.id, self.today(), amount, now)?;
            run.status = RunStatus::Running;
            run.attempts = run.attempts.saturating_add(1);
            run.started_at = Some(now);
            run.reserved = amount;
            if turns::is_waiting_words(&run.end_reason) {
                run.end_reason.clear();
            }
            self.store.save_run(&run)?;
            starts.started(member.id, brain, now, in_a_world);
            let context = RunContext {
                run: run.id,
                member: member.id,
                task: run.task,
                told: run.told.clone(),
                token: self.store.issue_token(run.id)?,
                effort: self.effort_of(&on, &run)?,
            };
            if let Err(why) = self.runner.start(&context) {
                self.fail(run, &format!("could not start: {}", why.0), Fault::Worker)?;
                continue;
            }
            self.store
                .set_wake(run.wake, Receipt::Started, Some(run.id))?;
            self.emit(
                EventKind::RunStarted,
                json!({ "run": run.id.0, "member": member.id.0, "task": run.task.map(|t| t.0),
                        "reserved": amount, "reason": self.reason_for(&run)?,
                        "kind": need.kind.label(), "brain": brain.name() }),
            )?;
            if fell_back {
                self.alarm_on_the_house(&run, &member, brain)?;
            }
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
        let mut member = self.member(run.member)?;
        // The turn is answered by the brain it was started on (ADR 0131), so the data tier,
        // the model relays and the tools all see what the turn is, not what its member
        // otherwise has.
        if let Some(brain) = run.brain {
            member.brain = brain;
        }
        Ok(Some((run, member)))
    }

    /// A member asks the owner something about its task: a question on the desk, and the end
    /// of its turn, which can do nothing useful until the owner has seen it. Seeing it wakes
    /// that member again. One question a member about a task waits at a time, so a turn
    /// cannot fill the owner's desk.
    ///
    /// While the team has a CEO at work, nobody's question but the CEO's reaches the desk
    /// (ADR 0111): it goes up the chain instead (see [`chain::escalation`]), posted in the task's
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
        self.ask_owner_citing(run, text, &[])
    }

    /// [`Self::ask_owner`], citing evidence from HQ's record (ADR 0114). What goes to the
    /// owner's desk as a decision, or from the lead up to the CEO, needs it, each reference in
    /// the record; the ids go with the question.
    ///
    /// # Errors
    /// As [`Self::ask_owner`]; [`Refusal::NoEvidenceFor`] or [`Refusal::NoSuchEvidence`].
    pub fn ask_owner_citing(
        &self,
        run: RunId,
        text: &str,
        cited: &[crate::domain::Ref],
    ) -> Done<Asked> {
        let _held = self.held();
        // Room for the question and the worker's line saying what the turn actually did; what
        // could turn text around or hide in it is replaced, since the owner or the member
        // above reads it as it stands (ADR 0129).
        let text = cut(record::scrub(text).trim(), 1200);
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
            return self.to_the_board(turn, &task, &member, &decision, cited);
        }
        // The reviewer passing on the CEO's decision, checked: it reaches the board as the
        // CEO's, as the reviewer wrote it, or as the CEO did if the reviewer's lost its shape.
        if member.role == Role::Reviewer {
            if let Some((ceo, drafted)) = self.ceos_draft(&task)? {
                self.judge_decision(cited, turn.id)?;
                let decision = Decision::read(&text).unwrap_or(drafted);
                let evidence = self.evidence_said(cited)?;
                return self
                    .board_desk(turn, &task, &ceo, &decision, Some(&member), &evidence)
                    .map(Asked::Desk);
            }
        }
        // Up the chain, while the team has a CEO at work; otherwise as before there was one.
        if let Some(above) = self.asked_of(&member, Some(&task))? {
            // The lead sends something up to the CEO only on evidence (ADR 0114).
            if member.role == Role::Lead && above.role == Role::Ceo {
                self.judge_decision(cited, turn.id)?;
            }
            let text = self.with_evidence(&text, cited)?;
            return self.up_the_chain(turn, &task, &member, &above, &text);
        }
        let text = self.with_evidence(&text, cited)?;
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
    pub fn ask_board(
        &self,
        run: RunId,
        decision: &Decision,
        cited: &[crate::domain::Ref],
    ) -> Done<Asked> {
        let _held = self.held();
        let (turn, ceo) = self.the_ceos_turn(run)?;
        let task = self.task(
            turn.task
                .ok_or(EngineError::Unknown("task for this turn"))?,
        )?;
        self.to_the_board(turn, &task, &ceo, decision, cited)
    }

    /// A question with the evidence it cites under it, by id.
    fn with_evidence(&self, text: &str, cited: &[crate::domain::Ref]) -> Done<String> {
        if cited.is_empty() {
            return Ok(text.to_owned());
        }
        Ok(format!("{text}\nEvidence: {}", self.evidence_said(cited)?))
    }

    /// The CEO's decision on its way to the board: only on evidence from HQ's record (ADR
    /// 0114), through the reviewer first.
    fn to_the_board(
        &self,
        run: Run,
        task: &Task,
        ceo: &Member,
        decision: &Decision,
        cited: &[crate::domain::Ref],
    ) -> Done<Asked> {
        self.judge_decision(cited, run.id)?;
        let evidence = self.evidence_said(cited)?;
        let reviewer = self
            .reviewer()?
            .filter(|r| r.at_work() && r.id != ceo.id && !task.status.finished());
        if let Some(reviewer) = reviewer {
            let text = format!("{}\nEvidence: {evidence}", decision.text());
            return self.to_review(run, task, ceo, reviewer.id, &text);
        }
        self.board_desk(run, task, ceo, decision, None, &evidence)
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
        evidence: &str,
    ) -> Done<crate::domain::DeskId> {
        if self.waiting_question(ceo.id, task.id)? {
            return Err(EngineError::Refused(Refusal::AlreadyAsked));
        }
        let mut text = decision.text();
        // What the CEO cited, and what the reviewer did when she checked it.
        let drafted = self
            .store
            .thread(task.id)?
            .into_iter()
            .rev()
            .find(|p| p.text.starts_with(review::DRAFT) && p.author == ceo.name)
            .and_then(|p| {
                p.text
                    .lines()
                    .find_map(|l| l.strip_prefix("Evidence: ").map(str::to_owned))
            });
        if let Some(cited) = drafted.filter(|_| checked_by.is_some()) {
            text.push_str(&format!("\nEvidence: {cited}"));
        }
        if !evidence.is_empty() {
            let whose = if checked_by.is_some() {
                "Checked against"
            } else {
                "Evidence"
            };
            text.push_str(&format!("\n{whose}: {evidence}"));
        }
        if let Some(reviewer) = checked_by {
            text.push_str(&format!("\nChecked by {}, the reviewer.", reviewer.name));
        }
        // The same decision from another task, already waiting, is one card (ADR 0124).
        let desk = self.ask_on_the_desk("decision", task.id, ceo.id, &text)?;
        let id = run.id;
        self.end_blocked(run, RunStatus::Succeeded, "put a decision to the board")?;
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

    /// Who `member` asks up the chain of command, if anyone: whom it reports to, or the next
    /// one up while that one is paused (ADR 0113, [`chain::escalation`]); the CEO at work when
    /// the chain ends before it. Nobody for the CEO, who asks the board, and nobody while the
    /// team has no CEO at work: then the team works as it did before there was one.
    fn above(&self, member: &Member) -> Done<Option<Member>> {
        let members = self.store.members()?;
        let ceo = members
            .iter()
            .filter(|m| m.role == Role::Ceo && m.at_work() && m.id != member.id)
            .min_by_key(|m| m.id)
            .cloned();
        if member.role == Role::Ceo || ceo.is_none() {
            return Ok(None);
        }
        let up = chain::escalation(member, &members)
            .and_then(|id| members.iter().find(|m| m.id == id).cloned());
        Ok(up.or(ceo))
    }

    /// Whom `member` asks about `task` (ADR 0111, amended): the lead of the task's project,
    /// when it is in one and that lead is at work and not the asker; otherwise up its own
    /// chain ([`Self::above`]). Live, the tester asked the lead she reports to about a task of
    /// another lead's project, who knew nothing of it. Nobody while the team has no CEO at work.
    fn asked_of(&self, member: &Member, task: Option<&Task>) -> Done<Option<Member>> {
        let Some(above) = self.above(member)? else {
            return Ok(None);
        };
        let Some(project) = task.and_then(|t| t.project) else {
            return Ok(Some(above));
        };
        let lead = self.project(project)?.lead;
        let lead = self
            .store
            .members()?
            .into_iter()
            .find(|m| m.id == lead && m.at_work() && m.id != member.id);
        Ok(Some(lead.unwrap_or(above)))
    }

    /// Whether a turn is work, on the house's model, rather than judgment on Claude (ADR 0131).
    fn on_work_turn(
        &self,
        turn: &crate::application::ports::Run,
        member: &crate::domain::Member,
    ) -> Done<bool> {
        let reason = self.store.wake(turn.wake)?.map(|w| w.reason);
        Ok(!reason.is_some_and(|r| {
            crate::domain::turns::kind_of(r, member.role)
                == crate::domain::turns::TurnKind::Judgment
        }))
    }

    /// Counts one escalation of `task` a model tier up, if it has not escalated twice today
    /// (ADR 0139: a problem cannot bounce between tiers). Whether it may. Never in a scenario:
    /// the Holodeck measures the model a player plays on.
    pub(super) fn take_a_tier_up(&self, task: TaskId) -> Done<bool> {
        const MOST_A_DAY: u32 = 2;
        if self.in_a_scenario(task)? {
            return Ok(false);
        }
        let key = format!("escalated:{}:{}", task.0, self.today());
        let done: u32 = self
            .store
            .setting(&key)?
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        if done >= MOST_A_DAY {
            return Ok(false);
        }
        self.store.set_setting(&key, &(done + 1).to_string())?;
        Ok(true)
    }

    /// A member's turn on the house's model is stuck: it takes the problem a model tier up, not
    /// up the org chart (ADR 0139). Its turn ends, why goes in the thread as its own words, and
    /// the same member is woken on its Claude. Twice a task a day at most: after that the
    /// problem is a decision for whoever holds it.
    ///
    /// # Errors
    /// [`Refusal::Escalation`] for a turn already on Claude, a turn about no task, an empty
    /// reason, or a task that has escalated twice today; a stopped or unknown turn.
    pub fn escalate_in(&self, run: RunId, why: &str) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        let member = self.member(turn.member)?;
        if !self.on_work_turn(&turn, &member)? {
            return Err(EngineError::Refused(Refusal::Escalation(
                "this turn already thinks with Claude: a decision goes to whoever holds it, with ask_owner",
            )));
        }
        let Some(task) = turn.task else {
            return Err(EngineError::Refused(Refusal::Escalation(
                "escalate takes a task's problem up: this turn is about no task",
            )));
        };
        let why = one_line(why, 600);
        if why.is_empty() {
            return Err(EngineError::Refused(Refusal::Escalation(
                "say why you are stuck: what you tried and what keeps failing",
            )));
        }
        if self.in_a_scenario(task)? {
            return Err(EngineError::Refused(Refusal::Escalation(
                "a scenario measures the model you play on: play it on this one",
            )));
        }
        if !self.take_a_tier_up(task)? {
            return Err(EngineError::Refused(Refusal::Escalation(
                "this task has escalated twice today: put the decision to whoever holds it, with ask_owner",
            )));
        }
        self.post_own_held_lock(
            member.id,
            task,
            &format!("Escalating to my Claude: {why}"),
            &[],
        )?;
        let id = turn.id;
        self.end_blocked(turn, RunStatus::Succeeded, "escalated to its Claude")?;
        self.wake_held_lock(member.id, Some(task), WakeReason::Escalated)?;
        self.runner.stop(id, self.settings.grace);
        Ok(
            "Escalated: you take this up again on your Claude in your next turn, with why in the \
            thread. This turn ends."
                .to_owned(),
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
        self.post_own_held_lock(member.id, task.id, &chain::question(&above.name, text), &[])?;
        let id = run.id;
        self.end_blocked(
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
        self.post_own_held_lock(member.id, task.id, &format!("{}{text}", review::DRAFT), &[])?;
        let id = run.id;
        self.end_blocked(
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
        for d in self.store.desk()? {
            if !matches!(d.kind.as_str(), "question" | "approval" | "decision")
                || d.status != "waiting"
            {
                continue;
            }
            if d.member == Some(member) && d.task == Some(task) {
                return Ok(true);
            }
            // Put with another task's card, the same ask still waits (ADR 0124).
            if self.store.attached(d.id.0)?.contains(&(task, Some(member))) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The node asks the owner's approval for an action a member proposed through Endora's
    /// bridge (ADR 0108): an `approval` on the desk, saying exactly what would run, and the end
    /// of the member's turn, as [`Self::ask_owner`] ends it. It counts as the member's question
    /// about the task: one of either waits at a time. Nothing runs here; the node runs what it
    /// stored once the owner approves, and tells HQ what came of it
    /// ([`Self::approval_outcome`]).
    ///
    /// A request says which of its task's criteria it `serves` (ADR 0124, amended): its number
    /// or its words. One that serves none, or asked on a task with no criteria, is refused with
    /// the reason, the turn goes on, and nothing reaches the desk. The card shows the criterion
    /// it serves.
    ///
    /// # Errors
    /// As [`Self::ask_owner`]; [`Refusal::Guard`] with what the request serves.
    pub fn ask_approval(
        &self,
        run: RunId,
        text: &str,
        serves: Option<&str>,
    ) -> Done<crate::domain::DeskId> {
        let _held = self.held();
        let text = cut(record::scrub(text).trim(), 1200);
        let turn = self.running(run)?;
        let task = turn
            .task
            .ok_or(EngineError::Unknown("task for this turn"))?;
        // Already waiting on the owner: said before asking what this request serves.
        if self.waiting_question(turn.member, task)? {
            return Err(EngineError::Refused(Refusal::AlreadyAsked));
        }
        let (n, served) = self.approval_serves(task, serves)?;
        let desk = self.ask_held_lock(run, "approval", &|_| text.clone())?;
        self.store.set_setting(
            &discipline::approval_serves_key(desk),
            &json!({ "n": n, "text": served }).to_string(),
        )?;
        Ok(desk)
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
        // The same question from another task, already waiting, is one card (ADR 0124). An
        // approval never is: each runs exactly the one action it stored.
        let desk = if kind == "question" {
            self.ask_on_the_desk(kind, task, member.id, &say(&member.name))?
        } else {
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
            desk
        };
        // The worker ends its turn on reading that it has; its container is let go too, so a
        // worker that does not cannot linger.
        let id = run.id;
        self.end_blocked(run, RunStatus::Succeeded, "asked the owner")?;
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
        let _asked = self.for_owner();
        let found = self.waiting_on_desk(item)?;
        if matches!(found.kind.as_str(), "project_cap" | "project_budget") {
            return self.owner_decides_cap(&found, decision, "");
        }
        if found.kind == "out_for_day" {
            return self.owner_decides_the_day(&found, decision, "");
        }
        self.decide_held_lock(&found, decision)?;
        self.answer_the_same_asks(item, decision, "")
    }

    /// The owner answers a member's question: the answer goes in the task's thread, as the
    /// owner's, and then the question is seen, which wakes the member who asked to read it.
    ///
    /// # Errors
    /// As [`Self::decide`]; [`EngineError::Unknown`] for an item that is not a question or an
    /// answer with no words.
    pub fn answer(&self, item: crate::domain::DeskId, text: &str) -> Done<()> {
        let _held = self.held();
        let _asked = self.for_owner();
        let found = self.waiting_on_desk(item)?;
        let text = text.trim();
        if found.kind != "question" || text.is_empty() {
            return Err(EngineError::Unknown("answer for this item"));
        }
        let task = found.task.ok_or(EngineError::Unknown("task"))?;
        self.post_held_lock(Author::Owner, task, text)?;
        self.decide_held_lock(&found, "seen")?;
        // One answer to the same ask from other tasks reaches each of them (ADR 0124).
        self.answer_the_same_asks(item, "seen", text)
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
        let _asked = self.for_owner();
        let found = self.waiting_on_desk(item)?;
        let words = cut(words.trim(), 4000);
        if found.kind == "decision" {
            self.board_decides(&found, decision, &words)?;
            // The board's one word on the same decision from other tasks (ADR 0124).
            return self.answer_the_same_asks(item, decision, &words);
        }
        if matches!(found.kind.as_str(), "project_cap" | "project_budget") {
            return self.owner_decides_cap(&found, decision, &words);
        }
        if found.kind == "out_for_day" {
            return self.owner_decides_the_day(&found, decision, &words);
        }
        if decision == "seen" && !words.is_empty() && found.kind == "question" {
            let task = found.task.ok_or(EngineError::Unknown("task"))?;
            self.post_held_lock(Author::Owner, task, &words)?;
        }
        self.decide_held_lock(&found, decision)?;
        self.answer_the_same_asks(item, decision, &words)
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
                // Tried again means someone is woken. If nobody can be, say so and change
                // nothing: the item stays on the desk, rather than the button doing nothing
                // with no word about it.
                let member = to.ok_or(EngineError::Refused(Refusal::NobodyHasIt))?;
                let who = self.member(member)?;
                let asked = self.situation(&who, Some(task.id), WakeReason::ProposalDecided)?;
                if let WakeDecision::Refuse(why) = wakes::decide(member, &asked) {
                    return Err(EngineError::Refused(Refusal::NotWoken(why)));
                }
                self.store.release(task.id)?;
                self.store.set_task_status(task.id, TaskStatus::Todo)?;
                self.store.set_nudged(task.id, false)?;
                self.wake_held_lock(member, Some(task.id), WakeReason::ProposalDecided)?;
            }
            ("stuck", "drop") => {
                let task = self.task(found.task.ok_or(EngineError::Unknown("task"))?)?;
                self.store.set_task_status(task.id, TaskStatus::Dropped)?;
                // Stuck work the owner lets go of cannot go on (ADR 0124).
                self.closed(
                    task.id,
                    crate::domain::closure::Closure::Dropped(
                        crate::domain::closure::DropWhy::BlockedForever,
                    ),
                    None,
                    "owner",
                )?;
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
            // Work sent back three times is the owner's to advance (ADR 0114).
            ("rounds", "accept") => {
                let task = self.task(found.task.ok_or(EngineError::Unknown("task"))?)?;
                if task.status == TaskStatus::InReview {
                    self.post_held_lock(Author::Owner, task.id, "The owner accepts this as done.")?;
                    self.store.set_task_status(task.id, TaskStatus::Done)?;
                    self.emit_status(&task, TaskStatus::Done)?;
                    self.after_finishing(&task)?;
                }
            }
            ("rounds", "send_back") => {
                let task = self.task(found.task.ok_or(EngineError::Unknown("task"))?)?;
                if task.status == TaskStatus::InReview {
                    self.store.set_task_status(task.id, TaskStatus::Doing)?;
                    self.post_held_lock(
                        Author::Owner,
                        task.id,
                        "The owner sends this back: put it right and submit it again.",
                    )?;
                    self.emit_status(&task, TaskStatus::Doing)?;
                    if let Some(member) = task.owner.or(task.assignee) {
                        self.wake_held_lock(member, Some(task.id), WakeReason::SentBack)?;
                    }
                }
            }
            // A piece proposed for canon (ADR 0122): only the owner makes it so.
            ("canon", "approve" | "decline") => self.decide_canon(found, decision == "approve")?,
            // The wing (ADR 0115): a lead's request to hire, and a branch to push.
            ("hire", "approve" | "decline") => self.decide_hire(found, decision == "approve")?,
            ("push", "approve" | "decline") => self.decide_push(found, decision == "approve")?,
            // The check after a deploy (ADR 0115, amended): the owner's word that it is deployed.
            ("deploy", "deployed" | "not_deploying") => {
                self.decide_deploy(found, decision == "deployed")?;
            }
            // Elastic staffing (ADR 0118): furlough an idle member or keep it, and bring a
            // furloughed one back for a lead with a backlog.
            ("furlough", "approve" | "decline") => {
                self.decide_furlough(found, decision == "approve")?;
            }
            ("recall", "approve" | "decline") => {
                self.decide_recall(found, decision == "approve")?;
            }
            // A guard's notice (ADR 0124): the owner's verdict on its finding.
            ("guard", "real" | "false_positive" | "not_enough" | "resolved" | "dismiss") => {
                self.desk_verdict(item, decision)?;
            }
            // The runaway tripwire's card (ADR 0133): resume the team, which answers the card
            // itself, or leave it stopped and have the card seen.
            ("tripwire", "resume") => return self.resume_held_lock(),
            ("tripwire", "seen" | "dismiss") => {}
            ("push", "seen" | "dismiss")
            | ("ready_to_merge", "merged" | "not_now")
            | ("ready_to_read", "read" | "not_now")
            | ("question", "dismiss")
            | ("notice", "seen" | "dismiss")
            | ("deploy_check", "seen" | "dismiss") => {}
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
                || !self.store.held_wakes(task.id)?.is_empty()
                || self.in_a_scenario(task.id)?
            {
                continue;
            }
            let Some(member) = task.owner.or(task.assignee) else {
                continue;
            };
            if members.iter().any(|m| m.id == member && !m.at_work()) {
                continue;
            }
            let on_the_desk = desk
                .iter()
                .any(|d| d.kind == "stuck" && d.status == "waiting" && d.task == Some(task.id));
            if on_the_desk {
                // Still stuck, and in the digest already: its finding is seen again, and not
                // raised again (ADR 0132).
                if let Some(found) = self
                    .diagnosis(task.id)?
                    .filter(|d| d.stopped == Stopped::Parked)
                {
                    self.found_stuck(task, found.why)?;
                }
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
            // Still waiting on work that will happen (what it waits for, or its open sub-tasks),
            // or with a check-back or an answer to come: held on purpose, and what it waits for
            // is its wake (ADR 0114, 0124). Waiting on work that was dropped: parked, and its
            // member is told why. What HQ cannot tell about, its lead looks at once.
            let Some(found) = self.diagnosis(task.id)? else {
                continue;
            };
            match found.stopped {
                Stopped::Working | Stopped::Held => continue,
                Stopped::Indeterminate => {
                    self.look_once(task, found)?;
                    continue;
                }
                Stopped::Parked => {}
            }
            let dropped = self.dropped_wait(&task.waits_on)?;
            if !task.nudged {
                self.store.set_nudged(task.id, true)?;
                if let Some(gone) = &dropped {
                    self.post_held_lock(
                        Author::Endora,
                        task.id,
                        &format!(
                            "#{} ({}), which this waited on, was dropped: decide whether this \
                             still needs doing, and say so here, or ask.",
                            gone.id, gone.title
                        ),
                    )?;
                }
                if task.status == TaskStatus::InReview {
                    nudged += 1;
                    self.review_asked(task)?;
                    continue;
                }
                // Recorded before it is sent, and never sent again unchanged (ADR 0124).
                if self.nudge_once(task, member, WakeReason::AssignmentRecovery)? {
                    nudged += 1;
                }
                continue;
            }
            // Work in review is never put out of the tester's reach (task 670: blocked 44
            // minutes after it entered review, with the review not yet done; only its holder
            // could put it back, and it climbed to the CEO five times). It stays in review,
            // and its lead is woken to decide it.
            let in_review = task.status == TaskStatus::InReview;
            if task.status != TaskStatus::Blocked && !in_review {
                self.store.set_task_status(task.id, TaskStatus::Blocked)?;
                self.emit(
                    EventKind::TaskChanged,
                    json!({ "task": task.id.0, "goal": task.goal.0, "owner": task.owner.map(|o| o.0),
                            "status": TaskStatus::Blocked.name(), "priority": task.priority.name() }),
                )?;
            }
            stuck += 1;
            self.found_stuck(task, found.why)?;
            // Stuck work climbs the ladder (ADR 0120); the owner's desk only with no lead.
            let (why, on_the_desk) = if in_review {
                (
                    "it waits in review, its tester was woken once, and no verdict came",
                    "It waits in review, its tester was woken once, and no verdict came.",
                )
            } else {
                (
                    "it was handed on, woken once, and nobody came back to it",
                    "It was handed on, woken once, and nobody came back to it.",
                )
            };
            if !self.climb_stuck(task, why)? {
                self.stuck_on_the_desk(task, on_the_desk)?;
            }
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
        let text = cut(text.trim(), 4000);
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
        let text = cut(text.trim(), 600);
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
        self.drop_projects_of(goal)?;
        let open: Vec<Task> = self
            .store
            .tasks()?
            .into_iter()
            .filter(|t| t.goal == goal && !t.status.finished())
            .collect();
        self.put_away(&open, &|item| item.goal == Some(goal), why)?;
        Ok(open.len())
    }

    // ---- routines, alarms and check-backs (ADR 0107) -------------------------------------

    /// The routine whose last task this is, if any: the one task a routine has open.
    /// Whether a task is a check its member closes itself: a routine's, or the check after a
    /// deploy (ADR 0107; 0115, amended). A check changes no code.
    fn is_a_check(&self, task: TaskId) -> Done<bool> {
        Ok(self.routine_of(task)?.is_some() || self.deploy_check_of(task)?.is_some())
    }

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
        if let Some(project) = routine.project {
            let found = self.project(project)?;
            crate::domain::projects::may_hold(found.goal, found.status, routine.goal)
                .map_err(EngineError::Refused)?;
        }
        let routine = NewRoutine { title, ..routine };
        let next = routines::next_due(self.clock.now(), routine.every_ms);
        let id = self.store.add_routine(&routine, next)?;
        // Who wrote its words, for the tasks it fires (ADR 0138 A2b): the owner's, or HQ's own
        // template. A member's stays unknown.
        let origin = match by {
            Author::Owner => Some("owner"),
            Author::Endora => Some("system"),
            Author::Member(_) => None,
        };
        if let Some(origin) = origin {
            self.store
                .set_setting(&format!("{ROUTINE_ORIGIN}{}", id.0), origin)?;
        }
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

    /// The owner moves a routine into a project under its goal, or out of one (`None`). What
    /// it fires from then on is the project's, with its help scope, tools and budget (D2,
    /// 2026-10-25: a lead's standing reviews belong to the lead's area project).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a routine or project that does not exist; what
    /// `projects::may_hold` refuses for a project under another goal or not active.
    pub fn set_routine_project(&self, id: RoutineId, project: Option<ProjectId>) -> Done<()> {
        let _held = self.held();
        let mut routine = self.routine(id)?;
        if let Some(project) = project {
            let found = self.project(project)?;
            crate::domain::projects::may_hold(found.goal, found.status, routine.goal)
                .map_err(EngineError::Refused)?;
        }
        routine.project = project;
        self.store.save_routine(&routine)?;
        self.emit(
            EventKind::RoutineChanged,
            json!({ "routine": id.0, "title": routine.title, "project": project.map(|p| p.0) }),
        )
    }

    /// The owner runs a routine now, whatever its rhythm. Its next time is unchanged.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a routine that does not exist; a store failure.
    pub fn fire_routine(&self, id: RoutineId) -> Done<Fired> {
        let _held = self.held();
        let _asked = self.for_owner();
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

    /// Endora's word that something in the home lab that alarmed is back up, in its words:
    /// said in the open task of the first routine that is on and takes the home lab's alarms,
    /// as Endora's post, waking whoever has it as an alarm does. An alarm is a moment; this
    /// tells the member that the moment passed, so it looks at the state now rather than
    /// diagnosing what already recovered. `None`, and nothing done, when no routine takes the
    /// alarms or its task is not open: a recovery hands out no work.
    ///
    /// # Errors
    /// A store failure.
    pub fn recovered(&self, said: &str) -> Done<Option<TaskId>> {
        let _held = self.held();
        let said = one_line(said, 300);
        if said.is_empty() {
            return Err(EngineError::Unknown("recovery text"));
        }
        let Some(routine) = self
            .store
            .routines()?
            .into_iter()
            .find(|r| r.enabled && r.on_lab_trouble)
        else {
            return Ok(None);
        };
        let open = match routine.last_task {
            Some(task) => Some(self.task(task)?).filter(|t| !t.status.finished()),
            None => None,
        };
        let Some(task) = open else {
            return Ok(None);
        };
        self.post_held_lock(Author::Endora, task.id, &format!("Recovered: {said}"))?;
        let to = self.alarm_to(&task, routine.assignee)?;
        self.wake_held_lock(to, Some(task.id), WakeReason::Alarm)?;
        self.emit(
            EventKind::RoutinePosted,
            json!({ "routine": routine.id.0, "title": routine.title, "task": task.id.0,
                    "said": said, "recovered": true }),
        )?;
        Ok(Some(task.id))
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
            let to = self.alarm_to(&task, routine.assignee)?;
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
        // The routine's words and HQ's template carry the routine's origin; an alarm's words
        // come from outside (ADR 0138 A2b).
        let origin = match firing {
            Firing::Alarm(_) => InputOrigin::Unknown,
            _ => self.routine_origin(routine.id)?,
        };
        let task = self.add_task_authored_held_lock(
            Author::Endora,
            NewTask {
                goal: routine.goal,
                project: routine.project,
                parent: None,
                title: routine.title.clone(),
                description,
                assignee: Some(routine.assignee),
                priority,
                waits_on: Vec::new(),
            },
            origin,
        )?;
        // A lead's report or patterns turn is not work on the board (ADR 0120).
        let report = self.report_kind(routine.id, routine.assignee)?;
        if let Some(kind) = report {
            self.store.mark_report_task(task, routine.assignee, kind)?;
        }
        // What done means for it (ADR 0114, 0124): the home lab's check is the lab as it is now,
        // a standing review (a lead's report and patterns, the CEO's review and planning) is
        // closed by filing or deciding it and has none, and any other routine's is a reading of
        // what it is about, cited.
        let standing_review = report.is_some() || self.member(routine.assignee)?.role == Role::Ceo;
        let criteria =
            routines::criteria_for(routine.on_lab_trouble, standing_review, &routine.title);
        if !criteria.is_empty() {
            self.store.set_criteria_from(task, &criteria, origin)?;
        }
        // An alarm made the task: the member is woken for it as it is for an alarm passed into
        // an open task (ADR 0131). The wake joins the turn the task's making queued, and makes
        // it the emergency it is.
        if matches!(firing, Firing::Alarm(_)) {
            self.wake_held_lock(routine.assignee, Some(task), WakeReason::Alarm)?;
        }
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

    /// Routines made before their origin was recorded (ADR 0138 A2b) whose words are exactly
    /// one of HQ's own templates (`title`, `description`): HQ's planning and leads' area
    /// routines, and any the composition root names. They are recorded as HQ's; every other
    /// stays unknown. Returns how many were.
    ///
    /// # Errors
    /// A store failure.
    pub fn vouch_template_routines(&self, also: &[(&str, &str)]) -> Done<usize> {
        let _held = self.held();
        let ours = [
            ("Planning", super::engine::planning::PLANNING),
            (
                crate::domain::areas::REPORT_ROUTINE,
                super::engine::areas::REPORT_ASKS,
            ),
            (
                crate::domain::areas::PATTERNS_ROUTINE,
                super::engine::areas::PATTERNS_ASKS,
            ),
        ];
        let mut vouched = 0;
        for routine in self.store.routines()? {
            let key = format!("{ROUTINE_ORIGIN}{}", routine.id.0);
            if self
                .store
                .setting(&key)?
                .is_some_and(|kept| !kept.is_empty())
            {
                continue;
            }
            let template = ours.iter().chain(also.iter()).any(|(title, words)| {
                routine.title == *title && routine.description.trim() == words.trim()
            });
            if template {
                self.store.set_setting(&key, "system")?;
                vouched += 1;
            }
        }
        Ok(vouched)
    }

    /// A task's whole thread, each post with its recorded origin, in pages read with their
    /// origins.
    fn all_authored_posts(
        &self,
        task: TaskId,
    ) -> Done<Vec<crate::application::history_ports::PostInput>> {
        const PAGE: usize = 50;
        let mut all = Vec::new();
        let mut after = 0;
        loop {
            let page = self.store.authored_posts(task, after, PAGE)?;
            let full = page.len() == PAGE;
            if let Some(last) = page.last() {
                after = last.id;
            }
            all.extend(page);
            if !full {
                return Ok(all);
            }
        }
    }

    /// Who wrote a routine's words, as recorded when it was made: unknown for one made before
    /// that was recorded, or by a member.
    fn routine_origin(&self, id: RoutineId) -> Done<InputOrigin> {
        let kept = self.store.setting(&format!("{ROUTINE_ORIGIN}{}", id.0))?;
        Ok(match kept.as_deref() {
            Some("owner") => InputOrigin::Owner,
            Some("system") => InputOrigin::System,
            _ => InputOrigin::Unknown,
        })
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
        if member.furloughed() {
            return Ok(Some(format!("{} is on furlough", member.name)));
        }
        if self.held_by_owner(routine.project)? {
            return Ok(Some("its project is paused".to_owned()));
        }
        // Money is not a reason to skip a firing (ADR 0131): what a firing makes is work, which
        // costs nothing, or a decision, which waits for budget; and an alarm never waits.
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

    /// A turn of the lead's or the reviewer's notes a lesson the team will need again. Since
    /// stories (ADR 0122) it is kept as a story marked to retell, under a handle from its first
    /// words, in its writer's area, and no longer in the goal's notes; `tell_story` is the tool
    /// members are offered now, and this stays for a worker told of it before. Each is an event
    /// in the feed and a notice on the owner's desk. Returns the story's handle.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`] for a turn that has ended; [`Refusal::NotALessonKeeper`] for a
    /// member that is neither; [`EngineError::Unknown`] for a turn with no task, or no lesson;
    /// [`Refusal::TooLong`] for a lesson past [`lessons::LESSON_MOST`].
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
        let handle = self.lesson_as_story(&run, &member, lesson)?;
        let said = lesson.split_whitespace().collect::<Vec<_>>().join(" ");
        self.emit(
            EventKind::LessonNoted,
            json!({ "goal": goal.0, "task": task.0, "member": member.id.0, "lesson": said,
                    "story": handle }),
        )?;
        let text = format!("{} noted: {said}", member.name);
        let desk = self
            .store
            .add_desk("notice", Some(task), None, Some(member.id), &text, now)?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": "notice", "task": task.0 }),
        )?;
        Ok(handle)
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
        let every_run = self.store.runs()?;
        // A lead's report and patterns turns are not work on the board (ADR 0120).
        let reports = self.report_task_ids()?;
        let tasks: Vec<Task> = self
            .store
            .tasks()?
            .into_iter()
            .filter(|t| !reports.contains(&t.id))
            .collect();
        // Downtime is not work: its turns are not on the scorecard (ADR 0119).
        let downtime = self.downtime_wakes()?;
        let worked: Vec<Run> = every_run
            .iter()
            .filter(|r| !downtime.contains(&r.wake))
            .cloned()
            .collect();
        let scorecard = self.scorecard(&worked, &tasks, day)?;
        let runs: Vec<Run> = every_run.into_iter().filter(|r| r.status.live()).collect();
        let goals = self.store.goals()?;
        let mut notes = std::collections::HashMap::new();
        let mut strategies = std::collections::HashMap::new();
        for g in &goals {
            notes.insert(g.id.0, self.store.goal_note(g.id)?);
            let s = self.strategy_of(g.id)?;
            strategies.insert(
                g.id.0,
                json!({ "purpose": s.purpose, "success": s.success, "measures": s.measures,
                        "bets": s.bets, "not_now": s.not_now, "by": s.by,
                        "updated_at": (s.updated_at > 0).then_some(s.updated_at) }),
            );
        }
        let mut members = Vec::new();
        let paid = self.paid_weights()?;
        let pool_day = self.pool_day_today()?;
        let topped_up = self.topup_of(day)?;
        let all_members = self.store.members()?;
        let rescues = self.rescues()?;
        for m in all_members.clone() {
            let committed = self.store.committed(Some(m.id), day)?;
            let spent = self.store.spent(Some(m.id), day)?;
            members.push(json!({
                "id": m.id.0, "name": m.name, "role": m.role.name(), "look": m.look,
                "paused": m.paused, "slots": m.slots,
                // On furlough (ADR 0118): when it began, and when it ended if it has.
                "furloughed": m.furloughed(), "furloughed_at": m.furloughed_at,
                "recalled_at": m.recalled_at,
                // Whom it reports to (ADR 0113): null for the CEO, who reports to the board.
                "reports_to": chain::manager(&m, &all_members).map(|r| r.0),
                // What it thinks with, and what the owner may choose for its role (ADR 0109).
                "brain": m.brain.name(),
                "brains": Brain::choices(m.role).into_iter()
                    .map(|b| json!({ "name": b.name(), "label": b.label() }))
                    .collect::<Vec<_>>(),
                // Two brains (ADR 0131): what it does its work with (the Mac or the house's
                // model), and the Claude it judges with, each with what the owner may choose.
                "work_brain": m.work_brain.name(), "work_brain_label": m.work_brain.label(),
                "work_brains": Brain::work_choices().into_iter()
                    .map(|b| json!({ "name": b.name(), "label": b.label() }))
                    .collect::<Vec<_>>(),
                "judgment_brain": m.judgment_brain().name(),
                "judgment_brain_label": m.judgment_brain().label(),
                "judgment_brains": Brain::judgment_choices(m.role).into_iter()
                    .map(|b| json!({ "name": b.name(), "label": b.label() }))
                    .collect::<Vec<_>>(),
                // What runs its turns (ADR 0117), and whether Claude Code may.
                "runtime": self.store.runtime_of(m.id)?.effective(m.role, m.brain).name(),
                "claude_code": crate::domain::Runtime::ClaudeCode.allowed_for(m.role, m.brain),
                // Only its judgment is paid: the pool is shared by the members' claims.
                "per_day": self.per_day(&m, paid, pool_day),
                "committed": committed, "spent": spent, "held": (committed - spent).max(0),
                "running": runs.iter().filter(|r| r.member == m.id && r.status == RunStatus::Running)
                    .map(|r| json!({ "run": r.id.0, "task": r.task.map(|t| t.0) }))
                    .collect::<Vec<_>>(),
                // How often the CEO reached into its work this week (ADR 0124); null when the
                // pruned feed no longer holds the week, and HQ cannot tell.
                "rescues": rescues.as_ref().map(|r| r.get(&m.id.0).copied().unwrap_or(0)),
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
            // Finished work carries only the posts its card shows: whole threads of hundreds
            // of done tasks made the snapshot over a megabyte, on every poll. Each post keeps
            // its place in the whole thread, which the activity log finds it by.
            let thread = self.store.thread(t.id)?;
            let posts = thread.len();
            let from = if t.status.finished() {
                posts.saturating_sub(THREAD_TAIL)
            } else {
                0
            };
            let (documents, branches) = self.documents_shown(t.id)?;
            task_rows.push(json!({
                "id": t.id.0, "goal": t.goal.0, "project": t.project.map(|p| p.0),
                "parent": t.parent.map(|p| p.0), "title": t.title,
                "description": t.description, "owner": t.owner.map(|o| o.0),
                "assignee": t.assignee.map(|a| a.0), "status": t.status.name(),
                "priority": t.priority.name(), "depth": t.depth,
                "waits_on": t.waits_on.iter().map(|w| w.0).collect::<Vec<_>>(),
                "gate": self.store.gate(t.id)?, "why": why(t),
                "routine": routine_list.iter().find(|r| r.last_task == Some(t.id)).map(|r| r.id.0),
                "posts": posts,
                "thread": thread.iter().enumerate().skip(from)
                    .map(|(n, p)| json!({ "n": n, "author": p.author, "text": p.text,
                                          "at": p.at, "unsupported": p.unsupported }))
                    .collect::<Vec<_>>(),
                // What done means, and what was cited for it at each stage (ADR 0114).
                "evidence": self.evidence_shown(t)?,
                // The wing's documents and branches on it (ADR 0115).
                "documents": documents,
                "branches": branches,
                // Why it closed, and what carries it on (ADR 0124).
                "closure": self.store.closure(t.id)?.map(|c| json!({
                    "why": c.closure.name(), "said": c.closure.plainly(),
                    "duplicate_of": c.closure.duplicate_of().map(|d| d.0),
                    "successor": c.successor.map(|s| s.0), "by": c.by, "at": c.at })),
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
        let stopped = self.store.stopped()?;
        // Who stopped the team, while it is stopped: the owner, or the runaway tripwire (ADR
        // 0133).
        let stopped_by = if stopped {
            self.store
                .setting(runaway::STOPPED_BY)?
                .filter(|by| !by.is_empty())
        } else {
            None
        };
        Ok(json!({
            "stopped": stopped,
            "stopped_by": stopped_by,
            "last_event": self.store.last_event()?,
            "counts": { "ticks": ticks, "running": running, "queued": queued, "held": held,
                        "folded": folded, "open_tasks": tasks.iter().filter(|t| !t.status.finished()).count() },
            "last_tick_at": last_tick_at,
            "pool": { "per_day": pool_day, "base_per_day": money::pool_a_day(pool),
                      "topped_up": topped_up, "committed": pool_committed,
                      "spent": pool_spent, "held": (pool_committed - pool_spent).max(0),
                      // Turns of judgment that wait for budget (ADR 0131): none is dropped.
                      "waiting": runs.iter().filter(|r| r.status == RunStatus::Queued
                          && turns::is_waiting_for_budget(&r.end_reason)).count(),
                      // Turns of work that wait for the day to turn, their member having taken
                      // its day's turns on the house's own models (ADR 0133).
                      "capped": runs.iter().filter(|r| r.status == RunStatus::Queued
                          && turns::is_waiting_for_cap(&r.end_reason)).count() },
            "members": members,
            "goals": goals.iter().map(|g| json!({
                "id": g.id.0, "title": g.title, "reason": g.reason, "status": g.status.name(),
                "parent": g.parent.map(|p| p.0), "note": notes.get(&g.id.0).cloned().unwrap_or_default(),
                // The CEO's strategy for it (ADR 0120).
                "strategy": strategies.get(&g.id.0).cloned().unwrap_or_default(),
            })).collect::<Vec<_>>(),
            // Each lead's latest report on its area (ADR 0120).
            "area_reports": self.reports_shown()?,
            "tasks": task_rows,
            "runs": runs.iter().map(|r| json!({
                "id": r.id.0, "member": r.member.0, "task": r.task.map(|t| t.0),
                "status": r.status.name(), "started_at": r.started_at, "reserved": r.reserved,
                "spent": r.spent,
                // Why its last attempt ended, for one waiting to be tried again.
                "why": r.end_reason,
                // The brain it runs or ran on (ADR 0131), once started.
                "brain": r.brain.map(Brain::name),
                "input_tokens": r.input_tokens + r.cache_read_tokens + r.cache_write_tokens,
                "output_tokens": r.output_tokens,
            })).collect::<Vec<_>>(),
            "routines": routine_list.iter().map(|r| json!({
                "id": r.id.0, "title": r.title, "description": r.description, "goal": r.goal.0,
                "project": r.project.map(|p| p.0),
                "assignee": r.assignee.0, "every_ms": r.every_ms,
                "rhythm": routines::rhythm(r.every_ms), "next_due_at": r.next_due_at,
                "enabled": r.enabled, "on_lab_trouble": r.on_lab_trouble,
                "last_fired_at": r.last_fired_at, "last_task": r.last_task.map(|t| t.0),
                "last_task_open": r.last_task
                    .and_then(|id| tasks.iter().find(|t| t.id == id))
                    .is_some_and(|t| !t.status.finished()),
            })).collect::<Vec<_>>(),
            // What waits on the owner and needs their word (ADR 0132); the rest waits below.
            "desk": self.desk_shown()?,
            // "While you were away": stuck work, findings and notices, newest first (ADR 0132).
            "digest": self.digest_shown()?,
            "house_log": self.store.setting(HOUSE_LOG)?.unwrap_or_default(),
            // The team's world (ADR 0122): its pages by stage, and its stories.
            "world": self.world_shown()?,
            "projects": self.projects_shown(&tasks)?,
            // The turns that ended last, with their labels (ADR 0114).
            "recent_runs": self.recent_runs(30)?,
            // How the team is doing: today and the last seven days, from the record alone.
            "scorecard": scorecard,
            // The engineering wing (ADR 0115): who sits in it, and its seats.
            "wing": self.wing_shown()?,
            // The office as data (ADR 0119): what stands on the floor, and what the team is
            // deciding about it.
            "office": self.office_shown()?,
            // Ping-pong, pool and chess in the office (ADR 0121).
            "games": self.games_shown()?,
            // The Holodeck's scenarios and trials (ADR 0135).
            "holodeck": self.holodeck_shown()?,
            // The guards (ADR 0124): their thresholds, precision, ceremony and findings.
            "guards": self.guards_shown()?,
            // The owner's Mac as the team sees it (ADR 0130).
            "mac": self.mac_shown(&runs, &all_members),
        }))
    }

    /// The owner's Mac as the team's screen reads it (ADR 0130): whether a turn may start on it
    /// now and why not, how many turns wait for it, and whether one runs.
    fn mac_shown(&self, live: &[Run], members: &[Member]) -> serde_json::Value {
        // A turn on the Mac is one that was started there; one that waits for it says so.
        let on_the_mac = |run: &Run| match run.brain {
            Some(brain) => brain == Brain::Mac,
            None => members
                .iter()
                .any(|m| m.id == run.member && m.brain == Brain::Mac),
        };
        let running = live
            .iter()
            .filter(|r| r.status == RunStatus::Running && on_the_mac(r))
            .count();
        let waiting = live
            .iter()
            .filter(|r| r.status == RunStatus::Queued && mac::is_waiting_words(&r.end_reason))
            .count();
        let why = mac::wait_for(self.mac_facts().as_ref(), self.clock.now(), false);
        json!({ "ready": why.is_none(), "why": why.unwrap_or_else(|| "ready".to_owned()),
                "running": running, "waiting": waiting })
    }

    /// The scorecard ("How the team is doing"), for today and the last seven days by the
    /// owner's day boundaries, over every run, task, desk item and event the store keeps. The
    /// feed is pruned by the node, so each window says where the record starts when that is
    /// later than the window does.
    fn scorecard(&self, runs: &[Run], tasks: &[Task], day: i64) -> Done<serde_json::Value> {
        let desk = self.store.desk()?;
        // A lead's report and patterns turns are not work on the scorecard (ADR 0120).
        let reports = self.report_task_ids()?;
        let events: Vec<crate::domain::Event> = self
            .store
            .events_after(0)?
            .into_iter()
            .filter(|e| {
                !e.data["task"]
                    .as_i64()
                    .is_some_and(|t| reports.contains(&TaskId(t)))
            })
            .collect();
        let record = scorecard::Record {
            runs,
            tasks,
            desk: &desk,
            events: &events,
        };
        let offset = self.settings.offset_minutes;
        Ok(json!({
            "today": scorecard::score(&record, &scorecard::Window::day(day, offset)).json(),
            "week": scorecard::score(&record, &scorecard::Window::days_ending(day, 7, offset)).json(),
            // What the work came to, over the last day and week: landed changes, what one cost,
            // talk against work, interruptions and what waits (ADR 0132).
            "outcomes": self.outcomes_shown(runs, &events)?,
            // Today's pool: the base day and the owner's top-up (ADR 0113 rule 6).
            "pool": { "base": money::pool_a_day(self.settings.pool_per_month),
                      "topped_up": self.topup_of(day)? },
            // Each team's seats, at work and on furlough (ADR 0118).
            "teams": self.teams_shown()?,
            // Each area's measures, today and the last seven days (ADR 0120).
            "areas": self.areas_shown()?,
        }))
    }

    /// Keeps the newest `keep` events that are not ticks, and the ticks among the newest `keep`
    /// of all, and deletes the rest. Returns how many went.
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

    /// The `limit` events just before an id (at most [`MOST_EVENTS_BEFORE`]), oldest first:
    /// how the activity log goes further back than what it was opened with.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn events_before(&self, id: i64, limit: usize) -> Done<Vec<crate::domain::Event>> {
        Ok(self
            .store
            .events_before(id, limit.min(MOST_EVENTS_BEFORE))?)
    }

    /// Every wake, with its receipt.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails.
    pub fn wakes(&self) -> Done<Vec<Wake>> {
        Ok(self.store.wakes()?)
    }
}
