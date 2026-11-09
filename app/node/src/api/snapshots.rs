//! Endora's own copies of its database, made every night so the house's backup always finds a
//! whole one.
//!
//! Everything Endora knows (every setting, key and sign-in secret, and its memory) is one
//! SQLite file. The schedule, the names, the pruning and the copy itself are
//! `endora_persistence::nightly`'s, shared with HQ's own store; what is here is the node's loop
//! around them, and its words in Activity.

use super::*;
use endora_persistence::nightly::{copy_tonight, local_day};
use std::path::Path;

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
            let today = local_day(now, offset);
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
