//! Secrets at rest (ADR 0125). Every secret here is an obvious placeholder.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{KeyFile, MemoryKey, SEALED, SecretColumn};
use crate::Db;

const PLACEHOLDER: &str = "placeholder-secret-not-real-0001";

const COLUMNS: &[SecretColumn] = &[
    SecretColumn {
        table: "vault",
        column: "api_key",
    },
    SecretColumn {
        table: "vault",
        column: "bearer",
    },
];

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "endora-secrets-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        Self(dir)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A database on disk with its key file beside it, the way the node opens one.
fn on_disk(scratch: &Scratch) -> Db {
    let path = scratch.path("node.db");
    let db = Db::open(path.to_str().expect("utf-8")).expect("opens");
    let key = KeyFile::open(
        &scratch.path("secrets.key"),
        !db.has_data_keys().expect("reads"),
    )
    .expect("a key file");
    db.attach_key_source(Arc::new(key)).expect("attaches");
    db
}

/// The table as an older node left it: plaintext credentials.
fn with_plaintext(db: &Db, rows: &[(&str, &str)]) {
    let conn = db.lock().expect("lock");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vault (name TEXT PRIMARY KEY, api_key TEXT NOT NULL, \
         bearer TEXT NOT NULL DEFAULT '') STRICT;",
    )
    .expect("schema");
    for (i, (key, bearer)) in rows.iter().enumerate() {
        conn.execute(
            "INSERT INTO vault (name, api_key, bearer) VALUES (?1, ?2, ?3)",
            rusqlite::params![format!("row{i}"), key, bearer],
        )
        .expect("insert");
    }
}

fn stored(db: &Db, column: &str) -> Vec<String> {
    let conn = db.lock().expect("lock");
    let mut stmt = conn
        .prepare(&format!("SELECT {column} FROM vault ORDER BY name"))
        .expect("select");
    stmt.query_map([], |r| r.get(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("values")
}

fn file_holds(path: &Path, needle: &str) -> bool {
    let bytes = std::fs::read(path).expect("the file");
    bytes
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[test]
fn a_sealed_value_opens_to_what_was_sealed_and_shows_nothing_of_it() {
    let db = Db::open_in_memory().unwrap();
    let sealed = db.seal(PLACEHOLDER).unwrap();
    assert!(sealed.starts_with(SEALED), "{sealed}");
    assert_eq!(
        sealed.split(':').count(),
        5,
        "enc:v1:<kid>:<nonce>:<ct>, got {sealed}"
    );
    assert!(!sealed.contains(PLACEHOLDER));
    assert_ne!(
        db.seal(PLACEHOLDER).unwrap(),
        sealed,
        "a fresh nonce every time"
    );
    assert_eq!(db.unseal("vault.api_key", &sealed).unwrap(), PLACEHOLDER);
}

#[test]
fn nothing_stays_nothing() {
    // An empty credential means "none" all over the node (`api_key = ''`); sealing it would
    // turn "none" into something.
    let db = Db::open_in_memory().unwrap();
    assert_eq!(db.seal("").unwrap(), "");
    assert_eq!(db.unseal("vault.api_key", "").unwrap(), "");
}

#[test]
fn a_plaintext_value_is_refused_by_column_and_never_repeated() {
    let db = Db::open_in_memory().unwrap();
    let refused = db
        .unseal("vault.api_key", PLACEHOLDER)
        .expect_err("plaintext where a sealed value belongs");
    let message = refused.to_string();
    assert!(message.contains("vault.api_key"), "{message}");
    assert!(!message.contains(PLACEHOLDER), "{message}");
}

#[test]
fn an_altered_value_does_not_open_and_the_error_carries_nothing_of_it() {
    let db = Db::open_in_memory().unwrap();
    let sealed = db.seal(PLACEHOLDER).unwrap();
    let mut altered = sealed.into_bytes();
    let last = altered.len() - 2;
    altered[last] = if altered[last] == b'A' { b'B' } else { b'A' };
    let altered = String::from_utf8(altered).unwrap();
    let message = db
        .unseal("vault.api_key", &altered)
        .unwrap_err()
        .to_string();
    assert!(!message.contains(PLACEHOLDER), "{message}");

    // Nor does a value sealed by another database's key.
    let other = Db::open_in_memory().unwrap().seal(PLACEHOLDER).unwrap();
    let message = db.unseal("vault.api_key", &other).unwrap_err().to_string();
    assert!(!message.contains(PLACEHOLDER), "{message}");
}

#[test]
fn a_database_on_disk_keeps_no_secret_until_a_key_source_is_attached() {
    let scratch = Scratch::new();
    let db = Db::open(scratch.path("node.db").to_str().unwrap()).unwrap();
    assert!(db.seal(PLACEHOLDER).is_err(), "never sealed with no key");
    assert!(!db.has_data_keys().unwrap());
}

#[cfg(unix)]
#[test]
fn the_key_file_is_made_owner_only_and_the_database_too() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let _db = on_disk(&scratch);
    for name in ["secrets.key", "node.db"] {
        let mode = std::fs::metadata(scratch.path(name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{name} was {:o}", mode & 0o777);
    }
    assert_eq!(
        std::fs::read(scratch.path("secrets.key")).unwrap().len(),
        32
    );
}

#[cfg(unix)]
#[test]
fn a_key_file_others_can_read_is_made_owner_only_again() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    drop(on_disk(&scratch));
    let key = scratch.path("secrets.key");
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
    drop(on_disk(&scratch));
    let mode = std::fs::metadata(&key).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_restart_with_the_same_key_file_opens_what_was_sealed() {
    let scratch = Scratch::new();
    let sealed = on_disk(&scratch).seal(PLACEHOLDER).unwrap();
    let reopened = on_disk(&scratch);
    assert_eq!(
        reopened.unseal("vault.api_key", &sealed).unwrap(),
        PLACEHOLDER
    );
}

#[test]
fn a_key_kept_as_base64_text_is_the_same_key() {
    // What a copy kept in a password manager comes back as.
    use base64::Engine as _;
    let scratch = Scratch::new();
    let sealed = on_disk(&scratch).seal(PLACEHOLDER).unwrap();
    let raw = std::fs::read(scratch.path("secrets.key")).unwrap();
    let text = base64::engine::general_purpose::STANDARD.encode(raw);
    std::fs::write(scratch.path("secrets.key"), format!("{text}\n")).unwrap();
    assert_eq!(
        on_disk(&scratch).unseal("vault.api_key", &sealed).unwrap(),
        PLACEHOLDER
    );
}

#[test]
fn a_missing_key_file_over_sealed_secrets_is_an_error_not_a_new_key() {
    let scratch = Scratch::new();
    drop(on_disk(&scratch));
    std::fs::remove_file(scratch.path("secrets.key")).unwrap();
    let db = Db::open(scratch.path("node.db").to_str().unwrap()).unwrap();
    assert!(db.has_data_keys().unwrap());
    let refused = KeyFile::open(&scratch.path("secrets.key"), false);
    assert!(refused.is_err());
    assert!(
        !scratch.path("secrets.key").exists(),
        "no fresh key was made over sealed secrets"
    );
}

#[test]
fn the_wrong_key_file_does_not_open_the_data_keys() {
    let scratch = Scratch::new();
    drop(on_disk(&scratch));
    std::fs::write(scratch.path("secrets.key"), [7u8; 32]).unwrap();
    let db = Db::open(scratch.path("node.db").to_str().unwrap()).unwrap();
    let key = KeyFile::open(&scratch.path("secrets.key"), false).unwrap();
    assert!(db.attach_key_source(Arc::new(key)).is_err());
}

#[test]
fn a_key_file_that_is_not_a_key_is_refused() {
    let scratch = Scratch::new();
    std::fs::write(scratch.path("secrets.key"), b"too short").unwrap();
    assert!(KeyFile::open(&scratch.path("secrets.key"), true).is_err());
}

#[test]
fn plaintext_left_by_an_older_node_is_sealed_once_and_leaves_no_trace_in_the_file() {
    let scratch = Scratch::new();
    let db = on_disk(&scratch);
    with_plaintext(&db, &[(PLACEHOLDER, ""), ("", "placeholder-bearer-0002")]);

    assert_eq!(db.seal_plaintext(COLUMNS).unwrap(), 2);

    let keys = stored(&db, "api_key");
    assert!(keys[0].starts_with(SEALED), "{}", keys[0]);
    assert_eq!(keys[1], "", "an empty credential stays empty");
    assert_eq!(db.unseal("vault.api_key", &keys[0]).unwrap(), PLACEHOLDER);
    assert_eq!(
        db.unseal("vault.bearer", &stored(&db, "bearer")[1])
            .unwrap(),
        "placeholder-bearer-0002"
    );

    // Idempotent: a second start seals nothing and changes nothing.
    assert_eq!(db.seal_plaintext(COLUMNS).unwrap(), 0);
    assert_eq!(stored(&db, "api_key"), keys);

    // And the plaintext is gone from the file itself, not only from the rows.
    drop(db);
    for needle in [PLACEHOLDER, "placeholder-bearer-0002"] {
        assert!(
            !file_holds(&scratch.path("node.db"), needle),
            "{needle} survived in the database file"
        );
    }
}

#[test]
fn rekeying_seals_everything_under_a_new_key_and_forgets_the_old_one() {
    let scratch = Scratch::new();
    let db = on_disk(&scratch);
    with_plaintext(&db, &[(PLACEHOLDER, "placeholder-bearer-0002")]);
    db.seal_plaintext(COLUMNS).unwrap();
    let before = stored(&db, "api_key")[0].clone();
    let old_kid = before.split(':').nth(2).unwrap().to_owned();

    let new_kid = db.rekey(COLUMNS).unwrap();

    assert_ne!(new_kid, old_kid);
    let after = stored(&db, "api_key")[0].clone();
    assert_eq!(after.split(':').nth(2).unwrap(), new_kid);
    assert_eq!(db.unseal("vault.api_key", &after).unwrap(), PLACEHOLDER);
    assert!(
        db.unseal("vault.api_key", &before).is_err(),
        "the old data key is gone"
    );
    let kids: Vec<String> = {
        let conn = db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT kid FROM secret_keys").unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(kids, vec![new_kid.clone()]);
    // New values are sealed under the new key, and a restart still opens everything.
    assert!(db.seal("x").unwrap().contains(&new_kid));
    drop(db);
    let reopened = on_disk(&scratch);
    assert_eq!(
        reopened
            .unseal("vault.bearer", &stored(&reopened, "bearer")[0])
            .unwrap(),
        "placeholder-bearer-0002"
    );
}

#[test]
fn moving_to_another_key_source_rewraps_the_keys_and_touches_no_value() {
    // The path to a Transit engine: the values are sealed by the data key, so only the data
    // keys' wrapping changes.
    let scratch = Scratch::new();
    let db = on_disk(&scratch);
    with_plaintext(&db, &[(PLACEHOLDER, "")]);
    db.seal_plaintext(COLUMNS).unwrap();
    let sealed = stored(&db, "api_key");

    let new_file = scratch.path("secrets-2.key");
    db.rewrap(Arc::new(KeyFile::open(&new_file, true).unwrap()))
        .unwrap();

    assert_eq!(stored(&db, "api_key"), sealed, "no value was rewritten");
    drop(db);
    let db = Db::open(scratch.path("node.db").to_str().unwrap()).unwrap();
    let old = KeyFile::open(&scratch.path("secrets.key"), false).unwrap();
    assert!(
        db.attach_key_source(Arc::new(old)).is_err(),
        "the old key no longer opens them"
    );
    db.attach_key_source(Arc::new(KeyFile::open(&new_file, false).unwrap()))
        .unwrap();
    assert_eq!(db.unseal("vault.api_key", &sealed[0]).unwrap(), PLACEHOLDER);
}

#[test]
fn a_data_key_wrapped_by_one_source_is_never_offered_to_another() {
    let scratch = Scratch::new();
    drop(on_disk(&scratch));
    let db = Db::open(scratch.path("node.db").to_str().unwrap()).unwrap();
    let message = db
        .attach_key_source(Arc::new(MemoryKey::random()))
        .unwrap_err()
        .to_string();
    assert!(message.contains("'file'"), "{message}");
}

#[test]
fn forgetting_empties_the_sealed_values_and_lets_a_new_key_start_over() {
    let scratch = Scratch::new();
    let db = on_disk(&scratch);
    with_plaintext(&db, &[(PLACEHOLDER, "")]);
    db.seal_plaintext(COLUMNS).unwrap();
    drop(db);
    std::fs::remove_file(scratch.path("secrets.key")).unwrap();

    let db = Db::open(scratch.path("node.db").to_str().unwrap()).unwrap();
    assert_eq!(db.forget_sealed(COLUMNS).unwrap(), 1);
    assert_eq!(stored(&db, "api_key"), vec![String::new()]);
    assert!(!db.has_data_keys().unwrap());
    drop(db);
    let db = on_disk(&scratch);
    assert!(db.seal(PLACEHOLDER).is_ok(), "a new key, and secrets again");
}

// OpenBao and the passkey unlock (ADR 0127): a database whose data keys a sealed vault wraps
// opens *waiting*, says so when a secret is asked of it, and opens fully once a source is
// attached.

#[test]
fn the_sources_that_wrapped_the_keys_are_named_without_opening_them() {
    let scratch = Scratch::new();
    let fresh = Db::open(scratch.path("fresh.db").to_str().unwrap()).unwrap();
    assert!(
        fresh.key_sources_on_disk().unwrap().is_empty(),
        "no keys yet"
    );
    drop(on_disk(&scratch));
    let db = Db::open(scratch.path("node.db").to_str().unwrap()).unwrap();
    assert_eq!(db.key_sources_on_disk().unwrap(), vec!["file".to_owned()]);
}

#[test]
fn a_database_waiting_for_the_vault_says_so_and_opens_once_a_source_is_attached() {
    let scratch = Scratch::new();
    let db = on_disk(&scratch);
    let sealed = db.seal(PLACEHOLDER).unwrap();
    drop(db);

    let db = Db::open(scratch.path("node.db").to_str().unwrap()).unwrap();
    db.wait_for_the_vault();
    assert!(!db.has_key_source());
    let message = db.unseal("vault.api_key", &sealed).unwrap_err().to_string();
    assert!(message.contains("vault is locked"), "{message}");
    assert!(!message.contains(PLACEHOLDER));
    assert!(
        db.seal("x")
            .unwrap_err()
            .to_string()
            .contains("vault is locked")
    );

    db.attach_key_source(Arc::new(
        KeyFile::open(&scratch.path("secrets.key"), false).unwrap(),
    ))
    .unwrap();
    assert!(db.has_key_source());
    assert_eq!(db.unseal("vault.api_key", &sealed).unwrap(), PLACEHOLDER);
}

#[test]
fn a_note_sealed_by_the_key_file_opens_only_for_its_purpose_and_that_key() {
    let scratch = Scratch::new();
    let key = KeyFile::open(&scratch.path("secrets.key"), true).unwrap();
    let note = key.seal_note("vault-approle", PLACEHOLDER).unwrap();
    assert!(note.starts_with("file:note:v1:"), "{note}");
    assert!(!note.contains(PLACEHOLDER));
    assert_eq!(
        &*key.open_note("vault-approle", &note).unwrap(),
        PLACEHOLDER
    );
    assert!(
        key.open_note("something-else", &note).is_err(),
        "bound to its purpose"
    );
    let other = KeyFile::open(&scratch.path("other.key"), true).unwrap();
    let message = other
        .open_note("vault-approle", &note)
        .unwrap_err()
        .to_string();
    assert!(!message.contains(PLACEHOLDER), "{message}");
}
