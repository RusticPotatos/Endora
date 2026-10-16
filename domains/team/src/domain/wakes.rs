//! Wakes: every turn a member takes starts with one, and every one leaves a receipt.

use super::work::{MemberId, RunId};

/// Why a member is asked to take a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeReason {
    /// A task was handed to it.
    Assigned,
    /// Someone named it in a thread.
    Mentioned,
    /// A task it waits on finished.
    WaitFinished,
    /// Every sub-task of its task finished.
    SubtasksFinished,
    /// The owner decided something it asked.
    ProposalDecided,
    /// The lead's regular look over the team's work.
    LeadReview,
    /// A failed turn, tried again.
    Retry,
    /// Work that stopped moving, nudged once.
    Recovery,
}

impl WakeReason {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Assigned => "assigned",
            Self::Mentioned => "mentioned",
            Self::WaitFinished => "wait_finished",
            Self::SubtasksFinished => "subtasks_finished",
            Self::ProposalDecided => "proposal_decided",
            Self::LeadReview => "lead_review",
            Self::Retry => "retry",
            Self::Recovery => "recovery",
        }
    }

    /// The reason with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Assigned,
            Self::Mentioned,
            Self::WaitFinished,
            Self::SubtasksFinished,
            Self::ProposalDecided,
            Self::LeadReview,
            Self::Retry,
            Self::Recovery,
        ]
        .into_iter()
        .find(|r| r.name() == name)
    }
}

/// Why a wake was turned away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The kill switch is on.
    KillSwitch,
    /// The owner paused the member.
    Paused,
    /// The member's budget for today is spent.
    OutOfBudget,
    /// The member already has a wake held for this task.
    Duplicate,
}

impl Refused {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::KillSwitch => "kill_switch",
            Self::Paused => "paused",
            Self::OutOfBudget => "out_of_budget",
            Self::Duplicate => "duplicate",
        }
    }

    /// The reason with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::KillSwitch,
            Self::Paused,
            Self::OutOfBudget,
            Self::Duplicate,
        ]
        .into_iter()
        .find(|r| r.name() == name)
    }
}

/// What became of a wake. Every wake has one, whatever happened to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receipt {
    /// A turn is queued for it.
    Queued,
    /// Folded into a turn the member is already taking on the same task.
    Folded,
    /// Held: another member is working the task. Released, oldest first, when that turn ends.
    Held,
    /// Turned away, and why.
    Refused(Refused),
    /// Its turn started.
    Started,
    /// Its turn ended.
    Finished,
}

impl Receipt {
    /// Stable lowercase name (a refusal carries its reason separately).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Folded => "folded",
            Self::Held => "held",
            Self::Refused(_) => "refused",
            Self::Started => "started",
            Self::Finished => "finished",
        }
    }
}

/// A live turn on the task the wake is about: queued, waiting to retry, or running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveTurn {
    /// The turn.
    pub run: RunId,
    /// Whose it is.
    pub member: MemberId,
}

/// Everything the decision about one wake depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeSituation {
    /// Whether the kill switch is on.
    pub stopped: bool,
    /// Whether the member is paused.
    pub paused: bool,
    /// Whether the member could still have a turn reserved today.
    pub can_afford_a_turn: bool,
    /// The live turns on the same task (or, for a wake about no task, the member's own live
    /// turns about no task).
    pub live: Vec<LiveTurn>,
    /// Whether this member already has a wake held for this task.
    pub already_held: bool,
}

/// What to do with a wake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeDecision {
    /// Queue a new turn.
    Queue,
    /// Fold it into this turn of the same member.
    Fold(RunId),
    /// Hold it until the other member's turn ends.
    Hold,
    /// Turn it away.
    Refuse(Refused),
}

/// Decides what happens to a wake for `member`. A task has at most one live turn, so a wake
/// for a member already working the task joins that turn, and one for any other member waits
/// for it. Folding costs nothing, so it is decided before the budget is.
#[must_use]
pub fn decide(member: MemberId, situation: &WakeSituation) -> WakeDecision {
    if situation.stopped {
        return WakeDecision::Refuse(Refused::KillSwitch);
    }
    if situation.paused {
        return WakeDecision::Refuse(Refused::Paused);
    }
    if let Some(own) = situation.live.iter().find(|turn| turn.member == member) {
        return WakeDecision::Fold(own.run);
    }
    if !situation.live.is_empty() {
        if situation.already_held {
            return WakeDecision::Refuse(Refused::Duplicate);
        }
        return WakeDecision::Hold;
    }
    if !situation.can_afford_a_turn {
        return WakeDecision::Refuse(Refused::OutOfBudget);
    }
    WakeDecision::Queue
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet() -> WakeSituation {
        WakeSituation {
            stopped: false,
            paused: false,
            can_afford_a_turn: true,
            live: Vec::new(),
            already_held: false,
        }
    }

    const ME: MemberId = MemberId(1);
    const OTHER: MemberId = MemberId(2);

    #[test]
    fn a_wake_on_a_free_task_is_queued() {
        assert_eq!(decide(ME, &quiet()), WakeDecision::Queue);
    }

    #[test]
    fn a_wake_for_a_member_already_on_the_task_folds_into_its_turn() {
        let s = WakeSituation {
            live: vec![LiveTurn {
                run: RunId(7),
                member: ME,
            }],
            ..quiet()
        };
        assert_eq!(decide(ME, &s), WakeDecision::Fold(RunId(7)));
    }

    #[test]
    fn a_wake_while_another_member_works_the_task_is_held_once() {
        let mut s = WakeSituation {
            live: vec![LiveTurn {
                run: RunId(7),
                member: OTHER,
            }],
            ..quiet()
        };
        assert_eq!(decide(ME, &s), WakeDecision::Hold);
        s.already_held = true;
        assert_eq!(decide(ME, &s), WakeDecision::Refuse(Refused::Duplicate));
    }

    #[test]
    fn the_kill_switch_and_a_pause_turn_every_wake_away_first() {
        let s = WakeSituation {
            stopped: true,
            paused: true,
            live: vec![LiveTurn {
                run: RunId(7),
                member: ME,
            }],
            ..quiet()
        };
        assert_eq!(decide(ME, &s), WakeDecision::Refuse(Refused::KillSwitch));
        let s = WakeSituation {
            paused: true,
            ..quiet()
        };
        assert_eq!(decide(ME, &s), WakeDecision::Refuse(Refused::Paused));
    }

    #[test]
    fn folding_costs_nothing_but_a_new_turn_needs_budget() {
        let broke = WakeSituation {
            can_afford_a_turn: false,
            ..quiet()
        };
        assert_eq!(
            decide(ME, &broke),
            WakeDecision::Refuse(Refused::OutOfBudget)
        );
        let folding = WakeSituation {
            live: vec![LiveTurn {
                run: RunId(7),
                member: ME,
            }],
            ..broke
        };
        assert_eq!(decide(ME, &folding), WakeDecision::Fold(RunId(7)));
    }

    #[test]
    fn names_round_trip() {
        for r in [
            WakeReason::Assigned,
            WakeReason::Mentioned,
            WakeReason::WaitFinished,
            WakeReason::SubtasksFinished,
            WakeReason::ProposalDecided,
            WakeReason::LeadReview,
            WakeReason::Retry,
            WakeReason::Recovery,
        ] {
            assert_eq!(WakeReason::from_name(r.name()), Some(r));
        }
        for r in [
            Refused::KillSwitch,
            Refused::Paused,
            Refused::OutOfBudget,
            Refused::Duplicate,
        ] {
            assert_eq!(Refused::from_name(r.name()), Some(r));
        }
    }
}
