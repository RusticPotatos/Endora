//! What the house does at the same time of day, found by counting (ADR 0096).
//!
//! The transition log already holds a fortnight of every change the house settled on. This
//! reads it for the thing a person means by "my routine": a door that opens, a light that
//! goes on, a phone that arrives, at about the same time on most days. It is arithmetic over
//! recorded changes with no model anywhere in it, so every rule below is a unit test, and
//! what it says is a count the person can check: *on 11 of the last 14 days*.
//!
//! It finds only regularity in **when**. It does not say why, what it means, or what to do
//! about it; that is for the person, or later for a model that is handed these lines as
//! facts and not asked to find them.
use crate::domain::Transition;
use std::collections::{BTreeMap, BTreeSet};

const MINUTE_MS: i64 = 60 * 1000;
const DAY_MINUTES: i64 = 24 * 60;
const DAY_MS: i64 = DAY_MINUTES * MINUTE_MS;

/// The fewest days a pattern must show on, and the fewest days of log worth reading at all.
/// Below this, "usually" is a coincidence being flattered.
pub const MIN_DAYS: usize = 5;

/// How much of the log's days a pattern must show on: three days in five. A routine that
/// happens on weekdays only (five days in seven) clears it; one that happened three times in
/// a fortnight does not.
pub const AGREE_ON: f64 = 0.6;

/// How wide "about the same time" is, in minutes. An hour: wide enough that leaving for work
/// at 8:05 and 8:40 is one habit, narrow enough that morning and evening are two.
pub const WITHIN_MINUTES: i64 = 60;

/// More changes than this per day, on average, is a thing that flaps, not one that keeps a
/// routine: a motion sensor, a light on an automation. It would show a "pattern" at every
/// hour of the day.
pub const MOST_PER_DAY: usize = 6;

/// How many patterns are returned. A list of everything is a dashboard; the strongest are
/// what a person reads.
pub const MOST_RHYTHMS: usize = 12;

/// Something the house does at about the same time on most days.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rhythm {
    /// Which thing, namespaced by the server that reported it.
    pub key: String,
    /// What it became.
    pub to: String,
    /// The middle of the usual window, in minutes after local midnight.
    pub minute: u32,
    /// On how many days it happened inside the window.
    pub days_seen: usize,
    /// How many days the log covers (days on which anything at all was recorded).
    pub days_observed: usize,
}

impl Rhythm {
    /// What to say, as a count the person can check.
    #[must_use]
    pub fn describe(&self) -> String {
        // Nearest five minutes: the window is an hour wide, so a minute is false precision.
        let rounded = (self.minute + 2) / 5 * 5;
        let thing = self.key.rsplit("::").next().unwrap_or(&self.key);
        format!(
            "{thing} goes {} around {:02}:{:02}, on {} of the last {} days",
            self.to,
            (rounded / 60) % 24,
            rounded % 60,
            self.days_seen,
            self.days_observed
        )
    }
}

/// The routines in `transitions`, strongest first.
///
/// `offset_minutes_at` gives the person's distance from UTC at a moment (negative to the
/// west): a pattern at 07:30 is only a pattern at 07:30 if the days are theirs, including
/// either side of a clock change. The server stores instants and does not know where the
/// person is.
#[must_use]
pub fn find_rhythms(
    transitions: &[Transition],
    offset_minutes_at: impl Fn(i64) -> i64,
) -> Vec<Rhythm> {
    let local = |at_ms: i64| at_ms + offset_minutes_at(at_ms) * MINUTE_MS;
    let day_of = |at_ms: i64| local(at_ms).div_euclid(DAY_MS);

    // The days the log has anything on, whichever thing it was. A day the log was off is not
    // a day a routine failed to happen.
    let days_observed = transitions
        .iter()
        .map(|t| day_of(t.at_ms))
        .collect::<BTreeSet<_>>()
        .len();
    if days_observed < MIN_DAYS {
        return Vec::new();
    }

    let mut groups: BTreeMap<(&str, &str), Vec<&Transition>> = BTreeMap::new();
    for transition in transitions {
        if is_not_a_routine(&transition.to) || keeps_time(&transition.key) {
            continue;
        }
        groups
            .entry((&transition.key, &transition.to))
            .or_default()
            .push(transition);
    }

    let mut found = Vec::new();
    for ((key, to), moves) in groups {
        if moves.len() > MOST_PER_DAY * days_observed {
            continue;
        }
        let when: Vec<i64> = moves.iter().map(|t| local(t.at_ms)).collect();
        let Some((start, mut inside)) = densest_window(&when) else {
            continue;
        };
        if inside.len() < MIN_DAYS || (inside.len() as f64) < AGREE_ON * days_observed as f64 {
            continue;
        }
        inside.sort_unstable();
        let middle = inside[inside.len() / 2];
        found.push(Rhythm {
            key: key.to_owned(),
            to: to.to_owned(),
            minute: u32::try_from((start + middle).rem_euclid(DAY_MINUTES)).unwrap_or(0),
            days_seen: inside.len(),
            days_observed,
        });
    }
    found.sort_by(|a, b| {
        let share = |r: &Rhythm| r.days_seen as f64 / r.days_observed as f64;
        share(b)
            .total_cmp(&share(a))
            .then(b.days_seen.cmp(&a.days_seen))
            .then(a.key.cmp(&b.key))
            .then(a.to.cmp(&b.to))
    });
    found.truncate(MOST_RHYTHMS);
    found
}

/// Whether a state is not something that happens at a time of day: a reading (a number),
/// or a thing admitting it cannot be seen, which is trouble and not routine.
fn is_not_a_routine(state: &str) -> bool {
    let state = state.trim();
    state.parse::<f64>().is_ok()
        || crate::not_answering(state)
        || state.eq_ignore_ascii_case("unknown")
        || state.is_empty()
        || reads_as_a_date_or_time(state)
}

/// `2026-10-15`, `07:30`, `2026-10-15T07:30:00`: a clock reading, which changes on schedule.
fn reads_as_a_date_or_time(state: &str) -> bool {
    let digits_and = |allowed: &[char]| {
        state.chars().any(|c| c.is_ascii_digit())
            && state
                .chars()
                .all(|c| c.is_ascii_digit() || allowed.contains(&c))
    };
    (state.len() >= 5 && digits_and(&['-', ':', 'T', '.', '+', 'Z', ' ']))
        && (state.contains('-') || state.contains(':'))
}

/// Whether a thing changes on the clock or the sky by definition, so its "routine" is a fact
/// about the calendar and not about the house: the sun rising, the moon's phase, a sensor
/// whose state is today's date. Found on the first live run, where sunrise and sunset were
/// two of the four patterns reported. A short list of what Home Assistant itself provides,
/// matched on the thing's own name; a state that reads as a date or a time is caught below
/// whatever the thing is called.
fn keeps_time(key: &str) -> bool {
    const CLOCKS: &[&str] = &[
        "sun.",
        "sensor.date",
        "sensor.time",
        "sensor.moon",
        "sensor.season",
    ];
    let thing = key.rsplit("::").next().unwrap_or(key);
    CLOCKS.iter().any(|clock| thing.starts_with(clock))
}

/// The `WITHIN_MINUTES` stretch of the day that the most separate days fall in, going round the
/// clock (23:50 and 00:10 are twenty minutes apart), as its start in minutes after midnight
/// and, for each day it holds, how far into the window the first of that day's changes came.
///
/// A "day" here is one occurrence of the window itself, not a calendar date: 23:50 on Monday
/// and 00:10 on Tuesday are the same night, so a routine across midnight is counted once a
/// night and not once for each side of the date change. Several changes in one occurrence of
/// the window count once, because a routine is when it starts.
fn densest_window(when: &[i64]) -> Option<(i64, Vec<i64>)> {
    let starts: BTreeSet<i64> = when
        .iter()
        .map(|at| at.rem_euclid(DAY_MS) / MINUTE_MS)
        .collect();
    starts
        .into_iter()
        .map(|start| {
            let mut first_each_night: BTreeMap<i64, i64> = BTreeMap::new();
            for at in when {
                let offset = (at.rem_euclid(DAY_MS) / MINUTE_MS - start).rem_euclid(DAY_MINUTES);
                if offset >= WITHIN_MINUTES {
                    continue;
                }
                let night = (at - offset * MINUTE_MS).div_euclid(DAY_MS);
                first_each_night
                    .entry(night)
                    .and_modify(|first| *first = (*first).min(offset))
                    .or_insert(offset);
            }
            (start, first_each_night.into_values().collect::<Vec<_>>())
        })
        .max_by(|(sa, a), (sb, b)| a.len().cmp(&b.len()).then(sb.cmp(sa)))
}

#[cfg(test)]
mod tests;
