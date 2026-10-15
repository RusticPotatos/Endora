//! `endora-node backup <file>` — a consistent copy of the database, taken while the node
//! keeps running.
//!
//! Run it inside the container (`make backup` does) so it sees the same `ENDORA_DB` the
//! node does. It exists because the settings, keys and sign-in secrets live in that one
//! SQLite file, and `endora export` covers memory only (messages, beliefs, goals…) — not
//! the configuration — so it cannot stand in for a backup of the node.
//!
//! The copy goes through SQLite ([`endora_persistence::snapshot`]) rather than a file
//! copy, because the node writes continuously and a copy that lands mid-write can look
//! whole and fail to open on the day it is needed.

use std::error::Error;
use std::path::Path;

/// Handles `backup <destination>` if that is what `args` (the arguments after the program
/// name) say, returning `None` for anything else so startup carries on as normal.
///
/// A `backup` with the wrong number of arguments is an error and **not** a fall-through:
/// `endora-node backup` typed inside the running container must not start a second
/// server against the same database.
pub fn dispatch(args: &[String], database: &str) -> Option<Result<(), Box<dyn Error>>> {
    if args.first().map(String::as_str) != Some("backup") {
        return None;
    }
    Some(match args {
        [_, destination] => run(database, destination),
        _ => Err("usage: endora-node backup <destination-file>".into()),
    })
}

fn run(database: &str, destination: &str) -> Result<(), Box<dyn Error>> {
    endora_persistence::snapshot(Path::new(database), Path::new(destination))?;
    let bytes = std::fs::metadata(destination)?.len();
    println!("Snapshot written to {destination} ({bytes} bytes) and integrity-checked.");
    println!(
        "It contains the node's credentials (API keys, tokens, sign-in secrets): keep it private."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::dispatch;
    use std::path::PathBuf;

    /// A directory unique to one test, removed when it drops.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("endora-node-backup-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a scratch directory");
            Self(dir)
        }
        fn path(&self, name: &str) -> String {
            self.0.join(name).to_string_lossy().into_owned()
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn anything_that_is_not_backup_is_left_for_startup() {
        assert!(
            dispatch(&args(&[]), "x.db").is_none(),
            "no arguments is the server"
        );
        assert!(dispatch(&args(&["serve"]), "x.db").is_none());
        assert!(
            dispatch(&args(&["not-backup", "dest"]), "x.db").is_none(),
            "only the first word selects the command"
        );
    }

    #[test]
    fn backup_without_a_destination_is_an_error_and_never_starts_a_server() {
        // The case this exists for. Falling through here would launch a second node
        // inside the running container, against the very database being backed up.
        let outcome = dispatch(&args(&["backup"]), "x.db").expect("handled, not passed on");
        let message = outcome.expect_err("a usage error").to_string();
        assert!(message.contains("usage"), "{message}");

        assert!(
            dispatch(&args(&["backup", "a", "b"]), "x.db")
                .expect("handled")
                .is_err(),
            "extra arguments are refused too"
        );
    }

    #[test]
    fn backup_writes_a_snapshot_of_the_database() {
        let scratch = Scratch::new("writes");
        let database = scratch.path("endora.db");
        let db = endora_persistence::Db::open(&database).expect("a database");
        db.lock()
            .expect("lock")
            .execute_batch("CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('kept');")
            .expect("seed");
        let destination = scratch.path("backups/snap.db");

        dispatch(&args(&["backup", &destination]), &database)
            .expect("handled")
            .expect("a snapshot");

        let copy = rusqlite_free_count(&destination);
        assert_eq!(copy, 1, "the snapshot holds the row the database held");
    }

    #[test]
    fn a_wrong_database_path_fails_loudly_and_makes_no_file() {
        let scratch = Scratch::new("missing");
        let destination = scratch.path("snap.db");

        let outcome =
            dispatch(&args(&["backup", &destination]), &scratch.path("nope.db")).expect("handled");

        assert!(outcome.is_err());
        assert!(!std::path::Path::new(&destination).exists());
    }

    /// Reads the row count back through the persistence crate's own handle, so this test
    /// needs no direct `rusqlite` dependency in the node.
    fn rusqlite_free_count(path: &str) -> i64 {
        let db = endora_persistence::Db::open(path).expect("opens the snapshot");
        let conn = db.lock().expect("lock");
        conn.query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0))
            .expect("the table survived")
    }
}
