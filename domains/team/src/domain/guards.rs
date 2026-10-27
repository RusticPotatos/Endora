//! Guards against how agent teams fail (ADR 0124), as pure rules over HQ's record.
//!
//! A team of models fails in a few known ways that no single turn shows: work that stops by
//! accident and is nudged for ever; talk about work that stands in for the work; a task
//! bounced between its worker and its checkers; a member woken again and again for nothing; a
//! claim that was wrong copied from note to note; proof polished after it was enough; a fence
//! pushed at by members who believe they should be outside it. Each guard here is a rule over
//! what HQ already keeps, never a model's opinion of itself.
//!
//! Every guard's number is in one table, [`THRESHOLDS`], so it can be tuned from how often a
//! finding was real ([`precision`]): a guard that never fires looks the same as one that works,
//! and a guard that fires on everything is worse than none.

use super::evidence::Label;
use super::money::Micros;
use super::work::{Millis, TaskStatus};

/// A minute, in milliseconds.
const MINUTE: Millis = 60_000;
/// A day, in milliseconds.
const DAY: Millis = 24 * 60 * MINUTE;

/// Every guard's threshold, in one place (ADR 0124): tuned from measured precision, never
/// scattered through the code that enforces them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    /// Turns in a row that showed nothing, each after a wake, before a member stops being woken
    /// on a task.
    pub redundant_wakes: usize,
    /// The most open sub-tasks one task may have.
    pub open_subtasks_most: usize,
    /// The deepest a sub-task may be made (the owner's task is depth 0).
    pub depth_most: u8,
    /// Re-checking turns, after a member was told its evidence is enough, before HQ submits it.
    pub sufficient_turns: usize,
    /// How alike (in percent of the retracted text's shingles found) a text must be to a
    /// retraction to match it.
    pub retracted_percent: usize,
    /// The fewest shingles a text must share with a retraction to match it.
    pub retracted_shared_least: usize,
    /// How often the tick looks for process-health findings.
    pub look_every_ms: Millis,
    /// How alike, both ways, a new question or decision for the owner must be to one waiting
    /// on the desk to be the same ask (in percent of each one's shingles the other holds).
    pub same_ask_percent: usize,
    /// The most turns a member starts in a day on the house's own models, the Mac or the NAS's
    /// (ADR 0133): they cost nothing, so no budget stops a member that has run away with them.
    /// Past it the member's work turns wait until the day turns.
    pub free_turns_a_day: usize,
    /// The most turns the whole team starts within [`Self::runaway_window_ms`] before HQ stops it
    /// itself (ADR 0133): the one that would be the next is a runaway, and trips the wire.
    pub runaway_turns: usize,
    /// How far back the tripwire counts the turns the team started.
    pub runaway_window_ms: Millis,
}

/// The thresholds HQ starts with (ADR 0124): loose enough to learn real numbers before they
/// bite, from each finding's verdict.
pub const THRESHOLDS: Thresholds = Thresholds {
    redundant_wakes: 3,
    open_subtasks_most: 5,
    depth_most: 3,
    sufficient_turns: 3,
    retracted_percent: 60,
    retracted_shared_least: 2,
    look_every_ms: 5 * MINUTE,
    same_ask_percent: 60,
    free_turns_a_day: 60,
    runaway_turns: 40,
    runaway_window_ms: 10 * MINUTE,
};

/// Whether a new ask for the owner (`new`, a fingerprint) is the same as one waiting
/// (`waiting`): each holds at least [`Thresholds::same_ask_percent`] of the other's shingles,
/// and at least [`Thresholds::retracted_shared_least`] of them (ADR 0124).
#[must_use]
pub fn same_ask(waiting: &[String], new: &[String]) -> bool {
    let one = super::retraction::likeness(waiting, new);
    let other = super::retraction::likeness(new, waiting);
    let least = THRESHOLDS.retracted_shared_least;
    one.shared >= least
        && one.percent >= THRESHOLDS.same_ask_percent
        && other.percent >= THRESHOLDS.same_ask_percent
}

/// What a guard refuses (ADR 0124), as the member or the owner reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardRefusal {
    /// A closing reason not in the set for the status it closes to.
    NotAClosure(TaskStatus),
    /// Work closed as superseded names the task that carries it on.
    NoSuccessor,
    /// A task is not its own successor.
    SuccessorIsItself(super::work::TaskId),
    /// A task is made to wait on work already finished: done, so there is nothing to wait for,
    /// or dropped, so what it waited for will not happen.
    WaitsOnFinished {
        /// The finished task.
        task: super::work::TaskId,
        /// How it finished.
        status: TaskStatus,
    },
    /// The parent already has as many open sub-tasks as one task may.
    TooManySubtasks {
        /// The parent.
        parent: super::work::TaskId,
        /// Its open sub-tasks.
        open: usize,
    },
    /// A sub-task would sit deeper than work may.
    TooDeep {
        /// The parent.
        parent: super::work::TaskId,
        /// How deep the sub-task would be.
        depth: u8,
    },
    /// A sub-task says which of its parent's criteria it serves.
    ServesWhich {
        /// The parent.
        parent: super::work::TaskId,
        /// The parent's criteria, in order.
        criteria: Vec<String>,
    },
    /// An approval asked for says which of its task's criteria it serves.
    ApprovalServesWhich {
        /// The task the turn is on.
        task: super::work::TaskId,
        /// Its criteria, in order.
        criteria: Vec<String>,
    },
    /// The task has no criteria, so an approval asked for has nothing to serve.
    ApprovalNothingToServe {
        /// The task the turn is on.
        task: super::work::TaskId,
    },
    /// What a member writes says again what the owner retracted.
    Retracted {
        /// What was retracted.
        target: String,
        /// On what day, as the owner reads it.
        on: String,
    },
    /// Not something the owner can retract: a note, a section or a story.
    NotRetractable(String),
}

impl core::fmt::Display for GuardRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let t = THRESHOLDS;
        match self {
            Self::NotAClosure(status) => write!(
                f,
                "work {} says why, one of: {} (a duplicate names the task it duplicates)",
                status.name(),
                super::closure::Closure::names(*status).join(", ")
            ),
            Self::NoSuccessor => f.write_str(
                "work done as superseded names the task that carries it on: give its number as \
                 `successor`",
            ),
            Self::SuccessorIsItself(task) => write!(
                f,
                "#{task} cannot be its own successor: name the task that carries the work on"
            ),
            Self::WaitsOnFinished { task, status } => {
                if *status == TaskStatus::Dropped {
                    return write!(
                        f,
                        "#{task} was dropped, so what a task waits on it for will not happen: \
                         leave it out of waits_on, or wait on the task that carries its work on"
                    );
                }
                write!(
                    f,
                    "#{task} is {} already, so there is nothing to wait for: leave it out of \
                     waits_on",
                    status.name()
                )
            }
            Self::TooManySubtasks { parent, open } => write!(
                f,
                "#{parent} already has {open} open sub-tasks, and {} is the most: finish or \
                 drop one, or add to one with post, before making another",
                t.open_subtasks_most
            ),
            Self::TooDeep { parent, depth } => write!(
                f,
                "a sub-task of #{parent} would be {depth} deep, and {} is the deepest work goes: \
                 do it in #{parent}, or ask its lead to plan it",
                t.depth_most
            ),
            Self::ServesWhich { parent, criteria } => {
                let listed: Vec<String> = criteria
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{}. {c}", i + 1))
                    .collect();
                write!(
                    f,
                    "which of #{parent}'s criteria does this serve? Give serves (its number or \
                     its words): {}. Every part of a split serves one of the criteria it splits, \
                     whoever asked for the task; a part that serves none is not part of it",
                    listed.join("; ")
                )
            }
            Self::ApprovalServesWhich { task, criteria } => {
                let listed: Vec<String> = criteria
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{}. {c}", i + 1))
                    .collect();
                write!(
                    f,
                    "which of #{task}'s criteria does asking the owner serve? Give serves (its \
                     number or its words): {}. A request that serves none of them never reaches \
                     the owner's desk",
                    listed.join("; ")
                )
            }
            Self::ApprovalNothingToServe { task } => write!(
                f,
                "#{task} has no criteria, so there is nothing for asking the owner to serve: a \
                 lead says what done means with narrow_criteria, and anyone else asks its lead \
                 in the task's thread, or asks the owner in words with ask_owner"
            ),
            Self::Retracted { target, on } => write!(
                f,
                "this says again what was retracted on {on} ({target}): the owner said it is \
                 not true. Do not write it again; if the record now shows otherwise, ask the \
                 owner"
            ),
            Self::NotRetractable(said) => write!(
                f,
                "`{said}` is not a note (note:12), a section (world:house#backups) or a story \
                 (story:a-handle)"
            ),
        }
    }
}

/// Whether one more sub-task fits under a parent with `open` open sub-tasks, `depth` deep
/// itself (ADR 0124): at most [`Thresholds::open_subtasks_most`] open under one task, and no
/// deeper than [`Thresholds::depth_most`].
///
/// # Errors
/// [`GuardRefusal::TooManySubtasks`] or [`GuardRefusal::TooDeep`], with the counts.
pub fn room_for_a_subtask(
    parent: super::work::TaskId,
    open: usize,
    depth: u8,
) -> Result<(), GuardRefusal> {
    if open >= THRESHOLDS.open_subtasks_most {
        return Err(GuardRefusal::TooManySubtasks { parent, open });
    }
    let deeper = depth.saturating_add(1);
    if deeper > THRESHOLDS.depth_most {
        return Err(GuardRefusal::TooDeep {
            parent,
            depth: deeper,
        });
    }
    Ok(())
}

/// Which of `criteria` (counted from 1) `said` names: its number (`2`, `#2`, `criterion 2`)
/// or its words, case and spacing aside. `None` when it names none of them.
#[must_use]
pub fn serves(criteria: &[super::evidence::Criterion], said: &str) -> Option<usize> {
    let squeezed = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let said = squeezed(said);
    let number = said
        .trim_start_matches("criterion")
        .trim()
        .trim_start_matches('#')
        .trim()
        .parse::<usize>()
        .ok();
    if let Some(n) = number.filter(|n| (1..=criteria.len()).contains(n)) {
        return Some(n);
    }
    if said.is_empty() {
        return None;
    }
    criteria
        .iter()
        .position(|c| squeezed(&c.text) == said)
        .map(|i| i + 1)
}

/// Which of its task's `criteria` (counted from 1) a request for the owner's yes serves, by
/// number or words as [`serves`] reads them (ADR 0124, amended): a request that serves none, or
/// for a task with no criteria at all, never reaches the owner's desk. Live, a developer on the
/// house's small model asked the owner to approve a web map search "to find the NAS backup
/// location", with coordinates it had invented; it served nothing its task asked for.
///
/// # Errors
/// [`GuardRefusal::ApprovalNothingToServe`] for a task with no criteria,
/// [`GuardRefusal::ApprovalServesWhich`] (listing them) for a `said` that names none.
pub fn approval_serves(
    task: super::work::TaskId,
    criteria: &[super::evidence::Criterion],
    said: Option<&str>,
) -> Result<usize, GuardRefusal> {
    if criteria.is_empty() {
        return Err(GuardRefusal::ApprovalNothingToServe { task });
    }
    said.and_then(|said| serves(criteria, said))
        .ok_or_else(|| GuardRefusal::ApprovalServesWhich {
            task,
            criteria: criteria.iter().map(|c| c.text.clone()).collect(),
        })
}

/// What a member whose evidence already meets its task's criteria is told (ADR 0124): submit
/// now, with a fresh reading first for any live criterion.
#[must_use]
pub const fn enough_said(live: bool) -> &'static str {
    if live {
        return "Your evidence already meets the criteria: submit now (set_status in_review, \
                citing it), with one fresh reading this turn for each live criterion. More \
                checking of the same is not more evidence.";
    }
    "Your evidence already meets the criteria: submit now (set_status in_review, citing it). \
     More checking of the same is not more evidence."
}

/// The two windows the process-health detectors look over: a day, and a week.
pub const WINDOWS: [(Millis, &str); 2] = [(DAY, "24 h"), (7 * DAY, "7 d")];

/// How far back the CEO's stepping in on a member's work is counted: a week, the longest of the
/// guards' windows (ADR 0124, amended).
pub const RESCUE_WINDOW_MS: Millis = 7 * DAY;

// ---- the stopped diagnosis ------------------------------------------------------------------

/// Whether open work is moving, and if it stopped, whether on purpose (ADR 0124).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// A turn is queued, running or waiting to retry on it, or a wake is held for it.
    Working,
    /// Stopped by accident: its member is idle and it is still owed. Only this climbs the
    /// escalation ladder.
    Parked,
    /// Stopped on purpose: blocked with a named reason and something live that will bring it
    /// back (a check-back pending, an answer awaited, the work it waits on).
    Held,
    /// HQ cannot tell. Never taken for fine: the area's lead looks at it once.
    Indeterminate,
}

impl Stopped {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Parked => "parked",
            Self::Held => "held",
            Self::Indeterminate => "indeterminate",
        }
    }
}

/// The diagnosis of one open task: where it is, and why, in a few words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnosis {
    /// Where it is.
    pub stopped: Stopped,
    /// Why, in a few words a briefing or the screen can say.
    pub why: &'static str,
}

/// What the diagnosis reads of one task, worked out from the record at the moment it is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskFacts {
    /// Where the task is.
    pub status: TaskStatus,
    /// Its holder, if anyone holds it (taken or handed to): whether that member is at work.
    pub holder_at_work: Option<bool>,
    /// A turn on it is running, or queued and ready to start, or waiting to retry.
    pub live_turn: bool,
    /// A turn on it is queued but waits to start, for budget or for the Mac (ADR 0131): the
    /// wait is its wake. Work whose only turn waits is held, never parked.
    pub waiting_turn: bool,
    /// A wake for it is held behind another member's turn.
    pub held_wake: bool,
    /// It waits on open work: a task it waits for, or one of its sub-tasks.
    pub waits_on_open_work: bool,
    /// A task it waits on was dropped: that wait will never end by itself.
    pub wait_dropped: bool,
    /// A member said why it is blocked (a move to blocked carries a reason).
    pub blocked_reason: bool,
    /// Something live will bring it back: a pending check-back of its holder's, a question or
    /// approval of its holder's still waiting, a huddle or a ladder rung being decided.
    pub live_wake: bool,
    /// The newest turn on it failed for good, its retries spent: the runner stopped it, not its
    /// member. (A turn that ran out of time is its member's, and parks the work.)
    pub last_turn_failed: bool,
    /// Its holder was woken on it [`Thresholds::redundant_wakes`] times in a row for nothing.
    pub woken_for_nothing: bool,
}

/// The stopped diagnosis of an open task (ADR 0124): read-time and pure, never stored, so it
/// cannot go stale. `None` for finished work, which owes nothing.
#[must_use]
pub fn diagnose(facts: &TaskFacts) -> Option<Diagnosis> {
    let said = |stopped, why| Some(Diagnosis { stopped, why });
    if facts.status.finished() {
        return None;
    }
    if facts.live_turn || facts.held_wake {
        return said(Stopped::Working, "a turn on it is queued, running or held");
    }
    // A turn that waits for budget or for the Mac is a live wake: the work stopped on purpose,
    // not by accident, and nothing is nudged, found stuck or climbed while it does (ADR 0131).
    if facts.waiting_turn {
        return said(
            Stopped::Held,
            "its turn waits for budget or for the Mac, and starts when it can",
        );
    }
    let Some(at_work) = facts.holder_at_work else {
        return said(Stopped::Indeterminate, "nobody holds it");
    };
    if facts.last_turn_failed {
        return said(
            Stopped::Indeterminate,
            "its last turn failed: the runner stopped it, not its member",
        );
    }
    if facts.woken_for_nothing {
        return said(
            Stopped::Parked,
            "its member was woken on it again and again, and nothing came of it",
        );
    }
    if !at_work {
        return said(
            Stopped::Held,
            "its member is paused or on furlough, by the owner",
        );
    }
    if facts.wait_dropped {
        return said(Stopped::Parked, "what it waited on was dropped");
    }
    if facts.waits_on_open_work {
        return said(Stopped::Held, "it waits on open work, whose end wakes it");
    }
    let named = facts.status != TaskStatus::Blocked || facts.blocked_reason;
    if facts.live_wake && named {
        return said(
            Stopped::Held,
            "a check-back or an answer it waits for will bring it back",
        );
    }
    if facts.live_wake {
        return said(
            Stopped::Parked,
            "it is blocked, and nobody said on what: a wake alone does not hold it",
        );
    }
    said(Stopped::Parked, "its member is idle and it is still owed")
}

/// The key a recovery wake is recorded under before it is sent (ADR 0124): the task, the
/// member woken, why, and what HQ saw of the task. The same key is never sent twice: only
/// something new on the task (a post, a move, evidence, another wake) makes a new one.
#[must_use]
pub fn nudge_key(task: i64, member: i64, reason: &str, saw: &str) -> String {
    format!("{task}:{member}:{reason}:{saw}")
}

// ---- process health ---------------------------------------------------------------------------

/// What one ended turn did, as the outcomes line reads it from the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnSeen {
    /// When it ended.
    pub ended_at: Millis,
    /// Its label.
    pub label: Label,
    /// It was woken to test or accept work (a review round), or for a huddle.
    pub checking: bool,
    /// An act of its succeeded.
    pub acted: bool,
    /// It submitted a branch.
    pub branch: bool,
    /// Evidence it cited was judged to meet a criterion.
    pub criterion_met: bool,
    /// It took a reading nobody on the task had taken before.
    pub new_evidence: bool,
    /// It put a question up the chain or to the owner.
    pub asked: bool,
    /// What it spent, as the ledger kept it (ADR 0133): a finding says what the talk cost.
    pub spent: Micros,
}

/// What a turn was, to the outcomes line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnKind {
    /// It changed the product: an act, a branch, a criterion met.
    Product,
    /// It coordinated: talk, a re-check with nothing new, a review round, a question.
    Coordination,
    /// Neither: an empty turn, a first reading, a move with nothing behind it.
    Neither,
}

/// What a turn was (ADR 0124): product change, coordination, or neither.
#[must_use]
pub const fn turn_kind(t: &TurnSeen) -> TurnKind {
    let product = t.acted || t.branch || t.criterion_met;
    if matches!(t.label, Label::Advanced) && product {
        return TurnKind::Product;
    }
    if t.checking {
        return TurnKind::Coordination;
    }
    match t.label {
        Label::TalkOnly => TurnKind::Coordination,
        Label::Checked if !t.new_evidence => TurnKind::Coordination,
        Label::Blocked if t.asked => TurnKind::Coordination,
        _ => TurnKind::Neither,
    }
}

// ---- findings and their verdicts ------------------------------------------------------------

/// The guards that make findings (ADR 0124; the ones that policed coordination went with the
/// company, ADR 0142).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Guard {
    /// Work stopped by accident, on the owner's desk.
    Stuck,
    /// The same ask for the owner from two tasks, put on one desk card.
    SameAsk,
}

impl Guard {
    /// Every guard, in the order the scorecard shows them.
    pub const ALL: [Self; 2] = [Self::Stuck, Self::SameAsk];

    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Stuck => "stuck",
            Self::SameAsk => "same_ask",
        }
    }

    /// The guard with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.name() == name)
    }

    /// What the owner reads it as.
    #[must_use]
    pub const fn plainly(self) -> &'static str {
        match self {
            Self::Stuck => "Stuck work",
            Self::SameAsk => "The same ask twice",
        }
    }

    /// How much more evidence than was judged makes a finding worth raising again (ADR 0132),
    /// in the units of its [`Finding::mark`]: one more of the thing counted, each of which is an
    /// event of its own.
    #[must_use]
    pub const fn step(self) -> i64 {
        1
    }

    /// The rule that fires it, as a finding states it, with its numbers from [`THRESHOLDS`].
    #[must_use]
    pub fn rule(self) -> String {
        match self {
            Self::Stuck => {
                "open work parked (its member idle, still owed) after its nudge".to_owned()
            }
            Self::SameAsk => format!(
                "a question or decision for the owner {}% alike, both ways, to one already waiting",
                THRESHOLDS.same_ask_percent
            ),
        }
    }
}

/// What the owner or the lead found a finding to be (ADR 0124).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The condition was real.
    Real,
    /// It was not: the guard fired on something healthy.
    FalsePositive,
    /// The record does not say either way.
    NotEnough,
    /// It was real, and it has been put right.
    Resolved,
}

impl Verdict {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Real => "real",
            Self::FalsePositive => "false_positive",
            Self::NotEnough => "not_enough",
            Self::Resolved => "resolved",
        }
    }

    /// The verdict with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Real,
            Self::FalsePositive,
            Self::NotEnough,
            Self::Resolved,
        ]
        .into_iter()
        .find(|v| v.name() == name)
    }
}

/// One guard finding, as HQ keeps it: what fired, on what, the rule and the evidence, and what
/// the owner or the lead said of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Its id; 0 before it is kept.
    pub id: i64,
    /// Which guard.
    pub guard: Guard,
    /// What it is about, as a key: one finding is open per key at a time.
    pub key: String,
    /// The task it is about, if one.
    pub task: Option<i64>,
    /// The project it is about, if one.
    pub project: Option<i64>,
    /// The rule that fired, in words.
    pub rule: String,
    /// What it fired on, in words, with the record's numbers.
    pub evidence: String,
    /// When it opened.
    pub opened_at: Millis,
    /// When its condition cleared, if it has.
    pub cleared_at: Option<Millis>,
    /// What the owner or the lead found it to be, if anyone said.
    pub verdict: Option<Verdict>,
    /// Whether what was done about it worked, if anyone said.
    pub fix_worked: Option<bool>,
    /// Who gave the verdict: `owner` or a member's name.
    pub judged_by: String,
    /// The desk notice it put up, if it put one.
    pub desk: Option<i64>,
    /// When the condition was last seen to hold: when the finding opened, and again each time
    /// it was detected since (ADR 0132).
    pub seen_at: Millis,
    /// How many times it has opened: once, and once more each time the same finding opened again
    /// after it cleared (ADR 0132).
    pub episodes: u32,
    /// The evidence it rests on, as a count that only grows with new evidence (ADR 0132): what
    /// was judged is what stands here, so that a later detection can be told from the same
    /// evidence again. What it counts depends on the guard (see [`Guard::step`]).
    pub mark: i64,
}

/// What a guard's detection does to the finding for its key (ADR 0132). There is one finding for
/// a guard and what it is about, however often the guard sees it: a new detection refreshes the
/// finding that is open, opens again the one that cleared unjudged, and leaves alone one that was
/// judged until there is new evidence, so the owner is told once and the same thing is not raised
/// again and again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raised {
    /// No finding yet for this key, or the one that was judged has cleared and the evidence has
    /// grown since: a new finding.
    Open,
    /// A judged finding is still open and its evidence has grown: it closes, and a new one opens.
    Renew,
    /// A finding that cleared with nobody having judged it, seen again: the same one opens again.
    Reopen,
    /// The finding is open: its evidence and time are refreshed, and nothing else happens.
    Refresh,
    /// The finding was judged and there is nothing new since: nothing happens.
    Hold,
}

impl Raised {}

/// What a detection with this `mark` does to the key's `latest` finding (ADR 0132); `step` is
/// how much the mark must have grown past the judged finding's for the evidence to count as new
/// ([`Guard::step`]).
#[must_use]
pub fn reconcile(latest: Option<&Finding>, mark: i64, step: i64) -> Raised {
    let Some(latest) = latest else {
        return Raised::Open;
    };
    // What was judged is what the finding's mark stands at (it is not refreshed after), so the
    // evidence is new when it is a step beyond that.
    let new = mark >= latest.mark.saturating_add(step.max(1));
    match (latest.cleared_at.is_some(), latest.verdict.is_some()) {
        (false, false) => Raised::Refresh,
        (false, true) if new => Raised::Renew,
        (false, true) => Raised::Refresh,
        (true, false) => Raised::Reopen,
        (true, true) if new => Raised::Open,
        (true, true) => Raised::Hold,
    }
}

/// How one guard has done: how often it fired, and how often it was right (ADR 0124).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Precision {
    /// Which guard.
    pub guard: Guard,
    /// Findings it made.
    pub fired: usize,
    /// Of those, still open.
    pub open: usize,
    /// Judged real.
    pub real: usize,
    /// Judged real and put right.
    pub resolved: usize,
    /// Judged a false positive.
    pub false_positive: usize,
    /// Judged as not enough to say.
    pub not_enough: usize,
    /// Of those whose fix was judged, the ones that worked.
    pub fixed: usize,
    /// Of those, how many fixes were judged at all.
    pub fix_judged: usize,
}

impl Precision {
    /// Real (or resolved) out of those judged real, resolved or a false positive, in whole
    /// percent; `None` until one is. "Not enough to say" is no verdict on the guard.
    #[must_use]
    pub fn percent(&self) -> Option<usize> {
        let real = self.real + self.resolved;
        let judged = real + self.false_positive;
        (judged > 0).then(|| real * 100 / judged)
    }
}

/// Each guard's precision, over its findings.
#[must_use]
pub fn precision(findings: &[Finding]) -> Vec<Precision> {
    Guard::ALL
        .into_iter()
        .map(|guard| {
            let mine: Vec<&Finding> = findings.iter().filter(|f| f.guard == guard).collect();
            let with = |v: Verdict| mine.iter().filter(|f| f.verdict == Some(v)).count();
            Precision {
                guard,
                fired: mine.len(),
                open: mine.iter().filter(|f| f.cleared_at.is_none()).count(),
                real: with(Verdict::Real),
                resolved: with(Verdict::Resolved),
                false_positive: with(Verdict::FalsePositive),
                not_enough: with(Verdict::NotEnough),
                fixed: mine.iter().filter(|f| f.fix_worked == Some(true)).count(),
                fix_judged: mine.iter().filter(|f| f.fix_worked.is_some()).count(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle(status: TaskStatus) -> TaskFacts {
        TaskFacts {
            status,
            holder_at_work: Some(true),
            live_turn: false,
            waiting_turn: false,
            held_wake: false,
            waits_on_open_work: false,
            wait_dropped: false,
            blocked_reason: false,
            live_wake: false,
            last_turn_failed: false,
            woken_for_nothing: false,
        }
    }

    fn stopped(facts: TaskFacts) -> Stopped {
        diagnose(&facts).expect("open work").stopped
    }

    #[test]
    fn work_with_a_turn_is_working_and_finished_work_owes_nothing() {
        let running = TaskFacts {
            live_turn: true,
            ..idle(TaskStatus::Doing)
        };
        assert_eq!(stopped(running), Stopped::Working);
        let held = TaskFacts {
            held_wake: true,
            ..idle(TaskStatus::Todo)
        };
        assert_eq!(stopped(held), Stopped::Working);
        assert_eq!(diagnose(&idle(TaskStatus::Done)), None);
        assert_eq!(diagnose(&idle(TaskStatus::Dropped)), None);
    }

    /// A turn that waits for budget or for the Mac is a live wake (ADR 0131): work whose only
    /// turn waits is held, however long, and whatever its member's status, and is never parked
    /// for the ladder to climb. A turn that runs beside it makes the work simply working.
    #[test]
    fn work_whose_turn_waits_for_budget_or_the_mac_is_held_never_parked() {
        for status in [
            TaskStatus::Todo,
            TaskStatus::Doing,
            TaskStatus::Blocked,
            TaskStatus::InReview,
        ] {
            let waiting = TaskFacts {
                waiting_turn: true,
                ..idle(status)
            };
            let found = diagnose(&waiting).expect("open work");
            assert_eq!(found.stopped, Stopped::Held, "{status:?}");
            assert!(found.why.contains("waits for budget or for the Mac"));
        }
        let and_running = TaskFacts {
            waiting_turn: true,
            live_turn: true,
            ..idle(TaskStatus::Doing)
        };
        assert_eq!(stopped(and_running), Stopped::Working);
        // Without the wait it is the accident it was.
        assert_eq!(stopped(idle(TaskStatus::Doing)), Stopped::Parked);
        // Finished work owes nothing, a turn waiting for it or not.
        let finished = TaskFacts {
            waiting_turn: true,
            ..idle(TaskStatus::Done)
        };
        assert_eq!(diagnose(&finished), None);
    }

    #[test]
    fn an_idle_member_with_work_still_owed_has_parked_it() {
        for status in [TaskStatus::Todo, TaskStatus::Doing, TaskStatus::InReview] {
            assert_eq!(stopped(idle(status)), Stopped::Parked, "{status:?}");
        }
    }

    /// Stopped on purpose is blocked with a reason and something live to bring it back; a
    /// blocked task whose wake was spent, or that nobody said why it is blocked, is parked.
    #[test]
    fn blocked_with_a_reason_and_a_live_wake_is_held_and_without_either_parked() {
        let held = TaskFacts {
            blocked_reason: true,
            live_wake: true,
            ..idle(TaskStatus::Blocked)
        };
        assert_eq!(stopped(held), Stopped::Held);
        let spent = TaskFacts {
            blocked_reason: true,
            ..idle(TaskStatus::Blocked)
        };
        assert_eq!(stopped(spent), Stopped::Parked, "its wake was spent");
        let unnamed = TaskFacts {
            live_wake: true,
            ..idle(TaskStatus::Blocked)
        };
        assert_eq!(stopped(unnamed), Stopped::Parked, "nobody said on what");
        let planned = TaskFacts {
            live_wake: true,
            ..idle(TaskStatus::Doing)
        };
        assert_eq!(
            stopped(planned),
            Stopped::Held,
            "its member's own check-back"
        );
    }

    #[test]
    fn a_wait_on_open_work_holds_it_and_a_dropped_wait_parks_it() {
        let waiting = TaskFacts {
            waits_on_open_work: true,
            ..idle(TaskStatus::Blocked)
        };
        assert_eq!(stopped(waiting), Stopped::Held);
        let dropped = TaskFacts {
            wait_dropped: true,
            waits_on_open_work: true,
            ..idle(TaskStatus::Blocked)
        };
        assert_eq!(stopped(dropped), Stopped::Parked);
    }

    /// Unknown is a value: what HQ cannot tell is never taken for fine.
    #[test]
    fn what_hq_cannot_tell_is_indeterminate_never_fine() {
        let nobody = TaskFacts {
            holder_at_work: None,
            ..idle(TaskStatus::Todo)
        };
        assert_eq!(stopped(nobody), Stopped::Indeterminate);
        let failed = TaskFacts {
            last_turn_failed: true,
            ..idle(TaskStatus::Doing)
        };
        assert_eq!(stopped(failed), Stopped::Indeterminate);
        let paused = TaskFacts {
            holder_at_work: Some(false),
            ..idle(TaskStatus::Doing)
        };
        assert_eq!(
            stopped(paused),
            Stopped::Held,
            "the owner paused its member"
        );
    }

    #[test]
    fn a_member_woken_again_and_again_for_nothing_has_parked_it_whatever_it_waits_on() {
        let woken = TaskFacts {
            woken_for_nothing: true,
            live_wake: true,
            blocked_reason: true,
            ..idle(TaskStatus::Blocked)
        };
        assert_eq!(stopped(woken), Stopped::Parked);
    }

    #[test]
    fn a_nudge_is_keyed_by_what_it_saw() {
        let a = nudge_key(4, 2, "recovery", "doing|posts:3");
        assert_eq!(a, "4:2:recovery:doing|posts:3");
        assert_ne!(a, nudge_key(4, 2, "recovery", "doing|posts:4"));
    }

    fn finding(guard: Guard, verdict: Option<Verdict>, fix: Option<bool>) -> Finding {
        Finding {
            id: 1,
            guard,
            key: "k".to_owned(),
            task: None,
            project: None,
            rule: guard.rule(),
            evidence: String::new(),
            opened_at: 0,
            cleared_at: verdict.map(|_| 1),
            verdict,
            fix_worked: fix,
            judged_by: String::new(),
            desk: None,
            seen_at: 0,
            episodes: 1,
            mark: 0,
        }
    }

    /// The finding for a key, open or cleared, judged or not, resting on `mark`.
    fn kept(open: bool, verdict: Option<Verdict>, mark: i64) -> Finding {
        Finding {
            cleared_at: (!open).then_some(50),
            mark,
            ..finding(Guard::Stuck, verdict, None)
        }
    }

    #[test]
    fn a_detection_with_no_finding_before_it_opens_one() {
        assert_eq!(reconcile(None, 4, 1), Raised::Open);
    }

    /// One finding is open for a key, and a new detection of it only refreshes it, whether the
    /// owner has judged it or not: it is not raised again while it stands.
    #[test]
    fn a_detection_while_a_finding_is_open_only_refreshes_it() {
        assert_eq!(reconcile(Some(&kept(true, None, 3)), 3, 1), Raised::Refresh);
        assert_eq!(
            reconcile(Some(&kept(true, None, 3)), 9, 1),
            Raised::Refresh,
            "nobody has looked yet: more evidence is just more evidence"
        );
        let resolved = kept(true, Some(Verdict::Resolved), 3);
        assert_eq!(reconcile(Some(&resolved), 3, 1), Raised::Refresh);
    }

    /// A judged finding that is still open is raised again once its evidence has grown by its
    /// guard's step past what was judged: not by one more turn of talk, but by a dozen.
    #[test]
    fn a_judged_finding_still_open_is_renewed_once_its_evidence_has_grown_by_its_step() {
        let judged = kept(true, Some(Verdict::Real), 14);
        assert_eq!(
            reconcile(Some(&judged), 25, 12),
            Raised::Refresh,
            "eleven more turns of talk is not a dozen"
        );
        assert_eq!(reconcile(Some(&judged), 26, 12), Raised::Renew);
        let not_sure = kept(true, Some(Verdict::NotEnough), 2);
        assert_eq!(reconcile(Some(&not_sure), 2, 1), Raised::Refresh);
        assert_eq!(reconcile(Some(&not_sure), 3, 1), Raised::Renew);
    }

    /// A finding that cleared with nobody having judged it, and is seen again, is the same
    /// finding opening again: one finding for the task, however often it stops and starts.
    #[test]
    fn a_finding_that_cleared_unjudged_opens_again_as_itself() {
        assert_eq!(reconcile(Some(&kept(false, None, 3)), 3, 1), Raised::Reopen);
        assert_eq!(
            reconcile(Some(&kept(false, None, 3)), 1, 1),
            Raised::Reopen,
            "whatever the evidence is now"
        );
    }

    /// The owner judged it, and it cleared: it is not raised again for the same evidence, and is
    /// for new.
    #[test]
    fn a_finding_judged_and_cleared_stays_quiet_until_its_evidence_has_grown_by_its_step() {
        for verdict in [
            Verdict::Real,
            Verdict::FalsePositive,
            Verdict::NotEnough,
            Verdict::Resolved,
        ] {
            let done = kept(false, Some(verdict), 14);
            assert_eq!(reconcile(Some(&done), 14, 12), Raised::Hold, "{verdict:?}");
            assert_eq!(reconcile(Some(&done), 25, 12), Raised::Hold, "{verdict:?}");
            assert_eq!(reconcile(Some(&done), 26, 12), Raised::Open, "{verdict:?}");
        }
    }

    /// Talk is coordination, by the turns' labels: a turn that took
    /// a first reading, or nothing, is neither; a review round and a re-check are.
    #[test]
    fn only_coordination_turns_are_talk() {
        let now = 10 * DAY;
        let at = |label: Label| TurnSeen {
            ended_at: now - 1_000,
            ..seen(label)
        };
        let turns = [
            at(Label::Empty),
            TurnSeen {
                new_evidence: true,
                ..at(Label::Checked)
            },
            at(Label::Advanced),
            TurnSeen {
                checking: true,
                ..at(Label::Empty)
            },
            at(Label::Checked),
            TurnSeen {
                asked: true,
                ..at(Label::Blocked)
            },
            at(Label::TalkOnly),
        ];
        let talk = turns
            .iter()
            .filter(|t| turn_kind(t) == TurnKind::Coordination)
            .count();
        assert_eq!(talk, 4);
    }

    #[test]
    fn a_guards_precision_is_real_out_of_judged_and_not_enough_is_no_verdict() {
        let all = [
            finding(Guard::SameAsk, Some(Verdict::Real), Some(true)),
            finding(Guard::SameAsk, Some(Verdict::Resolved), Some(false)),
            finding(Guard::SameAsk, Some(Verdict::FalsePositive), None),
            finding(Guard::SameAsk, Some(Verdict::NotEnough), None),
            finding(Guard::SameAsk, None, None),
            finding(Guard::Stuck, None, None),
        ];
        let p = precision(&all);
        let same = p.iter().find(|p| p.guard == Guard::SameAsk).unwrap();
        assert_eq!((same.fired, same.open), (5, 1));
        assert_eq!(same.percent(), Some(66));
        assert_eq!((same.fixed, same.fix_judged), (1, 2));
        let stuck = p.iter().find(|p| p.guard == Guard::Stuck).unwrap();
        assert_eq!(stuck.percent(), None, "nothing judged yet");
        assert_eq!(p.len(), Guard::ALL.len());
        for g in Guard::ALL {
            assert_eq!(Guard::from_name(g.name()), Some(g));
            assert!(!g.rule().is_empty());
        }
        for v in ["real", "false_positive", "not_enough", "resolved"] {
            assert_eq!(Verdict::from_name(v).map(Verdict::name), Some(v));
        }
    }

    fn seen(label: Label) -> TurnSeen {
        TurnSeen {
            ended_at: 100,
            label,
            checking: false,
            acted: false,
            branch: false,
            criterion_met: false,
            new_evidence: false,
            asked: false,
            spent: 0,
        }
    }

    #[test]
    fn product_change_is_an_act_a_branch_or_a_criterion_met_and_talk_is_coordination() {
        let acted = TurnSeen {
            acted: true,
            ..seen(Label::Advanced)
        };
        assert_eq!(turn_kind(&acted), TurnKind::Product);
        let claimed = seen(Label::Advanced);
        assert_eq!(
            turn_kind(&claimed),
            TurnKind::Neither,
            "a move is not product"
        );
        assert_eq!(turn_kind(&seen(Label::TalkOnly)), TurnKind::Coordination);
        assert_eq!(turn_kind(&seen(Label::Checked)), TurnKind::Coordination);
        let first_look = TurnSeen {
            new_evidence: true,
            ..seen(Label::Checked)
        };
        assert_eq!(turn_kind(&first_look), TurnKind::Neither);
        let review = TurnSeen {
            checking: true,
            ..seen(Label::Advanced)
        };
        assert_eq!(turn_kind(&review), TurnKind::Coordination, "a review round");
        let asked = TurnSeen {
            asked: true,
            ..seen(Label::Blocked)
        };
        assert_eq!(turn_kind(&asked), TurnKind::Coordination);
        assert_eq!(turn_kind(&seen(Label::Blocked)), TurnKind::Neither);
        assert_eq!(turn_kind(&seen(Label::Empty)), TurnKind::Neither);
    }

    #[test]
    fn a_task_holds_five_open_sub_tasks_three_deep_and_says_so() {
        use super::super::work::TaskId;
        let p = TaskId(4);
        assert!(room_for_a_subtask(p, 4, 2).is_ok());
        let full = room_for_a_subtask(p, 5, 1).unwrap_err();
        assert_eq!(full, GuardRefusal::TooManySubtasks { parent: p, open: 5 });
        assert!(
            full.to_string()
                .contains("#4 already has 5 open sub-tasks, and 5 is the most")
        );
        let deep = room_for_a_subtask(p, 0, 3).unwrap_err();
        assert_eq!(
            deep,
            GuardRefusal::TooDeep {
                parent: p,
                depth: 4
            }
        );
        assert!(
            deep.to_string()
                .contains("would be 4 deep, and 3 is the deepest")
        );
    }

    #[test]
    fn a_sub_task_serves_a_criterion_named_by_number_or_words() {
        use super::super::evidence::Criterion;
        let c = [
            Criterion::new("The cause is found", false),
            Criterion::new("Sonarr is running", true),
        ];
        for said in [
            "2",
            "#2",
            "criterion 2",
            " Criterion  2 ",
            "sonarr is  RUNNING",
        ] {
            assert_eq!(serves(&c, said), Some(2), "{said:?}");
        }
        for said in ["", "3", "0", "sonarr", "the fix"] {
            assert_eq!(serves(&c, said), None, "{said:?}");
        }
        let asked = GuardRefusal::ServesWhich {
            parent: super::super::work::TaskId(4),
            criteria: c.iter().map(|k| k.text.clone()).collect(),
        }
        .to_string();
        assert!(
            asked.contains("which of #4's criteria does this serve?")
                && asked.contains("1. The cause is found; 2. Sonarr is running"),
            "{asked}"
        );
        // It applies to the owner's tasks and a routine's as to a member's (ADR 0124, amended),
        // and the refusal says so, with what a part that serves none is.
        assert!(
            asked.contains("whoever asked for the task")
                && asked.contains("a part that serves none is not part of it"),
            "{asked}"
        );
    }

    #[test]
    fn a_request_for_the_owners_yes_serves_a_criterion_of_its_task_or_is_refused_saying_which() {
        use super::super::evidence::Criterion;
        use super::super::work::TaskId;
        let c = [
            Criterion::new("The cause is found", false),
            Criterion::new("Sonarr is running", true),
        ];
        let task = TaskId(7);
        assert_eq!(approval_serves(task, &c, Some("2")), Ok(2));
        assert_eq!(
            approval_serves(task, &c, Some("the cause is  FOUND")),
            Ok(1),
            "by its words, case and spacing aside"
        );
        for said in [None, Some(""), Some("  "), Some("3"), Some("the moon base")] {
            let refused = approval_serves(task, &c, said).unwrap_err().to_string();
            assert!(
                refused.contains("which of #7's criteria does asking the owner serve?")
                    && refused.contains("1. The cause is found; 2. Sonarr is running")
                    && refused.contains("never reaches the owner's desk"),
                "{said:?}: {refused}"
            );
        }
        // A task with no criteria has nothing to serve, however the request is worded.
        let nothing = approval_serves(task, &[], Some("1")).unwrap_err();
        assert_eq!(nothing, GuardRefusal::ApprovalNothingToServe { task });
        let words = nothing.to_string();
        assert!(
            words.contains("#7 has no criteria")
                && words.contains("narrow_criteria")
                && words.contains("ask_owner"),
            "{words}"
        );
    }

    /// Live, 18 October 2026: the CEO put the same board decision on the desk twice, once a
    /// task, both checks exiting 127.
    #[test]
    fn the_same_ask_from_two_tasks_is_one_and_a_different_one_is_not() {
        use super::super::retraction::fingerprint;
        let first = fingerprint(
            "Decision: Install ansible-playbook, ansible-lint and yamllint on the dev node?\n\
             Recommendation: Yes.\nWhy: The check exited 127.\nEvidence: task 671",
        );
        let again = fingerprint(
            "Decision: Install ansible-playbook, ansible-lint and yamllint on the dev node?\n\
             Recommendation: Yes, once.\nWhy: The check exited 127.\nEvidence: task 672",
        );
        let other = fingerprint(
            "Decision: Give sonarr 2 GB of memory?\nRecommendation: Yes.\nWhy: It was killed \
             for memory three times.\nEvidence: task 690",
        );
        assert!(same_ask(&first, &again));
        assert!(!same_ask(&first, &other));
        assert!(
            !same_ask(&first, &fingerprint("Install yamllint?")),
            "a short ask inside a long one is not the same ask"
        );
        let short = fingerprint("Install yamllint on the dev node?");
        assert!(
            !same_ask(&short, &first),
            "nor is a long ask that holds a short one waiting: each must hold the other's"
        );
        assert!(
            !same_ask(&first, &short),
            "whichever of the two is waiting and which is new"
        );
    }

    /// The caps on a runaway (ADR 0133) start well above the live record of 17 and 18 October
    /// (201 turns in 39 hours between all the members) and well below the scripted demo's
    /// 1,539 turns in 3 h 40 min: sixty a member a day, and forty a team in ten minutes, which is
    /// four a minute and only turns that end at once can make.
    #[test]
    fn the_caps_on_a_runaway_start_where_adr_0133_says() {
        let t = THRESHOLDS;
        assert_eq!(t.free_turns_a_day, 60);
        assert_eq!((t.runaway_turns, t.runaway_window_ms), (40, 10 * MINUTE));
    }

    #[test]
    fn the_thresholds_start_where_adr_0124_says() {
        let t = THRESHOLDS;
        assert_eq!(t.redundant_wakes, 3);
        assert_eq!((t.open_subtasks_most, t.depth_most), (5, 3));
    }
}
