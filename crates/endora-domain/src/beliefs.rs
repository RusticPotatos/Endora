//! Understanding — the living model of the person (see the direction reset,
//! `docs/direction-reset.md`, and `docs/adr/0020-intent-first-understanding-loop.md`).
//!
//! A [`Belief`] is one thing Endora currently believes about the person — an
//! **intent**, a value, a preference, a pattern, a motivation, a frustration —
//! carrying the **evidence** that supports it, a **confidence**, and timestamps.
//! Beliefs are Endora's own model (not actions), so Endora forms them itself and
//! the person **reviews and corrects** them; they can be **affirmed** (raising
//! confidence) or left to **expire**. Nothing is assumed permanently.

use crate::error::{DomainError, require_non_empty};
use crate::ids::{BeliefId, Timestamp};

/// What sort of thing a belief is about. Intent is the most important — it changes
/// slowly and is what Endora is really trying to model (goals are a fast-changing
/// expression of it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeliefKind {
    /// What the person is ultimately trying to achieve or experience.
    Intent,
    /// A long-term value.
    Value,
    /// A taste or default they prefer.
    Preference,
    /// A recurring behavioural pattern (e.g. energy in the mornings).
    Pattern,
    /// What drives or motivates them.
    Motivation,
    /// A recurring frustration.
    Frustration,
    /// A source of stress.
    Stressor,
    /// A person or relationship that matters to them.
    Relationship,
    /// Anything else worth remembering.
    Other,
}

impl BeliefKind {
    /// Stable, lowercase name for storage and the protocol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Value => "value",
            Self::Preference => "preference",
            Self::Pattern => "pattern",
            Self::Motivation => "motivation",
            Self::Frustration => "frustration",
            Self::Stressor => "stressor",
            Self::Relationship => "relationship",
            Self::Other => "other",
        }
    }

    /// Parses a kind from its [`name`](Self::name); unknown kinds become `Other`
    /// (understanding is soft — an unrecognised label should not be lost).
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "intent" => Self::Intent,
            "value" => Self::Value,
            "preference" => Self::Preference,
            "pattern" => Self::Pattern,
            "motivation" => Self::Motivation,
            "frustration" => Self::Frustration,
            "stressor" => Self::Stressor,
            "relationship" => Self::Relationship,
            _ => Self::Other,
        }
    }
}

/// How sure Endora is. Small, human scale — interventions are sized to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// A tentative guess.
    Low,
    /// A reasonable read.
    Medium,
    /// Well-supported.
    High,
}

impl Confidence {
    /// Stable name for storage/protocol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Parses from a name; unknown becomes `Low` (be cautious when unsure).
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "high" => Self::High,
            "medium" => Self::Medium,
            _ => Self::Low,
        }
    }

    /// The next step up (affirming raises confidence; `High` stays `High`).
    #[must_use]
    pub const fn raised(self) -> Self {
        match self {
            Self::Low => Self::Medium,
            Self::Medium | Self::High => Self::High,
        }
    }
}

/// Where a belief is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeliefStatus {
    /// Currently held.
    Active,
    /// The person said it was wrong.
    Corrected,
    /// Aged out.
    Expired,
}

impl BeliefStatus {
    /// Stable name for storage/protocol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Corrected => "corrected",
            Self::Expired => "expired",
        }
    }

    /// Parses from a name; unknown becomes `Active`.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "corrected" => Self::Corrected,
            "expired" => Self::Expired,
            _ => Self::Active,
        }
    }
}

/// One thing Endora believes about the person, with its supporting evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Belief {
    id: BeliefId,
    statement: String,
    kind: BeliefKind,
    confidence: Confidence,
    evidence: String,
    created_at: Timestamp,
    last_affirmed_at: Timestamp,
    status: BeliefStatus,
}

impl Belief {
    /// Forms a new, active belief.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `statement` is blank.
    pub fn new(
        id: BeliefId,
        statement: &str,
        kind: BeliefKind,
        confidence: Confidence,
        evidence: &str,
        at: Timestamp,
    ) -> Result<Self, DomainError> {
        let statement = require_non_empty("belief.statement", statement)?;
        Ok(Self {
            id,
            statement,
            kind,
            confidence,
            evidence: evidence.trim().to_owned(),
            created_at: at,
            last_affirmed_at: at,
            status: BeliefStatus::Active,
        })
    }

    /// Reconstitutes a stored belief (all fields explicit).
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        id: BeliefId,
        statement: String,
        kind: BeliefKind,
        confidence: Confidence,
        evidence: String,
        created_at: Timestamp,
        last_affirmed_at: Timestamp,
        status: BeliefStatus,
    ) -> Self {
        Self {
            id,
            statement,
            kind,
            confidence,
            evidence,
            created_at,
            last_affirmed_at,
            status,
        }
    }

    /// The person confirmed it: raise confidence and mark it freshly affirmed.
    pub fn affirm(&mut self, at: Timestamp) {
        self.confidence = self.confidence.raised();
        self.last_affirmed_at = at;
        self.status = BeliefStatus::Active;
    }

    /// The person said it was wrong.
    pub fn correct(&mut self) {
        self.status = BeliefStatus::Corrected;
    }

    /// Its identifier.
    #[must_use]
    pub const fn id(&self) -> BeliefId {
        self.id
    }
    /// What is believed.
    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }
    /// Its kind.
    #[must_use]
    pub const fn kind(&self) -> BeliefKind {
        self.kind
    }
    /// How sure Endora is.
    #[must_use]
    pub const fn confidence(&self) -> Confidence {
        self.confidence
    }
    /// What supports it.
    #[must_use]
    pub fn evidence(&self) -> &str {
        &self.evidence
    }
    /// When it was first formed.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
    /// When it was last affirmed (or formed).
    #[must_use]
    pub const fn last_affirmed_at(&self) -> Timestamp {
        self.last_affirmed_at
    }
    /// Its status.
    #[must_use]
    pub const fn status(&self) -> BeliefStatus {
        self.status
    }
}

#[cfg(test)]
mod tests {
    use super::{Belief, BeliefKind, BeliefStatus, Confidence};
    use crate::error::DomainError;
    use crate::ids::{BeliefId, Timestamp};

    #[test]
    fn forms_and_keeps_fields() {
        let at = Timestamp::from_unix_millis(1_000);
        let b = Belief::new(
            BeliefId::new(1),
            "  wants enough energy to travel  ",
            BeliefKind::Intent,
            Confidence::Medium,
            "mentioned fatigue; enjoys hiking",
            at,
        )
        .unwrap();
        assert_eq!(b.statement(), "wants enough energy to travel");
        assert_eq!(b.kind(), BeliefKind::Intent);
        assert_eq!(b.confidence(), Confidence::Medium);
        assert_eq!(b.status(), BeliefStatus::Active);
    }

    #[test]
    fn affirming_raises_confidence() {
        let mut b = Belief::new(
            BeliefId::new(1),
            "prefers mornings",
            BeliefKind::Pattern,
            Confidence::Low,
            "",
            Timestamp::from_unix_millis(0),
        )
        .unwrap();
        b.affirm(Timestamp::from_unix_millis(10));
        assert_eq!(b.confidence(), Confidence::Medium);
        b.affirm(Timestamp::from_unix_millis(20));
        assert_eq!(b.confidence(), Confidence::High);
        b.affirm(Timestamp::from_unix_millis(30));
        assert_eq!(b.confidence(), Confidence::High);
        assert_eq!(b.last_affirmed_at(), Timestamp::from_unix_millis(30));
    }

    #[test]
    fn correcting_marks_status() {
        let mut b = Belief::new(
            BeliefId::new(1),
            "dislikes crowds",
            BeliefKind::Preference,
            Confidence::High,
            "",
            Timestamp::from_unix_millis(0),
        )
        .unwrap();
        b.correct();
        assert_eq!(b.status(), BeliefStatus::Corrected);
    }

    #[test]
    fn rejects_blank_statement() {
        assert_eq!(
            Belief::new(
                BeliefId::new(1),
                "   ",
                BeliefKind::Other,
                Confidence::Low,
                "",
                Timestamp::from_unix_millis(0)
            ),
            Err(DomainError::EmptyField {
                field: "belief.statement"
            })
        );
    }

    #[test]
    fn kind_and_confidence_names_round_trip() {
        for k in [BeliefKind::Intent, BeliefKind::Value, BeliefKind::Pattern] {
            assert_eq!(BeliefKind::from_name(k.name()), k);
        }
        assert_eq!(BeliefKind::from_name("bogus"), BeliefKind::Other);
        for c in [Confidence::Low, Confidence::Medium, Confidence::High] {
            assert_eq!(Confidence::from_name(c.name()), c);
        }
    }
}
