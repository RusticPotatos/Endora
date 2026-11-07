//! The owner's scenarios for `endora-core setup` (`app/node/features/setup/`): the command is
//! run against a node's store as on first run, and the owner it makes signs in through the
//! node's own router, exactly as Console or a client would.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cucumber::{World, given, then, when};
use endora_infrastructure::{RandomIdSource, SystemClock};
use tower::ServiceExt as _;

#[derive(World, Default)]
struct SetupWorld {
    dir: Option<PathBuf>,
    /// The command's result: what it printed, or why it refused.
    outcome: Option<Result<String, String>>,
    /// The authenticator key the first successful setup printed.
    key: Option<String>,
    /// Every row of the node's store but its sign-in, before the command ran.
    before: Option<Vec<String>>,
}

impl std::fmt::Debug for SetupWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupWorld")
            .field("dir", &self.dir)
            .field("outcome", &self.outcome)
            .finish_non_exhaustive()
    }
}

impl Drop for SetupWorld {
    fn drop(&mut self) {
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

impl SetupWorld {
    fn database(&self) -> String {
        self.dir
            .as_ref()
            .expect("a node")
            .join("endora.db")
            .to_string_lossy()
            .into_owned()
    }

    fn setup(&mut self, password: &str) {
        let mut input = std::io::Cursor::new(format!("{password}\n"));
        let mut out = Vec::new();
        let result = super::run(&self.database(), &mut input, &mut out);
        let printed = String::from_utf8(out).expect("text");
        self.outcome = Some(result.map(|()| printed).map_err(|e| e.to_string()));
    }

    /// Every row of every table but the sign-in's, as text, in a stable order.
    fn rows_but_sign_in(&self) -> Vec<String> {
        let sql = rusqlite::Connection::open(self.database()).expect("the database opens");
        let mut tables: Vec<String> = sql
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .expect("tables")
            .query_map([], |r| r.get(0))
            .expect("tables")
            .map(Result::unwrap)
            .collect();
        tables.retain(|t| t != "node_auth");
        let mut rows = Vec::new();
        for table in tables {
            let mut each = sql
                .prepare(&format!("SELECT * FROM \"{table}\""))
                .expect("a table");
            let columns = each.column_count();
            let mut found = each.query([]).expect("rows");
            while let Some(row) = found.next().expect("a row") {
                let cells: Vec<String> = (0..columns)
                    .map(|i| format!("{:?}", row.get_ref(i).expect("a cell")))
                    .collect();
                rows.push(format!("{table}: {}", cells.join(" | ")));
            }
        }
        rows.sort();
        rows
    }

    /// Signs in through the node's router with `password` and `code`.
    async fn sign_in(&self, password: &str, code: &str) -> (StatusCode, serde_json::Value) {
        let store = crate::secrets::open_store(&self.database()).expect("the store opens");
        let state = crate::api::AppState::new(
            Arc::new(store),
            Arc::new(RandomIdSource),
            Arc::new(SystemClock),
        );
        let body = serde_json::json!({ "password": password, "code": code }).to_string();
        let response = crate::api::app(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/session")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .expect("a request"),
            )
            .await
            .expect("an answer");
        let status = response.status();
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("a body")
            .to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }
}

/// The current code for a base32 key, as an authenticator app computes it.
fn code_for(key: &str) -> u32 {
    let mut bits = 0_u64;
    let mut count = 0;
    let mut secret = Vec::new();
    for c in key.trim_end_matches('=').chars() {
        let value = match c {
            'A'..='Z' => c as u64 - 'A' as u64,
            '2'..='7' => c as u64 - '2' as u64 + 26,
            other => panic!("not base32: {other:?}"),
        };
        bits = (bits << 5) | value;
        count += 5;
        if count >= 8 {
            count -= 8;
            secret.push(u8::try_from((bits >> count) & 0xff).expect("a byte"));
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_secs();
    crate::totp::code_at(&secret, now)
}

#[given(expr = "a new node nobody owns")]
fn a_new_node(w: &mut SetupWorld) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "endora-setup-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a node's folder");
    w.dir = Some(dir);
    // Opened once, as the node's first start would: its tables and its key file exist.
    drop(crate::secrets::open_store(&w.database()).expect("the store opens"));
}

#[given(expr = "a node whose owner was made with the password {string}")]
fn a_node_with_an_owner(w: &mut SetupWorld, password: String) {
    a_new_node(w);
    w.setup(&password);
    setup_made_the_owner(w);
}

#[when(expr = "the owner runs setup with the password {string}")]
#[when(expr = "someone runs setup with the password {string}")]
fn runs_setup(w: &mut SetupWorld, password: String) {
    w.before = Some(w.rows_but_sign_in());
    w.setup(&password);
}

#[when(expr = "the node starts for the first time")]
fn the_node_starts(w: &mut SetupWorld) {
    // As the node's start does: it makes its token, which must leave the owner alone.
    let store = crate::secrets::open_store(&w.database()).expect("the store opens");
    drop(crate::api::AppState::new(
        Arc::new(store),
        Arc::new(RandomIdSource),
        Arc::new(SystemClock),
    ));
}

#[then(expr = "setup says the owner is made and prints an authenticator key")]
fn setup_made_the_owner(w: &mut SetupWorld) {
    let printed = match w.outcome.as_ref().expect("setup ran") {
        Ok(printed) => printed.clone(),
        Err(why) => panic!("setup refused: {why}"),
    };
    assert!(printed.contains("owner"), "{printed}");
    let uri = printed
        .lines()
        .find(|l| l.trim_start().starts_with("otpauth://"))
        .unwrap_or_else(|| panic!("no otpauth URI in {printed}"));
    let key = uri
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("secret="))
        .expect("a secret in the URI")
        .to_owned();
    assert!(printed.contains(&key), "the key is printed on its own too");
    w.key = Some(key);
}

#[then(expr = "the owner signs in with {string} and a code from that key")]
#[then(expr = "the owner still signs in with {string} and a code from the first key")]
async fn signs_in(w: &mut SetupWorld, password: String) {
    let code = format!("{:06}", code_for(w.key.as_ref().expect("a key")));
    let (status, body) = w.sign_in(&password, &code).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["token"].as_str().is_some_and(|t| t.len() >= 32),
        "{body}"
    );
}

#[then(expr = "a wrong code with the right password is refused")]
async fn wrong_code(w: &mut SetupWorld) {
    let right = code_for(w.key.as_ref().expect("a key"));
    let wrong = format!("{:06}", (right + 500_000) % 1_000_000);
    let (status, body) = w.sign_in("correct horse battery staple", &wrong).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.get("token").is_none(), "{body}");
}

#[then(expr = "setup refuses, saying {string}")]
fn setup_refuses(w: &mut SetupWorld, words: String) {
    match w.outcome.as_ref().expect("setup ran") {
        Ok(printed) => panic!("setup did not refuse: {printed}"),
        Err(why) => assert!(why.contains(&words), "{why}"),
    }
}

#[then(expr = "the node still has no owner")]
fn no_owner(w: &mut SetupWorld) {
    let store = crate::secrets::open_store(&w.database()).expect("the store opens");
    let (hash, _) = store.sign_in_setup().expect("the sign-in row");
    assert!(hash.is_empty(), "a password was stored");
}

#[then(expr = "setup changed nothing in the node but its sign-in")]
fn nothing_else(w: &mut SetupWorld) {
    assert!(
        matches!(w.outcome, Some(Ok(_))),
        "setup refused: {:?}",
        w.outcome
    );
    assert_eq!(
        w.before.as_ref().expect("rows before"),
        &w.rows_but_sign_in(),
        "setup changed something besides the owner's sign-in"
    );
}

#[test]
fn the_owners_scenarios_for_endora_core_setup_pass() {
    use cucumber::cli::Parser as _;
    use cucumber::writer::Stats as _;
    let none = cucumber::cli::Opts::<
        cucumber::parser::basic::Cli,
        cucumber::runner::basic::Cli,
        cucumber::writer::basic::Cli,
    >::try_parse_from(["cucumber"])
    .expect("cucumber's own options");
    let writer = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(
            SetupWorld::cucumber()
                .with_cli(none)
                .fail_on_skipped()
                .run("features/setup"),
        );
    assert!(
        !writer.execution_has_failed() && writer.parsing_errors() == 0,
        "a scenario failed: see the steps above"
    );
    assert!(writer.passed_steps() > 0, "no scenario ran");
}
