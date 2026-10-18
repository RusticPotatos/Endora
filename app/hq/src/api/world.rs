//! The team's world for the owner (ADR 0122), through the node: read it, download it as
//! Markdown, and make something canon or demote it. Nothing here is a member's: canon is the
//! owner's alone.

use axum::Json;
use axum::Router;
use axum::extract::State as Extract;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;

use endora_team::domain::world::Stage;

use super::{Failed, State, blocking};

/// The world's routes, signed by the node like the rest of the owner's.
pub(super) fn routes() -> Router<State> {
    Router::new()
        .route("/v1/world", get(shown))
        .route("/v1/world/export", get(export))
        .route("/v1/world/promote", post(promote))
        .route("/v1/world/demote", post(demote))
}

/// The world as the HQ screen shows it.
async fn shown(Extract(state): Extract<State>) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    Ok(Json(blocking(move || hq.world_shown()).await?))
}

/// The world as one Markdown document.
async fn export(Extract(state): Extract<State>) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let markdown = blocking(move || hq.world_markdown()).await?;
    Ok(Json(json!({ "markdown": markdown })))
}

#[derive(Deserialize)]
struct Promote {
    address: String,
    #[serde(default)]
    why: String,
}

/// The owner makes a section or a story canon.
async fn promote(
    Extract(state): Extract<State>,
    Json(req): Json<Promote>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let address = req.address.clone();
    blocking(move || hq.promote(&req.address, &req.why)).await?;
    Ok(Json(json!({ "canon": address })))
}

#[derive(Deserialize)]
struct Demote {
    address: String,
    to: String,
    why: String,
}

/// The owner demotes canon, saying why: to an insight, an observation, or retired.
async fn demote(
    Extract(state): Extract<State>,
    Json(req): Json<Demote>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let address = req.address.clone();
    let to = Stage::from_name(&req.to).unwrap_or(Stage::Canon);
    blocking(move || hq.demote(&req.address, to, &req.why)).await?;
    Ok(Json(json!({ "demoted": address, "to": to.name() })))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use endora_team::application::engine::{Engine, Settings};
    use endora_team::domain::Role;
    use endora_team::infrastructure::{RemoteRunner, Runner, SqliteTeamStore};
    use http_body_util::BodyExt as _;
    use serde_json::json;
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
            demo: false,
            tick_seconds: 30,
            last_tick: Arc::default(),
        }
    }

    async fn send(
        app: &axum::Router,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let res = app
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    #[tokio::test]
    async fn the_owner_reads_downloads_promotes_and_demotes_the_world_through_the_node() {
        let st = state();
        st.hq.hire("Jane Doe", Role::Lead, 25, 2, 0).unwrap();
        let app = app(st.clone());
        // Only the node is answered.
        let (status, _) = send(&app, "GET", "/v1/world", None, json!(null)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, world) = send(&app, "GET", "/v1/world", Some(TOKEN), json!(null)).await;
        assert_eq!(status, StatusCode::OK, "{world}");
        assert_eq!(world["pages"][0]["slug"], "house");
        assert_eq!(world["stories"], json!([]));
        let (status, export) =
            send(&app, "GET", "/v1/world/export", Some(TOKEN), json!(null)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            export["markdown"]
                .as_str()
                .unwrap()
                .starts_with("# The team's world"),
            "{export}"
        );
        // Nothing at the address: refused, and nothing changes.
        let (status, said) = send(
            &app,
            "POST",
            "/v1/world/promote",
            Some(TOKEN),
            json!({ "address": "world:house#backups" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
        let (status, said) = send(
            &app,
            "POST",
            "/v1/world/demote",
            Some(TOKEN),
            json!({ "address": "world:house#backups", "to": "insight", "why": "" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
        assert!(
            said["error"].as_str().unwrap().contains("say why"),
            "{said}"
        );
    }
}
