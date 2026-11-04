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
    /// A check of the member's submitted branch reported, pass or fail (ADR 0115): the
    /// submitter reads it, and so does the tester.
    CheckDone,
    /// Nothing to do: a turn about no task, for the office and the notebook (ADR 0119).
    Downtime,
    /// A task its task waits on was dropped (ADR 0124): the wait will not end by itself.
    WaitDropped,
    /// Its own turn on its task ended at its prompt's budget (ADR 0142, amended 2026-10-30): it
    /// goes on in a fresh turn, from HQ's handover rather than the whole conversation.
    Continued,
    /// A reason nothing raises any more, under the name it was stored with (one of
    /// [`RETIRED`]): the lead's look on a timer, a decision up the stuck ladder, a lead's
    /// acceptance after the tester, a colleague's consult (ADR 0142). Kept so that wakes already
    /// on record still read, and read back as they were written.
    Retired(&'static str),
}

/// The stored names of the reasons nothing raises any more (see [`WakeReason::Retired`]).
pub const RETIRED: [&str; 4] = ["lead_review", "acceptance_asked", "decide", "consulted"];

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
            Self::Retry => "retry",
            Self::Recovery => "recovery",
            Self::CheckBack => "check_back",
            Self::Alarm => "alarm",
            Self::OwnerReplied => "owner_replied",
            Self::AssignmentRecovery => "assignment_recovery",
            Self::ReviewRequested => "review_requested",
            Self::SentBack => "sent_back",
            Self::CheckDone => "check_done",
            Self::Downtime => "downtime",
            Self::WaitDropped => "wait_dropped",
            Self::Continued => "continued",
            Self::Retired(name) => name,
        }
    }

    /// Whether the wake brings something new to its task: anything but a stale continuation
    /// (a recovery nudge, a retry) or a retired reason. Only a wake that brings nothing new is
    /// ever delayed by the back-off.
    #[must_use]
    pub const fn brings_news(self) -> bool {
        !matches!(
            self,
            Self::Recovery | Self::AssignmentRecovery | Self::Retry | Self::Retired(_)
        )
    }

    /// Whether the wake comes from a clock rather than from someone: a check-back or an alarm.
    /// One of these that names no task, for a member with no open task, finds nothing to do.
    #[must_use]
    pub const fn on_a_clock(self) -> bool {
        matches!(self, Self::CheckBack | Self::Alarm)
    }

    /// Whether the wake reaches the owner of a task waiting for its reviewer: only the owner's
    /// own word (a reply, or the answer to a question) and the reviewer sending it back do.
    /// Anything else would be a turn on work that is not the owner's to touch until reviewed.
    #[must_use]
    pub const fn reaches_work_in_review(self) -> bool {
        matches!(
            self,
            Self::OwnerReplied | Self::ProposalDecided | Self::SentBack | Self::CheckDone
        )
    }

    /// Whether the wake reaches a blocked item in the loops (ADR 0142, amended 2026-10-31): only
    /// the owner's word (a reply, or an answer) and the end of what it waits on do. Anything else
    /// (a recovery nudge, a check-back, a mention) would be a turn on work that waits for the
    /// owner, and item 938 took one about every eleven minutes.
    #[must_use]
    pub const fn reaches_a_blocked_item(self) -> bool {
        matches!(
            self,
            Self::OwnerReplied
                | Self::ProposalDecided
                | Self::WaitFinished
                | Self::SubtasksFinished
                | Self::WaitDropped
        )
    }

    /// Every reason that is raised, in the order they are declared ([`Self::Retired`] is not).
    /// A reason added to the enum must be added here too.
    #[must_use]
    pub const fn all() -> [Self; 17] {
        [
            Self::Assigned,
            Self::Mentioned,
            Self::WaitFinished,
            Self::SubtasksFinished,
            Self::ProposalDecided,
            Self::Retry,
            Self::Recovery,
            Self::CheckBack,
            Self::Alarm,
            Self::OwnerReplied,
            Self::AssignmentRecovery,
            Self::ReviewRequested,
            Self::SentBack,
            Self::CheckDone,
            Self::Downtime,
            Self::WaitDropped,
            Self::Continued,
        ]
    }

    /// The reason with this [`name`](Self::name), if there is one: a retired name reads as
    /// [`Self::Retired`] under that same name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        if let Some(live) = Self::all().into_iter().find(|r| r.name() == name) {
            return Some(live);
        }
        RETIRED
            .into_iter()
            .find(|retired| *retired == name)
            .map(Self::Retired)
    }
}

/// Why a wake was turned away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The kill switch is on.
    KillSwitch,
    /// The owner paused the member.
    Paused,
    /// The member's budget for today was spent. No longer raised (ADR 0131): such a wake is kept
    /// as a turn that waits for budget. Kept so that wakes already on record still read.
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
    /// (ADR 0111). No longer raised (ADR 0142: there is no CEO); kept because it was, and a
    /// refused wake on record whose reason does not read is a corrupt row.
    NotForTheCeo,
    /// The task's project is paused by the owner (ADR 0113): nobody takes a turn on it until
    /// the owner resumes it.
    ProjectPaused,
    /// The member is on furlough (ADR 0118): it is woken again only when recalled.
    Furloughed,
    /// A wake that brings nothing new, for a member woken on the task again and again for
    /// nothing (ADR 0124): it is parked, and its area's lead decides it.
    Redundant,
    /// The check after a deploy has had its one turn and its verdict (ADR 0115, amended).
    Checked,
    /// The item is blocked in the loops and waits for the owner: their answer in words, or
    /// their move of it back to work (ADR 0142, amended 2026-10-31).
    Blocked,
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
            Self::Redundant => "redundant",
            Self::Checked => "checked",
            Self::Blocked => "blocked",
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
            Self::Redundant => "it was woken on this again and again for nothing",
            Self::Checked => "the check after the deploy has had its one turn",
            Self::Blocked => "the item is blocked and waits for the owner",
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
            Self::Redundant,
            Self::Checked,
            Self::Blocked,
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
    /// Whether this wake may join it. A wake that the member's Claude brain must judge does
    /// not join a work turn already running on the house's models (ADR 0131).
    pub foldable: bool,
}

/// Everything the decision about one wake depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeSituation {
    /// Whether the kill switch is on.
    pub stopped: bool,
    /// Whether the member is paused.
    pub paused: bool,
    /// Whether the wake is about a task. A wake about none that cannot join the member's
    /// running turn starts a turn of its own; one about a task waits for that turn to end
    /// (ADR 0131).
    pub about_a_task: bool,
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
    /// Whether the wake's task is in a project the owner paused (ADR 0113).
    pub project_paused: bool,
    /// Whether the member is on furlough (ADR 0118): never woken.
    pub furloughed: bool,
    /// Whether the wake brings nothing new, to a member whose last turns on the task showed
    /// nothing, again and again (ADR 0124): it is not woken for nothing once more.
    pub woken_for_nothing: bool,
    /// Whether the wake's task is blocked in the loops and waits for the owner, and the wake is
    /// neither the owner's asking nor one that [reaches a blocked
    /// item](WakeReason::reaches_a_blocked_item).
    pub blocked_for_the_owner: bool,
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
/// for it. Money is not asked of a wake (ADR 0131): a turn of judgment that cannot be paid for
/// is queued and waits for budget where turns start, so nothing a wake asked for is thrown away.
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
    if situation.nothing_to_do {
        return WakeDecision::Refuse(Refused::NothingToDo);
    }
    if situation.waits_for_review {
        return WakeDecision::Refuse(Refused::InReview);
    }
    if situation.project_paused {
        return WakeDecision::Refuse(Refused::ProjectPaused);
    }
    if situation.woken_for_nothing {
        return WakeDecision::Refuse(Refused::Redundant);
    }
    if situation.blocked_for_the_owner {
        return WakeDecision::Refuse(Refused::Blocked);
    }
    if let Some(own) = situation.live.iter().find(|turn| turn.member == member) {
        if own.foldable {
            return WakeDecision::Fold(own.run);
        }
        // Its own work turn already runs on the house's models, and this wake is for the
        // member's judgment brain: it waits for that turn to end, or, about no task, is a
        // turn of its own.
        if !situation.about_a_task {
            return WakeDecision::Queue;
        }
        if situation.already_held {
            return WakeDecision::Refuse(Refused::Duplicate);
        }
        return WakeDecision::Hold;
    }
    if !situation.live.is_empty() {
        if situation.already_held {
            return WakeDecision::Refuse(Refused::Duplicate);
        }
        return WakeDecision::Hold;
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
            about_a_task: true,
            live: Vec::new(),
            already_held: false,
            nothing_to_do: false,
            waits_for_review: false,
            project_paused: false,
            furloughed: false,
            woken_for_nothing: false,
            blocked_for_the_owner: false,
        }
    }

    const ME: MemberId = MemberId(1);

    #[test]
    fn a_member_woken_for_nothing_again_and_again_is_not_woken_once_more() {
        let s = WakeSituation {
            woken_for_nothing: true,
            ..quiet()
        };
        assert_eq!(decide(ME, &s), WakeDecision::Refuse(Refused::Redundant));
        assert_eq!(Refused::from_name("redundant"), Some(Refused::Redundant));
    }
    const OTHER: MemberId = MemberId(2);

    #[test]
    fn a_blocked_item_waiting_for_the_owner_wakes_nobody() {
        let s = WakeSituation {
            blocked_for_the_owner: true,
            ..quiet()
        };
        assert_eq!(decide(ME, &s), WakeDecision::Refuse(Refused::Blocked));
        assert_eq!(Refused::from_name("blocked"), Some(Refused::Blocked));
    }

    #[test]
    fn only_the_owners_word_and_the_end_of_a_wait_reach_a_blocked_item() {
        let reach: Vec<&str> = WakeReason::all()
            .into_iter()
            .filter(|r| r.reaches_a_blocked_item())
            .map(WakeReason::name)
            .collect();
        assert_eq!(
            reach,
            [
                "wait_finished",
                "subtasks_finished",
                "proposal_decided",
                "owner_replied",
                "wait_dropped"
            ]
        );
    }

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
                foldable: true,
            }],
            ..quiet()
        };
        assert_eq!(decide(ME, &s), WakeDecision::Fold(RunId(7)));
    }

    #[test]
    fn a_wake_for_judgment_does_not_join_a_work_turn_already_running() {
        let running_work = |about_a_task| WakeSituation {
            about_a_task,
            live: vec![LiveTurn {
                run: RunId(7),
                member: ME,
                foldable: false,
            }],
            ..quiet()
        };
        // About a task, it waits for that turn to end, once.
        assert_eq!(decide(ME, &running_work(true)), WakeDecision::Hold);
        let held = WakeSituation {
            already_held: true,
            ..running_work(true)
        };
        assert_eq!(decide(ME, &held), WakeDecision::Refuse(Refused::Duplicate));
        // About no task, it is a turn of its own: nothing releases a wake held for no task.
        assert_eq!(decide(ME, &running_work(false)), WakeDecision::Queue);
    }

    #[test]
    fn a_wake_while_another_member_works_the_task_is_held_once() {
        let mut s = WakeSituation {
            live: vec![LiveTurn {
                run: RunId(7),
                member: OTHER,
                foldable: true,
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
                foldable: true,
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
    fn a_wake_is_never_turned_away_for_money_and_wakes_already_on_record_still_read() {
        // The decision does not read the budget at all (ADR 0131): whether the turn can be
        // paid for is asked where turns start, and a turn that cannot waits there.
        assert_eq!(decide(ME, &quiet()), WakeDecision::Queue);
        assert_eq!(
            Refused::from_name("out_of_budget"),
            Some(Refused::OutOfBudget),
            "a wake turned away before kinds still reads"
        );
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
            WakeReason::Retired("lead_review"),
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
        assert!(WakeReason::Alarm.on_a_clock() && WakeReason::CheckBack.on_a_clock());
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
            WakeReason::Retry,
            WakeReason::Recovery,
            WakeReason::CheckBack,
            WakeReason::Alarm,
        ] {
            assert_eq!(WakeReason::from_name(r.name()), Some(r));
        }
        // A retired reason on record reads back under the name it was written with.
        for name in RETIRED {
            let read = WakeReason::from_name(name).expect("a retired name still reads");
            assert_eq!((read, read.name()), (WakeReason::Retired(name), name));
            assert!(!WakeReason::all().contains(&read), "{name} is never raised");
        }
        assert_eq!(WakeReason::from_name("huddle"), None);
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
