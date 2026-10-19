//! Runs: one turn of a member, from queued to how it ended.

use super::work::Millis;

/// At most this many retries after a turn fails.
pub const MOST_RETRIES: u8 = 2;
/// A retry waits this long after the failure.
pub const RETRY_AFTER_MS: Millis = 60_000;
/// A turn longer than this is stopped and counted as timed out.
pub const LONGEST_TURN_MS: Millis = 30 * 60_000;

/// Where a run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// Waiting for a slot, budget and a runner.
    Queued,
    /// Taking its turn.
    Running,
    /// Waiting to be tried again.
    RetryWaiting,
    /// Ended as it should.
    Succeeded,
    /// Ended in failure, with no retries left.
    Failed,
    /// Stopped: by the kill switch, the owner, or a limit crossed.
    Stopped,
    /// Went on longer than a turn may.
    TimedOut,
}

impl RunStatus {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::RetryWaiting => "retry_waiting",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
            Self::TimedOut => "timed_out",
        }
    }

    /// The status with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::Running,
            Self::RetryWaiting,
            Self::Succeeded,
            Self::Failed,
            Self::Stopped,
            Self::TimedOut,
        ]
        .into_iter()
        .find(|s| s.name() == name)
    }

    /// Whether the run is still live: it holds its task's one turn.
    #[must_use]
    pub const fn live(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::RetryWaiting)
    }
}

/// What the runner says about a turn it was asked to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Still taking its turn.
    Running,
    /// Ended on its own: well, or not.
    Finished {
        /// Whether it ended as it should.
        succeeded: bool,
    },
    /// Its container is gone without a word.
    Gone,
}

/// What happens to a run whose turn failed after `attempts` tries: retried after a minute
/// (`Some(due)`), or failed for good (`None`).
#[must_use]
pub fn retry_after_failure(attempts: u8, now: Millis) -> Option<Millis> {
    (attempts <= MOST_RETRIES).then_some(now + RETRY_AFTER_MS)
}

/// How a turn's worker ended, as far as its runner said (ADR 0102).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// The runner gave no exit code: an older runner, or a worker it could not read.
    Unsaid,
    /// The worker's container was gone before the runner saw it end: removed, or lost with a
    /// restart.
    Vanished,
    /// The worker exited with this code.
    Code(i64),
}

/// Whose it was that a turn did not finish well (ADR 0124, amended).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Its worker's: it failed, its turn was over, or it ran out of time.
    Worker,
    /// The platform's: HQ or the runner restarted under it, or its container vanished. The
    /// member did nothing wrong.
    Platform,
}

/// How many of a run's interruptions by the platform are tried again without counting against
/// it: the first. A deploy restarts HQ, the node and the runner in turn, and a turn caught by
/// the first of them lost nothing of its own.
pub const MOST_QUIET: u8 = 1;

/// The exit code of a container killed, as Docker reports one (SIGKILL).
const KILLED: i64 = 137;

/// The exit code of a container told to stop (SIGTERM).
const TERMINATED: i64 = 143;

/// Whose fault a turn that ended badly was, from how its worker ended and how long it had run.
/// A container that vanished, or one killed from outside (SIGKILL, 137, or SIGTERM, 143) before
/// its full time, was cut short by the platform; a kill at its full time is the turn's own
/// timeout, and every other exit is its worker's.
#[must_use]
pub fn fault_of(exit: Exit, ran: Millis) -> Fault {
    match exit {
        Exit::Vanished => Fault::Platform,
        Exit::Code(KILLED | TERMINATED) if ran < LONGEST_TURN_MS => Fault::Platform,
        Exit::Code(_) | Exit::Unsaid => Fault::Worker,
    }
}

/// How many of a run's `attempts` count against its retries, given how many times the platform
/// `interrupted` it: all but the first interruption's.
#[must_use]
pub fn own_attempts(attempts: u8, interrupted: u8) -> u8 {
    attempts.saturating_sub(interrupted.min(MOST_QUIET))
}

/// How much of a turn's input Claude read from the prompt cache, in whole percent: `read` of
/// all it sent (`fresh`, `read` and `written` to the cache together). `None` for a turn that
/// sent nothing, such as one on the house's model.
#[must_use]
pub fn cached_share(fresh: i64, read: i64, written: i64) -> Option<i64> {
    let all = fresh.max(0) + read.max(0) + written.max(0);
    (all > 0).then(|| read.max(0) * 100 / all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_share_read_from_the_cache_is_of_all_the_input() {
        assert_eq!(cached_share(200, 9_000, 9_800), Some(47));
        assert_eq!(cached_share(0, 10, 0), Some(100));
        assert_eq!(cached_share(10, 0, 0), Some(0));
        assert_eq!(cached_share(0, 0, 0), None);
    }

    #[test]
    fn a_failed_turn_is_tried_again_twice_a_minute_apart_and_then_not() {
        assert_eq!(retry_after_failure(1, 1_000), Some(61_000));
        assert_eq!(retry_after_failure(2, 1_000), Some(61_000));
        assert_eq!(retry_after_failure(3, 1_000), None);
    }

    #[test]
    fn a_container_that_vanished_or_was_killed_from_outside_was_cut_short_by_the_platform() {
        assert_eq!(fault_of(Exit::Vanished, 5_000), Fault::Platform);
        for code in [137, 143] {
            assert_eq!(fault_of(Exit::Code(code), 5_000), Fault::Platform, "{code}");
        }
        // Its worker's own: it failed (1), its turn was over (3), its container never started
        // (-1), or the runner said nothing of how it ended.
        for exit in [
            Exit::Code(1),
            Exit::Code(3),
            Exit::Code(-1),
            Exit::Code(0),
            Exit::Unsaid,
        ] {
            assert_eq!(fault_of(exit, 5_000), Fault::Worker, "{exit:?}");
        }
    }

    #[test]
    fn a_kill_at_the_turns_full_time_is_its_timeout_and_not_an_interruption() {
        assert_eq!(
            fault_of(Exit::Code(137), LONGEST_TURN_MS - 1),
            Fault::Platform,
            "a minute short of it"
        );
        assert_eq!(fault_of(Exit::Code(137), LONGEST_TURN_MS), Fault::Worker);
        assert_eq!(
            fault_of(Exit::Code(137), LONGEST_TURN_MS + 5_000),
            Fault::Worker
        );
        // A vanished container is the platform's however long the turn had run.
        assert_eq!(
            fault_of(Exit::Vanished, LONGEST_TURN_MS * 2),
            Fault::Platform
        );
    }

    #[test]
    fn the_first_interruption_does_not_count_against_a_runs_retries_and_the_rest_do() {
        // Never interrupted: every start counts, as ever.
        assert_eq!(own_attempts(1, 0), 1);
        assert_eq!(own_attempts(3, 0), 3);
        // Interrupted once: that start was not its own.
        assert_eq!(own_attempts(1, 1), 0);
        assert_eq!(own_attempts(2, 1), 1);
        // Interrupted again: only the first was free.
        assert_eq!(own_attempts(2, 2), 1);
        assert_eq!(own_attempts(4, 3), 3);
        // So a run interrupted at every start is tried four times, not three.
        let mut starts = 0;
        for attempts in 1..=10u8 {
            starts = attempts;
            if retry_after_failure(own_attempts(attempts, attempts), 0).is_none() {
                break;
            }
        }
        assert_eq!(starts, 4);
        // And one that is never interrupted three times, as ever.
        for attempts in 1..=10u8 {
            starts = attempts;
            if retry_after_failure(own_attempts(attempts, 0), 0).is_none() {
                break;
            }
        }
        assert_eq!(starts, 3);
    }

    #[test]
    fn only_queued_waiting_and_running_turns_are_live() {
        assert!(RunStatus::Queued.live());
        assert!(RunStatus::Running.live());
        assert!(RunStatus::RetryWaiting.live());
        for s in [
            RunStatus::Succeeded,
            RunStatus::Failed,
            RunStatus::Stopped,
            RunStatus::TimedOut,
        ] {
            assert!(!s.live());
            assert_eq!(RunStatus::from_name(s.name()), Some(s));
        }
    }
}
