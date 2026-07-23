//! Scheduling domain model — the cadences for the butler's proactive moments.

use endora_kernel::ids::Timestamp;

/// The person's cadence for proactive **check-ins** — the butler reaching out on
/// its own (ADR 0019 §heartbeat/check-ins). The person owns it: whether it is on,
/// how often, and when the next one is due. Interval-based for now; time-of-day
/// windows ("mornings") are a later refinement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckinSchedule {
    /// Whether proactive check-ins are on. Off by default — the butler never
    /// reaches out uninvited until the person turns it on.
    pub enabled: bool,
    /// How long between check-ins, in milliseconds.
    pub interval_ms: i64,
    /// When the next check-in is due.
    pub next_at: Timestamp,
}

impl CheckinSchedule {
    /// The default: **off**, with a daily cadence ready if the person enables it.
    #[must_use]
    pub fn disabled_default(now: Timestamp) -> Self {
        let day_ms = 24 * 60 * 60 * 1_000;
        Self {
            enabled: false,
            interval_ms: day_ms,
            next_at: Timestamp::from_unix_millis(now.unix_millis() + day_ms),
        }
    }

    /// Whether a check-in is due now (enabled and past its next time).
    #[must_use]
    pub fn is_due(&self, now: Timestamp) -> bool {
        self.enabled && now.unix_millis() >= self.next_at.unix_millis()
    }
}

/// The person's **daily brief** schedule — the butler preparing a weather/safety/news
/// brief on its own each day at a chosen hour (ADRs 0024/0025). Off by default. The
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

/// The person's **nightly self-improvement loop** schedule (ADR 0024): while they
/// sleep, the butler reviews the day and its understanding of them, reflects (forms
/// and refines beliefs), and leaves a short overnight note — all within the
/// *reversible band*, so it can research, draft, and learn but never send, spend, or
/// change anything. Off by default; the person owns whether it runs and at what
/// (off-)hour. The hour is **UTC** (the console converts local), keeping the
/// server-side scheduler timezone-free — same convention as [`BriefSchedule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NightlyLoopSchedule {
    /// Whether the nightly loop is on.
    pub enabled: bool,
    /// The UTC hour (0–23) to run it — pick a quiet one (default 03:00 UTC).
    pub hour_utc: u8,
    /// When it last ran (so it fires once per night, not every tick).
    pub last_at: Timestamp,
}

impl NightlyLoopSchedule {
    /// The default: **off**, at a quiet 03:00 UTC if the person enables it.
    #[must_use]
    pub const fn disabled_default() -> Self {
        Self {
            enabled: false,
            hour_utc: 3,
            last_at: Timestamp::from_unix_millis(0),
        }
    }

    /// Whether the loop is due: enabled, the current UTC hour matches, and it hasn't
    /// run in the last ~20 hours (so it fires once per night, not every tick).
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
