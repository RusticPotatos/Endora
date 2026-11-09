use super::*;
use std::cell::Cell;
use std::path::{Path, PathBuf};

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
    // brain (ADR 0136).
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

const DUE: i64 = SUNDAY_0500_UTC + 30 * MINUTE;

/// The real copy: what the nightly does, so a test can count its calls.
fn real_copy(from: &Path, to: &Path) -> Result<(), String> {
    crate::snapshot(from, to).map_err(|e| e.to_string())
}

/// A copy that comes out cut in half, as a disk that filled up would leave it.
fn truncating_copy(from: &Path, to: &Path) -> Result<(), String> {
    real_copy(from, to)?;
    let bytes = std::fs::read(to).map_err(|e| e.to_string())?;
    std::fs::write(to, &bytes[..bytes.len() / 2]).map_err(|e| e.to_string())
}

fn real_proof(copy: &Path, live: &Path) -> Result<(), String> {
    crate::prove_restore(copy, live).map_err(|e| e.to_string())
}

/// A larger database than the one-row `a_database`, with rows in two tables.
fn a_live_database(at: &Path) {
    let conn = rusqlite::Connection::open(at).unwrap();
    conn.execute_batch(
        "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
         CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL);",
    )
    .unwrap();
    conn.execute_batch("BEGIN").unwrap();
    for i in 0..300 {
        conn.execute(
            "INSERT INTO people (name) VALUES (?1)",
            [format!("John Doe {i}")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO notes (body) VALUES (?1)",
            [format!("Jane Doe note {i:0>60}")],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
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

/// A live database and its `backups/`, ready for a night's ticks.
fn a_house(tag: &str) -> (Scratch, PathBuf, PathBuf) {
    let scratch = Scratch::new(tag);
    let db = scratch.0.join("endora.db");
    a_live_database(&db);
    let dir = scratch.0.join("backups");
    (scratch, db, dir)
}

fn failed(tick: &Tick) -> bool {
    matches!(tick, Tick::Failed { .. })
}

/// Case 1: a good database is copied, the copy is proven by restore, and only then is it
/// named as a daily.
#[test]
fn tonights_copy_is_proven_then_kept_and_old_ones_pruned() {
    let (_scratch, db, dir) = a_house("write");
    std::fs::create_dir_all(&dir).unwrap();
    for d in 1..=7 {
        std::fs::write(dir.join(format!("daily-2026-10-{d:02}.db")), b"old").unwrap();
    }
    let proofs = Cell::new(0);
    let counting = |copy: &Path, live: &Path| {
        proofs.set(proofs.get() + 1);
        real_proof(copy, live)
    };
    let nightly = Nightly::new();
    let tick = nightly.tick_with(&db, &dir, DUE, -240, &real_copy, &counting);
    assert!(
        matches!(tick, Tick::Proven { bytes } if bytes > 0),
        "{tick:?}"
    );
    assert_eq!(proofs.get(), 1, "one proof per copy");
    for name in ["daily-2026-10-18.db", "weekly-2026-10-18.db"] {
        let copy = rusqlite::Connection::open(dir.join(name)).unwrap();
        let n: i64 = copy
            .query_row("SELECT COUNT(*) FROM people", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 300, "{name} holds the data");
    }
    assert!(!dir.join("daily-2026-10-18.db.unproven").exists());
    assert!(
        !dir.join("daily-2026-10-01.db").exists(),
        "the eighth daily is pruned"
    );
    assert!(dir.join("daily-2026-10-02.db").exists());
    // Already proven tonight: nothing more to do, and no second copy or proof.
    assert_eq!(
        nightly.tick_with(&db, &dir, DUE + HOUR, -240, &real_copy, &counting),
        Tick::Nothing
    );
    assert_eq!(proofs.get(), 1);
    // Not due yet: nothing written.
    let early = Scratch::new("early");
    let early_dir = early.0.join("backups");
    let tick = Nightly::new().tick(&db, &early_dir, SUNDAY_0500_UTC, -240);
    assert_eq!(tick, Tick::Nothing);
    assert!(!early_dir.exists());
}

#[test]
fn a_database_that_cannot_be_copied_is_a_failure_not_silence() {
    let scratch = Scratch::new("missing");
    let dir = scratch.0.join("backups");
    let tick = Nightly::new().tick(&scratch.0.join("no-such.db"), &dir, DUE, -240);
    assert!(failed(&tick), "{tick:?}");
}

/// Case 2 at the loop: a copy that fails its proof is deleted, not kept, and the next tick
/// tries again.
#[test]
fn a_copy_that_fails_its_proof_is_deleted_and_the_next_tick_tries_again() {
    let (_scratch, db, dir) = a_house("retry");
    let nightly = Nightly::new();
    let tick = nightly.tick_with(&db, &dir, DUE, -240, &truncating_copy, &real_proof);
    assert!(
        matches!(
            &tick,
            Tick::Failed {
                giving_up: false,
                ..
            }
        ),
        "{tick:?}"
    );
    assert_eq!(
        listing(&dir),
        Vec::<String>::new(),
        "the bad copy is removed"
    );
    let tick = nightly.tick_with(&db, &dir, DUE + 10 * MINUTE, -240, &real_copy, &real_proof);
    assert!(matches!(tick, Tick::Proven { .. }), "{tick:?}");
    assert_eq!(
        listing(&dir),
        names(&["daily-2026-10-18.db", "weekly-2026-10-18.db"])
    );
}

/// Case 7: three failures in a night are three attempts and one thing to tell the person,
/// then no fourth attempt; a new night starts again.
#[test]
fn three_failures_in_a_day_are_three_attempts_and_one_failure_to_report() {
    let (_scratch, db, dir) = a_house("cap");
    let copies = Cell::new(0);
    let counting = |from: &Path, to: &Path| {
        copies.set(copies.get() + 1);
        truncating_copy(from, to)
    };
    let nightly = Nightly::new();
    let ticks: Vec<Tick> = (0..6)
        .map(|n| {
            nightly.tick_with(
                &db,
                &dir,
                DUE + n * 10 * MINUTE,
                -240,
                &counting,
                &real_proof,
            )
        })
        .collect();
    let giving_up: Vec<bool> = ticks
        .iter()
        .filter_map(|t| match t {
            Tick::Failed { giving_up, .. } => Some(*giving_up),
            _ => None,
        })
        .collect();
    assert_eq!(giving_up, [false, false, true], "{ticks:?}");
    assert_eq!(&ticks[3..], [Tick::Nothing, Tick::Nothing, Tick::Nothing]);
    assert_eq!(copies.get(), 3, "exactly three attempts, no fourth");
    assert_eq!(listing(&dir), Vec::<String>::new());
    // The next night is a new day with its own three.
    let tomorrow = nightly.tick_with(&db, &dir, DUE + 24 * HOUR, -240, &counting, &real_proof);
    assert!(failed(&tomorrow), "{tomorrow:?}");
    assert_eq!(copies.get(), 4);
}

/// Case 7 again: success on the last allowed attempt reports no failure at all.
#[test]
fn a_late_success_is_a_success() {
    let (_scratch, db, dir) = a_house("late");
    let nightly = Nightly::new();
    for n in 0..2 {
        let tick = nightly.tick_with(
            &db,
            &dir,
            DUE + n * 10 * MINUTE,
            -240,
            &truncating_copy,
            &real_proof,
        );
        assert!(
            matches!(
                tick,
                Tick::Failed {
                    giving_up: false,
                    ..
                }
            ),
            "{tick:?}"
        );
    }
    let tick = nightly.tick_with(&db, &dir, DUE + 20 * MINUTE, -240, &real_copy, &real_proof);
    assert!(matches!(tick, Tick::Proven { .. }), "{tick:?}");
}

/// Case 6: the node restarted after the daily was copied and before it was proven. The
/// leftover copy is proven on the next tick: not copied again, not skipped.
#[test]
fn a_copy_left_unproven_by_a_restart_is_proven_not_recopied() {
    let (_scratch, db, dir) = a_house("restart");
    std::fs::create_dir_all(&dir).unwrap();
    // What the process left behind when it died between the copy and the proof.
    crate::snapshot(&db, &dir.join("daily-2026-10-18.db.unproven")).unwrap();
    let copies = Cell::new(0);
    let counting = |from: &Path, to: &Path| {
        copies.set(copies.get() + 1);
        real_copy(from, to)
    };
    // A new process: no memory of anything.
    let tick = Nightly::new().tick_with(&db, &dir, DUE, -240, &counting, &real_proof);
    assert!(matches!(tick, Tick::Proven { .. }), "{tick:?}");
    assert_eq!(copies.get(), 0, "not copied again");
    assert_eq!(
        listing(&dir),
        names(&["daily-2026-10-18.db", "weekly-2026-10-18.db"])
    );
}

/// Case 6, the bad half: a leftover that does not prove is removed and tonight's copy is
/// made afresh on the next tick.
#[test]
fn a_leftover_that_fails_its_proof_is_replaced_next_tick() {
    let (_scratch, db, dir) = a_house("leftover");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("daily-2026-10-18.db.unproven"), b"half a file").unwrap();
    let nightly = Nightly::new();
    let tick = nightly.tick_with(&db, &dir, DUE, -240, &real_copy, &real_proof);
    assert!(failed(&tick), "{tick:?}");
    assert_eq!(listing(&dir), Vec::<String>::new());
    let tick = nightly.tick_with(&db, &dir, DUE + 10 * MINUTE, -240, &real_copy, &real_proof);
    assert!(matches!(tick, Tick::Proven { .. }), "{tick:?}");
}

/// An unproven leftover from an earlier night is not tonight's to prove: it goes.
#[test]
fn an_unproven_leftover_from_an_earlier_night_is_removed() {
    let (_scratch, db, dir) = a_house("stale");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("daily-2026-10-16.db.unproven"), b"old").unwrap();
    Nightly::new().tick(&db, &dir, DUE, -240);
    assert_eq!(
        listing(&dir),
        names(&["daily-2026-10-18.db", "weekly-2026-10-18.db"])
    );
}

/// Case 8: eight dailies on disk and tonight's copy failing its proof. Pruning is never
/// reached, so the newest proven copy survives; when a copy does prove, the newest survives
/// the prune.
#[test]
fn the_newest_proven_copy_survives_pruning() {
    let (_scratch, db, dir) = a_house("prune");
    std::fs::create_dir_all(&dir).unwrap();
    for d in 10..=17 {
        crate::snapshot(&db, &dir.join(format!("daily-2026-10-{d}.db"))).unwrap();
    }
    let before = listing(&dir);
    assert_eq!(before.len(), 8);
    let nightly = Nightly::new();
    let tick = nightly.tick_with(&db, &dir, DUE, -240, &truncating_copy, &real_proof);
    assert!(failed(&tick), "{tick:?}");
    assert_eq!(listing(&dir), before, "a failed night prunes nothing");
    crate::prove_restore(&dir.join("daily-2026-10-17.db"), &db).unwrap();

    let tick = nightly.tick_with(&db, &dir, DUE + 10 * MINUTE, -240, &real_copy, &real_proof);
    assert!(matches!(tick, Tick::Proven { .. }), "{tick:?}");
    let after = listing(&dir);
    assert!(
        after.contains(&"daily-2026-10-18.db".to_owned()),
        "{after:?}"
    );
    assert!(
        after.contains(&"daily-2026-10-17.db".to_owned()),
        "{after:?}"
    );
    assert!(!after.contains(&"daily-2026-10-10.db".to_owned()));
    assert!(!after.contains(&"daily-2026-10-11.db".to_owned()));
    assert_eq!(after.iter().filter(|n| n.starts_with("daily-")).count(), 7);
}

/// Case 9: files the nightly did not name are left alone, on a failed night and a good one.
#[test]
fn unrelated_files_in_backups_are_untouched() {
    let (_scratch, db, dir) = a_house("unrelated");
    std::fs::create_dir_all(&dir).unwrap();
    let mine = [
        ("before-0.12.0-hq.db", &b"release"[..]),
        ("by-hand.db", b"mine"),
        ("daily-notes.txt", b"notes"),
        ("daily-2026-10-15-before-upgrade.db", b"hand-made"),
        ("brain-daily-2026-10-01.bundle", b"brain"),
    ];
    for (name, bytes) in mine {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    let nightly = Nightly::new();
    let tick = nightly.tick_with(&db, &dir, DUE, -240, &truncating_copy, &real_proof);
    assert!(failed(&tick), "{tick:?}");
    let tick = nightly.tick_with(&db, &dir, DUE + 10 * MINUTE, -240, &real_copy, &real_proof);
    assert!(matches!(tick, Tick::Proven { .. }), "{tick:?}");
    for (name, bytes) in mine {
        assert_eq!(std::fs::read(dir.join(name)).unwrap(), bytes, "{name}");
    }
}

/// Every file under `root` outside `skip`, with its bytes, as one comparable value.
fn tree(root: &Path, skip: &[&Path]) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if skip.contains(&path.as_path()) {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            found.push((path, bytes));
        }
    }
    found.sort();
    found
}

/// Case 10: the live database is byte-identical (so its SHA-256 is too) after a good night
/// and a failed one, and nothing outside `backups/` and the scratch folder changes; the
/// scratch folder is empty again. The persistence crate has no network or model crate to call.
#[test]
fn the_live_database_and_everything_else_is_unchanged() {
    let (scratch, db, dir) = a_house("authority");
    let work = scratch.0.join("scratch");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(scratch.0.join("neighbour.txt"), b"not the nightly's").unwrap();
    let live_before = std::fs::read(&db).unwrap();
    let modified = std::fs::metadata(&db).unwrap().modified().unwrap();
    let outside = tree(&scratch.0, &[&dir, &work]);
    let in_scratch = |copy: &Path, live: &Path| {
        crate::restore::prove_restore_in(copy, live, &work).map_err(|e| e.to_string())
    };
    let nightly = Nightly::new();
    let tick = nightly.tick_with(&db, &dir, DUE, -240, &truncating_copy, &in_scratch);
    assert!(failed(&tick), "{tick:?}");
    let tick = nightly.tick_with(&db, &dir, DUE + 10 * MINUTE, -240, &real_copy, &in_scratch);
    assert!(matches!(tick, Tick::Proven { .. }), "{tick:?}");
    assert_eq!(
        std::fs::read(&db).unwrap(),
        live_before,
        "live bytes unchanged"
    );
    assert_eq!(
        std::fs::metadata(&db).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(
        tree(&scratch.0, &[&dir, &work]),
        outside,
        "nothing else changed"
    );
    assert_eq!(
        listing(&work),
        Vec::<String>::new(),
        "the scratch folder is emptied"
    );

    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let dependencies = manifest.split("[lints]").next().unwrap();
    for client in [
        "reqwest",
        "hyper",
        "ureq",
        "tokio",
        "curl",
        "isahc",
        "attohttpc",
    ] {
        assert!(
            !dependencies.contains(client),
            "persistence depends on {client}"
        );
    }
    assert!(!dependencies.contains("endora-ai") && !dependencies.contains("endora-team"));
}
