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
