use super::*;

const MINUTE: i64 = 60 * 1000;
const HOUR: i64 = 60 * MINUTE;
// 2026-10-18 is a Sunday. 05:00 UTC on it is 01:00 four hours west of UTC.
const SUNDAY_0500_UTC: i64 = 1_792_299_600_000;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|n| (*n).to_owned()).collect()
}

#[test]
fn nothing_is_due_before_half_past_one_by_the_persons_clock() {
    assert_eq!(tonights_copies(SUNDAY_0500_UTC, -240), None, "01:00 local");
    assert_eq!(
        tonights_copies(SUNDAY_0500_UTC + 30 * MINUTE, -240),
        Some(names(&["daily-2026-10-18.db", "weekly-2026-10-18.db"])),
        "01:30 local, and a Sunday: the weekly too"
    );
    // A weekday has only the daily.
    assert_eq!(
        tonights_copies(SUNDAY_0500_UTC + 30 * MINUTE + 24 * HOUR, -240),
        Some(names(&["daily-2026-10-19.db"]))
    );
    // The day is the person's: 23:30 UTC on Sunday is already 01:30 on Monday two hours east.
    assert_eq!(
        tonights_copies(SUNDAY_0500_UTC + 18 * HOUR + 30 * MINUTE, 120),
        Some(names(&["daily-2026-10-19.db"]))
    );
}

#[test]
fn the_newest_seven_of_each_are_kept_and_nothing_else_is_touched() {
    let mut existing: Vec<String> = (1..=9)
        .map(|d| format!("daily-2026-10-{d:02}.db"))
        .chain((1..=8).map(|w| format!("weekly-2026-0{w}-01.db")))
        .collect();
    existing.push("by-hand-before-upgrade.db".to_owned());
    existing.push("daily-notes.txt".to_owned());
    // Copies someone made by hand, named like ours but not ours, are not pruned.
    existing.push("daily-2026-10-15-before-upgrade.db".to_owned());
    existing.push("daily-2026-1x-01.db".to_owned());
    existing.push("daily-2026-10.db".to_owned());
    // The brain's copies are no longer the nightly's to prune: they go once, at start, with the
    // brain (ADR 0136, `crate::retired`).
    existing.push("brain-daily-2026-10-01.bundle".to_owned());
    let mut gone = to_remove(&existing);
    gone.sort();
    assert_eq!(
        gone,
        names(&[
            "daily-2026-10-01.db",
            "daily-2026-10-02.db",
            "weekly-2026-01-01.db"
        ])
    );
}

/// A directory unique to one test, removed when it drops.
struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("endora-snapshots-{tag}-{}", std::process::id()));
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

fn a_database(at: &std::path::Path) {
    let conn = rusqlite::Connection::open(at).unwrap();
    conn.execute_batch("CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (42);")
        .unwrap();
}

#[test]
fn tonights_copy_is_written_once_checked_and_old_ones_pruned() {
    let scratch = Scratch::new("write");
    let db = scratch.0.join("endora.db");
    a_database(&db);
    let dir = scratch.0.join("backups");
    std::fs::create_dir_all(&dir).unwrap();
    for d in 1..=7 {
        std::fs::write(dir.join(format!("daily-2026-10-{d:02}.db")), b"old").unwrap();
    }
    let at = SUNDAY_0500_UTC + 30 * MINUTE;
    let wrote = copy_tonight(&db, &dir, at, -240)
        .unwrap()
        .expect("due, so written");
    assert!(wrote > 0);
    for name in ["daily-2026-10-18.db", "weekly-2026-10-18.db"] {
        let copy = rusqlite::Connection::open(dir.join(name)).unwrap();
        let x: i64 = copy.query_row("SELECT x FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(x, 42, "{name} holds the data");
    }
    assert!(
        !dir.join("daily-2026-10-01.db").exists(),
        "the eighth daily is pruned"
    );
    assert!(dir.join("daily-2026-10-02.db").exists());
    // Already written tonight: nothing more to do.
    assert_eq!(copy_tonight(&db, &dir, at + HOUR, -240).unwrap(), None);
    // Not due yet: nothing written.
    let early = Scratch::new("early");
    let early_dir = early.0.join("backups");
    assert_eq!(
        copy_tonight(&db, &early_dir, SUNDAY_0500_UTC, -240).unwrap(),
        None
    );
    assert!(!early_dir.join("daily-2026-10-18.db").exists());
}

#[test]
fn a_database_that_cannot_be_copied_is_an_error_not_silence() {
    let scratch = Scratch::new("missing");
    let dir = scratch.0.join("backups");
    let missing = scratch.0.join("no-such.db");
    assert!(copy_tonight(&missing, &dir, SUNDAY_0500_UTC + 30 * MINUTE, -240).is_err());
}
