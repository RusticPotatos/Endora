//! Where the owner is in time: their distance from UTC at a moment, following the clock changes
//! of their time zone.
//!
//! The node has the same in `endora-infrastructure`'s `time_zone`; HQ keeps its own copy so it
//! builds from the team alone (ADR 0136). A fix to one is a fix to both.

use chrono::{Offset, TimeZone as _};

/// The owner's time zone: an IANA zone (`ENDORA_TIME_ZONE`, e.g. `America/New_York`), whose
/// offset follows daylight saving, or a fixed offset in minutes when no zone is named
/// (`ENDORA_UTC_OFFSET_MINUTES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeZone {
    zone: Option<chrono_tz::Tz>,
    fixed_minutes: i64,
}

impl TimeZone {
    /// A named zone, or the fixed offset when the name is empty.
    ///
    /// # Errors
    /// A message naming the zone if it is not one the time-zone database knows: a typo must
    /// not quietly become UTC.
    pub fn named_or_fixed(name: &str, fixed_minutes: i64) -> Result<Self, String> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(Self {
                zone: None,
                fixed_minutes,
            });
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
    use super::TimeZone;

    #[test]
    fn on_the_night_the_clocks_go_back_the_offset_changes_at_the_minute() {
        let new_york = TimeZone::named_or_fixed("America/New_York", 0).unwrap();
        let change = 1_793_512_800_000; // 2026-11-01 06:00 UTC
        assert_eq!(new_york.offset_minutes_at(change - 60_000), -240);
        assert_eq!(new_york.offset_minutes_at(change), -300);
    }
}
