//! The exit codes the runner reports for a turn whose worker gave none (ADR 0142, amended
//! 2026-10-31): one figure each, read alike by the runner that sends it and the team that
//! judges the turn by it.

/// The exit the runner gives a turn whose container never started (it could not start the
/// member's runtime, or its image): the worker never ran, and the turn is nobody's failure.
pub const NEVER_STARTED_EXIT: i64 = -1;

/// The exit the runner gives a turn whose container ended without an exit code it could read:
/// the worker ran, and how it ended is not known, so it is not a success.
pub const UNREADABLE_EXIT: i64 = -2;
