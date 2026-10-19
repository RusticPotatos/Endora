//! Outcomes (ADR 0132): what the team's work came to, as against how busy it was. Computed from
//! HQ's own record and nothing else, pure over the rows it is handed, for the "How the team is
//! doing" card and the top bar.
//!
//! Live, over 17 and 18 October, 201 turns and $8.85 delivered no accepted change, 362 posts in
//! threads against 46 acts, and thirty items for the owner's desk in 38.6 hours, six of them
//! real decisions; nothing on the screen said so, because every number it showed counted
//! activity. These count what the activity bought.
//!
//! What cannot be known is not guessed: a cost per landed change with nothing landed is "none",
//! never nought or infinite; talk against work, which needs the process-health look, is `None`
//! until that look has been made.

use std::collections::HashSet;

use serde_json::{Value, json};

use super::{Window, ended_in};
use crate::application::ports::Run;
use crate::domain::consult::{Consult, Status};
use crate::domain::guards::{self, TurnKind, TurnSeen};
use crate::domain::money::{DAY_MS, Micros};
use crate::domain::review::SendBackReason;
use crate::domain::{Branch, Check, Event, Millis, RunStatus, desk, turns};

/// HQ's record as the outcomes read it.
#[derive(Debug, Clone, Copy)]
pub struct OutcomeRecord<'a> {
    /// Every turn, ended or not.
    pub runs: &'a [Run],
    /// Every event kept, oldest first.
    pub events: &'a [Event],
    /// Every branch submitted.
    pub branches: &'a [Branch],
    /// Every check of a branch.
    pub checks: &'a [Check],
    /// Every consult.
    pub consults: &'a [Consult],
}

/// Consults asked of other members, and how they were answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Consults {
    /// Asked in the window, answered or not.
    pub asked: usize,
    /// Answered by the member asked.
    pub answered: usize,
    /// Answered from the record, the member asked not being woken.
    pub from_record: usize,
}

/// How much the team's memory is used, and whether work is still reinvented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Memory {
    /// Work sent back because it already existed.
    pub reinvented: usize,
    /// Times a member searched the record with `ask_record`.
    pub record_asked: usize,
    /// Consults of other members.
    pub consults: Consults,
}

/// Talk against work, as the ceremony guard counts them (ADR 0124).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TalkWork {
    /// Turns that coordinated: talk only, a re-check with nothing new, a review round, a
    /// question up the chain.
    pub coordination: usize,
    /// Turns that changed the product: an act, a branch, a criterion met.
    pub product: usize,
    /// Posts members wrote in threads.
    pub posts: usize,
    /// Acts that succeeded.
    pub acts: usize,
}

impl TalkWork {
    /// For the screen, and for the process-health look to keep.
    #[must_use]
    pub fn json(&self) -> Value {
        json!({ "coordination": self.coordination, "product": self.product,
                "posts": self.posts, "acts": self.acts })
    }

    /// What [`Self::json`] kept, read back; `None` for anything else.
    #[must_use]
    pub fn read(kept: &Value) -> Option<Self> {
        let number = |key: &str| kept[key].as_u64().and_then(|n| usize::try_from(n).ok());
        Some(Self {
            coordination: number("coordination")?,
            product: number("product")?,
            posts: number("posts")?,
            acts: number("acts")?,
        })
    }
}

/// What the team's work came to in one window.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcomes {
    /// The window's first moment.
    pub from: Millis,
    /// The first moment after it.
    pub until: Millis,
    /// Where the record starts, when that is after the window does: the feed is pruned, and
    /// nothing older is counted.
    pub since: Option<Millis>,
    /// Tasks whose branch the node pushed to the fork on the owner's word, each once.
    pub landed: usize,
    /// Branches submitted.
    pub submitted: usize,
    /// Checks of a branch that passed.
    pub checks_passed: usize,
    /// What the turns that ended in the window spent.
    pub spent: Micros,
    /// Things that interrupted the owner: desk items that need their word.
    pub interrupts: usize,
    /// Things that went to the digest instead.
    pub digest: usize,
    /// Memory.
    pub memory: Memory,
}

impl Outcomes {
    /// What one landed change cost: the window's spend over what landed. `None` when nothing
    /// landed, never a cost of nought and never a division by nothing.
    #[must_use]
    pub fn per_landed(&self) -> Option<Micros> {
        if self.landed == 0 {
            return None;
        }
        Some(self.spent / self.landed as Micros)
    }

    /// Interruptions a day over the window, to one decimal.
    #[must_use]
    pub fn interrupts_a_day(&self) -> f64 {
        let days = (self.until - self.from) as f64 / DAY_MS as f64;
        if days <= 0.0 {
            return 0.0;
        }
        (self.interrupts as f64 / days * 10.0).round() / 10.0
    }

    /// For the screen, which only renders it. `talk` is what the last look at process health
    /// measured, if it has made one.
    #[must_use]
    pub fn json(&self, talk: Option<&TalkWork>) -> Value {
        json!({
            "from": self.from,
            "until": self.until,
            "since": self.since,
            "landed": self.landed,
            "submitted": self.submitted,
            "checks_passed": self.checks_passed,
            "spent": self.spent,
            "per_landed": self.per_landed(),
            "talk": talk.map(TalkWork::json),
            "interrupts": self.interrupts,
            "interrupts_a_day": self.interrupts_a_day(),
            "digest": self.digest,
            "memory": {
                "reinvented": self.memory.reinvented,
                "record_asked": self.memory.record_asked,
                "consults": {
                    "asked": self.memory.consults.asked,
                    "answered": self.memory.consults.answered,
                    "from_record": self.memory.consults.from_record,
                },
            },
        })
    }
}

/// Turns that wait now, and how long the oldest has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Waiting {
    /// Turns waiting for budget.
    pub budget: usize,
    /// Turns waiting for the Mac.
    pub mac: usize,
    /// How long the oldest has waited.
    pub oldest_ms: Option<Millis>,
}

impl Waiting {
    /// For the screen.
    #[must_use]
    pub fn json(&self) -> Value {
        json!({ "budget": self.budget, "mac": self.mac, "oldest_ms": self.oldest_ms })
    }
}

/// The distinct tasks whose branch was pushed to the fork, as the node told HQ: the push's own
/// outcome, so an approval not yet pushed is not a change that landed, and a push that failed
/// never was.
fn landed(events: &[Event], window: &Window) -> usize {
    events
        .iter()
        .filter(|e| window.holds(e.at) && e.kind == "branch.pushed" && e.data["ok"] == true)
        .filter_map(|e| e.data["task"].as_i64())
        .collect::<HashSet<_>>()
        .len()
}

/// Events of this kind in the window.
fn count(events: &[Event], window: &Window, kind: &str) -> usize {
    events
        .iter()
        .filter(|e| window.holds(e.at) && e.kind == kind)
        .count()
}

/// Items added to the owner's desk in the window, those that interrupt and those that went to
/// the digest.
fn on_the_desk(events: &[Event], window: &Window) -> (usize, usize) {
    let (mut interrupts, mut digest) = (0, 0);
    for e in events
        .iter()
        .filter(|e| window.holds(e.at) && e.kind == "desk.added")
    {
        if desk::interrupts(e.data["kind"].as_str().unwrap_or_default()) {
            interrupts += 1;
        } else {
            digest += 1;
        }
    }
    (interrupts, digest)
}

fn memory(record: &OutcomeRecord<'_>, window: &Window) -> Memory {
    let reinvented = record
        .events
        .iter()
        .filter(|e| window.holds(e.at) && e.kind == "task.sent_back")
        .filter(|e| e.data["reason"] == SendBackReason::ExistsAlready.name())
        .count();
    let asked: Vec<&Consult> = record
        .consults
        .iter()
        .filter(|c| window.holds(c.at))
        .collect();
    let with = |status: Status| asked.iter().filter(|c| c.status == status).count();
    Memory {
        reinvented,
        record_asked: count(record.events, window, "record.asked"),
        consults: Consults {
            asked: asked.len(),
            answered: with(Status::Answered),
            from_record: with(Status::FromRecord),
        },
    }
}

/// The outcomes of one window.
#[must_use]
pub fn outcomes(record: &OutcomeRecord<'_>, window: &Window) -> Outcomes {
    let (interrupts, digest) = on_the_desk(record.events, window);
    Outcomes {
        from: window.from,
        until: window.until,
        since: record
            .events
            .first()
            .map(|e| e.at)
            .filter(|at| *at > window.from),
        landed: landed(record.events, window),
        submitted: record
            .branches
            .iter()
            .filter(|b| window.holds(b.at))
            .count(),
        checks_passed: record
            .checks
            .iter()
            .filter(|c| c.ok == Some(true) && c.done_at.is_some_and(|at| window.holds(at)))
            .count(),
        spent: ended_in(record.runs, window).map(|r| r.spent).sum(),
        interrupts,
        digest,
        memory: memory(record, window),
    }
}

/// The turns that wait now, from the queued ones among `runs`: a turn waits when its words say
/// it waits for budget or for the Mac (ADR 0131), and has since it was queued.
#[must_use]
pub fn waiting(runs: &[Run], now: Millis) -> Waiting {
    let waits: Vec<&Run> = runs
        .iter()
        .filter(|r| r.status == RunStatus::Queued && turns::is_waiting_words(&r.end_reason))
        .collect();
    Waiting {
        budget: waits
            .iter()
            .filter(|r| turns::is_waiting_for_budget(&r.end_reason))
            .count(),
        mac: waits
            .iter()
            .filter(|r| crate::domain::mac::is_waiting_words(&r.end_reason))
            .count(),
        oldest_ms: waits.iter().map(|r| now - r.queued_at).max(),
    }
}

/// Talk against work over a window, with the ceremony guard's own labels (ADR 0124): a turn that
/// changed the product, a turn that coordinated, and neither. `posts` and `acts` are when members
/// posted and when an act of theirs succeeded.
#[must_use]
pub fn talk_work(
    turns: &[TurnSeen],
    posts: &[Millis],
    acts: &[Millis],
    window: &Window,
) -> TalkWork {
    let mut found = TalkWork::default();
    for turn in turns.iter().filter(|t| window.holds(t.ended_at)) {
        match guards::turn_kind(turn) {
            TurnKind::Product => found.product += 1,
            TurnKind::Coordination => found.coordination += 1,
            TurnKind::Neither => {}
        }
    }
    found.posts = posts.iter().filter(|at| window.holds(**at)).count();
    found.acts = acts.iter().filter(|at| window.holds(**at)).count();
    found
}
