//! Per-member notebooks (ADR 0118). Each member keeps short notes of what it has learned, and
//! each note rests on evidence from HQ's record, so a note is a reading and not a hunch. A
//! member that makes mistakes is not trusted to remember them for ever: notes are tidied, and
//! one nobody used for a month is marked stale. A member writes only its own notebook; the
//! owner may write any, and an owner's note needs no evidence and says whose it is.

use super::evidence::Ref;
use super::work::{MemberId, Millis, Refusal, RunId};

/// The most characters a note may run to.
pub const NOTE_MOST: usize = 300;

/// The most notes one notebook holds: past it, a member forgets some first.
pub const NOTES_MOST: usize = 60;

/// The most notes a briefing lists, newest first.
pub const BRIEFED_NOTES: usize = 12;

/// The most characters of notes a briefing carries.
pub const BRIEFED_CHARS: usize = 1_200;

/// How long a member goes between tidying its notebook before it is asked to.
pub const TIDY_EVERY_MS: Millis = 7 * 24 * 60 * 60 * 1000;

/// How long a briefing waits before asking again for a tidy it already asked for.
pub const TIDY_ASKED_EVERY_MS: Millis = 24 * 60 * 60 * 1000;

/// How long a note goes unused before a briefing marks it stale.
pub const STALE_AFTER_MS: Millis = 30 * 24 * 60 * 60 * 1000;

/// What a member is asked, once its notebook is a week without a tidy.
pub const TIDY_UP: &str =
    "Tidy your notes: forget any that are wrong or no longer true (cite why).";

/// One note in a member's notebook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Its number: `n<id>` as members name it.
    pub id: i64,
    /// Whose notebook.
    pub member: MemberId,
    /// What it says: one line, at most [`NOTE_MOST`] characters.
    pub text: String,
    /// What it rests on, from HQ's record: at least one for a member's note; none for the
    /// owner's.
    pub evidence: Vec<Ref>,
    /// Whether the owner wrote it.
    pub from_owner: bool,
    /// When it was written.
    pub written_at: Millis,
    /// The turn it was written in, for a member's note.
    pub written_in_run: Option<RunId>,
    /// When a briefing last had the member cite it or its evidence; its writing, until then.
    pub last_used_at: Millis,
}

impl Note {
    /// Whether the note has gone unused for [`STALE_AFTER_MS`].
    #[must_use]
    pub fn stale(&self, now: Millis) -> bool {
        now - self.last_used_at.max(self.written_at) > STALE_AFTER_MS
    }

    /// The note as a briefing and a reading show it: `n12: text (stale)`.
    #[must_use]
    pub fn line(&self, now: Millis) -> String {
        let mark = if self.from_owner {
            " (from the owner)"
        } else {
            ""
        };
        let stale = if self.stale(now) { " (stale)" } else { "" };
        format!("n{}: {}{mark}{stale}", self.id, self.text)
    }

    /// Whether `text` cites the note by its number, `n12`, as its own word.
    #[must_use]
    pub fn cited_in(&self, text: &str) -> bool {
        let want = format!("n{}", self.id);
        text.split(|c: char| !c.is_alphanumeric())
            .any(|word| word.eq_ignore_ascii_case(&want))
    }

    /// Whether the note rests on any of `cited`.
    #[must_use]
    pub fn rests_on(&self, cited: &[Ref]) -> bool {
        self.evidence.iter().any(|e| cited.contains(e))
    }
}

/// A note's words, kept: on one line, without control characters.
///
/// # Errors
/// [`Refusal::EmptyNote`] with no words; [`Refusal::TooLong`] past [`NOTE_MOST`].
pub fn clean(text: &str) -> Result<String, Refusal> {
    let text: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if text.is_empty() {
        return Err(Refusal::EmptyNote);
    }
    if text.chars().count() > NOTE_MOST {
        return Err(Refusal::TooLong(NOTE_MOST));
    }
    Ok(text)
}

/// The notebook as a briefing carries it: `Your notes (N):` and the newest first, at most
/// [`BRIEFED_NOTES`] of them and [`BRIEFED_CHARS`] characters, the rest summarised. Nothing for
/// an empty notebook. With `short`, only the count and the tool that reads them (the CEO's
/// briefing is kept short).
#[must_use]
pub fn briefing(notes: &[Note], now: Millis, short: bool) -> String {
    if notes.is_empty() {
        return String::new();
    }
    if short {
        return format!("Your notes ({}): read_notebook shows them.\n", notes.len());
    }
    let mut told = format!("Your notes ({}):\n", notes.len());
    let mut shown = 0;
    let mut chars = 0;
    for note in notes.iter().rev().take(BRIEFED_NOTES) {
        let line = note.line(now);
        if shown > 0 && chars + line.chars().count() > BRIEFED_CHARS {
            break;
        }
        chars += line.chars().count();
        told.push_str(&format!("- {line}\n"));
        shown += 1;
    }
    if shown < notes.len() {
        let more = notes.len() - shown;
        told.push_str(&format!(
            "and {more} older note{}: use read_notebook\n",
            if more == 1 { "" } else { "s" }
        ));
    }
    told
}

/// Whether a member is asked to tidy its notebook now: it has notes, the last tidy (or, never
/// tidied, its oldest note) is a week old, and it was not asked in the last day.
#[must_use]
pub fn tidy_due(
    notes: &[Note],
    tidied: Option<Millis>,
    asked: Option<Millis>,
    now: Millis,
) -> bool {
    let Some(oldest) = notes.iter().map(|n| n.written_at).min() else {
        return false;
    };
    if now - tidied.unwrap_or(oldest) < TIDY_EVERY_MS {
        return false;
    }
    asked.is_none_or(|at| now - at >= TIDY_ASKED_EVERY_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Millis = 24 * 60 * 60 * 1000;

    fn note(id: i64, text: &str, at: Millis) -> Note {
        Note {
            id,
            member: MemberId(1),
            text: text.to_owned(),
            evidence: vec![Ref::Task(super::super::work::TaskId(3))],
            from_owner: false,
            written_at: at,
            written_in_run: None,
            last_used_at: at,
        }
    }

    #[test]
    fn a_note_is_one_line_of_words_and_at_most_300_characters() {
        assert_eq!(
            clean("  sonarr \n restarts   fine "),
            Ok("sonarr restarts fine".to_owned())
        );
        assert_eq!(clean("  \n "), Err(Refusal::EmptyNote));
        assert_eq!(clean(&"x".repeat(300)), Ok("x".repeat(300)));
        assert_eq!(clean(&"x".repeat(301)), Err(Refusal::TooLong(300)));
    }

    #[test]
    fn a_note_is_stale_after_a_month_unused_and_said_so() {
        let n = note(4, "deluge needs the VPN up", 0);
        assert!(!n.stale(30 * DAY));
        assert!(n.stale(30 * DAY + 1));
        assert_eq!(n.line(DAY), "n4: deluge needs the VPN up");
        assert_eq!(n.line(31 * DAY), "n4: deluge needs the VPN up (stale)");
        let mut used = n.clone();
        used.last_used_at = 20 * DAY;
        assert!(!used.stale(31 * DAY));
        let mut owners = n;
        owners.from_owner = true;
        assert_eq!(
            owners.line(0),
            "n4: deluge needs the VPN up (from the owner)"
        );
    }

    #[test]
    fn a_note_is_cited_by_its_number_or_its_evidence() {
        let n = note(12, "x", 0);
        assert!(n.cited_in("As n12 says, the VPN is needed"));
        assert!(n.cited_in("(N12)"));
        assert!(!n.cited_in("n123 and n1 are other notes"));
        assert!(n.rests_on(&[Ref::Event(9), Ref::Task(super::super::work::TaskId(3))]));
        assert!(!n.rests_on(&[Ref::Event(9)]));
    }

    #[test]
    fn a_briefing_lists_the_newest_twelve_in_twelve_hundred_characters_and_sums_up_the_rest() {
        assert_eq!(briefing(&[], 0, false), "");
        let few: Vec<Note> = (1..=3).map(|i| note(i, "short", 0)).collect();
        let told = briefing(&few, 0, false);
        assert!(
            told.starts_with("Your notes (3):\n- n3: short\n- n2: short\n- n1: short\n"),
            "{told}"
        );
        assert!(!told.contains("older"), "{told}");
        // Fifteen short notes: twelve shown, three summarised.
        let many: Vec<Note> = (1..=15).map(|i| note(i, "short", 0)).collect();
        let told = briefing(&many, 0, false);
        assert_eq!(told.matches("\n- n").count(), 12, "{told}");
        assert!(
            told.ends_with("and 3 older notes: use read_notebook\n"),
            "{told}"
        );
        // Long notes run out of characters first.
        let long: Vec<Note> = (1..=12).map(|i| note(i, &"y".repeat(290), 0)).collect();
        let told = briefing(&long, 0, false);
        let shown = told.matches("\n- n").count();
        assert!((1..=4).contains(&shown), "{shown}");
        assert!(told.contains("older notes: use read_notebook"), "{told}");
        assert!(
            told.chars().count() < BRIEFED_CHARS + 200,
            "{}",
            told.chars().count()
        );
        // One older note is said in the singular.
        let two: Vec<Note> = (1..=13).map(|i| note(i, "short", 0)).collect();
        assert!(briefing(&two, 0, false).ends_with("and 1 older note: use read_notebook\n"));
    }

    #[test]
    fn the_ceos_short_briefing_carries_only_the_count_and_the_tool() {
        let many: Vec<Note> = (1..=15).map(|i| note(i, "short", 0)).collect();
        assert_eq!(
            briefing(&many, 0, true),
            "Your notes (15): read_notebook shows them.\n"
        );
        assert_eq!(briefing(&[], 0, true), "");
    }

    #[test]
    fn a_tidy_is_asked_a_week_after_the_last_one_and_at_most_once_a_day() {
        let notes = [note(1, "x", 0)];
        assert!(
            !tidy_due(&[], None, None, 99 * DAY),
            "no notes, nothing to tidy"
        );
        assert!(
            !tidy_due(&notes, None, None, 6 * DAY),
            "counted from its oldest note"
        );
        assert!(tidy_due(&notes, None, None, 7 * DAY));
        assert!(!tidy_due(&notes, Some(3 * DAY), None, 9 * DAY));
        assert!(tidy_due(&notes, Some(3 * DAY), None, 10 * DAY));
        assert!(
            !tidy_due(&notes, None, Some(7 * DAY), 7 * DAY + 1000),
            "asked an hour ago"
        );
        assert!(tidy_due(&notes, None, Some(7 * DAY), 8 * DAY));
    }
}
