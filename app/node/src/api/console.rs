//! The routes the Core ↔ Console contract adds (ADR 0145; `docs/api/console-v1.md`): when a
//! session ends and ending one, the owner's answers to desk items, and one event stream.
//!
//! Console holds no authority. Every route here sits behind the node's token gate like any
//! other, and an answer is bound here, on Core, to the session that gave it, the exact desk
//! item and a choice HQ allows. What the answer did is kept in stages, so a client shows
//! requested, accepted, executed and verified separately, and a second press or a retry after a
//! dropped connection never applies it twice.

use std::collections::VecDeque;
use std::convert::Infallible;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use endora_application::Clock as _;
use endora_infrastructure::{DecisionAnswer, HqError, SessionSeen};
use futures_util::stream::{Stream, unfold};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use super::AppState;
use super::auth::{SESSIONS_IDLE_MS, SESSIONS_LAST_MS};

/// The contract's version, said at `/health` as `api`. The minor number grows with each
/// addition; the major never changes within `/v1`.
pub(super) const CONTRACT_VERSION: &str = "1.0";

/// How often the stream asks HQ for new events when nothing on the node has changed.
const POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// How long the stream waits before asking an unreachable HQ again.
const HQ_DOWN_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// The most events HQ hands over at once (`app/hq`'s `/v1/events`).
const HQ_EVENTS_MOST: usize = 500;

type Refusal = (StatusCode, Json<Value>);
type Reply = Result<Json<Value>, Refusal>;

fn refuse(status: StatusCode, error: impl Into<String>) -> Refusal {
    (status, Json(json!({ "error": error.into() })))
}

fn now_ms(state: &AppState) -> i64 {
    state.clock.now().unix_millis()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, Refusal> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| refuse(StatusCode::INTERNAL_SERVER_ERROR, "a worker thread failed"))
}

fn broken(e: impl std::fmt::Display) -> Refusal {
    refuse(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ---- who is asking --------------------------------------------------------------------------

/// The credential a request carries in its header: the only place this module reads one.
fn bearer(headers: &HeaderMap) -> String {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map_or_else(String::new, |t| t.trim().to_owned())
}

/// Who a request is: the bootstrap token (the recovery path), or a live session.
enum Who {
    Bootstrap,
    Session(SessionSeen),
}

async fn who(state: &AppState, headers: &HeaderMap) -> Result<Option<Who>, Refusal> {
    let token = bearer(headers);
    if !state.token.is_empty() && crate::auth::same_secret(&token, &state.token) {
        return Ok(Some(Who::Bootstrap));
    }
    let store = state.store.clone();
    let now = now_ms(state);
    let seen =
        blocking(move || store.session_seen(&token, now, SESSIONS_LAST_MS, SESSIONS_IDLE_MS))
            .await?
            .map_err(broken)?;
    Ok(seen.map(Who::Session))
}

// ---- sessions -------------------------------------------------------------------------------

/// When this session ends: thirty days from sign-in, and a console session after seven days
/// unused. The bootstrap token never ends.
pub(super) async fn session_current(State(state): State<AppState>, headers: HeaderMap) -> Reply {
    match who(&state, &headers).await? {
        Some(Who::Bootstrap) => Ok(Json(json!({
            "scope": "full", "bootstrap": true,
            "issued_ms": null, "expires_ms": null, "idle_expires_ms": null,
        }))),
        Some(Who::Session(s)) => Ok(Json(json!({
            "scope": s.scope, "bootstrap": false, "issued_ms": s.issued_ms,
            "expires_ms": s.issued_ms + SESSIONS_LAST_MS,
            "idle_expires_ms": (s.scope == "full").then_some(s.last_seen_ms + SESSIONS_IDLE_MS),
        }))),
        None => Err(refuse(StatusCode::UNAUTHORIZED, "no session")),
    }
}

/// Ends the session that asks, at once: its next request is refused.
pub(super) async fn sign_out(State(state): State<AppState>, headers: HeaderMap) -> Reply {
    if matches!(who(&state, &headers).await?, Some(Who::Bootstrap)) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "the bootstrap token is the recovery path, not a session: it cannot be signed out",
        ));
    }
    let token = bearer(&headers);
    let store = state.store.clone();
    let ended = blocking(move || store.end_session(&token))
        .await?
        .map_err(broken)?;
    if ended {
        super::platform::record_event(&state.events, &state.clock, "a console session signed out");
    }
    Ok(Json(json!({ "signed_out": ended })))
}

/// Ends every console session, the one asking included. A checking or dashboard credential is
/// left alone: it is a separate thing, made and kept on purpose.
pub(super) async fn sign_out_everywhere(State(state): State<AppState>) -> Reply {
    let store = state.store.clone();
    let ended = blocking(move || store.end_sessions_of("full", ""))
        .await?
        .map_err(broken)?;
    super::platform::record_event(
        &state.events,
        &state.clock,
        &format!("every console session was signed out ({ended})"),
    );
    Ok(Json(json!({ "ended": ended })))
}

// ---- decisions ------------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct Answering {
    choice: String,
    #[serde(default)]
    words: String,
    #[serde(default)]
    request: String,
    #[serde(default)]
    channel: String,
}

fn word_like(text: &str, most: usize, extra: &[u8]) -> bool {
    !text.is_empty()
        && text.len() <= most
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || extra.contains(&b))
}

/// Whether HQ's status for an item records `choice`. An approved approval becomes `ran` once
/// the node has run it and told HQ.
fn recorded_as(choice: &str, status: &str) -> bool {
    status == choice || (choice == "approve" && status == "ran")
}

fn stage(at: Option<i64>) -> Value {
    at.map_or(Value::Null, |at| json!({ "at": at }))
}

fn outcome(kept: Option<&(i64, bool, String)>, word: &str) -> Value {
    kept.map_or(
        Value::Null,
        |(at, ok, said)| json!({ "at": at, "ok": ok, word: said }),
    )
}

/// An answer as the contract shows it.
fn shown(a: &DecisionAnswer) -> Value {
    json!({
        "id": a.desk, "kind": a.kind, "choice": a.choice, "words": a.words,
        "by": { "person": a.person, "session": a.session },
        "channel": a.channel, "request": a.request,
        "stages": {
            "requested": stage(Some(a.requested_ms)),
            "accepted": stage(a.accepted_ms),
            "executed": outcome(a.executed.as_ref(), "said"),
            "verified": outcome(a.verified.as_ref(), "how"),
        },
    })
}

async fn kept(state: &AppState, desk: i64) -> Result<Option<DecisionAnswer>, Refusal> {
    let store = state.store.clone();
    blocking(move || store.decision(desk))
        .await?
        .map_err(broken)
}

async fn kept_shown(state: &AppState, desk: i64) -> Result<Value, Refusal> {
    Ok(kept(state, desk).await?.as_ref().map_or(Value::Null, shown))
}

/// HQ's desk item by id, decided or not.
async fn desk_item(state: &AppState, desk: i64) -> Result<Result<Value, HqError>, Refusal> {
    let hq = super::hq::client(state)?;
    blocking(move || hq.get(&format!("/v1/desk/{desk}"))).await
}

/// The owner answers a desk item: bound to the session, the item and a choice HQ allows;
/// idempotent; and kept in stages.
pub(super) async fn decide(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(desk): Path<i64>,
    Json(req): Json<Answering>,
) -> Reply {
    if !word_like(&req.choice, 32, b"_") {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "a choice is one plain word",
        ));
    }
    let words = req.words.trim().to_owned();
    if words.chars().count() > 4000 {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "at most 4000 characters of words",
        ));
    }
    if req.request.len() > 64 || req.request.bytes().any(|b| !b.is_ascii_graphic()) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "a request key is up to 64 plain characters",
        ));
    }
    let channel = if req.channel.is_empty() {
        "console".to_owned()
    } else {
        req.channel.clone()
    };
    if !word_like(&channel, 24, b"-") {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "a channel is one plain word",
        ));
    }
    super::hq::client(&state)?;
    let session = match who(&state, &headers).await? {
        Some(Who::Bootstrap) => "bootstrap".to_owned(),
        Some(Who::Session(s)) => s.reference,
        None => return Err(refuse(StatusCode::UNAUTHORIZED, "no session")),
    };
    let answer = DecisionAnswer {
        desk,
        choice: req.choice.clone(),
        words: words.clone(),
        person: "owner".to_owned(),
        session,
        channel,
        request: req.request.clone(),
        requested_ms: now_ms(&state),
        ..DecisionAnswer::default()
    };
    let store = state.store.clone();
    let reserving = answer.clone();
    let reserved = blocking(move || store.reserve_decision(&reserving))
        .await?
        .map_err(broken)?;
    if !reserved {
        let existing = kept(&state, desk).await?.ok_or_else(|| {
            refuse(
                StatusCode::CONFLICT,
                "that item was being answered; ask again",
            )
        })?;
        if existing.choice != answer.choice {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": format!("that item is already answered: {}", existing.choice),
                    "decision": shown(&existing),
                })),
            ));
        }
        if existing.accepted_ms.is_some() {
            if !existing.verified.as_ref().is_some_and(|v| v.1) {
                verify(&state, desk, &existing.choice).await?;
            }
            return Ok(Json(
                json!({ "decision": kept_shown(&state, desk).await?, "duplicate": true }),
            ));
        }
        // Reserved by an earlier press that never reached HQ: carry it through now.
    }
    let duplicate = !reserved;
    apply(&state, desk, &answer.choice, &words, duplicate).await
}

async fn release(state: &AppState, desk: i64) -> Result<(), Refusal> {
    let store = state.store.clone();
    blocking(move || store.release_decision(desk))
        .await?
        .map_err(broken)
}

async fn unreachable(state: &AppState, desk: i64, why: String) -> Refusal {
    let decision = kept_shown(state, desk).await.unwrap_or(Value::Null);
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({
            "error": format!("HQ did not answer ({why}); send the same answer again to finish it"),
            "decision": decision,
        })),
    )
}

/// HQ recorded the answer: accepted, and (where nothing else runs) executed with it.
async fn mark(
    state: &AppState,
    desk: i64,
    kind: String,
    executed: (bool, String),
) -> Result<(), Refusal> {
    let store = state.store.clone();
    let at = now_ms(state);
    blocking(move || {
        store.decision_accepted(desk, &kind, at)?;
        store.decision_executed(desk, at, executed.0, &executed.1)
    })
    .await?
    .map_err(broken)
}

async fn apply(state: &AppState, desk: i64, choice: &str, words: &str, duplicate: bool) -> Reply {
    let item = match desk_item(state, desk).await? {
        Ok(item) => item,
        Err(HqError::Answered { status: 404, .. }) => {
            release(state, desk).await?;
            return Err(refuse(
                StatusCode::NOT_FOUND,
                "there is no such item on the desk",
            ));
        }
        Err(e) => return Err(unreachable(state, desk, e.to_string()).await),
    };
    let kind = item["kind"].as_str().unwrap_or_default().to_owned();
    let status = item["status"].as_str().unwrap_or_default().to_owned();
    if status != "waiting" {
        return settled_elsewhere(state, desk, choice, kind, item).await;
    }
    let decided = super::hq::decide_on_desk(state, desk, choice, words).await;
    match decided {
        Ok(Json(answer)) => {
            let executed = if let Some(pushed) = answer["pushed"].as_bool() {
                (
                    pushed,
                    answer["said"].as_str().unwrap_or_default().to_owned(),
                )
            } else if let Some(ran) = answer["ran"].as_bool() {
                let said = if ran {
                    "what was approved ran"
                } else if choice == "approve" {
                    "it was approved but did not run"
                } else {
                    "nothing was run"
                };
                (ran || choice != "approve", said.to_owned())
            } else {
                (true, "HQ applied it as it recorded the decision".to_owned())
            };
            mark(state, desk, kind, executed).await?;
        }
        Err((StatusCode::CONFLICT, _)) => {
            // Decided between the look and the answer: by this same answer, or by another.
            let again = match desk_item(state, desk).await? {
                Ok(again) => again,
                Err(e) => return Err(unreachable(state, desk, e.to_string()).await),
            };
            return settled_elsewhere(state, desk, choice, kind, again).await;
        }
        Err((status, Json(said))) if status.is_client_error() => {
            release(state, desk).await?;
            let why = said["error"].as_str().unwrap_or("refused");
            return Err(refuse(
                StatusCode::BAD_REQUEST,
                format!("{choice} is not a choice this item allows ({why})"),
            ));
        }
        Err((_, Json(said))) => {
            let why = said["error"].as_str().unwrap_or("no answer").to_owned();
            return Err(unreachable(state, desk, why).await);
        }
    }
    verify(state, desk, choice).await?;
    Ok(Json(
        json!({ "decision": kept_shown(state, desk).await?, "duplicate": duplicate }),
    ))
}

/// The item was no longer waiting: if HQ records this choice, it was this answer (a retry
/// after a lost reply, or a concurrent press), so it is accepted without asking HQ again;
/// otherwise somebody decided it differently and nothing is recorded here.
async fn settled_elsewhere(
    state: &AppState,
    desk: i64,
    choice: &str,
    kind: String,
    item: Value,
) -> Reply {
    let status = item["status"].as_str().unwrap_or_default();
    if !recorded_as(choice, status) {
        release(state, desk).await?;
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!("that item was already decided: {status}"),
                "desk": item,
                "decision": kept_shown(state, desk).await?,
            })),
        ));
    }
    mark(
        state,
        desk,
        kind,
        (
            true,
            "HQ had already recorded it, so it was not applied again".to_owned(),
        ),
    )
    .await?;
    verify(state, desk, choice).await?;
    Ok(Json(
        json!({ "decision": kept_shown(state, desk).await?, "duplicate": true }),
    ))
}

/// Reads the item back from HQ and writes down whether it records the choice and whether what
/// ran succeeded. An HQ that cannot be read leaves the stage open, not failed.
async fn verify(state: &AppState, desk: i64, choice: &str) -> Result<(), Refusal> {
    let Ok(item) = desk_item(state, desk).await? else {
        return Ok(());
    };
    let status = item["status"].as_str().unwrap_or_default().to_owned();
    let executed_ok = kept(state, desk)
        .await?
        .and_then(|a| a.executed)
        .is_some_and(|e| e.1);
    let ok = recorded_as(choice, &status) && executed_ok;
    let how = if executed_ok {
        format!("read back from HQ's desk: recorded as {status}")
    } else {
        format!("read back from HQ's desk: recorded as {status}, but what it does did not succeed")
    };
    let store = state.store.clone();
    let at = now_ms(state);
    blocking(move || store.decision_verified(desk, at, ok, &how))
        .await?
        .map_err(broken)
}

/// What waits on the owner (the desk and *While you were away*, as the snapshot shows them),
/// and the latest answers with their stages.
pub(super) async fn decisions(State(state): State<AppState>) -> Reply {
    let hq = super::hq::client(&state)?;
    let snap = blocking(move || hq.get("/v1/snapshot"))
        .await?
        .map_err(super::hq::failed)?;
    let mut pending: Vec<Value> = Vec::new();
    for part in ["desk", "digest"] {
        pending.extend(snap[part].as_array().cloned().unwrap_or_default());
    }
    let store = state.store.clone();
    let answered: Vec<Value> = blocking(move || store.latest_decisions(50))
        .await?
        .map_err(broken)?
        .iter()
        .map(shown)
        .collect();
    Ok(Json(json!({ "pending": pending, "answered": answered })))
}

/// One item: the answer recorded here, if any, and the desk item as HQ holds it now.
pub(super) async fn decision(State(state): State<AppState>, Path(desk): Path<i64>) -> Reply {
    let item = match desk_item(&state, desk).await? {
        Ok(item) => item,
        Err(HqError::Answered { status: 404, .. }) => Value::Null,
        Err(e) => return Err(super::hq::failed(e)),
    };
    Ok(Json(
        json!({ "decision": kept_shown(&state, desk).await?, "desk": item }),
    ))
}

// ---- events ---------------------------------------------------------------------------------

/// What a page of HQ's events after `after` says: the new cursor, whether events between were
/// pruned (the first is not the next id), and whether there may be more to ask for.
pub(super) fn page_after(after: i64, events: &[Value]) -> (i64, bool, bool) {
    let id = |e: &Value| e["id"].as_i64();
    let cursor = events.last().and_then(id).unwrap_or(after);
    let gap = after > 0
        && events
            .first()
            .and_then(id)
            .is_some_and(|first| first > after + 1);
    (cursor, gap, events.len() >= HQ_EVENTS_MOST)
}

#[derive(Deserialize)]
pub(super) struct Cursor {
    after: Option<i64>,
    before: Option<i64>,
    limit: Option<usize>,
}

/// HQ's events after a cursor, with what a client needs to catch up; or, with `before`, the
/// older ones, for paging back.
pub(super) async fn events(State(state): State<AppState>, Query(q): Query<Cursor>) -> Reply {
    let hq = super::hq::client(&state)?;
    if let Some(before) = q.before {
        let limit = q.limit.unwrap_or(50).min(200);
        let got = blocking(move || hq.get(&format!("/v1/events?before={before}&limit={limit}")))
            .await?
            .map_err(super::hq::failed)?;
        let events = got["events"].as_array().cloned().unwrap_or_default();
        let cursor = events
            .first()
            .and_then(|e| e["id"].as_i64())
            .unwrap_or(before);
        return Ok(Json(json!({ "events": events, "cursor": cursor })));
    }
    let after = q.after.unwrap_or(0).max(0);
    let got = blocking(move || hq.get(&format!("/v1/events?after={after}")))
        .await?
        .map_err(super::hq::failed)?;
    let events = got["events"].as_array().cloned().unwrap_or_default();
    let (cursor, gap, more) = page_after(after, &events);
    Ok(Json(
        json!({ "events": events, "cursor": cursor, "gap": gap, "more": more }),
    ))
}

/// Where a stream is: what it has sent, and what it has still to send.
struct Tail {
    hq: std::sync::Arc<endora_infrastructure::HqClient>,
    changes: broadcast::Receiver<()>,
    cursor: i64,
    greeted: bool,
    down: bool,
    queue: VecDeque<Event>,
}

impl Tail {
    /// Asks HQ for what came after the cursor and queues it. True if HQ answered.
    async fn fetch(&mut self) -> bool {
        let hq = self.hq.clone();
        let after = self.cursor;
        let got =
            tokio::task::spawn_blocking(move || hq.get(&format!("/v1/events?after={after}"))).await;
        let Ok(Ok(page)) = got else {
            if !self.down {
                self.down = true;
                self.queue.push_back(
                    Event::default()
                        .event("unavailable")
                        .data("HQ did not answer"),
                );
            }
            return false;
        };
        self.down = false;
        let events = page["events"].as_array().cloned().unwrap_or_default();
        let (cursor, gap, _) = page_after(after, &events);
        if gap {
            self.queue.push_back(
                Event::default()
                    .event("gap")
                    .data(json!({ "cursor": cursor }).to_string()),
            );
        }
        for e in events {
            let id = e["id"].as_i64().unwrap_or_default();
            self.queue.push_back(
                Event::default()
                    .event("hq")
                    .id(id.to_string())
                    .data(e.to_string()),
            );
        }
        self.cursor = cursor;
        true
    }
}

/// One stream for Console: `hello` with the cursor, then HQ's events from the cursor on, each
/// with its id (so a reconnect with `Last-Event-ID` catches up exactly), `changed` when
/// something outside HQ's record changed, and `gap` when the cursor is older than what HQ keeps.
/// Signed with the header only: the query-token exception is the old feed's alone.
pub(super) async fn events_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<Cursor>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, Refusal> {
    let hq = super::hq::client(&state)?;
    let resumed = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<i64>().ok());
    let cursor = resumed.or(q.after).unwrap_or(0).max(0);
    let tail = Tail {
        hq,
        changes: state.changes.subscribe(),
        cursor,
        greeted: false,
        down: false,
        queue: VecDeque::new(),
    };
    let stream = unfold(tail, |mut tail| async move {
        loop {
            if !tail.greeted {
                tail.greeted = true;
                let hello = json!({ "cursor": tail.cursor, "api": CONTRACT_VERSION }).to_string();
                return Some((Ok(Event::default().event("hello").data(hello)), tail));
            }
            if let Some(next) = tail.queue.pop_front() {
                return Some((Ok(next), tail));
            }
            let answered = tail.fetch().await;
            if !tail.queue.is_empty() {
                continue;
            }
            let wait = if answered { POLL } else { HQ_DOWN_WAIT };
            tokio::select! {
                changed = tail.changes.recv() => match changed {
                    Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        return Some((Ok(Event::default().event("changed").data("changed")), tail));
                    }
                    Err(broadcast::error::RecvError::Closed) => return None,
                },
                () = tokio::time::sleep(wait) => {}
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
