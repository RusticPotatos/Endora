//! Autonomy levels.
//!
//! Endora's constitution states that AI models are *reasoning components, not
//! sources of authority*: models may propose actions, but deterministic policy
//! code controls permission and execution. [`AutonomyLevel`] is the domain
//! vocabulary for *how much* a component is permitted to do before a human is
//! involved. It is a plain, deterministic value type — no model, transport, or
//! policy engine is referenced here. The Policy & Consent context consumes
//! these levels to make authorization decisions.

use core::cmp::Ordering;

/// How much authority a component (including an AI model) is granted before a
/// human must be involved.
///
/// Levels are ordered from least to most autonomy. The ordering is meaningful:
/// a higher level grants a superset of what a lower level grants. Crucially,
/// even the most permissive level ([`AutonomyLevel::ActWithinPolicy`]) never
/// implies that a *model* self-authorizes — it means deterministic policy code
/// has pre-authorized a bounded, reversible class of actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutonomyLevel {
    /// May only observe and read. It proposes no actions and takes none.
    Observe,
    /// May propose actions, but every proposal requires explicit human approval
    /// before anything happens.
    Suggest,
    /// May carry out actions only after explicit, per-action human confirmation.
    ConfirmEachAction,
    /// May act without per-action confirmation, but only within reversible,
    /// proportionate bounds that deterministic policy code has pre-authorized.
    ActWithinPolicy,
}

impl AutonomyLevel {
    /// Rank from least (`0`) to most autonomy. Used to order and compare levels.
    const fn rank(self) -> u8 {
        match self {
            Self::Observe => 0,
            Self::Suggest => 1,
            Self::ConfirmEachAction => 2,
            Self::ActWithinPolicy => 3,
        }
    }

    /// Whether a human must confirm before an action at this level takes effect.
    ///
    /// This is `true` for every level except [`AutonomyLevel::ActWithinPolicy`],
    /// which is the only level where deterministic policy may authorize a
    /// bounded action without a per-action human confirmation. A model's
    /// proposal never changes this answer.
    #[must_use]
    pub const fn requires_human_confirmation(self) -> bool {
        !matches!(self, Self::ActWithinPolicy)
    }

    /// Whether a component at this level may take any action at all (as opposed
    /// to only observing).
    #[must_use]
    pub const fn permits_action(self) -> bool {
        !matches!(self, Self::Observe)
    }

    /// A stable, lowercase name for the level, for interfaces and storage. The
    /// round trip with [`from_name`](Self::from_name) is part of the contract.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Suggest => "suggest",
            Self::ConfirmEachAction => "confirm_each_action",
            Self::ActWithinPolicy => "act_within_policy",
        }
    }

    /// Parses a level from its [`name`](Self::name), or `None` if unrecognized.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "observe" => Some(Self::Observe),
            "suggest" => Some(Self::Suggest),
            "confirm_each_action" => Some(Self::ConfirmEachAction),
            "act_within_policy" => Some(Self::ActWithinPolicy),
            _ => None,
        }
    }
}

impl PartialOrd for AutonomyLevel {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AutonomyLevel {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}

#[cfg(test)]
mod tests {
    use super::AutonomyLevel::{ActWithinPolicy, ConfirmEachAction, Observe, Suggest};

    #[test]
    fn only_act_within_policy_skips_human_confirmation() {
        assert!(Observe.requires_human_confirmation());
        assert!(Suggest.requires_human_confirmation());
        assert!(ConfirmEachAction.requires_human_confirmation());
        assert!(!ActWithinPolicy.requires_human_confirmation());
    }

    #[test]
    fn observe_is_the_only_level_that_cannot_act() {
        assert!(!Observe.permits_action());
        assert!(Suggest.permits_action());
        assert!(ConfirmEachAction.permits_action());
        assert!(ActWithinPolicy.permits_action());
    }

    #[test]
    fn level_names_round_trip() {
        use super::AutonomyLevel;
        for level in [Observe, Suggest, ConfirmEachAction, ActWithinPolicy] {
            assert_eq!(AutonomyLevel::from_name(level.name()), Some(level));
        }
        assert_eq!(AutonomyLevel::from_name("bogus"), None);
    }

    #[test]
    fn levels_are_ordered_from_least_to_most_autonomy() {
        assert!(Observe < Suggest);
        assert!(Suggest < ConfirmEachAction);
        assert!(ConfirmEachAction < ActWithinPolicy);
    }
}
