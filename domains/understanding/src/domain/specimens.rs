//! Specimens — asks the butler failed, kept so it can notice itself improving
//! (ADR 0075).
//!
//! A turn that ends at the honesty valve is a measurement the fleet throws away:
//! the person asked something real, the machinery could not answer it, and the only
//! record was a log line. A specimen keeps that ask — **privately, in the house's
//! own database, never in a checked-in fixture** (the constitution forbids
//! harvesting conversations into git) — so the nightly loop can re-ask it and
//! notice when it starts passing. The verdict that files one is the same
//! deterministic check that gated retries and escalation; the model's opinion of
//! itself files nothing.

/// Where a specimen has got to.
///
/// One field with three states, not a `retired` flag, because "finished with" and
/// "finished **how**" are different questions and only the second one is evidence.
/// A specimen that ran out of replays is the live record saying *this house asks
/// this and the machinery cannot answer it* — the trigger
/// [ADR 0071](../../../docs/adr/0071-capabilities-it-writes-itself.md) draws a
/// recipe proposal from. A specimen that passed is the opposite: a gap that closed
/// on its own. Under the flag those two were the same row, so the arithmetic could
/// only tell them apart by comparing `replays` against
/// [`REPLAYS_BEFORE_GIVING_UP`] — which is wrong in exactly the case that matters,
/// since a specimen that *passes on its last allowed replay* lands on that number
/// too. It would have proposed a new capability for a gap that had just closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecimenState {
    /// Still worth re-asking; the nightly loop replays these, oldest first.
    Open,
    /// A replay passed — the machinery caught up, and there is nothing to prove.
    Answered,
    /// Enough replays failed that re-asking stopped being information. The only
    /// state that is evidence of a real gap.
    GaveUp,
}

impl SpecimenState {
    /// Stable name for storage/protocol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Answered => "answered",
            Self::GaveUp => "gave_up",
        }
    }

    /// Parses from a name; unknown becomes `Open`, which only costs a replay.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "answered" => Self::Answered,
            "gave_up" => Self::GaveUp,
            _ => Self::Open,
        }
    }

    /// Whether the nightly loop should still re-ask this one.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }

    /// What one replay leaves behind, given how many had already been tried.
    ///
    /// The retirement rule itself, in the one place both the store and its test
    /// doubles can read it: passing finishes a specimen at any count, and so does
    /// the [`REPLAYS_BEFORE_GIVING_UP`]th failure. Deriving it here rather than in
    /// SQL is what lets "it gave up" be *recorded* rather than inferred later from
    /// a count.
    #[must_use]
    pub const fn after_replay(replays_before: u32, passed: bool) -> Self {
        if passed {
            return Self::Answered;
        }
        if replays_before + 1 >= REPLAYS_BEFORE_GIVING_UP {
            return Self::GaveUp;
        }
        Self::Open
    }
}

/// One ask the butler failed, and how its replays have gone since.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Specimen {
    /// Stable id, from the same source as every other record.
    pub id: String,
    /// The person's ask, verbatim.
    pub asked: String,
    /// Which deterministic check rejected the turn — for the operator, not the model.
    pub verdict: String,
    /// When it was filed.
    pub filed_ms: i64,
    /// How many times the nightly loop has re-asked it.
    pub replays: u32,
    /// The last replay, if any.
    pub last_replay_ms: Option<i64>,
    /// Open, answered, or given up on.
    pub state: SpecimenState,
}

/// How many specimens may be open at once. A shelf, not an archive: past this,
/// new failures are not filed — the loop replays one per night, and a backlog
/// deeper than this is a signal to fix the machinery, not to queue more evidence.
pub const MOST_SPECIMENS_OPEN: usize = 12;

/// How many failed replays before a specimen retires unresolved. A question that
/// still fails after two weeks of nightly attempts is not going to be fixed by
/// asking again; it retires so the shelf stays useful, and the activity trail
/// carries the fact.
pub const REPLAYS_BEFORE_GIVING_UP: u32 = 14;

#[cfg(test)]
mod tests {
    use super::{REPLAYS_BEFORE_GIVING_UP, SpecimenState};

    #[test]
    fn state_round_trips_by_name() {
        for state in [
            SpecimenState::Open,
            SpecimenState::Answered,
            SpecimenState::GaveUp,
        ] {
            assert_eq!(SpecimenState::from_name(state.name()), state);
        }
        // An unreadable name costs a replay, never a wrong gap.
        assert_eq!(SpecimenState::from_name("nonsense"), SpecimenState::Open);
    }

    #[test]
    fn a_failure_keeps_it_open_until_the_replays_run_out() {
        assert_eq!(SpecimenState::after_replay(0, false), SpecimenState::Open);
        assert_eq!(
            SpecimenState::after_replay(REPLAYS_BEFORE_GIVING_UP - 2, false),
            SpecimenState::Open
        );
        assert_eq!(
            SpecimenState::after_replay(REPLAYS_BEFORE_GIVING_UP - 1, false),
            SpecimenState::GaveUp,
            "the last allowed failure is the one that gives up"
        );
    }

    #[test]
    fn passing_on_the_last_allowed_replay_is_answered_not_a_gap() {
        // The case the `retired` flag could not express, and the reason this enum
        // exists: both of these used to land on `replays = REPLAYS_BEFORE_GIVING_UP`
        // with `retired = 1`, so anything reading the count would have called this
        // pass a proven gap and proposed a capability for it (ADR 0071).
        assert_eq!(
            SpecimenState::after_replay(REPLAYS_BEFORE_GIVING_UP - 1, true),
            SpecimenState::Answered
        );
        assert_eq!(
            SpecimenState::after_replay(REPLAYS_BEFORE_GIVING_UP - 1, false),
            SpecimenState::GaveUp
        );
    }

    #[test]
    fn only_open_is_replayed_again() {
        assert!(SpecimenState::Open.is_open());
        assert!(!SpecimenState::Answered.is_open());
        assert!(!SpecimenState::GaveUp.is_open());
    }
}
