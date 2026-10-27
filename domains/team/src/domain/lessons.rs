//! Lessons kept by the team (owner's decision of 2026-10-17): when the owner corrects a member,
//! or a member learns something about the house the team will need again, the lead or the
//! reviewer notes it, one line, in the goal's notes for the team, which every member reads
//! every turn and the owner edits. The notes stay within their limit: a lesson that would not
//! fit is refused, and the owner tidies them.

use super::work::{Millis, Refusal};

/// The most characters one lesson may run to.
pub const LESSON_MOST: usize = 300;

/// The months' short names, January first.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The day of the month and the month's short name of `day` (days since the Unix epoch).
pub(crate) fn day_and_month(day: i64) -> (u32, &'static str) {
    let (_, month, d) = civil(day);
    (d, MONTHS[(month as usize + 11) % 12])
}

/// The year, the month (1 to 12) and the day of the month of `day` (days since the Unix
/// epoch), by Howard Hinnant's civil-from-days, for the proleptic Gregorian calendar.
pub(crate) fn civil(day: i64) -> (i64, u32, u32) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(m <= 2);
    (
        year,
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

/// Whether `word` names a month, as people write them: `Oct`, `October`, `oct.`.
pub(crate) fn is_month(word: &str) -> bool {
    const NAMES: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    let word = word.trim_end_matches('.').to_ascii_lowercase();
    word.len() >= 3 && NAMES.iter().any(|name| name.starts_with(&word))
}

/// A lesson as its line in the notes: `- <lesson> (<who>, <day> <month>)`, on one line, at
/// `at` in the owner's time (`offset_minutes` from UTC).
///
/// # Errors
/// [`Refusal::TooLong`] past [`LESSON_MOST`] characters. An empty lesson is the caller's to
/// refuse.
pub fn line(lesson: &str, who: &str, at: Millis, offset_minutes: i64) -> Result<String, Refusal> {
    let lesson = lesson.split_whitespace().collect::<Vec<_>>().join(" ");
    let lesson: String = lesson.chars().filter(|c| !c.is_control()).collect();
    if lesson.chars().count() > LESSON_MOST {
        return Err(Refusal::TooLong(LESSON_MOST));
    }
    let (day, month) = day_and_month(super::money::day_of(at, offset_minutes));
    Ok(format!("- {lesson} ({who}, {day} {month})"))
}

/// The notes with `line` added at their end, or [`Refusal::NotesFull`] when they would then
/// run past `most` characters.
///
/// # Errors
/// [`Refusal::NotesFull`].
pub fn appended(notes: &str, line: &str, most: usize) -> Result<String, Refusal> {
    let notes = notes.trim_end();
    let kept = if notes.is_empty() {
        line.to_owned()
    } else {
        format!("{notes}\n{line}")
    };
    if kept.chars().count() > most {
        return Err(Refusal::NotesFull);
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 17 October 2026, at noon UTC.
    const OCT_17: Millis = 1_792_238_400_000;

    #[test]
    fn a_lesson_is_one_line_saying_who_and_when() {
        assert_eq!(
            line(
                "Sonarr's RSS sync that grabs nothing is normal.",
                "Ada",
                OCT_17,
                0
            ),
            Ok("- Sonarr's RSS sync that grabs nothing is normal. (Ada, 17 Oct)".to_owned())
        );
        assert_eq!(
            line("Two\nlines   become one", "Ada", OCT_17, 0),
            Ok("- Two lines become one (Ada, 17 Oct)".to_owned())
        );
        assert_eq!(
            line("Late", "Ada", OCT_17 + 11 * 3_600_000 + 1, 60).map(|l| l.ends_with("18 Oct)")),
            Ok(true),
            "the owner's day, not UTC's"
        );
        assert_eq!(
            line(&"x".repeat(LESSON_MOST + 1), "Ada", OCT_17, 0),
            Err(Refusal::TooLong(LESSON_MOST))
        );
        assert!(line(&"x".repeat(LESSON_MOST), "Ada", OCT_17, 0).is_ok());
        assert_eq!(day_and_month(0), (1, "Jan"));
        assert_eq!(day_and_month(-1), (31, "Dec"));
        assert_eq!(day_and_month(19_782), (29, "Feb"), "2024 was a leap year");
    }

    #[test]
    fn the_notes_take_a_lesson_only_while_it_fits() {
        assert_eq!(appended("", "- a", 10), Ok("- a".to_owned()));
        assert_eq!(
            appended("Old notes\n", "- a", 20),
            Ok("Old notes\n- a".to_owned())
        );
        assert_eq!(appended("123456", "- a", 10), Ok("123456\n- a".to_owned()));
        assert_eq!(appended("1234567", "- a", 10), Err(Refusal::NotesFull));
    }
}
