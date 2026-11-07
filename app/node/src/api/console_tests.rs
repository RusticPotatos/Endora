//! The Core ↔ Console contract (ADR 0145), held from Core's side: every route the contract
//! lists is served by the node alone (Core runs with no Console), sessions end when asked, an
//! answer to a desk item is bound, idempotent and staged, and the event stream resumes from a
//! cursor.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{Request, StatusCode};
use axum::routing::get;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt as _;

use super::app;
use super::tests::{TEST_TOKEN, get as signed_get, json_body, post, test_state};

const CONTRACT: &str = include_str!("../../../../docs/api/console-v1.json");
const DAY: i64 = 24 * 60 * 60 * 1_000;

fn contract() -> Value {
    serde_json::from_str(CONTRACT).expect("the contract is JSON")
}

fn with(method: &str, uri: &str, token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_owned()))
        .unwrap()
}

fn now_ms() -> i64 {
    use endora_application::Clock as _;
    endora_infrastructure::SystemClock.now().unix_millis()
}

// ---- the contract ---------------------------------------------------------------------------

#[tokio::test]
async fn health_says_the_contract_version() {
    let health = json_body(
        app(test_state())
            .oneshot(signed_get("/health"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(health["api"], contract()["version"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn every_route_the_contract_lists_is_served_by_the_node_alone() {
    // Core with Console disabled: nothing but the node's own router, and not one listed route
    // missing from it. A route the router does not have answers 404 with an empty body (or
    // 405 for a method it lacks); a handler's own refusal always says why.
    let router = app(test_state());
    let routes = contract()["routes"].as_array().unwrap().clone();
    assert!(routes.len() > 50, "the whole list, not a sample");
    for route in routes {
        let method = route["method"].as_str().unwrap();
        let path = route["path"].as_str().unwrap();
        let uri = path
            .replace("{id}", "1")
            .replace("{name}", "fixture")
            .replace("{slug}", "fixture")
            .replace("{action}", "up");
        let body = if method == "GET" { "" } else { "{}" };
        let res = router
            .clone()
            .oneshot(with(method, &uri, TEST_TOKEN, body))
            .await
            .unwrap();
        let status = res.status();
        let said = res.into_body().collect().await.unwrap().to_bytes();
        assert_ne!(status, StatusCode::METHOD_NOT_ALLOWED, "{method} {path}");
        assert!(
            !(status == StatusCode::NOT_FOUND && said.is_empty()),
            "{method} {path} is not served"
        );
    }
}

#[test]
fn the_contract_holds_no_hq_route_and_none_of_what_console_must_never_call() {
    let doc = contract();
    let never: Vec<&str> = doc["never"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for required in [
        "/v1/runner",
        "/v1/team",
        "/v1/hq/desk/",
        "/v1/round",
        "/team-relay",
    ] {
        assert!(never.contains(&required), "{required} is not refused");
    }
    for route in doc["routes"].as_array().unwrap() {
        let path = route["path"].as_str().unwrap();
        assert!(
            !never.iter().any(|n| path.starts_with(n)),
            "{path} is on the never list"
        );
        assert!(
            ["none", "session"].contains(&route["auth"].as_str().unwrap()),
            "{path}"
        );
        assert!(path.starts_with("/v1/") || path == "/health" || path.starts_with("/connect/"));
    }
}

// ---- sessions -------------------------------------------------------------------------------

#[tokio::test]
async fn a_session_says_when_it_ends_and_the_bootstrap_token_never_does() {
    let state = test_state();
    let issued = now_ms();
    state
        .store
        .add_session("a-console-session", "full", issued, 30 * DAY)
        .unwrap();
    let router = app(state);
    let mine = json_body(
        router
            .clone()
            .oneshot(with("GET", "/v1/session/current", "a-console-session", ""))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(mine["scope"], "full");
    assert_eq!(mine["bootstrap"], false);
    assert_eq!(mine["expires_ms"].as_i64(), Some(issued + 30 * DAY));
    assert!(mine["idle_expires_ms"].as_i64().unwrap() >= issued + 7 * DAY);
    let boot = json_body(
        router
            .oneshot(signed_get("/v1/session/current"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(boot["bootstrap"], true);
    assert!(boot["expires_ms"].is_null() && boot["idle_expires_ms"].is_null());
}

#[tokio::test]
async fn signing_out_ends_that_session_at_once() {
    let state = test_state();
    state
        .store
        .add_session("leaving", "full", now_ms(), 30 * DAY)
        .unwrap();
    state
        .store
        .add_session("staying", "full", now_ms(), 30 * DAY)
        .unwrap();
    let router = app(state);
    let out = router
        .clone()
        .oneshot(with("POST", "/v1/session/sign-out", "leaving", "{}"))
        .await
        .unwrap();
    assert_eq!(out.status(), StatusCode::OK);
    let after = |token: &'static str| {
        router
            .clone()
            .oneshot(with("GET", "/v1/session/current", token, ""))
    };
    assert_eq!(
        after("leaving").await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(after("staying").await.unwrap().status(), StatusCode::OK);
    // The bootstrap token is the recovery path, not a session: it cannot be signed out.
    let boot = router
        .clone()
        .oneshot(post("/v1/session/sign-out", "{}"))
        .await
        .unwrap();
    assert_eq!(boot.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn signing_out_everywhere_ends_every_console_session_and_leaves_the_checking_one() {
    let state = test_state();
    for (token, scope) in [("one", "full"), ("two", "full"), ("smoke", "checks")] {
        state
            .store
            .add_session(token, scope, now_ms(), 30 * DAY)
            .unwrap();
    }
    let router = app(state);
    let said = json_body(
        router
            .clone()
            .oneshot(with("POST", "/v1/session/sign-out-everywhere", "one", "{}"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(said["ended"], 2);
    for (token, open) in [("one", false), ("two", false), ("smoke", true)] {
        let res = router
            .clone()
            .oneshot(with("GET", "/v1/session/current", token, ""))
            .await
            .unwrap();
        assert_eq!(res.status() == StatusCode::OK, open, "{token}");
    }
}

#[tokio::test]
async fn changing_the_password_ends_every_other_console_session() {
    let state = test_state();
    let hash = super::auth::hash_password("the-old-fixture-password").unwrap();
    state.store.set_sign_in_setup(&hash, "00").unwrap();
    for token in ["here", "elsewhere"] {
        state
            .store
            .add_session(token, "full", now_ms(), 30 * DAY)
            .unwrap();
    }
    let router = app(state);
    let changed = router
        .clone()
        .oneshot(with(
            "POST",
            "/v1/session/password",
            "here",
            r#"{"current":"the-old-fixture-password","new":"a-new-fixture-password"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(changed.status(), StatusCode::OK);
    let open = |token: &'static str| {
        router
            .clone()
            .oneshot(with("GET", "/v1/session/current", token, ""))
    };
    assert_eq!(open("here").await.unwrap().status(), StatusCode::OK);
    assert_eq!(
        open("elsewhere").await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

// ---- a fake HQ, with a desk and events ------------------------------------------------------

#[derive(Default)]
struct Hq {
    /// id → (kind, status)
    desk: HashMap<i64, (String, String)>,
    /// POSTs to the desk that changed something.
    applied: usize,
    /// The next POST answers 503 and changes nothing.
    fail_next: bool,
    /// The next POST changes the item and then answers 502, as if the answer were lost.
    lose_next: bool,
    events: Vec<i64>,
}

type Shared = Arc<Mutex<Hq>>;

fn allowed(kind: &str, choice: &str) -> bool {
    match kind {
        "goal" => ["approve", "decline"].contains(&choice),
        "question" => ["seen", "dismiss"].contains(&choice),
        _ => false,
    }
}

async fn fake_desk_item(
    State(hq): State<Shared>,
    Path(id): Path<i64>,
) -> (StatusCode, Json<Value>) {
    let hq = hq.lock().unwrap();
    match hq.desk.get(&id) {
        Some((kind, status)) => (
            StatusCode::OK,
            Json(json!({"id": id, "kind": kind, "text": "A fixture item", "status": status})),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown desk item"})),
        ),
    }
}

async fn fake_decide(
    State(hq): State<Shared>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let mut hq = hq.lock().unwrap();
    if std::mem::take(&mut hq.fail_next) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "busy"})),
        );
    }
    let choice = body["decision"].as_str().unwrap_or_default().to_owned();
    let Some((kind, status)) = hq.desk.get(&id).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown desk item"})),
        );
    };
    if status != "waiting" {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": "already finished"})),
        );
    }
    if !allowed(&kind, &choice) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown decision for this item"})),
        );
    }
    hq.desk.insert(id, (kind, choice));
    hq.applied += 1;
    if std::mem::take(&mut hq.lose_next) {
        return (StatusCode::BAD_GATEWAY, Json(json!({"error": "lost"})));
    }
    (StatusCode::OK, Json(json!({"decided": true})))
}

async fn fake_snapshot(State(hq): State<Shared>) -> Json<Value> {
    let hq = hq.lock().unwrap();
    let waiting: Vec<Value> = hq
        .desk
        .iter()
        .filter(|(_, (_, s))| s == "waiting")
        .map(|(id, (kind, _))| json!({"id": id, "kind": kind, "text": "A fixture item"}))
        .collect();
    Json(
        json!({"desk": waiting, "digest": [], "last_event": hq.events.last().copied().unwrap_or(0)}),
    )
}

#[derive(serde::Deserialize)]
struct After {
    #[serde(default)]
    after: i64,
    before: Option<i64>,
    limit: Option<usize>,
}

async fn fake_events(State(hq): State<Shared>, Query(q): Query<After>) -> Json<Value> {
    let hq = hq.lock().unwrap();
    let rows: Vec<Value> = match q.before {
        Some(before) => {
            let older: Vec<i64> = hq
                .events
                .iter()
                .copied()
                .filter(|id| *id < before)
                .collect();
            let limit = q.limit.unwrap_or(50);
            older[older.len().saturating_sub(limit)..]
                .iter()
                .map(|id| json!({"id": id, "at": id * 10, "kind": "fixture", "data": {}}))
                .collect()
        }
        None => hq
            .events
            .iter()
            .filter(|id| **id > q.after)
            .take(500)
            .map(|id| json!({"id": id, "at": id * 10, "kind": "fixture", "data": {}}))
            .collect(),
    };
    Json(json!({"events": rows}))
}

/// A node whose HQ is the fake above.
async fn node_with_hq(hq: Shared) -> super::AppState {
    let fake = Router::new()
        .route("/v1/desk/{id}", get(fake_desk_item).post(fake_decide))
        .route("/v1/snapshot", get(fake_snapshot))
        .route("/v1/events", get(fake_events))
        .with_state(hq);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });
    test_state().with_hq(endora_infrastructure::HqClient::new(
        &url,
        "fixture-hq-token",
    ))
}

fn a_desk(items: &[(i64, &str, &str)]) -> Shared {
    let mut hq = Hq::default();
    for (id, kind, status) in items {
        hq.desk
            .insert(*id, ((*kind).to_owned(), (*status).to_owned()));
    }
    Arc::new(Mutex::new(hq))
}

// ---- decisions ------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_is_bound_to_the_person_the_item_and_the_choice_and_shows_each_stage() {
    let hq = a_desk(&[(1, "goal", "waiting")]);
    let state = node_with_hq(hq.clone()).await;
    state
        .store
        .add_session("owners-session", "full", now_ms(), 30 * DAY)
        .unwrap();
    let res = app(state)
        .oneshot(with(
            "POST",
            "/v1/decisions/1",
            "owners-session",
            r#"{"choice":"approve","request":"k-1","channel":"console"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let said = json_body(res).await;
    let d = &said["decision"];
    assert_eq!(said["duplicate"], false);
    assert_eq!(
        (d["id"].as_i64(), d["choice"].as_str()),
        (Some(1), Some("approve"))
    );
    assert_eq!(d["by"]["person"], "owner");
    assert!(d["by"]["session"].as_str().unwrap().starts_with("s-"));
    assert!(
        !said.to_string().contains("owners-session"),
        "never the token"
    );
    assert_eq!(
        (d["channel"].as_str(), d["request"].as_str()),
        (Some("console"), Some("k-1"))
    );
    for stage in ["requested", "accepted", "executed", "verified"] {
        assert!(d["stages"][stage]["at"].is_i64(), "{stage}: {d}");
    }
    assert_eq!(d["stages"]["executed"]["ok"], true);
    assert_eq!(d["stages"]["verified"]["ok"], true);
    assert_eq!(hq.lock().unwrap().applied, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_same_answer_twice_changes_nothing_and_a_different_one_is_refused() {
    let hq = a_desk(&[(1, "goal", "waiting")]);
    let router = app(node_with_hq(hq.clone()).await);
    let answer = |body: &'static str| router.clone().oneshot(post("/v1/decisions/1", body));
    assert_eq!(
        answer(r#"{"choice":"approve"}"#).await.unwrap().status(),
        StatusCode::OK
    );
    let again = answer(r#"{"choice":"approve"}"#).await.unwrap();
    assert_eq!(again.status(), StatusCode::OK);
    let again = json_body(again).await;
    assert_eq!(again["duplicate"], true);
    assert_eq!(again["decision"]["stages"]["verified"]["ok"], true);
    let other = answer(r#"{"choice":"decline"}"#).await.unwrap();
    assert_eq!(other.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(other).await["decision"]["choice"], "approve");
    assert_eq!(hq.lock().unwrap().applied, 1, "applied once");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_choice_the_item_does_not_allow_is_refused_and_nothing_is_recorded() {
    let hq = a_desk(&[(2, "goal", "waiting")]);
    let router = app(node_with_hq(hq.clone()).await);
    let res = router
        .clone()
        .oneshot(post("/v1/decisions/2", r#"{"choice":"seen"}"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let kept = json_body(
        router
            .clone()
            .oneshot(signed_get("/v1/decisions/2"))
            .await
            .unwrap(),
    )
    .await;
    assert!(kept["decision"].is_null(), "{kept}");
    assert_eq!(kept["desk"]["status"], "waiting");
    // It can still be answered properly afterwards.
    let res = router
        .oneshot(post("/v1/decisions/2", r#"{"choice":"decline"}"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_item_is_not_found_and_one_decided_elsewhere_is_a_conflict() {
    let hq = a_desk(&[(3, "goal", "decline")]);
    let router = app(node_with_hq(hq.clone()).await);
    let missing = router
        .clone()
        .oneshot(post("/v1/decisions/99", r#"{"choice":"approve"}"#))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let elsewhere = router
        .clone()
        .oneshot(post("/v1/decisions/3", r#"{"choice":"approve"}"#))
        .await
        .unwrap();
    assert_eq!(elsewhere.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(elsewhere).await["desk"]["status"], "decline");
    assert_eq!(hq.lock().unwrap().applied, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn hq_failing_leaves_the_answer_requested_and_sending_it_again_finishes_it_once() {
    let hq = a_desk(&[(4, "goal", "waiting")]);
    hq.lock().unwrap().fail_next = true;
    let router = app(node_with_hq(hq.clone()).await);
    let first = router
        .clone()
        .oneshot(post("/v1/decisions/4", r#"{"choice":"approve"}"#))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::BAD_GATEWAY);
    let kept = json_body(
        router
            .clone()
            .oneshot(signed_get("/v1/decisions/4"))
            .await
            .unwrap(),
    )
    .await;
    assert!(kept["decision"]["stages"]["requested"]["at"].is_i64());
    assert!(kept["decision"]["stages"]["accepted"].is_null());
    let retried = router
        .clone()
        .oneshot(post("/v1/decisions/4", r#"{"choice":"approve"}"#))
        .await
        .unwrap();
    assert_eq!(retried.status(), StatusCode::OK);
    assert_eq!(hq.lock().unwrap().applied, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_hq_applied_but_whose_reply_was_lost_is_not_applied_again() {
    let hq = a_desk(&[(5, "question", "waiting")]);
    hq.lock().unwrap().lose_next = true;
    let router = app(node_with_hq(hq.clone()).await);
    let first = router
        .clone()
        .oneshot(post(
            "/v1/decisions/5",
            r#"{"choice":"seen","words":"Fixture words."}"#,
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::BAD_GATEWAY);
    let retried = router
        .clone()
        .oneshot(post(
            "/v1/decisions/5",
            r#"{"choice":"seen","words":"Fixture words."}"#,
        ))
        .await
        .unwrap();
    assert_eq!(retried.status(), StatusCode::OK);
    let d = json_body(retried).await;
    assert_eq!(d["decision"]["stages"]["verified"]["ok"], true, "{d}");
    assert_eq!(hq.lock().unwrap().applied, 1, "never twice");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_decisions_list_shows_what_waits_and_what_was_answered() {
    let hq = a_desk(&[(6, "goal", "waiting"), (7, "goal", "waiting")]);
    let router = app(node_with_hq(hq).await);
    router
        .clone()
        .oneshot(post("/v1/decisions/7", r#"{"choice":"approve"}"#))
        .await
        .unwrap();
    let list = json_body(router.oneshot(signed_get("/v1/decisions")).await.unwrap()).await;
    let pending: Vec<i64> = list["pending"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_i64().unwrap())
        .collect();
    assert_eq!(pending, [6]);
    assert_eq!(list["answered"][0]["id"], 7);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_checking_credential_cannot_answer() {
    let hq = a_desk(&[(8, "goal", "waiting")]);
    let state = node_with_hq(hq.clone()).await;
    state
        .store
        .add_session("smoke-only", "checks", now_ms(), 30 * DAY)
        .unwrap();
    let res = app(state)
        .oneshot(with(
            "POST",
            "/v1/decisions/8",
            "smoke-only",
            r#"{"choice":"approve"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(hq.lock().unwrap().applied, 0);
}

// ---- events ---------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn events_resume_from_a_cursor_and_say_when_some_were_pruned() {
    let hq = a_desk(&[]);
    hq.lock().unwrap().events = vec![5, 6, 7];
    let router = app(node_with_hq(hq).await);
    let from = |after: i64| {
        router
            .clone()
            .oneshot(signed_get(&format!("/v1/events?after={after}")))
    };
    let caught = json_body(from(5).await.unwrap()).await;
    let ids: Vec<i64> = caught["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [6, 7]);
    assert_eq!(
        (caught["cursor"].as_i64(), caught["gap"].as_bool()),
        (Some(7), Some(false))
    );
    assert_eq!(caught["more"], false);
    let pruned = json_body(from(2).await.unwrap()).await;
    assert_eq!(
        pruned["gap"], true,
        "4 and below are gone: take a new snapshot"
    );
    let idle = json_body(from(7).await.unwrap()).await;
    assert_eq!(
        (idle["cursor"].as_i64(), idle["gap"].as_bool()),
        (Some(7), Some(false))
    );
    let older = json_body(
        router
            .clone()
            .oneshot(signed_get("/v1/events?before=7&limit=1"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(older["events"][0]["id"], 6);
}

#[test]
fn a_page_of_events_says_its_cursor_gap_and_more() {
    use super::console::page_after;
    let ids = |v: &[i64]| v.iter().map(|id| json!({"id": id})).collect::<Vec<_>>();
    assert_eq!(page_after(3, &ids(&[4, 5])), (5, false, false));
    assert_eq!(page_after(3, &ids(&[6])), (6, true, false));
    assert_eq!(
        page_after(0, &ids(&[9])),
        (9, false, false),
        "from nothing, no gap"
    );
    assert_eq!(page_after(8, &[]), (8, false, false));
    let full: Vec<i64> = (1..=500).collect();
    assert_eq!(page_after(0, &ids(&full)), (500, false, true));
}

/// Reads what an event stream has sent until `want` appears or a second passes.
async fn stream_until(res: axum::response::Response, want: &str) -> String {
    let mut body = res.into_body();
    let mut seen = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while !seen.contains(want) {
        let Ok(Some(Ok(frame))) = tokio::time::timeout_at(deadline, body.frame()).await else {
            break;
        };
        if let Ok(bytes) = frame.into_data() {
            seen.push_str(&String::from_utf8_lossy(&bytes));
        }
    }
    seen
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stream_catches_up_from_the_last_event_id_with_each_id_marked() {
    let hq = a_desk(&[]);
    hq.lock().unwrap().events = vec![4, 5, 6];
    let router = app(node_with_hq(hq.clone()).await);
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/events/stream")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("last-event-id", "4")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let seen = stream_until(res, "id: 6").await;
    assert!(seen.contains("event: hello"), "{seen}");
    assert!(seen.contains("id: 5") && seen.contains("id: 6"), "{seen}");
    assert!(!seen.contains("id: 4"), "already had it: {seen}");
    assert!(seen.contains("event: hq"), "{seen}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stream_says_when_its_cursor_is_older_than_what_hq_keeps() {
    let hq = a_desk(&[]);
    hq.lock().unwrap().events = vec![10, 11];
    let router = app(node_with_hq(hq).await);
    let res = router
        .oneshot(signed_get("/v1/events/stream?after=3"))
        .await
        .unwrap();
    let seen = stream_until(res, "event: gap").await;
    assert!(seen.contains("event: gap"), "{seen}");
}

#[tokio::test]
async fn the_stream_takes_no_token_in_the_query() {
    // The query-token exception stays only on the old feed (`/v1/activity/stream`).
    let res = app(test_state())
        .oneshot(
            Request::builder()
                .uri(format!("/v1/events/stream?token={TEST_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
