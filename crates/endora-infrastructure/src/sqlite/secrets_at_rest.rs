//! Secrets at rest in the node's own database (ADR 0125): what the live database goes
//! through on its first start, and what every later write and read does. Every secret here
//! is an obvious placeholder.

use endora_capabilities::model_lists::ModelListRepository as _;
use endora_capabilities::{
    ButlerModelConfig, ButlerModelConfigRepository, CapabilitySettingsRepository, ConfigStore,
    DeepModel, DeepModelRepository, McpServer, McpServerRegistry, McpTransport,
};
use endora_persistence::Db;
use rusqlite::{Connection, params};

use super::{SCHEMA, SECRET_COLUMNS, SqliteStore};

const DAY_MS: i64 = 24 * 60 * 60 * 1_000;
const LIFE_MS: i64 = 30 * DAY_MS;
const IDLE_MS: i64 = 7 * DAY_MS;

/// A database as the node on `develop` left it: the production schema, with sessions in
/// their old shape (no `last_seen_ms`, the token itself as the key) and every credential in
/// plaintext.
fn a_database_from_before(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE node_sessions (
            token     TEXT PRIMARY KEY,
            issued_ms INTEGER NOT NULL,
            scope     TEXT NOT NULL DEFAULT 'full'
        ) STRICT;",
    )
    .unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn.execute_batch(
        "INSERT INTO model_providers (name, url, api_key)
             VALUES ('cloud', 'https://models.example.com/v1', 'placeholder-provider-key-01');
         INSERT INTO deep_model (id, url, model, api_key)
             VALUES (0, 'https://models.example.com/v1', 'big', 'placeholder-deep-key-02');
         INSERT INTO butler_model_config (id, base_url, api_key, mixture, single_model,
             router_model, synth_model)
             VALUES (0, 'http://localhost:11434/v1', 'placeholder-butler-key-03', 0, 'm', '', '');
         INSERT INTO mcp_servers (name, kind, url, auth)
             VALUES ('home', 'http', 'http://192.168.1.10:8123/mcp', 'placeholder-bearer-04');
         INSERT INTO mcp_servers (name, kind, command, args, env)
             VALUES ('files', 'stdio', 'run', '[]', '{\"TOKEN\":\"placeholder-env-05\"}');
         INSERT INTO capability_settings (capability_id, key, value)
             VALUES ('home-assistant', 'token', 'placeholder-ha-token-06'),
                    ('homelab-sonarr', 'origin', 'http://192.168.1.10:8989');
         INSERT INTO node_auth (id, token, password_hash, totp_secret, pending_totp)
             VALUES (0, 'placeholder-bootstrap-07', '$argon2id$placeholder-hash',
                     'placeholder-totp-08', 'placeholder-pending-09');
         INSERT INTO node_sessions (token, issued_ms, scope)
             VALUES ('placeholder-session-10', 1000, 'full'),
                    ('placeholder-session-11', 1000, 'checks');",
    )
    .unwrap();
}

const PLAINTEXT: &[&str] = &[
    "placeholder-provider-key-01",
    "placeholder-deep-key-02",
    "placeholder-butler-key-03",
    "placeholder-bearer-04",
    "placeholder-env-05",
    "placeholder-ha-token-06",
    "placeholder-bootstrap-07",
    "placeholder-totp-08",
    "placeholder-pending-09",
    "placeholder-session-10",
    "placeholder-session-11",
];

/// Every text cell of every table: what a copy of the file would hand over.
fn every_cell(db: &Db) -> Vec<(String, String)> {
    let conn = db.lock().unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut cells = Vec::new();
    for table in tables {
        let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
        let width = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..width {
                if let Ok(text) = row.get::<_, String>(i) {
                    cells.push((table.clone(), text));
                }
            }
        }
    }
    cells
}

fn raw(db: &Db, sql: &str) -> Vec<String> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(sql).unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn assert_nothing_in_the_clear(db: &Db, secrets: &[&str]) {
    for (table, cell) in every_cell(db) {
        for secret in secrets {
            assert!(
                !cell.contains(secret),
                "{secret} is readable in {table}: the database still holds it in the clear"
            );
        }
    }
}

#[test]
fn the_first_start_seals_every_credential_and_hashes_every_session_once() {
    let conn = Connection::open_in_memory().unwrap();
    a_database_from_before(&conn);
    let db = Db::from_connection(conn).unwrap();

    let store = SqliteStore::from_db(db.clone()).expect("the live database migrates");

    assert_nothing_in_the_clear(&db, PLAINTEXT);
    for column in SECRET_COLUMNS {
        for value in raw(
            &db,
            &format!(
                "SELECT {c} FROM {t} WHERE {c} != ''",
                c = column.column,
                t = column.table
            ),
        ) {
            assert!(
                value.starts_with("enc:v1:"),
                "{} was left unsealed",
                column.label()
            );
        }
    }
    let hashed = raw(&db, "SELECT token FROM node_sessions ORDER BY scope");
    assert!(
        hashed.iter().all(|t| t.starts_with("sha256:")),
        "{hashed:?}"
    );
    assert_eq!(
        raw(&db, "SELECT password_hash FROM node_auth"),
        ["$argon2id$placeholder-hash"],
        "the password hash is a hash already, and is left as it is"
    );

    // Everything reads back through the stores exactly as it was.
    let config = ConfigStore::new(store.db());
    assert_eq!(
        config.providers().unwrap()[0].api_key,
        "placeholder-provider-key-01"
    );
    assert_eq!(
        DeepModelRepository::get(&config).unwrap().unwrap().api_key,
        "placeholder-deep-key-02"
    );
    assert_eq!(
        ButlerModelConfigRepository::get(&config)
            .unwrap()
            .unwrap()
            .api_key,
        "placeholder-butler-key-03"
    );
    let servers = McpServerRegistry::list(&config).unwrap();
    let files = servers.iter().find(|s| s.name == "files").unwrap();
    let McpTransport::Stdio { env, .. } = &files.transport else {
        panic!("files is stdio");
    };
    assert_eq!(env["TOKEN"], "placeholder-env-05");
    let home = servers.iter().find(|s| s.name == "home").unwrap();
    let McpTransport::Http { auth, .. } = &home.transport else {
        panic!("home is http");
    };
    assert_eq!(auth, "placeholder-bearer-04");
    let settings = config.all_settings().unwrap();
    assert!(settings.contains(&(
        "home-assistant".to_owned(),
        "token".to_owned(),
        "placeholder-ha-token-06".to_owned()
    )));
    assert!(settings.contains(&(
        "homelab-sonarr".to_owned(),
        "origin".to_owned(),
        "http://192.168.1.10:8989".to_owned()
    )));
    assert_eq!(
        store.node_token().unwrap().as_deref(),
        Some("placeholder-bootstrap-07")
    );
    assert_eq!(store.sign_in_setup().unwrap().1, "placeholder-totp-08");
    assert_eq!(
        store.pending_authenticator().unwrap(),
        "placeholder-pending-09"
    );
    // A session held before the upgrade still opens, and nobody was signed out by it.
    let now = 1000 + DAY_MS;
    assert_eq!(
        store
            .session_scope("placeholder-session-11", now, LIFE_MS, IDLE_MS)
            .unwrap()
            .as_deref(),
        Some("checks")
    );

    // A second start changes nothing: guarded and idempotent.
    let before = every_cell(&db);
    SqliteStore::from_db(db.clone()).unwrap();
    assert_eq!(every_cell(&db), before);
}

#[test]
fn a_session_signed_in_before_the_upgrade_is_not_signed_out_by_it() {
    // The old rows have no last use; they are given the moment of the upgrade, not 0, or
    // every console would idle out the instant the new node started.
    let conn = Connection::open_in_memory().unwrap();
    a_database_from_before(&conn);
    let store = SqliteStore::from_db(Db::from_connection(conn).unwrap()).unwrap();
    let now = super::now_ms();
    assert_eq!(
        store
            .session_scope("placeholder-session-10", now, i64::MAX / 4, IDLE_MS)
            .unwrap()
            .as_deref(),
        Some("full")
    );
}

#[test]
fn every_credential_the_stores_write_is_sealed_and_reads_back() {
    let store = SqliteStore::open_in_memory().unwrap();
    let db = store.db();
    let config = ConfigStore::new(db.clone());
    config
        .add_provider(
            "cloud",
            "https://models.example.com/v1",
            "placeholder-key-21",
        )
        .unwrap();
    let id = config.providers().unwrap()[0].id;
    config
        .update_provider(
            id,
            "cloud",
            "https://models.example.com/v1",
            Some("placeholder-key-22"),
        )
        .unwrap();
    DeepModelRepository::set(
        &config,
        &DeepModel {
            url: "https://models.example.com/v1".to_owned(),
            model: "big".to_owned(),
            api_key: "placeholder-key-23".to_owned(),
            escalate: false,
        },
    )
    .unwrap();
    ButlerModelConfigRepository::set(
        &config,
        &ButlerModelConfig {
            api_key: "placeholder-key-24".to_owned(),
            ..ButlerModelConfig::default()
        },
    )
    .unwrap();
    McpServerRegistry::register(
        &config,
        &McpServer::http_with_auth("home", "http://192.168.1.10:8123/mcp", "placeholder-key-25")
            .unwrap(),
    )
    .unwrap();
    McpServerRegistry::register(
        &config,
        &McpServer::stdio_with_env(
            "files",
            "run",
            Vec::new(),
            [("TOKEN".to_owned(), "placeholder-key-26".to_owned())].into(),
        )
        .unwrap(),
    )
    .unwrap();
    config
        .set_setting("home-assistant", "token", "placeholder-key-27")
        .unwrap();
    store.set_node_token("placeholder-key-28").unwrap();
    store
        .set_sign_in_setup("$argon2id$placeholder-hash", "placeholder-key-29")
        .unwrap();
    store
        .set_pending_authenticator("placeholder-key-30")
        .unwrap();
    store
        .add_session("placeholder-key-31", "full", 1_000, LIFE_MS)
        .unwrap();

    let written: Vec<String> = (21..=31).map(|n| format!("placeholder-key-{n}")).collect();
    let written: Vec<&str> = written.iter().map(String::as_str).collect();
    assert_nothing_in_the_clear(&db, &written);

    assert_eq!(config.providers().unwrap()[0].api_key, "placeholder-key-22");
    assert_eq!(
        store.node_token().unwrap().as_deref(),
        Some("placeholder-key-28")
    );
    assert_eq!(store.pending_authenticator().unwrap(), "placeholder-key-30");
    // Clearing still clears: an empty value means none, and is stored as none.
    store.set_pending_authenticator("").unwrap();
    assert_eq!(raw(&db, "SELECT pending_totp FROM node_auth"), [""]);
}

#[test]
fn a_value_written_around_the_store_is_refused_and_the_error_never_repeats_it() {
    let store = SqliteStore::open_in_memory().unwrap();
    let config = ConfigStore::new(store.db());
    config
        .add_provider(
            "cloud",
            "https://models.example.com/v1",
            "placeholder-key-41",
        )
        .unwrap();
    store
        .db()
        .lock()
        .unwrap()
        .execute(
            "UPDATE model_providers SET api_key = ?1",
            params!["placeholder-planted-42"],
        )
        .unwrap();

    let refused = config.providers().expect_err("plaintext is refused");
    let message = refused.to_string();
    assert!(message.contains("model_providers.api_key"), "{message}");
    assert!(!message.contains("placeholder-planted-42"), "{message}");
}

#[test]
fn a_store_on_disk_without_a_key_source_is_refused_not_left_in_the_clear() {
    let path = std::env::temp_dir().join(format!("endora-no-key-{}.db", uuid::Uuid::new_v4()));
    let db = Db::open(path.to_str().unwrap()).unwrap();
    assert!(SqliteStore::from_db(db).is_err());
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_session_is_kept_only_as_its_digest() {
    let store = SqliteStore::open_in_memory().unwrap();
    let token = "placeholder-session-token-51";
    store.add_session(token, "full", 1_000, LIFE_MS).unwrap();

    let kept = raw(&store.db(), "SELECT token FROM node_sessions");
    assert_eq!(kept.len(), 1);
    assert!(kept[0].starts_with("sha256:"), "{}", kept[0]);
    assert!(!kept[0].contains(token));
    assert_eq!(
        store
            .session_scope(token, 2_000, LIFE_MS, IDLE_MS)
            .unwrap()
            .as_deref(),
        Some("full")
    );
    for wrong in ["", "placeholder-session-token-5", &kept[0]] {
        assert_eq!(
            store.session_scope(wrong, 2_000, LIFE_MS, IDLE_MS).unwrap(),
            None,
            "{wrong:?} opened a session"
        );
    }
}

#[test]
fn a_session_lasts_thirty_days_and_a_console_one_ends_after_a_week_unused() {
    let store = SqliteStore::open_in_memory().unwrap();
    store.add_session("console", "full", 0, LIFE_MS).unwrap();
    store.add_session("checks", "checks", 0, LIFE_MS).unwrap();
    let scope = |token: &str, at: i64| store.session_scope(token, at, LIFE_MS, IDLE_MS).unwrap();

    // Used every few days, the console's session lasts its whole life.
    for day in [3, 6, 9, 12, 15, 18, 21, 24, 27, 30] {
        assert_eq!(
            scope("console", day * DAY_MS).as_deref(),
            Some("full"),
            "day {day}"
        );
    }
    assert_eq!(
        scope("console", 30 * DAY_MS + 1),
        None,
        "and not a moment more"
    );

    // Left alone for over a week, it ends; the checking credential does not idle out.
    store.add_session("idle", "full", 0, LIFE_MS).unwrap();
    assert_eq!(scope("idle", 7 * DAY_MS + 1), None);
    assert_eq!(scope("checks", 20 * DAY_MS).as_deref(), Some("checks"));
}

/// A node whose keys a sealed vault wraps (ADR 0127) still starts: the schema is applied, the
/// stores answer what needs no secret, and every secret says the vault is locked until the
/// keys are attached, after which the same values open.
#[test]
fn a_store_waiting_for_the_vault_starts_says_so_and_opens_when_its_keys_arrive() {
    use std::sync::Arc;
    let dir = std::env::temp_dir().join(format!("endora-waiting-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("endora.db").to_string_lossy().into_owned();
    let key_file = dir.join("secrets.key");
    {
        let store = SqliteStore::open_sealed(&path, &key_file).unwrap();
        ConfigStore::new(store.db())
            .set_setting("home-assistant", "token", "placeholder-token-0009")
            .unwrap();
    }

    let store = SqliteStore::open_waiting(Db::open(&path).unwrap()).unwrap();
    let error = ConfigStore::new(store.db())
        .all_settings()
        .unwrap_err()
        .to_string();
    assert!(error.contains("vault is locked"), "{error}");
    assert!(
        store
            .session_scope("nobody", 0, LIFE_MS, IDLE_MS)
            .unwrap()
            .is_none(),
        "what needs no secret still answers"
    );

    let keys = endora_persistence::secrets::KeyFile::open(&key_file, false).unwrap();
    store.attach_keys(Arc::new(keys)).unwrap();
    let settings = ConfigStore::new(store.db()).all_settings().unwrap();
    assert!(settings.contains(&(
        "home-assistant".to_owned(),
        "token".to_owned(),
        "placeholder-token-0009".to_owned()
    )));
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}
