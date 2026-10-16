//! A system clock adapter.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use endora_application::Clock;
use endora_application::Timestamp;

/// A [`Clock`] backed by the operating-system wall clock.
///
/// This is the one place the system clock is read; the domain and application
/// receive time only through the [`Clock`] port, which keeps them deterministic.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

/// The person's time zone, set once at start (`ENDORA_TIME_ZONE`). A fact about the whole
/// process, like the operating system's own zone, so it is kept here rather than threaded
/// through every place that holds a clock.
static LOCAL_ZONE: OnceLock<crate::TimeZone> = OnceLock::new();

impl SystemClock {
    /// Makes [`Clock::utc_offset_minutes`] answer for `zone`. The first call wins.
    pub fn use_time_zone(zone: crate::TimeZone) {
        let _ = LOCAL_ZONE.set(zone);
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(0);
        Timestamp::from_unix_millis(millis)
    }

    fn utc_offset_minutes(&self) -> i64 {
        LOCAL_ZONE
            .get()
            .map_or(0, |zone| zone.offset_minutes_at(self.now().unix_millis()))
    }
}

#[cfg(test)]
mod tests {
    use super::SystemClock;
    use endora_application::Clock;

    #[test]
    fn the_system_clock_answers_for_the_zone_it_was_given() {
        SystemClock::use_time_zone(crate::TimeZone::fixed(-240));
        assert_eq!(SystemClock.utc_offset_minutes(), -240);
    }

    #[test]
    fn system_clock_is_after_2020() {
        // 2020-01-01T00:00:00Z in ms — a sanity floor, not a precise assertion.
        assert!(SystemClock.now().unix_millis() > 1_577_836_800_000);
    }
}
