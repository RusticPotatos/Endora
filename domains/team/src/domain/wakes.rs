//! Wakes: every turn a member takes starts with one, and every one leaves a receipt.

use super::work::{MemberId, Millis, RunId};

/// How long after a member's turn on a task ended a wake that brings nothing new waits before
/// its next turn on it: two minutes. Something new (a reply, an answer, an alarm, a
/// check-back's time) is never kept waiting.
pub const AGAIN_AFTER_MS: Millis = 2 * 60 * 1000;

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
    /// The member said it would come back to its task, and the time came (ADR 0107).
    CheckBack,
    /// Endora raised an alarm about the home lab, passed into the open task of the routine
    /// that takes it (ADR 0107).
    Alarm,
    /// The owner wrote in the task's thread: whoever has the task reads it at once.
    OwnerReplied,
    /// Work handed to the member that nothing would ever bring it back to, nudged once.
    AssignmentRecovery,
    /// A member's draft for the owner waits for the reviewer to check it.
    ReviewRequested,
    /// The reviewer sent the member's draft back: something in it did not hold.
    SentBack,
    /// A member below asked this one a question about its task, up the chain of command
    /// (ADR 0111): it is in the task's thread, "Question for <name>: …".
    QuestionAsked,
    /// The one this member asked up the chain answered in the task's thread.
    QuestionAnswered,
    /// Work the tester passed (or that nobody else tests) waits for its lead to accept it
    /// against its criteria, or send it back (ADR 0114).
    AcceptanceAsked,
    /// A check of the member's submitted branch reported, pass or fail (ADR 0115): the
    /// submitter reads it, and so does the tester.
    CheckDone,
    /// Nothing to do: a turn about no task, for the office and the notebook (ADR 0119).
    Downtime,
    /// The CEO gave the lead of the task's area a direction (ADR 0120): the lead decides what
    /// to do with it.
    Directed,
    /// Stuck work climbed the ladder to this member (ADR 0120): the area's lead, to decide;
    /// or the CEO, when the lead said it needs the CEO.
    Decide,
    /// A huddle on stuck work asks this member for its view, in one short turn (ADR 0120).
    Huddle,
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
            Self::CheckBack => "check_back",
            Self::Alarm => "alarm",
            Self::OwnerReplied => "owner_replied",
            Self::AssignmentRecovery => "assignment_recovery",
            Self::ReviewRequested => "review_requested",
            Self::SentBack => "sent_back",
            Self::QuestionAsked => "question_asked",
            Self::QuestionAnswered => "question_answered",
            Self::AcceptanceAsked => "acceptance_asked",
            Self::CheckDone => "check_done",
            Self::Downtime => "downtime",
            Self::Directed => "directed",
            Self::Decide => "decide",
            Self::Huddle => "huddle",
        }
    }

    /// Whether the wake brings something new to its task: anything but a stale continuation
    /// (a recovery nudge, a retry) or the lead's look round on a timer. Only a wake that brings
    /// nothing new is ever delayed by the back-off.
    #[must_use]
    pub const fn brings_news(self) -> bool {
        !matches!(
            self,
            Self::Recovery | Self::AssignmentRecovery | Self::Retry | Self::LeadReview
        )
    }

    /// Whether the wake comes from a clock rather than from someone: a timer, a check-back or
    /// an alarm. One of these that names no task, for a member with no open task, finds
    /// nothing to do.
    #[must_use]
    pub const fn on_a_clock(self) -> bool {
        matches!(self, Self::LeadReview | Self::CheckBack | Self::Alarm)
    }

    /// Whether the wake reaches the owner of a task waiting for its reviewer: only the owner's
    /// own word (a reply, or the answer to a question) and the reviewer sending it back do.
    /// Anything else would be a turn on work that is not the owner's to touch until reviewed.
    #[must_use]
    pub const fn reaches_work_in_review(self) -> bool {
        matches!(
            self,
            Self::OwnerReplied
                | Self::ProposalDecided
                | Self::SentBack
                | Self::QuestionAsked
                | Self::QuestionAnswered
                | Self::CheckDone
                | Self::Directed
                | Self::Decide
                | Self::Huddle
        )
    }

    /// Whether the wake reaches the CEO (ADR 0111), who wakes only for escalations, the
    /// board's word, its own work (its daily review, what it handed on) and the continuations
    /// of its own turns: never to be named in a thread, for an alarm, for a look round, or for
    /// downtime (the CEO is paid, and has an office of its own).
    #[must_use]
    pub const fn reaches_the_ceo(self) -> bool {
        !matches!(
            self,
            Self::Mentioned
                | Self::LeadReview
                | Self::Alarm
                | Self::ReviewRequested
                | Self::Downtime
        )
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
            Self::CheckBack,
            Self::Alarm,
            Self::OwnerReplied,
            Self::AssignmentRecovery,
            Self::ReviewRequested,
            Self::SentBack,
            Self::QuestionAsked,
            Self::QuestionAnswered,
            Self::AcceptanceAsked,
            Self::CheckDone,
            Self::Downtime,
            Self::Directed,
            Self::Decide,
            Self::Huddle,
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
    /// A wake on a clock, about no task, for a member with no open task: no turn is started
    /// to look for work (ADR 0107).
    NothingToDo,
    /// The task waits for its reviewer: its owner takes no turn on it until the reviewer sends
    /// it back or the owner speaks.
    InReview,
    /// The CEO is not woken for that: only for escalations, the board's word and its own work
    /// (ADR 0111).
    NotForTheCeo,
    /// The task's project is paused by the owner (ADR 0113): nobody takes a turn on it until
    /// the owner resumes it.
    ProjectPaused,
    /// The member is on furlough (ADR 0118): it is woken again only when recalled.
    Furloughed,
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
            Self::NothingToDo => "nothing_to_do",
            Self::InReview => "in_review",
            Self::NotForTheCeo => "not_for_the_ceo",
            Self::ProjectPaused => "project_paused",
            Self::Furloughed => "furloughed",
        }
    }

    /// The reason in plain words, for the owner.
    #[must_use]
    pub const fn plainly(self) -> &'static str {
        match self {
            Self::KillSwitch => "the kill switch is on; resume first",
            Self::Paused => "the member is paused",
            Self::OutOfBudget => "the budget for today is spent",
            Self::Duplicate => "a wake for it is already waiting",
            Self::NothingToDo => "there is nothing for it to do",
            Self::InReview => "the work waits for its reviewer",
            Self::NotForTheCeo => "the CEO is woken only for escalations",
            Self::ProjectPaused => "its project is paused",
            Self::Furloughed => "the member is on furlough",
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
            Self::NothingToDo,
            Self::InReview,
            Self::NotForTheCeo,
            Self::ProjectPaused,
            Self::Furloughed,
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
    /// Whether the wake is on a clock, about no task, for a member with no open task.
    pub nothing_to_do: bool,
    /// Whether the wake is for the owner of a task waiting for its reviewer, and is not one
    /// that [reaches work in review](WakeReason::reaches_work_in_review).
    pub waits_for_review: bool,
    /// Whether the wake is for the CEO, and is not one that
    /// [reaches the CEO](WakeReason::reaches_the_ceo).
    pub not_for_the_ceo: bool,
    /// Whether the wake's task is in a project the owner paused (ADR 0113).
    pub project_paused: bool,
    /// Whether the member is on furlough (ADR 0118): never woken.
    pub furloughed: bool,
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
    if situation.furloughed {
        return WakeDecision::Refuse(Refused::Furloughed);
    }
    if situation.not_for_the_ceo {
        return WakeDecision::Refuse(Refused::NotForTheCeo);
    }
    if situation.nothing_to_do {
        return WakeDecision::Refuse(Refused::NothingToDo);
    }
    if situation.waits_for_review {
        return WakeDecision::Refuse(Refused::InReview);
    }
    if situation.project_paused {
        return WakeDecision::Refuse(Refused::ProjectPaused);
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
            nothing_to_do: false,
            waits_for_review: false,
            not_for_the_ceo: false,
            project_paused: false,
            furloughed: false,
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
    fn work_in_review_gets_no_turn_for_its_owner_unless_the_owner_or_the_reviewer_says() {
        let waiting = WakeSituation {
            waits_for_review: true,
            ..quiet()
        };
        assert_eq!(
            decide(ME, &waiting),
            WakeDecision::Refuse(Refused::InReview)
        );
        for reaches in [
            WakeReason::OwnerReplied,
            WakeReason::ProposalDecided,
            WakeReason::SentBack,
        ] {
            assert!(reaches.reaches_work_in_review(), "{reaches:?}");
        }
        for not in [
            WakeReason::Mentioned,
            WakeReason::Recovery,
            WakeReason::AssignmentRecovery,
            WakeReason::CheckBack,
            WakeReason::Alarm,
            WakeReason::ReviewRequested,
        ] {
            assert!(!not.reaches_work_in_review(), "{not:?}");
        }
        assert_eq!(Refused::from_name("in_review"), Some(Refused::InReview));
        assert_eq!(
            WakeReason::from_name("review_requested"),
            Some(WakeReason::ReviewRequested)
        );
        assert_eq!(
            WakeReason::from_name("sent_back"),
            Some(WakeReason::SentBack)
        );
    }

    #[test]
    fn the_ceo_wakes_for_escalations_and_its_own_work_and_never_for_routine_noise() {
        let noise = WakeSituation {
            not_for_the_ceo: true,
            ..quiet()
        };
        assert_eq!(
            decide(ME, &noise),
            WakeDecision::Refuse(Refused::NotForTheCeo)
        );
        for reaches in [
            WakeReason::QuestionAsked,
            WakeReason::QuestionAnswered,
            WakeReason::ProposalDecided,
            WakeReason::OwnerReplied,
            WakeReason::Assigned,
            WakeReason::SentBack,
            WakeReason::SubtasksFinished,
            WakeReason::CheckBack,
        ] {
            assert!(reaches.reaches_the_ceo(), "{reaches:?}");
        }
        for not in [
            WakeReason::Mentioned,
            WakeReason::LeadReview,
            WakeReason::Alarm,
            WakeReason::ReviewRequested,
        ] {
            assert!(!not.reaches_the_ceo(), "{not:?}");
        }
        for asked in [WakeReason::QuestionAsked, WakeReason::QuestionAnswered] {
            assert!(asked.brings_news() && asked.reaches_work_in_review());
            assert_eq!(WakeReason::from_name(asked.name()), Some(asked));
        }
        assert_eq!(
            Refused::from_name("not_for_the_ceo"),
            Some(Refused::NotForTheCeo)
        );
    }

    #[test]
    fn a_furloughed_member_is_never_woken_whatever_the_wake_is_for() {
        let out = WakeSituation {
            furloughed: true,
            ..quiet()
        };
        assert_eq!(decide(ME, &out), WakeDecision::Refuse(Refused::Furloughed));
        assert_eq!(Refused::from_name("furloughed"), Some(Refused::Furloughed));
    }

    #[test]
    fn a_project_the_owner_paused_takes_no_turn() {
        let held = WakeSituation {
            project_paused: true,
            ..quiet()
        };
        assert_eq!(
            decide(ME, &held),
            WakeDecision::Refuse(Refused::ProjectPaused)
        );
        assert_eq!(
            Refused::from_name("project_paused"),
            Some(Refused::ProjectPaused)
        );
    }

    #[test]
    fn a_wake_on_a_clock_with_nothing_to_do_starts_no_turn() {
        let idle = WakeSituation {
            nothing_to_do: true,
            ..quiet()
        };
        assert_eq!(
            decide(ME, &idle),
            WakeDecision::Refuse(Refused::NothingToDo)
        );
        let stopped = WakeSituation {
            stopped: true,
            ..idle
        };
        assert_eq!(
            decide(ME, &stopped),
            WakeDecision::Refuse(Refused::KillSwitch),
            "the kill switch is said first"
        );
    }

    #[test]
    fn only_stale_continuations_bring_nothing_new() {
        for stale in [
            WakeReason::Recovery,
            WakeReason::Retry,
            WakeReason::LeadReview,
        ] {
            assert!(!stale.brings_news(), "{stale:?}");
        }
        for news in [
            WakeReason::Assigned,
            WakeReason::Mentioned,
            WakeReason::WaitFinished,
            WakeReason::SubtasksFinished,
            WakeReason::ProposalDecided,
            WakeReason::CheckBack,
            WakeReason::Alarm,
        ] {
            assert!(news.brings_news(), "{news:?}");
        }
        assert!(WakeReason::LeadReview.on_a_clock() && WakeReason::CheckBack.on_a_clock());
        assert!(!WakeReason::Assigned.on_a_clock());
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
            WakeReason::CheckBack,
            WakeReason::Alarm,
        ] {
            assert_eq!(WakeReason::from_name(r.name()), Some(r));
        }
        for r in [
            Refused::KillSwitch,
            Refused::Paused,
            Refused::OutOfBudget,
            Refused::Duplicate,
            Refused::NothingToDo,
        ] {
            assert_eq!(Refused::from_name(r.name()), Some(r));
        }
    }
}
