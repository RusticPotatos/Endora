//! The owner's day as briefings say it.

use super::Engine;
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::{Millis, lessons, money};

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Today in the owner's time with its weekday and year, as `Sat 18 Oct 2026`.
    pub(super) fn said_today(&self) -> String {
        let day = money::day_of(self.clock.now(), self.settings.offset_minutes);
        let (year, _, _) = lessons::civil(day);
        let (d, month) = lessons::day_and_month(day);
        // The epoch day 0 was a Thursday.
        let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][day.rem_euclid(7) as usize];
        format!("{weekday} {d} {month} {year}")
    }

    /// The day and month of `at` in the owner's time, as `16 Oct`.
    pub(super) fn said_day(&self, at: Millis) -> String {
        let (day, month) = lessons::day_and_month(money::day_of(at, self.settings.offset_minutes));
        format!("{day} {month}")
    }
}
