//! The owner's scenarios for Endora keeping its own secrets (`app/node/features/secrets/`,
//! ADR 0144): the node opens its store as it does at start, with no vault anywhere, and leaves
//! what an older Endora kept for the vault untouched.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use cucumber::{World, given, then, when};
use endora_capabilities::{CapabilitySettingsRepository as _, ConfigStore};
use endora_infrastructure::SqliteStore;

/// The two tables an older Endora kept about OpenBao, as it made them (ADR 0127). This node
/// neither makes nor reads them; a rollback finds them as they were.
const OLDER_VAULT_TABLES: &str = "
CREATE TABLE vault_passkeys (
    credential_id TEXT PRIMARY KEY, label TEXT NOT NULL, algorithm INTEGER NOT NULL,
    public_key BLOB NOT NULL, sign_count INTEGER NOT NULL DEFAULT 0, prf_salt BLOB NOT NULL,
    wrapped TEXT NOT NULL DEFAULT '', created_ms INTEGER NOT NULL,
    last_used_ms INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE vault_state (
    id INTEGER PRIMARY KEY CHECK (id = 1), shares TEXT NOT NULL DEFAULT '',
    approle TEXT NOT NULL DEFAULT '', keys_in_vault INTEGER NOT NULL DEFAULT 0,
    initialised_ms INTEGER NOT NULL DEFAULT 0
) STRICT;
INSERT INTO vault_state (id) VALUES (1);
INSERT INTO vault_passkeys (credential_id, label, algorithm, public_key, prf_salt, created_ms)
    VALUES ('Y3JlZC1h', 'Jane Doe phone', -7, x'00', x'00', 1);
";

/// A data key as an older Endora left it after moving its keys into OpenBao's Transit.
const MOVED_INTO_OPENBAO: &str = "openbao:v1:placeholder-wrapped-key";

#[derive(World, Default)]
struct SecretsWorld {
    dir: Option<PathBuf>,
    store: Option<SqliteStore>,
    refusal: Option<String>,
}

impl std::fmt::Debug for SecretsWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretsWorld")
            .field("dir", &self.dir)
            .field("refusal", &self.refusal)
            .finish_non_exhaustive()
    }
}

impl SecretsWorld {
    fn database(&self) -> String {
        self.dir
            .as_ref()
            .expect("a node")
            .join("endora.db")
            .to_string_lossy()
            .into_owned()
    }

    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.database()).expect("the database opens")
    }

    fn count(&self, sql: &str) -> i64 {
        self.sql().query_row(sql, [], |r| r.get(0)).expect(sql)
    }
}

impl Drop for SecretsWorld {
    fn drop(&mut self) {
        self.store = None;
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[given(expr = "a new node with no vault address and no vault network")]
fn a_new_node(w: &mut SecretsWorld) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "endora-own-secrets-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    w.dir = Some(dir);
}

#[given(expr = "a node whose database holds an older Endora's vault tables, with one passkey")]
fn older_vault_tables(w: &mut SecretsWorld) {
    a_new_node(w);
    w.sql()
        .execute_batch(OLDER_VAULT_TABLES)
        .expect("the older tables");
}

#[given(expr = "a node whose data keys an older Endora moved into OpenBao")]
fn keys_in_openbao(w: &mut SecretsWorld) {
    a_new_node(w);
    w.sql()
        .execute_batch(&format!(
            "CREATE TABLE secret_keys (kid TEXT PRIMARY KEY, wrapped TEXT NOT NULL,
                 created_ms INTEGER NOT NULL, active INTEGER NOT NULL DEFAULT 0) STRICT;
             INSERT INTO secret_keys VALUES ('k1', '{MOVED_INTO_OPENBAO}', 1, 1);"
        ))
        .expect("a moved key");
}

#[when(expr = "the node starts")]
fn starts(w: &mut SecretsWorld) {
    let database = w.database();
    let key_file = crate::secrets::key_file(&database, None);
    match crate::secrets::open_own_store(&database, &key_file) {
        Ok(store) => w.store = Some(store),
        Err(why) => w.refusal = Some(why.to_string()),
    }
}

#[when(expr = "the owner saves the token {string} for {string}")]
fn saves(w: &mut SecretsWorld, token: String, service: String) {
    let store = w.store.as_ref().expect("the node started");
    ConfigStore::new(store.db())
        .set_setting(&service, "token", &token)
        .expect("saved");
}

#[then(expr = "the node reads the token {string} back for {string}")]
fn reads_back(w: &mut SecretsWorld, token: String, service: String) {
    let store = w.store.as_ref().expect("the node started");
    let settings = ConfigStore::new(store.db()).all_settings().expect("read");
    assert!(settings.contains(&(service, "token".to_owned(), token)));
}

#[then(expr = "{string} is nowhere in the database in the clear")]
fn sealed(w: &mut SecretsWorld, secret: String) {
    w.store = None;
    let bytes = std::fs::read(w.database()).expect("the file");
    let clear = bytes
        .windows(secret.len())
        .any(|win| win == secret.as_bytes());
    assert!(!clear, "the secret is in the database in the clear");
}

#[then(expr = "every data key is wrapped by the node's own key file")]
fn only_the_key_file(w: &mut SecretsWorld) {
    let sql = w.sql();
    let sources: Vec<String> = sql
        .prepare("SELECT wrapped FROM secret_keys")
        .expect("the keys")
        .query_map([], |r| r.get::<_, String>(0))
        .expect("read")
        .map(|w| {
            w.expect("a key")
                .split(':')
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert!(!sources.is_empty(), "no data key");
    assert!(sources.iter().all(|s| s == "file"), "{sources:?}");
}

#[then(expr = "the vault tables are still there, with one passkey")]
fn tables_kept(w: &mut SecretsWorld) {
    assert!(w.store.is_some(), "the node did not start: {:?}", w.refusal);
    assert_eq!(w.count("SELECT COUNT(*) FROM vault_passkeys"), 1);
    assert_eq!(w.count("SELECT COUNT(*) FROM vault_state"), 1);
}

#[then(expr = "it refuses to start, saying {string}")]
fn refuses(w: &mut SecretsWorld, words: String) {
    assert!(w.store.is_none(), "the node started");
    let why = w.refusal.as_deref().expect("a refusal");
    assert!(why.contains(&words), "refused with: {why}");
}

#[then(expr = "the data keys are still there as they were")]
fn keys_kept(w: &mut SecretsWorld) {
    let wrapped: String = w
        .sql()
        .query_row(
            "SELECT wrapped FROM secret_keys WHERE kid = 'k1'",
            [],
            |r| r.get(0),
        )
        .expect("the key");
    assert_eq!(wrapped, MOVED_INTO_OPENBAO);
    assert_eq!(w.count("SELECT COUNT(*) FROM secret_keys"), 1);
}

/// Runs every scenario; a failed, skipped or unparsed step fails this test.
#[test]
fn the_owners_scenarios_for_keeping_its_own_secrets_pass() {
    use cucumber::cli::Parser as _;
    use cucumber::writer::Stats as _;
    let none = cucumber::cli::Opts::<
        cucumber::parser::basic::Cli,
        cucumber::runner::basic::Cli,
        cucumber::writer::basic::Cli,
    >::try_parse_from(["cucumber"])
    .expect("cucumber's own options");
    let writer = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime")
        .block_on(
            SecretsWorld::cucumber()
                .with_cli(none)
                .fail_on_skipped()
                .run("features/secrets"),
        );
    assert!(
        !writer.execution_has_failed() && writer.parsing_errors() == 0,
        "a scenario failed: see the steps above"
    );
    assert!(writer.passed_steps() > 0, "no scenario ran");
}
