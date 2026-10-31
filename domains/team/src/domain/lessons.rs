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

/// What marks a line as a lesson for its loop (ADR 0142, amended 2026-10-31, after
/// snarktank/ralph's "Codebase Patterns"): a line of a worker's last word, or of a reviewer's
/// notes, that begins with it (after a list's dash, in any case).
pub const MARK: &str = "Lesson:";

/// The most characters one lesson of a loop's may run to.
pub const LOOP_LESSON_MOST: usize = 160;

/// The most lessons a loop keeps.
pub const LOOP_LESSONS_MOST: usize = 10;
/// The most characters a loop's lessons run to together, a line each.
pub const LOOP_LESSONS_CHARS: usize = 1_200;

/// How a member leaves its loop a lesson: told in every work and review turn's fixed opening.
pub const HOW: &str = "A lesson for the loop, which every later work turn on it reads: a line of \
                       its own starting `Lesson:` in your last word or a review's notes, at most \
                       160 characters. HQ keeps it when the item is accepted or sent back.\n";

/// Why a lesson is not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unkept {
    /// It says nothing.
    Empty,
    /// It runs past [`LOOP_LESSON_MOST`] characters.
    TooLong,
    /// The list already says it.
    Duplicate,
    /// It holds something shaped like a credential (what it looks like).
    Secret(&'static str),
}

impl std::fmt::Display for Unkept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("a lesson says something"),
            Self::TooLong => write!(f, "a lesson is at most {LOOP_LESSON_MOST} characters"),
            Self::Duplicate => f.write_str("the loop's lessons already say that"),
            Self::Secret(what) => write!(f, "a lesson never holds a secret, and this holds {what}"),
        }
    }
}

/// A lesson's words on one line: spaces made single, control characters gone.
fn one_line(text: &str) -> String {
    let text: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The lines of `text` marked as lessons, each without its mark, on one line.
#[must_use]
pub fn marked(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let line = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .unwrap_or(line)
                .trim_start();
            let head = line.get(..MARK.len())?;
            if !head.eq_ignore_ascii_case(MARK) {
                return None;
            }
            let lesson = one_line(&line[MARK.len()..]);
            (!lesson.is_empty()).then_some(lesson)
        })
        .collect()
}

/// A lesson as two are compared: its words, lower case, without punctuation.
fn normalised(lesson: &str) -> String {
    lesson
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `lesson` may be one of a loop's: something said, short, and holding nothing shaped
/// like a credential. The lesson on one line.
///
/// # Errors
/// What is wrong with it.
pub fn checked(lesson: &str) -> Result<String, Unkept> {
    let lesson = one_line(lesson);
    if normalised(&lesson).is_empty() {
        return Err(Unkept::Empty);
    }
    if lesson.chars().count() > LOOP_LESSON_MOST {
        return Err(Unkept::TooLong);
    }
    if let Some(what) = endora_kernel::secrets::scan_outbound_secret(&lesson) {
        return Err(Unkept::Secret(what));
    }
    Ok(lesson)
}

/// The loop's lessons with `lesson` added last, the oldest going first while they are more than
/// [`LOOP_LESSONS_MOST`] or run past [`LOOP_LESSONS_CHARS`] characters.
///
/// # Errors
/// What [`checked`] refuses, and [`Unkept::Duplicate`] for a lesson the list already says.
pub fn kept(lessons: &[String], lesson: &str) -> Result<Vec<String>, Unkept> {
    let lesson = checked(lesson)?;
    let words = normalised(&lesson);
    if lessons.iter().any(|l| normalised(l) == words) {
        return Err(Unkept::Duplicate);
    }
    let mut kept: Vec<String> = lessons.to_vec();
    kept.push(lesson);
    while kept.len() > LOOP_LESSONS_MOST || chars_of(&kept) > LOOP_LESSONS_CHARS {
        kept.remove(0);
    }
    Ok(kept)
}

/// The characters lessons run to, a line each.
fn chars_of(lessons: &[String]) -> usize {
    lessons.iter().map(|l| l.chars().count() + 1).sum()
}

/// The owner's own list for a loop: each line checked, a mark taken off, blank lines and
/// repeats left out, within [`LOOP_LESSONS_MOST`] and [`LOOP_LESSONS_CHARS`].
///
/// # Errors
/// The first line refused, with why; or a list too long, said as [`Unkept::TooLong`] with the
/// line that does not fit.
pub fn owners(lines: &[String]) -> Result<Vec<String>, (String, Unkept)> {
    let mut list: Vec<String> = Vec::new();
    for line in lines {
        let line = marked(line).pop().unwrap_or_else(|| one_line(line));
        if line.is_empty() {
            continue;
        }
        let lesson = checked(&line).map_err(|why| (line.clone(), why))?;
        if list.iter().any(|l| normalised(l) == normalised(&lesson)) {
            continue;
        }
        list.push(lesson);
        if list.len() > LOOP_LESSONS_MOST || chars_of(&list) > LOOP_LESSONS_CHARS {
            return Err((line, Unkept::TooLong));
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lesson_is_a_marked_line_of_a_last_word_or_a_send_back() {
        let said = "Fixed the lint; the check passes.\n\
                    Lesson: ansible-lint runs from the fork's root, never inside a role.\n\
                    - lesson:   pin images in defaults/main.yml,\tnot in tasks\n\
                    * LESSON: Containers behind gluetun stop when it restarts.\n\
                    No lesson: here\n\
                    Lesson:\n\
                    The lessons: are not this.";
        assert_eq!(
            marked(said),
            [
                "ansible-lint runs from the fork's root, never inside a role.",
                "pin images in defaults/main.yml, not in tasks",
                "Containers behind gluetun stop when it restarts.",
            ]
        );
        assert!(marked("No lessons at all.").is_empty());
    }

    #[test]
    fn a_lesson_is_kept_only_short_new_and_with_nothing_secret_in_it() {
        let list = vec!["Pin images in defaults/main.yml.".to_owned()];
        assert_eq!(
            kept(&list, "Run the lint from the root."),
            Ok(vec![
                "Pin images in defaults/main.yml.".to_owned(),
                "Run the lint from the root.".to_owned()
            ])
        );
        assert_eq!(
            kept(&list, "pin IMAGES in defaults/main.yml"),
            Err(Unkept::Duplicate),
            "compared on its words, not its case or stops"
        );
        assert_eq!(
            kept(&list, &"x".repeat(LOOP_LESSON_MOST + 1)),
            Err(Unkept::TooLong)
        );
        assert!(kept(&list, &"x".repeat(LOOP_LESSON_MOST)).is_ok());
        assert_eq!(
            kept(&list, "The token is ghp_1234567890abcdefghijklmnopqrstuv"),
            Err(Unkept::Secret("a GitHub token"))
        );
        assert_eq!(kept(&list, " ... "), Err(Unkept::Empty));
    }

    #[test]
    fn a_full_list_lets_its_oldest_lesson_go() {
        let mut list: Vec<String> = Vec::new();
        for i in 0..LOOP_LESSONS_MOST {
            list = kept(&list, &format!("Lesson number {i}.")).unwrap();
        }
        let list = kept(&list, "The newest.").unwrap();
        assert_eq!(list.len(), LOOP_LESSONS_MOST);
        assert_eq!(list[0], "Lesson number 1.");
        assert_eq!(list.last().unwrap(), "The newest.");
        // Long lessons fill the characters first.
        let mut long: Vec<String> = Vec::new();
        for i in 0..9 {
            long = kept(&long, &format!("{i} {}", "y".repeat(150))).unwrap();
        }
        assert!(long.len() < 9, "{} lessons", long.len());
        assert!(chars_of(&long) <= LOOP_LESSONS_CHARS);
        assert!(long.last().unwrap().starts_with("8 "));
    }

    #[test]
    fn the_owners_list_is_checked_like_any_lesson() {
        let lines: Vec<String> = ["Lesson: one", "", "two", "Two."]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(owners(&lines), Ok(vec!["one".to_owned(), "two".to_owned()]));
        let secret = vec!["key sk-abcdefabcdefabcdefabcdef".to_owned()];
        assert!(matches!(owners(&secret), Err((_, Unkept::Secret(_)))));
        let many: Vec<String> = (0..=LOOP_LESSONS_MOST).map(|i| format!("l {i}")).collect();
        assert!(matches!(owners(&many), Err((_, Unkept::TooLong))));
    }

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
