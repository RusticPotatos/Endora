//! The house log (ADR 0111): the CEO's own document of what it knows about the house and the
//! team, kept in HQ's store for the whole team rather than for one goal. The CEO adds dated
//! lines and rewrites it compactly when it fills; the owner reads and edits it on HQ's screen;
//! every turn of the CEO and the lead starts with it.

use super::work::{Millis, Refusal};

/// The most characters the house log may run to.
pub const HOUSE_LOG_MOST: usize = 6_000;
/// The most characters one entry may run to.
pub const HOUSE_ENTRY_MOST: usize = 500;

/// An entry as its line in the log: `- 17 Oct: <entry>`, on one line, dated in the owner's
/// time (`offset_minutes` from UTC).
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
    if entry.chars().count() > HOUSE_ENTRY_MOST {
        return Err(Refusal::TooLong(HOUSE_ENTRY_MOST));
    }
    let (day, month) = super::lessons::day_and_month(super::money::day_of(at, offset_minutes));
    Ok(format!("- {day} {month}: {entry}"))
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
    fn an_entry_is_one_dated_line_within_its_limit() {
        assert_eq!(
            line("Sonarr's disk\nis 90% full.", OCT_17, 0),
            Ok("- 17 Oct: Sonarr's disk is 90% full.".to_owned())
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
