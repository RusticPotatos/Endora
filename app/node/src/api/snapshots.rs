//! Endora's own copies of its database, made every night so the house's backup always finds a
//! whole one.
//!
//! Everything Endora knows (every setting, key and sign-in secret, and its memory) is one
//! SQLite file. The house's backup copies that file at night, but a file copied while the node
//! writes to it can look whole and fail to open on the day it is needed. So at 01:30 by the
//! person's clock the node writes a consistent, integrity-checked copy beside it
//! (`daily-YYYY-MM-DD.db`, and on Sundays `weekly-YYYY-MM-DD.db`) and keeps the newest seven
//! of each. Files it did not name are never touched.

use super::*;
use std::path::Path;

/// When tonight's copy is due, in minutes after local midnight.
const COPY_AT_MINUTE: i64 = 90;

/// How many of each kind are kept.
const KEEP_EACH: usize = 7;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

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
pub(super) fn tonights_copies(now_ms: i64, offset_minutes: i64) -> Option<Vec<String>> {
    let local = now_ms + offset_minutes * 60 * 1000;
    let day = local.div_euclid(DAY_MS);
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
pub(super) fn to_remove(existing: &[String]) -> Vec<String> {
    let ours = |kind: &str| {
        let mut mine: Vec<&String> = existing
            .iter()
            .filter(|n| {
                n.strip_prefix(kind)
                    .and_then(|rest| rest.strip_suffix(".db"))
                    .is_some_and(is_date)
            })
            .collect();
        // The date is in the name, so the names sort oldest first.
        mine.sort();
        let surplus = mine.len().saturating_sub(KEEP_EACH);
        mine.into_iter().take(surplus).cloned().collect::<Vec<_>>()
    };
    let mut gone = ours("daily-");
    gone.extend(ours("weekly-"));
    gone
}

/// Writes tonight's copies of `database` into `folder` if they are due and not yet written,
/// then prunes. Returns the size of the copy written, `None` when there was nothing to do.
///
/// # Errors
/// A message if the copy could not be made or checked: worth telling the person, because a
/// backup that silently stopped is the one that is missing on the day it is needed.
pub(super) fn copy_tonight(
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
    endora_persistence::snapshot(database, &daily).map_err(|e| e.to_string())?;
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

/// Makes tonight's copies once they are due, checking every ten minutes, and says in Activity
/// what happened: one line a night, and a failure once a day rather than every ten minutes.
/// The copies go in `backups/` beside the database, where the house's backup finds them.
pub fn spawn_snapshots(state: AppState, database: std::path::PathBuf) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(10 * 60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let folder = database
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("backups");
        let mut failed_on: Option<i64> = None;
        loop {
            ticker.tick().await;
            let now = state.clock.now().unix_millis();
            let offset = state.time_zone.offset_minutes_at(now);
            let (db, dir) = (database.clone(), folder.clone());
            let outcome =
                tokio::task::spawn_blocking(move || copy_tonight(&db, &dir, now, offset)).await;
            let error = match outcome {
                Ok(Ok(None)) => continue,
                Ok(Ok(Some(bytes))) => {
                    failed_on = None;
                    #[allow(clippy::cast_precision_loss)]
                    let megabytes = bytes as f64 / 1_000_000.0;
                    record_event(
                        state.events.as_ref(),
                        state.clock.as_ref(),
                        &format!(
                            "Saved tonight's copy of Endora's data ({megabytes:.1} MB) and checked \
                             it opens; the last 7 nights and 7 weeks are kept"
                        ),
                    );
                    let _ = state.changes.send(());
                    continue;
                }
                Ok(Err(error)) => error,
                Err(error) => error.to_string(),
            };
            let today = (now + offset * 60 * 1000).div_euclid(DAY_MS);
            if failed_on == Some(today) {
                continue;
            }
            failed_on = Some(today);
            eprintln!("snapshots: tonight's copy failed: {error}");
            record_event(
                state.events.as_ref(),
                state.clock.as_ref(),
                &format!("Couldn't save tonight's copy of Endora's data: {error}"),
            );
            let _ = state.changes.send(());
        }
    });
}

#[cfg(test)]
mod tests;
