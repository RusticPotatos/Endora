//! The two kinds of turn (ADR 0131): which brain a turn runs on is decided here, by code, from
//! why it was asked for, and never by the member. Work (reading, building, checking, drafting)
//! is done by the house's models; judgment (planning, deciding, accepting, reviewing, raising
//! an alarm) only by the member's Claude brain. The language model proposes; this authorizes.

use super::brain::Brain;
use super::wakes::WakeReason;
use super::work::Role;

/// What a turn is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TurnKind {
    /// Reading the lab, building, checking, drafting, summarising, picking work up again: the
    /// house's models do it, and it costs nothing.
    Work,
    /// Planning, reviews, deciding, accepting, a tester's verdict, a ladder or a huddle, an
    /// alarm: anything that ends in a decision or a desk item. Only Claude does it, and a turn
    /// that cannot have Claude waits.
    Judgment,
}

impl TurnKind {
    /// What the owner reads.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::Judgment => "judgment",
        }
    }
}

/// Where the least a work turn may run is: the lowest brain it is allowed on, never a lower one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Allowed {
    /// The house's own model on the NAS, and never the Mac (the Mac is scarce, and this is not
    /// quality work: the office's downtime, a colleague's consult; ADR 0130).
    Local,
    /// The Mac when it is free, otherwise the NAS: reading the lab, status, a summary, a draft.
    Either,
    /// The Mac, and only the Mac: code and branch work, and the work after a review. The turn
    /// waits for the Mac and is never given the NAS's model: a small model sent back again and
    /// again by a reviewer is how a team ends up in a carousel (ADR 0131).
    Mac,
}

/// Whether a turn asked for by `reason`, for a member in `role`, is work or judgment.
///
/// The CEO's every turn is judgment (it is woken only for escalations, the board's word and its
/// own plans), except the office's and a consult. The match has no wildcard: a new reason does
/// not compile until it is classified here.
#[must_use]
pub const fn kind_of(reason: WakeReason, role: Role) -> TurnKind {
    use WakeReason as W;
    let kind = match reason {
        // A decision, a plan, a verdict or an alarm.
        W::ProposalDecided
        | W::LeadReview
        | W::Alarm
        | W::ReviewRequested
        | W::QuestionAsked
        | W::AcceptanceAsked
        | W::Directed
        | W::Decide
        | W::Huddle
        | W::Guard => TurnKind::Judgment,
        // Doing, reading, checking, drafting and picking work up again.
        W::Assigned
        | W::Mentioned
        | W::WaitFinished
        | W::SubtasksFinished
        | W::Retry
        | W::Recovery
        | W::CheckBack
        | W::OwnerReplied
        | W::AssignmentRecovery
        | W::SentBack
        | W::QuestionAnswered
        | W::CheckDone
        | W::Downtime
        | W::WaitDropped
        | W::Consulted => TurnKind::Work,
    };
    let office = matches!(reason, W::Downtime | W::Consulted);
    if matches!(role, Role::Ceo) && !office {
        return TurnKind::Judgment;
    }
    kind
}

/// How far down the house a work turn asked for by `reason`, for a member in `role`, may go.
/// `in_the_wing` is whether the member sits in the engineering wing (ADR 0115), whose
/// developers build with the repo's tools. Meaningful for a work turn only.
#[must_use]
pub const fn allowed_for(reason: WakeReason, role: Role, in_the_wing: bool) -> Allowed {
    match reason {
        // The office and a consult are not quality work.
        WakeReason::Downtime | WakeReason::Consulted => Allowed::Local,
        // Reading what a check found in a submitted branch, and mending what a reviewer sent
        // back, are code work whoever does them.
        WakeReason::CheckDone | WakeReason::SentBack => Allowed::Mac,
        _ => match (role, in_the_wing) {
            // A wing developer builds with the repo's tools (repo_checkout, repo_submit); a
            // tester reviews.
            (Role::Developer, true) | (Role::Reviewer, _) => Allowed::Mac,
            _ => Allowed::Either,
        },
    }
}

/// What one turn needs: the strictest of everything that asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Need {
    /// Work, or judgment.
    pub kind: TurnKind,
    /// For work, how far down it may go.
    pub allowed: Allowed,
}

impl Need {
    /// What a turn needs because of one wake. `a_review` is whether the turn's task is one of
    /// the leads' standing reviews (their daily report on their area, their weekly patterns):
    /// routines that are handed over as any task is, and are judgment whatever woke them.
    #[must_use]
    pub const fn of(reason: WakeReason, role: Role, a_review: bool, in_the_wing: bool) -> Self {
        let kind = match (a_review, kind_of(reason, role)) {
            (true, _) => TurnKind::Judgment,
            (false, kind) => kind,
        };
        Self {
            kind,
            allowed: allowed_for(reason, role, in_the_wing),
        }
    }

    /// What a turn needs when another wake joined it: judgment wins over work, and the
    /// higher floor over the lower. A wake folded into a waiting turn never lowers it.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        Self {
            kind: self.kind.max(other.kind),
            allowed: self.allowed.max(other.allowed),
        }
    }
}

/// Why a turn waits instead of starting, as the owner reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wait {
    /// It is judgment, and the member has no Claude brain (the owner has not given it one).
    JudgmentBrain,
    /// It is judgment, and today's budget is spent: the pool's day, the member's share, or its
    /// project's month.
    Budget,
    /// It needs the Mac, and the Mac cannot take it now, for this reason (ADR 0130).
    Mac(String),
    /// It needs the Mac, and the member works on the house's own model only.
    NeedsTheMac,
}

/// The prefixes a waiting turn's words begin with.
const WAITING: [&str; 3] = [
    "waiting for the Mac: ",
    "waiting for a judgment brain: ",
    "waiting for budget: ",
];

impl Wait {
    /// The turn's `why` while it waits.
    #[must_use]
    pub fn words(&self) -> String {
        match self {
            Self::JudgmentBrain => {
                "waiting for a judgment brain: the owner has not given this member one".to_owned()
            }
            Self::Budget => "waiting for budget: the budget for it is spent".to_owned(),
            Self::Mac(why) => super::mac::waiting_words(why),
            Self::NeedsTheMac => super::mac::waiting_words(
                "this work needs the Mac, and the member's work brain is the house's model",
            ),
        }
    }
}

/// Whether `said` is a turn's waiting words (see [`Wait::words`]): the turn's reason, which the
/// engine clears when the turn starts.
#[must_use]
pub fn is_waiting_words(said: &str) -> bool {
    WAITING.iter().any(|prefix| said.starts_with(prefix))
}

/// Whether `said` is a turn waiting for budget.
#[must_use]
pub fn is_waiting_for_budget(said: &str) -> bool {
    said.starts_with(WAITING[2])
}

/// What decides where a turn runs, besides what it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The member's work brain: the Mac or the house's own model.
    pub work_brain: Brain,
    /// The member's Claude brain, if the owner gave it one.
    pub judgment_brain: Option<Brain>,
    /// Why the Mac cannot take a turn now (see [`super::mac::wait_for`]); `None` when it can.
    pub mac_waits: Option<String>,
    /// Whether the budget lets a judgment turn start now.
    pub can_afford: bool,
}

/// Where a turn goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placed {
    /// It starts on this brain.
    On(Brain),
    /// It waits, and says why. It is never given a brain it is not allowed on.
    Waits(Wait),
}

/// Where a turn with `need` runs: judgment only on the member's Claude brain, work on the best
/// free work brain it is allowed on. A turn that cannot go anywhere it is allowed waits; it is
/// never sent anywhere else.
#[must_use]
pub fn place(need: Need, facts: &Facts) -> Placed {
    if need.kind == TurnKind::Judgment {
        let Some(claude) = facts.judgment_brain else {
            return Placed::Waits(Wait::JudgmentBrain);
        };
        if !facts.can_afford {
            return Placed::Waits(Wait::Budget);
        }
        return Placed::On(claude);
    }
    let the_mac = facts.work_brain == Brain::Mac;
    match need.allowed {
        Allowed::Local => Placed::On(Brain::Local),
        Allowed::Either => match (&facts.mac_waits, the_mac) {
            (None, true) => Placed::On(Brain::Mac),
            _ => Placed::On(Brain::Local),
        },
        Allowed::Mac => match (&facts.mac_waits, the_mac) {
            (None, true) => Placed::On(Brain::Mac),
            (Some(why), true) => Placed::Waits(Wait::Mac(why.clone())),
            (_, false) => Placed::Waits(Wait::NeedsTheMac),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use WakeReason as W;

    /// Every reason, with what it is and, for a member who is not building in the wing and is
    /// not the tester, how far down its work may go (a judgment turn never goes down at all,
    /// so its column says what its work would be). A reason added to the enum fails
    /// `every_reason_is_classified` until it is in this table.
    const TABLE: [(WakeReason, TurnKind, Allowed); 25] = [
        (W::Assigned, TurnKind::Work, Allowed::Either),
        (W::Mentioned, TurnKind::Work, Allowed::Either),
        (W::WaitFinished, TurnKind::Work, Allowed::Either),
        (W::SubtasksFinished, TurnKind::Work, Allowed::Either),
        (W::ProposalDecided, TurnKind::Judgment, Allowed::Either),
        (W::LeadReview, TurnKind::Judgment, Allowed::Either),
        (W::Retry, TurnKind::Work, Allowed::Either),
        (W::Recovery, TurnKind::Work, Allowed::Either),
        (W::CheckBack, TurnKind::Work, Allowed::Either),
        (W::Alarm, TurnKind::Judgment, Allowed::Either),
        (W::OwnerReplied, TurnKind::Work, Allowed::Either),
        (W::AssignmentRecovery, TurnKind::Work, Allowed::Either),
        (W::ReviewRequested, TurnKind::Judgment, Allowed::Either),
        (W::SentBack, TurnKind::Work, Allowed::Mac),
        (W::QuestionAsked, TurnKind::Judgment, Allowed::Either),
        (W::QuestionAnswered, TurnKind::Work, Allowed::Either),
        (W::AcceptanceAsked, TurnKind::Judgment, Allowed::Either),
        (W::CheckDone, TurnKind::Work, Allowed::Mac),
        (W::Downtime, TurnKind::Work, Allowed::Local),
        (W::Directed, TurnKind::Judgment, Allowed::Either),
        (W::Decide, TurnKind::Judgment, Allowed::Either),
        (W::Huddle, TurnKind::Judgment, Allowed::Either),
        (W::Guard, TurnKind::Judgment, Allowed::Either),
        (W::WaitDropped, TurnKind::Work, Allowed::Either),
        (W::Consulted, TurnKind::Work, Allowed::Local),
    ];

    #[test]
    fn every_reason_is_classified() {
        let all = WakeReason::all();
        assert_eq!(TABLE.len(), all.len());
        for reason in all {
            let row = TABLE
                .iter()
                .find(|row| row.0 == reason)
                .unwrap_or_else(|| panic!("{reason:?} is not in the table: classify it"));
            for role in [Role::Lead, Role::Researcher, Role::Developer] {
                assert_eq!(kind_of(reason, role), row.1, "{reason:?} for {role:?}");
                assert_eq!(
                    allowed_for(reason, role, false),
                    row.2,
                    "{reason:?} for {role:?}"
                );
            }
        }
        // Each reason once.
        for (i, row) in TABLE.iter().enumerate() {
            assert!(
                TABLE.iter().skip(i + 1).all(|other| other.0 != row.0),
                "{:?} twice",
                row.0
            );
        }
    }

    #[test]
    fn building_in_the_wing_and_reviewing_need_the_mac_but_the_offices_turns_never_do() {
        for reason in WakeReason::all() {
            let base = allowed_for(reason, Role::Lead, false);
            for (role, in_the_wing) in [
                (Role::Developer, true),
                (Role::Reviewer, true),
                (Role::Reviewer, false),
            ] {
                let allowed = allowed_for(reason, role, in_the_wing);
                if base == Allowed::Local {
                    assert_eq!(allowed, Allowed::Local, "{reason:?}: not quality work");
                    continue;
                }
                assert_eq!(allowed, Allowed::Mac, "{reason:?} for {role:?}");
            }
        }
        // Outside the wing a developer reads the lab like anyone.
        assert_eq!(
            allowed_for(W::Assigned, Role::Developer, false),
            Allowed::Either
        );
        // A lead in the wing plans and decides; it does not build.
        assert_eq!(allowed_for(W::Assigned, Role::Lead, true), Allowed::Either);
    }

    #[test]
    fn the_ceo_judges_everything_but_the_office() {
        for reason in WakeReason::all() {
            let kind = kind_of(reason, Role::Ceo);
            if matches!(reason, W::Downtime | W::Consulted) {
                assert_eq!(kind, TurnKind::Work, "{reason:?}");
                continue;
            }
            assert_eq!(kind, TurnKind::Judgment, "{reason:?}");
        }
    }

    #[test]
    fn the_reasons_the_owner_named_are_judgment_and_the_rest_are_work() {
        for judged in [
            W::LeadReview,
            W::Decide,
            W::AcceptanceAsked,
            W::ReviewRequested,
            W::Huddle,
            W::Alarm,
            W::Guard,
        ] {
            assert_eq!(
                kind_of(judged, Role::Lead),
                TurnKind::Judgment,
                "{judged:?}"
            );
        }
        for done in [W::Assigned, W::CheckDone, W::Recovery, W::SubtasksFinished] {
            assert_eq!(kind_of(done, Role::Lead), TurnKind::Work, "{done:?}");
        }
    }

    fn facts(work: Brain, judgment: Option<Brain>, mac_waits: Option<&str>) -> Facts {
        Facts {
            work_brain: work,
            judgment_brain: judgment,
            mac_waits: mac_waits.map(str::to_owned),
            can_afford: true,
        }
    }

    const JUDGMENT: Need = Need {
        kind: TurnKind::Judgment,
        allowed: Allowed::Either,
    };

    fn work(allowed: Allowed) -> Need {
        Need {
            kind: TurnKind::Work,
            allowed,
        }
    }

    #[test]
    fn judgment_runs_only_on_the_claude_brain_and_never_on_the_house() {
        let claude = facts(Brain::Mac, Some(Brain::Sonnet), None);
        assert_eq!(place(JUDGMENT, &claude), Placed::On(Brain::Sonnet));
        // The Mac is free and the NAS is idle, and still it waits for a brain it has no
        // right to use.
        let none = facts(Brain::Mac, None, None);
        assert_eq!(place(JUDGMENT, &none), Placed::Waits(Wait::JudgmentBrain));
        let broke = Facts {
            can_afford: false,
            ..claude
        };
        assert_eq!(place(JUDGMENT, &broke), Placed::Waits(Wait::Budget));
        // A member with no Claude brain and no budget says the first thing: it is not about
        // the money.
        let neither = Facts {
            can_afford: false,
            ..none
        };
        assert_eq!(
            place(JUDGMENT, &neither),
            Placed::Waits(Wait::JudgmentBrain)
        );
    }

    #[test]
    fn code_work_waits_for_the_mac_and_is_never_given_the_nas_model() {
        let busy = facts(Brain::Mac, Some(Brain::Haiku), Some("owner active"));
        assert_eq!(
            place(work(Allowed::Mac), &busy),
            Placed::Waits(Wait::Mac("owner active".to_owned()))
        );
        let free = facts(Brain::Mac, Some(Brain::Haiku), None);
        assert_eq!(place(work(Allowed::Mac), &free), Placed::On(Brain::Mac));
        // A member whose work brain is the house's model cannot do it at all.
        let local = facts(Brain::Local, Some(Brain::Haiku), None);
        assert_eq!(
            place(work(Allowed::Mac), &local),
            Placed::Waits(Wait::NeedsTheMac)
        );
    }

    #[test]
    fn reading_work_takes_the_mac_when_it_is_free_and_the_nas_otherwise() {
        let free = facts(Brain::Mac, None, None);
        assert_eq!(place(work(Allowed::Either), &free), Placed::On(Brain::Mac));
        let busy = facts(Brain::Mac, None, Some("another turn is using the Mac"));
        assert_eq!(
            place(work(Allowed::Either), &busy),
            Placed::On(Brain::Local)
        );
        let local = facts(Brain::Local, None, None);
        assert_eq!(
            place(work(Allowed::Either), &local),
            Placed::On(Brain::Local)
        );
    }

    #[test]
    fn the_offices_turns_stay_off_the_mac() {
        let free = facts(Brain::Mac, None, None);
        assert_eq!(place(work(Allowed::Local), &free), Placed::On(Brain::Local));
    }

    #[test]
    fn a_turn_several_wakes_asked_for_needs_the_most_any_of_them_did() {
        let reading = Need::of(W::Mentioned, Role::Lead, false, false);
        let deciding = Need::of(W::Decide, Role::Lead, false, false);
        assert_eq!(reading.and(deciding).kind, TurnKind::Judgment);
        assert_eq!(deciding.and(reading).kind, TurnKind::Judgment);
        let building = Need::of(W::CheckDone, Role::Lead, false, false);
        assert_eq!(reading.and(building).allowed, Allowed::Mac);
    }

    #[test]
    fn a_leads_standing_review_is_judgment_whatever_woke_it() {
        // A routine's task is handed over like any task: its wake is "assigned".
        assert_eq!(
            Need::of(W::Assigned, Role::Lead, false, false).kind,
            TurnKind::Work
        );
        assert_eq!(
            Need::of(W::Assigned, Role::Lead, true, false).kind,
            TurnKind::Judgment
        );
    }

    #[test]
    fn waiting_words_are_recognised_so_they_can_be_cleared() {
        for wait in [
            Wait::JudgmentBrain,
            Wait::Budget,
            Wait::Mac("on battery".to_owned()),
            Wait::NeedsTheMac,
        ] {
            assert!(is_waiting_words(&wait.words()), "{wait:?}");
        }
        assert!(is_waiting_for_budget(&Wait::Budget.words()));
        assert!(!is_waiting_for_budget(&Wait::NeedsTheMac.words()));
        assert!(!is_waiting_words(
            "backing off: its last turns moved nothing"
        ));
    }
}
