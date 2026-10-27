//! The guards for the owner (ADR 0124), through the node: the verdict on a guard's finding.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State as Extract};
use axum::routing::post;
use serde::Deserialize;
use serde_json::json;

use endora_team::application::engine::EngineError;
use endora_team::domain::guards::Verdict;

use super::{Failed, State, blocking};

/// The guards' routes, signed by the node like the rest of the owner's.
pub(super) fn routes() -> Router<State> {
    Router::new().route("/v1/guards/findings/{id}/verdict", post(verdict))
}

#[derive(Deserialize)]
struct Judged {
    verdict: String,
    #[serde(default)]
    fix_worked: Option<bool>,
}

/// The owner says whether a finding was real, and whether its fix worked.
async fn verdict(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<Judged>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let verdict =
        Verdict::from_name(&req.verdict).ok_or(Failed(EngineError::Unknown("verdict")))?;
    blocking(move || hq.judge_finding(id, verdict, req.fix_worked)).await?;
    Ok(Json(
        json!({ "finding": id, "verdict": verdict.name(), "fix_worked": req.fix_worked }),
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use endora_team::application::engine::{Engine, Settings};
    use endora_team::infrastructure::{RemoteRunner, Runner, SqliteTeamStore};
    use tower::ServiceExt as _;

    use super::super::{State, SystemClock, app};

    const TOKEN: &str = "a-token-of-at-least-thirty-two-characters";

    fn state() -> State {
        State {
            hq: Arc::new(Engine::new(
                SqliteTeamStore::in_memory().unwrap(),
                Runner::Remote(RemoteRunner::default()),
                SystemClock,
                Settings::default(),
            )),
            token: Arc::new(TOKEN.to_owned()),
            runner_token: None,
            tick_seconds: 30,
            last_tick: Arc::default(),
        }
    }

    async fn post(state: &State, path: &str, body: &str, signed: bool) -> StatusCode {
        let mut request = Request::post(path).header("content-type", "application/json");
        if signed {
            request = request.header("authorization", format!("Bearer {TOKEN}"));
        }
        app(state.clone())
            .oneshot(request.body(Body::from(body.to_owned())).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn the_owners_verdict_is_signed_by_the_node_and_names_a_finding_and_a_verdict() {
        let s = state();
        let path = "/v1/guards/findings/1/verdict";
        assert_eq!(
            post(&s, path, r#"{"verdict":"real"}"#, false).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            post(&s, path, r#"{"verdict":"real"}"#, true).await,
            StatusCode::NOT_FOUND,
            "no such finding"
        );
        assert_eq!(
            post(&s, path, r#"{"verdict":"maybe"}"#, true).await,
            StatusCode::NOT_FOUND,
            "no such verdict"
        );
        assert_eq!(
            post(&s, "/v1/retractions/7/forget", r#"{"targets":[]}"#, true).await,
            StatusCode::NOT_FOUND,
            "no such retraction"
        );
    }
}
