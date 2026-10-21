use super::{AppState, app};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use endora_infrastructure::{RandomIdSource, SqliteStore, SystemClock};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt; // for `oneshot`

/// A known token, so the helpers below can sign like the console does.
pub(super) const TEST_TOKEN: &str = "a-token-for-tests";

#[tokio::test]
async fn dashboard_token_is_narrow_and_overview_does_not_disclose_items() {
    let state = test_state();
    *state.homelab.lock().unwrap() = serde_json::json!({"observed_at_ms":1,"services":[{"name":"docker","state":"healthy","total":1,"items":[{"name":"private-container"}]}]});
    let router = app(state);
    let minted = json_body(
        router
            .clone()
            .oneshot(post("/v1/session/dashboard", "{}"))
            .await
            .unwrap(),
    )
    .await;
    let token = minted["token"].as_str().unwrap();
    for path in [
        "/v1/homelab/overview",
        "/v1/audit",
        "/v1/homelab",
        "/v1/activity",
    ] {
        let request = Request::builder()
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        if path == "/v1/homelab/overview" {
            assert_eq!(response.status(), StatusCode::OK);
            assert!(
                !json_body(response)
                    .await
                    .to_string()
                    .contains("private-container")
            );
        } else {
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }
}

#[tokio::test]
async fn handheld_actions_refuse_missing_confirmation_and_do_not_reserve_a_submission() {
    let state = test_state();
    let router = app(state);
    let response=router.clone().oneshot(post("/v1/terminal/action",r#"{"request_id":"fictional-operation","kind":"home","action":"on","target":"light.example","confirmed":false}"#)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let result = json_body(response).await;
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .contains("confirmation_required")
    );
    let result = router
        .oneshot(get("/v1/terminal/operations/fictional-operation"))
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn homelab_is_authenticated_and_unconfigured_by_default() {
    let router = app(test_state());
    let unsigned = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/homelab")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsigned.status(), StatusCode::UNAUTHORIZED);
    let response = router.oneshot(get("/v1/homelab")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["services"].as_array().unwrap().len(), 3);
    assert!(
        body["services"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["state"] == "unconfigured")
    );
}

#[tokio::test]
async fn homelab_pages_and_marks_old_observations_stale() {
    let state = test_state();
    let items: Vec<_> = (0..50)
        .map(|n| serde_json::json!({"title":format!("Example {n}")}))
        .collect();
    *state.homelab.lock().unwrap() = serde_json::json!({"observed_at_ms":1,"services":[{"name":"sonarr","state":"healthy","items":items,"total":50}]});
    let response = app(state).oneshot(get("/v1/homelab?page=2")).await.unwrap();
    let body = json_body(response).await;
    assert_eq!(body["services"][0]["state"], "stale");
    assert_eq!(body["services"][0]["items"].as_array().unwrap().len(), 20);
    assert_eq!(body["services"][0]["items"][0]["title"], "Example 20");
}

#[tokio::test]
async fn homelab_worker_reads_the_production_settings_and_projects_upstream_data() {
    use endora_capabilities::CapabilitySettingsRepository;
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut request = [0; 4096];
        let n = socket.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..n]);
        assert!(request.starts_with("GET /api/v3/queue?page=1&pageSize=100"));
        assert!(
            request
                .to_lowercase()
                .contains("x-api-key: fictional-test-key")
        );
        let body = r#"{"totalRecords":1,"records":[{"title":"Example","size":100,"sizeleft":25,"status":"downloading","outputPath":"/private"}]}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let state = test_state();
    state
        .config
        .set_setting("homelab-sonarr", "origin", &origin)
        .unwrap();
    state
        .config
        .set_setting("homelab-sonarr", "api_key", "fictional-test-key")
        .unwrap();
    super::spawn_homelab(state.clone());
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if state.homelab.lock().unwrap()["services"][1]["state"] == "healthy" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let body = json_body(app(state).oneshot(get("/v1/homelab")).await.unwrap()).await;
    assert_eq!(body["services"][1]["items"][0]["progress"], 75.0);
    assert_eq!(body["services"][0]["state"], "unconfigured");
    assert!(!body.to_string().contains("private"));
    assert!(!body.to_string().contains("fictional-test-key"));
    thread.join().unwrap();
}

pub(super) fn test_state() -> AppState {
    let mut state = AppState::new(
        Arc::new(SqliteStore::open_in_memory().unwrap()),
        Arc::new(RandomIdSource),
        Arc::new(SystemClock),
    );
    // Pinned rather than read back, so a test signs with something it chose. The
    // generated-on-first-run path is exercised by `the_node_makes_and_keeps_a_token`.
    state.token = Arc::new(TEST_TOKEN.to_owned());
    state
}

pub(super) async fn json_body(res: axum::response::Response) -> serde_json::Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

pub(super) fn post(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {TEST_TOKEN}"))
        .body(Body::from(body.to_owned()))
        .unwrap()
}

pub(super) fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("authorization", format!("Bearer {TEST_TOKEN}"))
        .body(Body::empty())
        .unwrap()
}

/// Exactly the same request, unsigned — for proving a route is actually shut.
pub(super) fn unsigned(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn an_unsigned_request_gets_nothing() {
    // The hole this exists for: every one of these answered anyone who could reach the
    // port, on an interface bound to 0.0.0.0. `/v1/export` handed over the whole record, and
    // a model list would have taken an endpoint of somebody's choosing.
    let app = app(test_state());
    for path in [
        "/v1/export",
        "/v1/hq",
        "/v1/capabilities",
        "/v1/model-lists",
        "/v1/activity/stream",
    ] {
        let res = app.clone().oneshot(unsigned(path)).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "{path} answered an unsigned request"
        );
    }
}

#[tokio::test]
async fn a_wrong_token_is_no_better_than_none() {
    let res = app(test_state())
        .oneshot(
            Request::builder()
                .uri("/v1/export")
                .header("authorization", "Bearer not-the-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_console_still_loads_without_one() {
    // It must: the screen that asks for the token is served by this node, and a health
    // check has to answer before anybody has one.
    let app = app(test_state());
    for path in ["/", "/app.js", "/styles.css", "/health"] {
        let res = app.clone().oneshot(unsigned(path)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{path} should stay open");
    }
}

#[tokio::test]
async fn a_password_and_a_real_code_sign_you_in() {
    // End to end, through the router, with a code computed from the secret the node
    // actually stored. This is the test that would have caught hashing the base32 text
    // instead of the bytes it encodes — a mistake whose only symptom is "the codes never
    // work", with nothing in any log to say why.
    let state = test_state();
    let store = state.store.clone();
    super::ensure_enrolment_secret(&store, &RandomIdSource).unwrap();
    let hash = super::hash_password("a-long-enough-password").unwrap();
    let (_, secret) = store.sign_in_setup().unwrap();
    store.set_sign_in_setup(&hash, &secret).unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let code = format!(
        "{:06}",
        crate::totp::code_at(&crate::totp::from_hex(&secret), now)
    );

    let app = app(state);
    let res = app
        .clone()
        .oneshot(post(
            "/v1/session",
            &format!(r#"{{"password":"a-long-enough-password","code":"{code}"}}"#),
        ))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "a correct sign-in was refused"
    );
    let issued = json_body(res).await;
    let session = issued["token"]
        .as_str()
        .expect("a session token")
        .to_owned();

    // And the token it handed back actually opens the door.
    let res = app
        .oneshot(
            Request::builder()
                .uri("/v1/capabilities")
                .header("authorization", format!("Bearer {session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "the session token was refused"
    );
}

#[tokio::test]
async fn signing_in_is_reachable_without_a_token_but_a_wrong_one_gets_nothing() {
    // It must answer unsigned — a credential is what it exists to hand out — and it must
    // still refuse. Both halves matter; either alone is a bug.
    let res = app(test_state())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/session")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"password":"wrong","code":"000000"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(
        res.status(),
        StatusCode::UNAUTHORIZED,
        "sign-in must be reachable without a credential"
    );
    assert!(!res.status().is_success(), "and must refuse a wrong one");
}

#[tokio::test]
async fn a_password_too_short_to_be_worth_hashing_is_refused() {
    let res = app(test_state())
        .oneshot(post("/v1/session/setup", r#"{"password":"short"}"#))
        .await
        .unwrap();
    assert!(!res.status().is_success());
}

#[tokio::test]
async fn a_node_nobody_owns_can_be_claimed_without_the_token() {
    // The password-first page. Home Assistant, on the same network and controlling more,
    // leaves this open indefinitely; this closes it after a quarter of an hour, because a
    // node left unclaimed for weeks would otherwise be claimable by anyone who ever
    // reached it — which was this node's actual situation.
    let app = app(test_state());
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/session/setup")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"password":"a-long-enough-password"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        res.status().is_success(),
        "a fresh node should be claimable: {}",
        res.status()
    );
}

#[tokio::test]
async fn a_node_that_has_an_owner_cannot_be_claimed_again() {
    // The half that matters. Once a password exists the window is shut permanently, so a
    // stranger cannot take a running house by asking nicely at the right moment.
    let state = test_state();
    let hash = super::hash_password("the-owners-password").unwrap();
    let (_, secret) = state.store.sign_in_setup().unwrap();
    state.store.set_sign_in_setup(&hash, &secret).unwrap();

    let res = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/session/setup")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"password":"someone-elses-password"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

/// A signed-in state with a known password, for the credential-management tests.
fn signed_in_state() -> (AppState, String) {
    let state = test_state();
    super::ensure_enrolment_secret(&state.store, &RandomIdSource).unwrap();
    let hash = super::hash_password("the-current-password").unwrap();
    let (_, secret) = state.store.sign_in_setup().unwrap();
    state.store.set_sign_in_setup(&hash, &secret).unwrap();
    (state, secret)
}

fn code_now(secret_hex: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!(
        "{:06}",
        crate::totp::code_at(&crate::totp::from_hex(secret_hex), now)
    )
}

#[tokio::test]
async fn changing_the_password_needs_the_current_one() {
    // Being signed in is not enough. A session lives in a browser and can be taken; the
    // password is the thing only the person knows.
    let (state, _) = signed_in_state();
    let app = app(state);

    let refused = app
        .clone()
        .oneshot(post(
            "/v1/session/password",
            r#"{"current":"a-wrong-guess","new":"a-brand-new-password"}"#,
        ))
        .await
        .unwrap();
    assert!(!refused.status().is_success());

    let allowed = app
        .oneshot(post(
            "/v1/session/password",
            r#"{"current":"the-current-password","new":"a-brand-new-password"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_half_finished_authenticator_swap_leaves_the_old_phone_working() {
    // The lock-out this two-step exists to prevent. Somebody asks for a new authenticator,
    // the QR does not scan, they close the tab — and their existing phone must still open
    // the door.
    let (state, original) = signed_in_state();
    let store = state.store.clone();
    let app = app(state);

    let started = app
        .clone()
        .oneshot(post(
            "/v1/session/authenticator",
            r#"{"current":"the-current-password"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(started.status(), StatusCode::OK);

    // Nothing has changed yet.
    assert_eq!(
        store.sign_in_setup().unwrap().1,
        original,
        "live secret moved"
    );
    assert!(
        !store.pending_authenticator().unwrap().is_empty(),
        "a replacement should be waiting"
    );
    // And the original still signs in.
    let res = app
        .oneshot(post(
            "/v1/session",
            &format!(
                r#"{{"password":"the-current-password","code":"{}"}}"#,
                code_now(&original)
            ),
        ))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "the old phone stopped working"
    );
}

#[tokio::test]
async fn a_code_from_the_old_phone_does_not_lock_in_the_new_one() {
    // The subtle version of the same mistake: confirming against the *live* secret would
    // happily accept a code from the phone somebody is replacing, and lock in a secret
    // nobody ever scanned.
    let (state, original) = signed_in_state();
    let store = state.store.clone();
    let app = app(state);
    app.clone()
        .oneshot(post(
            "/v1/session/authenticator",
            r#"{"current":"the-current-password"}"#,
        ))
        .await
        .unwrap();

    let res = app
        .oneshot(post(
            "/v1/session/authenticator/confirm",
            &format!(r#"{{"code":"{}"}}"#, code_now(&original)),
        ))
        .await
        .unwrap();
    assert!(
        !res.status().is_success(),
        "the old code confirmed the new secret"
    );
    assert_eq!(
        store.sign_in_setup().unwrap().1,
        original,
        "live secret moved"
    );
}

#[tokio::test]
async fn a_code_from_the_new_one_locks_it_in() {
    let (state, original) = signed_in_state();
    let store = state.store.clone();
    let app = app(state);
    app.clone()
        .oneshot(post(
            "/v1/session/authenticator",
            r#"{"current":"the-current-password"}"#,
        ))
        .await
        .unwrap();
    let pending = store.pending_authenticator().unwrap();

    let res = app
        .oneshot(post(
            "/v1/session/authenticator/confirm",
            &format!(r#"{{"code":"{}"}}"#, code_now(&pending)),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(store.sign_in_setup().unwrap().1, pending, "not locked in");
    assert_ne!(store.sign_in_setup().unwrap().1, original);
    assert!(
        store.pending_authenticator().unwrap().is_empty(),
        "the pending secret should be spent"
    );
}

#[tokio::test]
async fn managing_credentials_is_closed_to_strangers() {
    let app = app(test_state());
    for (path, method) in [
        ("/v1/session/password", "POST"),
        ("/v1/session/authenticator", "POST"),
        ("/v1/session/authenticator/confirm", "POST"),
        ("/v1/session/bootstrap", "GET"),
    ] {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{method} {path}");
    }
}

#[tokio::test]
async fn a_checking_token_reads_but_cannot_touch_anything() {
    // End to end, through the real router. This credential is the one that ends up in a
    // plaintext file on a laptop, so what it *cannot* do is the whole point of it.
    let app = app(test_state());
    let minted = json_body(
        app.clone()
            .oneshot(post("/v1/session/checks", "{}"))
            .await
            .unwrap(),
    )
    .await;
    let checking = minted["token"].as_str().expect("a token").to_owned();
    let with = |uri: &str, method: &str| {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {checking}"))
            .body(Body::from("{}"))
            .unwrap()
    };

    // Reads the suite depends on.
    for path in ["/v1/audit", "/v1/capabilities"] {
        let res = app.clone().oneshot(with(path, "GET")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{path} should be readable");
    }
    // The bulk dump, and every write — including the one that would redirect every later
    // turn to somebody else's server.
    for (path, method) in [
        ("/v1/export", "GET"),
        ("/v1/model-lists/providers", "POST"),
        ("/v1/memory/purge", "POST"),
        ("/v1/session/checks", "POST"),
    ] {
        let res = app.clone().oneshot(with(path, method)).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path} should be refused"
        );
    }
}

#[tokio::test]
async fn minting_one_needs_a_full_credential() {
    let res = app(test_state())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/session/checks")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_claim_window_does_not_open_anything_else() {
    // Being unclaimed is a reason to allow *setting the first password*, and nothing else.
    // Without this the fifteen minutes would be fifteen minutes of no security at all.
    let app = app(test_state());
    for path in [
        "/v1/export",
        "/v1/hq",
        "/v1/model-lists",
        "/v1/capabilities",
    ] {
        let res = app.clone().oneshot(unsigned(path)).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{path} was opened");
    }
}

#[tokio::test]
async fn reading_the_enrolment_secret_is_behind_the_token_even_when_unclaimed() {
    // The claim window allows *setting* the first password. Handing out the authenticator
    // secret to an unsigned GET would let somebody enrol their own phone against a node
    // they never claimed, and then wait.
    let res = app(test_state())
        .oneshot(unsigned("/v1/session/setup"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_secret_is_never_replaced_once_it_exists() {
    // Regenerating one a phone already holds would silently break every code, and the
    // person would have no way to tell that from a broken implementation.
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    super::ensure_enrolment_secret(&store, &RandomIdSource).unwrap();
    let (_, first) = store.sign_in_setup().unwrap();
    assert!(!first.is_empty());
    super::ensure_enrolment_secret(&store, &RandomIdSource).unwrap();
    assert_eq!(store.sign_in_setup().unwrap().1, first);
}

#[tokio::test]
async fn the_token_is_findable_for_as_long_as_it_is_the_only_way_in() {
    // The lockout this exists for: printed once on the run that generated it, then gone —
    // `docker logs` shows only the current container, every deploy recreates it, and the
    // token is stored rather than remade. Within a day the console was asking for
    // something nobody could obtain.
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let token = super::the_nodes_token(&store, &RandomIdSource);

    // Not yet claimed: it must be shown.
    assert!(!super::already_signed_in(&store), "nothing is set up yet");

    // Once a password exists it is a recovery credential, not a setup step, and should
    // stop appearing in a log that gets pasted into an issue the first time anything
    // goes wrong.
    let hash = super::hash_password("a-long-enough-password").unwrap();
    let (_, secret) = store.sign_in_setup().unwrap();
    store.set_sign_in_setup(&hash, &secret).unwrap();
    assert!(super::already_signed_in(&store));

    assert!(!token.is_empty());
}

#[tokio::test]
async fn the_node_makes_and_keeps_a_token() {
    // Generated once and remembered, so a restart does not lock the person out of their
    // own console.
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    assert_eq!(
        store.node_token().unwrap(),
        None,
        "nothing before first run"
    );

    let ids = Arc::new(RandomIdSource);
    let first = super::the_nodes_token(&store, &ids);
    assert!(first.len() >= 32, "not enough token: {first:?}");
    assert_eq!(
        super::the_nodes_token(&store, &ids),
        first,
        "kept, not remade"
    );
}

#[tokio::test]
async fn root_serves_the_web_console() {
    let res = app(test_state())
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let ct = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.starts_with("text/html"), "content-type was {ct}");
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("<title>Endora</title>"));
}

#[tokio::test]
async fn there_is_no_way_for_the_person_to_create_an_intention() {
    // ADR 0052's first constraint, as a test rather than a promise. Endora forms
    // its own intentions; a console with an "add" button would mean the ADR failed,
    // and the API is where that would have to start.
    let app = app(test_state());
    for (uri, body) in [
        ("/v1/intentions", r#"{"statement":"do a thing"}"#),
        ("/v1/intentions/1", r#"{"statement":"edit a thing"}"#),
    ] {
        let res = app.clone().oneshot(post(uri, body)).await.unwrap();
        assert!(
            res.status() == StatusCode::METHOD_NOT_ALLOWED || res.status() == StatusCode::NOT_FOUND,
            "POST {uri} answered {} — the person must not be able to file work",
            res.status()
        );
    }
}

#[test]
fn an_enabled_server_with_no_tools_is_the_state_worth_retrying() {
    // The condition the heartbeat looks for, stated as a test because the bug it
    // fixes was silent: after a reboot the Home Assistant server was registered,
    // enabled, and exposing zero tools, so the butler narrated actions it had no
    // way to take. Zero tools on an ENABLED server never makes sense — it means the
    // connection found nothing — and nothing retried it.
    use endora_application::CapabilitySpec;
    let has_tools = |server: &str, live: &[CapabilitySpec]| {
        let prefix = format!("{server}.");
        live.iter().any(|c| c.id.starts_with(&prefix))
    };
    let spec = |id: &str| CapabilitySpec {
        id: id.to_owned(),
        wants_place: false,
        third_party: false,
        description: String::new(),
        configured: true,
        autonomous: false,
        input_schema: None,
        reversibility: endora_application::Reversibility::Irreversible,
    };

    // A healthy server is left alone.
    let live = vec![spec("home-assistant.HassTurnOff")];
    assert!(has_tools("home-assistant", &live));

    // One that came up empty is detected — including when another server is fine,
    // which is the case a naive "any tools at all?" check would miss.
    assert!(!has_tools("calendar", &live));
}

#[tokio::test]
async fn a_write_notifies_change_subscribers_but_a_read_does_not() {
    let state = test_state();
    let mut rx = state.changes.subscribe();
    let app = app(state);

    // A read must not signal a change.
    app.clone().oneshot(get("/v1/activity")).await.unwrap();
    assert!(rx.try_recv().is_err());

    // A successful write signals exactly one change.
    let res = app
        .clone()
        .oneshot(post(
            "/v1/model-lists/providers",
            r#"{"name":"house","url":"http://192.168.1.10:11434/v1"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(rx.try_recv().is_ok());
}

#[tokio::test]
async fn a_rejected_write_does_not_notify() {
    let state = test_state();
    let mut rx = state.changes.subscribe();
    let app = app(state);

    // An invalid request (no endpoint) is a 400 and must not signal.
    let res = app
        .clone()
        .oneshot(post(
            "/v1/model-lists/providers",
            r#"{"name":"house","url":"  "}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn activity_stream_opens_as_an_event_stream() {
    let res = app(test_state())
        .oneshot(get("/v1/activity/stream"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let ct = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(ct.starts_with("text/event-stream"), "got content-type {ct}");
}

#[tokio::test]
async fn editing_a_server_with_a_blank_token_keeps_the_saved_one() {
    // Registered `disabled` so no real connection is attempted — this exercises the
    // config merge, not networking.
    let router = app(test_state());
    let reg = router
        .clone()
        .oneshot(post(
            "/v1/mcp/servers",
            r#"{"name":"ha","transport":"http","url":"http://old/sse","auth":"secret-token","command":"","args":[],"env":{},"enabled":false}"#,
        ))
        .await
        .unwrap();
    assert_eq!(reg.status(), StatusCode::OK);

    // Edit: change only the URL, leave the token blank (the UI can't echo secrets).
    let edit = router
        .clone()
        .oneshot(post(
            "/v1/mcp/servers",
            r#"{"name":"ha","transport":"http","url":"http://new/sse","auth":"","command":"","args":[],"env":{},"enabled":false}"#,
        ))
        .await
        .unwrap();
    assert_eq!(edit.status(), StatusCode::OK);

    let list = json_body(router.oneshot(get("/v1/mcp/servers")).await.unwrap()).await;
    let ha = list["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "ha")
        .expect("ha server present");
    // URL updated, but the token was preserved rather than wiped.
    assert_eq!(ha["url"], "http://new/sse");
    assert_eq!(ha["auth_set"], true);
}

#[tokio::test]
async fn trust_all_defaults_on_and_can_be_toggled() {
    let router = app(test_state());
    // Registered without a trust_all field → defaults on. Disabled so no connect.
    let reg = router
        .clone()
        .oneshot(post(
            "/v1/mcp/servers",
            r#"{"name":"ha","transport":"http","url":"http://ha/sse","auth":"t","enabled":false}"#,
        ))
        .await
        .unwrap();
    assert_eq!(reg.status(), StatusCode::OK);
    let list = json_body(
        router
            .clone()
            .oneshot(get("/v1/mcp/servers"))
            .await
            .unwrap(),
    )
    .await;
    let ha = list["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "ha")
        .unwrap();
    assert_eq!(ha["trust_all"], true);

    // Turn it off via the endpoint.
    let off = router
        .clone()
        .oneshot(post("/v1/mcp/servers/ha/trust", r#"{"trust_all":false}"#))
        .await
        .unwrap();
    assert_eq!(off.status(), StatusCode::OK);
    let list = json_body(router.oneshot(get("/v1/mcp/servers")).await.unwrap()).await;
    let ha = list["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "ha")
        .unwrap();
    assert_eq!(ha["trust_all"], false);
}

#[tokio::test]
async fn testing_an_unknown_server_is_404() {
    let res = app(test_state())
        .oneshot(post("/v1/mcp/servers/nope/test", ""))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

/// A server with no state reader is not tested by calling something at random.
///
/// The reader nomination is the person saying "this one only looks" (ADR 0054). Without
/// it there is nothing here that is known to be safe to call, and the honest answer is
/// to say so rather than to pick a tool and find out what it does.
#[tokio::test]
async fn a_server_with_no_state_reader_says_so_instead_of_calling_something() {
    let router = app(test_state());
    let reg = router
        .clone()
        .oneshot(post(
            "/v1/mcp/servers",
            r#"{"name":"unread","transport":"http","url":"http://x/sse","enabled":false}"#,
        ))
        .await
        .unwrap();
    assert_eq!(reg.status(), StatusCode::OK);

    let res = router
        .oneshot(post("/v1/mcp/servers/unread/test", ""))
        .await
        .unwrap();
    // A soft result the person can read and act on, not a failed request.
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["ok"], false);
    let said = body["said"].as_str().unwrap();
    assert!(said.contains("reads this server's state"), "{said}");
}

#[tokio::test]
async fn nominating_a_reader_on_an_unknown_server_is_404() {
    let res = app(test_state())
        .oneshot(post(
            "/v1/mcp/servers/nope/reader",
            r#"{"reader_tool":"list_events"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_reader_nomination_must_name_a_tool_the_server_really_has() {
    // A typo must not quietly disable read-back — it would fail by simply never
    // verifying anything, which is the worst way for a safety mechanism to break.
    let state = test_state();
    let app = app(state);
    let res = app
        .clone()
        .oneshot(post(
            "/v1/mcp/servers/whatever/reader",
            r#"{"reader_tool":"GetLiveContxt"}"#,
        ))
        .await
        .unwrap();
    assert!(
        res.status() == StatusCode::BAD_REQUEST || res.status() == StatusCode::NOT_FOUND,
        "a tool the server doesn't expose was accepted: {}",
        res.status()
    );
}

#[tokio::test]
async fn setting_trust_on_an_unknown_server_is_404() {
    let res = app(test_state())
        .oneshot(post("/v1/mcp/servers/nope/trust", r#"{"trust_all":true}"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reconnecting_an_unknown_mcp_server_is_404() {
    let res = app(test_state())
        .oneshot(post("/v1/mcp/servers/nope/reconnect", ""))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reconnecting_a_registered_server_reports_its_tool_count() {
    let state = test_state();
    // Register a stdio server whose command won't produce tools; the point is that
    // reconnect answers for a real, known server rather than erroring.
    let router = app(state);
    let reg = router
        .clone()
        .oneshot(post(
            "/v1/mcp/servers",
            r#"{"name":"probe","transport":"stdio","command":"true","args":[],"env":{},"url":"","auth":"","enabled":true}"#,
        ))
        .await
        .unwrap();
    assert_eq!(reg.status(), StatusCode::OK);

    let res = router
        .oneshot(post("/v1/mcp/servers/probe/reconnect", ""))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["ok"], true);
    // It didn't connect (no real MCP server behind `true`), and the endpoint says
    // so honestly instead of failing.
    assert_eq!(body["connected"], false);
    assert_eq!(body["tools_live"], 0);
}

#[tokio::test]
async fn an_entry_that_only_holds_settings_is_marked_so_the_console_can_say_it_is_set_up() {
    // The seven handheld entries are never "configured" and their on/off does nothing, so
    // shown like a skill every one said "needs setup" forever.
    let res = app(test_state())
        .oneshot(get("/v1/capabilities"))
        .await
        .unwrap();
    let all = json_body(res).await;
    let find = |id: &str| {
        all.as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("{id} is listed"))
    };
    let portainer = find("terminal-portainer");
    assert_eq!(portainer["settings_only"], true);
    assert_eq!(portainer["settings_complete"], false, "nothing entered yet");
    let weather = find("weather");
    assert_eq!(weather["settings_only"], false);
    // A skill that rides a connection names it, so the console can send the person there.
    assert_eq!(find("containers")["borrows_from"], "homelab-docker");
    assert!(weather["borrows_from"].is_null());
}

#[tokio::test]
async fn a_skill_that_rides_a_connection_is_not_on_until_the_connection_is_set_up() {
    // The console and chat must agree. Chat counted the containers skill ready only once
    // Docker's address was entered; the skills list said "On" from the start.
    let usable = |all: &serde_json::Value| {
        all.as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "containers")
            .map(|c| c["usable"].clone())
            .unwrap()
    };
    let state = test_state();
    let before = json_body(
        app(state.clone())
            .oneshot(get("/v1/capabilities"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(usable(&before), false, "Docker is not set up yet");
    endora_capabilities::application::CapabilitySettingsRepository::set_setting(
        state.config.as_ref(),
        "homelab-docker",
        "origin",
        "http://127.0.0.1:9876",
    )
    .unwrap();
    let after = json_body(app(state).oneshot(get("/v1/capabilities")).await.unwrap()).await;
    assert_eq!(usable(&after), true, "Docker is set up now");
}

#[tokio::test]
async fn a_skill_whose_description_is_written_for_the_model_has_a_line_for_people() {
    // The settings screen showed the model's instructions: "Use this for any question about
    // what YOU did", "Civic business ONLY", "fetch useful URLs with web_fetch".
    let all = json_body(
        app(test_state())
            .oneshot(get("/v1/capabilities"))
            .await
            .unwrap(),
    )
    .await;
    for id in [
        "web_search",
        "web_fetch",
        "city_meetings",
        "ticketed_events",
    ] {
        let c = all
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("{id} is listed"));
        let summary = c["summary"]
            .as_str()
            .unwrap_or_else(|| panic!("{id} has a summary"));
        assert_ne!(summary, c["description"].as_str().unwrap(), "{id}");
        for model_talk in [
            "ONLY",
            "YOU",
            "Use this",
            "Use it",
            "web_fetch",
            "The person",
        ] {
            assert!(!summary.contains(model_talk), "{id}: {summary}");
        }
    }
    // A skill whose description already reads for people shows it as it is.
    let weather = all
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "weather")
        .unwrap();
    assert_eq!(weather["summary"], weather["description"]);
}

/// The AI screen says what the team's local turns think with (ADR 0136): the house's model is
/// what the relay calls, the installed model until an Everyday entry on the house's network
/// comes first, and never a cloud entry, whatever its place in the list.
#[tokio::test]
async fn the_ai_screen_names_the_house_model_the_relay_uses() {
    let mut state = test_state();
    state.installed_model = Arc::new((
        "http://192.168.1.10:11434/v1".to_owned(),
        "installed:7b".to_owned(),
    ));
    let role = |all: &serde_json::Value, id: &str| {
        all["roles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("{id} is listed"))
    };
    let before = json_body(app(state.clone()).oneshot(get("/v1/ai")).await.unwrap()).await;
    let ids: Vec<&str> = before["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["house", "vision"], "the chat's roles are gone");
    assert_eq!(role(&before, "house")["model"], "installed:7b");
    let add = |url: &'static str, model: &'static str| {
        let state = state.clone();
        async move {
            let provider = json_body(
                app(state.clone())
                    .oneshot(post(
                        "/v1/model-lists/providers",
                        &format!(r#"{{"name":"p","url":"{url}","api_key":"fictional"}}"#),
                    ))
                    .await
                    .unwrap(),
            )
            .await["id"]
                .as_i64()
                .unwrap();
            let res = app(state)
                .oneshot(post(
                    "/v1/model-lists/entries",
                    &format!(r#"{{"job":"everyday","provider":{provider},"model":"{model}"}}"#),
                ))
                .await
                .unwrap();
            assert!(res.status().is_success(), "{}", res.status());
        }
    };
    add("https://api.example.com/v1", "cloud-model").await;
    let cloud_first = json_body(app(state.clone()).oneshot(get("/v1/ai")).await.unwrap()).await;
    assert_eq!(
        role(&cloud_first, "house")["model"],
        "installed:7b",
        "a cloud entry is never the house's"
    );
    add("http://192.168.1.10:11434/v1", "house:14b").await;
    let after = json_body(app(state.clone()).oneshot(get("/v1/ai")).await.unwrap()).await;
    let house = role(&after, "house");
    assert_eq!(house["model"], "house:14b");
    assert_eq!(house["local"], true);
    // The Deep list served the chat; it takes nothing now.
    let refused = app(state)
        .oneshot(post(
            "/v1/model-lists/entries",
            r#"{"job":"deep","provider":1,"model":"big"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn spotify_is_one_sign_in_any_device_can_start_and_its_callback_needs_the_waiting_state() {
    let state = test_state();
    let status = |body: &serde_json::Value| body["state"].as_str().unwrap_or("").to_owned();
    let first = json_body(
        app(state.clone())
            .oneshot(get("/v1/connections/spotify"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status(&first), "not_set_up");
    let no_id = app(state.clone())
        .oneshot(post("/v1/connections/spotify/begin", r#"{"via":"paste"}"#))
        .await
        .unwrap();
    assert_eq!(
        no_id.status(),
        StatusCode::BAD_REQUEST,
        "a client id is needed"
    );
    // Started via Endora, from the address the person reached it at.
    let begin = Request::builder()
        .method("POST")
        .uri("/v1/connections/spotify/begin")
        .header("content-type", "application/json")
        .header("host", "192.168.1.10:8787")
        .header("authorization", format!("Bearer {TEST_TOKEN}"))
        .body(Body::from(
            r#"{"client_id":"exampleclientid","via":"endora"}"#,
        ))
        .unwrap();
    let waiting = json_body(app(state.clone()).oneshot(begin).await.unwrap()).await;
    assert_eq!(status(&waiting), "waiting");
    assert_eq!(
        waiting["redirect"],
        "http://192.168.1.10:8787/connect/spotify/callback"
    );
    assert!(
        waiting["sign_in_url"].as_str().unwrap().contains(
            "redirect_uri=http%3A%2F%2F192.168.1.10%3A8787%2Fconnect%2Fspotify%2Fcallback"
        )
    );
    // The callback needs no token, but a state that is not the waiting one finishes nothing.
    let res = app(state.clone())
        .oneshot(unsigned(
            "/connect/spotify/callback?code=x&state=not-the-one",
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let page = String::from_utf8(
        axum::body::to_bytes(res.into_body(), 64 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(page.contains("not connected"), "{page}");
    let still = json_body(
        app(state.clone())
            .oneshot(get("/v1/connections/spotify"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status(&still), "waiting");
    // Nothing to lend before it is connected.
    let lend = app(state)
        .oneshot(post("/v1/connections/spotify/token", "{}"))
        .await
        .unwrap();
    assert_eq!(lend.status(), StatusCode::BAD_REQUEST);
}

/// Answers one request from the node as HQ would, and hands back what the node sent.
fn one_hq_answer(body: &'static str) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut buf = [0u8; 8192];
        let n = socket.read(&mut buf).unwrap();
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(reply.as_bytes()).unwrap();
        String::from_utf8_lossy(&buf[..n]).into_owned()
    });
    (url, seen)
}

#[tokio::test]
async fn hq_that_is_not_set_up_says_so() {
    let res = app(test_state()).oneshot(get("/v1/hq")).await.unwrap();
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json_body(res).await["configured"], false);
}

#[tokio::test]
async fn the_kill_switch_goes_to_hq_signed_and_into_endoras_own_record() {
    let (url, seen) = one_hq_answer(r#"{"stopped":true}"#);
    let token = "a-token-of-at-least-thirty-two-characters";
    let state = test_state().with_hq(endora_infrastructure::HqClient::new(&url, token));
    let res = app(state.clone())
        .oneshot(post("/v1/hq/stop", r#"{"why":"trying it"}"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let sent = seen.join().unwrap();
    assert!(sent.starts_with("POST /v1/stop "), "{sent}");
    assert!(
        sent.to_lowercase()
            .contains(&format!("authorization: bearer {token}"))
    );
    let trail = endora_application::AuditLog::recent(state.audit.as_ref(), 10).unwrap();
    assert!(
        trail
            .iter()
            .any(|r| r.summary() == "HQ: you pulled the kill switch"),
        "{trail:?}"
    );
}

#[tokio::test]
async fn the_activity_feed_pages_on_with_an_offset() {
    let state = test_state();
    let now = endora_application::Clock::now(state.clock.as_ref());
    for n in 1..=5 {
        endora_application::EventLog::record(state.events.as_ref(), now, &format!("event {n}"))
            .unwrap();
    }
    let summaries = |page: serde_json::Value| -> Vec<String> {
        page.as_array()
            .unwrap()
            .iter()
            .map(|a| a["summary"].as_str().unwrap().to_owned())
            .collect()
    };
    let first = json_body(
        app(state.clone())
            .oneshot(get("/v1/activity?limit=2"))
            .await
            .unwrap(),
    )
    .await;
    let second = json_body(
        app(state.clone())
            .oneshot(get("/v1/activity?limit=2&offset=2"))
            .await
            .unwrap(),
    )
    .await;
    let last = json_body(
        app(state.clone())
            .oneshot(get("/v1/activity?limit=2&offset=4"))
            .await
            .unwrap(),
    )
    .await;
    let beyond = json_body(
        app(state)
            .oneshot(get("/v1/activity?limit=2&offset=9"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(summaries(first), ["event 5", "event 4"]);
    assert_eq!(summaries(second), ["event 3", "event 2"]);
    assert_eq!(summaries(last), ["event 1"]);
    assert!(summaries(beyond).is_empty());
}

#[tokio::test]
async fn the_log_can_ask_hq_for_the_events_before_the_ones_it_has() {
    let (url, seen) = one_hq_answer(r#"{"events":[]}"#);
    let token = "a-token-of-at-least-thirty-two-characters";
    let state = test_state().with_hq(endora_infrastructure::HqClient::new(&url, token));
    let res = app(state)
        .oneshot(get("/v1/hq/events?before=40&limit=50"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let sent = seen.join().unwrap();
    assert!(
        sent.starts_with("GET /v1/events?before=40&limit=50 "),
        "{sent}"
    );
}

#[tokio::test]
async fn the_stream_still_resumes_after_an_id() {
    let (url, seen) = one_hq_answer(r#"{"events":[]}"#);
    let token = "a-token-of-at-least-thirty-two-characters";
    let state = test_state().with_hq(endora_infrastructure::HqClient::new(&url, token));
    let res = app(state)
        .oneshot(get("/v1/hq/events?after=7"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let sent = seen.join().unwrap();
    assert!(sent.starts_with("GET /v1/events?after=7 "), "{sent}");
}

#[tokio::test]
async fn providers_go_in_once_lists_sort_and_no_key_ever_comes_back_out() {
    let state = test_state();
    let added = json_body(
        app(state.clone())
            .oneshot(post(
                "/v1/model-lists/providers",
                r#"{"name":"Anthropic","url":"https://api.anthropic.com/v1/","api_key":"sk-ant-fictional"}"#,
            ))
            .await
            .unwrap(),
    )
    .await;
    let claude = added["id"].as_i64().unwrap();
    let local = json_body(
        app(state.clone())
            .oneshot(post(
                "/v1/model-lists/providers",
                r#"{"url":"http://localhost:11434/v1"}"#,
            ))
            .await
            .unwrap(),
    )
    .await["id"]
        .as_i64()
        .unwrap();
    let add = |provider: i64, model: &str| {
        post(
            "/v1/model-lists/entries",
            &format!(r#"{{"job":"everyday","provider":{provider},"model":"{model}"}}"#),
        )
    };
    app(state.clone())
        .oneshot(add(local, "qwen2.5:7b"))
        .await
        .unwrap();
    let second = json_body(
        app(state.clone())
            .oneshot(add(claude, "claude-sonnet-5-5"))
            .await
            .unwrap(),
    )
    .await;
    let id = second["id"].as_i64().unwrap();
    let moved = app(state.clone())
        .oneshot(post(&format!("/v1/model-lists/entries/{id}/up"), "{}"))
        .await
        .unwrap();
    assert_eq!(moved.status(), StatusCode::OK);

    let shown = json_body(
        app(state.clone())
            .oneshot(get("/v1/model-lists"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        shown["everyday"][0]["model"], "claude-sonnet-5-5",
        "moved to the top"
    );
    assert_eq!(shown["everyday"][0]["local"], false);
    assert_eq!(shown["everyday"][1]["local"], true);
    assert_eq!(
        shown["everyday"][1]["thinks"], false,
        "an entry does not think unless the owner lets it"
    );
    let local_entry = shown["everyday"][1]["id"].as_i64().unwrap();
    let thinks = app(state.clone())
        .oneshot(post(
            &format!("/v1/model-lists/entries/{local_entry}/think"),
            "{}",
        ))
        .await
        .unwrap();
    assert_eq!(thinks.status(), StatusCode::OK);
    let shown_now = json_body(
        app(state.clone())
            .oneshot(get("/v1/model-lists"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(shown_now["everyday"][1]["thinks"], true);
    app(state.clone())
        .oneshot(post(
            &format!("/v1/model-lists/entries/{local_entry}/quiet"),
            "{}",
        ))
        .await
        .unwrap();
    let shown_now = json_body(
        app(state.clone())
            .oneshot(get("/v1/model-lists"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(shown_now["everyday"][1]["thinks"], false);
    assert_eq!(
        shown["providers"][0]["url"], "https://api.anthropic.com/v1",
        "kept without the trailing slash"
    );
    assert_eq!(shown["providers"][0]["key_set"], true);
    assert_eq!(shown["providers"][1]["key_set"], false);
    assert!(shown.get("deep").is_none() && shown.get("escalate").is_none());
    assert!(
        !shown.to_string().contains("sk-ant"),
        "no key is ever shown: {shown}"
    );

    let bad = app(state.clone())
        .oneshot(post(
            "/v1/model-lists/providers",
            r#"{"url":"api.example.com"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let nowhere = app(state.clone()).oneshot(add(999, "x")).await.unwrap();
    assert_eq!(nowhere.status(), StatusCode::NOT_FOUND);

    app(state.clone())
        .oneshot(post(
            &format!("/v1/model-lists/providers/{claude}/remove"),
            "{}",
        ))
        .await
        .unwrap();
    let shown = json_body(
        app(state.clone())
            .oneshot(get("/v1/model-lists"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        shown["everyday"].as_array().unwrap().len(),
        1,
        "its places went with it"
    );
    // Saving over a provider that is gone says so, rather than "saved" while the typed key is
    // dropped.
    let gone = app(state)
        .oneshot(post(
            &format!("/v1/model-lists/providers/{claude}"),
            r#"{"name":"DeepSeek","url":"https://api.deepseek.com/v1","api_key":"sk-fictional"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
}

/// The body of a response as text, whatever its status.
async fn text_body(res: axum::response::Response) -> String {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Every text cell in the node's database: the audit trail, the event log, the usage log
/// and everything else a copy of the file would carry.
fn every_cell(store: &SqliteStore) -> Vec<String> {
    let db = store.db();
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
                    cells.push(format!("{table}: {text}"));
                }
            }
        }
    }
    cells
}

#[tokio::test]
async fn no_secret_reaches_the_logs_the_answers_or_the_database_in_the_clear() {
    // ADR 0125. Each placeholder goes in through the way a real credential would, and is
    // then looked for everywhere the node writes or says anything.
    use endora_capabilities::CapabilitySettingsRepository as _;
    let (state, secret) = signed_in_state();
    let store = state.store.clone();
    let config = state.config.clone();
    let router = app(state);
    let provider_key = "placeholder-provider-secret-61";
    let refused_key = "placeholder-refused-secret-62";
    let ha_token = "placeholder-ha-secret-63";
    let wrong_password = "placeholder-wrong-password-64";
    let planted = "placeholder-planted-secret-65";

    let added = router
        .clone()
        .oneshot(post(
            "/v1/model-lists/providers",
            &format!(r#"{{"url":"https://models.example.com/v1","api_key":"{provider_key}"}}"#),
        ))
        .await
        .unwrap();
    assert_eq!(added.status(), StatusCode::OK);
    let mut said = text_body(added).await;
    let refused = router
        .clone()
        .oneshot(post(
            "/v1/model-lists/providers",
            &format!(r#"{{"url":"not-a-url","api_key":"{refused_key}"}}"#),
        ))
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    said.push_str(&text_body(refused).await);
    config
        .set_setting("home-assistant", "token", ha_token)
        .unwrap();

    // A failed sign-in, then a good one: neither answer, nor the throttle's record of the
    // failure, carries the password, and the session token is kept only as a digest.
    let failed = router
        .clone()
        .oneshot(post(
            "/v1/session",
            &format!(r#"{{"password":"{wrong_password}","code":"000000"}}"#),
        ))
        .await
        .unwrap();
    said.push_str(&text_body(failed).await);
    let good = router
        .clone()
        .oneshot(post(
            "/v1/session",
            &format!(
                r#"{{"password":"the-current-password","code":"{}"}}"#,
                code_now(&secret)
            ),
        ))
        .await
        .unwrap();
    let session = json_body(good).await["token"].as_str().unwrap().to_owned();

    for path in [
        "/v1/audit",
        "/v1/activity",
        "/v1/export",
        "/v1/model-lists",
        "/v1/capabilities",
    ] {
        said.push_str(&text_body(router.clone().oneshot(get(path)).await.unwrap()).await);
    }
    // And a value planted around the store is refused without being repeated.
    store
        .db()
        .lock()
        .unwrap()
        .execute(
            "UPDATE model_providers SET api_key = ?1",
            rusqlite::params![planted],
        )
        .unwrap();
    said.push_str(
        &text_body(
            router
                .clone()
                .oneshot(get("/v1/model-lists"))
                .await
                .unwrap(),
        )
        .await,
    );

    for secret in [provider_key, refused_key, ha_token, wrong_password, planted] {
        assert!(!said.contains(secret), "{secret} was said in an answer");
    }
    let cells = every_cell(&store);
    for secret in [
        provider_key,
        refused_key,
        ha_token,
        wrong_password,
        session.as_str(),
        secret.as_str(),
    ] {
        assert!(
            !cells.iter().any(|cell| cell.contains(secret)),
            "a secret is in the database in the clear"
        );
    }
}

/// The vault (ADR 0127): its status and the unlock answer before anyone signs in, and nothing
/// else of it does. A node with no vault configured says so.
#[tokio::test]
async fn the_vault_status_and_unlock_answer_unsigned_and_nothing_else_of_the_vault_does() {
    let router = app(test_state());
    let unsigned = |method: &str, path: &str| {
        Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap()
    };
    let status = router
        .clone()
        .oneshot(unsigned("GET", "/v1/vault"))
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    let status = json_body(status).await;
    assert_eq!(status["vault"], "not configured");
    assert_eq!(status["rp_id"], "endora.home.arpa");
    assert_eq!(status["node_waiting"], false);

    let begin = router
        .clone()
        .oneshot(unsigned("POST", "/v1/vault/unlock/begin"))
        .await
        .unwrap();
    assert_eq!(
        begin.status(),
        StatusCode::BAD_REQUEST,
        "let through, and refused for want of a passkey"
    );
    for path in [
        "/v1/vault/passkeys/begin",
        "/v1/vault/ceremony",
        "/v1/vault/initialise",
        "/v1/vault/keys-into-vault",
        "/v1/vault/start-over",
    ] {
        let res = router
            .clone()
            .oneshot(unsigned("POST", path))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
    let listed = router.oneshot(get("/v1/vault/passkeys")).await.unwrap();
    assert_eq!(
        listed.status(),
        StatusCode::OK,
        "signed, the owner may list"
    );
}

/// Endora is HQ (ADR 0136): the chat and everything that served only it are gone from the
/// node, so each of their routes is a plain 404, signed or not.
#[tokio::test]
async fn the_chats_routes_are_gone() {
    let router = app(test_state());
    for (method, path) in [
        ("POST", "/v1/chat"),
        ("GET", "/v1/chat"),
        ("GET", "/v1/chat/days"),
        ("GET", "/v1/chat/context"),
        ("POST", "/v1/chat/context/reset"),
        ("POST", "/v1/chat/stream"),
        ("POST", "/v1/brief"),
        ("GET", "/v1/brief/schedule"),
        ("GET", "/v1/research"),
        ("GET", "/v1/research/readiness"),
        ("GET", "/v1/brain"),
        ("POST", "/v1/brain/forget"),
        ("GET", "/v1/brain/pages/memory"),
        ("GET", "/v1/preferences"),
        ("GET", "/v1/context"),
        ("GET", "/v1/agenda"),
        ("GET", "/v1/agenda/rules"),
        ("GET", "/v1/mail"),
        ("GET", "/v1/recipes"),
        ("GET", "/v1/autonomy"),
        ("GET", "/v1/deep-model"),
        ("GET", "/v1/deep-model/usage"),
        ("POST", "/v1/deep-ask"),
        ("POST", "/v1/model-layer/run"),
        ("POST", "/v1/model-lists/escalate"),
        ("GET", "/v1/outcomes"),
        ("GET", "/v1/repairs"),
        ("GET", "/v1/reliability"),
        ("GET", "/v1/aliases"),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{method} {path} still answers"
        );
    }
}

/// What Endora keeps about the owner now is its record of what it did (Activity: the audit trail
/// and the events): the export is that record, and deleting forgets it while the settings the
/// team runs on stay (ADR 0136).
#[tokio::test]
async fn the_export_is_the_record_and_deleting_forgets_it() {
    use endora_application::{AuditId, AuditLog as _, AuditRecord, Timestamp};
    let state = test_state();
    state
        .audit
        .append(
            &AuditRecord::new(AuditId::new(1), Timestamp::from_unix_millis(5), "noted").unwrap(),
        )
        .unwrap();
    let router = app(state);
    let export = json_body(router.clone().oneshot(get("/v1/export")).await.unwrap()).await;
    let fields: Vec<&str> = export
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(fields, ["audit", "events"], "{export}");
    assert_eq!(export["audit"].as_array().unwrap().len(), 1);
    let refused = router
        .clone()
        .oneshot(post("/v1/memory/purge", r#"{"confirm":false}"#))
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    let done = router
        .clone()
        .oneshot(post("/v1/memory/purge", r#"{"confirm":true}"#))
        .await
        .unwrap();
    assert_eq!(done.status(), StatusCode::OK);
    let after = json_body(router.oneshot(get("/v1/export")).await.unwrap()).await;
    assert!(after["audit"].as_array().unwrap().is_empty());
}

/// Health says the node is up and which build it runs; with no chat there is no turn to be
/// busy with and no butler model to name (ADR 0136).
#[tokio::test]
async fn health_says_the_build_and_nothing_of_the_chat() {
    let health = json_body(app(test_state()).oneshot(get("/health")).await.unwrap()).await;
    assert_eq!(health["status"], "ok");
    assert!(health.get("version").is_some() && health.get("build").is_some());
    for gone in ["busy", "serving_url", "serving_model"] {
        assert!(
            health.get(gone).is_none(),
            "{gone} is still reported: {health}"
        );
    }
}
