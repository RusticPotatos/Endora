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

use super::house::without_dates;
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
pub(crate) fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Noon of a date in the owner's time; with no year, the latest one not after `now`'s day, in
/// the owner's calendar (compared by day: noon of the move's own day is later than a move in
/// the early morning, and that day is not after the move).
fn noon_of(day: u32, month: u32, year: Option<i64>, now: Millis, offset_minutes: i64) -> Millis {
    let at = |year: i64| {
        days_from_civil(year, month, day) * 86_400_000 + 12 * 3_600_000 - offset_minutes * 60_000
    };
    if let Some(year) = year {
        return at(year);
    }
    let today = day_of(now, offset_minutes);
    let (this_year, _, _) = civil(today);
    if days_from_civil(this_year, month, day) > today {
        at(this_year - 1)
    } else {
        at(this_year)
    }
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

/// A row of the house page's timeline, as the store keeps it, with its id.
pub type Row = (i64, TimelineLine);

/// A repair of one row: when and what it should say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// The row's id.
    pub id: i64,
    /// When it should be.
    pub at: Millis,
    /// What it should say.
    pub line: String,
}

/// How far before the move a yearless date must have been put to be a year out.
const A_YEAR_OUT_DAYS: i64 = 183;

/// The texts of the house log's lines whose date has no year, or follows one that has none
/// (an undated line takes the date before it): the rows that took a year from the move.
fn yearless_texts(house_log: &str) -> Vec<String> {
    let mut texts = Vec::new();
    let mut yearless = false;
    for line in house_log.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match dated(line) {
            Some(((_, _, year), entry)) => {
                yearless = year.is_none();
                if yearless {
                    texts.push(entry.to_owned());
                }
            }
            None if yearless => texts.push(line.strip_prefix("- ").unwrap_or(line).to_owned()),
            None => {}
        }
    }
    texts
}

/// What to mend in the rows the house log was moved into (`rows`, all written by
/// [`FROM_THE_HOUSE_LOG`]) after a move at `moved_at` that dated a yearless day a year out
/// (a move in the early morning compared the day's noon with the move's instant) and kept
/// the dates the writers put at the start of their lines. A row whose date had no year in the
/// log and lies more than half a year before the move takes the latest year not after the
/// move's day; a date at the start of the line goes. Rows already right are not listed.
#[must_use]
pub fn repairs(rows: &[Row], house_log: &str, moved_at: Millis, offset_minutes: i64) -> Vec<Fix> {
    let yearless = yearless_texts(house_log);
    let moved_day = day_of(moved_at, offset_minutes);
    let mut fixes = Vec::new();
    for (id, row) in rows {
        let said = row
            .line
            .strip_suffix(" (undated in the house log)")
            .unwrap_or(&row.line);
        let was_yearless = yearless.iter().any(|t| t.contains(said));
        let (_, month, day) = civil(day_of(row.at, offset_minutes));
        let a_year_out = day_of(row.at, offset_minutes) < moved_day - A_YEAR_OUT_DAYS;
        let at = if was_yearless && a_year_out {
            noon_of(day, month, None, moved_at, offset_minutes)
        } else {
            row.at
        };
        let line = without_dates(&row.line);
        let line = if line.is_empty() { &row.line } else { line };
        if at == row.at && line == row.line {
            continue;
        }
        fixes.push(Fix {
            id: *id,
            at,
            line: line.to_owned(),
        });
    }
    fixes
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
                (at, without_dates(entry).to_owned())
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

    /// 18 October 2026, 04:00 UTC: the move ran in the early morning, before the day's noon.
    const OCT_18_EARLY: Millis = OCT_18 - 8 * 3_600_000;

    #[test]
    fn a_date_with_no_year_is_compared_by_day_not_by_instant() {
        // The move's own day is not after the move: 18 Oct is this year's, though its noon is
        // later than the move's instant.
        assert_eq!(noon_of(18, 10, None, OCT_18_EARLY, 0), OCT_18);
        assert_eq!(
            noon_of(19, 10, None, OCT_18_EARLY, 0),
            OCT_18 + DAY - 365 * DAY
        );
        // In the owner's time: 01:00 on 19 Oct UTC is still 18 Oct at UTC-5.
        let late = OCT_18 + 13 * 3_600_000;
        assert_eq!(
            noon_of(19, 10, None, late, -300),
            noon_of(19, 10, Some(2025), late, -300)
        );
        assert_eq!(
            noon_of(18, 10, None, late, -300),
            noon_of(18, 10, Some(2026), late, -300)
        );
    }

    #[test]
    fn a_date_the_writer_put_in_the_text_is_not_kept_in_the_line() {
        let log = "- 17 Oct: 2025-10-18: Taken as intentional.\n\
                   - 18 Oct: 18 Oct: #668 accepted.\n\
                   - 18 Oct: 17 Oct 2025: 18 Oct: twice.";
        let plan = plan(log, &[], &[], &[], &[], OCT_18_EARLY, 0);
        let got: Vec<(Millis, &str)> = plan
            .timeline
            .iter()
            .map(|l| (l.at, l.line.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (OCT_18 - DAY, "Taken as intentional."),
                (OCT_18, "#668 accepted."),
                (OCT_18, "twice."),
            ]
        );
    }

    #[test]
    fn rows_moved_a_year_out_are_repaired_once_by_the_log_they_came_from() {
        let log = "- 18 Oct: 18 Oct: #668 accepted.\n- 16 Oct 2025: A year ago.\n\
                   - 17 Oct: 2025-10-18: Taken.\n- Undated.";
        let row = |id, at, line: &str| {
            (
                id,
                TimelineLine {
                    page: "house".to_owned(),
                    at,
                    by: FROM_THE_HOUSE_LOG.to_owned(),
                    line: line.to_owned(),
                },
            )
        };
        let a_year = 365 * DAY;
        let rows = vec![
            row(1, OCT_18 - a_year, "18 Oct: #668 accepted."),
            row(2, OCT_18 - 2 * DAY - a_year, "A year ago."),
            row(3, OCT_18 - DAY, "2025-10-18: Taken."),
            row(4, OCT_18 - DAY, "Undated. (undated in the house log)"),
        ];
        let fixes = repairs(&rows, log, OCT_18_EARLY, 0);
        let got: Vec<(i64, Millis, &str)> = fixes
            .iter()
            .map(|f| (f.id, f.at, f.line.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![(1, OCT_18, "#668 accepted."), (3, OCT_18 - DAY, "Taken."),],
            "the yearless date a year out gets the year; the year the writer wrote is kept; \
             the date the writer put in the text goes"
        );
        // Repaired rows need nothing more.
        let again: Vec<_> = fixes.iter().map(|f| row(f.id, f.at, &f.line)).collect();
        assert!(repairs(&again, log, OCT_18_EARLY, 0).is_empty());
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
