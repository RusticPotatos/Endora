//! Reflection context.
//!
//! A [`Reflection`] is a retrospective over observed evidence. It can surface a
//! [`ProposedProcessChange`] — but that change never takes effect on its own: a
//! human must approve it. This encodes the constitution's rule that human
//! autonomy is final and process changes require human approval.

use crate::error::{DomainError, require_non_empty};
use crate::ids::{GoalId, ObservationId, ProcessChangeId, ReflectionId};

/// A retrospective over observed evidence for a goal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reflection {
    id: ReflectionId,
    goal: GoalId,
    summary: String,
    evidence: Vec<ObservationId>,
}

impl Reflection {
    /// Creates a reflection over one or more observations.
    ///
    /// A reflection must cite at least one observation: Endora learns from
    /// evidence, not from nothing.
    ///
    /// # Errors
    /// - [`DomainError::EmptyField`] if `summary` is blank.
    /// - [`DomainError::ReflectionWithoutEvidence`] if `evidence` is empty.
    pub fn new(
        id: ReflectionId,
        goal: GoalId,
        summary: &str,
        evidence: Vec<ObservationId>,
    ) -> Result<Self, DomainError> {
        let summary = require_non_empty("reflection.summary", summary)?;
        if evidence.is_empty() {
            return Err(DomainError::ReflectionWithoutEvidence);
        }
        Ok(Self {
            id,
            goal,
            summary,
            evidence,
        })
    }

    /// The reflection's identifier.
    #[must_use]
    pub const fn id(&self) -> ReflectionId {
        self.id
    }

    /// The goal this reflection is about.
    #[must_use]
    pub const fn goal(&self) -> GoalId {
        self.goal
    }

    /// The reflection summary.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The observations this reflection is grounded in.
    #[must_use]
    pub fn evidence(&self) -> &[ObservationId] {
        &self.evidence
    }
}

/// Whether a [`ProposedProcessChange`] has been decided by a human.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalState {
    /// Awaiting a human decision. The default state of a new proposal.
    Pending,
    /// A human approved the change; it may take effect.
    Approved,
    /// A human rejected the change; it must not take effect.
    Rejected,
}

/// A change a [`Reflection`] proposes to how Endora works.
///
/// It is always *proposed*: it starts [`ApprovalState::Pending`] and only a
/// human decision moves it to approved or rejected. It never self-approves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedProcessChange {
    id: ProcessChangeId,
    reflection: ReflectionId,
    description: String,
    approval: ApprovalState,
}

impl ProposedProcessChange {
    /// Proposes a process change in the [`ApprovalState::Pending`] state.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `description` is blank.
    pub fn propose(
        id: ProcessChangeId,
        reflection: ReflectionId,
        description: &str,
    ) -> Result<Self, DomainError> {
        let description = require_non_empty("process_change.description", description)?;
        Ok(Self {
            id,
            reflection,
            description,
            approval: ApprovalState::Pending,
        })
    }

    /// The proposal's identifier.
    #[must_use]
    pub const fn id(&self) -> ProcessChangeId {
        self.id
    }

    /// The reflection that produced this proposal.
    #[must_use]
    pub const fn reflection(&self) -> ReflectionId {
        self.reflection
    }

    /// The proposed change.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The current approval state.
    #[must_use]
    pub const fn approval(&self) -> ApprovalState {
        self.approval
    }

    /// Whether the change may take effect — i.e. a human approved it.
    #[must_use]
    pub const fn is_approved(&self) -> bool {
        matches!(self.approval, ApprovalState::Approved)
    }

    /// Approves a pending proposal. This represents an explicit human decision.
    ///
    /// # Errors
    /// [`DomainError::AlreadyDecided`] if the proposal is not pending.
    pub fn approve(&mut self) -> Result<(), DomainError> {
        self.decide(ApprovalState::Approved)
    }

    /// Rejects a pending proposal.
    ///
    /// # Errors
    /// [`DomainError::AlreadyDecided`] if the proposal is not pending.
    pub fn reject(&mut self) -> Result<(), DomainError> {
        self.decide(ApprovalState::Rejected)
    }

    fn decide(&mut self, decision: ApprovalState) -> Result<(), DomainError> {
        if self.approval != ApprovalState::Pending {
            return Err(DomainError::AlreadyDecided);
        }
        self.approval = decision;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ApprovalState, ProposedProcessChange, Reflection};
    use crate::error::DomainError;
    use crate::ids::{GoalId, ObservationId, ProcessChangeId, ReflectionId};

    #[test]
    fn reflection_requires_evidence() {
        assert_eq!(
            Reflection::new(ReflectionId::new(1), GoalId::new(1), "went well", vec![]),
            Err(DomainError::ReflectionWithoutEvidence)
        );
    }

    #[test]
    fn reflection_requires_a_summary() {
        assert_eq!(
            Reflection::new(
                ReflectionId::new(1),
                GoalId::new(1),
                "   ",
                vec![ObservationId::new(1)]
            ),
            Err(DomainError::EmptyField {
                field: "reflection.summary"
            })
        );
    }

    #[test]
    fn reflection_keeps_its_evidence() {
        let r = Reflection::new(
            ReflectionId::new(1),
            GoalId::new(1),
            "mornings worked",
            vec![ObservationId::new(1), ObservationId::new(2)],
        )
        .unwrap();
        assert_eq!(
            r.evidence(),
            &[ObservationId::new(1), ObservationId::new(2)]
        );
    }

    fn pending() -> ProposedProcessChange {
        ProposedProcessChange::propose(
            ProcessChangeId::new(1),
            ReflectionId::new(1),
            "Default runs to mornings",
        )
        .unwrap()
    }

    #[test]
    fn a_new_proposal_is_pending_and_not_approved() {
        let p = pending();
        assert_eq!(p.approval(), ApprovalState::Pending);
        assert!(!p.is_approved());
    }

    #[test]
    fn approval_is_an_explicit_human_step() {
        let mut p = pending();
        p.approve().unwrap();
        assert!(p.is_approved());
        assert_eq!(p.approval(), ApprovalState::Approved);
    }

    #[test]
    fn a_decided_proposal_cannot_be_decided_again() {
        let mut p = pending();
        p.reject().unwrap();
        assert_eq!(p.approve(), Err(DomainError::AlreadyDecided));
        assert!(!p.is_approved());
    }
}
