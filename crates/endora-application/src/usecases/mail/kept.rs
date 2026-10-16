//! The calendar, kept for a while (ADR 0097). Reading every calendar the house has can take
//! seconds (live: one took ten for fifteen months), and the mail screen, the brief and every
//! bill ask for it, some of them while holding the turn.

use super::{DAY_MS, READ_BACK_MS};
use crate::{Calendar, Clock};
use std::sync::Mutex;

/// How long what was read is used before it is read again.
pub const CALENDAR_KEPT_FOR_MS: i64 = 60 * 60 * 1000;

/// After the calendar could not be read, how long before it is asked again. Each failed read
/// can take the whole HTTP timeout, and the reader asks once per bill.
const UNREADABLE_FOR_MS: i64 = 10 * 60 * 1000;

/// What is read at once: back to the appointment behind the oldest bill that is read, and a
/// year ahead, the furthest a date in mail is looked for.
const KEEP_BACK_MS: i64 = READ_BACK_MS + (endora_mail::domain::BILL_LOOKBACK_DAYS + 1) * DAY_MS;
const KEEP_AHEAD_MS: i64 = 370 * DAY_MS;

struct Held {
    read_at_ms: i64,
    from_ms: i64,
    to_ms: i64,
    entries: Vec<(String, String)>,
}

enum Kept {
    Nothing,
    Read(Held),
    Unreadable { at_ms: i64, error: String },
}

/// A [`Calendar`] that reads the one it wraps at most once an hour, over one wide span, and
/// answers each narrower question from that.
pub struct KeptCalendar<C, K> {
    calendar: C,
    clock: K,
    kept: Mutex<Kept>,
}

impl<C: Calendar, K: Clock> KeptCalendar<C, K> {
    /// Keeps nothing yet: the first question reads the calendar.
    pub fn new(calendar: C, clock: K) -> Self {
        Self {
            calendar,
            clock,
            kept: Mutex::new(Kept::Nothing),
        }
    }

    /// The calendar it wraps.
    pub fn calendar(&self) -> &C {
        &self.calendar
    }

    /// The clock it reads the time from.
    pub fn clock(&self) -> &K {
        &self.clock
    }
}

/// The entries whose day starts within the span.
fn within(entries: &[(String, String)], from_ms: i64, to_ms: i64) -> Vec<(String, String)> {
    let first_day = from_ms.div_euclid(DAY_MS);
    entries
        .iter()
        .filter(|(day, _)| {
            endora_mail::days_of(day).is_some_and(|d| d >= first_day && d * DAY_MS < to_ms)
        })
        .cloned()
        .collect()
}

impl<C: Calendar, K: Clock> Calendar for KeptCalendar<C, K> {
    fn entries(&self, from_ms: i64, to_ms: i64) -> Result<Vec<(String, String)>, String> {
        let now = self.clock.now().unix_millis();
        // Held while reading, so a second question waits for the first read instead of
        // starting its own.
        let mut kept = self
            .kept
            .lock()
            .map_err(|_| "the calendar is unavailable".to_owned())?;
        match &*kept {
            Kept::Unreadable { at_ms, error } if now - at_ms < UNREADABLE_FOR_MS => {
                return Err(error.clone());
            }
            Kept::Read(held) if now - held.read_at_ms < CALENDAR_KEPT_FOR_MS => {
                if from_ms >= held.from_ms && to_ms <= held.to_ms {
                    return Ok(within(&held.entries, from_ms, to_ms));
                }
                // Outside what is held: read as asked, and keep what is held.
                return self
                    .calendar
                    .entries(from_ms, to_ms)
                    .map(|entries| within(&entries, from_ms, to_ms));
            }
            _ => {}
        }
        let day_start = now - now.rem_euclid(DAY_MS);
        let (wide_from, wide_to) = (day_start - KEEP_BACK_MS, day_start + KEEP_AHEAD_MS);
        match self.calendar.entries(wide_from, wide_to) {
            Ok(entries) => {
                let answer = if from_ms >= wide_from && to_ms <= wide_to {
                    within(&entries, from_ms, to_ms)
                } else {
                    drop(kept);
                    return self
                        .calendar
                        .entries(from_ms, to_ms)
                        .map(|entries| within(&entries, from_ms, to_ms));
                };
                *kept = Kept::Read(Held {
                    read_at_ms: now,
                    from_ms: wide_from,
                    to_ms: wide_to,
                    entries,
                });
                Ok(answer)
            }
            Err(error) => {
                *kept = Kept::Unreadable {
                    at_ms: now,
                    error: error.clone(),
                };
                Err(error)
            }
        }
    }
}
