//! The drain before a restart (ADR 0134): announced by whoever is about to restart HQ or the
//! runner (a deploy, the NAS's shutdown), read while it waits for the turns under way, and lifted
//! when it is done. Signed with HQ's own token like the owner's actions; it is HQ-only, and the
//! node does not proxy it.

use axum::Json;
use axum::Router;
use axum::extract::State as Extract;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::Value;

use super::{Failed, State, blocking};

/// The drain's routes, signed like the rest.
pub(super) fn routes() -> Router<State> {
    Router::new()
        .route("/v1/team/drain", get(read).post(begin))
        .route("/v1/team/resume", post(resume))
}

#[derive(Deserialize)]
struct Begin {
    /// How long, in seconds, the restart waits for the turns under way; the default when left
    /// out.
    #[serde(default)]
    cap_seconds: Option<i64>,
}

/// A restart is announced: nothing new starts, and the turns under way are told to wrap up.
async fn begin(
    Extract(state): Extract<State>,
    Json(req): Json<Begin>,
) -> Result<Json<Value>, Failed> {
    let hq = state.hq.clone();
    Ok(Json(
        blocking(move || hq.begin_drain(req.cap_seconds)).await?,
    ))
}

/// Whether a restart is announced, since when, how many turns are running, and what the last
/// one came to.
async fn read(Extract(state): Extract<State>) -> Result<Json<Value>, Failed> {
    let hq = state.hq.clone();
    Ok(Json(blocking(move || hq.drain_shown()).await?))
}

/// The announcement is lifted: the restart is done, or was called off.
async fn resume(Extract(state): Extract<State>) -> Result<Json<Value>, Failed> {
    let hq = state.hq.clone();
    Ok(Json(blocking(move || hq.end_drain()).await?))
}
