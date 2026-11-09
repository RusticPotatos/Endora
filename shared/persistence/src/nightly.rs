//! Nightly copies of a database, made so the house's backup always finds a whole one (ADR 0098).
//!
//! A file copied while a service writes to it can look whole and fail to open on the day it is
//! needed. So at 01:30 by the person's clock the service writes a consistent, integrity-checked
//! copy beside the database (`daily-YYYY-MM-DD.db`, and on Sundays `weekly-YYYY-MM-DD.db`) and
//! keeps the newest seven of each. Files it did not name are never touched.
//!
//! The schedule, the names, the pruning and the copy are here once, for every store whose copy
//! Core owns: the node's `endora.db` and HQ's `hq.db` each call [`copy_tonight`] from their own
//! ten-minute loop and say in their own words what happened.

use std::path::Path;

/// When tonight's copy is due, in minutes after local midnight.
const COPY_AT_MINUTE: i64 = 90;

/// How many of each kind are kept.
const KEEP_EACH: usize = 7;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// The person's day as a count since 1970-01-01, at `now_ms` (`offset_minutes` from UTC then).
#[must_use]
pub fn local_day(now_ms: i64, offset_minutes: i64) -> i64 {
    (now_ms + offset_minutes * 60 * 1000).div_euclid(DAY_MS)
}

/// `YYYY-MM-DD` for a day count since 1970-01-01 (Howard Hinnant's civil-from-days).
fn civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The copies due tonight, or `None` before 01:30 by the person's clock (`offset_minutes` from
/// UTC at `now_ms`). A daily always; a weekly too on a Sunday.
pub fn tonights_copies(now_ms: i64, offset_minutes: i64) -> Option<Vec<String>> {
    let local = now_ms + offset_minutes * 60 * 1000;
    let day = local_day(now_ms, offset_minutes);
    if local.rem_euclid(DAY_MS) < COPY_AT_MINUTE * 60 * 1000 {
        return None;
    }
    let date = civil(day);
    let mut names = vec![format!("daily-{date}.db")];
    // 1970-01-01 was a Thursday, so a day count of 3 more than a multiple of 7 is a Sunday.
    if day.rem_euclid(7) == 3 {
        names.push(format!("weekly-{date}.db"));
    }
    Some(names)
}

/// Whether `text` is exactly `YYYY-MM-DD`, as this module names its copies.
fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// Which of `existing` to remove so the newest [`KEEP_EACH`] of each kind remain. Only names
/// this module writes are considered; anything else in the folder is left alone.
pub fn to_remove(existing: &[String]) -> Vec<String> {
    let ours = |kind: &str, suffix: &str| {
        let mut mine: Vec<&String> = existing
            .iter()
            .filter(|n| {
                n.strip_prefix(kind)
                    .and_then(|rest| rest.strip_suffix(suffix))
                    .is_some_and(is_date)
            })
            .collect();
        // The date is in the name, so the names sort oldest first.
        mine.sort();
        let surplus = mine.len().saturating_sub(KEEP_EACH);
        mine.into_iter().take(surplus).cloned().collect::<Vec<_>>()
    };
    let mut gone = ours("daily-", ".db");
    gone.extend(ours("weekly-", ".db"));
    gone
}

/// Writes tonight's copies of `database` into `folder` if they are due and not yet written,
/// then prunes. Returns the size of the copy written, `None` when there was nothing to do.
///
/// # Errors
/// A message if the copy could not be made or checked: worth telling the person, because a
/// backup that silently stopped is the one that is missing on the day it is needed.
pub fn copy_tonight(
    database: &Path,
    folder: &Path,
    now_ms: i64,
    offset_minutes: i64,
) -> Result<Option<u64>, String> {
    let Some(names) = tonights_copies(now_ms, offset_minutes) else {
        return Ok(None);
    };
    let daily = folder.join(&names[0]);
    if daily.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("could not make {}: {e}", folder.display()))?;
    crate::snapshot(database, &daily).map_err(|e| e.to_string())?;
    // The weekly is the same consistent copy; the daily is closed and checked by now.
    for weekly in names.iter().skip(1) {
        std::fs::copy(&daily, folder.join(weekly)).map_err(|e| e.to_string())?;
    }
    let existing: Vec<String> = std::fs::read_dir(folder)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    for old in to_remove(&existing) {
        let _ = std::fs::remove_file(folder.join(old));
    }
    let bytes = std::fs::metadata(&daily).map_err(|e| e.to_string())?.len();
    Ok(Some(bytes))
}

#[cfg(test)]
#[path = "nightly_tests.rs"]
mod tests;
