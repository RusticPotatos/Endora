//! The runner's door (spec section 15, phase 4): how `endora-runner` asks HQ for a turn to
//! start, reports how each ended, and hears which to stop. Signed with the runner's own
//! token, never the node's: neither token opens the other's routes, nor a worker's.
//!
//! With the demo on, HQ runs no real turns: these routes answer a signed runner 404.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Request, State as Extract};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;

use endora_team::application::ports::{StoreError, TeamStore as _};
use endora_team::domain::RunId;
use endora_team::infrastructure::RemoteRunner;

use crate::api::{State, bearer, same};

/// The runner's routes.
pub(crate) fn routes(state: State) -> Router<State> {
    Router::new()
        .route("/v1/runner/next", post(next))
        .route("/v1/runner/{run}/ended", post(ended))
        .route("/v1/runner/stops", get(stops))
        .route_layer(middleware::from_fn_with_state(state, signed_by_the_runner))
}

async fn signed_by_the_runner(
    Extract(state): Extract<State>,
    request: Request,
    next: Next,
) -> Response {
    // With no runner's token set, the door stays shut.
    let signed = state
        .runner_token
        .as_ref()
        .is_some_and(|token| same(bearer(&request).as_bytes(), token.as_bytes()));
    if !signed {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "not signed by the runner" })),
        )
            .into_response();
    }
    next.run(request).await
}

/// The demo is on: the simulated runner takes the turns, and a real one has none to take.
fn no_runner() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "the demo is on: HQ runs no real turns" })),
    )
        .into_response()
}

#[derive(Deserialize)]
struct Room {
    /// How many more turns the runner can take now.
    free: u32,
}

/// The oldest turn waiting to start, handed out now: its run, this attempt's lease, its token,
/// its member's name, role and brain, its task, and how hard its Claude tries (`null` for the
/// house's model, or the model's own). Before any turn, a check job waiting for the dev node
/// (ADR 0115): `kind: "check"`, its id, the job's own token, the branch and its head. 204 when
/// none waits, or the runner has no room.
async fn next(Extract(state): Extract<State>, Json(room): Json<Room>) -> Response {
    if state.hq.runner().remote().is_none() {
        return no_runner();
    }
    if room.free == 0 {
        return StatusCode::NO_CONTENT.into_response();
    }
    let checking = state.hq.clone();
    let check = tokio::task::spawn_blocking(move || {
        let check = checking.next_check()?;
        let branch = match &check {
            Some(c) => checking
                .store()
                .branches()?
                .into_iter()
                .find(|b| b.id == c.branch),
            None => None,
        };
        Ok::<_, endora_team::application::EngineError>(check.zip(branch))
    })
    .await;
    match check {
        Ok(Ok(Some((check, branch)))) => {
            return Json(json!({
                "kind": "check", "check": check.id, "token": check.token,
                "task": check.task.0, "branch": branch.name, "head": branch.head,
            }))
            .into_response();
        }
        Ok(Ok(None)) => {}
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
    let hq = state.hq.clone();
    let handed = tokio::task::spawn_blocking(move || {
        // The members first: a turn handed out is the runner's, so nothing may fail after.
        let members = hq.store().members()?;
        let turn = hq.runner().remote().and_then(RemoteRunner::next);
        Ok::<_, StoreError>(turn.map(|handed| {
            let member = members.into_iter().find(|m| m.id == handed.turn.member);
            (handed, member)
        }))
    })
    .await;
    match handed {
        Ok(Ok(Some((handed, member)))) => Json(json!({
            "run": handed.turn.run.0,
            "lease": handed.lease,
            "token": handed.turn.token,
            "member": member.as_ref().map_or("", |m| m.name.as_str()),
            "role": member.as_ref().map_or("", |m| m.role.name()),
            // What the member thinks with: the runner points the worker at the matching relay.
            "brain": member.as_ref().map_or("local", |m| m.brain.name()),
            "task": handed.turn.task.map(|t| t.0),
            "effort": handed.turn.effort.map(endora_team::domain::Effort::name),
        }))
        .into_response(),
        Ok(Ok(None)) => StatusCode::NO_CONTENT.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
struct Ended {
    succeeded: bool,
    /// The lease the turn was handed out with.
    lease: u64,
    /// The worker's exit code, from a runner that says it: 3 when HQ or the relay had
    /// already ended the turn (its budget, the kill switch, a stale token).
    #[serde(default)]
    code: Option<i64>,
    /// Why, in the runner's words: the exit code's meaning and the worker's last words on
    /// stderr. An older runner says neither, and the engine says "the turn failed".
    #[serde(default)]
    why: Option<String>,
}

/// The worker's exit code for a turn HQ or the relay had ended before the worker did.
const OVER: i64 = 3;

/// The reason kept on the run, from what the runner said: a turn that was over before it
/// ended says so, rather than that it failed.
fn reason(code: Option<i64>, why: Option<&str>) -> Option<String> {
    let why = why.map(str::trim).filter(|w| !w.is_empty())?;
    if code == Some(OVER) {
        return Some(format!("the turn was over: {why}"));
    }
    Some(format!("the turn failed: {why}"))
}

/// How a turn the runner was handed ended. The engine's next tick ends the run. A report from
/// an attempt that is no longer the run's current one (its lease is out of date) is refused,
/// 409, and changes nothing.
async fn ended(
    Extract(state): Extract<State>,
    Path(run): Path<i64>,
    Json(req): Json<Ended>,
) -> Response {
    let Some(remote) = state.hq.runner().remote() else {
        return no_runner();
    };
    let why = reason(req.code, req.why.as_deref());
    if !remote.report(RunId(run), req.lease, req.succeeded, why.as_deref()) {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "not a turn this runner holds, under that lease" })),
        )
            .into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
pub(crate) struct CheckReport {
    /// The job's token, when not in the `Authorization` header.
    #[serde(default)]
    token: String,
    ok: bool,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    report: String,
}

/// A check job's report (ADR 0115), with the job's own token (as the bearer, or in the body):
/// pass or fail, a line, and the commands' last words. Taken once; a token that names no
/// check is 404, a check reported already 409.
pub(crate) async fn check_done(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    request: Request,
) -> Response {
    let bearer = bearer(&request).to_owned();
    let Ok(body) = axum::body::to_bytes(request.into_body(), 1 << 20).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let Ok(report) = serde_json::from_slice::<CheckReport>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "a report says ok, summary and report" })),
        )
            .into_response();
    };
    let token = if bearer.is_empty() {
        report.token.clone()
    } else {
        bearer
    };
    if token.len() < 32 {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "not signed with a check's token" })),
        )
            .into_response();
    }
    let hq = state.hq.clone();
    let taken = tokio::task::spawn_blocking(move || {
        hq.check_done(id, &token, report.ok, &report.summary, &report.report)
    })
    .await;
    match taken {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(endora_team::application::EngineError::Refused(
            endora_team::domain::Refusal::Finished,
        ))) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "that check was reported already" })),
        )
            .into_response(),
        Ok(Err(_)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no check has that token" })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The turns to stop, each given once.
async fn stops(Extract(state): Extract<State>) -> Response {
    let Some(remote) = state.hq.runner().remote() else {
        return no_runner();
    };
    let runs: Vec<i64> = remote.stops().iter().map(|r| r.0).collect();
    Json(json!({ "runs": runs })).into_response()
}
