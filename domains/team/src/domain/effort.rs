//! How hard a member's Claude tries in a turn (ADR 0109's cost notes): the effort HQ names
//! with each cloud turn, by the member's role and what the turn is for. Effort is the one
//! dial on thinking these models leave (Opus 5.5 cannot turn thinking off), and what a
//! turn's thinking costs follows it. The CEO thinks at `low` on an ordinary turn, `medium` on
//! its daily review and its planning, and `high` when it decides a project or carries
//! something to the owner's desk (the owner's decision of 2026-10-17); the lead at `medium`;
//! the researcher, whose look-ups are short, at `low`. The developer and the
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

/// What a CEO's turn is for, as its effort follows it (the owner's decision of 2026-10-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    /// An ordinary turn: a question up the chain, a post, a recovery, a routine wake that is
    /// not a review.
    Ordinary,
    /// Its daily review or its weekly planning.
    Review,
    /// It decides a project (a proposal waits for it), or carries something to the owner's
    /// desk (its decision for the board sent back, or the board's word on one).
    Weighty,
}

/// What a CEO's turn woken for `reason` is for: `review` says its task is its daily review
/// or its planning, `proposals` that a project proposal waits for its decision, `drafted`
/// that its task holds its decision for the board.
#[must_use]
pub const fn ceo_weight(
    reason: WakeReason,
    review: bool,
    proposals: bool,
    drafted: bool,
) -> Weight {
    if proposals || drafted || matches!(reason, WakeReason::ProposalDecided | WakeReason::SentBack)
    {
        return Weight::Weighty;
    }
    if review {
        return Weight::Review;
    }
    Weight::Ordinary
}

/// The effort for a turn of a member in `role` thinking with `brain`; `weight` says what a
/// CEO's turn is for (see [`ceo_weight`]). `None` for a member on the house's model, whose
/// relay takes no effort, and for a role left to its model's own.
#[must_use]
pub const fn effort_for(brain: Brain, role: Role, weight: Weight) -> Option<Effort> {
    if !brain.cloud() {
        return None;
    }
    match role {
        Role::Ceo => Some(match weight {
            Weight::Ordinary => Effort::Low,
            Weight::Review => Effort::Medium,
            Weight::Weighty => Effort::High,
        }),
        Role::Lead => Some(Effort::Medium),
        Role::Researcher => Some(Effort::Low),
        Role::Developer | Role::Reviewer => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ceo_thinks_low_on_ordinary_turns_medium_on_its_reviews_and_high_on_decisions() {
        let ordinary = |r| ceo_weight(r, false, false, false);
        for reason in [
            WakeReason::QuestionAsked,
            WakeReason::Assigned,
            WakeReason::QuestionAnswered,
            WakeReason::CheckBack,
            WakeReason::Recovery,
            WakeReason::Retry,
        ] {
            assert_eq!(ordinary(reason), Weight::Ordinary, "{reason:?}");
            assert_eq!(ceo_weight(reason, true, false, false), Weight::Review);
            assert_eq!(
                ceo_weight(reason, false, true, false),
                Weight::Weighty,
                "a proposal"
            );
            assert_eq!(
                ceo_weight(reason, true, false, true),
                Weight::Weighty,
                "a draft"
            );
        }
        for reason in [WakeReason::ProposalDecided, WakeReason::SentBack] {
            assert_eq!(ordinary(reason), Weight::Weighty, "{reason:?}");
        }
        assert_eq!(
            [Weight::Ordinary, Weight::Review, Weight::Weighty].map(|w| effort_for(
                Brain::Opus,
                Role::Ceo,
                w
            )),
            [Some(Effort::Low), Some(Effort::Medium), Some(Effort::High)]
        );
    }

    #[test]
    fn the_lead_thinks_at_medium_the_researcher_at_low_and_a_local_brain_is_told_nothing() {
        for weight in [Weight::Ordinary, Weight::Review, Weight::Weighty] {
            assert_eq!(
                effort_for(Brain::Sonnet, Role::Lead, weight),
                Some(Effort::Medium)
            );
            assert_eq!(
                effort_for(Brain::Haiku, Role::Researcher, weight),
                Some(Effort::Low)
            );
            for role in [Role::Developer, Role::Reviewer] {
                assert_eq!(effort_for(Brain::Sonnet, role, weight), None, "{role:?}");
            }
            for role in [Role::Ceo, Role::Lead, Role::Researcher] {
                assert_eq!(effort_for(Brain::Local, role, weight), None, "{role:?}");
            }
        }
        assert_eq!(
            [Effort::Low, Effort::Medium, Effort::High].map(Effort::name),
            ["low", "medium", "high"]
        );
    }
}
