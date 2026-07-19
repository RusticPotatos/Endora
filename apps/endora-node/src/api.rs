//! HTTP interface for the node.
//!
//! This is the **Interface** layer: it translates the versioned HTTP/JSON
//! protocol to and from application use cases and holds no domain or storage
//! logic. Blocking SQLite work runs off the async executor via
//! [`tokio::task::spawn_blocking`] (see `docs/adr/0007-async-web-stack.md`).

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use endora_application::{AppError, RepositoryError, usecases};
use endora_domain::{Assumption, Direction, DirectionId, Goal, GoalId};
use endora_infrastructure::{RandomIdSource, SqliteStore};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Shared state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    /// The persistence adapter (implements the repository ports).
    pub store: Arc<SqliteStore>,
    /// The identifier source.
    pub ids: Arc<RandomIdSource>,
}

/// Builds the router for the node's HTTP API.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/directions", post(create_direction))
        .route(
            "/v1/directions/{id}/goals",
            post(create_goal).get(list_goals),
        )
        .route(
            "/v1/goals/{id}/assumptions",
            post(create_assumption).get(list_assumptions),
        )
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": endora_application::platform_identity() }))
}

#[derive(Deserialize)]
struct CreateDirectionRequest {
    title: String,
}

#[derive(Serialize)]
struct DirectionResponse {
    id: String,
    title: String,
}

impl From<&Direction> for DirectionResponse {
    fn from(d: &Direction) -> Self {
        Self {
            id: d.id().value().to_string(),
            title: d.title().to_owned(),
        }
    }
}

#[derive(Deserialize)]
struct CreateGoalRequest {
    statement: String,
}

#[derive(Serialize)]
struct GoalResponse {
    id: String,
    direction_id: String,
    statement: String,
}

impl From<&Goal> for GoalResponse {
    fn from(g: &Goal) -> Self {
        Self {
            id: g.id().value().to_string(),
            direction_id: g.direction().value().to_string(),
            statement: g.statement().to_owned(),
        }
    }
}

async fn create_direction(
    State(state): State<AppState>,
    Json(req): Json<CreateDirectionRequest>,
) -> Result<Json<DirectionResponse>, ApiError> {
    let store = state.store.clone();
    let ids = state.ids.clone();
    let direction =
        blocking(move || usecases::create_direction(store.as_ref(), ids.as_ref(), &req.title))
            .await?;
    Ok(Json(DirectionResponse::from(&direction)))
}

async fn create_goal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CreateGoalRequest>,
) -> Result<Json<GoalResponse>, ApiError> {
    let direction = parse_direction_id(&id)?;
    let store = state.store.clone();
    let ids = state.ids.clone();
    let goal = blocking(move || {
        usecases::create_goal(
            store.as_ref(),
            store.as_ref(),
            ids.as_ref(),
            direction,
            &req.statement,
        )
    })
    .await?;
    Ok(Json(GoalResponse::from(&goal)))
}

async fn list_goals(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<GoalResponse>>, ApiError> {
    let direction = parse_direction_id(&id)?;
    let store = state.store.clone();
    let goals = blocking(move || usecases::list_goals(store.as_ref(), direction)).await?;
    Ok(Json(goals.iter().map(GoalResponse::from).collect()))
}

#[derive(Deserialize)]
struct CreateAssumptionRequest {
    statement: String,
}

#[derive(Serialize)]
struct AssumptionResponse {
    id: String,
    goal_id: String,
    statement: String,
}

impl From<&Assumption> for AssumptionResponse {
    fn from(a: &Assumption) -> Self {
        Self {
            id: a.id().value().to_string(),
            goal_id: a.goal().value().to_string(),
            statement: a.statement().to_owned(),
        }
    }
}

async fn create_assumption(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CreateAssumptionRequest>,
) -> Result<Json<AssumptionResponse>, ApiError> {
    let goal = parse_goal_id(&id)?;
    let store = state.store.clone();
    let ids = state.ids.clone();
    let assumption = blocking(move || {
        usecases::create_assumption(
            store.as_ref(),
            store.as_ref(),
            ids.as_ref(),
            goal,
            &req.statement,
        )
    })
    .await?;
    Ok(Json(AssumptionResponse::from(&assumption)))
}

async fn list_assumptions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<AssumptionResponse>>, ApiError> {
    let goal = parse_goal_id(&id)?;
    let store = state.store.clone();
    let assumptions = blocking(move || usecases::list_assumptions(store.as_ref(), goal)).await?;
    Ok(Json(
        assumptions.iter().map(AssumptionResponse::from).collect(),
    ))
}

/// Parses a path id into a [`DirectionId`]; a malformed id can name no
/// direction, so it is reported as not found.
fn parse_direction_id(id: &str) -> Result<DirectionId, ApiError> {
    id.parse::<u128>().map(DirectionId::new).map_err(|_| {
        ApiError(AppError::NotFound {
            entity: "direction",
        })
    })
}

/// Parses a path id into a [`GoalId`]; a malformed id can name no goal.
fn parse_goal_id(id: &str) -> Result<GoalId, ApiError> {
    id.parse::<u128>()
        .map(GoalId::new)
        .map_err(|_| ApiError(AppError::NotFound { entity: "goal" }))
}

/// Runs blocking use-case work on the blocking thread pool, mapping a task
/// failure to a backend error.
async fn blocking<T>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, ApiError>
where
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(result) => result.map_err(ApiError),
        Err(_) => Err(ApiError(AppError::Repository(RepositoryError::Backend(
            "worker task failed".to_owned(),
        )))),
    }
}

/// Wraps [`AppError`] so it can be turned into an HTTP response.
struct ApiError(AppError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self.0 {
            AppError::Domain(e) => (StatusCode::BAD_REQUEST, e.to_string()),
            AppError::NotFound { .. } => (StatusCode::NOT_FOUND, self.0.to_string()),
            // Don't leak backend detail to clients.
            AppError::Repository(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error".to_owned(),
            ),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::{AppState, app};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use endora_infrastructure::{RandomIdSource, SqliteStore};
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tower::ServiceExt; // for `oneshot`

    fn test_state() -> AppState {
        AppState {
            store: Arc::new(SqliteStore::open_in_memory().unwrap()),
            ids: Arc::new(RandomIdSource),
        }
    }

    async fn json_body(res: axum::response::Response) -> serde_json::Value {
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn post(uri: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    }

    #[tokio::test]
    async fn create_direction_then_goal_then_list() {
        let app = app(test_state());

        let res = app
            .clone()
            .oneshot(post("/v1/directions", r#"{"title":"Be healthier"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let created = json_body(res).await;
        let dir_id = created["id"].as_str().unwrap().to_owned();

        let goals_uri = format!("/v1/directions/{dir_id}/goals");
        let res = app
            .clone()
            .oneshot(post(&goals_uri, r#"{"statement":"Run a 5k"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&goals_uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let listed = json_body(res).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["statement"], "Run a 5k");
    }

    #[tokio::test]
    async fn assumption_under_a_goal_round_trips() {
        let app = app(test_state());

        let res = app
            .clone()
            .oneshot(post("/v1/directions", r#"{"title":"Be healthier"}"#))
            .await
            .unwrap();
        let dir_id = json_body(res).await["id"].as_str().unwrap().to_owned();

        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/directions/{dir_id}/goals"),
                r#"{"statement":"Run a 5k"}"#,
            ))
            .await
            .unwrap();
        let goal_id = json_body(res).await["id"].as_str().unwrap().to_owned();

        let assumptions_uri = format!("/v1/goals/{goal_id}/assumptions");
        let res = app
            .clone()
            .oneshot(post(
                &assumptions_uri,
                r#"{"statement":"Mornings are freest"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&assumptions_uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let listed = json_body(res).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["statement"], "Mornings are freest");
    }

    #[tokio::test]
    async fn assumption_for_missing_goal_is_404() {
        let res = app(test_state())
            .oneshot(post("/v1/goals/999/assumptions", r#"{"statement":"x"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn goal_for_missing_direction_is_404() {
        let res = app(test_state())
            .oneshot(post("/v1/directions/999/goals", r#"{"statement":"x"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn blank_title_is_400() {
        let res = app(test_state())
            .oneshot(post("/v1/directions", r#"{"title":"   "}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
