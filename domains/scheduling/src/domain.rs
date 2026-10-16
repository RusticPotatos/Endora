//! Scheduling domain model — the cadences for the butler's proactive moments.

use endora_kernel::ids::Timestamp;

/// The person's **daily brief** schedule — the butler assembling their standing-order
/// brief on its own each day at a chosen hour (ADRs 0024/0025, 0074). Off by default. The
/// hour is stored in **UTC** (the console converts the person's local hour when they
/// set it), which keeps the server-side scheduler timezone-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BriefSchedule {
    /// Whether the daily brief is on.
    pub enabled: bool,
    /// The UTC hour (0–23) to prepare the brief.
    pub hour_utc: u8,
    /// When a brief was last prepared (so it fires once per day).
    pub last_at: Timestamp,
}

impl BriefSchedule {
    /// The default: **off**, at 07:00 local-ish (12:00 UTC) if the person enables it.
    #[must_use]
    pub const fn disabled_default() -> Self {
        Self {
            enabled: false,
            hour_utc: 12,
            last_at: Timestamp::from_unix_millis(0),
        }
    }

    /// Whether a brief is due: enabled, the current UTC hour matches, and one hasn't
    /// been prepared in the last ~20 hours (so it fires once per day, not every tick).
    #[must_use]
    pub fn is_due(&self, now: Timestamp) -> bool {
        if !self.enabled {
            return false;
        }
        let hour = (now.unix_millis().div_euclid(3_600_000) % 24) as u8;
        let since = now.unix_millis() - self.last_at.unix_millis();
        hour == self.hour_utc && since >= 20 * 60 * 60 * 1_000
    }
}

/// The UTC hour that keeps a schedule at the same local hour after the person's distance
/// from UTC changes from `set_under_minutes` to `now_minutes` (a clock change). Hours are
/// stored in UTC; without this a 07:00 brief set in summer arrives at 06:00 in winter.
#[must_use]
pub fn same_local_hour(hour_utc: u8, set_under_minutes: i64, now_minutes: i64) -> u8 {
    // To the nearest hour: a half-hour change moves nothing rather than guessing.
    let moved_by = (now_minutes - set_under_minutes + 30).div_euclid(60);
    let hour = (i64::from(hour_utc) - moved_by).rem_euclid(24);
    u8::try_from(hour).unwrap_or(hour_utc)
}

#[cfg(test)]
mod tests {
    use super::same_local_hour;

    #[test]
    fn an_hour_stays_on_the_clock_when_the_clock_changes() {
        // 07:00 four hours west of UTC is 11:00 UTC; five hours west, 12:00 UTC.
        assert_eq!(same_local_hour(11, -240, -300), 12);
        assert_eq!(same_local_hour(12, -300, -240), 11);
        assert_eq!(same_local_hour(11, -240, -240), 11);
        // Across midnight UTC either way.
        assert_eq!(same_local_hour(23, -240, -300), 0);
        assert_eq!(same_local_hour(0, -300, -240), 23);
        // A zone east of UTC: 07:00 is 06:00 UTC in winter and 05:00 UTC in summer.
        assert_eq!(same_local_hour(6, 60, 120), 5);
    }
}
