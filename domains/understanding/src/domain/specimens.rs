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
    /// Finished with: it passed a replay, or it failed enough replays that
    /// re-asking nightly stopped being information.
    pub retired: bool,
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
