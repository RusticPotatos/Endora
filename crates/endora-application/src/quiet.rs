//! When background work may use the model: only while the person is quiet, so one GPU is
//! never taken from someone mid-conversation. Decided here, by code, so it does not depend
//! on a caller remembering.
use crate::Timestamp;

/// How long the person must have been silent before background work (reading mail) takes the model.
///
/// Ten minutes to start with (ADR 0094). One GPU serves chat and background work, so the
/// worker steps aside for anyone who has just spoken; a number to argue from a week of the
/// activity trail, not a constant to defend.
pub const QUIET_WINDOW_MS: i64 = 10 * 60 * 1000;

/// Whether the person has been quiet long enough for background work to run.
///
/// `last_spoke` is when the person last wrote in chat, `None` if they never have. A message
/// dated *after* `now` (the clock moved, or the record is wrong) is not quiet: when the
/// answer cannot be trusted the work waits, because waiting costs nothing and a pass over
/// someone mid-conversation does.
#[must_use]
pub fn quiet_enough(now: Timestamp, last_spoke: Option<Timestamp>) -> bool {
    let Some(last_spoke) = last_spoke else {
        return true;
    };
    let silent_for = now.unix_millis().saturating_sub(last_spoke.unix_millis());
    silent_for >= QUIET_WINDOW_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_unix_millis(ms)
    }

    const MINUTE: i64 = 60 * 1000;

    #[test]
    fn nobody_having_spoken_is_quiet() {
        assert!(quiet_enough(at(1_000_000), None));
    }

    #[test]
    fn someone_who_has_just_spoken_is_not() {
        let now = 100 * MINUTE;
        assert!(!quiet_enough(at(now), Some(at(now))));
        assert!(!quiet_enough(at(now), Some(at(now - MINUTE))));
        assert!(
            !quiet_enough(at(now), Some(at(now - QUIET_WINDOW_MS + 1))),
            "one millisecond short of the window is still inside it"
        );
    }

    #[test]
    fn the_window_ends_exactly_at_its_length() {
        let now = 100 * MINUTE;
        assert!(quiet_enough(at(now), Some(at(now - QUIET_WINDOW_MS))));
        assert!(quiet_enough(at(now), Some(at(now - 3 * QUIET_WINDOW_MS))));
    }

    #[test]
    fn a_message_from_the_future_is_not_quiet() {
        // Fail closed: a clock that moved, or a wrong record, must not read as silence.
        let now = 100 * MINUTE;
        assert!(!quiet_enough(at(now), Some(at(now + MINUTE))));
        assert!(!quiet_enough(at(0), Some(at(i64::MAX))));
    }
}
