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
    ActivityItem, AppError, MemorySnapshot, Proposer, RepositoryError, usecases,
};
use endora_domain::{
    Assumption, AssumptionId, AuditRecord, AutonomyLevel, Direction, DirectionId, Experiment,
    ExperimentId, LifecycleStatus, Observation, ObservationId, PolicyDecision, ProcessChangeId,
    ProposedProcessChange, Reflection, ReflectionId, Target, TargetId,
};
use endora_infrastructure::{RandomIdSource, SqliteStore, SystemClock};
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
    /// Broadcasts a signal whenever a write succeeds, so activity-stream
    /// subscribers know to refresh. Carries no payload — it is a "something
    /// changed" nudge, and clients re-read the authoritative state.
    pub changes: broadcast::Sender<()>,
}

impl AppState {
    /// Creates the shared state, wiring up the change-broadcast channel.
    #[must_use]
    pub fn new(
        store: Arc<SqliteStore>,
        ids: Arc<RandomIdSource>,
        clock: Arc<SystemClock>,
        proposer: Arc<dyn Proposer + Send + Sync>,
    ) -> Self {
        // A small buffer is plenty: subscribers coalesce to a single refresh,
        // and a lagged receiver still gets one "changed" signal.
        let (changes, _) = broadcast::channel(16);
        Self {
            store,
            ids,
            clock,
            proposer,
            changes,
        }
    }
}

/// Builds the router for the node's HTTP API.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route(
            "/v1/directions",
            post(create_direction).get(list_directions),
        )
        .route(
            "/v1/directions/{id}",
            post(set_direction_status).delete(delete_direction),
        )
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
}

impl From<&Direction> for DirectionResponse {
    fn from(d: &Direction) -> Self {
        Self {
            id: d.id().value().to_string(),
            title: d.title().to_owned(),
            status: d.status().name().to_owned(),
        }
    }
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

/// The full export of the user's data — the "exportable" memory right.
#[derive(Serialize)]
struct ExportResponse {
    directions: Vec<DirectionResponse>,
    targets: Vec<TargetResponse>,
    assumptions: Vec<AssumptionResponse>,
    experiments: Vec<ExperimentResponse>,
    observations: Vec<ObservationResponse>,
    reflections: Vec<ReflectionResponse>,
    process_changes: Vec<ProcessChangeResponse>,
    audit: Vec<AuditResponse>,
}

impl From<&MemorySnapshot> for ExportResponse {
    fn from(s: &MemorySnapshot) -> Self {
        Self {
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
