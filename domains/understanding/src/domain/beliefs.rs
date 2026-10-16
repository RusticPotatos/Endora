//! The labels the model gives what it proposes to learn: what sort of thing it is, and how
//! sure it is. What it learns is kept on the brain's pages (ADR 0101); these labels only
//! decide what is worth keeping (a one-off ask or a passing remark is not) and let the model
//! evaluation measure how well a model judges.

/// What sort of thing a proposed fact is about. `Intent` (a one-off ask) and `Other` (a
/// passing remark) are not kept: only lasting things go on a page.
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
}

#[cfg(test)]
mod tests {
    use super::{BeliefKind, Confidence};

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
