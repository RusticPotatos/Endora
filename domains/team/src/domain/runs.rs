//! Runs: one turn of a member, from queued to how it ended.

use super::work::{MemberId, Millis, TaskStatus};

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

pub use endora_kernel::exit_code::NEVER_STARTED_EXIT;

/// How a turn that never started begins its end reason, so the record knows it again.
pub const NEVER_STARTED: &str = "never started";

/// The first wait before an item whose turn never started is woken again, and the longest.
const AGAIN_FIRST_MS: Millis = 5 * 60_000;
const AGAIN_LONGEST_MS: Millis = 60 * 60_000;

/// Whether a turn's worker never ran: its container never started (ADR 0142, amended
/// 2026-10-31). Such a turn did nothing, well or badly: it is nobody's failure.
#[must_use]
pub const fn never_started(exit: Exit) -> bool {
    matches!(exit, Exit::Code(NEVER_STARTED_EXIT))
}

/// The end reason of a turn that never started: [`NEVER_STARTED`], then what the runner said.
#[must_use]
pub fn never_started_said(why: &str) -> String {
    if is_never_started(why) {
        return why.to_owned();
    }
    format!("{NEVER_STARTED}: {why}")
}

/// Whether an end reason is a turn's that never started.
#[must_use]
pub fn is_never_started(said: &str) -> bool {
    said.starts_with(NEVER_STARTED)
}

/// How long after the last of `in_a_row` turns in a row that never started its item is woken
/// again for the same member: five minutes, doubling, at most an hour. Each such turn was
/// already tried three times a minute apart, and none of it is paid for; an hour bounds how
/// long an item waits once the runner is mended.
#[must_use]
pub fn wake_again_after(in_a_row: usize) -> Millis {
    if in_a_row == 0 {
        return 0;
    }
    let doublings = u32::try_from(in_a_row - 1).unwrap_or(u32::MAX).min(8);
    (AGAIN_FIRST_MS << doublings).min(AGAIN_LONGEST_MS)
}

/// One ended turn on an item, as [`woken_again_after_failed_starts`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndedTurn {
    /// Whose turn it was.
    pub member: MemberId,
    /// Whether it failed without ever starting ([`is_never_started`]).
    pub never_started: bool,
    /// When it ended.
    pub ended_at: Option<Millis>,
}

/// Whether an item is woken again for the member of its newest ended turn (`ended`, newest
/// first) after that member's turns on it never started (the runner could not start the
/// member's runtime), at `now`: once [`wake_again_after`] has passed since the newest ended,
/// for an item that is to do, being done or in review, while the member is at work and the
/// item still waits on them: in review, for its reviewer (anyone but its holder); otherwise,
/// for its holder. A turn that never started is no failure of the work: the item neither counts
/// one nor climbs.
#[must_use]
pub fn woken_again_after_failed_starts(
    ended: &[EndedTurn],
    status: TaskStatus,
    holder: Option<MemberId>,
    at_work: bool,
    now: Millis,
) -> bool {
    if !matches!(
        status,
        TaskStatus::Todo | TaskStatus::Doing | TaskStatus::InReview
    ) {
        return false;
    }
    let Some(last) = ended.first() else {
        return false;
    };
    let in_a_row = ended
        .iter()
        .take_while(|r| r.member == last.member && r.never_started)
        .count();
    let Some(ended_at) = last.ended_at.filter(|_| in_a_row > 0) else {
        return false;
    };
    if now < ended_at + wake_again_after(in_a_row) {
        return false;
    }
    let still_its = match status {
        TaskStatus::InReview => holder != Some(last.member),
        _ => holder == Some(last.member),
    };
    still_its && at_work
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
    fn an_item_whose_turns_never_started_is_woken_again_for_its_member_after_a_wait() {
        let bo = MemberId(1);
        let never = |at| EndedTurn {
            member: bo,
            never_started: true,
            ended_at: Some(at),
        };
        let ended = [never(1_000), never(500)];
        let wait = wake_again_after(2);
        let woken = |status, holder, at_work, now| {
            woken_again_after_failed_starts(&ended, status, holder, at_work, now)
        };
        assert!(!woken(TaskStatus::Doing, Some(bo), true, 1_000 + wait - 1));
        assert!(woken(TaskStatus::Doing, Some(bo), true, 1_000 + wait));
        assert!(
            !woken(TaskStatus::Doing, Some(MemberId(2)), true, 1_000 + wait),
            "moved on"
        );
        assert!(
            !woken(TaskStatus::Doing, Some(bo), false, 1_000 + wait),
            "not at work"
        );
        assert!(
            woken(TaskStatus::InReview, Some(MemberId(2)), true, 1_000 + wait),
            "its reviewer"
        );
        assert!(!woken(TaskStatus::Done, Some(bo), true, 1_000 + wait));
        let started = [EndedTurn {
            never_started: false,
            ..never(1_000)
        }];
        assert!(!woken_again_after_failed_starts(
            &started,
            TaskStatus::Doing,
            Some(bo),
            true,
            Millis::MAX / 2
        ));
    }

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
    fn only_a_container_that_never_started_is_a_turn_that_never_started() {
        assert!(never_started(Exit::Code(NEVER_STARTED_EXIT)));
        for exit in [
            Exit::Code(0),
            Exit::Code(1),
            Exit::Code(137),
            Exit::Vanished,
            Exit::Unsaid,
        ] {
            assert!(!never_started(exit), "{exit:?}");
        }
    }

    #[test]
    fn a_turn_that_never_started_says_so_and_is_known_by_it() {
        let said = never_started_said("its container never started: no runtime");
        assert!(said.starts_with(NEVER_STARTED));
        assert!(is_never_started(&said));
        assert!(!is_never_started("the turn failed: exit 1"));
        assert!(!is_never_started(""));
        // Said once, however often it is tried again.
        assert_eq!(never_started_said(&said), said);
    }

    #[test]
    fn an_item_whose_turn_never_started_is_woken_again_after_a_growing_wait() {
        assert_eq!(wake_again_after(0), 0);
        assert_eq!(wake_again_after(1), 5 * 60_000);
        assert_eq!(wake_again_after(2), 10 * 60_000);
        assert_eq!(wake_again_after(3), 20 * 60_000);
        assert_eq!(wake_again_after(4), 40 * 60_000);
        assert_eq!(wake_again_after(5), 60 * 60_000, "at most an hour");
        assert_eq!(wake_again_after(50), 60 * 60_000);
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
