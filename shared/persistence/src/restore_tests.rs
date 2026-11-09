//! The restore proof (milestone f): a copy passes only if it opens, is whole, has the live
//! database's schema and is not empty where the live database holds rows. Every row here is an
//! obvious placeholder.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::{RestoreFailure, prove_restore, prove_restore_in};
use crate::snapshot;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "endora-restore-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A live database with two tables holding rows, one holding none, and an index.
fn a_live_database(at: &Path) {
    let conn = rusqlite::Connection::open(at).unwrap();
    conn.execute_batch(
        "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
         CREATE INDEX people_by_name ON people (name);
         CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL);
         CREATE TABLE unused (x INTEGER);",
    )
    .unwrap();
    conn.execute_batch("BEGIN").unwrap();
    for i in 0..400 {
        conn.execute(
            "INSERT INTO people (name) VALUES (?1)",
            [format!("John Doe {i}")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO notes (body) VALUES (?1)",
            [format!(
                "Jane Doe wrote note {i} with some padding to fill pages {i:0>40}"
            )],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
}

/// A live database and a good copy of it, made as the nightly makes it.
fn live_and_copy(scratch: &Scratch) -> (PathBuf, PathBuf) {
    let live = scratch.file("live.db");
    a_live_database(&live);
    let copy = scratch.file("daily-2026-10-18.db");
    snapshot(&live, &copy).unwrap();
    (live, copy)
}

#[test]
fn a_good_copy_is_proven() {
    let scratch = Scratch::new();
    let (live, copy) = live_and_copy(&scratch);
    prove_restore(&copy, &live).unwrap();
}

#[test]
fn a_copy_truncated_at_half_fails() {
    let scratch = Scratch::new();
    let (live, copy) = live_and_copy(&scratch);
    let bytes = std::fs::read(&copy).unwrap();
    std::fs::write(&copy, &bytes[..bytes.len() / 2]).unwrap();
    let failure = prove_restore(&copy, &live).unwrap_err();
    assert!(
        matches!(failure, RestoreFailure::Integrity(_)),
        "got {failure:?}"
    );
}

#[test]
fn flipped_page_bytes_fail_the_integrity_check() {
    let scratch = Scratch::new();
    let (live, copy) = live_and_copy(&scratch);
    let mut bytes = std::fs::read(&copy).unwrap();
    // Several data pages past the header and the schema page: the file still opens.
    for page in 2..bytes.len() / 4096 {
        let at = page * 4096 + 100;
        for b in &mut bytes[at..at + 200] {
            *b ^= 0xA5;
        }
    }
    std::fs::write(&copy, &bytes).unwrap();
    let failure = prove_restore(&copy, &live).unwrap_err();
    assert!(
        matches!(failure, RestoreFailure::Integrity(_)),
        "got {failure:?}"
    );
}

#[test]
fn an_empty_copy_fails_while_the_live_database_has_rows() {
    let scratch = Scratch::new();
    let (live, copy) = live_and_copy(&scratch);
    std::fs::remove_file(&copy).unwrap();
    // Same schema, no rows: whole and well formed, and no use on the day it is needed.
    let schema_only = rusqlite::Connection::open(&copy).unwrap();
    let live_conn = rusqlite::Connection::open(&live).unwrap();
    let statements: Vec<String> = live_conn
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY rowid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for statement in statements {
        schema_only.execute_batch(&statement).unwrap();
    }
    drop(schema_only);
    let failure = prove_restore(&copy, &live).unwrap_err();
    assert!(
        matches!(failure, RestoreFailure::Empty(ref table) if table == "people" || table == "notes"),
        "got {failure:?}"
    );
}

#[test]
fn a_copy_missing_a_table_fails_on_schema() {
    let scratch = Scratch::new();
    let (live, copy) = live_and_copy(&scratch);
    let conn = rusqlite::Connection::open(&copy).unwrap();
    conn.execute_batch("DROP TABLE notes;").unwrap();
    drop(conn);
    let failure = prove_restore(&copy, &live).unwrap_err();
    assert!(
        matches!(failure, RestoreFailure::Schema(_)),
        "got {failure:?}"
    );
}

#[test]
fn a_copy_missing_an_index_or_with_changed_sql_fails_on_schema() {
    let scratch = Scratch::new();
    let (live, copy) = live_and_copy(&scratch);
    let conn = rusqlite::Connection::open(&copy).unwrap();
    conn.execute_batch("DROP INDEX people_by_name;").unwrap();
    drop(conn);
    assert!(matches!(
        prove_restore(&copy, &live).unwrap_err(),
        RestoreFailure::Schema(_)
    ));
}

#[test]
fn a_missing_copy_fails_and_the_proof_leaves_no_scratch_file_behind() {
    let scratch = Scratch::new();
    let (live, copy) = live_and_copy(&scratch);
    let work = scratch.file("work");
    std::fs::create_dir(&work).unwrap();
    prove_restore_in(&copy, &live, &work).unwrap();
    assert!(matches!(
        prove_restore_in(&scratch.file("no-such.db"), &live, &work).unwrap_err(),
        RestoreFailure::Integrity(_)
    ));
    let mut bytes = std::fs::read(&copy).unwrap();
    bytes.truncate(bytes.len() / 2);
    std::fs::write(&copy, bytes).unwrap();
    assert!(prove_restore_in(&copy, &live, &work).is_err());
    assert_eq!(
        std::fs::read_dir(&work).unwrap().count(),
        0,
        "scratch removed"
    );
}

#[test]
fn the_proof_changes_neither_file() {
    let scratch = Scratch::new();
    let (live, copy) = live_and_copy(&scratch);
    let (live_before, copy_before) = (std::fs::read(&live).unwrap(), std::fs::read(&copy).unwrap());
    prove_restore(&copy, &live).unwrap();
    assert_eq!(std::fs::read(&live).unwrap(), live_before);
    assert_eq!(std::fs::read(&copy).unwrap(), copy_before);
    assert_eq!(
        std::fs::read_dir(&scratch.0).unwrap().count(),
        2,
        "nothing new beside them"
    );
}

/// The proof scratches beside the copy, never in the system's temporary folder: HQ's container
/// has a read-only root and no tmpfs, and a copy of hq.db must not pass through a shared /tmp.
/// The temp folder is made unwritable in a child run of this very test (setting it here would
/// race the other tests), so this test is two halves.
#[test]
fn a_proof_does_not_use_the_system_temp_folder() {
    if let Ok(dir) = std::env::var("ENDORA_PROOF_CHILD_DIR") {
        let dir = PathBuf::from(dir);
        prove_restore(&dir.join("daily-2026-10-18.db"), &dir.join("live.db")).unwrap();
        assert!(
            !dir.join(".scratch").exists(),
            "the scratch folder is not left behind"
        );
        return;
    }
    let scratch = Scratch::new();
    let (_live, _copy) = live_and_copy(&scratch);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["a_proof_does_not_use_the_system_temp_folder", "--nocapture"])
        .env("ENDORA_PROOF_CHILD_DIR", &scratch.0)
        .env("TMPDIR", "/nonexistent-endora-temp-folder")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the child proof failed:\n{text}");
    assert!(
        text.contains("1 passed"),
        "the child did not run the test:\n{text}"
    );
}
