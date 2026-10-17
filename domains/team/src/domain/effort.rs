//! How hard a member's Claude tries in a turn (ADR 0109's cost notes): the effort HQ names
//! with each cloud turn, by the member's role and what the turn is for. Effort is the one
//! dial on thinking these models leave (Opus 5.5 cannot turn thinking off), and what a
//! turn's thinking costs follows it. The CEO thinks at `medium`, and at `high` when the
//! turn weighs a decision for the board or answers a question up the chain; the lead at
//! `medium`; the researcher, whose look-ups are short, at `low`. The developer and the
//! reviewer are left to their model's own, as they always were: nothing is named.

use super::brain::Brain;
use super::wakes::WakeReason;
use super::work::Role;

/// How hard Claude tries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    /// Brief look-ups.
    Low,
    /// The everyday.
    Medium,
    /// Decisions that weigh.
    High,
}

impl Effort {
    /// Its name in Anthropic's `output_config.effort`, and for the runner.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Whether a CEO's turn woken for `reason` weighs a decision: it answers a question up the
/// chain, or it is about a decision for the board (the board's word on one, or the
/// reviewer's sending one back). `drafted` says the turn's task holds the CEO's decision
/// draft, waiting for its review.
#[must_use]
pub const fn weighs_a_decision(reason: WakeReason, drafted: bool) -> bool {
    drafted
        || matches!(
            reason,
            WakeReason::QuestionAsked | WakeReason::ProposalDecided | WakeReason::SentBack
        )
}

/// The effort for a turn of a member in `role` thinking with `brain`; `weighty` says the
/// turn weighs a decision (see [`weighs_a_decision`]). `None` for a member on the house's
/// model, whose relay takes no effort, and for a role left to its model's own.
#[must_use]
pub const fn effort_for(brain: Brain, role: Role, weighty: bool) -> Option<Effort> {
    if !brain.cloud() {
        return None;
    }
    match role {
        Role::Ceo if weighty => Some(Effort::High),
        Role::Ceo | Role::Lead => Some(Effort::Medium),
        Role::Researcher => Some(Effort::Low),
        Role::Developer | Role::Reviewer => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ceo_weighs_decisions_at_high_and_the_rest_of_its_work_at_medium() {
        assert_eq!(
            effort_for(Brain::Opus, Role::Ceo, false),
            Some(Effort::Medium)
        );
        assert_eq!(effort_for(Brain::Opus, Role::Ceo, true), Some(Effort::High));
        assert_eq!(
            effort_for(Brain::Sonnet, Role::Ceo, true),
            Some(Effort::High)
        );
        for reason in [
            WakeReason::QuestionAsked,
            WakeReason::ProposalDecided,
            WakeReason::SentBack,
        ] {
            assert!(weighs_a_decision(reason, false), "{reason:?}");
        }
        for reason in [
            WakeReason::Assigned,
            WakeReason::QuestionAnswered,
            WakeReason::CheckBack,
            WakeReason::Retry,
        ] {
            assert!(!weighs_a_decision(reason, false), "{reason:?}");
            assert!(weighs_a_decision(reason, true), "a draft: {reason:?}");
        }
    }

    #[test]
    fn the_lead_thinks_at_medium_the_researcher_at_low_and_a_local_brain_is_told_nothing() {
        for weighty in [false, true] {
            assert_eq!(
                effort_for(Brain::Sonnet, Role::Lead, weighty),
                Some(Effort::Medium)
            );
            assert_eq!(
                effort_for(Brain::Haiku, Role::Researcher, weighty),
                Some(Effort::Low)
            );
            for role in [Role::Developer, Role::Reviewer] {
                assert_eq!(effort_for(Brain::Sonnet, role, weighty), None, "{role:?}");
            }
            for role in [Role::Ceo, Role::Lead, Role::Researcher] {
                assert_eq!(effort_for(Brain::Local, role, weighty), None, "{role:?}");
            }
        }
        assert_eq!(
            [Effort::Low, Effort::Medium, Effort::High].map(Effort::name),
            ["low", "medium", "high"]
        );
    }
}
