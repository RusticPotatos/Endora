//! Policy & Consent: the deterministic authorization boundary.
//!
//! The constitution requires that AI models *propose* and deterministic policy
//! *authorizes* (see `docs/adr/0005-models-propose-policy-authorizes.md`). This
//! module is that boundary, expressed as pure functions over domain state — it
//! is never a model, and a model can never stand in for it.
//!
//! A proposal (however it was produced — by a human, or later by a model) is
//! only ever *input*. Whether a consequential action takes effect is decided
//! here, deterministically, and can be tested and audited.

use crate::autonomy::AutonomyLevel;
use crate::reflection::ProposedProcessChange;

/// The outcome of a deterministic authorization decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    /// The action is authorized to proceed now.
    Permit,
    /// The action may proceed only after an explicit human approval.
    RequireHumanApproval,
    /// The action is refused.
    Deny {
        /// A stable, human-readable reason.
        reason: &'static str,
    },
}

impl PolicyDecision {
    /// Whether the action may proceed now (i.e. the decision is [`Permit`]).
    ///
    /// [`Permit`]: PolicyDecision::Permit
    #[must_use]
    pub const fn is_permitted(self) -> bool {
        matches!(self, Self::Permit)
    }
}

/// Decides whether *enacting* a proposed process change is authorized.
///
/// Enacting a process change alters how Endora works, so it is treated as
/// consequential. The deterministic rules are:
///
/// - It may only be enacted after an **explicit human approval**, regardless of
///   the actor's autonomy level — human autonomy is final, and process changes
///   require human approval.
/// - Even once approved, an actor that may only observe cannot enact it.
///
/// A model may *produce* the underlying proposal, but only this code decides
/// whether it takes effect.
#[must_use]
pub fn authorize_process_change(
    change: &ProposedProcessChange,
    actor: AutonomyLevel,
) -> PolicyDecision {
    if !change.is_approved() {
        return PolicyDecision::RequireHumanApproval;
    }
    if !actor.permits_action() {
        return PolicyDecision::Deny {
            reason: "actor at observe level may not enact changes",
        };
    }
    PolicyDecision::Permit
}

#[cfg(test)]
mod tests {
    use super::{PolicyDecision, authorize_process_change};
    use crate::autonomy::AutonomyLevel::{ActWithinPolicy, ConfirmEachAction, Observe, Suggest};
    use crate::ids::{ProcessChangeId, ReflectionId};
    use crate::reflection::ProposedProcessChange;

    fn proposal() -> ProposedProcessChange {
        ProposedProcessChange::propose(
            ProcessChangeId::new(1),
            ReflectionId::new(1),
            "Default runs to mornings",
        )
        .unwrap()
    }

    fn approved() -> ProposedProcessChange {
        let mut p = proposal();
        p.approve().unwrap();
        p
    }

    #[test]
    fn an_unapproved_change_requires_human_approval_at_any_level() {
        for actor in [Observe, Suggest, ConfirmEachAction, ActWithinPolicy] {
            assert_eq!(
                authorize_process_change(&proposal(), actor),
                PolicyDecision::RequireHumanApproval
            );
        }
    }

    #[test]
    fn an_observer_cannot_enact_even_an_approved_change() {
        assert_eq!(
            authorize_process_change(&approved(), Observe),
            PolicyDecision::Deny {
                reason: "actor at observe level may not enact changes"
            }
        );
    }

    #[test]
    fn an_acting_level_may_enact_an_approved_change() {
        for actor in [Suggest, ConfirmEachAction, ActWithinPolicy] {
            assert_eq!(
                authorize_process_change(&approved(), actor),
                PolicyDecision::Permit
            );
        }
    }

    #[test]
    fn is_permitted_reflects_the_decision() {
        assert!(authorize_process_change(&approved(), ActWithinPolicy).is_permitted());
        assert!(!authorize_process_change(&proposal(), ActWithinPolicy).is_permitted());
    }
}
