//! Outcomes — what actually happened after Endora acted (ADR 0053).
//!
//! A [`Belief`](crate::domain::Belief) is what Endora understands; an [`Outcome`] is
//! what it *did*, and what the world looked like afterwards. Together they are the two
//! halves of "memory learns": the first was built long ago, the second is this.
//!
//! The record keeps the actuator's **claim** and Endora's **observation** side by side,
//! verbatim, and derives no verdict from them. That is ADR 0053's reasoning carried into
//! storage: deciding *confirmed* versus *contradicted* needs a model of what the caller
//! intended, which does not exist. Keeping both unreconciled is honest, and it is the
//! raw material a later layer can reconcile against real data rather than against an
//! assumption baked in today.

use endora_kernel::error::{DomainError, require_non_empty};
use endora_kernel::ids::{BeliefId, OutcomeId, Timestamp};

/// What the person made of an action, once they say — and they are never asked
/// (ADR 0053). Absent is the normal state, not a gap to be filled.
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
/// observed (ADR 0053).
///
/// Only actions get one. A capability in the `Observe` band changes nothing, so there is
/// no outcome to record — its result is already evidence (ADR 0053).
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
    changed: Option<bool>,
}

impl Outcome {
    /// Records what an action claimed and what was observed afterwards.
    ///
    /// `observation` is `None` when nothing could read the effect back — an honest
    /// default for integrations Endora knows nothing about (ADR 0053). The reaction
    /// starts absent: the person is never asked.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `capability` is blank.
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit dependencies, no hidden state"
    )]
    pub fn record(
        id: OutcomeId,
        capability: &str,
        input: &str,
        claim: &str,
        observation: Option<&str>,
        at: Timestamp,
        motivating_belief: Option<BeliefId>,
        changed: Option<bool>,
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
            changed,
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
        changed: Option<bool>,
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
            changed,
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
    /// untrue (ADR 0053).
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

    /// Whether the world actually moved: `Some(false)` means Endora read the state
    /// before and after and they were identical, whatever the tool claimed (ADR 0054).
    /// `None` means there was nothing to compare — no reader, or no before-reading.
    #[must_use]
    pub const fn changed(&self) -> Option<bool> {
        self.changed
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
        // ADR 0053: both verbatim, no verdict derived. A claim of success alongside an
        // observation that contradicts it must survive as *both*, not collapse to one.
        let outcome = Outcome::record(
            OutcomeId::new(1),
            "home.HassTurnOff",
            "{}",
            "action_done",
            Some("kitchen switch: on"),
            at(1_000),
            None,
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
        // taught Endora about (ADR 0053), and distinguishable from "observed nothing".
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
            None,
        )
        .expect("valid");
        assert_eq!(outcome.motivating_belief(), Some(BeliefId::new(7)));
    }

    #[test]
    fn a_nameless_capability_is_rejected() {
        assert!(
            Outcome::record(
                OutcomeId::new(1),
                "  ",
                "{}",
                "done",
                None,
                at(1),
                None,
                None
            )
            .is_err(),
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

/// How the last stretch of Endora's actions actually landed (ADR 0053).
///
/// The eval battery scores the **model**; nothing scored the **system**. Without a number,
/// "more agentic" is a feeling — and reliability is the thing that decides how far autonomy
/// can safely extend, because it compounds: a step that works *p* of the time makes an
/// n-step task *pⁿ*.
///
/// **Deliberately not a single success rate.** Blending these would launder the two most
/// informative buckets into a percentage:
///
/// - a claim of success with **nothing changed** is the exact failure ADR 0053 was built for,
///   and it is not the same kind of miss as an outright error;
/// - **could not be checked** is genuinely unknown, and counting an unknown as a success is
///   how a system starts lying to itself about how well it works.
///
/// One honest caveat, visible in the numbers rather than hidden: a tool Endora has no way to
/// know is read-only counts as an actuator that changes nothing, because only a server's
/// *nominated reader* is classified as observing (ADR 0054). That inflates `unchanged`, and
/// `worst_offender` is what makes it obvious which tool is doing it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reliability {
    /// How many outcomes this covers.
    pub considered: usize,
    /// Read the world before and after, and it differed. The only bucket that is **proven**
    /// to have done what was asked.
    pub changed: usize,
    /// Reported success while the world stayed identical.
    pub unchanged: usize,
    /// Returned an error. Visible, and therefore the least dangerous kind of failure.
    pub failed: usize,
    /// No reader, so there was nothing to compare. Not a success and not a failure.
    pub unchecked: usize,
    /// The capability with the most "claimed success, changed nothing" outcomes, and how
    /// many — so the number points at something rather than just being a number.
    pub worst_offender: Option<(String, usize)>,
}

impl Reliability {
    /// Tallies the `most_recent` outcomes, newest first in `outcomes`.
    #[must_use]
    pub fn over(outcomes: &[Outcome], most_recent: usize) -> Self {
        let mut tally = Self::default();
        let mut per_capability: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for outcome in outcomes.iter().take(most_recent) {
            tally.considered += 1;
            if outcome.claim().trim_start().starts_with("error:") {
                tally.failed += 1;
                continue;
            }
            match outcome.changed() {
                Some(true) => tally.changed += 1,
                Some(false) => {
                    tally.unchanged += 1;
                    *per_capability.entry(outcome.capability()).or_default() += 1;
                }
                None => tally.unchecked += 1,
            }
        }
        tally.worst_offender = per_capability
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(id, n)| (id.to_owned(), n));
        tally
    }

    /// One line for a person: what is known, in the order that matters.
    #[must_use]
    pub fn in_words(&self) -> String {
        if self.considered == 0 {
            return "Nothing to judge yet — no actions on record.".to_owned();
        }
        let mut parts = vec![format!("{} of {} verified", self.changed, self.considered)];
        if self.unchanged > 0 {
            parts.push(format!(
                "{} claimed success but changed nothing",
                self.unchanged
            ));
        }
        if self.failed > 0 {
            parts.push(format!("{} failed outright", self.failed));
        }
        if self.unchecked > 0 {
            parts.push(format!("{} could not be checked", self.unchecked));
        }
        parts.join(" · ")
    }
}

#[cfg(test)]
mod how_the_last_stretch_landed {
    use super::*;
    fn outcome(id: u128, capability: &str, claim: &str, changed: Option<bool>) -> Outcome {
        Outcome::record(
            OutcomeId::new(id),
            capability,
            "{}",
            claim,
            changed.map(|_| "a reading"),
            Timestamp::from_unix_millis(id as i64),
            None,
            changed,
        )
        .expect("valid outcome")
    }

    #[test]
    fn the_four_buckets_stay_apart() {
        // Blending these into one percentage would launder the two most informative ones:
        // a claim of success that changed nothing is the failure ADR 0053 exists for, and
        // "could not be checked" is genuinely unknown. Counting an unknown as a success is
        // how a system starts lying to itself about how well it works.
        let history = vec![
            outcome(1, "home.HassTurnOff", "done", Some(true)),
            outcome(2, "home.HassTurnOff", "done", Some(false)),
            outcome(3, "home.HassTurnOn", "error: no matching entity", None),
            outcome(4, "home.GetDateTime", "the time is 05:40", Some(false)),
            outcome(5, "mail.Send", "sent", None),
        ];
        let tally = Reliability::over(&history, 50);

        assert_eq!(tally.considered, 5);
        assert_eq!(tally.changed, 1, "only a proven change counts as working");
        assert_eq!(tally.unchanged, 2);
        assert_eq!(tally.failed, 1);
        assert_eq!(tally.unchecked, 1, "no reader is not a success");
        assert_eq!(
            tally.changed + tally.unchanged + tally.failed + tally.unchecked,
            5
        );
    }

    #[test]
    fn it_names_what_keeps_changing_nothing() {
        // A number nobody can act on is decoration. This is also how the read-tool caveat
        // becomes visible rather than hidden: a tool Endora cannot know is read-only shows
        // up here by name.
        let history = vec![
            outcome(1, "home.GetDateTime", "the time is 05:40", Some(false)),
            outcome(2, "home.GetDateTime", "the time is 06:10", Some(false)),
            outcome(3, "home.HassTurnOff", "done", Some(false)),
        ];
        let tally = Reliability::over(&history, 50);
        assert_eq!(
            tally.worst_offender,
            Some(("home.GetDateTime".to_owned(), 2))
        );
    }

    #[test]
    fn with_nothing_on_record_it_says_so_rather_than_scoring_zero() {
        // Zero out of zero is not a bad score, and showing "0% reliable" on a fresh install
        // would be a false claim about a system that has not been asked to do anything.
        let tally = Reliability::over(&[], 50);
        assert_eq!(tally.considered, 0);
        assert!(tally.in_words().starts_with("Nothing to judge yet"));
    }

    #[test]
    fn only_the_recent_stretch_counts() {
        let history: Vec<Outcome> = (1..=10)
            .map(|i| outcome(i, "home.HassTurnOff", "done", Some(true)))
            .collect();
        assert_eq!(Reliability::over(&history, 3).considered, 3);
    }
}
