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

/// The oldest turn waiting to start, handed out now: its run, its token, its member's name and
/// role, and its task. 204 when none waits, or the runner has no room.
async fn next(Extract(state): Extract<State>, Json(room): Json<Room>) -> Response {
    if state.hq.runner().remote().is_none() {
        return no_runner();
    }
    if room.free == 0 {
        return StatusCode::NO_CONTENT.into_response();
    }
    let hq = state.hq.clone();
    let handed = tokio::task::spawn_blocking(move || {
        // The members first: a turn handed out is the runner's, so nothing may fail after.
        let members = hq.store().members()?;
        let turn = hq.runner().remote().and_then(RemoteRunner::next);
        Ok::<_, StoreError>(turn.map(|turn| {
            let member = members.into_iter().find(|m| m.id == turn.member);
            (turn, member)
        }))
    })
    .await;
    match handed {
        Ok(Ok(Some((turn, member)))) => Json(json!({
            "run": turn.run.0,
            "token": turn.token,
            "member": member.as_ref().map_or("", |m| m.name.as_str()),
            "role": member.as_ref().map_or("", |m| m.role.name()),
            "task": turn.task.map(|t| t.0),
        }))
        .into_response(),
        Ok(Ok(None)) => StatusCode::NO_CONTENT.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
struct Ended {
    succeeded: bool,
}

/// How a turn the runner was handed ended. The engine's next tick ends the run.
async fn ended(
    Extract(state): Extract<State>,
    Path(run): Path<i64>,
    Json(req): Json<Ended>,
) -> Response {
    let Some(remote) = state.hq.runner().remote() else {
        return no_runner();
    };
    remote.ended(RunId(run), req.succeeded);
    StatusCode::NO_CONTENT.into_response()
}

/// The turns to stop, each given once.
async fn stops(Extract(state): Extract<State>) -> Response {
    let Some(remote) = state.hq.runner().remote() else {
        return no_runner();
    };
    let runs: Vec<i64> = remote.stops().iter().map(|r| r.0).collect();
    Json(json!({ "runs": runs })).into_response()
}
