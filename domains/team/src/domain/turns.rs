//! The two kinds of turn (ADR 0131): which brain a turn runs on is decided here, by code, from
//! why it was asked for, and never by the member. Work (reading, building, checking, drafting)
//! is done by the house's models; judgment (planning, deciding, accepting, reviewing) only by
//! the member's Claude, except an alarm, which acts at once when the money is gone. The language
//! model proposes; this authorizes.

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
    /// that cannot have Claude waits (an alarm excepted).
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

/// The lowest brain a work turn may run on: never a lower one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Allowed {
    /// The house's own model on the NAS, and never the Mac (the Mac is scarce, and this is not
    /// quality work: the office's downtime, a colleague's consult; ADR 0130).
    Local,
    /// The Mac when it is free, otherwise the NAS: reading the lab, status, a summary, a draft.
    Either,
    /// The Mac, and only the Mac: code and branch work, a tester's work, and the work after a
    /// review. The turn waits for the Mac and is never given the NAS's model: a small model
    /// sent back again and again by a reviewer, or planning code, is how a team ends up in a
    /// carousel (ADR 0131).
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
        | W::AcceptanceAsked
        | W::Decide => TurnKind::Judgment,
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
/// `in_the_wing` is whether the member sits in the engineering wing (ADR 0115), whose developers
/// build with the repo's tools and whose lead plans their code. Meaningful for a work turn only.
#[must_use]
pub const fn allowed_for(reason: WakeReason, role: Role, in_the_wing: bool) -> Allowed {
    match reason {
        // The office is not quality work. A colleague's answer is (ADR 0139), but read-only and
        // on a deadline: it takes whichever house model is free, never waiting for the Mac (the
        // first one, production 2026-10-25, expired seventh in the Mac's queue).
        WakeReason::Downtime => Allowed::Local,
        WakeReason::Consulted => Allowed::Either,
        // Reading what a check found in a submitted branch, and mending what a reviewer sent
        // back, are code work whoever does them.
        WakeReason::CheckDone | WakeReason::SentBack => Allowed::Mac,
        _ => match (role, in_the_wing) {
            // A wing developer builds with the repo's tools (repo_checkout, repo_submit), its
            // lead plans that code, and a tester reviews it: none of it on the NAS's model.
            (Role::Developer | Role::Lead, true) | (Role::Reviewer, _) => Allowed::Mac,
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
    /// For judgment, whether it is an emergency: an alarm, which with no budget left runs at
    /// once on the member's work brain instead of waiting, and is the only judgment that may.
    pub emergency: bool,
}

impl Need {
    /// What a side of a league game needs (ADR 0135, amended 2026-10-25): its pinned brain and
    /// no other. A Claude is judgment, paid for; the Mac waits for the Mac; the house's model
    /// is the NAS's.
    #[must_use]
    pub const fn pinned(brain: Brain) -> Self {
        let (kind, allowed) = match brain {
            Brain::Mac => (TurnKind::Work, Allowed::Mac),
            Brain::Local => (TurnKind::Work, Allowed::Local),
            Brain::Sonnet | Brain::Haiku | Brain::Opus | Brain::DeepSeek => {
                (TurnKind::Judgment, Allowed::Either)
            }
        };
        Self {
            kind,
            allowed,
            emergency: false,
        }
    }

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
            emergency: matches!(kind, TurnKind::Judgment) && matches!(reason, WakeReason::Alarm),
        }
    }

    /// What a turn needs when another wake joined it: judgment wins over work, and the higher
    /// floor over the lower. A wake folded into a waiting turn never lowers it, and a turn is
    /// an emergency only if every wake of judgment in it is: an alarm folded into a decision
    /// does not let the decision run on the house's model.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        let emergency = match (self.kind, other.kind) {
            (TurnKind::Judgment, TurnKind::Judgment) => self.emergency && other.emergency,
            (TurnKind::Judgment, TurnKind::Work) => self.emergency,
            (TurnKind::Work, TurnKind::Judgment) => other.emergency,
            (TurnKind::Work, TurnKind::Work) => false,
        };
        Self {
            kind: self.kind.max(other.kind),
            allowed: self.allowed.max(other.allowed),
            emergency,
        }
    }
}

/// Why a turn waits instead of starting, as the owner reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wait {
    /// It is judgment, and today's budget is spent: the pool's day, the member's share, or its
    /// project's month.
    Budget,
    /// It needs the Mac, and the Mac cannot take it now, for this reason (ADR 0130).
    Mac(String),
    /// It is work on the house's own models, which cost nothing, and the member has started as
    /// many turns on them today as it may (ADR 0133): it waits until the day turns.
    DayCap,
    /// A restart is coming and a drain holds every new turn back (ADR 0134): it starts once
    /// HQ is up again.
    Restart,
}

/// The prefixes a waiting turn's words begin with.
const WAITING: [&str; 4] = [
    "waiting for the Mac: ",
    "waiting for budget: ",
    "waiting for tomorrow: ",
    "waiting for a restart: ",
];

impl Wait {
    /// The turn's `why` while it waits.
    #[must_use]
    pub fn words(&self) -> String {
        match self {
            Self::Budget => "waiting for budget: the budget for it is spent".to_owned(),
            Self::Mac(why) => super::mac::waiting_words(why),
            Self::DayCap => format!(
                "{}{} turns a day is the most a member takes on the house's own models",
                WAITING[2],
                super::guards::THRESHOLDS.free_turns_a_day
            ),
            Self::Restart => format!("{}nothing new starts until it has been", WAITING[3]),
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
    said.starts_with(WAITING[1])
}

/// Whether `said` is a turn waiting for the day to turn, a member having started its day's turns
/// on the house's own models (ADR 0133).
#[must_use]
pub fn is_waiting_for_cap(said: &str) -> bool {
    said.starts_with(WAITING[2])
}

/// What decides where a turn runs, besides what it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The member's work brain: the Mac or the house's own model.
    pub work_brain: Brain,
    /// The Claude the member judges with.
    pub judgment_brain: Brain,
    /// Why the Mac cannot take a turn now (see [`super::mac::wait_for`]); `None` when it can.
    pub mac_waits: Option<String>,
    /// Whether the budget lets a judgment turn start now.
    pub can_afford: bool,
    /// How many turns the member has started today on the house's own models, the one being
    /// placed aside (ADR 0133).
    pub free_turns_today: usize,
}

/// Where a turn goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placed {
    /// It starts on this brain.
    On(Brain),
    /// An emergency out of budget starts at once on this work brain, not Claude. The engine
    /// tells the owner on the desk.
    Fallback(Brain),
    /// It waits, and says why. It is never given a brain it is not allowed on.
    Waits(Wait),
}

/// The best free work brain: the Mac when the member works with it and it is free, else the
/// NAS's model.
fn best_free_work_brain(facts: &Facts) -> Brain {
    match (&facts.mac_waits, facts.work_brain) {
        (None, Brain::Mac) => Brain::Mac,
        _ => Brain::Local,
    }
}

/// Where a turn with `need` runs: judgment only on the member's Claude, work on the best free
/// work brain it is allowed on. A turn that cannot go anywhere it is allowed waits; it is never
/// sent anywhere else. The one exception is an emergency (an alarm) with no budget left, which
/// runs at once on the best free work brain: the turn's actions stay gated by the same
/// contracts, whoever thinks it.
#[must_use]
pub fn place(need: Need, facts: &Facts) -> Placed {
    if need.kind == TurnKind::Judgment {
        if facts.can_afford {
            return Placed::On(facts.judgment_brain);
        }
        if need.emergency {
            return Placed::Fallback(best_free_work_brain(facts));
        }
        return Placed::Waits(Wait::Budget);
    }
    // Work runs on the house's own models, which cost nothing, so no budget stops a member that
    // has run away with them: past its day's turns it waits until the day turns (ADR 0133). A
    // judgment turn is on Claude, and bounded by money; an alarm acts at once, and is above.
    if facts.free_turns_today >= super::guards::THRESHOLDS.free_turns_a_day {
        return Placed::Waits(Wait::DayCap);
    }
    match need.allowed {
        Allowed::Local => Placed::On(Brain::Local),
        Allowed::Either => Placed::On(best_free_work_brain(facts)),
        // The Mac is the floor for this work, not the member's ceiling: it runs there when the
        // Mac is free, whatever the member's own work brain, and waits for the Mac otherwise.
        Allowed::Mac => match &facts.mac_waits {
            None => Placed::On(Brain::Mac),
            Some(why) => Placed::Waits(Wait::Mac(why.clone())),
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
    const TABLE: [(WakeReason, TurnKind, Allowed); 20] = [
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
        (W::AcceptanceAsked, TurnKind::Judgment, Allowed::Either),
        (W::CheckDone, TurnKind::Work, Allowed::Mac),
        (W::Downtime, TurnKind::Work, Allowed::Local),
        (W::Decide, TurnKind::Judgment, Allowed::Either),
        (W::WaitDropped, TurnKind::Work, Allowed::Either),
        // ADR 0139: a colleague's answer is not forced onto the house's model.
        (W::Consulted, TurnKind::Work, Allowed::Either),
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
    fn the_wings_developers_and_lead_and_every_tester_need_the_mac_but_the_offices_turns_never_do()
    {
        for reason in WakeReason::all() {
            let base = allowed_for(reason, Role::Researcher, false);
            for (role, in_the_wing) in [
                (Role::Developer, true),
                (Role::Lead, true),
                (Role::Reviewer, true),
                (Role::Reviewer, false),
            ] {
                let allowed = allowed_for(reason, role, in_the_wing);
                if base == Allowed::Local {
                    assert_eq!(allowed, Allowed::Local, "{reason:?}: not quality work");
                    continue;
                }
                // A colleague's answer is read-only and on a deadline: whichever house model is
                // free (the owner, 2026-10-25: the first one expired behind the Mac's queue).
                if matches!(reason, W::Consulted) {
                    assert_eq!(allowed, Allowed::Either, "{reason:?} for {role:?}");
                    continue;
                }
                assert_eq!(allowed, Allowed::Mac, "{reason:?} for {role:?}");
            }
        }
        // Outside the wing a developer or a lead reads the lab like anyone.
        for role in [Role::Developer, Role::Lead, Role::Researcher, Role::Ceo] {
            assert_eq!(
                allowed_for(W::Assigned, role, false),
                Allowed::Either,
                "{role:?}"
            );
        }
        // A researcher in the wing's reach still only reads.
        assert_eq!(
            allowed_for(W::Assigned, Role::Researcher, true),
            Allowed::Either
        );
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
            W::Alarm,
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

    fn facts(work: Brain, mac_waits: Option<&str>) -> Facts {
        Facts {
            work_brain: work,
            judgment_brain: Brain::Haiku,
            mac_waits: mac_waits.map(str::to_owned),
            can_afford: true,
            free_turns_today: 0,
        }
    }

    fn judgment() -> Need {
        Need::of(W::Decide, Role::Lead, false, false)
    }

    fn work(allowed: Allowed) -> Need {
        Need {
            kind: TurnKind::Work,
            allowed,
            emergency: false,
        }
    }

    #[test]
    fn judgment_runs_only_on_the_claude_brain_and_waits_when_the_money_is_gone() {
        let claude = Facts {
            judgment_brain: Brain::Sonnet,
            ..facts(Brain::Mac, None)
        };
        assert_eq!(place(judgment(), &claude), Placed::On(Brain::Sonnet));
        // The Mac is free and the NAS is idle, and still it waits for a brain it has no right
        // to use.
        let broke = Facts {
            can_afford: false,
            ..claude
        };
        assert_eq!(place(judgment(), &broke), Placed::Waits(Wait::Budget));
    }

    #[test]
    fn no_judgment_but_an_alarm_ever_runs_on_the_house_for_want_of_money() {
        // The mutation check of the rule: every kind of judgment, with the Mac free and the
        // NAS idle and no money, waits. Only the alarm acts at once, and not on Claude.
        let broke = Facts {
            can_afford: false,
            ..facts(Brain::Mac, None)
        };
        let mut judged = 0;
        for reason in WakeReason::all() {
            for role in [
                Role::Lead,
                Role::Developer,
                Role::Reviewer,
                Role::Researcher,
            ] {
                let need = Need::of(reason, role, false, false);
                if need.kind != TurnKind::Judgment {
                    continue;
                }
                judged += 1;
                let placed = place(need, &broke);
                if reason == W::Alarm {
                    assert_eq!(placed, Placed::Fallback(Brain::Mac), "{reason:?}");
                    continue;
                }
                assert_eq!(
                    placed,
                    Placed::Waits(Wait::Budget),
                    "{reason:?} for {role:?}"
                );
            }
        }
        assert!(judged >= 20, "the table has judgment in it: {judged}");
        // A lead's standing review is judgment whatever woke it, and it waits too.
        assert_eq!(
            place(Need::of(W::Assigned, Role::Lead, true, false), &broke),
            Placed::Waits(Wait::Budget)
        );
        // And so does the CEO, whose every turn is judgment.
        assert_eq!(
            place(Need::of(W::Assigned, Role::Ceo, false, false), &broke),
            Placed::Waits(Wait::Budget)
        );
    }

    #[test]
    fn an_alarm_out_of_budget_runs_at_once_on_the_mac_if_it_is_free_else_the_nas() {
        let alarm = Need::of(W::Alarm, Role::Lead, false, false);
        assert!(alarm.emergency);
        let broke = |work, mac_waits| Facts {
            can_afford: false,
            ..facts(work, mac_waits)
        };
        assert_eq!(
            place(alarm, &broke(Brain::Mac, None)),
            Placed::Fallback(Brain::Mac)
        );
        assert_eq!(
            place(alarm, &broke(Brain::Mac, Some("owner active"))),
            Placed::Fallback(Brain::Local),
            "it does not wait for the Mac"
        );
        assert_eq!(
            place(alarm, &broke(Brain::Local, None)),
            Placed::Fallback(Brain::Local)
        );
        // With money it is judged on Claude like the rest.
        assert_eq!(
            place(alarm, &facts(Brain::Mac, None)),
            Placed::On(Brain::Haiku)
        );
        // Its floor does not hold it back: even a wing lead's alarm acts now.
        let wing_lead = Need::of(W::Alarm, Role::Lead, false, true);
        assert_eq!(
            place(wing_lead, &broke(Brain::Mac, Some("owner active"))),
            Placed::Fallback(Brain::Local)
        );
    }

    #[test]
    fn an_alarm_folded_into_a_decision_does_not_let_the_decision_run_on_the_house() {
        let alarm = Need::of(W::Alarm, Role::Lead, false, false);
        let decision = Need::of(W::Decide, Role::Lead, false, false);
        let reading = Need::of(W::Mentioned, Role::Lead, false, false);
        assert!(!alarm.and(decision).emergency);
        assert!(!decision.and(alarm).emergency);
        assert!(alarm.and(alarm).emergency);
        // Reading that joined an alarm is part of the alarm's turn.
        assert!(alarm.and(reading).emergency && reading.and(alarm).emergency);
        assert!(!reading.and(reading).emergency);
        let broke = Facts {
            can_afford: false,
            ..facts(Brain::Mac, None)
        };
        assert_eq!(
            place(alarm.and(decision), &broke),
            Placed::Waits(Wait::Budget)
        );
    }

    #[test]
    fn code_work_waits_for_the_mac_and_is_never_given_the_nas_model() {
        let busy = facts(Brain::Mac, Some("owner active"));
        assert_eq!(
            place(work(Allowed::Mac), &busy),
            Placed::Waits(Wait::Mac("owner active".to_owned()))
        );
        let free = facts(Brain::Mac, None);
        assert_eq!(place(work(Allowed::Mac), &free), Placed::On(Brain::Mac));
        // The Mac is a floor, not the member's ceiling: a member whose own work brain is the
        // house's model still does Mac-floor work on the Mac when it is free, and waits for it
        // when it is not. It never waits forever on its setting.
        let local = facts(Brain::Local, None);
        assert_eq!(place(work(Allowed::Mac), &local), Placed::On(Brain::Mac));
        let local_busy = facts(Brain::Local, Some("owner active"));
        assert_eq!(
            place(work(Allowed::Mac), &local_busy),
            Placed::Waits(Wait::Mac("owner active".to_owned()))
        );
    }

    #[test]
    fn reading_work_takes_the_mac_when_it_is_free_and_the_nas_otherwise() {
        let free = facts(Brain::Mac, None);
        assert_eq!(place(work(Allowed::Either), &free), Placed::On(Brain::Mac));
        let busy = facts(Brain::Mac, Some("another turn is using the Mac"));
        assert_eq!(
            place(work(Allowed::Either), &busy),
            Placed::On(Brain::Local)
        );
        let local = facts(Brain::Local, None);
        assert_eq!(
            place(work(Allowed::Either), &local),
            Placed::On(Brain::Local)
        );
    }

    #[test]
    fn work_is_free_whatever_the_budget() {
        let broke = Facts {
            can_afford: false,
            ..facts(Brain::Mac, None)
        };
        assert_eq!(place(work(Allowed::Either), &broke), Placed::On(Brain::Mac));
    }

    #[test]
    fn the_offices_turns_stay_off_the_mac() {
        let free = facts(Brain::Mac, None);
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
            Wait::Budget,
            Wait::Mac("on battery".to_owned()),
            Wait::DayCap,
        ] {
            assert!(is_waiting_words(&wait.words()), "{wait:?}");
        }
        assert!(is_waiting_for_budget(&Wait::Budget.words()));
        assert!(!is_waiting_for_budget(
            &Wait::Mac("on battery".to_owned()).words()
        ));
        assert!(is_waiting_for_cap(&Wait::DayCap.words()));
        assert!(!is_waiting_for_cap(&Wait::Budget.words()));
        assert!(!is_waiting_for_budget(&Wait::DayCap.words()));
        assert!(
            Wait::DayCap.words().starts_with("waiting for "),
            "the roster line shows every wait that begins so"
        );
        assert!(!is_waiting_words(
            "backing off: its last turns moved nothing"
        ));
    }

    /// A turn held back by a drain (ADR 0134) says so in the words the roster shows, and is a
    /// wait like the rest: recognised, so the engine clears it when the turn starts.
    #[test]
    fn a_turn_held_for_a_restart_says_so() {
        let words = Wait::Restart.words();
        assert_eq!(
            words,
            "waiting for a restart: nothing new starts until it has been"
        );
        assert!(is_waiting_words(&words));
        assert!(!is_waiting_for_budget(&words) && !is_waiting_for_cap(&words));
    }

    /// A member takes sixty turns a day on the house's own models (ADR 0133): they cost nothing,
    /// so no budget stops a member that has run away with them. The sixty-first, whichever of the
    /// house's brains it would run on, waits until the day turns.
    #[test]
    fn the_sixty_first_turn_of_the_day_on_the_house_waits_for_tomorrow() {
        let taken = |n| Facts {
            free_turns_today: n,
            ..facts(Brain::Mac, None)
        };
        for allowed in [Allowed::Local, Allowed::Either, Allowed::Mac] {
            assert!(
                matches!(place(work(allowed), &taken(59)), Placed::On(_)),
                "the sixtieth, {allowed:?}"
            );
            assert_eq!(
                place(work(allowed), &taken(60)),
                Placed::Waits(Wait::DayCap),
                "{allowed:?}"
            );
            assert_eq!(
                place(work(allowed), &taken(500)),
                Placed::Waits(Wait::DayCap),
                "{allowed:?}"
            );
        }
        // The cap is the longer wait: with the Mac busy as well, it is what the turn says.
        let both = Facts {
            free_turns_today: 60,
            ..facts(Brain::Mac, Some("owner active"))
        };
        assert_eq!(
            place(work(Allowed::Mac), &both),
            Placed::Waits(Wait::DayCap)
        );
        let local = Facts {
            free_turns_today: 60,
            ..facts(Brain::Local, None)
        };
        assert_eq!(
            place(work(Allowed::Either), &local),
            Placed::Waits(Wait::DayCap)
        );
    }

    /// Judgment runs on Claude and is bounded by money; an alarm acts at once. Neither is a turn
    /// on the house's own models that the cap is for.
    #[test]
    fn judgment_and_an_alarm_that_acts_at_once_are_not_capped() {
        let capped = Facts {
            free_turns_today: 500,
            ..facts(Brain::Mac, None)
        };
        assert_eq!(place(judgment(), &capped), Placed::On(Brain::Haiku));
        let broke = Facts {
            can_afford: false,
            ..capped
        };
        let alarm = Need::of(W::Alarm, Role::Lead, false, false);
        assert_eq!(place(alarm, &broke), Placed::Fallback(Brain::Mac));
        assert_eq!(
            place(judgment(), &broke),
            Placed::Waits(Wait::Budget),
            "money, not the cap, is what a decision waits for"
        );
    }
}
