//! Outcomes — what actually happened after Endora acted (ADR 0035).
//!
//! A [`Belief`](crate::domain::Belief) is what Endora understands; an [`Outcome`] is
//! what it *did*, and what the world looked like afterwards. Together they are the two
//! halves of "memory learns": the first was built long ago, the second is this.
//!
//! The record keeps the actuator's **claim** and Endora's **observation** side by side,
//! verbatim, and derives no verdict from them. That is ADR 0034's reasoning carried into
//! storage: deciding *confirmed* versus *contradicted* needs a model of what the caller
//! intended, which does not exist. Keeping both unreconciled is honest, and it is the
//! raw material a later layer can reconcile against real data rather than against an
//! assumption baked in today.

use endora_kernel::error::{DomainError, require_non_empty};
use endora_kernel::ids::{BeliefId, OutcomeId, Timestamp};

/// What the person made of an action, once they say — and they are never asked
/// (ADR 0035). Absent is the normal state, not a gap to be filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    /// It helped.
    Helped,
    /// It did not help.
    DidNotHelp,
    /// They saw it and it made no difference either way.
    NoReaction,
}

impl Reaction {
    /// A stable, lowercase name for storage and interfaces. The round trip with
    /// [`from_name`](Self::from_name) is part of the contract.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Helped => "helped",
            Self::DidNotHelp => "did_not_help",
            Self::NoReaction => "no_reaction",
        }
    }

    /// Parses a reaction from its [`name`](Self::name), or `None` if unrecognized.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "helped" => Some(Self::Helped),
            "did_not_help" => Some(Self::DidNotHelp),
            "no_reaction" => Some(Self::NoReaction),
            _ => None,
        }
    }
}

/// One thing Endora did, with the actuator's claim about it and what Endora then
/// observed (ADR 0035).
///
/// Only actions get one. A capability in the `Observe` band changes nothing, so there is
/// no outcome to record — its result is already evidence (ADR 0034).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    id: OutcomeId,
    capability: String,
    input: String,
    claim: String,
    observation: Option<String>,
    at: Timestamp,
    motivating_belief: Option<BeliefId>,
    reaction: Option<Reaction>,
}

impl Outcome {
    /// Records what an action claimed and what was observed afterwards.
    ///
    /// `observation` is `None` when nothing could read the effect back — an honest
    /// default for integrations Endora knows nothing about (ADR 0034). The reaction
    /// starts absent: the person is never asked.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `capability` is blank.
    pub fn record(
        id: OutcomeId,
        capability: &str,
        input: &str,
        claim: &str,
        observation: Option<&str>,
        at: Timestamp,
        motivating_belief: Option<BeliefId>,
    ) -> Result<Self, DomainError> {
        let capability = require_non_empty("outcome.capability", capability)?;
        Ok(Self {
            id,
            capability,
            input: input.trim().to_owned(),
            claim: claim.trim().to_owned(),
            observation: observation.map(|o| o.trim().to_owned()),
            at,
            motivating_belief,
            reaction: None,
        })
    }

    /// Reconstitutes a stored outcome (all fields explicit).
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn from_parts(
        id: OutcomeId,
        capability: String,
        input: String,
        claim: String,
        observation: Option<String>,
        at: Timestamp,
        motivating_belief: Option<BeliefId>,
        reaction: Option<Reaction>,
    ) -> Self {
        Self {
            id,
            capability,
            input,
            claim,
            observation,
            at,
            motivating_belief,
            reaction,
        }
    }

    /// The person said how it landed. Idempotent and re-settable — they may change
    /// their mind, and the latest word wins.
    pub const fn react(&mut self, reaction: Reaction) {
        self.reaction = Some(reaction);
    }

    /// Whether Endora saw the effect for itself, rather than only being told about it.
    #[must_use]
    pub const fn was_observed(&self) -> bool {
        self.observation.is_some()
    }

    /// The outcome's identifier.
    #[must_use]
    pub const fn id(&self) -> OutcomeId {
        self.id
    }

    /// The capability that acted.
    #[must_use]
    pub fn capability(&self) -> &str {
        &self.capability
    }

    /// The input it was called with.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    /// The actuator's own account of its work — which is exactly the thing that can be
    /// untrue (ADR 0034).
    #[must_use]
    pub fn claim(&self) -> &str {
        &self.claim
    }

    /// What Endora observed afterwards, if anything could read the effect back.
    #[must_use]
    pub fn observation(&self) -> Option<&str> {
        self.observation.as_deref()
    }

    /// When it happened.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }

    /// The belief that motivated the action, when it traces to one.
    #[must_use]
    pub const fn motivating_belief(&self) -> Option<BeliefId> {
        self.motivating_belief
    }

    /// What the person made of it, if they have said.
    #[must_use]
    pub const fn reaction(&self) -> Option<Reaction> {
        self.reaction
    }
}

#[cfg(test)]
mod tests {
    use super::{Outcome, Reaction};
    use endora_kernel::ids::{BeliefId, OutcomeId, Timestamp};

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_unix_millis(ms)
    }

    fn recorded() -> Outcome {
        Outcome::record(
            OutcomeId::new(1),
            "home.HassTurnOff",
            r#"{"name":"kitchen"}"#,
            "action_done",
            None,
            at(1_000),
            None,
        )
        .expect("a named capability is valid")
    }

    #[test]
    fn an_outcome_starts_with_no_reaction_because_the_person_is_never_asked() {
        assert_eq!(recorded().reaction(), None);
    }

    #[test]
    fn the_claim_and_the_observation_are_kept_apart() {
        // ADR 0035: both verbatim, no verdict derived. A claim of success alongside an
        // observation that contradicts it must survive as *both*, not collapse to one.
        let outcome = Outcome::record(
            OutcomeId::new(1),
            "home.HassTurnOff",
            "{}",
            "action_done",
            Some("kitchen switch: on"),
            at(1_000),
            None,
        )
        .expect("valid");
        assert_eq!(outcome.claim(), "action_done");
        assert_eq!(outcome.observation(), Some("kitchen switch: on"));
        assert!(outcome.was_observed());
    }

    #[test]
    fn an_unverifiable_action_is_recorded_as_unobserved() {
        // Nothing could read the effect back — honest for integrations nobody has
        // taught Endora about (ADR 0034), and distinguishable from "observed nothing".
        let outcome = recorded();
        assert_eq!(outcome.observation(), None);
        assert!(!outcome.was_observed());
    }

    #[test]
    fn the_person_can_say_how_it_landed_and_change_their_mind() {
        let mut outcome = recorded();
        outcome.react(Reaction::Helped);
        assert_eq!(outcome.reaction(), Some(Reaction::Helped));
        // The latest word wins.
        outcome.react(Reaction::DidNotHelp);
        assert_eq!(outcome.reaction(), Some(Reaction::DidNotHelp));
    }

    #[test]
    fn an_action_can_trace_to_the_belief_that_motivated_it() {
        let outcome = Outcome::record(
            OutcomeId::new(1),
            "weather",
            "{}",
            "done",
            None,
            at(1),
            Some(BeliefId::new(7)),
        )
        .expect("valid");
        assert_eq!(outcome.motivating_belief(), Some(BeliefId::new(7)));
    }

    #[test]
    fn a_nameless_capability_is_rejected() {
        assert!(
            Outcome::record(OutcomeId::new(1), "  ", "{}", "done", None, at(1), None).is_err(),
            "an outcome with no capability names nothing and can't be reasoned over"
        );
    }

    #[test]
    fn reaction_names_round_trip() {
        for reaction in [Reaction::Helped, Reaction::DidNotHelp, Reaction::NoReaction] {
            assert_eq!(Reaction::from_name(reaction.name()), Some(reaction));
        }
        assert_eq!(Reaction::from_name("bogus"), None);
    }
}
