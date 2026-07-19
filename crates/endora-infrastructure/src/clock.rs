//! A system clock adapter.

use std::time::{SystemTime, UNIX_EPOCH};

use endora_application::Clock;
use endora_domain::Timestamp;

/// A [`Clock`] backed by the operating-system wall clock.
///
/// This is the one place the system clock is read; the domain and application
/// receive time only through the [`Clock`] port, which keeps them deterministic.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(0);
        Timestamp::from_unix_millis(millis)
    }
}

#[cfg(test)]
mod tests {
    use super::SystemClock;
    use endora_application::Clock;

    #[test]
    fn system_clock_is_after_2020() {
        // 2020-01-01T00:00:00Z in ms — a sanity floor, not a precise assertion.
        assert!(SystemClock.now().unix_millis() > 1_577_836_800_000);
    }
}
