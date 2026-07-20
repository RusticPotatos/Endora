//! HTTP interface for the node.
//!
//! This is the **Interface** layer: it translates the versioned HTTP/JSON
//! protocol to and from application use cases and holds no domain or storage
//! logic. Blocking SQLite work runs off the async executor via
//! [`tokio::task::spawn_blocking`] (see `docs/adr/0007-async-web-stack.md`).

use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use endora_application::{
    ActivityItem, AppError, AttentionItem, AttentionKind, Butler, ButlerProposal, CheckinSchedule,
    MemorySnapshot, Proposer, RepositoryError, Suggestion, SuggestionStatus, usecases,
};
use endora_domain::{
    Assumption, AssumptionId, AuditRecord, AutonomyLevel, Belief, BeliefId, ChatMessage, Direction,
    DirectionId, Experiment, ExperimentId, LifecycleStatus, Observation, ObservationId,
    PolicyDecision, Preference, PreferenceId, PreferenceKind, ProcessChangeId,
    ProposedProcessChange, Reflection, ReflectionId, SuggestionId, Target, TargetId, Value,
    ValueId,
};
use endora_infrastructure::{
    Capability, CapabilityError, RandomIdSource, SqliteStore, SystemClock,
};
use futures_util::stream::{Stream, unfold};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;

/// Shared state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    /// The persistence adapter (implements the repository ports).
    pub store: Arc<SqliteStore>,
    /// The identifier source.
    pub ids: Arc<RandomIdSource>,
    /// The system clock.
    pub clock: Arc<SystemClock>,
    /// The reasoning model behind the policy boundary.
    pub proposer: Arc<dyn Proposer + Send + Sync>,
    /// The butler brain (proposes; never acts).
    pub butler: Arc<dyn Butler + Send + Sync>,
    /// Broadcasts a signal whenever a write succeeds, so activity-stream
    /// subscribers know to refresh. Carries no payload — it is a "something
    /// changed" nudge, and clients re-read the authoritative state.
    pub changes: broadcast::Sender<()>,
    /// The butler's skills (weather, web, …) — declared modules the butler can
    /// reach for, each gated by its autonomy level (ADR 0019).
    pub capabilities: Arc<Vec<Arc<dyn Capability>>>,
}

impl AppState {
    /// Creates the shared state, wiring up the change-broadcast channel.
    #[must_use]
    pub fn new(
        store: Arc<SqliteStore>,
        ids: Arc<RandomIdSource>,
        clock: Arc<SystemClock>,
        proposer: Arc<dyn Proposer + Send + Sync>,
        butler: Arc<dyn Butler + Send + Sync>,
    ) -> Self {
        // A small buffer is plenty: subscribers coalesce to a single refresh,
        // and a lagged receiver still gets one "changed" signal.
        let (changes, _) = broadcast::channel(16);
        Self {
            store,
            ids,
            clock,
            proposer,
            butler,
            changes,
            capabilities: Arc::new(endora_infrastructure::default_capabilities()),
        }
    }
}

/// Builds the router for the node's HTTP API.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/v1/values", post(create_value).get(list_values))
        .route("/v1/values/{id}", axum::routing::delete(delete_value))
        .route(
            "/v1/directions",
            post(create_direction).get(list_directions),
        )
        .route(
            "/v1/directions/{id}",
            post(set_direction_status).delete(delete_direction),
        )
        .route("/v1/directions/{id}/value", post(assign_direction_value))
        .route(
            "/v1/directions/{id}/targets",
            post(create_target).get(list_targets),
        )
        .route(
            "/v1/targets/{id}",
            post(set_target_status).delete(delete_target),
        )
        .route(
            "/v1/targets/{id}/assumptions",
            post(create_assumption).get(list_assumptions),
        )
        .route(
            "/v1/assumptions/{id}/experiments",
            post(propose_experiment).get(list_experiments),
        )
        .route("/v1/experiments/{id}/start", post(start_experiment))
        .route("/v1/experiments/{id}/conclude", post(conclude_experiment))
        .route("/v1/experiments/{id}/review", post(schedule_review))
        .route("/v1/reviews/due", get(due_reviews))
        .route(
            "/v1/experiments/{id}/observations",
            post(record_observation).get(list_observations),
        )
        .route(
            "/v1/targets/{id}/reflections",
            post(create_reflection).get(list_reflections),
        )
        .route(
            "/v1/reflections/{id}/process-changes",
            post(propose_process_change).get(list_process_changes),
        )
        .route(
            "/v1/reflections/{id}/process-changes/draft",
            post(draft_process_change),
        )
        .route(
            "/v1/process-changes/{id}/approve",
            post(approve_process_change),
        )
        .route(
            "/v1/process-changes/{id}/reject",
            post(reject_process_change),
        )
        .route(
            "/v1/process-changes/{id}/decision",
            post(decide_process_change),
        )
        .route("/v1/chat", post(send_chat).get(chat_history))
        .route("/v1/chat/stream", post(stream_chat))
        .route("/v1/suggestions", get(list_suggestions))
        .route("/v1/suggestions/{id}/apply", post(apply_suggestion))
        .route("/v1/suggestions/{id}/dismiss", post(dismiss_suggestion))
        .route("/v1/checkin", get(get_checkin).post(set_checkin))
        .route("/v1/understanding", get(list_understanding))
        .route("/v1/understanding/{id}/affirm", post(affirm_belief))
        .route("/v1/understanding/{id}/correct", post(correct_belief))
        .route("/v1/capabilities", get(list_capabilities))
        .route("/v1/capabilities/{id}/invoke", post(invoke_capability))
        .route(
            "/v1/preferences",
            post(create_preference).get(list_preferences),
        )
        .route(
            "/v1/preferences/{id}",
            axum::routing::delete(delete_preference),
        )
        .route("/v1/attention", get(attention))
        .route("/v1/attention/snooze", post(snooze_attention))
        .route("/v1/audit", get(audit))
        .route("/v1/activity", get(activity))
        .route("/v1/activity/stream", get(activity_stream))
        .route("/v1/export", get(export))
        .route("/v1/memory/purge", post(purge))
        // Notify activity-stream subscribers after any successful write.
        .layer(from_fn_with_state(state.changes.clone(), notify_on_change))
        .with_state(state)
}

/// Middleware: after a successful write (any `POST`), send a "changed" signal so
/// activity-stream subscribers refresh. Reads never notify, so the stream itself
/// (a `GET`) does not trigger it.
async fn notify_on_change(
    State(changes): State<broadcast::Sender<()>>,
    request: Request,
    next: Next,
) -> Response {
    let is_write = request.method() == Method::POST;
    let response = next.run(request).await;
    if is_write && response.status().is_success() {
        // Ignored on purpose: no subscribers is a normal, benign state.
        let _ = changes.send(());
    }
    response
}

/// Serves the self-contained web console (embedded in the binary; see ADR 0009).
async fn index() -> Html<&'static str> {
    Html(include_str!("web/index.html"))
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
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_id: Option<String>,
}

impl From<&Direction> for DirectionResponse {
    fn from(d: &Direction) -> Self {
        Self {
            id: d.id().value().to_string(),
            title: d.title().to_owned(),
            status: d.status().name().to_owned(),
            value_id: d.value().map(|v| v.value().to_string()),
        }
    }
}

#[derive(Deserialize)]
struct CreateValueRequest {
    name: String,
}

/// Files a North Star under a value: `{"value_id": "123"}`, or `{}`/`null` to
/// clear.
#[derive(Deserialize)]
struct AssignValueRequest {
    value_id: Option<String>,
}

#[derive(Serialize)]
struct ValueResponse {
    id: String,
    name: String,
}

impl From<&Value> for ValueResponse {
    fn from(v: &Value) -> Self {
        Self {
            id: v.id().value().to_string(),
            name: v.name().to_owned(),
        }
    }
}

async fn create_value(
    State(state): State<AppState>,
    Json(req): Json<CreateValueRequest>,
) -> Result<Json<ValueResponse>, ApiError> {
    let store = state.store.clone();
    let ids = state.ids.clone();
    let value =
        blocking(move || usecases::create_value(store.as_ref(), ids.as_ref(), &req.name)).await?;
    Ok(Json(ValueResponse::from(&value)))
}

async fn list_values(State(state): State<AppState>) -> Result<Json<Vec<ValueResponse>>, ApiError> {
    let store = state.store.clone();
    let values = blocking(move || usecases::list_values(store.as_ref())).await?;
    Ok(Json(values.iter().map(ValueResponse::from).collect()))
}

async fn delete_value(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let value_id = parse_value_id(&id)?;
    let store = state.store.clone();
    blocking(move || usecases::delete_value(store.as_ref(), store.as_ref(), value_id)).await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn assign_direction_value(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AssignValueRequest>,
) -> Result<Json<DirectionResponse>, ApiError> {
    let direction_id = parse_direction_id(&id)?;
    let value_id = match req.value_id {
        Some(raw) => Some(parse_value_id(&raw)?),
        None => None,
    };
    let store = state.store.clone();
    let direction = blocking(move || {
        usecases::assign_direction_value(store.as_ref(), store.as_ref(), direction_id, value_id)
    })
    .await?;
    Ok(Json(DirectionResponse::from(&direction)))
}

#[derive(Deserialize)]
struct CreateTargetRequest {
    statement: String,
}

/// A lifecycle transition request: `{"status": "achieved"}`.
#[derive(Deserialize)]
struct SetStatusRequest {
    status: String,
}

#[derive(Serialize)]
struct TargetResponse {
    id: String,
    direction_id: String,
    statement: String,
    status: String,
}

impl From<&Target> for TargetResponse {
    fn from(g: &Target) -> Self {
        Self {
            id: g.id().value().to_string(),
            direction_id: g.direction().value().to_string(),
            statement: g.statement().to_owned(),
            status: g.status().name().to_owned(),
        }
    }
}

/// Parses a lifecycle status from the request body, or a 400.
fn parse_lifecycle_status(raw: &str) -> Result<LifecycleStatus, ApiError> {
    LifecycleStatus::from_name(raw).ok_or_else(|| {
        ApiError(AppError::BadRequest {
            message: format!(
                "unknown status {raw:?}; expected one of: active, achieved, abandoned, archived"
            ),
        })
    })
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

async fn list_directions(
    State(state): State<AppState>,
) -> Result<Json<Vec<DirectionResponse>>, ApiError> {
    let store = state.store.clone();
    let directions = blocking(move || usecases::list_directions(store.as_ref())).await?;
    Ok(Json(
        directions.iter().map(DirectionResponse::from).collect(),
    ))
}

async fn create_target(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CreateTargetRequest>,
) -> Result<Json<TargetResponse>, ApiError> {
    let direction = parse_direction_id(&id)?;
    let store = state.store.clone();
    let ids = state.ids.clone();
    let target = blocking(move || {
        usecases::create_target(
            store.as_ref(),
            store.as_ref(),
            ids.as_ref(),
            direction,
            &req.statement,
        )
    })
    .await?;
    Ok(Json(TargetResponse::from(&target)))
}

async fn list_targets(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TargetResponse>>, ApiError> {
    let direction = parse_direction_id(&id)?;
    let store = state.store.clone();
    let targets = blocking(move || usecases::list_targets(store.as_ref(), direction)).await?;
    Ok(Json(targets.iter().map(TargetResponse::from).collect()))
}

async fn set_direction_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SetStatusRequest>,
) -> Result<Json<DirectionResponse>, ApiError> {
    let direction_id = parse_direction_id(&id)?;
    let status = parse_lifecycle_status(&req.status)?;
    let store = state.store.clone();
    let direction =
        blocking(move || usecases::set_direction_status(store.as_ref(), direction_id, status))
            .await?;
    Ok(Json(DirectionResponse::from(&direction)))
}

async fn delete_direction(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let direction_id = parse_direction_id(&id)?;
    let store = state.store.clone();
    blocking(move || usecases::delete_direction(store.as_ref(), store.as_ref(), direction_id))
        .await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn set_target_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SetStatusRequest>,
) -> Result<Json<TargetResponse>, ApiError> {
    let target_id = parse_target_id(&id)?;
    let status = parse_lifecycle_status(&req.status)?;
    let store = state.store.clone();
    let target =
        blocking(move || usecases::set_target_status(store.as_ref(), target_id, status)).await?;
    Ok(Json(TargetResponse::from(&target)))
}

async fn delete_target(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let target_id = parse_target_id(&id)?;
    let store = state.store.clone();
    blocking(move || usecases::delete_target(store.as_ref(), store.as_ref(), target_id)).await?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
struct CreateAssumptionRequest {
    statement: String,
}

#[derive(Serialize)]
struct AssumptionResponse {
    id: String,
    target_id: String,
    statement: String,
}

impl From<&Assumption> for AssumptionResponse {
    fn from(a: &Assumption) -> Self {
        Self {
            id: a.id().value().to_string(),
            target_id: a.target().value().to_string(),
            statement: a.statement().to_owned(),
        }
    }
}

async fn create_assumption(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CreateAssumptionRequest>,
) -> Result<Json<AssumptionResponse>, ApiError> {
    let target = parse_target_id(&id)?;
    let store = state.store.clone();
    let ids = state.ids.clone();
    let assumption = blocking(move || {
        usecases::create_assumption(
            store.as_ref(),
            store.as_ref(),
            ids.as_ref(),
            target,
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
    let target = parse_target_id(&id)?;
    let store = state.store.clone();
    let assumptions = blocking(move || usecases::list_assumptions(store.as_ref(), target)).await?;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    review_by_ms: Option<i64>,
}

impl From<&Experiment> for ExperimentResponse {
    fn from(e: &Experiment) -> Self {
        Self {
            id: e.id().value().to_string(),
            assumption_id: e.assumption().value().to_string(),
            hypothesis: e.hypothesis().to_owned(),
            status: e.status().name().to_owned(),
            review_by_ms: e.review_by().map(|t| t.unix_millis()),
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
struct ScheduleReviewRequest {
    in_days: u32,
}

async fn schedule_review(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ScheduleReviewRequest>,
) -> Result<Json<ExperimentResponse>, ApiError> {
    let experiment_id = parse_experiment_id(&id)?;
    let store = state.store.clone();
    let clock = state.clock.clone();
    let experiment = blocking(move || {
        usecases::schedule_experiment_review(
            store.as_ref(),
            clock.as_ref(),
            experiment_id,
            req.in_days,
        )
    })
    .await?;
    Ok(Json(ExperimentResponse::from(&experiment)))
}

async fn due_reviews(
    State(state): State<AppState>,
) -> Result<Json<Vec<ExperimentResponse>>, ApiError> {
    let store = state.store.clone();
    let clock = state.clock.clone();
    let experiments =
        blocking(move || usecases::list_due_reviews(store.as_ref(), clock.as_ref())).await?;
    Ok(Json(
        experiments.iter().map(ExperimentResponse::from).collect(),
    ))
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
    target_id: String,
    summary: String,
    evidence: Vec<String>,
}

impl From<&Reflection> for ReflectionResponse {
    fn from(r: &Reflection) -> Self {
        Self {
            id: r.id().value().to_string(),
            target_id: r.target().value().to_string(),
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
    let target = parse_target_id(&id)?;
    let evidence = parse_evidence(&req.evidence)?;
    let store = state.store.clone();
    let ids = state.ids.clone();
    let reflection = blocking(move || {
        usecases::create_reflection(
            store.as_ref(),
            store.as_ref(),
            ids.as_ref(),
            target,
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
    let target = parse_target_id(&id)?;
    let store = state.store.clone();
    let reflections = blocking(move || usecases::list_reflections(store.as_ref(), target)).await?;
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

async fn draft_process_change(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProcessChangeResponse>, ApiError> {
    let reflection = parse_reflection_id(&id)?;
    let store = state.store.clone();
    let ids = state.ids.clone();
    let proposer = state.proposer.clone();
    let change = blocking(move || {
        usecases::draft_process_change(
            store.as_ref(),
            store.as_ref(),
            ids.as_ref(),
            proposer.as_ref(),
            reflection,
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

#[derive(Deserialize, Default)]
struct DecisionRequest {
    /// The actor's autonomy level; defaults to the most conservative (observe).
    #[serde(default)]
    actor: Option<String>,
}

async fn decide_process_change(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<DecisionRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let change_id = parse_process_change_id(&id)?;
    let actor = match req.actor.as_deref() {
        None => AutonomyLevel::Observe,
        Some(name) => AutonomyLevel::from_name(name).ok_or_else(|| {
            ApiError(AppError::BadRequest {
                message: format!("unknown actor level {name:?}"),
            })
        })?,
    };
    let store = state.store.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let decision = blocking(move || {
        usecases::decide_stored_process_change(
            store.as_ref(),
            ids.as_ref(),
            clock.as_ref(),
            store.as_ref(),
            change_id,
            actor,
        )
    })
    .await?;
    Ok(Json(decision_json(decision)))
}

/// Renders a policy decision as JSON.
fn decision_json(decision: PolicyDecision) -> serde_json::Value {
    match decision {
        PolicyDecision::Permit => json!({ "decision": "permit" }),
        PolicyDecision::RequireHumanApproval => json!({ "decision": "require_human_approval" }),
        PolicyDecision::Deny { reason } => json!({ "decision": "deny", "reason": reason }),
    }
}

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<usize>,
}

#[derive(Serialize)]
struct AuditResponse {
    id: String,
    at_ms: i64,
    summary: String,
}

impl From<&AuditRecord> for AuditResponse {
    fn from(r: &AuditRecord) -> Self {
        Self {
            id: r.id().value().to_string(),
            at_ms: r.at().unix_millis(),
            summary: r.summary().to_owned(),
        }
    }
}

async fn audit(
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Vec<AuditResponse>>, ApiError> {
    let limit = query.limit.unwrap_or(50);
    let store = state.store.clone();
    let records = blocking(move || usecases::recent_audit(store.as_ref(), limit)).await?;
    Ok(Json(records.iter().map(AuditResponse::from).collect()))
}

#[derive(Serialize)]
struct ActivityResponse {
    at_ms: i64,
    kind: String,
    summary: String,
}

impl From<&ActivityItem> for ActivityResponse {
    fn from(item: &ActivityItem) -> Self {
        Self {
            at_ms: item.at().unix_millis(),
            kind: item.kind().name().to_owned(),
            summary: item.summary().to_owned(),
        }
    }
}

/// The activity feed: a merged, newest-first timeline of what has happened.
async fn activity(
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Vec<ActivityResponse>>, ApiError> {
    let limit = query.limit.unwrap_or(50);
    let store = state.store.clone();
    let items =
        blocking(move || usecases::recent_activity(store.as_ref(), store.as_ref(), limit)).await?;
    Ok(Json(items.iter().map(ActivityResponse::from).collect()))
}

/// A server-sent event stream that emits a `changed` event whenever a write
/// succeeds. Clients re-read `/v1/activity` (and other state) on each event —
/// the stream carries a nudge, never the data itself.
async fn activity_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.changes.subscribe();
    let stream = unfold(rx, |mut rx| async move {
        // A closed channel ends the stream; a lag still means "something
        // changed", so both a value and a lag emit one `changed` event.
        match rx.recv().await {
            Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {
                Some((Ok(Event::default().event("changed").data("changed")), rx))
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Serialize)]
struct MessageResponse {
    id: String,
    role: String,
    text: String,
    at_ms: i64,
}

impl From<&ChatMessage> for MessageResponse {
    fn from(m: &ChatMessage) -> Self {
        Self {
            id: m.id().value().to_string(),
            role: m.role().name().to_owned(),
            text: m.text().to_owned(),
            at_ms: m.at().unix_millis(),
        }
    }
}

/// Serializes a proposal as `{kind, label, …params}` so the console can render it
/// and, on confirm, call the matching create endpoint.
fn proposal_json(p: &ButlerProposal) -> serde_json::Value {
    let mut base = json!({ "kind": p.kind(), "label": p.label() });
    let obj = base.as_object_mut().expect("json object");
    match p {
        ButlerProposal::CreateValue { name } => {
            obj.insert("name".to_owned(), json!(name));
        }
        ButlerProposal::CreateNorthStar { title } => {
            obj.insert("title".to_owned(), json!(title));
        }
        ButlerProposal::CreateTarget {
            direction_ref,
            statement,
        } => {
            obj.insert("direction_id".to_owned(), json!(direction_ref));
            obj.insert("statement".to_owned(), json!(statement));
        }
        ButlerProposal::RememberPreference { text, kind } => {
            obj.insert("text".to_owned(), json!(text));
            obj.insert("preference_kind".to_owned(), json!(kind.name()));
        }
    }
    base
}

/// Serializes a persisted [`Suggestion`] as its proposal plus `id` and `status`,
/// so the console can render it and apply/dismiss it by id.
fn suggestion_json(s: &Suggestion) -> serde_json::Value {
    let mut v = proposal_json(&s.proposal);
    if let Some(obj) = v.as_object_mut() {
        obj.insert("id".to_owned(), json!(s.id.value().to_string()));
        obj.insert("status".to_owned(), json!(s.status.name()));
    }
    v
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
}

/// Sends a message to the butler; returns its reply and any proposed actions.
async fn send_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let butler = state.butler.clone();
    let (reply, suggestions) = blocking(move || {
        // Ground the butler in the person's current life before it answers.
        let context = usecases::butler_context(
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            clock.as_ref(),
        )?;
        usecases::send_to_butler(
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            butler.as_ref(),
            ids.as_ref(),
            clock.as_ref(),
            &context,
            &req.message,
        )
    })
    .await?;
    Ok(Json(json!({
        "reply": MessageResponse::from(&reply),
        "proposals": suggestions.iter().map(suggestion_json).collect::<Vec<_>>(),
    })))
}

/// Streams the butler's reply token-by-token as Server-Sent Events, for a live
/// chat. Each event's `data` is a JSON object with a `type`:
/// - `{"type":"token","text":"…"}` — the next piece of the reply's prose;
/// - `{"type":"done","reply":{…},"proposals":[…]}` — the persisted reply + cards;
/// - `{"type":"error","message":"…"}` — the exchange failed.
///
/// The person's message is persisted before the butler is called (as in the
/// non-streaming path), and the reply is persisted when complete — so a dropped
/// connection never loses the turn. The blocking model call runs on a worker
/// thread and feeds tokens through a channel to this async stream.
async fn stream_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let store = state.store.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let butler = state.butler.clone();
    let changes = state.changes.clone();
    let message = req.message;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Event>();

    tokio::task::spawn_blocking(move || {
        let event = |v: serde_json::Value| Event::default().data(v.to_string());
        let context = match usecases::butler_context(
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            clock.as_ref(),
        ) {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(event(json!({ "type": "error", "message": e.to_string() })));
                return;
            }
        };
        // Scope the token closure so its borrow of `tx` ends before the `done`
        // send below.
        let result = {
            let mut on_token = |chunk: &str| {
                let _ = tx.send(event(json!({ "type": "token", "text": chunk })));
            };
            usecases::send_to_butler_streaming(
                store.as_ref(),
                store.as_ref(),
                store.as_ref(),
                store.as_ref(),
                butler.as_ref(),
                ids.as_ref(),
                clock.as_ref(),
                &context,
                &message,
                &mut on_token,
            )
        };
        match result {
            Ok((reply, suggestions)) => {
                // A successful write nudges the change stream, like other writes.
                let _ = changes.send(());
                let _ = tx.send(event(json!({
                    "type": "done",
                    "reply": MessageResponse::from(&reply),
                    "proposals": suggestions.iter().map(suggestion_json).collect::<Vec<_>>(),
                })));
            }
            Err(e) => {
                let _ = tx.send(event(json!({ "type": "error", "message": e.to_string() })));
            }
        }
    });

    let stream = unfold(rx, |mut rx| async move {
        rx.recv().await.map(|ev| (Ok(ev), rx))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// The whole conversation with the butler, oldest first.
async fn chat_history(
    State(state): State<AppState>,
) -> Result<Json<Vec<MessageResponse>>, ApiError> {
    let store = state.store.clone();
    let messages = blocking(move || usecases::chat_history(store.as_ref())).await?;
    Ok(Json(messages.iter().map(MessageResponse::from).collect()))
}

#[derive(Deserialize)]
struct SuggestionsQuery {
    /// Optional filter: `pending`, `applied`, or `dismissed`.
    status: Option<String>,
}

/// The butler's persisted suggestions, newest first. `?status=pending` gives the
/// inbox — proposals waiting to be applied.
async fn list_suggestions(
    State(state): State<AppState>,
    Query(q): Query<SuggestionsQuery>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let store = state.store.clone();
    let status = q.status.as_deref().and_then(SuggestionStatus::from_name);
    let items = blocking(move || usecases::list_suggestions(store.as_ref(), status)).await?;
    Ok(Json(items.iter().map(suggestion_json).collect()))
}

fn parse_suggestion_id(id: &str) -> Result<SuggestionId, ApiError> {
    id.parse::<u128>().map(SuggestionId::new).map_err(|_| {
        ApiError(AppError::NotFound {
            entity: "suggestion",
        })
    })
}

/// Applies a pending suggestion — runs the deterministic create it stands for and
/// records it applied. The human-authorized step (the butler only proposed it).
async fn apply_suggestion(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let sid = parse_suggestion_id(&id)?;
    let store = state.store.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let suggestion = blocking(move || {
        usecases::apply_suggestion(
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            ids.as_ref(),
            clock.as_ref(),
            sid,
        )
    })
    .await?;
    Ok(Json(suggestion_json(&suggestion)))
}

/// Dismisses a pending suggestion (records the decision; nothing is created).
async fn dismiss_suggestion(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let sid = parse_suggestion_id(&id)?;
    let store = state.store.clone();
    let clock = state.clock.clone();
    blocking(move || usecases::dismiss_suggestion(store.as_ref(), clock.as_ref(), sid)).await?;
    Ok(Json(json!({ "dismissed": true })))
}

/// The person's proactive check-in cadence.
#[derive(Serialize)]
struct CheckinResponse {
    enabled: bool,
    interval_ms: i64,
    next_at_ms: i64,
}

impl From<CheckinSchedule> for CheckinResponse {
    fn from(s: CheckinSchedule) -> Self {
        Self {
            enabled: s.enabled,
            interval_ms: s.interval_ms,
            next_at_ms: s.next_at.unix_millis(),
        }
    }
}

async fn get_checkin(State(state): State<AppState>) -> Result<Json<CheckinResponse>, ApiError> {
    let store = state.store.clone();
    let clock = state.clock.clone();
    let schedule =
        blocking(move || usecases::checkin_schedule(store.as_ref(), clock.as_ref())).await?;
    Ok(Json(schedule.into()))
}

#[derive(Deserialize)]
struct SetCheckinRequest {
    enabled: bool,
    interval_ms: i64,
}

/// Sets the check-in cadence (on/off + interval). Enabling schedules the next one
/// an interval from now, so it is not an instant ping.
async fn set_checkin(
    State(state): State<AppState>,
    Json(req): Json<SetCheckinRequest>,
) -> Result<Json<CheckinResponse>, ApiError> {
    let store = state.store.clone();
    let clock = state.clock.clone();
    let schedule = blocking(move || {
        usecases::set_checkin_schedule(store.as_ref(), clock.as_ref(), req.enabled, req.interval_ms)
    })
    .await?;
    Ok(Json(schedule.into()))
}

fn belief_json(b: &Belief) -> serde_json::Value {
    json!({
        "id": b.id().value().to_string(),
        "statement": b.statement(),
        "kind": b.kind().name(),
        "confidence": b.confidence().name(),
        "evidence": b.evidence(),
        "last_affirmed_ms": b.last_affirmed_at().unix_millis(),
    })
}

/// Endora's understanding of the person — the active beliefs it holds.
async fn list_understanding(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let store = state.store.clone();
    let items = blocking(move || usecases::understanding(store.as_ref())).await?;
    Ok(Json(items.iter().map(belief_json).collect()))
}

fn parse_belief_id(id: &str) -> Result<BeliefId, ApiError> {
    id.parse::<u128>()
        .map(BeliefId::new)
        .map_err(|_| ApiError(AppError::NotFound { entity: "belief" }))
}

/// The person confirms a belief is right — raise its confidence.
async fn affirm_belief(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let bid = parse_belief_id(&id)?;
    let store = state.store.clone();
    let clock = state.clock.clone();
    let b = blocking(move || usecases::affirm_belief(store.as_ref(), clock.as_ref(), bid)).await?;
    Ok(Json(belief_json(&b)))
}

/// The person says a belief is wrong — drop it from understanding.
async fn correct_belief(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let bid = parse_belief_id(&id)?;
    let store = state.store.clone();
    blocking(move || usecases::correct_belief(store.as_ref(), bid)).await?;
    Ok(Json(json!({ "corrected": true })))
}

fn capability_json(info: &endora_infrastructure::CapabilityInfo) -> serde_json::Value {
    json!({
        "id": info.id,
        "name": info.name,
        "description": info.description,
        "category": info.category,
        "reaches_external": info.reaches_external,
        "autonomy": info.autonomy.name(),
        "configured": info.configured,
        "needs": info.needs,
    })
}

/// Lists the butler's skills (capabilities/modules) and their status.
async fn list_capabilities(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    Json(
        state
            .capabilities
            .iter()
            .map(|c| capability_json(&c.info()))
            .collect(),
    )
}

/// Invokes a capability by id with a JSON body. Read-only skills run directly;
/// this is the `act` path of the autonomy model. (Consequential skills will be
/// routed through propose→confirm as they are wired.)
async fn invoke_capability(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(cap) = state
        .capabilities
        .iter()
        .find(|c| c.info().id == id)
        .cloned()
    else {
        return Err(ApiError(AppError::NotFound {
            entity: "capability",
        }));
    };
    let result = tokio::task::spawn_blocking(move || cap.invoke(&input))
        .await
        .map_err(|_| {
            ApiError(AppError::Repository(RepositoryError::Backend(
                "worker task failed".to_owned(),
            )))
        })?;
    match result {
        Ok(value) => Ok(Json(json!({ "ok": true, "result": value }))),
        Err(CapabilityError::BadInput(m)) => Err(ApiError(AppError::BadRequest { message: m })),
        Err(CapabilityError::Unavailable(m)) => {
            // Not an error the person did wrong — report it as a soft result.
            Ok(Json(json!({ "ok": false, "unavailable": m })))
        }
    }
}

/// Spawns the butler's **heartbeat**: a background loop that periodically checks
/// whether a proactive check-in is due (per the person's cadence) and, if so, has
/// the butler post one. Only messages — nothing consequential — so it stays on the
/// safe side of the autonomy model (ADR 0010/0019). The blocking store work runs
/// on a worker thread; a posted check-in nudges the change stream.
pub fn spawn_heartbeat(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            ticker.tick().await;
            let store = state.store.clone();
            let ids = state.ids.clone();
            let clock = state.clock.clone();
            let posted = tokio::task::spawn_blocking(move || {
                let context = usecases::butler_context(
                    store.as_ref(),
                    store.as_ref(),
                    store.as_ref(),
                    store.as_ref(),
                    store.as_ref(),
                    clock.as_ref(),
                )?;
                usecases::run_due_checkin(
                    store.as_ref(),
                    store.as_ref(),
                    ids.as_ref(),
                    clock.as_ref(),
                    &context,
                )
            })
            .await;
            if let Ok(Ok(Some(_))) = posted {
                let _ = state.changes.send(());
            }
        }
    });
}

#[derive(Serialize)]
struct PreferenceResponse {
    id: String,
    text: String,
    kind: String,
    at_ms: i64,
}

impl From<&Preference> for PreferenceResponse {
    fn from(p: &Preference) -> Self {
        Self {
            id: p.id().value().to_string(),
            text: p.text().to_owned(),
            kind: p.kind().name().to_owned(),
            at_ms: p.at().unix_millis(),
        }
    }
}

#[derive(Deserialize)]
struct CreatePreferenceRequest {
    text: String,
    #[serde(default)]
    kind: Option<String>,
}

async fn create_preference(
    State(state): State<AppState>,
    Json(req): Json<CreatePreferenceRequest>,
) -> Result<Json<PreferenceResponse>, ApiError> {
    let kind = match req.kind.as_deref() {
        Some(k) => PreferenceKind::from_name(k).ok_or_else(|| {
            ApiError(AppError::BadRequest {
                message: format!("unknown preference kind {k:?}; expected taste or authority"),
            })
        })?,
        None => PreferenceKind::Taste,
    };
    let store = state.store.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let preference = blocking(move || {
        usecases::create_preference(
            store.as_ref(),
            ids.as_ref(),
            clock.as_ref(),
            &req.text,
            kind,
        )
    })
    .await?;
    Ok(Json(PreferenceResponse::from(&preference)))
}

async fn list_preferences(
    State(state): State<AppState>,
) -> Result<Json<Vec<PreferenceResponse>>, ApiError> {
    let store = state.store.clone();
    let prefs = blocking(move || usecases::list_preferences(store.as_ref())).await?;
    Ok(Json(prefs.iter().map(PreferenceResponse::from).collect()))
}

async fn delete_preference(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let pref_id = id.parse::<u128>().map(PreferenceId::new).map_err(|_| {
        ApiError(AppError::NotFound {
            entity: "preference",
        })
    })?;
    let store = state.store.clone();
    blocking(move || usecases::delete_preference(store.as_ref(), pref_id)).await?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Serialize)]
struct AttentionResponse {
    kind: String,
    subject: String,
    headline: String,
}

impl From<&AttentionItem> for AttentionResponse {
    fn from(i: &AttentionItem) -> Self {
        Self {
            kind: i.kind.name().to_owned(),
            subject: i.subject.clone(),
            headline: i.headline.clone(),
        }
    }
}

/// What currently needs the person's attention (snoozed items suppressed).
async fn attention(
    State(state): State<AppState>,
) -> Result<Json<Vec<AttentionResponse>>, ApiError> {
    let store = state.store.clone();
    let clock = state.clock.clone();
    let items = blocking(move || {
        usecases::attention(
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            store.as_ref(),
            clock.as_ref(),
        )
    })
    .await?;
    Ok(Json(items.iter().map(AttentionResponse::from).collect()))
}

#[derive(Deserialize)]
struct SnoozeRequest {
    kind: String,
    subject: String,
}

/// Snoozes an attention item ("not now"), with exponential backoff.
async fn snooze_attention(
    State(state): State<AppState>,
    Json(req): Json<SnoozeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let kind = AttentionKind::from_name(&req.kind).ok_or_else(|| {
        ApiError(AppError::BadRequest {
            message: format!("unknown attention kind {:?}", req.kind),
        })
    })?;
    let store = state.store.clone();
    let clock = state.clock.clone();
    let snooze = blocking(move || {
        usecases::snooze_attention(store.as_ref(), clock.as_ref(), kind, &req.subject)
    })
    .await?;
    Ok(Json(json!({
        "count": snooze.count,
        "snoozed_until_ms": snooze.until.unix_millis(),
    })))
}

/// The full export of the user's data — the "exportable" memory right.
#[derive(Serialize)]
struct ExportResponse {
    values: Vec<ValueResponse>,
    directions: Vec<DirectionResponse>,
    targets: Vec<TargetResponse>,
    assumptions: Vec<AssumptionResponse>,
    experiments: Vec<ExperimentResponse>,
    observations: Vec<ObservationResponse>,
    reflections: Vec<ReflectionResponse>,
    process_changes: Vec<ProcessChangeResponse>,
    audit: Vec<AuditResponse>,
    messages: Vec<MessageResponse>,
    preferences: Vec<PreferenceResponse>,
    suggestions: Vec<serde_json::Value>,
    beliefs: Vec<serde_json::Value>,
}

impl From<&MemorySnapshot> for ExportResponse {
    fn from(s: &MemorySnapshot) -> Self {
        Self {
            values: s.values.iter().map(ValueResponse::from).collect(),
            directions: s.directions.iter().map(DirectionResponse::from).collect(),
            targets: s.targets.iter().map(TargetResponse::from).collect(),
            assumptions: s.assumptions.iter().map(AssumptionResponse::from).collect(),
            experiments: s.experiments.iter().map(ExperimentResponse::from).collect(),
            observations: s
                .observations
                .iter()
                .map(ObservationResponse::from)
                .collect(),
            reflections: s.reflections.iter().map(ReflectionResponse::from).collect(),
            process_changes: s
                .process_changes
                .iter()
                .map(ProcessChangeResponse::from)
                .collect(),
            audit: s.audit.iter().map(AuditResponse::from).collect(),
            messages: s.messages.iter().map(MessageResponse::from).collect(),
            preferences: s.preferences.iter().map(PreferenceResponse::from).collect(),
            suggestions: s.suggestions.iter().map(suggestion_json).collect(),
            beliefs: s.beliefs.iter().map(belief_json).collect(),
        }
    }
}

async fn export(State(state): State<AppState>) -> Result<Json<ExportResponse>, ApiError> {
    let store = state.store.clone();
    let snapshot = blocking(move || usecases::export_memory(store.as_ref())).await?;
    Ok(Json(ExportResponse::from(&snapshot)))
}

#[derive(Deserialize)]
struct PurgeRequest {
    #[serde(default)]
    confirm: bool,
}

async fn purge(
    State(state): State<AppState>,
    Json(req): Json<PurgeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !req.confirm {
        return Err(ApiError(AppError::BadRequest {
            message: r#"send {"confirm": true} to permanently delete all data"#.to_owned(),
        }));
    }
    let store = state.store.clone();
    blocking(move || usecases::purge_memory(store.as_ref())).await?;
    Ok(Json(json!({ "purged": true })))
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

/// Parses a path/body id into a [`ValueId`]; a malformed id names no value.
fn parse_value_id(id: &str) -> Result<ValueId, ApiError> {
    id.parse::<u128>()
        .map(ValueId::new)
        .map_err(|_| ApiError(AppError::NotFound { entity: "value" }))
}

/// Parses a path id into a [`TargetId`]; a malformed id can name no target.
fn parse_target_id(id: &str) -> Result<TargetId, ApiError> {
    id.parse::<u128>()
        .map(TargetId::new)
        .map_err(|_| ApiError(AppError::NotFound { entity: "target" }))
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
            AppError::BadRequest { message } => (StatusCode::BAD_REQUEST, message.clone()),
            AppError::NotFound { .. } => (StatusCode::NOT_FOUND, self.0.to_string()),
            AppError::Model { message } => (StatusCode::SERVICE_UNAVAILABLE, message.clone()),
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

    /// A proposer that returns a fixed line, so the draft endpoint can be tested
    /// without a live model server.
    struct StubProposer;

    impl endora_application::Proposer for StubProposer {
        fn propose_process_change(
            &self,
            _summary: &str,
            _evidence_count: usize,
        ) -> Result<String, endora_application::ProposalError> {
            Ok("Default runs to mornings".to_owned())
        }
    }

    fn test_state() -> AppState {
        AppState::new(
            Arc::new(SqliteStore::open_in_memory().unwrap()),
            Arc::new(RandomIdSource),
            Arc::new(SystemClock),
            Arc::new(StubProposer),
            Arc::new(endora_infrastructure::ScriptedButler),
        )
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

    fn get(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    fn del(uri: &str) -> Request<Body> {
        Request::builder()
            .method("DELETE")
            .uri(uri)
            .body(Body::empty())
            .unwrap()
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
    async fn directions_can_be_listed() {
        let app = app(test_state());
        app.clone()
            .oneshot(post("/v1/directions", r#"{"title":"Be healthier"}"#))
            .await
            .unwrap();
        app.clone()
            .oneshot(post("/v1/directions", r#"{"title":"Learn guitar"}"#))
            .await
            .unwrap();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/directions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_body(res).await.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn create_direction_then_target_then_list() {
        let app = app(test_state());

        let res = app
            .clone()
            .oneshot(post("/v1/directions", r#"{"title":"Be healthier"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let created = json_body(res).await;
        let dir_id = created["id"].as_str().unwrap().to_owned();

        let targets_uri = format!("/v1/directions/{dir_id}/targets");
        let res = app
            .clone()
            .oneshot(post(&targets_uri, r#"{"statement":"Run a 5k"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&targets_uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let listed = json_body(res).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["statement"], "Run a 5k");
        assert_eq!(listed[0]["status"], "active");
    }

    #[tokio::test]
    async fn target_status_transitions_then_deletes() {
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
        let tid = json_body(
            app.clone()
                .oneshot(post(
                    &format!("/v1/directions/{did}/targets"),
                    r#"{"statement":"Run a 5k"}"#,
                ))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();

        // Achieve it.
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/targets/{tid}"),
                r#"{"status":"achieved"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_body(res).await["status"], "achieved");

        // An unknown status is a 400.
        let res = app
            .clone()
            .oneshot(post(&format!("/v1/targets/{tid}"), r#"{"status":"nope"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        // Delete it (no assumptions) and confirm it's gone.
        let res = app
            .clone()
            .oneshot(del(&format!("/v1/targets/{tid}")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let listed = json_body(
            app.clone()
                .oneshot(get(&format!("/v1/directions/{did}/targets")))
                .await
                .unwrap(),
        )
        .await;
        assert!(listed.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_a_target_with_assumptions_is_refused() {
        let app = app(test_state());
        let (tid, _oid) = seed_chain(&app).await; // target with an assumption under it
        let res = app
            .clone()
            .oneshot(del(&format!("/v1/targets/{tid}")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn attention_lists_north_star_items_and_snooze_hides_one() {
        let app = app(test_state());
        let did = json_body(
            app.clone()
                .oneshot(post(
                    "/v1/directions",
                    r#"{"title":"Get back into running"}"#,
                ))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();

        // Unfiled + empty North Star → two attention items.
        let items = json_body(app.clone().oneshot(get("/v1/attention")).await.unwrap()).await;
        let kinds: Vec<String> = items
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["kind"].as_str().unwrap().to_owned())
            .collect();
        assert!(kinds.contains(&"unfiled_north_star".to_owned()));
        assert!(kinds.contains(&"empty_north_star".to_owned()));

        // Snooze the unfiled item; it disappears, the other remains.
        let body = format!(r#"{{"kind":"unfiled_north_star","subject":"{did}"}}"#);
        let res = app
            .clone()
            .oneshot(post("/v1/attention/snooze", &body))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let after = json_body(app.clone().oneshot(get("/v1/attention")).await.unwrap()).await;
        let kinds2: Vec<String> = after
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["kind"].as_str().unwrap().to_owned())
            .collect();
        assert!(!kinds2.contains(&"unfiled_north_star".to_owned()));
        assert!(kinds2.contains(&"empty_north_star".to_owned()));
    }

    #[tokio::test]
    async fn preferences_round_trip_and_are_deletable() {
        let app = app(test_state());
        let res = app
            .clone()
            .oneshot(post("/v1/preferences", r#"{"text":"I prefer mornings"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let created = json_body(res).await;
        assert_eq!(created["text"], "I prefer mornings");
        assert_eq!(created["kind"], "taste");
        let pid = created["id"].as_str().unwrap().to_owned();

        let listed = json_body(app.clone().oneshot(get("/v1/preferences")).await.unwrap()).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);

        let res = app
            .clone()
            .oneshot(del(&format!("/v1/preferences/{pid}")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let after = json_body(app.clone().oneshot(get("/v1/preferences")).await.unwrap()).await;
        assert!(after.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn chat_records_the_exchange_and_returns_proposals() {
        let app = app(test_state());
        let res = app
            .clone()
            .oneshot(post(
                "/v1/chat",
                r#"{"message":"I want to get back into running"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = json_body(res).await;
        assert_eq!(body["reply"]["role"], "butler");
        assert_eq!(body["proposals"][0]["kind"], "create_north_star");
        assert_eq!(body["proposals"][0]["title"], "get back into running");

        // The history holds both turns.
        let hist = json_body(app.clone().oneshot(get("/v1/chat")).await.unwrap()).await;
        assert_eq!(hist.as_array().unwrap().len(), 2);
        assert_eq!(hist[0]["role"], "user");
        assert_eq!(hist[1]["role"], "butler");
    }

    #[tokio::test]
    async fn value_files_a_north_star_and_delete_is_guarded() {
        let app = app(test_state());
        let vid = json_body(
            app.clone()
                .oneshot(post("/v1/values", r#"{"name":"Health"}"#))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let did = json_body(
            app.clone()
                .oneshot(post(
                    "/v1/directions",
                    r#"{"title":"Get back into running"}"#,
                ))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();

        // File the North Star under the value.
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/directions/{did}/value"),
                &format!(r#"{{"value_id":"{vid}"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_body(res).await["value_id"], vid);

        // Deleting a value in use is refused.
        let res = app
            .clone()
            .oneshot(del(&format!("/v1/values/{vid}")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        // Unfile, then delete succeeds.
        app.clone()
            .oneshot(post(
                &format!("/v1/directions/{did}/value"),
                r#"{"value_id":null}"#,
            ))
            .await
            .unwrap();
        let res = app
            .clone()
            .oneshot(del(&format!("/v1/values/{vid}")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let listed = json_body(app.clone().oneshot(get("/v1/values")).await.unwrap()).await;
        assert!(listed.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn assumption_under_a_target_round_trips() {
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
                &format!("/v1/directions/{dir_id}/targets"),
                r#"{"statement":"Run a 5k"}"#,
            ))
            .await
            .unwrap();
        let target_id = json_body(res).await["id"].as_str().unwrap().to_owned();

        let assumptions_uri = format!("/v1/targets/{target_id}/assumptions");
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

        // direction -> target -> assumption
        let res = app
            .clone()
            .oneshot(post("/v1/directions", r#"{"title":"Be healthier"}"#))
            .await
            .unwrap();
        let did = json_body(res).await["id"].as_str().unwrap().to_owned();
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/directions/{did}/targets"),
                r#"{"statement":"Run a 5k"}"#,
            ))
            .await
            .unwrap();
        let gid = json_body(res).await["id"].as_str().unwrap().to_owned();
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/targets/{gid}/assumptions"),
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
                    &format!("/v1/directions/{did}/targets"),
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
                    &format!("/v1/targets/{gid}/assumptions"),
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

    /// Drives the full chain and returns (target_id, observation_id).
    async fn seed_chain(app: &axum::Router) -> (String, String) {
        async fn create(app: &axum::Router, uri: &str, body: &str) -> String {
            let res = app.clone().oneshot(post(uri, body)).await.unwrap();
            json_body(res).await["id"].as_str().unwrap().to_owned()
        }
        let did = create(app, "/v1/directions", r#"{"title":"D"}"#).await;
        let gid = create(
            app,
            &format!("/v1/directions/{did}/targets"),
            r#"{"statement":"G"}"#,
        )
        .await;
        let aid = create(
            app,
            &format!("/v1/targets/{gid}/assumptions"),
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
            .oneshot(post(&format!("/v1/targets/{gid}/reflections"), &body))
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
                    .uri(format!("/v1/targets/{gid}/reflections"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json_body(res).await.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn activity_feed_lists_a_recorded_observation() {
        let app = app(test_state());
        seed_chain(&app).await; // records an observation with note "N"

        let res = app.clone().oneshot(get("/v1/activity")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let feed = json_body(res).await;
        let items = feed.as_array().unwrap();
        assert!(
            items
                .iter()
                .any(|i| i["kind"] == "observation" && i["summary"] == "N"),
            "activity feed should include the recorded observation, got {feed}"
        );
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
            .oneshot(post("/v1/directions", r#"{"title":"D"}"#))
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

        // A domain-invalid request (blank title) is a 400 and must not signal.
        let res = app
            .clone()
            .oneshot(post("/v1/directions", r#"{"title":"  "}"#))
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
    async fn process_change_propose_approve_over_http() {
        let app = app(test_state());
        let (gid, oid) = seed_chain(&app).await;
        let body = format!(r#"{{"summary":"worked","evidence":["{oid}"]}}"#);
        let rid = json_body(
            app.clone()
                .oneshot(post(&format!("/v1/targets/{gid}/reflections"), &body))
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
    async fn policy_decision_flow_is_audited() {
        let app = app(test_state());
        let (gid, oid) = seed_chain(&app).await;
        let rid = json_body(
            app.clone()
                .oneshot(post(
                    &format!("/v1/targets/{gid}/reflections"),
                    &format!(r#"{{"summary":"worked","evidence":["{oid}"]}}"#),
                ))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let cid = json_body(
            app.clone()
                .oneshot(post(
                    &format!("/v1/reflections/{rid}/process-changes"),
                    r#"{"description":"Default to mornings"}"#,
                ))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();

        // Unapproved: policy requires human approval, and it's audited.
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/process-changes/{cid}/decision"),
                r#"{"actor":"act_within_policy"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_body(res).await["decision"], "require_human_approval");

        // Approve, then decide again: now permitted.
        app.clone()
            .oneshot(post(&format!("/v1/process-changes/{cid}/approve"), ""))
            .await
            .unwrap();
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/process-changes/{cid}/decision"),
                r#"{"actor":"act_within_policy"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(json_body(res).await["decision"], "permit");

        // Both decisions are on the audit trail.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/audit")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_body(res).await.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn export_then_purge_clears_all_data() {
        let app = app(test_state());
        let (gid, _oid) = seed_chain(&app).await;

        // Export shows the seeded data.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/export")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let export = json_body(res).await;
        assert_eq!(export["targets"].as_array().unwrap().len(), 1);
        assert_eq!(export["observations"].as_array().unwrap().len(), 1);

        // Purge without confirmation is refused.
        let res = app
            .clone()
            .oneshot(post("/v1/memory/purge", r#"{"confirm":false}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        // Purge with confirmation wipes everything.
        let res = app
            .clone()
            .oneshot(post("/v1/memory/purge", r#"{"confirm":true}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // The target's data is gone.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/directions/{gid}/targets"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // (gid no longer exists, but listing a target's assumptions is by target id)
        let res2 = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/export")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            json_body(res2).await["targets"].as_array().unwrap().len(),
            0
        );
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn an_unknown_actor_level_is_400() {
        let res = app(test_state())
            .oneshot(post(
                "/v1/process-changes/1/decision",
                r#"{"actor":"emperor"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn model_drafts_a_pending_change_that_still_needs_approval() {
        let app = app(test_state());
        let (gid, oid) = seed_chain(&app).await;
        let rid = json_body(
            app.clone()
                .oneshot(post(
                    &format!("/v1/targets/{gid}/reflections"),
                    &format!(r#"{{"summary":"worked","evidence":["{oid}"]}}"#),
                ))
                .await
                .unwrap(),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();

        // The model drafts a change — but it lands as pending, not approved.
        let res = app
            .clone()
            .oneshot(post(
                &format!("/v1/reflections/{rid}/process-changes/draft"),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let drafted = json_body(res).await;
        assert_eq!(drafted["description"], "Default runs to mornings");
        assert_eq!(drafted["approval"], "pending");
    }

    #[tokio::test]
    async fn drafting_for_a_missing_reflection_is_404() {
        let res = app(test_state())
            .oneshot(post("/v1/reflections/999/process-changes/draft", ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
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
                &format!("/v1/targets/{gid}/reflections"),
                r#"{"summary":"x","evidence":[]}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn assumption_for_missing_target_is_404() {
        let res = app(test_state())
            .oneshot(post("/v1/targets/999/assumptions", r#"{"statement":"x"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn target_for_missing_direction_is_404() {
        let res = app(test_state())
            .oneshot(post("/v1/directions/999/targets", r#"{"statement":"x"}"#))
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
