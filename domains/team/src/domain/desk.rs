//! What waits on the owner (ADR 0132): which kinds of desk item interrupt them, and which are
//! left for the "while you were away" digest.

/// The kinds the digest holds: stuck work, a guard's finding and a notice. Each is something the
/// owner may want to look at, and none needs a decision before the team can go on.
pub const DIGEST: [&str; 3] = ["stuck", "guard", "notice"];

/// Whether an item of this kind interrupts the owner: it needs their decision, approval, answer
/// or money. Every kind but the digest's does, so a kind added later interrupts until it is
/// said otherwise, and a decision is never quietly folded into a digest.
#[must_use]
pub fn interrupts(kind: &str) -> bool {
    !DIGEST.contains(&kind)
}

/// A card waiting on the owner's desk, as [`placing`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waiting<'a> {
    /// The card.
    pub id: super::DeskId,
    /// Its kind: `question`, `approval`, …
    pub kind: &'a str,
    /// The task it is about, if any.
    pub task: Option<super::TaskId>,
    /// What it says.
    pub text: &'a str,
}

/// Where a new ask goes on the owner's desk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placing {
    /// A card of its own.
    New,
    /// Added to the item's question already waiting: HQ's own ask about it.
    AddedTo(super::DeskId),
    /// Refused: a member's question about an item whose question already waits.
    Refused(super::DeskId),
    /// Put with another task's card that asks the same (ADR 0124).
    SameAs(super::DeskId),
}

/// The one rule for keeping the owner's desk free of repeats. One question about an item waits
/// at a time, whoever asked it (ADR 0142, amended 2026-10-31): a member's (`from_a_turn`) is
/// refused while one waits, HQ's own is added to it. A member's question the same as another
/// task's waiting one is put with it (ADR 0124). An approval is always a card of its own: each
/// runs exactly the one action it stored.
#[must_use]
pub fn placing(
    kind: &str,
    task: super::TaskId,
    from_a_turn: bool,
    text: &str,
    waiting: &[Waiting<'_>],
) -> Placing {
    if kind != "question" {
        return Placing::New;
    }
    let questions = || waiting.iter().filter(|d| d.kind == "question");
    if let Some(open) = questions().find(|d| d.task == Some(task)) {
        if from_a_turn {
            return Placing::Refused(open.id);
        }
        return Placing::AddedTo(open.id);
    }
    if !from_a_turn {
        return Placing::New;
    }
    let print = super::guards::fingerprint(text);
    questions()
        .find(|d| {
            d.task.is_some_and(|t| t != task)
                && super::guards::same_ask(&super::guards::fingerprint(d.text), &print)
        })
        .map_or(Placing::New, |open| Placing::SameAs(open.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_question_an_item_and_the_same_ask_once() {
        use crate::domain::{DeskId, TaskId};
        let ask = "Which VPN provider should gluetun use for the download stack now?";
        let waiting = [
            Waiting {
                id: DeskId(1),
                kind: "question",
                task: Some(TaskId(7)),
                text: "Is the port open?",
            },
            Waiting {
                id: DeskId(2),
                kind: "question",
                task: Some(TaskId(8)),
                text: ask,
            },
        ];
        let place = |kind, task, from_a_turn, text| {
            placing(kind, TaskId(task), from_a_turn, text, &waiting)
        };
        assert_eq!(
            place("question", 7, true, "Anything"),
            Placing::Refused(DeskId(1))
        );
        assert_eq!(
            place("question", 7, false, "Anything"),
            Placing::AddedTo(DeskId(1))
        );
        assert_eq!(place("question", 9, true, ask), Placing::SameAs(DeskId(2)));
        assert_eq!(
            place("question", 9, false, ask),
            Placing::New,
            "HQ's own asks are its item's"
        );
        assert_eq!(
            place("question", 9, true, "Something else entirely."),
            Placing::New
        );
        assert_eq!(place("approval", 7, true, ask), Placing::New);
    }

    /// Every kind the owner's desk has held, with whether it interrupts. A new kind is added here
    /// when it is added to the desk.
    const KINDS: [(&str, bool); 21] = [
        ("goal", true),
        ("decision", true),
        ("approval", true),
        ("question", true),
        ("push", true),
        ("ready_to_merge", true),
        ("ready_to_read", true),
        ("rounds", true),
        ("hire", true),
        ("furlough", true),
        ("recall", true),
        ("canon", true),
        ("project_cap", true),
        ("project_budget", true),
        ("out_for_day", true),
        ("tripwire", true),
        ("privacy", true),
        ("needs_owner", true),
        ("stuck", false),
        ("guard", false),
        ("notice", false),
    ];

    #[test]
    fn decisions_approvals_questions_pushes_and_money_interrupt_and_the_digest_does_not() {
        for (kind, interrupting) in KINDS {
            assert_eq!(interrupts(kind), interrupting, "{kind}");
        }
    }

    #[test]
    fn the_digest_is_exactly_the_kinds_that_do_not_interrupt() {
        let quiet: Vec<&str> = KINDS
            .iter()
            .filter(|(_, interrupting)| !interrupting)
            .map(|(kind, _)| *kind)
            .collect();
        assert_eq!(quiet, DIGEST);
    }

    #[test]
    fn a_kind_nobody_has_classified_interrupts() {
        assert!(interrupts("a_kind_from_the_future"));
    }
}
