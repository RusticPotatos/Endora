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
use crate::domain::cut::cut;
use crate::domain::docs;
use crate::domain::guards::{GuardRefusal, Stopped};
use crate::domain::history::InputOrigin;
use crate::domain::mac::{self, MacFacts};
use crate::domain::money::{self, Micros};
use crate::domain::record;
use crate::domain::review::{self, SendBackReason};
use crate::domain::routines::{self, FireStep, MINUTE_MS};
use crate::domain::runs::{self, LONGEST_TURN_MS};
use crate::domain::turns;
use crate::domain::wakes::{self};
use crate::domain::work::{self, mentions};
use crate::domain::{
    Brain, CheckBackStatus, EventKind, Fault, Firing, GoalId, GoalStatus, Liveness, Member,
    MemberId, Millis, Priority, ProjectId, Receipt, Refusal, Routine, RoutineId, RunId, RunStatus,
    Task, TaskId, TaskStatus, WakeDecision, WakeReason,
};

mod allowance;
mod areas;
mod days;
mod delivery;
mod digest;
mod discipline;
mod drain;
mod evidence;
mod games;
mod gaps;
mod guards;
mod handover;
mod health;
mod history;
mod lab_tools;
mod league;
mod lessons;
mod lineage;
mod live_work;
mod long_waits;
mod loops;
mod map;
mod mechanical;
mod memory;
mod office;
mod outcomes;
mod projects;
mod record_search;
mod replay;
mod runaway;
mod running;
mod runtime;
mod scenario;
mod team_map;
mod waking;
mod wing;
mod world;

pub use delivery::{Delivered, Deliveries, Due};
pub use evidence::Called;
pub use history::TranscriptKept;
pub use map::MapMigrated;
pub use runtime::ModelSet;
pub use wing::{DOCUMENT_PART, KeptBranch, Recorded, Submission};

/// How long open, assigned work may sit with nothing queued, running or held for it before its
/// member is woken once, and again before it goes on the owner's desk as stuck.
pub const STRANDED_AFTER_MS: Millis = 10 * 60 * 1000;

/// The most characters a goal's notes from the owner may run to.
pub const GOAL_NOTE_MOST: usize = 2_000;
/// The most events one read of the past returns.
pub const MOST_EVENTS_BEFORE: usize = 200;

/// How long an approval waits for the owner before it goes unrun (ADR 0108): a day.
pub const APPROVALS_LAST_MS: Millis = 24 * 60 * 60 * 1000;

/// Where HQ keeps the house log (ADR 0111).
const HOUSE_LOG: &str = "house_log";
/// Where HQ keeps who wrote a routine's words: `owner` or `system` (ADR 0138 A2b).
const ROUTINE_ORIGIN: &str = "routine_origin:";

/// Every member's briefing but the CEO's names the team's standards once, after who it is; the
/// CEO's `read_doc` names them (ADR 0116).
const STANDARDS: &str = "The team's standards: read_doc docs/standards.md\n";

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
    /// Asks the service for a pass of the tick now (ADR 0102, amended 2026-11-04): called once
    /// the owner's new work is in the store, so it is taken up at once instead of at the next
    /// tick. The service coalesces the asking; with none, work waits for the tick as before.
    pass_requests: Option<PassRequests>,
}

/// How the engine asks its service for a pass of the tick (ADR 0102, amended 2026-11-04).
pub type PassRequests = std::sync::Arc<dyn Fn() + Send + Sync>;

/// While it lives, the engine is doing what the owner asked; see [`Engine::for_owner`].
struct ForOwner<'a>(&'a AtomicBool);

impl Drop for ForOwner<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// A live turn as the screen shows it.
fn run_shown(r: &Run) -> serde_json::Value {
    json!({
        "id": r.id.0, "member": r.member.0, "task": r.task.map(|t| t.0),
        "status": r.status.name(), "started_at": r.started_at, "reserved": r.reserved,
        "spent": r.spent,
        // Why its last attempt ended, for one waiting to be tried again.
        "why": r.end_reason,
        // The brain it runs or ran on (ADR 0131), once started.
        "brain": r.brain.map(Brain::name),
        "input_tokens": r.input_tokens + r.cache_read_tokens + r.cache_write_tokens,
        "output_tokens": r.output_tokens,
    })
}

/// A routine as the screen shows it, with whether the task it last made is still open.
fn routine_shown(r: &crate::domain::Routine, tasks: &[Task]) -> serde_json::Value {
    json!({
        "id": r.id.0, "title": r.title, "description": r.description, "goal": r.goal.0,
        "project": r.project.map(|p| p.0),
        "assignee": r.assignee.0, "every_ms": r.every_ms,
        "rhythm": routines::rhythm(r.every_ms), "next_due_at": r.next_due_at,
        "enabled": r.enabled, "on_lab_trouble": r.on_lab_trouble,
        "last_fired_at": r.last_fired_at, "last_task": r.last_task.map(|t| t.0),
        "last_task_open": r.last_task
            .and_then(|id| tasks.iter().find(|t| t.id == id))
            .is_some_and(|t| !t.status.finished()),
    })
}

/// Where a queued turn was placed, and what it reserves; see [`Engine::launch`].
#[derive(Clone, Copy)]
struct Placement {
    brain: Brain,
    amount: Micros,
    kind: turns::TurnKind,
}

/// What the screen's task rows are read against; see [`Engine::board`].
struct Board<'a> {
    tasks: &'a [Task],
    goals: &'a [crate::domain::Goal],
    routines: &'a [crate::domain::Routine],
    branches: Vec<crate::domain::wing::Branch>,
    checks: Vec<crate::domain::wing::Check>,
    posts: std::collections::HashMap<TaskId, usize>,
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
            pass_requests: None,
        }
    }

    /// The engine that asks `ask` for a pass of the tick once the owner's new work is stored
    /// (ADR 0102, amended 2026-11-04). The service wires it to its ticker, which coalesces
    /// what is asked and keeps its own clock besides.
    #[must_use]
    pub fn with_pass_requests(mut self, ask: PassRequests) -> Self {
        self.pass_requests = Some(ask);
        self
    }

    /// Asks for a pass when the owner's work was stored: never for a member's own work, nor
    /// for work refused. Called with the engine's lock released.
    fn ask_for_a_pass_after(&self, by: Author, stored: &Done<TaskId>) {
        if by != Author::Owner || stored.is_err() {
            return;
        }
        self.ask_for_a_pass();
    }

    /// The runner's report that a turn ended was accepted: ask for a pass, so the turn is
    /// settled now rather than on the clock (ADR 0102, amended 2026-11-04, round 17). The pass
    /// is the ordinary tick; how long a failed turn waits to be tried again is unchanged.
    /// Called by the service with the engine's lock released.
    pub fn a_turn_has_ended(&self) {
        self.ask_for_a_pass();
    }

    /// Asks the service for a pass of the tick, when it listens (`with_pass_requests`).
    fn ask_for_a_pass(&self) {
        if let Some(ask) = &self.pass_requests {
            ask();
        }
    }

    /// The engine with Endora's public library, for every member's `read_doc`.
    #[must_use]
    pub fn with_docs(mut self, docs: Library) -> Self {
        self.docs = docs;
        self
    }

    /// Turns downtime on or off (ADR 0119): whether a member with nothing to do is given a turn
    /// for the office and its notebook. The service turns it on at start; a scenario, when it
    /// is about downtime.
    pub fn set_downtime(&mut self, on: bool) {
        self.settings.downtime = on;
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
    pub(super) fn mac_facts(&self) -> Option<MacFacts> {
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
    /// What a queued turn may reserve, if it may: only judgment is paid for, and work on the
    /// house's models reserves nothing. A league side is paid from the league's capped project
    /// alone; it reserves nothing outside one. Judgment is paid on `brain`, and needs at least
    /// what its opening call costs there.
    fn reservable(
        &self,
        run: &Run,
        member: &Member,
        (kind, brain): (turns::TurnKind, Brain),
        league: bool,
        project: Option<ProjectId>,
    ) -> Done<Option<Micros>> {
        let least = money::least_a_turn(brain);
        match kind {
            turns::TurnKind::Work => Ok(Some(0)),
            turns::TurnKind::Judgment if league => match self.project_left(project)? {
                Some(_) => self.affordable(member, project, true, least),
                None => Ok(None),
            },
            turns::TurnKind::Judgment => {
                self.affordable(member, project, run.asked_by_owner, least)
            }
        }
    }

    /// A member's share is its first claim, not a wall (ADR 0113, amended): past it, the
    /// member borrows at most one more share of what the other paid members have not used
    /// today, so its day reaches twice its share and no more, the owner's allowance aside
    /// ([`money::may_draw`]). Turned away at that limit with the pool's day not spent, it puts
    /// one "out for today" item on the owner's desk, and another after each allowance.
    /// A turn the owner asked for (`for_owner`) is the pool's alone and needs no share. The
    /// pool's day is the hard ceiling either way, and the per-turn cap holds.
    fn affordable(
        &self,
        member: &Member,
        project: Option<ProjectId>,
        for_owner: bool,
        least: Micros,
    ) -> Done<Option<Micros>> {
        let day = self.today();
        let pool_left = self.pool_left_today()?;
        let mut most = most_a_turn_for(member);
        if let Some(left) = self.project_left(project)? {
            most = most.min(left);
        }
        if for_owner {
            return Ok(money::reservation(pool_left, pool_left, most, least));
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
        let amount = money::reservation(draw, pool_left, most, least);
        // Turned away at twice its share while the pool's day and the project's month have
        // money: the owner is asked, once for each allowance (ADR 0113, amended 2026-11-01).
        let out_for_the_day = share > 0
            && !member.furloughed()
            && draw < least
            && pool_left >= least
            && most >= least;
        if amount.is_none() && out_for_the_day {
            self.ask_about_the_day(member, day, (share, spent, allowance), pool_left)?;
        }
        Ok(amount)
    }

    /// What is left of the pool's day, whoever spent it.
    fn pool_left_today(&self) -> Done<Micros> {
        Ok(self.pool_day_today()? - self.store.committed(None, self.today())?)
    }

    // ---- the team and the work ---------------------------------------------------------

    /// Adds a member. (Hiring goes through the owner's desk once HQ is a service.)
    pub fn hire(&self, name: &str, share_percent: u8, slots: u8, look: u8) -> Done<MemberId> {
        let _held = self.held();
        let members = self.store.members()?;
        // No name given: the next free one (ADR 0142).
        let given = if name.trim().is_empty() {
            let taken: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
            crate::domain::names::next_free(&taken)
        } else {
            name.to_owned()
        };
        let id = self.store.add_member(&given, share_percent, slots, look)?;
        Ok(id)
    }

    /// Pauses or resumes a member: a paused member's wakes are turned away.
    pub fn pause(&self, member: MemberId, paused: bool) -> Done<()> {
        let _held = self.held();
        Ok(self.store.set_paused(member, paused)?)
    }

    /// The owner sets a member's model (ADR 0109, 0142), by its name: every turn of the member's
    /// runs on it, whatever woke it. The node asks HQ on every call, so a turn already running is
    /// held to it at once. Returns the brain set.
    ///
    /// Any member may be any model; a title is a name only.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; [`Refusal::NotABrain`] for
    /// a name that is not a model Endora has; a store failure.
    pub fn set_brain(&self, member: MemberId, brain: &str) -> Done<Brain> {
        let _held = self.held();
        let found = self.member(member)?;
        self.brain_set_held_lock(&found, brain)
    }

    /// The owner sets a member's model from the console (ADR 0142), as [`Self::set_brain`] does.
    /// A member whose runtime cannot run the new model is put back on Endora's worker, and the
    /// answer says so (ADR 0117, amended 2026-10-31).
    ///
    /// # Errors
    /// As [`Self::set_brain`].
    pub fn set_model(&self, member: MemberId, brain: &str) -> Done<ModelSet> {
        let _held = self.held();
        let found = self.member(member)?;
        let brain = self.brain_set_held_lock(&found, brain)?;
        let said = self.runtime_after_model(&found, brain)?;
        Ok(ModelSet { brain, said })
    }

    /// Records a model set (the lock held), and says it in the feed.
    fn brain_set_held_lock(&self, found: &Member, brain: &str) -> Done<Brain> {
        let brain = Brain::named(brain).map_err(EngineError::Refused)?;
        self.store.set_brain(found.id, brain)?;
        self.emit(
            EventKind::BrainSet,
            json!({ "member": found.id.0, "name": found.name, "brain": brain.name(),
                    "slot": "model" }),
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
    pub fn set_goal(&self, title: &str, reason: &str, by: Author) -> Done<GoalId> {
        let _held = self.held();
        if by != Author::Owner {
            return self.propose_goal_held_lock(title, reason, None, &format!("A goal: {title}"));
        }
        self.set_goal_authored_held_lock(title, reason, InputOrigin::Owner)
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
        let stored = {
            let _held = self.held();
            let _asked = (by == Author::Owner).then(|| self.for_owner());
            let origin = if by == Author::Owner {
                InputOrigin::Owner
            } else {
                InputOrigin::Unknown
            };
            self.add_task_authored_held_lock(by, task, origin)
        };
        self.ask_for_a_pass_after(by, &stored);
        stored
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
        // Work goes to a member through its loop (ADR 0142): a member hands none to another.
        if let (Author::Member(from), Some(to)) = (by, task.assignee) {
            if from != to {
                return Err(EngineError::Refused(Refusal::NotBelow));
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

    // ---- what members do -------------------------------------------------------------------

    fn mark_progress(&self, member: MemberId, task: TaskId) -> Done<()> {
        for mut run in self.store.live_runs()? {
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
    /// names the member (`@name`, as a wake reads it).
    ///
    /// # Errors
    /// [`EngineError::Refused`] with [`Refusal::OutOfReach`] when it may not.
    fn reach(&self, run: &Run, task: TaskId, writing: bool) -> Done<()> {
        let target = self.task(task)?;
        let member = self.member(run.member)?;
        if let Some(own) = run.task {
            if self.under(target.id, own)? {
                return Ok(());
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
        // Unsupported only when it states something of the member's own, cites nothing, and
        // its turn has no receipt (ADR 0114, amended). The worker's last word (`answers`
        // false) is what the model said as it stopped, never tagged.
        // Climb 2, S1: only a receipt the node witnessed supports what cites nothing.
        let unsupported = answers && cited.is_empty() && !self.has_witnessed_receipts(run.id)?;
        self.post_authored_held_lock_marked(
            Author::Member(member.id),
            task,
            text,
            cited,
            unsupported,
            InputOrigin::Run(run.id),
        )?;
        Ok(())
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
        let woke = self.post_held(by, task, text)?;
        // The owner's reply that woke someone asks for a pass, with the lock released: a turn
        // it queued, or a waiting one it brought forward, starts now rather than on the clock
        // (ADR 0102, amended 2026-11-04, round 17).
        if woke {
            self.ask_for_a_pass();
        }
        Ok(())
    }

    /// [`Self::post`] under the lock: whether the owner's reply woke someone (a turn queued, or
    /// folded into one, which brings a waiting turn forward).
    fn post_held(&self, by: Author, task: TaskId, text: &str) -> Done<bool> {
        let _held = self.held();
        if let Author::Member(_) = by {
            return Err(EngineError::Refused(Refusal::OutOfReach));
        }
        if by != Author::Owner {
            return self.post_held_lock(by, task, text).map(|()| false);
        }
        // The owner's reply on an open task wakes whoever has it at once, named or not: it is
        // new, so the back-off never holds it.
        let _asked = self.for_owner();
        let found = self.task(task)?;
        if found.status.finished() {
            return Err(EngineError::Refused(Refusal::Finished));
        }
        self.post_authored_held_lock_marked(by, task, text, &[], false, InputOrigin::Owner)?;
        // Whoever the reply names was woken by the post itself.
        let members = self.store.members()?;
        let named = mentions(text, &members);
        let Some(member) = found.holder() else {
            return Ok(!named.is_empty());
        };
        if named.iter().any(|m| m.id == member) {
            return Ok(true);
        }
        let receipt = self.wake_held_lock(member, Some(task), WakeReason::OwnerReplied)?;
        Ok(!named.is_empty() || matches!(receipt, Receipt::Queued | Receipt::Folded))
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
        self.set_status_held_lock_noting(member, task, to, reason, &[])
    }

    /// As [`Self::set_status_held_lock`], writing `notes` to the thread after every check and
    /// before the status changes.
    pub(crate) fn set_status_held_lock_noting(
        &self,
        member: MemberId,
        task: TaskId,
        to: TaskStatus,
        reason: &str,
        notes: &[String],
    ) -> Done<()> {
        let current = self.task(task)?;
        // Its holder moves it.
        if current.owner != Some(member) {
            return Err(EngineError::Refused(Refusal::NotYours));
        }
        // A routine's check changes no code: its member closes it (ADR 0107).
        let check = to == TaskStatus::Done && self.is_a_check(task)?;
        if check {
            work::may_close_check(current.status).map_err(EngineError::Refused)?;
        } else {
            work::may_set_status(current.status, to).map_err(EngineError::Refused)?;
        }
        // Set ready in a loop worked alone, the item is done by HQ's checks: they must pass
        // first (ADR 0142, amended 2026-11-01).
        if to == TaskStatus::InReview && self.solo_of(&current)?.is_some() {
            self.ready_alone(&current)?;
        }
        for note in notes {
            self.post_system_held_lock(task, note)?;
        }
        self.store.set_task_status(task, to)?;
        if to == TaskStatus::Blocked {
            self.mark_turn(member, task, &|run| run.blocked = true)?;
            self.loop_blocked(task, member, reason)?;
        } else {
            self.mark_progress(member, task)?;
        }
        let said = if to == TaskStatus::Todo {
            format!("Handed back: {reason}")
        } else {
            format!("{} → {}: {reason}", current.status.name(), to.name())
        };
        self.post_own_held_lock(member, task, &said, &[])?;
        if to == TaskStatus::Todo {
            self.store.release(task)?;
            self.emit(
                EventKind::TaskReleased,
                json!({ "task": task.0, "member": member.0 }),
            )?;
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

    /// Work was moved to review by its owner: the reviewer is woken to look at it. With no
    /// reviewer, or the reviewer's own work, it is on the owner's desk to read, with its
    /// thread, as it stands.
    fn review_asked(&self, task: &Task) -> Done<()> {
        // A new ask is a new review: nobody is passed over on it yet (D26).
        self.review_starts_again(task.id)?;
        // What can be judged by rule is judged first (ADR 0142, amended 2026-10-30): a branch
        // that touches a forbidden path goes back without a reviewer.
        if self.checked_at_review(task)? {
            return Ok(());
        }
        // A loop worked alone has no review: HQ's checks passed, so it is done.
        if let Some(alone) = self.solo_of(task)? {
            return self.done_alone(task, alone);
        }
        // In the loops, a member at Haiku's rank or below reviews it, when there is one (ADR
        // 0142, amended 2026-10-31).
        if let Some(reviewer) = self.loop_reviewer_of(task)? {
            self.post_held_lock(
                Author::Endora,
                task.id,
                &format!("{} reviews this.", reviewer.name),
            )?;
            self.wake_held_lock(reviewer.id, Some(task.id), WakeReason::ReviewRequested)?;
            return Ok(());
        }
        // No reviewer on the team: the owner reads it.
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

    /// The one gate for a verdict on work in review (ADR 0142, amended 2026-10-31), a pass's
    /// ([`Self::test_in`]) or a send-back's ([`Self::send_back_in`]): the turn reaches the item,
    /// its member is the item's reviewer in the loops, the item is in review, and the work is
    /// not the reviewer's own; a pass also needs a CI criterion a push can meet. Returns the
    /// reviewer, the item and who holds its work.
    ///
    /// # Errors
    /// [`Refusal::OutOfReach`], [`Refusal::NotYours`] for a member who is not its reviewer,
    /// [`Refusal::NotAStep`] for a task not in review, [`Refusal::OwnWork`]; for a pass, what
    /// [`Self::ci_can_be_had`] refuses.
    fn verdict_gate(
        &self,
        run: &Run,
        task: TaskId,
        pass: bool,
    ) -> Done<(Member, Task, Option<MemberId>)> {
        self.reach(run, task, true)?;
        let reviewer = self.member(run.member)?;
        let current = self.task(task)?;
        if self
            .loop_reviewer_of(&current)?
            .is_none_or(|r| r.id != reviewer.id)
        {
            return Err(EngineError::Refused(Refusal::NotYours));
        }
        if current.status != TaskStatus::InReview {
            let to = if pass {
                TaskStatus::Done
            } else {
                TaskStatus::Doing
            };
            return Err(EngineError::Refused(Refusal::NotAStep {
                from: current.status,
                to,
            }));
        }
        let worker = current.holder();
        if worker == Some(reviewer.id) {
            return Err(EngineError::Refused(Refusal::OwnWork));
        }
        if pass {
            // A CI criterion no push can meet is never passed (ADR 0142, amended 2026-10-31).
            self.ci_can_be_had(&current)?;
        }
        Ok((reviewer, current, worker))
    }

    /// After a verdict (the lock held): what the worker and the reviewer marked as lessons goes
    /// in the loop's list; a send-back (`sent_back`: the reviewer's name and the work's holder)
    /// is also one of the item's failures, and kept at its rank the item goes back to its
    /// worker with a handover (ADR 0142, amended 2026-10-30).
    fn after_verdict(
        &self,
        current: &Task,
        notes: &str,
        sent_back: Option<(&str, Option<MemberId>)>,
    ) -> Done<()> {
        self.learn_held_lock(current, notes)?;
        let Some((reviewer, worker)) = sent_back else {
            return Ok(());
        };
        self.loop_failed(current.id)?;
        let now = self.task(current.id)?;
        if let Some(worker) = worker.filter(|w| now.holder() == Some(*w)) {
            self.hand_over(current.id, worker, &format!("sent back by {reviewer}"))?;
        }
        Ok(())
    }

    /// The reviewer sends work in review back to its holder, in its turn ([`Self::test_in`]
    /// without a pass): it returns as `doing`, its holder is woken, and the notes go in the
    /// thread. A send-back may name its `reason` when it is one HQ counts (ADR 0132).
    ///
    /// # Errors
    /// [`Refusal::TurnOver`], and what [`Self::verdict_gate`] refuses.
    fn send_back_in(
        &self,
        run: RunId,
        task: TaskId,
        notes: &str,
        reason: Option<SendBackReason>,
    ) -> Done<()> {
        let _held = self.held();
        let run = self.running(run)?;
        let (reviewer, current, worker) = self.verdict_gate(&run, task, false)?;
        // The reviewer's verdict is its deliverable, whichever way it goes (ADR 0114).
        self.mark_progress(reviewer.id, task)?;
        self.store.set_task_status(task, TaskStatus::Doing)?;
        self.post_own_held_lock(reviewer.id, task, &format!("Sent back: {notes}"), &[])?;
        self.emit_status(&current, TaskStatus::Doing)?;
        self.emit_sent_back(task, reviewer.id, "tester", reason)?;
        if let Some(owner) = worker {
            self.wake_held_lock(owner, Some(task), WakeReason::SentBack)?;
        }
        self.after_verdict(&current, notes, Some((&reviewer.name, worker)))
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
    /// Whether passing `task` waits for the gate: work that is not a routine's check nor the
    /// home lab's, and that has a branch for the gate to run on. Work that changed nothing has
    /// none (live, #855 found roles/ already clean and waited in review for a gate that could
    /// never run).
    pub(super) fn gated(&self, task: &Task) -> Done<bool> {
        let needs = review::needs_gate(
            self.routine_of(task.id)?.is_some(),
            self.the_home_labs(task)?,
        );
        Ok(needs && self.store.branches()?.iter().any(|b| b.task == task.id))
    }

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
        self.accept(&current)
    }

    /// Whether an item set ready in a loop worked alone may be (ADR 0142, amended 2026-11-01):
    /// the check of its newest head passed, for work the gate holds, and a CI run its criteria
    /// ask for can be had, as a reviewer's pass would need.
    ///
    /// # Errors
    /// [`Refusal::GateNotPassed`]; what [`Self::ci_can_be_had`] refuses.
    fn ready_alone(&self, task: &Task) -> Done<()> {
        if self.gated(task)? && !self.store.gate(task.id)?.unwrap_or(false) {
            return Err(EngineError::Refused(Refusal::GateNotPassed));
        }
        self.ci_can_be_had(task)
    }

    /// An item of a loop `alone` works by itself passed HQ's checks as it was set ready: it is
    /// done with no review, and goes on as accepted work does (ADR 0142, amended 2026-11-01).
    /// With no branch there was nothing to check but that its criteria cite evidence: it is
    /// completed, not verified, and kept so (ADR 0124, amended 2026-11-03).
    fn done_alone(&self, task: &Task, alone: MemberId) -> Done<()> {
        use crate::domain::closure::{Closure, DoneWhy};
        let name = self.member(alone)?.name;
        if self.gated(task)? {
            self.post_held_lock(
                Author::Endora,
                task.id,
                &format!(
                    "HQ's checks passed and {name} set it ready: done, with no review ({name} \
                     works this loop alone)."
                ),
            )?;
            return self.accept(task);
        }
        self.post_held_lock(
            Author::Endora,
            task.id,
            &format!(
                "{name} set it ready with its criteria cited: completed, not verified. Nothing \
                 checked that the evidence settles it: there is no branch to check, and nobody \
                 reviews ({name} works this loop alone)."
            ),
        )?;
        self.store.set_task_status(task.id, TaskStatus::Done)?;
        self.closed(
            task.id,
            Closure::Done(DoneWhy::CompletedUnverified),
            None,
            "endora",
        )?;
        // The feed says it too, so what counts outcomes keeps it apart from verified ones.
        self.emit(
            EventKind::TaskChanged,
            json!({ "task": task.id.0, "goal": task.goal.0, "owner": task.owner.map(|o| o.0),
                    "status": TaskStatus::Done.name(), "priority": task.priority.name(),
                    "unverified": true }),
        )?;
        self.after_finishing(task)
    }

    /// Work accepted, its gate passed: done, listed for the merge (or carded, if the owner put
    /// the cards back), and on to what follows a finish.
    fn accept(&self, current: &Task) -> Done<()> {
        let task = current.id;
        self.store.set_task_status(task, TaskStatus::Done)?;
        self.emit(
            EventKind::TaskChanged,
            json!({ "task": task.0, "goal": current.goal.0, "owner": current.owner.map(|o| o.0),
                    "status": TaskStatus::Done.name(), "priority": current.priority.name() }),
        )?;
        // Accepted work is listed, not carded, unless the owner put the cards back (ADR 0115,
        // amended 2026-10-31).
        if current.parent.is_none() && self.cards_back()? {
            let (kind, says) = ("ready_to_merge", "Ready to merge");
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
        self.after_finishing(current)
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
        if let Some(owner) = parent.holder() {
            self.wake_held_lock(owner, Some(parent.id), WakeReason::SubtasksFinished)?;
        }
        Ok(())
    }

    // ---- money -----------------------------------------------------------------------------

    /// The node's word on which model answered one of a turn's calls, as the provider named
    /// it, or that the answer named none (the agentic-harness climb, round 7). Kept beside the
    /// turn, every name with its calls; it changes no charge and no budget.
    ///
    /// # Errors
    /// The store's, and [`EngineError::Unknown`] for a run HQ does not have.
    pub fn note_served_model(&self, run: RunId, model: Option<&str>) -> Done<()> {
        let _held = self.held();
        self.store.run(run)?.ok_or(EngineError::Unknown("run"))?;
        self.store.note_served_model(run, model)?;
        Ok(())
    }

    /// The models that answered a turn's calls, and how many calls each; `None` for calls whose
    /// model is not known.
    ///
    /// # Errors
    /// The store's.
    pub fn served_models(&self, run: RunId) -> Done<Vec<(Option<String>, u32)>> {
        Ok(self.store.served_models(run)?)
    }

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
        let most = most_a_turn_for(&self.member(current.member)?);
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
    pub fn stop_all(&self, why: &str) -> Done<()> {
        let _held = self.held();
        self.stop_all_held_lock("owner", why, json!({}))
    }

    /// Stops every turn at once and turns every wake away, as `by` (the owner, or the runaway
    /// tripwire, ADR 0133) and for `why`; `evidence`, an object, is told in the feed with it. The
    /// lock is held.
    fn stop_all_held_lock(&self, by: &str, why: &str, evidence: serde_json::Value) -> Done<()> {
        self.store.set_stopped(true)?;
        self.store.set_setting(runaway::STOPPED_BY, by)?;
        let reason = if by == "owner" {
            "the kill switch"
        } else {
            "the runaway tripwire"
        };
        for run in self.store.live_runs()? {
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
    pub fn resume(&self) -> Done<()> {
        let _held = self.held();
        self.resume_held_lock()
    }

    // ---- the tick ----------------------------------------------------------------------------

    /// The engine's regular pass (every 30 seconds), in the spec's order: routines and
    /// check-backs, ended and vanished turns, retries, starts, stuck work, finished waits. A
    /// late tick runs once.
    pub fn tick(&self) -> Done<TickReport> {
        let _held = self.held();
        let mut report = TickReport::default();
        let now = self.clock.now();
        // A drain before a restart (ADR 0134): the one the last HQ kept is lifted now that HQ is
        // up again, before its turns are looked at, so the ones the restart cut are counted; and
        // one nobody ended outlasts its age.
        self.drain_tick_held_lock(now)?;
        if self.store.stopped()? {
            return Ok(report);
        }

        // 1. Routines that are due fire; a late one fires once.
        self.fire_due(now, &mut report)?;
        // Check-backs whose time came wake their member, while the task is still open.
        self.fire_check_backs(now, &mut report)?;
        // Approvals nobody decided within a day go unrun, and the thread says so (ADR 0108).
        self.expire_approvals(now)?;

        // 2. Running turns: ended, failed, vanished, or too long.
        self.settle_running(now, &mut report)?;

        // A project paused for its budget goes on once the month turns (ADR 0113).
        self.lift_budget_pauses()?;

        // 3. Retries whose time has come are queued again.
        self.requeue_retries(now, &mut report)?;

        // 3b. One member with nothing to do is given a downtime turn (ADR 0119).
        // The team's map (ADR 0126): a lead hired, a project or goal made since the last look
        // gets its page.
        self.migrate_map_held()?;
        // The team's history (ADR 0129): what is past its thirty days, or beyond the last five
        // of a member on a task, is let go; handover notes and posts are not touched.
        self.history_tick(now)?;
        if self.settings.downtime {
            self.downtime_look(now)?;
        }
        // Games (ADR 0121): free members play a match in code, and a game of chess nobody
        // has moved in a week is adjourned. No model is asked.
        self.games_tick(now)?;
        // Scenarios (ADR 0135): a world that has ended with its tasks open is closed.
        self.close_ended_scenarios()?;
        // The model league: the next pair of brains sits down to chess once nothing is played.
        self.league_tick(now)?;
        // The loops hand out their plans' next items (ADR 0142).
        self.loops_tick()?;

        // 4. Queued turns start where a member has a free slot, budget can be reserved, and a
        //    runner has room: urgent work first, then the oldest. The tripwire may stop the team
        //    here (ADR 0133): then nothing else in this tick is done, as when the owner stops it.
        report.started = self.start_queued(now)?;
        // Work that waits long for the Mac is said in the owner's digest (Phase 0 audit, P5).
        self.long_mac_waits(now)?;
        if self.store.stopped()? {
            return Ok(report);
        }

        let tasks = self.store.tasks()?;
        let live: Vec<Option<TaskId>> = self.store.live_runs()?.iter().map(|r| r.task).collect();
        // 5. Doing, with no turn: parked work (ADR 0124) gets one nudge, then climbs the
        //    ladder; work held on purpose is left alone, and work HQ cannot tell about gets
        //    one look from its area's lead.
        self.stalled_work(&tasks, &live, &mut report)?;

        // 5b. Open work handed to a member that nothing will bring it back to: no turn queued,
        //     running or waiting to retry, and no wake held for it, for ten minutes. One
        //     recovery wake for its member; still so ten minutes after, the owner's desk, once.
        //     Never handed to anyone else.
        let (stranded_nudged, stranded_stuck) = self.recover_stranded(&tasks, &live, now)?;
        report.nudged += stranded_nudged;
        report.stuck += stranded_stuck;
        // 5b'. Work whose last turn never started (the runner could not start it) is woken
        //      again for the same member after a wait: nobody's failure (ADR 0142, amended
        //      2026-10-31).
        self.wake_after_failed_starts(&tasks, &live, now)?;
        // 5c. The guards (ADR 0124): what is stuck and has moved again closes itself, and every
        //     few minutes each project's process health is looked at.
        self.guards_tick()?;
        self.health_tick()?;

        // 6. Tasks whose waits have all finished are woken, once.
        self.wake_finished_waits(&tasks, &mut report)?;

        self.emit(
            EventKind::Tick,
            json!({ "started": report.started, "ended": report.ended, "stuck": report.stuck }),
        )?;
        Ok(report)
    }

    /// Due routines fire, each once however late, and count on from now (tick step 1).
    fn fire_due(&self, now: Millis, report: &mut TickReport) -> Done<()> {
        for mut routine in self.store.routines()? {
            if !routine.enabled || routine.next_due_at > now {
                continue;
            }
            routine.next_due_at = routines::next_due(now, routine.every_ms);
            self.fire_held_lock(routine, &Firing::Schedule)?;
            report.routines += 1;
        }
        Ok(())
    }

    /// Check-backs whose time came wake their member, while the task is still open (tick step 1).
    fn fire_check_backs(&self, now: Millis, report: &mut TickReport) -> Done<()> {
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
        Ok(())
    }

    /// Approvals nobody decided within a day go unrun, and the thread says so (ADR 0108).
    fn expire_approvals(&self, now: Millis) -> Done<()> {
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
        Ok(())
    }

    /// Running turns that ended, failed, vanished or ran too long are settled (tick step 2).
    fn settle_running(&self, now: Millis, report: &mut TickReport) -> Done<()> {
        for run in self.store.live_runs()? {
            if run.status == RunStatus::Running {
                self.settle_one(run, now, report)?;
            }
        }
        Ok(())
    }

    fn settle_one(&self, run: Run, now: Millis, report: &mut TickReport) -> Done<()> {
        if run
            .started_at
            .is_some_and(|started| now - started > LONGEST_TURN_MS)
        {
            self.runner.stop(run.id, self.settings.grace);
            self.end(run, RunStatus::TimedOut, "took longer than a turn may")?;
            report.ended += 1;
            return Ok(());
        }
        let retrying = match self.runner.status(run.id) {
            Liveness::Running => return Ok(()),
            Liveness::Finished { succeeded: true } => {
                self.end(run, RunStatus::Succeeded, "ended its turn")?;
                false
            }
            Liveness::Finished { succeeded: false } => {
                // The runner's words when it gave them: the worker's exit code's meaning and
                // its last words, which would otherwise be lost with its container.
                let why = self
                    .runner
                    .why_ended(run.id)
                    .unwrap_or_else(|| "the turn failed".to_owned());
                // Whose it was: a container the runner found gone, or killed from outside
                // before its full time, was cut short by the platform (ADR 0124, amended).
                let ran = run.started_at.map_or(0, |started| now - started);
                let exit = self.runner.exit_of(run.id);
                let fault = runs::fault_of(exit, ran);
                // A container that never started ran nothing: it says so, so the record knows
                // it is nobody's failure (ADR 0142, amended 2026-10-31).
                let why = if runs::never_started(exit) {
                    runs::never_started_said(&why)
                } else {
                    why
                };
                self.fail(run, &why, fault)?
            }
            // HQ lost the turn: it restarted, and with it the memory of what the runner was
            // handed. Nothing its member did.
            Liveness::Gone => self.fail(run, "its container is gone", Fault::Platform)?,
        };
        if retrying {
            report.retrying += 1;
        } else {
            report.ended += 1;
        }
        Ok(())
    }

    /// Retries whose time has come are queued again (tick step 3).
    fn requeue_retries(&self, now: Millis, report: &mut TickReport) -> Done<()> {
        for mut run in self.store.live_runs()? {
            if run.status == RunStatus::RetryWaiting && run.retry_at.is_some_and(|due| due <= now) {
                run.status = RunStatus::Queued;
                run.retry_at = None;
                self.store.save_run(&run)?;
                report.requeued += 1;
            }
        }
        Ok(())
    }

    /// Doing work with no turn: nudged once, then marked stuck once it has sat long (tick step 5).
    fn stalled_work(
        &self,
        tasks: &[Task],
        live: &[Option<TaskId>],
        report: &mut TickReport,
    ) -> Done<()> {
        for task in tasks {
            if task.status == TaskStatus::Doing && !live.contains(&Some(task.id)) {
                self.stalled_one(task, report)?;
            }
        }
        Ok(())
    }

    fn stalled_one(&self, task: &Task, report: &mut TickReport) -> Done<()> {
        let Some(owner) = task.owner else {
            return Ok(());
        };
        // A scenario's world wakes its players (ADR 0135): waiting for it is no stall.
        if self.in_a_scenario(task.id)? {
            return Ok(());
        }
        // Its member said when it comes back, or it waits on open work: that is its plan, not
        // a stall (live, #667 was woken every ten seconds while its check-back waited).
        let Some(found) = self.diagnosis(task.id)? else {
            return Ok(());
        };
        // In the loops (ADR 0142) any stall but work and holds counts, is nudged once, then
        // each further stall is one of the item's failures: three climb a rank. No ladder, no
        // desk. Outside them only parked work does (ADR 0124).
        let looped = self.loop_holds(task)?;
        let stalled = if looped {
            !matches!(found.stopped, Stopped::Working | Stopped::Held)
        } else {
            found.stopped == Stopped::Parked
        };
        if !stalled {
            return Ok(());
        }
        if !task.nudged {
            self.store.set_nudged(task.id, true)?;
            if self.nudge_once(task, owner, WakeReason::Recovery)? {
                report.nudged += 1;
            }
            return Ok(());
        }
        // It is marked stuck once it has sat as long as stranded work does: never a reflex.
        if !self.idle_for(task.id, STRANDED_AFTER_MS)? {
            return Ok(());
        }
        report.stuck += 1;
        if looped {
            return self.loop_stalled(task);
        }
        self.store.set_task_status(task.id, TaskStatus::Blocked)?;
        self.emit(
            EventKind::TaskChanged,
            json!({ "task": task.id.0, "goal": task.goal.0, "owner": owner.0,
                    "status": TaskStatus::Blocked.name(), "priority": task.priority.name() }),
        )?;
        Ok(())
    }

    /// Tasks whose waits have all finished are woken, once (tick step 6).
    fn wake_finished_waits(&self, tasks: &[Task], report: &mut TickReport) -> Done<()> {
        for task in tasks {
            if task.waits_on.is_empty() || task.status.finished() {
                continue;
            }
            let Some(member) = task.holder() else {
                continue;
            };
            if self.store.woken_after_waits(task.id)? || !self.waits_over(&task.waits_on)? {
                continue;
            }
            self.store.set_woken_after_waits(task.id, true)?;
            self.wake_held_lock(member, Some(task.id), WakeReason::WaitFinished)?;
            report.woken += 1;
        }
        Ok(())
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
            .live_runs()?
            .into_iter()
            .filter(|r| r.status == RunStatus::Queued)
            .collect();
        queued.sort_by_key(|r| (priority(r), r.queued_at, r.id));
        // What the caps on a runaway read, once for the tick and kept up as turns start (ADR
        // 0133). A scripted team, the demo, has none.
        let mut starts = self.starts_read(now)?;
        let mut started = 0;
        // A downtime turn queued before a round of the experiment began waits for its close
        // (ADR 0142, amended 2026-11-01).
        let round = self.round_running()?;
        for run in queued {
            if round && self.reason_for(&run)? == WakeReason::Downtime.name() {
                continue;
            }
            let Some(run) = self.task_still_open(run, &tasks)? else {
                continue;
            };
            let running: Vec<Run> = self
                .store
                .live_runs()?
                .into_iter()
                .filter(|r| r.status == RunStatus::Running)
                .collect();
            if running.len() >= self.runner.capacity() {
                break;
            }
            let member = self.member(run.member)?;
            let Some(mut run) = self.member_still_free(run, &member)? else {
                continue;
            };
            let busy = running.iter().filter(|r| r.member == member.id).count();
            if busy >= usize::from(member.slots) || member.paused {
                continue;
            }
            let project = self.project_of_run(&run)?;
            if self.held_by_owner(project)? {
                continue;
            }
            // A member is its model (ADR 0142): every turn of its runs there, whatever woke it.
            // A side of a league game is pinned to its brain, whoever plays it, and a Claude
            // side is paid from the league's capped project alone. A turn that cannot run there
            // now waits where it is, and says why; it is never given another brain.
            let league = self.league_brain_of(run.task)?;
            let brain = league.unwrap_or(member.brain);
            let kind = turns::TurnKind::on(brain);
            // A scenario's turn is bounded by its world, not its player's day (ADR 0133, amended).
            let in_a_world = match run.task {
                Some(task) => self.in_a_scenario(task)?,
                None => false,
            };
            let asked = self.reservable(&run, &member, (kind, brain), league.is_some(), project)?;
            let on_the_mac = running.iter().any(|r| r.brain == Some(Brain::Mac));
            let facts = turns::Facts {
                mac_waits: mac::wait_for(self.mac_facts().as_ref(), now, on_the_mac),
                can_afford: asked.is_some(),
                free_turns_wasted: if in_a_world {
                    0
                } else {
                    starts.free_turns_wasted_of(&run)
                },
            };
            let (brain, amount) = match turns::place(brain, &facts) {
                turns::Placed::On(brain) => (brain, asked.unwrap_or(0)),
                turns::Placed::Waits(wait) => {
                    self.say_why_it_waits(&mut run, wait.words())?;
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
            starts.started(member.id, now);
            let placed = Placement {
                brain,
                amount,
                kind,
            };
            if self.launch(run, &member, &placed, now)? {
                started += 1;
            }
        }
        Ok(started)
    }

    /// A queued turn whose task has since finished (dropped, or done) is ended, not started:
    /// nothing is left for it to do.
    fn task_still_open(&self, run: Run, tasks: &[Task]) -> Done<Option<Run>> {
        let over = run
            .task
            .and_then(|t| tasks.iter().find(|task| task.id == t))
            .is_some_and(|t| t.status.finished());
        if over {
            self.end(run, RunStatus::Stopped, "its task is over")?;
            return Ok(None);
        }
        Ok(Some(run))
    }

    /// A queued turn whose member went on furlough, or was paused before its downtime, since
    /// its wake is ended, not kept waiting.
    fn member_still_free(&self, run: Run, member: &Member) -> Done<Option<Run>> {
        if member.furloughed() {
            self.end(run, RunStatus::Stopped, "its member is on furlough")?;
            return Ok(None);
        }
        let downtime = self.reason_for(&run)? == WakeReason::Downtime.name();
        if downtime && member.paused {
            self.end(run, RunStatus::Stopped, "its member was paused")?;
            return Ok(None);
        }
        Ok(Some(run))
    }

    /// Starts a placed turn: briefed, told how hard to try and answered by the brain it runs
    /// on, with its budget reserved. False when the runner could not start it.
    fn launch(&self, mut run: Run, member: &Member, placed: &Placement, now: Millis) -> Done<bool> {
        let Placement {
            brain,
            amount,
            kind,
        } = *placed;
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
        let context = RunContext {
            run: run.id,
            member: member.id,
            task: run.task,
            told: run.told.clone(),
            token: self.store.issue_token(run.id)?,
        };
        if let Err(why) = self.runner.start(&context) {
            let why = runs::never_started_said(&format!("could not start: {}", why.0));
            self.fail(run, &why, Fault::Worker)?;
            return Ok(false);
        }
        self.store
            .set_wake(run.wake, Receipt::Started, Some(run.id))?;
        self.emit(
            EventKind::RunStarted,
            json!({ "run": run.id.0, "member": member.id.0, "task": run.task.map(|t| t.0),
                    "reserved": amount, "reason": self.reason_for(&run)?,
                    "kind": kind.label(), "brain": brain.name() }),
        )?;
        Ok(true)
    }

    // ---- the team protocol (spec section 6) ------------------------------------------------

    /// The turn a token was issued for, while it runs: its run and its member. A token for a
    /// turn that has ended opens nothing.
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
    /// cannot fill the owner's desk. Whatever reaches the desk lists the container-like names
    /// the home lab does not have (see [`review::checked`]).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a turn with no task; [`EngineError::Refused`] with
    /// [`Refusal::TurnOver`] for a turn that has ended, [`Refusal::AlreadyAsked`] while its
    /// last question waits, or [`Refusal::Answered`] once the owner has answered a question
    /// about the item in words; a store failure.
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
        let task = turn
            .task
            .ok_or(EngineError::Unknown("task for this turn"))?;
        // Once answered in words, the item's answer is acted on, not asked for again (ADR 0142,
        // amended 2026-11-01).
        self.not_asked_again(task)?;
        // There is no chain: a question goes to the owner's desk (ADR 0142).
        // Climb 2, S1 (round 36): a question carries no attached receipts; the same ask across
        // tasks is matched on its words, and attachments would split one card into two.
        let text = self.with_evidence(&text, cited)?;
        let checked = review::checked(&text, lab.as_deref());
        self.ask_held_lock(run, "question", &|name| format!("{name} asks: {checked}"))
            .map(Asked::Desk)
    }

    /// A question with the evidence it cites under it, by id.
    fn with_evidence(&self, text: &str, cited: &[crate::domain::Ref]) -> Done<String> {
        if cited.is_empty() {
            return Ok(text.to_owned());
        }
        Ok(format!("{text}\nEvidence: {}", self.evidence_said(cited)?))
    }

    /// Whether a question or an approval from `member` about `task` still waits on the desk.
    fn waiting_question(&self, member: MemberId, task: TaskId) -> Done<bool> {
        for d in self.store.desk()? {
            if !matches!(d.kind.as_str(), "question" | "approval") || d.status != "waiting" {
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
        // One question about an item waits at a time, and the same question from another task
        // is one card; an approval never is one with another (ADR 0124; ADR 0142, amended
        // 2026-10-31).
        let desk = self.put_on_the_desk(kind, task, Some(member.id), &say(&member.name), true)?;
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
        self.decide_held_lock(&found, decision, "")?;
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
        self.decide_held_lock(&found, "seen", text)?;
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
        self.decide_held_lock(&found, decision, &words)?;
        self.answer_the_same_asks(item, decision, &words)
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

    fn decide_held_lock(&self, found: &DeskItem, decision: &str, words: &str) -> Done<()> {
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
                let to = task.holder();
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
                // In the loops the card is the item's: whoever holds it now reads the answer,
                // and a blocked item is woken only by the owner's words (ADR 0142, amended
                // 2026-10-31).
                let task = self.task(found.task.ok_or(EngineError::Unknown("task"))?)?;
                // Answered in words, the item is not asked about again (ADR 0142, amended
                // 2026-11-01).
                self.keep_the_answer(task.id, words)?;
                let holder = task.holder();
                let to = if self.loop_holds(&task)? {
                    holder.or(found.member)
                } else {
                    found.member.or(holder)
                };
                let wordless =
                    self.waits_for_the_owner(task.id, WakeReason::ProposalDecided, Some(words))?;
                if let (Some(member), false) = (to, wordless) {
                    self.wake_held_lock(member, Some(task.id), WakeReason::ProposalDecided)?;
                }
            }
            // Approved: the node runs exactly what it stored, then tells what came of it, and
            // only that wakes the member (ADR 0108).
            ("approval", "approve") => {}
            ("approval", "decline") => {
                self.settle_unrun(found, "The owner declined this, and it was not run")?;
            }
            // The wing (ADR 0115): a branch to push.
            ("push", "approve" | "decline") => self.decide_push(found, decision == "approve")?,
            // The runaway tripwire's card (ADR 0133): resume the team, which answers the card
            // itself, or leave it stopped and have the card seen.
            ("tripwire", "resume") => return self.resume_held_lock(),
            ("tripwire", "seen" | "dismiss") => {}
            // A branch the privacy scan held (ADR 0115, amended 2026-10-31): back to the team,
            // or pushed anyway on the owner's word.
            ("privacy", "send_back" | "push_anyway") => {
                self.decide_held(found, decision == "push_anyway")?;
            }
            ("push" | "notice" | "needs_owner" | "privacy", "seen" | "dismiss")
            | ("ready_to_merge", "merged" | "not_now")
            | ("ready_to_read", "read" | "not_now")
            | ("question", "dismiss") => {}
            _ => return Err(EngineError::Unknown("decision for this item")),
        }
        self.store.set_desk_status(item, decision)?;
        self.emit(
            EventKind::DeskDecided,
            json!({ "item": item.0, "kind": found.kind, "decision": decision }),
        )?;
        Ok(())
    }

    /// Stranded work's one nudge: what it waited on and was dropped is said in its thread;
    /// work in review asks its review again, other work wakes its member, recorded before it
    /// is sent and never sent again unchanged (ADR 0124). True when a nudge went out.
    fn nudge_stranded(&self, task: &Task, member: MemberId, dropped: Option<&Task>) -> Done<bool> {
        self.store.set_nudged(task.id, true)?;
        if let Some(gone) = dropped {
            self.post_held_lock(
                Author::Endora,
                task.id,
                &format!(
                    "#{} ({}), which this waited on, was dropped: decide whether this still \
                     needs doing, and say so here, or ask.",
                    gone.id, gone.title
                ),
            )?;
        }
        if task.status == TaskStatus::InReview {
            self.review_asked(task)?;
            return Ok(true);
        }
        self.nudge_once(task, member, WakeReason::AssignmentRecovery)
    }

    /// Whether `task` is `own` or sits under it.
    fn under(&self, task: TaskId, own: TaskId) -> Done<bool> {
        let mut at = Some(task);
        while let Some(id) = at {
            if id == own {
                return Ok(true);
            }
            at = self.task(id)?.parent;
        }
        Ok(false)
    }

    /// A turn that has nowhere it may run says why, once.
    fn say_why_it_waits(&self, run: &mut Run, said: String) -> Done<()> {
        if run.end_reason == said {
            return Ok(());
        }
        run.end_reason = said;
        Ok(self.store.save_run(run)?)
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
            let Some(member) = task.holder() else {
                continue;
            };
            if members.iter().any(|m| m.id == member && !m.at_work()) {
                continue;
            }
            // A blocked item in the loops waits for the owner, not for a nudge: item 938 woke
            // Kit about every eleven minutes this way (ADR 0142, amended 2026-10-31).
            if self.waits_for_the_owner(task.id, WakeReason::Recovery, None)? {
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
            // Still waiting on work that will happen (what it waits for, or its open sub-tasks),
            // or with a check-back or an answer to come: held on purpose, and what it waits for
            // is its wake (ADR 0114, 0124). Waiting on work that was dropped: parked, and its
            // member is told why. What HQ cannot tell about, its lead looks at once.
            let Some(found) = self.diagnosis(task.id)? else {
                continue;
            };
            match found.stopped {
                Stopped::Working | Stopped::Held | Stopped::Indeterminate => continue,
                Stopped::Parked => {}
            }
            let dropped = self.dropped_wait(&task.waits_on)?;
            if !task.nudged {
                nudged += usize::from(self.nudge_stranded(task, member, dropped.as_ref())?);
                continue;
            }
            // In the loops (ADR 0142) stranded work is one of the item's failures.
            if self.loop_holds(task)? {
                self.loop_stalled(task)?;
                stuck += 1;
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
        self.quiet_in_the_loops(desk)?;
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
    /// Whether a task is a check its member closes itself: a routine's (ADR 0107). A check
    /// changes no code.
    fn is_a_check(&self, task: TaskId) -> Done<bool> {
        Ok(self.routine_of(task)?.is_some())
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
        let to = task.holder().unwrap_or(routine.assignee);
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
            let to = task.holder().unwrap_or(routine.assignee);
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
        // What done means for it (ADR 0114, 0124): the home lab's check is the lab as it is now,
        // and any other routine's is a reading of what it is about, cited.
        let criteria = routines::criteria_for(routine.on_lab_trouble, false, &routine.title);
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
    /// one of HQ's own templates (`title`, `description`) the composition root names. They are recorded as HQ's; every other
    /// stays unknown. Returns how many were.
    pub fn vouch_template_routines(&self, also: &[(&str, &str)]) -> Done<usize> {
        let _held = self.held();
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
            let template = also.iter().any(|(title, words)| {
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

    /// A setting kept as JSON, read as `T`; `None` when it is not set or does not read.
    fn setting_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Done<Option<T>> {
        Ok(self
            .store
            .setting(key)?
            .and_then(|s| serde_json::from_str(&s).ok()))
    }

    // ---- what the office shows ------------------------------------------------------------

    /// One task whole, as the owner opens it: its whole thread, evidence and documents.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for no such task; a store failure.
    pub fn task_shown(&self, id: TaskId) -> Done<serde_json::Value> {
        let tasks = self.store.tasks()?;
        let task = tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or(EngineError::Unknown("task"))?;
        let goals = self.store.goals()?;
        let routines = self.store.routines()?;
        let board = self.board(&tasks, &goals, &routines)?;
        self.task_row(task, &board, true)
    }

    /// What every task row is read against, read once for the whole board, not once a row.
    fn board<'a>(
        &self,
        tasks: &'a [Task],
        goals: &'a [crate::domain::Goal],
        routines: &'a [crate::domain::Routine],
    ) -> Done<Board<'a>> {
        Ok(Board {
            tasks,
            goals,
            routines,
            branches: self.store.branches()?,
            checks: self.store.checks()?,
            posts: self.store.post_counts()?,
        })
    }

    /// A task as the screen shows it. Finished work comes in brief, with no thread, evidence,
    /// documents, description or chain of why (those of 800 done tasks made the snapshot 1.6 MB on every poll); the
    /// owner's screen asks for it `whole` when it is opened.
    fn task_row(&self, t: &Task, board: &Board<'_>, whole: bool) -> Done<serde_json::Value> {
        let mut row = json!({
            "id": t.id.0, "goal": t.goal.0, "project": t.project.map(|p| p.0),
            "parent": t.parent.map(|p| p.0), "title": t.title,
            "owner": t.owner.map(|o| o.0),
            "assignee": t.assignee.map(|a| a.0), "status": t.status.name(),
            "priority": t.priority.name(), "depth": t.depth,
            "waits_on": t.waits_on.iter().map(|w| w.0).collect::<Vec<_>>(),
            "gate": self.store.gate(t.id)?,
            "routine": board.routines.iter().find(|r| r.last_task == Some(t.id)).map(|r| r.id.0),
            "posts": board.posts.get(&t.id).copied().unwrap_or(0), "brief": !whole,
            // The wing's branches on it (ADR 0115).
            "branches": wing::branches_shown(t.id, &board.branches, &board.checks),
            // Why it closed, and what carries it on (ADR 0124).
            "closure": self.store.closure(t.id)?.map(|c| json!({
                "why": c.closure.name(), "said": c.closure.plainly(),
                "duplicate_of": c.closure.duplicate_of().map(|d| d.0),
                "successor": c.successor.map(|s| s.0), "by": c.by, "at": c.at })),
        });
        if whole {
            let thread = self.store.thread(t.id)?;
            row["posts"] = json!(thread.len());
            row["description"] = json!(t.description);
            let mut chain = vec![t.title.clone()];
            let mut parent = t.parent;
            while let Some(up) = parent.and_then(|id| board.tasks.iter().find(|x| x.id == id)) {
                chain.push(up.title.clone());
                parent = up.parent;
            }
            if let Some(goal) = board.goals.iter().find(|g| g.id == t.goal) {
                chain.push(format!("Goal: {}", goal.title));
            }
            row["why"] = json!(chain);
            // Each post with its place in the thread, which the activity log finds it by.
            row["thread"] = thread
                .iter()
                .enumerate()
                .map(|(n, p)| {
                    json!({ "n": n, "author": p.author, "text": p.text,
                                      "at": p.at, "unsupported": p.unsupported })
                })
                .collect();
            // What done means, and what was cited for it at each stage (ADR 0114).
            row["evidence"] = self.evidence_shown(t)?;
            row["documents"] = self.documents_shown(t.id)?;
        }
        Ok(row)
    }

    /// Everything on screen now (spec section 12), and the id of the last event in it: the
    /// event stream resumes after that id, so nothing is shown twice or missed.
    pub fn snapshot(&self) -> Done<serde_json::Value> {
        let _held = self.held();
        let day = self.today();
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
        // Today's pool, sized once for the snapshot and its scorecard alike.
        let sized = self.pool_sized_today()?;
        let topped_up = self.topup_of(day)?;
        let scorecard = self.scorecard(&worked, &every_run, &tasks, day, sized.base, topped_up)?;
        let runs: Vec<Run> = every_run.into_iter().filter(|r| r.status.live()).collect();
        let goals = self.store.goals()?;
        let mut notes = std::collections::HashMap::new();
        for g in &goals {
            notes.insert(g.id.0, self.store.goal_note(g.id)?);
        }
        let pool_day = sized.base + topped_up;
        let all_members = self.store.members()?;
        let members = self.members_shown(&all_members, &runs, day, pool_day)?;
        let routine_list = self.store.routines()?;
        let board = self.board(&tasks, &goals, &routine_list)?;
        let mut task_rows = Vec::new();
        for t in &tasks {
            task_rows.push(self.task_row(t, &board, !t.status.finished())?);
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
            "pool": { "per_day": pool_day, "base_per_day": sized.base,
                      // Which figure today's base is and why (ADR 0113, amended 2026-10-31).
                      "sized": { "by": sized.sizing.name(), "why": sized.why(),
                                 "floor": sized.floor, "ceiling": sized.ceiling },
                      "topped_up": topped_up, "committed": pool_committed,
                      "spent": pool_spent, "held": (pool_committed - pool_spent).max(0),
                      // Turns of judgment that wait for budget (ADR 0131): none is dropped.
                      "waiting": runs.iter().filter(|r| r.status == RunStatus::Queued
                          && turns::is_waiting_for_budget(&r.end_reason)).count(),
                      // Turns of work that wait for the day to turn, their member's turns on
                      // the house's own models having shown nothing too long (ADR 0133).
                      "capped": runs.iter().filter(|r| r.status == RunStatus::Queued
                          && turns::is_waiting_for_cap(&r.end_reason)).count() },
            "members": members,
            "goals": goals.iter().map(|g| json!({
                "id": g.id.0, "title": g.title, "reason": g.reason, "status": g.status.name(),
                "parent": g.parent.map(|p| p.0), "note": notes.get(&g.id.0).cloned().unwrap_or_default(),
            })).collect::<Vec<_>>(),
            "tasks": task_rows,
            "runs": runs.iter().map(run_shown).collect::<Vec<_>>(),
            "routines": routine_list.iter().map(|r| routine_shown(r, &tasks)).collect::<Vec<_>>(),
            // What waits on the owner and needs their word (ADR 0132); the rest waits below.
            "desk": self.desk_shown()?,
            // Accepted work waiting to merge, and the owner's two settings for it (ADR 0115,
            // amended 2026-10-31): listed on Manage, never carded unless the cards are back.
            "accepted": self.accepted_shown()?,
            "delivery": self.delivery_shown()?,
            // "While you were away": stuck work, findings and notices, newest first (ADR 0132).
            "digest": self.digest_shown()?,
            "house_log": self.store.setting(HOUSE_LOG)?.unwrap_or_default(),
            // The team's world (ADR 0122): its pages by stage, and its stories.
            "world": self.world_shown()?,
            "map_lines": self.map_lines_shown()?,
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
            // The Holodeck's chess matches and the model league (ADR 0135).
            "holodeck": self.holodeck_shown()?,
            // The guards (ADR 0124): their thresholds, precision, ceremony and findings.
            // The owner's Mac as the team sees it (ADR 0130).
            "mac": self.mac_shown(&runs, &all_members),
        }))
    }

    /// The members as the screen shows them: what each thinks with, may be given, holds of
    /// the day's pool and is running now.
    fn members_shown(
        &self,
        members: &[Member],
        runs: &[Run],
        day: i64,
        pool_day: Micros,
    ) -> Done<Vec<serde_json::Value>> {
        let paid = self.paid_weights()?;
        let mut shown = Vec::new();
        for m in members {
            let committed = self.store.committed(Some(m.id), day)?;
            let spent = self.store.spent(Some(m.id), day)?;
            shown.push(json!({
                "id": m.id.0, "name": m.name, "look": m.look,
                "paused": m.paused, "slots": m.slots,
                // On furlough (ADR 0118): when it began, and when it ended if it has.
                "furloughed": m.furloughed(), "furloughed_at": m.furloughed_at,
                "recalled_at": m.recalled_at,
                // What it thinks with, and what the owner may choose (ADR 0109, 0142).
                "brain": m.brain.name(),
                "brains": Brain::all().into_iter()
                    .map(|b| json!({ "name": b.name(), "label": b.label() }))
                    .collect::<Vec<_>>(),
                "rank": m.brain.rank(),
                // What runs its turns (ADR 0117), and whether Claude Code may.
                "runtime": self.store.runtime_of(m.id)?.effective(m.brain).name(),
                "claude_code": crate::domain::Runtime::ClaudeCode.allowed_for(m.brain),
                // Only its judgment is paid: the pool is shared by the members' claims.
                "per_day": self.per_day(m, paid, pool_day),
                "committed": committed, "spent": spent, "held": (committed - spent).max(0),
                "running": runs.iter().filter(|r| r.member == m.id && r.status == RunStatus::Running)
                    .map(|r| json!({ "run": r.id.0, "task": r.task.map(|t| t.0) }))
                    .collect::<Vec<_>>(),
                // How often the CEO reached into its work this week (ADR 0124); null when the
                // pruned feed no longer holds the week, and HQ cannot tell.
            }));
        }
        Ok(shown)
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
    /// later than the window does. `every_run` is every turn, downtime's too: what each model
    /// cost counts every turn that ran, work or not. `pool_base` and `topped_up` are today's
    /// pool as the snapshot sized it.
    fn scorecard(
        &self,
        runs: &[Run],
        every_run: &[Run],
        tasks: &[Task],
        day: i64,
        pool_base: Micros,
        topped_up: Micros,
    ) -> Done<serde_json::Value> {
        let desk = self.store.desk()?;
        // A lead's report and patterns turns are not work on the scorecard (ADR 0120).
        let reports = self.report_task_ids()?;
        let events: Vec<crate::domain::Event> = self
            .store
            .events_of(&scorecard::KINDS_READ)?
            .into_iter()
            .filter(|e| {
                !e.data["task"]
                    .as_i64()
                    .is_some_and(|t| reports.contains(&TaskId(t)))
            })
            .collect();
        let kept_from = self.store.first_event_at()?;
        let record = scorecard::Record {
            runs,
            tasks,
            desk: &desk,
            events: &events,
            kept_from,
        };
        let offset = self.settings.offset_minutes;
        Ok(json!({
            "today": scorecard::score(&record, &scorecard::Window::day(day, offset)).json(),
            "week": scorecard::score(&record, &scorecard::Window::days_ending(day, 7, offset)).json(),
            // What the work came to, over the last day and week: landed changes, what one cost,
            // talk against work, interruptions and what waits (ADR 0132).
            "outcomes": self.outcomes_shown(runs, &events, kept_from)?,
            // Today's pool: the base day and the owner's top-up (ADR 0113 rule 6). The base is
            // sized from the Claude credit (amended 2026-10-31); a top-up is counted in floors.
            "pool": { "base": pool_base,
                      "floor": money::pool_a_day(self.settings.pool_per_month),
                      "topped_up": topped_up },
            // What each model cost, the owner's last seven days against the seven before (the
            // owner, 2026-10-31): shown on the Money card.
            "by_model": scorecard::by_model_shown(every_run, day, offset),
            // Each team's seats, at work and on furlough (ADR 0118).
            // Each area's measures, today and the last seven days (ADR 0120).
        }))
    }

    /// Keeps the newest `keep` events that are not ticks, and the ticks among the newest `keep`
    /// of all, and deletes the rest. Returns how many went.
    pub fn prune_events(&self, keep: i64) -> Done<usize> {
        let _held = self.held();
        Ok(self.store.prune_events(keep)?)
    }

    /// The events after `id`, for the stream.
    pub fn events_after(&self, id: i64) -> Done<Vec<crate::domain::Event>> {
        Ok(self.store.events_after(id)?)
    }

    /// The `limit` events just before an id (at most [`MOST_EVENTS_BEFORE`]), oldest first:
    /// how the activity log goes further back than what it was opened with.
    pub fn events_before(&self, id: i64, limit: usize) -> Done<Vec<crate::domain::Event>> {
        Ok(self
            .store
            .events_before(id, limit.min(MOST_EVENTS_BEFORE))?)
    }

    /// Every wake, with its receipt.
    pub fn wakes(&self) -> Done<Vec<Wake>> {
        Ok(self.store.wakes()?)
    }
}

/// The most one of `member`'s turns may spend: the dollar when it thinks with Opus (ADR 0142),
/// forty cents otherwise.
fn most_a_turn_for(member: &Member) -> Micros {
    money::most_a_turn_of(member.brain)
}
