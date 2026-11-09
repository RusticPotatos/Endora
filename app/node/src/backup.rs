//! `endora-node backup <file>` — a consistent copy of the database, taken while the node
//! keeps running.
//!
//! Run it inside the container (`make backup` does) so it sees the same `ENDORA_DB` the
//! node does. It exists because the settings, keys and sign-in secrets live in that one
//! SQLite file, and `endora export` covers memory only (messages, beliefs, research…) — not
//! the configuration — so it cannot stand in for a backup of the node.
//!
//! `endora-node verify-backup <copy> [<live-database>]` runs the nightly's restore proof on a
//! copy: it restores it to a scratch file and exits 0 only if it is whole, has the live database's
//! schema and holds rows wherever the live database does. It works on a node copy and on an
//! `hq.db` copy (name HQ's live file as the second argument), so HQ needs no binary of its own.
//! It never starts a server.
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
    match args.first().map(String::as_str) {
        Some("backup") => Some(match args {
            [_, destination] => run(database, destination),
            _ => Err("usage: endora-node backup <destination-file>".into()),
        }),
        // Also not a fall-through when mistyped: it must never start a second server.
        Some("verify-backup") => Some(match args {
            [_, copy] => verify(database, copy),
            [_, copy, live] => verify(live, copy),
            _ => Err("usage: endora-node verify-backup <copy> [<live-database>]".into()),
        }),
        _ => None,
    }
}

/// Proves `copy` restores `live`, as the nightly does.
fn verify(live: &str, copy: &str) -> Result<(), Box<dyn Error>> {
    endora_persistence::prove_restore(Path::new(copy), Path::new(live))?;
    println!("{copy} restores: it is whole, has {live}'s schema and holds rows wherever it does.");
    Ok(())
}

fn run(database: &str, destination: &str) -> Result<(), Box<dyn Error>> {
    endora_persistence::snapshot(Path::new(database), Path::new(destination))?;
    let bytes = std::fs::metadata(destination)?.len();
    println!("Snapshot written to {destination} ({bytes} bytes) and integrity-checked.");
    println!(
        "Its credentials are sealed under the secrets key (ADR 0125), which is not in it: keep \
         the snapshot private, and keep the key's copy apart from it."
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

    #[test]
    fn verify_backup_with_the_wrong_arguments_is_an_error_and_never_starts_a_server() {
        for words in [
            &["verify-backup"][..],
            &["verify-backup", "a", "b", "c"][..],
        ] {
            let outcome = dispatch(&args(words), "x.db").expect("handled, not passed on");
            let message = outcome.expect_err("a usage error").to_string();
            assert!(message.contains("usage"), "{message}");
        }
    }

    /// A node database and a copy of it as the nightly makes it.
    fn live_and_copy(scratch: &Scratch) -> (String, String) {
        let database = scratch.path("endora.db");
        let db = endora_persistence::Db::open(&database).expect("a database");
        db.lock()
            .expect("lock")
            .execute_batch("CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('John Doe');")
            .expect("seed");
        let copy = scratch.path("backups/daily-2026-10-18.db");
        dispatch(&args(&["backup", &copy]), &database)
            .expect("handled")
            .expect("a snapshot");
        (database, copy)
    }

    #[test]
    fn verify_backup_passes_a_good_copy() {
        let scratch = Scratch::new("verify-good");
        let (database, copy) = live_and_copy(&scratch);
        dispatch(&args(&["verify-backup", &copy]), &database)
            .expect("handled")
            .expect("a good copy proves");
    }

    #[test]
    fn verify_backup_fails_a_truncated_copy_and_a_missing_one() {
        let scratch = Scratch::new("verify-bad");
        let (database, copy) = live_and_copy(&scratch);
        let bytes = std::fs::read(&copy).expect("the copy");
        std::fs::write(&copy, &bytes[..bytes.len() / 2]).expect("truncate");
        let outcome = dispatch(&args(&["verify-backup", &copy]), &database).expect("handled");
        assert!(outcome.is_err(), "a truncated copy does not prove");
        let missing = scratch.path("backups/nothing.db");
        let outcome = dispatch(&args(&["verify-backup", &missing]), &database).expect("handled");
        assert!(outcome.is_err(), "a missing copy does not prove");
    }

    #[test]
    fn verify_backup_names_the_live_database_for_an_hq_copy() {
        let scratch = Scratch::new("verify-hq");
        // HQ's own file, not the node's: the copy is compared with the live file named.
        let hq = scratch.path("hq.db");
        let db = endora_persistence::Db::open(&hq).expect("a database");
        db.lock()
            .expect("lock")
            .execute_batch(
                "CREATE TABLE goals (title TEXT); INSERT INTO goals VALUES ('Water the plants');",
            )
            .expect("seed");
        let copy = scratch.path("backups/daily-2026-10-18.db");
        endora_persistence::snapshot(std::path::Path::new(&hq), std::path::Path::new(&copy))
            .expect("a copy");
        // Against the node's ENDORA_DB (which does not exist) it cannot prove; against HQ's it does.
        assert!(
            dispatch(&args(&["verify-backup", &copy]), &scratch.path("endora.db"))
                .expect("handled")
                .is_err()
        );
        dispatch(
            &args(&["verify-backup", &copy, &hq]),
            &scratch.path("endora.db"),
        )
        .expect("handled")
        .expect("an hq copy proves against hq.db");
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
