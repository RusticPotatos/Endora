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
use endora_domain::{
    Assumption, AssumptionId, Direction, DirectionId, Experiment, ExperimentId, Goal, GoalId,
    Observation, ObservationId, ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId,
};
use endora_infrastructure::{RandomIdSource, SqliteStore, SystemClock};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Shared state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    /// The persistence adapter (implements the repository ports).
    pub store: Arc<SqliteStore>,
    /// The identifier source.
    pub ids: Arc<RandomIdSource>,
    /// The system clock.
    pub clock: Arc<SystemClock>,
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
        .route(
            "/v1/assumptions/{id}/experiments",
            post(propose_experiment).get(list_experiments),
        )
        .route("/v1/experiments/{id}/start", post(start_experiment))
        .route("/v1/experiments/{id}/conclude", post(conclude_experiment))
        .route(
            "/v1/experiments/{id}/observations",
            post(record_observation).get(list_observations),
        )
        .route(
            "/v1/goals/{id}/reflections",
            post(create_reflection).get(list_reflections),
        )
        .route(
            "/v1/reflections/{id}/process-changes",
            post(propose_process_change).get(list_process_changes),
        )
        .route(
            "/v1/process-changes/{id}/approve",
            post(approve_process_change),
        )
        .route(
            "/v1/process-changes/{id}/reject",
            post(reject_process_change),
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

#[derive(Deserialize)]
struct CreateExperimentRequest {
    hypothesis: String,
}

#[derive(Serialize)]
struct ExperimentResponse {
    id: String,
    assumption_id: String,
    hypothesis: String,
    status: String,
}

impl From<&Experiment> for ExperimentResponse {
    fn from(e: &Experiment) -> Self {
        Self {
            id: e.id().value().to_string(),
            assumption_id: e.assumption().value().to_string(),
            hypothesis: e.hypothesis().to_owned(),
            status: e.status().name().to_owned(),
        }
    }
}

async fn propose_experiment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CreateExperimentRequest>,
) -> Result<Json<ExperimentResponse>, ApiError> {
    let assumption = parse_assumption_id(&id)?;
    let store = state.store.clone();
    let ids = state.ids.clone();
    let experiment = blocking(move || {
        usecases::propose_experiment(
            store.as_ref(),
            store.as_ref(),
            ids.as_ref(),
            assumption,
            &req.hypothesis,
        )
    })
    .await?;
    Ok(Json(ExperimentResponse::from(&experiment)))
}

async fn list_experiments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ExperimentResponse>>, ApiError> {
    let assumption = parse_assumption_id(&id)?;
    let store = state.store.clone();
    let experiments =
        blocking(move || usecases::list_experiments(store.as_ref(), assumption)).await?;
    Ok(Json(
        experiments.iter().map(ExperimentResponse::from).collect(),
    ))
}

async fn start_experiment(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ExperimentResponse>, ApiError> {
    let experiment_id = parse_experiment_id(&id)?;
    let store = state.store.clone();
    let experiment =
        blocking(move || usecases::start_experiment(store.as_ref(), experiment_id)).await?;
    Ok(Json(ExperimentResponse::from(&experiment)))
}

async fn conclude_experiment(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ExperimentResponse>, ApiError> {
    let experiment_id = parse_experiment_id(&id)?;
    let store = state.store.clone();
    let experiment =
        blocking(move || usecases::conclude_experiment(store.as_ref(), experiment_id)).await?;
    Ok(Json(ExperimentResponse::from(&experiment)))
}

#[derive(Deserialize)]
struct RecordObservationRequest {
    note: String,
}

#[derive(Serialize)]
struct ObservationResponse {
    id: String,
    experiment_id: String,
    note: String,
    recorded_at_ms: i64,
}

impl From<&Observation> for ObservationResponse {
    fn from(o: &Observation) -> Self {
        Self {
            id: o.id().value().to_string(),
            experiment_id: o.experiment().value().to_string(),
            note: o.note().to_owned(),
            recorded_at_ms: o.recorded_at().unix_millis(),
        }
    }
}

async fn record_observation(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RecordObservationRequest>,
) -> Result<Json<ObservationResponse>, ApiError> {
    let experiment = parse_experiment_id(&id)?;
    let store = state.store.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let observation = blocking(move || {
        usecases::record_observation(
            store.as_ref(),
            store.as_ref(),
            ids.as_ref(),
            clock.as_ref(),
            experiment,
            &req.note,
        )
    })
    .await?;
    Ok(Json(ObservationResponse::from(&observation)))
}

async fn list_observations(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ObservationResponse>>, ApiError> {
    let experiment = parse_experiment_id(&id)?;
    let store = state.store.clone();
    let observations =
        blocking(move || usecases::list_observations(store.as_ref(), experiment)).await?;
    Ok(Json(
        observations.iter().map(ObservationResponse::from).collect(),
    ))
}

#[derive(Deserialize)]
struct CreateReflectionRequest {
    summary: String,
    evidence: Vec<String>,
}

#[derive(Serialize)]
struct ReflectionResponse {
    id: String,
    goal_id: String,
    summary: String,
    evidence: Vec<String>,
}

impl From<&Reflection> for ReflectionResponse {
    fn from(r: &Reflection) -> Self {
        Self {
            id: r.id().value().to_string(),
            goal_id: r.goal().value().to_string(),
            summary: r.summary().to_owned(),
            evidence: r.evidence().iter().map(|o| o.value().to_string()).collect(),
        }
    }
}

async fn create_reflection(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CreateReflectionRequest>,
) -> Result<Json<ReflectionResponse>, ApiError> {
    let goal = parse_goal_id(&id)?;
    let evidence = parse_evidence(&req.evidence)?;
    let store = state.store.clone();
    let ids = state.ids.clone();
    let reflection = blocking(move || {
        usecases::create_reflection(
            store.as_ref(),
            store.as_ref(),
            ids.as_ref(),
            goal,
            &req.summary,
            evidence,
        )
    })
    .await?;
    Ok(Json(ReflectionResponse::from(&reflection)))
}

async fn list_reflections(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ReflectionResponse>>, ApiError> {
    let goal = parse_goal_id(&id)?;
    let store = state.store.clone();
    let reflections = blocking(move || usecases::list_reflections(store.as_ref(), goal)).await?;
    Ok(Json(
        reflections.iter().map(ReflectionResponse::from).collect(),
    ))
}

#[derive(Deserialize)]
struct ProposeProcessChangeRequest {
    description: String,
}

#[derive(Serialize)]
struct ProcessChangeResponse {
    id: String,
    reflection_id: String,
    description: String,
    approval: String,
}

impl From<&ProposedProcessChange> for ProcessChangeResponse {
    fn from(c: &ProposedProcessChange) -> Self {
        Self {
            id: c.id().value().to_string(),
            reflection_id: c.reflection().value().to_string(),
            description: c.description().to_owned(),
            approval: c.approval().name().to_owned(),
        }
    }
}

async fn propose_process_change(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ProposeProcessChangeRequest>,
) -> Result<Json<ProcessChangeResponse>, ApiError> {
    let reflection = parse_reflection_id(&id)?;
    let store = state.store.clone();
    let ids = state.ids.clone();
    let change = blocking(move || {
        usecases::propose_process_change(
            store.as_ref(),
            store.as_ref(),
            ids.as_ref(),
            reflection,
            &req.description,
        )
    })
    .await?;
    Ok(Json(ProcessChangeResponse::from(&change)))
}

async fn list_process_changes(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ProcessChangeResponse>>, ApiError> {
    let reflection = parse_reflection_id(&id)?;
    let store = state.store.clone();
    let changes =
        blocking(move || usecases::list_process_changes(store.as_ref(), reflection)).await?;
    Ok(Json(
        changes.iter().map(ProcessChangeResponse::from).collect(),
    ))
}

async fn approve_process_change(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProcessChangeResponse>, ApiError> {
    let change_id = parse_process_change_id(&id)?;
    let store = state.store.clone();
    let change =
        blocking(move || usecases::approve_process_change(store.as_ref(), change_id)).await?;
    Ok(Json(ProcessChangeResponse::from(&change)))
}

async fn reject_process_change(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProcessChangeResponse>, ApiError> {
    let change_id = parse_process_change_id(&id)?;
    let store = state.store.clone();
    let change =
        blocking(move || usecases::reject_process_change(store.as_ref(), change_id)).await?;
    Ok(Json(ProcessChangeResponse::from(&change)))
}

/// Parses a path id into a [`ReflectionId`]; a malformed id names no reflection.
fn parse_reflection_id(id: &str) -> Result<ReflectionId, ApiError> {
    id.parse::<u128>().map(ReflectionId::new).map_err(|_| {
        ApiError(AppError::NotFound {
            entity: "reflection",
        })
    })
}

/// Parses a path id into a [`ProcessChangeId`]; a malformed id names no change.
fn parse_process_change_id(id: &str) -> Result<ProcessChangeId, ApiError> {
    id.parse::<u128>().map(ProcessChangeId::new).map_err(|_| {
        ApiError(AppError::NotFound {
            entity: "process change",
        })
    })
}

/// Parses evidence observation ids from the request body; a malformed id can
/// name no observation.
fn parse_evidence(raw: &[String]) -> Result<Vec<ObservationId>, ApiError> {
    raw.iter()
        .map(|s| {
            s.parse::<u128>().map(ObservationId::new).map_err(|_| {
                ApiError(AppError::NotFound {
                    entity: "observation",
                })
            })
        })
        .collect()
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

/// Parses a path id into an [`AssumptionId`]; a malformed id names no assumption.
fn parse_assumption_id(id: &str) -> Result<AssumptionId, ApiError> {
    id.parse::<u128>().map(AssumptionId::new).map_err(|_| {
        ApiError(AppError::NotFound {
            entity: "assumption",
        })
    })
}

/// Parses a path id into an [`ExperimentId`]; a malformed id names no experiment.
fn parse_experiment_id(id: &str) -> Result<ExperimentId, ApiError> {
    id.parse::<u128>().map(ExperimentId::new).map_err(|_| {
        ApiError(AppError::NotFound {
            entity: "experiment",
        })
    })
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
    use endora_infrastructure::{RandomIdSource, SqliteStore, SystemClock};
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tower::ServiceExt; // for `oneshot`

    fn test_state() -> AppState {
        AppState {
            store: Arc::new(SqliteStore::open_in_memory().unwrap()),
            ids: Arc::new(RandomIdSource),
            clock: Arc::new(SystemClock),
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
    async fn experiment_lifecycle_over_http() {
        let app = app(test_state());

        // direction -> goal -> assumption
        let res = app
            .clone()
            .oneshot(post("/v1/directions", r#"{"title":"Be healthier"}"#))
            .await
            .unwrap();
        let did = json_body(res).await["id"].as_str().unwrap().to_owned();
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/directions/{did}/goals"),
                r#"{"statement":"Run a 5k"}"#,
            ))
            .await
            .unwrap();
        let gid = json_body(res).await["id"].as_str().unwrap().to_owned();
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/goals/{gid}/assumptions"),
                r#"{"statement":"Mornings are freest"}"#,
            ))
            .await
            .unwrap();
        let aid = json_body(res).await["id"].as_str().unwrap().to_owned();

        // propose experiment
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/assumptions/{aid}/experiments"),
                r#"{"hypothesis":"Try mornings"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let created = json_body(res).await;
        assert_eq!(created["status"], "proposed");
        let eid = created["id"].as_str().unwrap().to_owned();

        // start -> running
        let res = app
            .clone()
            .oneshot(post(&format!("/v1/experiments/{eid}/start"), ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_body(res).await["status"], "running");

        // conclude -> concluded
        let res = app
            .clone()
            .oneshot(post(&format!("/v1/experiments/{eid}/conclude"), ""))
            .await
            .unwrap();
        assert_eq!(json_body(res).await["status"], "concluded");

        // concluding again is a domain error -> 400
        let res = app
            .clone()
            .oneshot(post(&format!("/v1/experiments/{eid}/conclude"), ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn observation_recorded_against_an_experiment() {
        let app = app(test_state());
        let did = json_body(
            app.clone()
                .oneshot(post("/v1/directions", r#"{"title":"Be healthier"}"#))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let gid = json_body(
            app.clone()
                .oneshot(post(
                    &format!("/v1/directions/{did}/goals"),
                    r#"{"statement":"Run a 5k"}"#,
                ))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let aid = json_body(
            app.clone()
                .oneshot(post(
                    &format!("/v1/goals/{gid}/assumptions"),
                    r#"{"statement":"Mornings"}"#,
                ))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let eid = json_body(
            app.clone()
                .oneshot(post(
                    &format!("/v1/assumptions/{aid}/experiments"),
                    r#"{"hypothesis":"Try mornings"}"#,
                ))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();

        let obs_uri = format!("/v1/experiments/{eid}/observations");
        let res = app
            .clone()
            .oneshot(post(&obs_uri, r#"{"note":"felt good"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let created = json_body(res).await;
        assert_eq!(created["note"], "felt good");
        assert!(created["recorded_at_ms"].as_i64().unwrap() > 0);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&obs_uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json_body(res).await.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn observation_for_missing_experiment_is_404() {
        let res = app(test_state())
            .oneshot(post("/v1/experiments/999/observations", r#"{"note":"x"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    /// Drives the full chain and returns (goal_id, observation_id).
    async fn seed_chain(app: &axum::Router) -> (String, String) {
        async fn create(app: &axum::Router, uri: &str, body: &str) -> String {
            let res = app.clone().oneshot(post(uri, body)).await.unwrap();
            json_body(res).await["id"].as_str().unwrap().to_owned()
        }
        let did = create(app, "/v1/directions", r#"{"title":"D"}"#).await;
        let gid = create(
            app,
            &format!("/v1/directions/{did}/goals"),
            r#"{"statement":"G"}"#,
        )
        .await;
        let aid = create(
            app,
            &format!("/v1/goals/{gid}/assumptions"),
            r#"{"statement":"A"}"#,
        )
        .await;
        let eid = create(
            app,
            &format!("/v1/assumptions/{aid}/experiments"),
            r#"{"hypothesis":"H"}"#,
        )
        .await;
        let oid = create(
            app,
            &format!("/v1/experiments/{eid}/observations"),
            r#"{"note":"N"}"#,
        )
        .await;
        (gid, oid)
    }

    #[tokio::test]
    async fn reflection_with_evidence_over_http() {
        let app = app(test_state());
        let (gid, oid) = seed_chain(&app).await;

        let body = format!(r#"{{"summary":"mornings worked","evidence":["{oid}"]}}"#);
        let res = app
            .clone()
            .oneshot(post(&format!("/v1/goals/{gid}/reflections"), &body))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let created = json_body(res).await;
        assert_eq!(created["summary"], "mornings worked");
        assert_eq!(created["evidence"][0], oid);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/goals/{gid}/reflections"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json_body(res).await.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn process_change_propose_approve_over_http() {
        let app = app(test_state());
        let (gid, oid) = seed_chain(&app).await;
        let body = format!(r#"{{"summary":"worked","evidence":["{oid}"]}}"#);
        let rid = json_body(
            app.clone()
                .oneshot(post(&format!("/v1/goals/{gid}/reflections"), &body))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();

        // propose -> pending
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/reflections/{rid}/process-changes"),
                r#"{"description":"Default to mornings"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let created = json_body(res).await;
        assert_eq!(created["approval"], "pending");
        let cid = created["id"].as_str().unwrap().to_owned();

        // approve -> approved
        let res = app
            .clone()
            .oneshot(post(&format!("/v1/process-changes/{cid}/approve"), ""))
            .await
            .unwrap();
        assert_eq!(json_body(res).await["approval"], "approved");

        // approving again is a domain error -> 400
        let res = app
            .clone()
            .oneshot(post(&format!("/v1/process-changes/{cid}/approve"), ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn process_change_for_missing_reflection_is_404() {
        let res = app(test_state())
            .oneshot(post(
                "/v1/reflections/999/process-changes",
                r#"{"description":"x"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn reflection_without_evidence_is_400() {
        let app = app(test_state());
        let (gid, _) = seed_chain(&app).await;
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/goals/{gid}/reflections"),
                r#"{"summary":"x","evidence":[]}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
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
