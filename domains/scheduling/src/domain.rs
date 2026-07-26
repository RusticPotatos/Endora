//! Scheduling domain model — the cadences for the butler's proactive moments.

use endora_kernel::ids::Timestamp;

/// How long the butler stays quiet after the person's own last message. If they
/// just spoke they are present and can simply ask for something; reaching out on
/// top of that is noise, not service.
const QUIET_AFTER_ACTIVITY_MS: i64 = 60 * 60 * 1_000;

/// The person's bounds on proactive **check-ins** — the butler reaching out on its
/// own (ADR 0056 §heartbeat/check-ins, ADR 0056).
///
/// This is a **budget, not a trigger**. It says how *often* the butler may speak
/// uninvited; whether it has anything worth saying is the butler's judgement, made
/// against what it understands (ADR 0056). The person owns the budget: whether it
/// is on at all, and the minimum gap between outreaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckinSchedule {
    /// Whether proactive check-ins are on. Off by default — the butler never
    /// reaches out uninvited until the person turns it on.
    pub enabled: bool,
    /// The **minimum** gap between outreaches, in milliseconds.
    pub interval_ms: i64,
    /// The earliest the butler may next consider reaching out.
    pub next_at: Timestamp,
}

impl CheckinSchedule {
    /// The default: **off**, with a daily budget ready if the person enables it.
    #[must_use]
    pub fn disabled_default(now: Timestamp) -> Self {
        let day_ms = 24 * 60 * 60 * 1_000;
        Self {
            enabled: false,
            interval_ms: day_ms,
            next_at: Timestamp::from_unix_millis(now.unix_millis() + day_ms),
        }
    }

    /// Whether the butler is **allowed to consider** reaching out now.
    ///
    /// Deliberately the *gate*, not the decision. Keeping the rate limit here — in
    /// deterministic code — rather than asking the model to be restrained is the
    /// same principle the honesty guarantees follow: a model talked into speaking
    /// too often is a prompt away from a pest, whereas a budget it cannot see or
    /// argue with simply holds.
    ///
    /// `last_person_activity` is when the person themselves last said something.
    #[must_use]
    pub fn may_reach_out(&self, now: Timestamp, last_person_activity: Option<Timestamp>) -> bool {
        if !self.enabled || now.unix_millis() < self.next_at.unix_millis() {
            return false;
        }
        // Don't talk over someone who is already here.
        last_person_activity
            .is_none_or(|last| now.unix_millis() - last.unix_millis() >= QUIET_AFTER_ACTIVITY_MS)
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

/// The person's **nightly self-improvement loop** schedule (ADR 0051): while they
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

#[cfg(test)]
mod tests {
    use super::{CheckinSchedule, Timestamp};

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_unix_millis(ms)
    }

    const HOUR: i64 = 60 * 60 * 1_000;

    fn enabled_from(next_at_ms: i64) -> CheckinSchedule {
        CheckinSchedule {
            enabled: true,
            interval_ms: 24 * HOUR,
            next_at: at(next_at_ms),
        }
    }

    #[test]
    fn disabled_never_permits_reaching_out() {
        let off = CheckinSchedule {
            enabled: false,
            ..enabled_from(0)
        };
        assert!(!off.may_reach_out(at(10 * HOUR), None));
    }

    #[test]
    fn the_budget_holds_until_the_interval_has_elapsed() {
        let s = enabled_from(10 * HOUR);
        assert!(!s.may_reach_out(at(9 * HOUR), None));
        assert!(s.may_reach_out(at(10 * HOUR), None));
    }

    #[test]
    fn it_stays_quiet_while_the_person_is_around() {
        let s = enabled_from(10 * HOUR);
        // They spoke a moment ago — they are here and can just ask.
        assert!(!s.may_reach_out(at(10 * HOUR), Some(at(10 * HOUR - 60_000))));
        // Long enough after they last spoke, it may.
        assert!(s.may_reach_out(at(10 * HOUR), Some(at(9 * HOUR - 1))));
    }

    #[test]
    fn a_person_who_has_never_spoken_does_not_block_outreach() {
        let s = enabled_from(10 * HOUR);
        assert!(s.may_reach_out(at(10 * HOUR), None));
    }
}
