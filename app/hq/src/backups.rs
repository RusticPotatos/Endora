//! HQ's own nightly copy of its store (milestone f).
//!
//! HQ's goals, tasks and desk are one SQLite file, `hq.db`, in its own volume. The house's backup
//! copies that file at night, but a file copied while HQ writes to it can look whole and fail to
//! open on the day it is needed. So HQ keeps what the node keeps for `endora.db`: at 01:30 by the
//! owner's clock a consistent, integrity-checked copy in `backups/` beside the database
//! (`daily-YYYY-MM-DD.db`, `weekly-YYYY-MM-DD.db` on Sundays, the newest seven of each). Each
//! copy is proven by restore before it is kept, and retried if it fails (`endora_persistence`'s
//! `nightly` and `restore`).
//!
//! The schedule, the names, the pruning and the copy are `endora_persistence::nightly`'s, the
//! same code the node runs; this file is only HQ's loop around it and its words in the log.

use std::path::PathBuf;

use endora_persistence::nightly::{Nightly, Tick};

use crate::time_zone::TimeZone;

/// What HQ's log says about one tick of the nightly, if anything: a line a night when the copy is
/// proven, and one line, however many tries it took, when it has given up.
fn words(tick: &Tick) -> Option<String> {
    tick.summary("HQ's data")
}

/// Makes tonight's copies once they are due, proving each by restore and trying again if one
/// does not prove (the loop is `Nightly::run`'s, shared with the node), and says in the log what
/// happened.
pub fn spawn(database: PathBuf, zone: TimeZone) {
    tokio::spawn(Nightly::new().run(
        database,
        move || {
            let now = crate::now_ms();
            (now, zone.offset_minutes_at(now))
        },
        |tick| {
            if let Tick::Failed { reason, .. } = tick {
                eprintln!("hq: tonight's copy of HQ's data failed: {reason}");
            }
            if let Some(line) = words(tick) {
                println!("hq: {line}");
            }
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use endora_persistence::nightly::folder_of;
    use endora_persistence::prove_restore;
    use endora_team::infrastructure::SqliteTeamStore;
    use std::path::Path;

    const MINUTE: i64 = 60 * 1000;
    const HOUR: i64 = 60 * MINUTE;
    // 2026-10-18 is a Sunday. 05:00 UTC on it is 01:00 four hours west of UTC.
    const SUNDAY_0500_UTC: i64 = 1_792_299_600_000;
    const DUE: i64 = SUNDAY_0500_UTC + 30 * MINUTE;

    /// A folder unique to one test, removed when it drops.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("endora-hq-backups-{tag}-{}", std::process::id()));
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

    /// A real HQ store with a goal and a desk card in it, as HQ's own code makes the file.
    /// Returned still open: HQ is running while its copy is made.
    fn an_hq_store(at: &Path) -> SqliteTeamStore {
        let store = SqliteTeamStore::open(at.to_str().unwrap()).unwrap();
        let conn = rusqlite::Connection::open(at).unwrap();
        conn.execute(
            "INSERT INTO goals (title, reason, status) VALUES ('Water the plants', 'Jane Doe asked', 'open')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO desk (kind, goal, text, at_ms) VALUES ('question', 1, 'John Doe: which pot?', 1)",
            [],
        )
        .unwrap();
        store
    }

    /// One tick of HQ's nightly on `db`, as its loop runs it.
    fn tonight(nightly: &Nightly, db: &Path, now_ms: i64, offset: i64) -> Tick {
        nightly.tick(db, &folder_of(db), now_ms, offset)
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|d| {
                d.filter_map(|e| e.ok()?.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// H1: a daily copy (and on a Sunday a weekly) is made when due, in `backups/` beside
    /// `hq.db`, once a night, and the newest seven of each are kept.
    #[test]
    fn h1_a_nightly_copy_is_made_on_schedule_and_the_newest_seven_are_kept() {
        let scratch = Scratch::new("h1");
        let db = scratch.0.join("hq.db");
        let _open = an_hq_store(&db);
        let dir = scratch.0.join("backups");
        std::fs::create_dir_all(&dir).unwrap();
        for d in 1..=7 {
            std::fs::write(dir.join(format!("daily-2026-10-{d:02}.db")), b"old").unwrap();
        }
        // The release-time copies and a hand-made one are not the nightly's to prune.
        std::fs::write(dir.join("before-0.12.0-hq.db"), b"release").unwrap();
        std::fs::write(dir.join("by-hand.db"), b"mine").unwrap();

        let nightly = Nightly::new();
        assert_eq!(
            tonight(&nightly, &db, SUNDAY_0500_UTC, -240),
            Tick::Nothing,
            "01:00 local: not due"
        );
        assert!(!dir.join("daily-2026-10-18.db").exists());

        let Tick::Proven { bytes } = tonight(&nightly, &db, DUE, -240) else {
            panic!("due, so written and proven");
        };
        assert!(bytes > 0);
        assert!(dir.join("daily-2026-10-18.db").exists());
        assert!(dir.join("weekly-2026-10-18.db").exists(), "a Sunday");
        assert!(
            !dir.join("daily-2026-10-01.db").exists(),
            "the eighth is pruned"
        );
        assert!(dir.join("daily-2026-10-02.db").exists());
        assert!(dir.join("before-0.12.0-hq.db").exists());
        assert!(dir.join("by-hand.db").exists());
        assert_eq!(
            tonight(&nightly, &db, DUE + HOUR, -240),
            Tick::Nothing,
            "already written tonight"
        );
        assert_eq!(folder_of(&db), dir);
    }

    /// H2: the copy passes the same restore proof the node's does.
    #[test]
    fn h2_the_copy_passes_the_restore_proof() {
        let scratch = Scratch::new("h2");
        let db = scratch.0.join("hq.db");
        let _open = an_hq_store(&db);
        let nightly = Nightly::new();
        assert!(matches!(
            tonight(&nightly, &db, DUE, -240),
            Tick::Proven { .. }
        ));
        for name in ["daily-2026-10-18.db", "weekly-2026-10-18.db"] {
            let copy = scratch.0.join("backups").join(name);
            prove_restore(&copy, &db).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        // And the proof is not a rubber stamp: a copy missing the goal rows fails it.
        let hollow = scratch.0.join("hollow.db");
        endora_persistence::snapshot(&db, &hollow).unwrap();
        rusqlite::Connection::open(&hollow)
            .unwrap()
            .execute_batch("DELETE FROM desk; DELETE FROM goals;")
            .unwrap();
        assert!(prove_restore(&hollow, &db).is_err());
    }

    /// H3: neither the copy nor its proof writes to the live store: it is byte-identical and
    /// no older (a stronger test than a SHA-256, which would also miss a touched mtime).
    #[test]
    fn h3_the_live_store_is_unchanged_by_the_copy_and_its_proof() {
        let scratch = Scratch::new("h3");
        let db = scratch.0.join("hq.db");
        let open = an_hq_store(&db);
        let (bytes, modified) = (
            std::fs::read(&db).unwrap(),
            std::fs::metadata(&db).unwrap().modified().unwrap(),
        );
        assert!(matches!(
            tonight(&Nightly::new(), &db, DUE, -240),
            Tick::Proven { .. }
        ));
        prove_restore(&scratch.0.join("backups/daily-2026-10-18.db"), &db).unwrap();
        assert_eq!(std::fs::read(&db).unwrap(), bytes, "bytes unchanged");
        assert_eq!(
            std::fs::metadata(&db).unwrap().modified().unwrap(),
            modified,
            "not even touched"
        );
        // No journal or other file appeared beside it; only `backups/` was added to.
        assert_eq!(listing(&scratch.0), ["backups", "hq.db"]);
        drop(open);
    }

    /// A copy that comes out cut in half, as a disk that filled up would leave it.
    fn truncating_copy(from: &Path, to: &Path) -> Result<(), String> {
        endora_persistence::snapshot(from, to).map_err(|e| e.to_string())?;
        let bytes = std::fs::read(to).map_err(|e| e.to_string())?;
        std::fs::write(to, &bytes[..bytes.len() / 2]).map_err(|e| e.to_string())
    }

    fn real_copy(from: &Path, to: &Path) -> Result<(), String> {
        endora_persistence::snapshot(from, to).map_err(|e| e.to_string())
    }

    fn real_proof(copy: &Path, live: &Path) -> Result<(), String> {
        prove_restore(copy, live).map_err(|e| e.to_string())
    }

    /// HQ's copy is proven by restore, said so in the log, and a bad one is retried: the same
    /// shared nightly the node runs, with HQ's real rows.
    #[test]
    fn hq_proves_its_copy_and_retries_a_bad_one_with_the_shared_cap() {
        let scratch = Scratch::new("hq-proof");
        let db = scratch.0.join("hq.db");
        let _open = an_hq_store(&db);
        let dir = folder_of(&db);
        let nightly = Nightly::new();
        let bad: Vec<Tick> = (0..4)
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
        let lines: Vec<String> = bad.iter().filter_map(words).collect();
        assert_eq!(
            lines.len(),
            1,
            "one failure line, not one a tick: {lines:?}"
        );
        assert!(lines[0].contains("3 tries"), "{}", lines[0]);
        assert_eq!(listing(&dir), Vec::<String>::new(), "no bad copy is kept");

        let tomorrow = nightly.tick_with(&db, &dir, DUE + 24 * HOUR, -240, &real_copy, &real_proof);
        let line = words(&tomorrow).expect("a good night is a line");
        assert!(line.contains("proven by restore"), "{line}");
        assert!(line.contains("HQ"), "{line}");
    }
}
