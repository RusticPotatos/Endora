//! The house log (ADR 0111): the CEO's own document of what it knows about the house and the
//! team, kept in HQ's store for the whole team rather than for one goal. The CEO adds dated
//! lines and rewrites it compactly when it fills; the owner reads and edits it on HQ's screen;
//! every turn of the CEO and the lead starts with it.

use super::work::{Millis, Refusal};

/// The house log's first entry, which Endora seeds once when the log is empty (ADR 0112):
/// how this lab is built, a line a sentence.
#[must_use]
pub fn lab_built() -> String {
    let lines: Vec<String> = endora_kernel::lab::LAB_BUILT
        .iter()
        .map(|said| format!("- {said}"))
        .collect();
    format!(
        "{}:\n{}",
        endora_kernel::lab::LAB_BUILT_TITLE,
        lines.join("\n")
    )
}

/// The most characters the house log may run to.
pub const HOUSE_LOG_MOST: usize = 6_000;
/// The most characters one entry may run to.
pub const HOUSE_ENTRY_MOST: usize = 500;

/// An entry as its line in the log: `- 17 Oct 2026: <entry>`, on one line, dated in the
/// owner's time (`offset_minutes` from UTC), with the year. The date is HQ's: a date the
/// writer put at the start of the entry is taken off (live, a CEO wrote its own, a year out,
/// and the line was dated twice).
///
/// # Errors
/// [`Refusal::TooLong`] past [`HOUSE_ENTRY_MOST`] characters. An empty entry is the caller's
/// to refuse.
pub fn line(entry: &str, at: Millis, offset_minutes: i64) -> Result<String, Refusal> {
    let entry: String = entry
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let mut entry = entry.as_str();
    while let Some(rest) = without_date(entry) {
        entry = rest;
    }
    if entry.chars().count() > HOUSE_ENTRY_MOST {
        return Err(Refusal::TooLong(HOUSE_ENTRY_MOST));
    }
    let (year, _, _) = super::lessons::civil(super::money::day_of(at, offset_minutes));
    let (day, month) = super::lessons::day_and_month(super::money::day_of(at, offset_minutes));
    Ok(format!("- {day} {month} {year}: {entry}"))
}

/// The entry after a date it starts with (`2025-10-18:`, `17 Oct:`, `18 October 2025 -`,
/// `Oct 18, 2025:`), when one is followed by a separator; `None` when it starts with none.
fn without_date(entry: &str) -> Option<&str> {
    let digits = |w: &str, most: usize| {
        !w.is_empty() && w.len() <= most && w.bytes().all(|b| b.is_ascii_digit())
    };
    let iso = |w: &str| {
        let parts: Vec<&str> = w.split('-').collect();
        parts.len() == 3
            && parts[0].len() == 4
            && digits(parts[0], 4)
            && digits(parts[1], 2)
            && digits(parts[2], 2)
    };
    let words: Vec<&str> = entry.splitn(6, ' ').collect();
    fn bare(w: &str) -> &str {
        w.trim_end_matches([':', ','])
    }
    let month = super::lessons::is_month;
    // How many words the date takes: an ISO date, a day and a month, or a month and a day.
    let mut took = match words.as_slice() {
        [first, ..] if iso(bare(first)) => 1,
        [day, m, ..] if digits(day, 2) && month(bare(m)) => 2,
        [m, day, ..] if month(m) && digits(bare(day), 2) => 2,
        _ => return None,
    };
    // And a year after it.
    if words.get(took).is_some_and(|w| digits(bare(w), 4)) {
        took += 1;
    }
    let last = words[took - 1];
    if last.ends_with(':') || last.ends_with(',') {
        return Some(entry_after(entry, took));
    }
    // A separator standing alone: `18 October 2025 - …`.
    if words
        .get(took)
        .is_some_and(|w| matches!(*w, "-" | "–" | "—" | ":"))
    {
        return Some(entry_after(entry, took + 1));
    }
    None
}

/// What follows the first `words` words of `entry`.
fn entry_after(entry: &str, words: usize) -> &str {
    entry
        .splitn(words + 1, ' ')
        .nth(words)
        .unwrap_or("")
        .trim_start()
}

/// The log with `line` added at its end.
///
/// # Errors
/// [`Refusal::HouseLogFull`] when it would then run past [`HOUSE_LOG_MOST`] characters.
pub fn appended(log: &str, line: &str) -> Result<String, Refusal> {
    let log = log.trim_end();
    let kept = if log.is_empty() {
        line.to_owned()
    } else {
        format!("{log}\n{line}")
    };
    if kept.chars().count() > HOUSE_LOG_MOST {
        return Err(Refusal::HouseLogFull);
    }
    Ok(kept)
}

/// The log as rewritten whole, by the CEO or the owner: trimmed, within its limit.
///
/// # Errors
/// [`Refusal::TooLong`] past [`HOUSE_LOG_MOST`] characters.
pub fn rewritten(log: &str) -> Result<String, Refusal> {
    let log = log.trim();
    if log.chars().count() > HOUSE_LOG_MOST {
        return Err(Refusal::TooLong(HOUSE_LOG_MOST));
    }
    Ok(log.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 17 October 2026, at noon UTC.
    const OCT_17: Millis = 1_792_238_400_000;

    #[test]
    fn an_entry_is_dated_once_with_the_year_whatever_date_the_model_wrote_in_it() {
        // Live, 17 Oct 2026 at 23:46 EDT: the CEO wrote a date of its own, with the wrong
        // year, and the log read "- 17 Oct: 2025-10-18: …".
        let late_evening = OCT_17 + 15 * 60 * 60 * 1000 + 46 * 60 * 1000;
        let edt = -4 * 60;
        for written in [
            "2025-10-18: deluge runs in gluetun's network.",
            "17 Oct: deluge runs in gluetun's network.",
            "18 October 2025 - deluge runs in gluetun's network.",
            "Oct 18, 2025: 17 Oct: deluge runs in gluetun's network.",
        ] {
            assert_eq!(
                line(written, late_evening, edt),
                Ok("- 17 Oct 2026: deluge runs in gluetun's network.".to_owned()),
                "{written}"
            );
        }
        // A date inside the entry is the entry's own.
        assert_eq!(
            line("Restarted on 17 Oct, held.", OCT_17, 0),
            Ok("- 17 Oct 2026: Restarted on 17 Oct, held.".to_owned())
        );
        assert_eq!(
            line("17 containers run.", OCT_17, 0),
            Ok("- 17 Oct 2026: 17 containers run.".to_owned())
        );
    }

    #[test]
    fn an_entry_is_one_dated_line_within_its_limit() {
        assert_eq!(
            line("Sonarr's disk\nis 90% full.", OCT_17, 0),
            Ok("- 17 Oct 2026: Sonarr's disk is 90% full.".to_owned())
        );
        assert_eq!(
            line(&"x".repeat(HOUSE_ENTRY_MOST + 1), OCT_17, 0),
            Err(Refusal::TooLong(HOUSE_ENTRY_MOST))
        );
        assert!(line(&"x".repeat(HOUSE_ENTRY_MOST), OCT_17, 0).is_ok());
    }

    #[test]
    fn the_log_takes_a_line_while_it_fits_and_is_rewritten_within_its_limit() {
        assert_eq!(appended("", "- a"), Ok("- a".to_owned()));
        assert_eq!(appended("- a\n", "- b"), Ok("- a\n- b".to_owned()));
        let full = "x".repeat(HOUSE_LOG_MOST - 2);
        assert_eq!(appended(&full, "- a"), Err(Refusal::HouseLogFull));
        assert_eq!(rewritten("  short  "), Ok("short".to_owned()));
        assert_eq!(
            rewritten(&"x".repeat(HOUSE_LOG_MOST + 1)),
            Err(Refusal::TooLong(HOUSE_LOG_MOST))
        );
    }
}
