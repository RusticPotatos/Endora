//! Reversibility bands — the primary axis of the autonomy envelope.
//!
//! Endora's constitution states that models *propose* and deterministic policy
//! *authorizes* (ADR 0051). [`AutonomyLevel`](crate::AutonomyLevel) says how much
//! standing authority a component is granted; this module says how *undoable* a
//! given action is — the axis ADR 0051 makes primary. The person's rule is
//! **experiment freely, never do the un-undoable**, so an action is graded by
//! whether its effect can be taken back, and that grade maps to a [`Decision`]:
//! act on its own, ask first, or refuse outright.
//!
//! Like [`AutonomyLevel`](crate::AutonomyLevel), these are plain, deterministic
//! value types — no model, transport, or policy engine is referenced here. A band
//! comes from a capability's *declared metadata*, never a model's self-report
//! (ADR 0051). The Policy & Consent and Capabilities contexts consume these to
//! decide what may run without a human.

use core::cmp::Ordering;

/// How undoable an action's effect is — the primary axis of the autonomy
/// envelope (ADR 0051).
///
/// Bands are ordered from least to most consequential; a more consequential band
/// is never treated more permissively than a less consequential one. The band is
/// declared in a capability's metadata, never inferred by a model.
///
/// [`Default`] is [`Irreversible`](Self::Irreversible): anything unknown or
/// unclassifiable is treated as the un-undoable and therefore blocked, so the
/// failure mode is always "it asked / it didn't act," never "it did something
/// permanent."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reversibility {
    /// Reads and thinks only — no effect on the world at all.
    Observe,
    /// Internal effect with an undo or no lasting trace: research, drafts,
    /// beliefs, experiments. The *experiment band* — autonomous by default.
    Reversible,
    /// An outward effect the person can still undo (e.g. a draft posted somewhere
    /// deletable). Reaches beyond the device but is recoverable — confirm first.
    OutwardReversible,
    /// Sends, spends, or edits/deletes external state. Cannot be taken back —
    /// blocked by default ("never, for now"), not merely confirmed.
    Irreversible,
}

impl Default for Reversibility {
    /// Deny-by-default: an unclassifiable action is treated as the un-undoable.
    fn default() -> Self {
        Self::Irreversible
    }
}

/// What deterministic policy does with an action of a given [`Reversibility`]:
/// run it, surface it for confirmation, or refuse it. A model's proposal never
/// changes this answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Decision {
    /// May run on its own — no human confirmation required.
    Act,
    /// Must be surfaced for explicit human confirmation before it takes effect.
    Confirm,
    /// Refused outright. Not offered for confirmation — "never, for now."
    Block,
}

impl Reversibility {
    /// Rank from least (`0`) to most consequential. Used to order and compare
    /// bands so a widening rule can never make a band more permissive by accident.
    const fn rank(self) -> u8 {
        match self {
            Self::Observe => 0,
            Self::Reversible => 1,
            Self::OutwardReversible => 2,
            Self::Irreversible => 3,
        }
    }

    /// The **default posture** for this band, independent of the person's
    /// envelope (ADR 0051): experiment freely, confirm outward-but-reversible
    /// effects, and block the un-undoable.
    ///
    /// The envelope may *widen* a band's disposition (e.g. auto-approve outward
    /// reversible actions), but the [`Irreversible`](Self::Irreversible) band
    /// stays [`Block`](Decision::Block) until the person explicitly opens it,
    /// per-capability — so widening is applied on top of this, never below it.
    #[must_use]
    pub const fn default_decision(self) -> Decision {
        match self {
            Self::Observe | Self::Reversible => Decision::Act,
            Self::OutwardReversible => Decision::Confirm,
            Self::Irreversible => Decision::Block,
        }
    }

    /// A stable, lowercase name for the band, for interfaces, storage, and the
    /// audit trail. The round trip with [`from_name`](Self::from_name) is part of
    /// the contract.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Reversible => "reversible",
            Self::OutwardReversible => "outward_reversible",
            Self::Irreversible => "irreversible",
        }
    }

    /// Parses a band from its [`name`](Self::name), or `None` if unrecognized.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "observe" => Some(Self::Observe),
            "reversible" => Some(Self::Reversible),
            "outward_reversible" => Some(Self::OutwardReversible),
            "irreversible" => Some(Self::Irreversible),
            _ => None,
        }
    }
}

impl Decision {
    /// A stable, lowercase name for the decision, for interfaces, storage, and the
    /// audit trail. The round trip with [`from_name`](Self::from_name) is part of
    /// the contract.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Act => "act",
            Self::Confirm => "confirm",
            Self::Block => "block",
        }
    }

    /// Parses a decision from its [`name`](Self::name), or `None` if unrecognized.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "act" => Some(Self::Act),
            "confirm" => Some(Self::Confirm),
            "block" => Some(Self::Block),
            _ => None,
        }
    }
}

impl PartialOrd for Reversibility {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Reversibility {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}

#[cfg(test)]
mod tests {
    use super::Decision;
    use super::Reversibility::{self, Irreversible, Observe, OutwardReversible, Reversible};

    #[test]
    fn the_default_band_is_the_un_undoable_so_unknowns_are_blocked() {
        // Deny-by-default (ADR 0051): anything unclassifiable is treated as
        // irreversible, which blocks.
        assert_eq!(Reversibility::default(), Irreversible);
        assert_eq!(Reversibility::default().default_decision(), Decision::Block);
    }

    #[test]
    fn each_band_maps_to_the_person_s_rule() {
        // Experiment freely...
        assert_eq!(Observe.default_decision(), Decision::Act);
        assert_eq!(Reversible.default_decision(), Decision::Act);
        // ...confirm outward-but-recoverable effects...
        assert_eq!(OutwardReversible.default_decision(), Decision::Confirm);
        // ...never the un-undoable.
        assert_eq!(Irreversible.default_decision(), Decision::Block);
    }

    #[test]
    fn irreversible_is_blocked_not_merely_confirmed() {
        // The distinguishing invariant of ADR 0051: the un-undoable is refused
        // outright, not offered for a (mistakable) confirmation.
        assert_ne!(Irreversible.default_decision(), Decision::Confirm);
        assert_eq!(Irreversible.default_decision(), Decision::Block);
    }

    #[test]
    fn bands_are_ordered_from_least_to_most_consequential() {
        assert!(Observe < Reversible);
        assert!(Reversible < OutwardReversible);
        assert!(OutwardReversible < Irreversible);
    }

    #[test]
    fn band_names_round_trip() {
        for band in [Observe, Reversible, OutwardReversible, Irreversible] {
            assert_eq!(Reversibility::from_name(band.name()), Some(band));
        }
        assert_eq!(Reversibility::from_name("bogus"), None);
    }

    #[test]
    fn decision_names_round_trip() {
        for decision in [Decision::Act, Decision::Confirm, Decision::Block] {
            assert_eq!(Decision::from_name(decision.name()), Some(decision));
        }
        assert_eq!(Decision::from_name("bogus"), None);
    }
}
