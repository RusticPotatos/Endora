//! Endora's own copies of its database, made every night so the house's backup always finds a
//! whole one.
//!
//! Everything Endora knows (every setting, key and sign-in secret, and its memory) is one
//! SQLite file. The schedule, the names, the pruning and the copy itself are
//! `endora_persistence::nightly`'s, shared with HQ's own store; what is here is the node's loop
//! around them, and its words in Activity.

use super::*;
use endora_persistence::nightly::{Nightly, Tick};

/// What Activity says about one tick of the nightly, if anything: a line a night when the
/// copy is proven, and, once a night however many tries it took, when it has given up.
fn line(tick: &Tick) -> Option<String> {
    tick.summary("Endora's data")
}

/// Makes tonight's copies once they are due, proving each by restore and trying again if one
/// does not prove (the loop is `Nightly::run`'s, shared with HQ), and says in Activity what
/// happened. The copies go in `backups/` beside the database, where the house's backup finds
/// them.
pub fn spawn_snapshots(state: AppState, database: std::path::PathBuf) {
    let clock = state.clone();
    tokio::spawn(Nightly::new().run(
        database,
        move || {
            let now = clock.clock.now().unix_millis();
            (now, clock.time_zone.offset_minutes_at(now))
        },
        move |tick| {
            if let Tick::Failed { reason, .. } = tick {
                eprintln!("snapshots: tonight's copy failed: {reason}");
            }
            let Some(summary) = line(tick) else {
                return;
            };
            record_event(state.events.as_ref(), state.clock.as_ref(), &summary);
            let _ = state.changes.send(());
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    const MINUTE: i64 = 60 * 1000;
    // 2026-10-18 is a Sunday. 05:30 UTC on it is 01:30 four hours west of UTC: the copy is due.
    const DUE: i64 = 1_792_299_600_000 + 30 * MINUTE;

    /// A folder unique to one test, removed when it drops.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "endora-node-snapshots-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A live node database with rows, as the nightly finds it.
    fn a_node_database(at: &Path, padding_kb: usize) {
        let conn = rusqlite::Connection::open(at).unwrap();
        conn.execute_batch(
            "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE blobs (id INTEGER PRIMARY KEY, bytes BLOB NOT NULL);
             INSERT INTO people (name) VALUES ('John Doe');",
        )
        .unwrap();
        conn.execute_batch("BEGIN").unwrap();
        for _ in 0..padding_kb {
            conn.execute("INSERT INTO blobs (bytes) VALUES (randomblob(1024))", [])
                .unwrap();
        }
        conn.execute_batch("COMMIT").unwrap();
    }

    fn truncating_copy(from: &Path, to: &Path) -> Result<(), String> {
        endora_persistence::snapshot(from, to).map_err(|e| e.to_string())?;
        let bytes = std::fs::read(to).map_err(|e| e.to_string())?;
        std::fs::write(to, &bytes[..bytes.len() / 2]).map_err(|e| e.to_string())
    }

    fn real_copy(from: &Path, to: &Path) -> Result<(), String> {
        endora_persistence::snapshot(from, to).map_err(|e| e.to_string())
    }

    fn real_proof(copy: &Path, live: &Path) -> Result<(), String> {
        endora_persistence::prove_restore(copy, live).map_err(|e| e.to_string())
    }

    /// Case 1, as the node says it: a good night is one Activity line, and it says the copy was
    /// proven by restore.
    #[test]
    fn a_good_night_is_one_line_that_says_it_was_proven_by_restore() {
        let scratch = Scratch::new("good");
        let (db, dir) = (scratch.0.join("endora.db"), scratch.0.join("backups"));
        a_node_database(&db, 8);
        let nightly = Nightly::new();
        let ticks = [
            nightly.tick_with(&db, &dir, DUE, -240, &real_copy, &real_proof),
            nightly.tick_with(&db, &dir, DUE + 10 * MINUTE, -240, &real_copy, &real_proof),
        ];
        let lines: Vec<String> = ticks.iter().filter_map(line).collect();
        assert_eq!(lines.len(), 1, "one line a night: {lines:?}");
        assert!(lines[0].contains("proven by restore"), "{}", lines[0]);
        assert!(lines[0].contains("7 nights and 7 weeks"), "{}", lines[0]);
    }

    /// Case 7, as the node says it: three failed tries are one failure line, said on the third.
    #[test]
    fn three_failed_tries_are_one_failure_line() {
        let scratch = Scratch::new("three");
        let (db, dir) = (scratch.0.join("endora.db"), scratch.0.join("backups"));
        a_node_database(&db, 8);
        let live_before = std::fs::read(&db).unwrap();
        let nightly = Nightly::new();
        let ticks: Vec<Tick> = (0..5)
            .map(|n| {
                nightly.tick_with(
                    &db,
                    &dir,
                    DUE + n * 10 * MINUTE,
                    -240,
                    &truncating_copy,
                    &real_proof,
                )
            })
            .collect();
        let lines: Vec<String> = ticks.iter().filter_map(line).collect();
        assert_eq!(lines.len(), 1, "exactly one failure event: {lines:?}");
        assert!(
            lines[0].starts_with("Couldn't save tonight's copy"),
            "{}",
            lines[0]
        );
        assert!(lines[0].contains("3 tries"), "{}", lines[0]);
        assert_eq!(
            std::fs::read(&db).unwrap(),
            live_before,
            "live database unchanged"
        );
    }

    /// The proof of a 50 MB database finishes in under 30 s on a blocking thread, as the loop
    /// runs it, and passes.
    #[tokio::test]
    async fn a_fifty_megabyte_proof_takes_under_thirty_seconds_off_the_executor() {
        let scratch = Scratch::new("big");
        let (db, copy) = (scratch.0.join("endora.db"), scratch.0.join("daily.db"));
        a_node_database(&db, 50 * 1024);
        endora_persistence::snapshot(&db, &copy).unwrap();
        assert!(std::fs::metadata(&copy).unwrap().len() >= 50_000_000);
        let started = std::time::Instant::now();
        let proof =
            tokio::task::spawn_blocking(move || endora_persistence::prove_restore(&copy, &db))
                .await
                .unwrap();
        proof.unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "{:?}",
            started.elapsed()
        );
    }
}
