//! Moving what the team kept before its world into it (ADR 0122), as a plan worked out from the
//! old record and applied once by the store. Nothing old is deleted or changed: the house log
//! and the goals' notes stay where they were, read-only for a release, and the plan only adds.
//!
//! - Each line of the house log becomes a line of the house page's timeline at its date. The
//!   lines Endora seeded (how the lab is built, ADR 0112) are the lab page now, derived from
//!   code, so they are not copied. A line with no date takes the date of the line before it
//!   (or the move's own) and says it was undated.
//! - Each lesson in a goal's notes (`- <lesson> (<who>, <day> <month>)`) becomes a story marked
//!   to retell: what to do instead is the lesson, its handle its first words; it belongs to the
//!   area of the lead who noted it (or the tester's lead), else to the house.

use super::lessons::civil;
use super::money::day_of;
use super::work::Millis;
use super::world::{Scope, TIMELINE_LINE_MOST, handle_from};

/// Who wrote what is moved into the timeline.
pub const FROM_THE_HOUSE_LOG: &str = "the house log";

/// One line for a page's timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineLine {
    /// The page's slug.
    pub page: String,
    /// When.
    pub at: Millis,
    /// Who wrote it.
    pub by: String,
    /// What it says.
    pub line: String,
}

/// A lesson, as a story to retell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lesson {
    /// Whose it is.
    pub scope: Scope,
    /// Its handle.
    pub handle: String,
    /// The lesson, as what to do instead.
    pub instead: String,
    /// Who noted it.
    pub author: String,
    /// When it was noted.
    pub at: Millis,
    /// Its line in the goal's notes.
    pub line: String,
}

/// What the move adds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Lines for the house page's timeline, in the house log's order.
    pub timeline: Vec<TimelineLine>,
    /// Lessons, as stories to retell.
    pub lessons: Vec<Lesson>,
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// The month (1 to 12) a word names: `Oct`, `October`, `oct.`.
fn month_of(word: &str) -> Option<u32> {
    let word = word.trim_end_matches('.').to_ascii_lowercase();
    if word.len() < 3 {
        return None;
    }
    let at = MONTHS.iter().position(|m| word.starts_with(m))?;
    u32::try_from(at + 1).ok()
}

/// Days since the Unix epoch of a civil date (Howard Hinnant's days-from-civil).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Noon of a date in the owner's time; with no year, the latest one not after `now`.
fn noon_of(day: u32, month: u32, year: Option<i64>, now: Millis, offset_minutes: i64) -> Millis {
    let at = |year: i64| {
        days_from_civil(year, month, day) * 86_400_000 + 12 * 3_600_000 - offset_minutes * 60_000
    };
    if let Some(year) = year {
        return at(year);
    }
    let (this_year, _, _) = civil(day_of(now, offset_minutes));
    let this = at(this_year);
    if this > now { at(this_year - 1) } else { this }
}

/// A day, a month and maybe a year.
type Date = (u32, u32, Option<i64>);

/// A date as the house log and the lessons write it: `17 Oct 2026` or `17 Oct`.
fn date_of(said: &str) -> Option<Date> {
    let words: Vec<&str> = said.split_whitespace().collect();
    let (day, month, year) = match words.as_slice() {
        [day, month] => (day, month, None),
        [day, month, year] => (day, month, Some(year)),
        _ => return None,
    };
    let day: u32 = day.parse().ok().filter(|d| (1..=31).contains(d))?;
    let month = month_of(month)?;
    let year = match year {
        Some(y) => Some(
            y.parse::<i64>()
                .ok()
                .filter(|y| (1970..=9999).contains(y))?,
        ),
        None => None,
    };
    Some((day, month, year))
}

/// A line of the house log as `(date, entry)`: `- 17 Oct 2026: entry` or `- 17 Oct: entry`.
fn dated(line: &str) -> Option<(Date, &str)> {
    let line = line.trim().strip_prefix("- ")?;
    let (date, entry) = line.split_once(": ")?;
    Some((date_of(date)?, entry.trim()))
}

/// A lesson's line as `(lesson, who, date)`: `- <lesson> (<who>, <day> <month>)`.
fn lesson_of(line: &str) -> Option<(&str, &str, Date)> {
    let line = line.trim().strip_prefix("- ")?.strip_suffix(')')?;
    let (lesson, said) = line.rsplit_once(" (")?;
    let (who, date) = said.split_once(", ")?;
    let lesson = lesson.trim();
    if lesson.is_empty() || who.trim().is_empty() {
        return None;
    }
    Some((lesson, who.trim(), date_of(date)?))
}

/// `text` in pieces of at most [`TIMELINE_LINE_MOST`] characters.
fn pieces(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(TIMELINE_LINE_MOST)
        .map(|c| c.iter().collect())
        .collect()
}

/// The plan for moving `house_log` and the lessons in `goal_notes` into the world, at `now` in
/// the owner's time (`offset_minutes`). `seeded` are the lines Endora seeded the log with (the
/// lab page's now); `writers` say whose area each member's lessons belong to, by name; and
/// `handles` are the stories' handles already taken.
#[must_use]
pub fn plan(
    house_log: &str,
    seeded: &[String],
    goal_notes: &[String],
    writers: &[(String, Scope)],
    handles: &[String],
    now: Millis,
    offset_minutes: i64,
) -> Plan {
    let mut out = Plan::default();
    let mut last_at = None;
    for line in house_log.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if seeded.iter().any(|s| s.trim() == line) {
            continue;
        }
        let (at, said) = match dated(line) {
            Some(((day, month, year), entry)) => {
                let at = noon_of(day, month, year, now, offset_minutes);
                last_at = Some(at);
                (at, entry.to_owned())
            }
            None => (
                last_at.unwrap_or(now),
                format!(
                    "{} (undated in the house log)",
                    line.strip_prefix("- ").unwrap_or(line)
                ),
            ),
        };
        for piece in pieces(&said) {
            out.timeline.push(TimelineLine {
                page: super::world::HOUSE.slug.to_owned(),
                at,
                by: FROM_THE_HOUSE_LOG.to_owned(),
                line: piece,
            });
        }
    }
    let mut taken: Vec<String> = handles.to_vec();
    for notes in goal_notes {
        for line in notes.lines().map(str::trim) {
            let Some((lesson, who, (day, month, year))) = lesson_of(line) else {
                continue;
            };
            if out.lessons.iter().any(|l| l.line == line) {
                continue;
            }
            let scope = writers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(who))
                .map_or(Scope::House, |(_, scope)| *scope);
            let handle = handle_from(lesson, &taken);
            taken.push(handle.clone());
            out.lessons.push(Lesson {
                scope,
                handle,
                instead: lesson.to_owned(),
                author: who.to_owned(),
                at: noon_of(day, month, year, now, offset_minutes),
                line: line.to_owned(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::work::MemberId;

    /// 18 October 2026, at noon UTC.
    const OCT_18: Millis = 1_792_324_800_000;
    const DAY: Millis = 86_400_000;

    #[test]
    fn days_and_noons_are_counted_from_the_civil_date() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
        assert_eq!(noon_of(18, 10, Some(2026), OCT_18, 0), OCT_18);
        assert_eq!(noon_of(17, 10, None, OCT_18, 0), OCT_18 - DAY);
        assert_eq!(
            noon_of(20, 10, None, OCT_18, 0),
            noon_of(20, 10, Some(2025), OCT_18, 0),
            "a date with no year is the latest not after now"
        );
        assert_eq!(
            noon_of(18, 10, Some(2026), OCT_18, -240),
            OCT_18 + 4 * 3_600_000
        );
    }

    #[test]
    fn every_line_of_the_house_log_becomes_a_dated_line_of_the_house_pages_timeline() {
        let seeded = vec![
            "How this lab is built:".to_owned(),
            "- The home lab is managed by ansible-nas.".to_owned(),
        ];
        let log = "How this lab is built:\n- The home lab is managed by ansible-nas.\n\
                   - 16 Oct: The NAS has 16 GB.\n- 17 Oct 2026: Sonarr may have 2 GB.\n\
                   Kept by hand, no date.\n\n";
        let plan = plan(log, &seeded, &[], &[], &[], OCT_18, 0);
        let lines: Vec<(Millis, &str)> = plan
            .timeline
            .iter()
            .map(|l| (l.at, l.line.as_str()))
            .collect();
        assert_eq!(
            lines,
            vec![
                (OCT_18 - 2 * DAY, "The NAS has 16 GB."),
                (OCT_18 - DAY, "Sonarr may have 2 GB."),
                (
                    OCT_18 - DAY,
                    "Kept by hand, no date. (undated in the house log)"
                ),
            ]
        );
        assert!(
            plan.timeline
                .iter()
                .all(|l| l.page == "house" && l.by == "the house log")
        );
    }

    #[test]
    fn a_long_line_is_kept_whole_in_pieces() {
        let long = format!("- 17 Oct: {}", "x".repeat(TIMELINE_LINE_MOST + 20));
        let plan = plan(&long, &[], &[], &[], &[], OCT_18, 0);
        assert_eq!(plan.timeline.len(), 2);
        assert_eq!(
            plan.timeline.iter().map(|l| l.line.len()).sum::<usize>(),
            TIMELINE_LINE_MOST + 20
        );
    }

    #[test]
    fn each_lesson_becomes_a_story_to_retell_in_its_writers_area() {
        let notes = vec![
            "Owntone is being retired.\n- Closed too soon: the alarm came back. (Dee, 11 Oct)\n\
             - RSS that grabs nothing is normal. (Ada, 16 Oct)"
                .to_owned(),
            "- Closed too soon: again. (Someone gone, 12 Oct)\n\
             - RSS that grabs nothing is normal. (Ada, 16 Oct)"
                .to_owned(),
        ];
        let writers = vec![
            ("Ada".to_owned(), Scope::Area(MemberId(2))),
            ("Dee".to_owned(), Scope::Area(MemberId(2))),
        ];
        let plan = plan("", &[], &notes, &writers, &[], OCT_18, 0);
        let got: Vec<(&str, Scope, &str, &str)> = plan
            .lessons
            .iter()
            .map(|l| {
                (
                    l.handle.as_str(),
                    l.scope,
                    l.author.as_str(),
                    l.instead.as_str(),
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "closed-too-soon",
                    Scope::Area(MemberId(2)),
                    "Dee",
                    "Closed too soon: the alarm came back."
                ),
                (
                    "rss-that-grabs",
                    Scope::Area(MemberId(2)),
                    "Ada",
                    "RSS that grabs nothing is normal."
                ),
                (
                    "closed-too-soon-2",
                    Scope::House,
                    "Someone gone",
                    "Closed too soon: again."
                ),
            ],
            "the same line twice is one story; a writer no longer on the team is the house's"
        );
        assert_eq!(plan.lessons[0].at, OCT_18 - 7 * DAY);
        assert_eq!(
            plan.lessons[0].line,
            "- Closed too soon: the alarm came back. (Dee, 11 Oct)"
        );
    }
}
