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
    /// Coordination turns per product change at which a project is talking about work
    /// rather than doing it.
    pub ceremony_ratio: usize,
    /// The fewest coordination turns in a window before the ratio means anything.
    pub ceremony_least: usize,
    /// Times a task is sent back, with no new evidence between, before it goes straight to a
    /// huddle.
    pub carousel_returns: usize,
    /// Turns in a row that showed nothing, each after a wake, before a member stops being woken
    /// on a task.
    pub redundant_wakes: usize,
    /// Refused acts in one turn that make a fence-push finding.
    pub fence_refusals_in_a_turn: usize,
    /// Different members refused the same act, within [`Self::fence_window_ms`], that make a
    /// fence-push finding.
    pub fence_members: usize,
    /// How far back the same refused act by different members is counted.
    pub fence_window_ms: Millis,
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
    /// The most coordination turns one task may take within [`Self::coordination_window_ms`] with
    /// nothing changing before the talk it asks for is held (ADR 0133): counted from the task's
    /// newest product change, or from its lead's last decision about it, if later. The verdicts on
    /// its findings tune it.
    pub coordination_budget: usize,
    /// How far back a task's coordination turns are counted against that budget.
    pub coordination_window_ms: Millis,
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
    ceremony_ratio: 4,
    ceremony_least: 12,
    carousel_returns: 3,
    redundant_wakes: 3,
    fence_refusals_in_a_turn: 3,
    fence_members: 2,
    fence_window_ms: DAY,
    open_subtasks_most: 5,
    depth_most: 3,
    sufficient_turns: 3,
    retracted_percent: 60,
    retracted_shared_least: 2,
    look_every_ms: 5 * MINUTE,
    same_ask_percent: 60,
    coordination_budget: 20,
    coordination_window_ms: DAY,
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

/// What one ended turn did, as the ceremony detector reads it from the record.
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

/// What a turn was, to the ceremony detector.
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

/// The ceremony of a project, or of a piece of work outside one, over one window: its
/// coordination turns, its product changes and what all its turns spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceremony {
    /// Coordination turns.
    pub coordination: usize,
    /// Turns that changed the product.
    pub product: usize,
    /// What every turn that ended in the window spent, whatever it did (ADR 0133).
    pub spent: Micros,
}

impl Ceremony {
    /// Whether the window is talk about work rather than work: at least
    /// [`Thresholds::ceremony_least`] coordination turns, and [`Thresholds::ceremony_ratio`]
    /// of them or more for each product change.
    #[must_use]
    pub const fn over(&self) -> bool {
        self.coordination >= THRESHOLDS.ceremony_least
            && self.coordination >= THRESHOLDS.ceremony_ratio * self.product
    }

    /// As the scorecard and a finding say it.
    #[must_use]
    pub fn said(&self, window: &str) -> String {
        format!(
            "{} coordination turns, {} product {} in {window}",
            self.coordination,
            self.product,
            if self.product == 1 {
                "change"
            } else {
                "changes"
            }
        )
    }

    /// As a finding says it: [`Self::said`], and what the window's turns spent (ADR 0133).
    #[must_use]
    pub fn said_with_cost(&self, window: &str) -> String {
        format!(
            "{} ({} spent)",
            self.said(window),
            super::projects::dollars(self.spent)
        )
    }
}

/// The ceremony of `turns` that ended within `ms` before `now`, and what every one of them
/// spent.
#[must_use]
pub fn ceremony(turns: &[TurnSeen], now: Millis, ms: Millis) -> Ceremony {
    let within = turns
        .iter()
        .filter(|t| t.ended_at <= now && now - t.ended_at < ms);
    let mut found = Ceremony {
        coordination: 0,
        product: 0,
        spent: 0,
    };
    for t in within {
        found.spent = found.spent.saturating_add(t.spent);
        match turn_kind(t) {
            TurnKind::Product => found.product += 1,
            TurnKind::Coordination => found.coordination += 1,
            TurnKind::Neither => {}
        }
    }
    found
}

/// The talk a task has had with nothing coming of it (ADR 0133): its coordination turns within
/// [`Thresholds::coordination_window_ms`] of `now` that came after its newest product change and
/// after `since`, when its lead last decided it. A turn that changed the product, or the lead's
/// deciding, starts the count again. `turns` are in the order they ran: which came after a change
/// is the order, not the clock, which two turns can share to the millisecond.
#[must_use]
pub fn talk_turns(turns: &[TurnSeen], now: Millis, since: Millis) -> Vec<TurnSeen> {
    let within: Vec<&TurnSeen> = turns
        .iter()
        .filter(|t| t.ended_at <= now && t.ended_at > since)
        .filter(|t| now - t.ended_at < THRESHOLDS.coordination_window_ms)
        .collect();
    let after_the_change = within
        .iter()
        .rposition(|t| turn_kind(t) == TurnKind::Product)
        .map_or(0, |changed| changed + 1);
    within[after_the_change..]
        .iter()
        .filter(|t| turn_kind(t) == TurnKind::Coordination)
        .map(|t| **t)
        .collect()
}

/// How many turns of [`talk_turns`] a task has had.
#[must_use]
pub fn talk_without_progress(turns: &[TurnSeen], now: Millis, since: Millis) -> usize {
    talk_turns(turns, now, since).len()
}

/// Whether that much talk is more than a task's budget (ADR 0133).
#[must_use]
pub const fn over_budget(talk: usize) -> bool {
    talk > THRESHOLDS.coordination_budget
}

/// Whether a task is on the review carousel (ADR 0124): sent back at least
/// [`Thresholds::carousel_returns`] times (`returns`, when, oldest first), with no new evidence
/// submitted between the last of them: every reference each submission (`submitted`, when
/// and what it cited) made between two returns had been cited before the first of the two.
#[must_use]
pub fn carousel(returns: &[Millis], submitted: &[(Millis, Vec<super::evidence::Ref>)]) -> bool {
    let most = THRESHOLDS.carousel_returns;
    if returns.len() < most {
        return false;
    }
    let last = &returns[returns.len() - most..];
    last.windows(2).all(|pair| {
        let (from, until) = (pair[0], pair[1]);
        let before: Vec<super::evidence::Ref> = submitted
            .iter()
            .filter(|(at, _)| *at <= from)
            .flat_map(|(_, refs)| refs.iter().copied())
            .collect();
        submitted
            .iter()
            .filter(|(at, _)| from < *at && *at <= until)
            .flat_map(|(_, refs)| refs.iter())
            .all(|r| before.contains(r))
    })
}

// ---- findings and their verdicts ------------------------------------------------------------

/// The guards that make findings (ADR 0124).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Guard {
    /// Work stopped by accident, on the escalation ladder or the owner's desk.
    Stuck,
    /// A project's coordination far outweighs its product change.
    Ceremony,
    /// A task sent back again and again with no new evidence between.
    Carousel,
    /// A member woken on a task again and again for nothing.
    RedundantWake,
    /// Members pushing at the fence: acts refused again and again.
    FencePush,
    /// The same ask for the owner from two tasks, put on one desk card.
    SameAsk,
    /// A task that has taken more coordination turns than its budget with nothing changing
    /// (ADR 0133). The one guard that changes what the team does, and so the one that is tried in
    /// shadow before it acts ([`GuardMode`]).
    Coordination,
}

impl Guard {
    /// Every guard, in the order the scorecard shows them.
    pub const ALL: [Self; 7] = [
        Self::Stuck,
        Self::Ceremony,
        Self::Carousel,
        Self::RedundantWake,
        Self::FencePush,
        Self::SameAsk,
        Self::Coordination,
    ];

    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Stuck => "stuck",
            Self::Ceremony => "ceremony",
            Self::Carousel => "carousel",
            Self::RedundantWake => "redundant_wake",
            Self::FencePush => "fence_push",
            Self::SameAsk => "same_ask",
            Self::Coordination => "coordination",
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
            Self::Ceremony => "Talk about work",
            Self::Carousel => "Review carousel",
            Self::RedundantWake => "Woken for nothing",
            Self::FencePush => "Pushing at the fence",
            Self::SameAsk => "The same ask twice",
            Self::Coordination => "Coordination over budget",
        }
    }

    /// Whether the guard is tried in shadow before it acts (ADR 0133): it records what it would
    /// have done and does nothing, until the owner switches it on. Only a guard that changes what
    /// the team does can be: the rest only record, and have nothing to hold back.
    #[must_use]
    pub const fn has_shadow(self) -> bool {
        matches!(self, Self::Coordination)
    }

    /// The mode a guard starts in: shadow for one that is tried first, on for the rest.
    #[must_use]
    pub const fn starts_in(self) -> GuardMode {
        if self.has_shadow() {
            return GuardMode::Shadow;
        }
        GuardMode::On
    }

    /// How much more evidence than was judged makes a finding worth raising again (ADR 0132),
    /// in the units of its [`Finding::mark`]: another [`Thresholds::ceremony_least`] turns of
    /// talk for the ceremony, whose count creeps up with every turn, and another
    /// [`Thresholds::coordination_budget`] for a task's budget of talk (ADR 0133), which creeps
    /// the same way; one more of the thing counted for the rest, each of which is an event of
    /// its own.
    #[must_use]
    pub const fn step(self) -> i64 {
        match self {
            Self::Ceremony => THRESHOLDS.ceremony_least as i64,
            Self::Coordination => THRESHOLDS.coordination_budget as i64,
            Self::Stuck
            | Self::Carousel
            | Self::RedundantWake
            | Self::FencePush
            | Self::SameAsk => 1,
        }
    }

    /// The rule that fires it, as a finding states it, with its numbers from [`THRESHOLDS`].
    #[must_use]
    pub fn rule(self) -> String {
        let t = THRESHOLDS;
        match self {
            Self::Stuck => {
                "open work parked (its member idle, still owed) after its nudge".to_owned()
            }
            Self::Ceremony => format!(
                "at least {} coordination turns and {}:1 or more against product change, over 24 h or 7 d",
                t.ceremony_least, t.ceremony_ratio
            ),
            Self::Carousel => format!(
                "sent back {} times with no new evidence between",
                t.carousel_returns
            ),
            Self::RedundantWake => format!(
                "{} turns in a row on the task that showed nothing",
                t.redundant_wakes
            ),
            Self::FencePush => format!(
                "{} refused acts in one turn, or the same refused act by {} members within 24 h",
                t.fence_refusals_in_a_turn, t.fence_members
            ),
            Self::SameAsk => format!(
                "a question or decision for the owner {}% alike, both ways, to one already waiting",
                t.same_ask_percent
            ),
            Self::Coordination => format!(
                "more than {} coordination turns on one task in 24 h with nothing changed since \
                 (a product change, or its lead's decision, starts the count again)",
                t.coordination_budget
            ),
        }
    }
}

/// How a guard that changes what the team does behaves (ADR 0133). New authority is tried before
/// it is given: in shadow a guard records what it would have done and does nothing, so its
/// verdicts say how often it would have been right; on, it acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardMode {
    /// It records what it would have done, and holds nothing back.
    Shadow,
    /// It acts.
    On,
}

impl GuardMode {
    /// Stable lowercase name, as stored and shown.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Shadow => "shadow",
            Self::On => "on",
        }
    }

    /// The mode with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Shadow, Self::On]
            .into_iter()
            .find(|m| m.name() == name)
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

impl Raised {
    /// Whether the detection is news: a finding opens, or opens again, and whoever is told of
    /// findings may be told.
    #[must_use]
    pub const fn is_news(self) -> bool {
        matches!(self, Self::Open | Self::Renew | Self::Reopen)
    }
}

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

    #[test]
    fn only_a_finding_that_opens_is_news() {
        for (how, news) in [
            (Raised::Open, true),
            (Raised::Renew, true),
            (Raised::Reopen, true),
            (Raised::Refresh, false),
            (Raised::Hold, false),
        ] {
            assert_eq!(how.is_news(), news, "{how:?}");
        }
    }

    #[test]
    fn talk_has_to_grow_by_a_dozen_turns_a_budget_by_a_budget_and_every_other_guard_by_one_thing() {
        assert_eq!(Guard::Ceremony.step(), 12);
        assert_eq!(
            Guard::Ceremony.step(),
            i64::try_from(THRESHOLDS.ceremony_least).unwrap()
        );
        assert_eq!(Guard::Coordination.step(), 20, "another budget of talk");
        for guard in Guard::ALL
            .into_iter()
            .filter(|g| !matches!(g, Guard::Ceremony | Guard::Coordination))
        {
            assert_eq!(guard.step(), 1, "{guard:?}");
        }
    }

    /// A task's budget of talk (ADR 0133): twenty coordination turns in a day with nothing
    /// changed. The twenty-first is the one that exceeds it.
    #[test]
    fn the_budget_of_talk_is_twenty_coordination_turns_in_a_day_and_the_twenty_first_is_over_it() {
        assert_eq!(
            (
                THRESHOLDS.coordination_budget,
                THRESHOLDS.coordination_window_ms
            ),
            (20, DAY)
        );
        assert!(!over_budget(0) && !over_budget(20));
        assert!(over_budget(21));
        assert!(
            Guard::Coordination
                .rule()
                .contains("more than 20 coordination turns")
        );
    }

    fn talk_ago(now: Millis, ago: Millis) -> TurnSeen {
        TurnSeen {
            ended_at: now - ago,
            ..seen(Label::TalkOnly)
        }
    }

    fn change_ago(now: Millis, ago: Millis) -> TurnSeen {
        TurnSeen {
            ended_at: now - ago,
            acted: true,
            ..seen(Label::Advanced)
        }
    }

    /// A product change starts a task's count again, and so does its lead's decision about it;
    /// talk older than the window is out.
    #[test]
    fn talk_is_counted_from_the_newest_change_or_decision_and_within_the_day() {
        let now = 10 * DAY;
        // Four turns of talk, then a change, then three more.
        let turns = [
            talk_ago(now, 9_000),
            talk_ago(now, 8_000),
            talk_ago(now, 7_000),
            talk_ago(now, 6_000),
            change_ago(now, 5_000),
            talk_ago(now, 4_000),
            talk_ago(now, 3_000),
            talk_ago(now, 2_000),
        ];
        assert_eq!(
            talk_without_progress(&turns, now, 0),
            3,
            "only what came after the change"
        );
        assert_eq!(
            talk_without_progress(&turns[..4], now, 0),
            4,
            "no change yet"
        );
        assert_eq!(
            talk_without_progress(&turns, now, now - 3_500),
            2,
            "its lead decided it between the second and the third"
        );
        let old = [
            talk_ago(now, DAY + 1),
            talk_ago(now, DAY),
            talk_ago(now, DAY - 1),
        ];
        assert_eq!(
            talk_without_progress(&old, now, 0),
            1,
            "a day ago to the millisecond is out"
        );
        let later = TurnSeen {
            ended_at: now + 1,
            ..seen(Label::TalkOnly)
        };
        assert_eq!(talk_without_progress(&[later], now, 0), 0, "not yet");
    }

    /// Talk is what the ceremony guard calls coordination, by the same labels: a turn that took
    /// a first reading, or nothing, is neither; a review round and a re-check are.
    #[test]
    fn only_coordination_turns_are_talk_by_the_ceremony_guards_own_labels() {
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
        assert_eq!(talk_without_progress(&turns, now, 0), 4);
        assert_eq!(
            talk_without_progress(&turns, now, 0),
            ceremony(&turns, now, DAY).coordination,
            "the same count the ceremony makes"
        );
    }

    /// Only the guard that changes what the team does is tried in shadow first (ADR 0133).
    #[test]
    fn only_the_budget_of_talk_is_tried_in_shadow_first() {
        assert!(Guard::Coordination.has_shadow());
        assert_eq!(Guard::Coordination.starts_in(), GuardMode::Shadow);
        for guard in Guard::ALL.into_iter().filter(|g| *g != Guard::Coordination) {
            assert!(!guard.has_shadow(), "{guard:?}");
            assert_eq!(guard.starts_in(), GuardMode::On, "{guard:?}");
        }
        for mode in ["shadow", "on"] {
            assert_eq!(GuardMode::from_name(mode).map(GuardMode::name), Some(mode));
        }
        assert_eq!(GuardMode::from_name("off"), None);
    }

    #[test]
    fn a_guards_precision_is_real_out_of_judged_and_not_enough_is_no_verdict() {
        let all = [
            finding(Guard::Ceremony, Some(Verdict::Real), Some(true)),
            finding(Guard::Ceremony, Some(Verdict::Resolved), Some(false)),
            finding(Guard::Ceremony, Some(Verdict::FalsePositive), None),
            finding(Guard::Ceremony, Some(Verdict::NotEnough), None),
            finding(Guard::Ceremony, None, None),
            finding(Guard::Stuck, None, None),
        ];
        let p = precision(&all);
        let ceremony = p.iter().find(|p| p.guard == Guard::Ceremony).unwrap();
        assert_eq!((ceremony.fired, ceremony.open), (5, 1));
        assert_eq!(ceremony.percent(), Some(66));
        assert_eq!((ceremony.fixed, ceremony.fix_judged), (1, 2));
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
    fn talking_about_work_is_over_at_twelve_turns_and_four_to_one() {
        let talk = |n: usize, at: Millis| {
            vec![
                TurnSeen {
                    ended_at: at,
                    ..seen(Label::TalkOnly)
                };
                n
            ]
        };
        let product = TurnSeen {
            acted: true,
            ..seen(Label::Advanced)
        };
        let mut turns = talk(11, 1_000);
        let c = ceremony(&turns, 2_000, DAY);
        assert_eq!(
            (c.coordination, c.product, c.over()),
            (11, 0, false),
            "too few"
        );
        turns.extend(talk(1, 1_500));
        assert!(ceremony(&turns, 2_000, DAY).over());
        turns.extend(vec![product; 3]);
        let c = ceremony(&turns, 2_000, DAY);
        assert_eq!(
            (c.coordination, c.product, c.over()),
            (12, 3, true),
            "12:3 is 4:1"
        );
        turns.push(product);
        assert!(!ceremony(&turns, 2_000, DAY).over(), "12:4 is under");
        assert_eq!(
            ceremony(&turns, 1_000 + DAY, DAY).coordination,
            1,
            "only the window's turns"
        );
        assert_eq!(
            ceremony(&turns, 2_000, DAY).said("24 h"),
            "12 coordination turns, 4 product changes in 24 h"
        );
    }

    /// A finding says what the talk cost (ADR 0133): what every turn that ended in the window
    /// spent, whatever it did, in dollars and cents. The plain line the scorecard shows is as it
    /// was.
    #[test]
    fn a_ceremonys_cost_is_what_the_windows_turns_spent_and_is_said_with_it() {
        let paid = |at: Millis, spent: Micros| TurnSeen {
            ended_at: at,
            spent,
            ..seen(Label::TalkOnly)
        };
        let product = TurnSeen {
            acted: true,
            label: Label::Advanced,
            ..paid(9 * DAY + 3, 20_000)
        };
        let turns = [
            paid(9 * DAY + 1, 50_000),
            paid(9 * DAY + 2, 30_000),
            product,
            paid(5 * DAY, 100_000),
            paid(2 * DAY, 900_000),
        ];
        let now = 10 * DAY;
        let day = ceremony(&turns, now, DAY);
        assert_eq!((day.coordination, day.product, day.spent), (2, 1, 100_000));
        assert_eq!(
            day.said_with_cost("24 h"),
            "2 coordination turns, 1 product change in 24 h ($0.10 spent)"
        );
        assert_eq!(
            day.said("24 h"),
            "2 coordination turns, 1 product change in 24 h"
        );
        let week = ceremony(&turns, now, 7 * DAY);
        assert_eq!(
            (week.coordination, week.product, week.spent),
            (3, 1, 200_000),
            "the turn eight days old is in neither"
        );
        assert_eq!(
            week.said_with_cost("7 d"),
            "3 coordination turns, 1 product change in 7 d ($0.20 spent)"
        );
    }

    #[test]
    fn a_task_bounced_three_times_with_nothing_new_is_on_the_carousel() {
        use super::super::evidence::Ref;
        let r = Ref::Receipt;
        let first = vec![(10, vec![r(1)])];
        assert!(!carousel(&[20, 40], &first), "twice is not yet");
        let same = vec![(10, vec![r(1)]), (30, vec![r(1)]), (50, vec![r(1)])];
        assert!(carousel(&[20, 40, 60], &same));
        let renewed = vec![(10, vec![r(1)]), (30, vec![r(1)]), (50, vec![r(2)])];
        assert!(
            !carousel(&[20, 40, 60], &renewed),
            "new evidence between the last two returns"
        );
        let silent = vec![(10, vec![r(1)])];
        assert!(
            carousel(&[20, 40, 60], &silent),
            "nothing submitted between is nothing new"
        );
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
        assert_eq!((t.ceremony_ratio, t.ceremony_least), (4, 12));
        assert_eq!((t.carousel_returns, t.redundant_wakes), (3, 3));
        assert_eq!((t.open_subtasks_most, t.depth_most), (5, 3));
        assert_eq!(t.fence_window_ms, DAY);
    }
}
