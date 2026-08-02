//! How hard it is to guess your way in.
//!
//! A password you can remember and a six-digit code are both **guessable** in a way the
//! 256-bit token they replace was not — a million combinations is nothing to a script on a
//! LAN. Rate limiting is therefore not a refinement here; it is the thing that makes the
//! whole trade sound.
//!
//! The escalation is deliberately boring: a handful of free attempts for fat fingers, then
//! locks that grow. Five tries per half hour against a million possibilities is not a contest.
//!
//! **The token printed to the node's log is never subject to any of this.** It is
//! high-entropy, so throttling buys nothing against it — and it is the recovery path. Locking
//! it too would mean a stranger with a script could shut the owner out of their own house,
//! which turns a security feature into a denial of service.

/// Attempts allowed before anything slows down. Enough for a mistyped password and a code
/// that expired while it was being typed; not enough to be a search.
const FREE_TRIES: u32 = 5;

/// With nothing tried for this long, the count starts again.
///
/// Without it, a failure a week — a phone with a drifting clock, a stale browser tab retrying —
/// would accumulate until the owner was permanently locked out by nothing at all.
const FORGIVEN_AFTER_MS: i64 = 30 * 60 * 1_000;

/// How long each failure past the free ones costs, in milliseconds. The last value repeats.
const LOCKS_FOR_MS: &[i64] = &[
    60 * 1_000,      // a minute
    5 * 60 * 1_000,  // then five
    15 * 60 * 1_000, // then a quarter of an hour
    60 * 60 * 1_000, // and an hour from then on
];

/// What the node remembers about failed sign-ins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Attempts {
    /// Consecutive failures that have not yet been forgiven.
    pub failures: u32,
    /// When the last one happened.
    pub last_failure_ms: i64,
}

/// Whether a sign-in may be tried, and how long until it may be if not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Go ahead.
    Allow,
    /// Refused for this many milliseconds more.
    WaitFor(i64),
}

/// How long a given number of consecutive failures locks for.
fn lock_after(failures: u32) -> i64 {
    if failures <= FREE_TRIES {
        return 0;
    }
    let step = (failures - FREE_TRIES - 1) as usize;
    LOCKS_FOR_MS[step.min(LOCKS_FOR_MS.len() - 1)]
}

/// Whether somebody may try to sign in right now.
#[must_use]
pub fn may_try(attempts: Attempts, now_ms: i64) -> Gate {
    let quiet_for = now_ms - attempts.last_failure_ms;
    if quiet_for >= FORGIVEN_AFTER_MS {
        return Gate::Allow;
    }
    let locked_for = lock_after(attempts.failures);
    if quiet_for >= locked_for {
        return Gate::Allow;
    }
    Gate::WaitFor(locked_for - quiet_for)
}

/// The record after a failed attempt.
#[must_use]
pub fn after_failure(attempts: Attempts, now_ms: i64) -> Attempts {
    // A failure long after the last one starts a fresh streak rather than extending an old.
    let carried = if now_ms - attempts.last_failure_ms >= FORGIVEN_AFTER_MS {
        0
    } else {
        attempts.failures
    };
    Attempts {
        failures: carried.saturating_add(1),
        last_failure_ms: now_ms,
    }
}

/// The record after getting in: a clean slate.
#[must_use]
pub fn after_success() -> Attempts {
    Attempts::default()
}

#[cfg(test)]
mod tests {
    use super::{
        Attempts, FORGIVEN_AFTER_MS, FREE_TRIES, Gate, after_failure, after_success, may_try,
    };

    const MINUTE: i64 = 60 * 1_000;

    /// Walks `n` consecutive failures, one second apart, starting at `from`.
    fn after_failures(n: u32, from: i64) -> Attempts {
        let mut a = Attempts::default();
        for i in 0..n {
            a = after_failure(a, from + i64::from(i) * 1_000);
        }
        a
    }

    #[test]
    fn a_fresh_node_lets_you_try() {
        assert_eq!(may_try(Attempts::default(), 0), Gate::Allow);
    }

    #[test]
    fn a_few_mistakes_cost_nothing() {
        // A mistyped password and a code that expired mid-typing are both ordinary, and
        // punishing them teaches people the thing is broken.
        let now = 10 * MINUTE;
        for tries in 1..=FREE_TRIES {
            let attempts = after_failures(tries, now);
            assert_eq!(
                may_try(attempts, now + i64::from(tries) * 1_000),
                Gate::Allow,
                "{tries} failures should still be free"
            );
        }
    }

    #[test]
    fn past_that_it_starts_costing() {
        let now = 10 * MINUTE;
        let attempts = after_failures(FREE_TRIES + 1, now);
        let at = now + i64::from(FREE_TRIES) * 1_000;
        assert!(
            matches!(may_try(attempts, at), Gate::WaitFor(ms) if ms > 0),
            "the sixth failure should lock"
        );
    }

    #[test]
    fn the_locks_grow() {
        // The point of escalation: a script that waits out one lock finds a longer one.
        let now = 10 * MINUTE;
        let mut seen = Vec::new();
        for extra in 1..=4 {
            let attempts = after_failures(FREE_TRIES + extra, now);
            if let Gate::WaitFor(ms) = may_try(attempts, now) {
                seen.push(ms);
            }
        }
        assert_eq!(seen.len(), 4, "every one of those should have locked");
        assert!(
            seen.windows(2).all(|w| w[1] > w[0]),
            "locks should lengthen: {seen:?}"
        );
    }

    #[test]
    fn the_longest_lock_stops_growing() {
        // An attacker who keeps going should not be able to push the owner's own lock-out to
        // a week. The cap is what keeps this throttling rather than a permanent door.
        let now = 10 * MINUTE;
        // Measured from each one's own last failure, so this compares the *lock* rather than
        // how long each walk of failures happened to take.
        let lock_after = |extra| {
            let a = after_failures(FREE_TRIES + extra, now);
            may_try(a, a.last_failure_ms)
        };
        assert_eq!(lock_after(8), lock_after(98));
    }

    #[test]
    fn waiting_out_a_lock_lets_you_in_again() {
        let now = 10 * MINUTE;
        let attempts = after_failures(FREE_TRIES + 1, now);
        let Gate::WaitFor(ms) = may_try(attempts, now) else {
            panic!("expected a lock");
        };
        assert_eq!(
            may_try(attempts, attempts.last_failure_ms + ms),
            Gate::Allow
        );
    }

    #[test]
    fn a_quiet_spell_forgives_everything() {
        // Without this, one failure a week — a phone with a drifting clock, a stale tab
        // retrying — would accumulate until the owner was locked out by nothing at all.
        let now = 10 * MINUTE;
        let attempts = after_failures(20, now);
        assert_eq!(
            may_try(attempts, attempts.last_failure_ms + FORGIVEN_AFTER_MS),
            Gate::Allow
        );
    }

    #[test]
    fn a_failure_after_a_quiet_spell_starts_a_fresh_streak() {
        let attempts = after_failures(20, 10 * MINUTE);
        let much_later = attempts.last_failure_ms + FORGIVEN_AFTER_MS;
        assert_eq!(after_failure(attempts, much_later).failures, 1);
    }

    #[test]
    fn getting_in_clears_the_slate() {
        assert_eq!(after_success(), Attempts::default());
        assert_eq!(may_try(after_success(), 0), Gate::Allow);
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_unlock_anything() {
        // Time moving the wrong way must never be a way in.
        let now = 10 * MINUTE;
        let attempts = after_failures(FREE_TRIES + 3, now);
        assert!(matches!(
            may_try(attempts, now - 999 * MINUTE),
            Gate::WaitFor(_)
        ));
    }
}
