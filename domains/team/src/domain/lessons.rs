//! A loop's lessons (ADR 0142, amended 2026-10-31, after snarktank/ralph's "Codebase
//! Patterns"): a few short lines every later work turn on the loop reads, marked by a worker or
//! a reviewer, or set by the owner. A lead's lessons in a goal's notes retired with the leads.

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
}
