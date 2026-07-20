//! Typed identifiers and time for the domain.
//!
//! Identifiers are opaque values **supplied by outer layers** — the domain never
//! generates them, and it never reads the system clock. Time enters the domain
//! only as a [`Timestamp`] passed in by the caller. This keeps the domain pure
//! and fully deterministic in tests.

/// Declares an opaque, strongly-typed identifier newtype over `u128`.
///
/// Distinct id types (e.g. [`TargetId`] vs [`ExperimentId`]) cannot be mixed up,
/// which is the point: the compiler rejects passing one where another is meant.
macro_rules! id_type {
    ($name:ident, $what:literal) => {
        #[doc = concat!("Opaque identifier for a ", $what, ".")]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u128);

        impl $name {
            #[doc = concat!("Wraps a raw value as a ", $what, " identifier.")]
            #[must_use]
            pub const fn new(value: u128) -> Self {
                Self(value)
            }

            /// Returns the raw identifier value.
            #[must_use]
            pub const fn value(self) -> u128 {
                self.0
            }
        }
    };
}

id_type!(ValueId, "value");
id_type!(MessageId, "message");
id_type!(PreferenceId, "preference");
id_type!(DirectionId, "direction");
id_type!(TargetId, "target");
id_type!(AssumptionId, "assumption");
id_type!(ExperimentId, "experiment");
id_type!(ObservationId, "observation");
id_type!(ReflectionId, "reflection");
id_type!(SuggestionId, "suggestion");
id_type!(ProcessChangeId, "proposed process change");
id_type!(AuditId, "audit record");

/// A point in time, as milliseconds since the Unix epoch, **supplied by the
/// caller**. The domain never reads the system clock, so its behavior stays
/// deterministic and testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Creates a timestamp from Unix epoch milliseconds.
    #[must_use]
    pub const fn from_unix_millis(millis: i64) -> Self {
        Self(millis)
    }

    /// Returns the Unix epoch milliseconds.
    #[must_use]
    pub const fn unix_millis(self) -> i64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{TargetId, Timestamp};

    #[test]
    fn id_round_trips_its_value() {
        assert_eq!(TargetId::new(42).value(), 42);
    }

    #[test]
    fn timestamp_round_trips_its_value() {
        assert_eq!(
            Timestamp::from_unix_millis(1_700_000_000_000).unix_millis(),
            1_700_000_000_000
        );
    }
}
