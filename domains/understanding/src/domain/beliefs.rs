//! Understanding — the living model of the person (see the direction reset,
//! `docs/direction-reset.md`, and `docs/adr/0020-intent-first-understanding-loop.md`).
//!
//! A [`Belief`] is one thing Endora currently believes about the person — an
//! **intent**, a value, a preference, a pattern, a motivation, a frustration —
//! carrying the **evidence** that supports it, a **confidence**, and timestamps.
//! Beliefs are Endora's own model (not actions), so Endora forms them itself and
//! the person **reviews and corrects** them; they can be **affirmed** (raising
//! confidence) or left to **expire**. Nothing is assumed permanently.

use endora_kernel::error::{DomainError, require_non_empty};
use endora_kernel::ids::{BeliefId, Timestamp};

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

/// One day, in milliseconds — the unit the belief half-lives are written in.
const DAY_MS: i64 = 24 * 60 * 60 * 1_000;

impl BeliefKind {
    /// How long a belief of this kind stays trustworthy **without reinforcement**,
    /// before Endora should be one step less sure of it.
    ///
    /// The rates encode what ADR 0052 says about the person: **intent and values
    /// change slowly** and are what Endora is really modelling, so they hold for a
    /// long time. A **frustration or stressor is often about a particular week** —
    /// continuing to act on "you're stressed about the move" six months after the
    /// move is worse than having forgotten it. Preferences and patterns sit between.
    ///
    /// These are deliberately generous. Forgetting something true is a smaller harm
    /// than confidently holding something stale, but both are harms, and the person
    /// can always affirm a belief to reset its clock.
    #[must_use]
    pub const fn half_life_ms(self) -> i64 {
        match self {
            Self::Intent | Self::Value => 365 * DAY_MS,
            Self::Motivation | Self::Relationship => 180 * DAY_MS,
            Self::Preference | Self::Pattern => 120 * DAY_MS,
            Self::Other => 90 * DAY_MS,
            Self::Frustration | Self::Stressor => 45 * DAY_MS,
        }
    }

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

    /// The next step down, or `None` once it falls below `Low` — at which point
    /// Endora no longer believes it at all. Unlike [`raised`](Self::raised), which
    /// saturates at `High`, this bottoms out into nothing: a belief that keeps
    /// weakening with nothing to support it should eventually be let go, not held
    /// forever at `low`.
    #[must_use]
    pub const fn lowered(self) -> Option<Self> {
        match self {
            Self::High => Some(Self::Medium),
            Self::Medium => Some(Self::Low),
            Self::Low => None,
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

    /// Whether this is **settled** — held with high confidence and reinforced at least
    /// once since it was formed (ADR 0052).
    ///
    /// Written after a screen of cards, every one of them high-confidence and confirmed
    /// more than once, all still offering *That's right / Not quite*. Asking about
    /// something already settled is the queue behaviour this context exists to prevent,
    /// wearing a different costume: it is a list of chores that never empties, because
    /// answering an item does not remove it.
    ///
    /// The asymmetry is the point. **Affirming a settled belief adds nothing** — the
    /// confidence is already at the top and the evidence already exists — so the prompt is
    /// pure cost. **Correcting one always matters**, however sure Endora was, so that stays
    /// available everywhere and forever.
    ///
    /// `last_affirmed_at > created_at` is exactly "reinforced since forming": both are set
    /// to the same instant at birth, and only [`affirm`](Self::affirm) moves the second.
    ///
    /// Judged on the **decayed** confidence, so a settled belief that fades with time
    /// becomes a question again by itself. That is the right way round: what stops Endora
    /// asking is being sure *now*, not having once been sure.
    #[must_use]
    pub fn is_settled(&self, now: Timestamp) -> bool {
        self.confidence_at(now) == Some(Confidence::High)
            && self.last_affirmed_at.unix_millis() > self.created_at.unix_millis()
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

    /// How many half-lives have passed since this belief was last reinforced.
    fn half_lives_elapsed(&self, now: Timestamp) -> i64 {
        let since = now.unix_millis() - self.last_affirmed_at.unix_millis();
        if since <= 0 {
            return 0;
        }
        since / self.kind.half_life_ms()
    }

    /// How sure Endora should be **right now**, given how long it has been since
    /// anything reinforced this belief.
    ///
    /// Confidence steps down one level per elapsed half-life. This is what makes
    /// understanding a *living* model rather than a list that only grows: a belief
    /// nothing has supported for a year should not still be presented as `high`.
    /// Purely derived from the stored timestamp, so it needs no background job to
    /// stay honest and cannot drift from what is on disk.
    #[must_use]
    pub fn confidence_at(&self, now: Timestamp) -> Option<Confidence> {
        let mut confidence = self.confidence;
        for _ in 0..self.half_lives_elapsed(now) {
            confidence = confidence.lowered()?;
        }
        Some(confidence)
    }

    /// Whether this belief has decayed past the point of being worth holding —
    /// faded below `Low` with nothing reinforcing it.
    ///
    /// A belief the person **corrected** is already out of `understanding`, and one
    /// they **affirmed** has had its clock reset, so this only catches beliefs that
    /// simply stopped being true and were never mentioned again.
    #[must_use]
    pub fn has_faded(&self, now: Timestamp) -> bool {
        self.status == BeliefStatus::Active && self.confidence_at(now).is_none()
    }

    /// Marks a faded belief as aged out. Endora forgets it rather than continuing to
    /// act on something nothing has supported in a long time.
    pub fn expire(&mut self) {
        self.status = BeliefStatus::Expired;
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
    use endora_kernel::error::DomainError;
    use endora_kernel::ids::{BeliefId, Timestamp};

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

    // --- Decay and expiry (ADR 0052) ---

    const DAY: i64 = 24 * 60 * 60 * 1_000;

    fn belief_of(kind: BeliefKind, confidence: Confidence) -> Belief {
        Belief::new(
            BeliefId::new(1),
            "you want more energy",
            kind,
            confidence,
            "said so",
            Timestamp::from_unix_millis(0),
        )
        .unwrap()
    }

    #[test]
    fn confidence_steps_down_once_per_half_life_without_reinforcement() {
        let b = belief_of(BeliefKind::Preference, Confidence::High);
        let hl = BeliefKind::Preference.half_life_ms();
        assert_eq!(
            b.confidence_at(Timestamp::from_unix_millis(0)),
            Some(Confidence::High)
        );
        // Just short of a half-life it is unchanged.
        assert_eq!(
            b.confidence_at(Timestamp::from_unix_millis(hl - 1)),
            Some(Confidence::High)
        );
        assert_eq!(
            b.confidence_at(Timestamp::from_unix_millis(hl)),
            Some(Confidence::Medium)
        );
        assert_eq!(
            b.confidence_at(Timestamp::from_unix_millis(2 * hl)),
            Some(Confidence::Low)
        );
        // Past Low it is no longer believed at all.
        assert_eq!(b.confidence_at(Timestamp::from_unix_millis(3 * hl)), None);
    }

    #[test]
    fn intent_outlives_a_stressor() {
        // ADR 0052: intent is the slow-changing thing worth modelling; a stressor is
        // often about a particular week. A year on, one should survive and one should
        // not, from the same starting confidence.
        let year = Timestamp::from_unix_millis(365 * DAY);
        let intent = belief_of(BeliefKind::Intent, Confidence::High);
        let stressor = belief_of(BeliefKind::Stressor, Confidence::High);
        assert!(
            !intent.has_faded(year),
            "intent should still be held after a year"
        );
        assert!(
            stressor.has_faded(year),
            "a year-old stressor should have faded"
        );
    }

    #[test]
    fn affirming_resets_the_clock() {
        let hl = BeliefKind::Preference.half_life_ms();
        let mut b = belief_of(BeliefKind::Preference, Confidence::High);
        let late = Timestamp::from_unix_millis(3 * hl);
        assert!(b.has_faded(late));
        // The person says it is still right — it is current again, not fading.
        b.affirm(late);
        assert!(!b.has_faded(late));
        assert_eq!(b.confidence_at(late), Some(Confidence::High));
    }

    #[test]
    fn a_corrected_belief_is_not_also_reported_as_faded() {
        // It is already out of understanding; calling it "faded" would double-count
        // and put a wrong belief in the expiry trail as though time removed it.
        let mut b = belief_of(BeliefKind::Stressor, Confidence::Low);
        b.correct();
        assert!(!b.has_faded(Timestamp::from_unix_millis(999 * DAY)));
    }

    #[test]
    fn expiring_marks_it_aged_out_rather_than_corrected() {
        // The distinction matters: "you were wrong" and "this stopped being true"
        // are different things to have recorded about a person.
        let mut b = belief_of(BeliefKind::Stressor, Confidence::Low);
        b.expire();
        assert_eq!(b.status(), BeliefStatus::Expired);
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_age_anything() {
        let b = Belief::new(
            BeliefId::new(1),
            "you like tea",
            BeliefKind::Preference,
            Confidence::Medium,
            "said so",
            Timestamp::from_unix_millis(10 * DAY),
        )
        .unwrap();
        assert_eq!(
            b.confidence_at(Timestamp::from_unix_millis(0)),
            Some(Confidence::Medium)
        );
    }

    #[test]
    fn confidence_bottoms_out_rather_than_saturating() {
        assert_eq!(Confidence::High.lowered(), Some(Confidence::Medium));
        assert_eq!(Confidence::Medium.lowered(), Some(Confidence::Low));
        assert_eq!(Confidence::Low.lowered(), None);
    }
}
