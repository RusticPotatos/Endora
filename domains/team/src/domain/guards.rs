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
};

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
                     its words): {}",
                    listed.join("; ")
                )
            }
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
    /// A turn on it is queued, running, or waiting to retry.
    pub live_turn: bool,
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

/// A project's ceremony over one window: its coordination turns and its product changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceremony {
    /// Coordination turns.
    pub coordination: usize,
    /// Turns that changed the product.
    pub product: usize,
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
}

/// The ceremony of `turns` that ended within `ms` before `now`.
#[must_use]
pub fn ceremony(turns: &[TurnSeen], now: Millis, ms: Millis) -> Ceremony {
    let within = turns
        .iter()
        .filter(|t| t.ended_at <= now && now - t.ended_at < ms);
    let mut found = Ceremony {
        coordination: 0,
        product: 0,
    };
    for t in within {
        match turn_kind(t) {
            TurnKind::Product => found.product += 1,
            TurnKind::Coordination => found.coordination += 1,
            TurnKind::Neither => {}
        }
    }
    found
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
}

impl Guard {
    /// Every guard, in the order the scorecard shows them.
    pub const ALL: [Self; 5] = [
        Self::Stuck,
        Self::Ceremony,
        Self::Carousel,
        Self::RedundantWake,
        Self::FencePush,
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
        }
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
