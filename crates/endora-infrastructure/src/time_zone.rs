//! Where the person is in time: their distance from UTC at any moment, following the clock
//! changes of their time zone.

use chrono::{Offset, TimeZone as _};

/// The person's time zone: an IANA zone (`ENDORA_TIME_ZONE`, e.g. `America/New_York`), whose
/// offset follows daylight saving, or a fixed offset in minutes when no zone is named
/// (`ENDORA_UTC_OFFSET_MINUTES`, which is wrong for half the year wherever the clocks change).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeZone {
    zone: Option<chrono_tz::Tz>,
    fixed_minutes: i64,
}

impl TimeZone {
    /// A fixed distance from UTC, in minutes (negative to the west).
    #[must_use]
    pub const fn fixed(minutes: i64) -> Self {
        Self {
            zone: None,
            fixed_minutes: minutes,
        }
    }

    /// A named zone, or the fixed offset when the name is empty.
    ///
    /// # Errors
    /// A message naming the zone if it is not one the time-zone database knows: a typo must
    /// not quietly become UTC.
    pub fn named_or_fixed(name: &str, fixed_minutes: i64) -> Result<Self, String> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(Self::fixed(fixed_minutes));
        }
        let zone = name.parse::<chrono_tz::Tz>().map_err(|_| {
            format!("unknown time zone \"{name}\" (expected a name like America/New_York)")
        })?;
        Ok(Self {
            zone: Some(zone),
            fixed_minutes,
        })
    }

    /// The distance from UTC in minutes at `at_ms` (negative to the west).
    #[must_use]
    pub fn offset_minutes_at(&self, at_ms: i64) -> i64 {
        let Some(zone) = self.zone else {
            return self.fixed_minutes;
        };
        let Some(utc) = chrono::DateTime::from_timestamp_millis(at_ms) else {
            return self.fixed_minutes;
        };
        let seconds = zone
            .offset_from_utc_datetime(&utc.naive_utc())
            .fix()
            .local_minus_utc();
        i64::from(seconds) / 60
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-10-15 12:00 UTC, and a month later, after the clocks went back on 2026-11-01.
    const OCTOBER: i64 = 1_792_065_600_000;
    const NOVEMBER: i64 = OCTOBER + 31 * 24 * 60 * 60 * 1000;

    #[test]
    fn a_named_zone_follows_its_clock_change() {
        let new_york = TimeZone::named_or_fixed("America/New_York", 0).unwrap();
        assert_eq!(new_york.offset_minutes_at(OCTOBER), -240);
        assert_eq!(new_york.offset_minutes_at(NOVEMBER), -300);
        // On the night itself: 05:59 UTC is 01:59 EDT, 06:00 UTC is 01:00 EST.
        let change = 1_793_512_800_000; // 2026-11-01 06:00 UTC
        assert_eq!(new_york.offset_minutes_at(change - 60_000), -240);
        assert_eq!(new_york.offset_minutes_at(change), -300);
        let kolkata = TimeZone::named_or_fixed(" Asia/Kolkata ", 0).unwrap();
        assert_eq!(kolkata.offset_minutes_at(NOVEMBER), 330);
    }

    #[test]
    fn without_a_name_the_fixed_offset_is_used_and_a_wrong_name_is_refused() {
        let fixed = TimeZone::named_or_fixed("", -240).unwrap();
        assert_eq!(fixed.offset_minutes_at(NOVEMBER), -240);
        assert_eq!(fixed, TimeZone::fixed(-240));
        let wrong = TimeZone::named_or_fixed("America/Nowhere", 0).unwrap_err();
        assert!(wrong.contains("America/Nowhere"), "{wrong}");
    }
}
