//! HQ's API, for the Endora node only: the snapshot, the events after an id, and the owner's
//! own actions (goals, work, desk decisions, the kill switch), each signed with the shared
//! token. The node authenticates the owner and records every action in its audit log; HQ
//! never sees the owner's sign-in. Beside it are a worker's door and a runner's, each opened
//! by its own token and never by the node's.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, Request, State as Extract};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;

use endora_team::application::engine::{Author, EngineError, Fired};
use endora_team::application::ports::{Clock, NewRoutine, NewTask};
use endora_team::domain::{
    DeskId, GoalId, MemberId, Priority, ProjectId, Refusal, Role, RoutineId, RunId, TaskId,
};
use endora_team::infrastructure::demo;

use crate::Hq;

/// The system clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

/// What every handler shares.
#[derive(Clone)]
pub struct State {
    /// The engine.
    pub hq: Arc<Hq>,
    /// The token the node signs with.
    pub token: Arc<String>,
    /// The token a runner signs with; with none, no runner is answered.
    pub runner_token: Option<Arc<String>>,
    /// Whether the scripted demo team is on.
    pub demo: bool,
    /// How often the engine ticks, for the office's clock.
    pub tick_seconds: u64,
}

/// The demo's goal: set up the demo team the first time, and find its goal after (or give
/// it a new one, if the demo's was put away while the demo was off).
///
/// # Errors
/// What the engine refuses, or a store failure.
pub fn demo_goal(hq: &Hq) -> Result<GoalId, EngineError> {
    demo::goal(hq)
}

/// HQ's routes.
pub fn app(state: State) -> Router {
    let signed = Router::new()
        .route("/v1/snapshot", get(snapshot))
        .route("/v1/events", get(events))
        // Replay: a turn, or a task's turns, read back in full for the owner's screen.
        .route("/v1/runs/{id}/replay", get(run_replay))
        .route("/v1/tasks/{id}/replay", get(task_replay))
        .route("/v1/goals", post(set_goal))
        .route("/v1/tasks", post(add_task))
        .route("/v1/desk/{id}", post(decide))
        // The owner's reply on a task, and a goal's notes for the team.
        .route("/v1/tasks/{id}/post", post(reply))
        .route("/v1/goals/{id}/note", post(goal_note))
        // A goal's strategy, the owner's edit of the CEO's (ADR 0120).
        .route("/v1/goals/{id}/strategy", post(goal_strategy))
        // What a member thinks with (ADR 0109): the owner's, through the node.
        .route("/v1/members/{id}/brain", post(set_brain))
        // What runs a member's turns (ADR 0117): the worker, or Claude Code for a developer.
        .route("/v1/members/{id}/runtime", post(set_runtime))
        // The owner renames a member, and reads and edits the CEO's house log (ADR 0111).
        .route("/v1/members/{id}/name", post(rename))
        .route("/v1/house", post(house_log))
        // Projects and whom members report to (ADR 0113): the owner's, through the node.
        .route("/v1/members/{id}/reports_to", post(reports_to))
        // Elastic staffing (ADR 0118): the owner furloughs and recalls any member but the CEO.
        // There is no route that removes a member: nobody is ever fired.
        .route("/v1/members/{id}/furlough", post(furlough))
        .route("/v1/members/{id}/recall", post(recall))
        // The owner as landlord of the office (ADR 0119): undo the last change, reset it to the
        // layout it began with, veto an open proposal, and say what paid members' downtime may
        // spend a day. The team's own proposals and votes come through the worker's door.
        .route("/v1/office/undo", post(office_undo))
        .route("/v1/office/reset", post(office_reset))
        .route("/v1/office/proposals/{id}/veto", post(office_veto))
        .route("/v1/office/downtime", post(office_downtime))
        // A member's notebook (ADR 0118): the owner reads it, and writes, edits and deletes
        // notes in it (the node maps its DELETE to the last route).
        .route(
            "/v1/members/{id}/notebook",
            get(notebook).post(notebook_add),
        )
        .route("/v1/members/{id}/notebook/{note}", post(notebook_edit))
        .route(
            "/v1/members/{id}/notebook/{note}/delete",
            post(notebook_delete),
        )
        // The owner hires (ADR 0115): a name, a role, a brain, whom it reports to.
        .route("/v1/members", post(hire))
        .route("/v1/projects", post(add_project))
        .route("/v1/projects/{id}", post(change_project))
        .route("/v1/projects/{id}/cap", post(project_cap))
        .route("/v1/stop", post(stop))
        .route("/v1/resume", post(resume))
        // Routines (ADR 0107): the owner's, through the node; and Endora's alarms.
        .route("/v1/routines", post(add_routine))
        .route("/v1/routines/{id}", post(switch_routine))
        .route("/v1/routines/{id}/fire", post(fire_routine))
        .route("/v1/alarms", post(alarm))
        .route("/v1/alarms/recovered", post(recovered))
        // The home lab's containers, as the node sees them: what a question to the owner is
        // checked against before it reaches the desk.
        .route("/v1/lab/names", post(lab_names))
        // The node's model relay: whose a turn's token is, and what each call cost.
        .route("/v1/team/turn", post(crate::team::turn))
        .route("/v1/team/usage", post(crate::team::usage))
        // The node's receipts (ADR 0114): each call a turn made of Endora's tools, with the
        // turn's own token, written by the node and never by the model.
        .route("/v1/team/receipt", post(crate::team::receipt))
        // The node's contracts: a safe fix a member made, told to the owner (ADR 0106).
        .route("/v1/team/notice", post(crate::team::notice))
        // The bridge (ADR 0108): an action waiting for the owner's yes, and what came of it.
        .route("/v1/team/approval", post(crate::team::approval))
        .route(
            "/v1/team/approval/{id}/outcome",
            post(crate::team::approval_outcome),
        )
        // The wing's git flow (ADR 0115): a branch the node took, the check a job token names,
        // and what came of a push the owner approved.
        .route("/v1/team/branch", post(crate::team::branch))
        .route("/v1/team/check", post(crate::team::check))
        .route(
            "/v1/team/push/{id}/outcome",
            post(crate::team::push_outcome),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            signed_by_the_node,
        ));
    Router::new()
        .route("/health", get(health))
        // A worker's door: its turn's token, not the node's (spec section 6).
        .route("/v1/team/mcp", post(crate::team::mcp))
        // A runner's door: the runner's token, not the node's.
        .merge(crate::runner::routes(state.clone()))
        // A check job's report (ADR 0115): the job's own token, which HQ made for it.
        .route(
            "/v1/runner/check/{id}/done",
            post(crate::runner::check_done),
        )
        .merge(signed)
        .with_state(state)
}

/// Equal in time whatever the input, so the token cannot be guessed a byte at a time.
pub(crate) fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The bearer token a request offers, or nothing.
pub(crate) fn bearer(request: &Request) -> &str {
    request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
}

async fn signed_by_the_node(
    Extract(state): Extract<State>,
    request: Request,
    next: Next,
) -> Response {
    let offered = bearer(&request);
    if !same(offered.as_bytes(), state.token.as_bytes()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "not signed by the node" })),
        )
            .into_response();
    }
    next.run(request).await
}

/// A failure, as the node receives it.
struct Failed(EngineError);

impl IntoResponse for Failed {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            EngineError::Unknown(_) => StatusCode::NOT_FOUND,
            EngineError::Refused(Refusal::Finished) => StatusCode::CONFLICT,
            EngineError::Refused(_) => StatusCode::BAD_REQUEST,
            EngineError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({ "error": self.0.to_string() }))).into_response()
    }
}

/// Runs engine work off the async threads: the store is SQLite, and the engine holds a lock.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, EngineError> + Send + 'static,
) -> Result<T, Failed> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| {
            Failed(EngineError::Store(
                endora_team::application::ports::StoreError("a worker thread failed".to_owned()),
            ))
        })?
        .map_err(Failed)
}

async fn health(Extract(state): Extract<State>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "demo": state.demo }))
}

async fn snapshot(Extract(state): Extract<State>) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let mut snap = blocking(move || hq.snapshot()).await?;
    snap["demo"] = json!(state.demo);
    snap["tick_seconds"] = json!(state.tick_seconds);
    Ok(Json(snap))
}

#[derive(Deserialize)]
struct After {
    #[serde(default)]
    after: i64,
    /// With `before`, the past instead: the events just below this id.
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    limit: Option<usize>,
}

/// The events after an id, at most 500 at a time: the stream, resumed from the last id seen.
/// With `before` (and a `limit`, 50 by default), the events just below that id instead, oldest
/// first: the activity log asking for what it did not open with.
async fn events(
    Extract(state): Extract<State>,
    Query(q): Query<After>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let events = blocking(move || match q.before {
        Some(before) => hq.events_before(before, q.limit.unwrap_or(50)),
        None => hq.events_after(q.after),
    })
    .await?;
    let rows: Vec<serde_json::Value> = events
        .iter()
        .take(500)
        .map(|e| json!({ "id": e.id, "at": e.at, "kind": e.kind, "data": e.data }))
        .collect();
    Ok(Json(json!({ "events": rows })))
}

/// A turn read back: everything that happened in it, in order. Confidential excerpts are in
/// it: this route is the owner's, through the node, as every signed one is.
async fn run_replay(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let replay = blocking(move || hq.replay_run(RunId(id))).await?;
    Ok(Json(json!({ "replay": replay.shown() })))
}

/// A task's turns read back, oldest first.
async fn task_replay(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let runs = blocking(move || hq.replay_task(TaskId(id))).await?;
    let shown: Vec<serde_json::Value> = runs.iter().map(|r| r.shown()).collect();
    Ok(Json(json!({ "task": id, "runs": shown })))
}

#[derive(Deserialize)]
struct GoalRequest {
    title: String,
    #[serde(default)]
    reason: String,
    /// Who it comes from: the owner, or Endora proposing it.
    #[serde(default)]
    from_endora: bool,
}

async fn set_goal(
    Extract(state): Extract<State>,
    Json(req): Json<GoalRequest>,
) -> Result<Json<serde_json::Value>, Failed> {
    if req.title.trim().is_empty() {
        return Err(Failed(EngineError::Unknown("goal title")));
    }
    let hq = state.hq.clone();
    let by = if req.from_endora {
        Author::Endora
    } else {
        Author::Owner
    };
    let goal = blocking(move || hq.set_goal(req.title.trim(), req.reason.trim(), by)).await?;
    Ok(Json(json!({ "goal": goal.0 })))
}

#[derive(Deserialize)]
struct TaskRequest {
    goal: i64,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    assignee: Option<i64>,
    #[serde(default)]
    priority: Option<String>,
    /// The project of the goal it goes in, if any (ADR 0113).
    #[serde(default)]
    project: Option<i64>,
    /// What done means, a line each; `live:` before one marks it about the lab now
    /// (ADR 0114). The owner's tasks may have none.
    #[serde(default)]
    criteria: Vec<String>,
}

/// The owner hands the team a piece of work.
async fn add_task(
    Extract(state): Extract<State>,
    Json(req): Json<TaskRequest>,
) -> Result<Json<serde_json::Value>, Failed> {
    let priority = match req.priority.as_deref() {
        None => Priority::Normal,
        Some(name) => Priority::from_name(name).ok_or(Failed(EngineError::Unknown("priority")))?,
    };
    if req.title.trim().is_empty() {
        return Err(Failed(EngineError::Unknown("task title")));
    }
    let hq = state.hq.clone();
    let criteria = endora_team::domain::Criterion::from_lines(&req.criteria);
    let task = blocking(move || {
        hq.add_task_with(
            Author::Owner,
            NewTask {
                goal: GoalId(req.goal),
                project: req.project.map(ProjectId),
                parent: None,
                title: req.title.trim().to_owned(),
                description: req.description,
                assignee: req.assignee.map(MemberId),
                priority,
                waits_on: Vec::new(),
            },
            &criteria,
        )
    })
    .await?;
    Ok(Json(json!({ "task": task.0 })))
}

#[derive(Deserialize)]
struct Decision {
    decision: String,
    /// For a question seen, or the CEO's decision: the owner's words, written in its task's
    /// thread first.
    #[serde(default)]
    answer: String,
}

async fn decide(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<Decision>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let push = blocking(move || {
        hq.decide_saying(DeskId(id), &req.decision, &req.answer)?;
        // A push the owner approved (ADR 0115): the node is told which branch to push.
        hq.push_asked(DeskId(id))
    })
    .await?;
    let mut answer = json!({ "decided": true });
    if let Some(branch) = push {
        answer["push"] = json!({
            "item": id, "task": branch.task.0, "branch": branch.name, "head": branch.head,
        });
    }
    Ok(Json(answer))
}

#[derive(Deserialize)]
struct HireRequest {
    name: String,
    role: String,
    #[serde(default)]
    brain: Option<String>,
    #[serde(default)]
    reports_to: Option<i64>,
}

/// The owner hires a member (ADR 0115).
async fn hire(
    Extract(state): Extract<State>,
    Json(req): Json<HireRequest>,
) -> Result<Json<serde_json::Value>, Failed> {
    let role = Role::from_name(req.role.trim()).ok_or(Failed(EngineError::Unknown("role")))?;
    let hq = state.hq.clone();
    let brain = req.brain.unwrap_or_else(|| "local".to_owned());
    let id =
        blocking(move || hq.hire_by_owner(&req.name, role, &brain, req.reports_to.map(MemberId)))
            .await?;
    Ok(Json(json!({ "member": id.0 })))
}

#[derive(Deserialize)]
struct NameAsked {
    name: String,
}

/// The owner renames a member.
async fn rename(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<NameAsked>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let name = blocking(move || hq.rename(MemberId(id), &req.name)).await?;
    Ok(Json(json!({ "member": id, "name": name })))
}

#[derive(Deserialize)]
struct HouseLog {
    #[serde(default)]
    log: String,
}

/// The owner rewrites the house log, which the CEO keeps; empty clears it.
async fn house_log(
    Extract(state): Extract<State>,
    Json(req): Json<HouseLog>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.set_house_log(&req.log)).await?;
    Ok(Json(json!({ "kept": true })))
}

#[derive(Deserialize)]
struct Reply {
    text: String,
}

/// The most of an owner's reply, as of any post in a thread.
const LONGEST_REPLY: usize = 4_000;

/// The owner writes in a task's thread: whoever has the task is woken to read it at once.
async fn reply(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<Reply>,
) -> Response {
    let text = req.text.trim().to_owned();
    if text.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "a reply says something" })),
        )
            .into_response();
    }
    if text.chars().count() > LONGEST_REPLY {
        return Failed(EngineError::Refused(Refusal::TooLong(LONGEST_REPLY))).into_response();
    }
    let hq = state.hq.clone();
    match blocking(move || hq.post(Author::Owner, TaskId(id), &text)).await {
        Ok(()) => Json(json!({ "posted": true })).into_response(),
        Err(failed) => failed.into_response(),
    }
}

#[derive(Deserialize)]
struct Note {
    #[serde(default)]
    note: String,
}

/// The owner writes a goal's notes, which every briefing under it carries; empty clears them.
async fn goal_note(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<Note>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.set_goal_note(GoalId(id), &req.note)).await?;
    Ok(Json(json!({ "noted": true })))
}

#[derive(Deserialize)]
struct StrategyAsked {
    #[serde(default)]
    purpose: Option<String>,
    #[serde(default)]
    success: Option<String>,
    #[serde(default)]
    measures: Option<String>,
    #[serde(default)]
    bets: Option<String>,
    #[serde(default)]
    not_now: Option<String>,
}

/// The owner writes a goal's strategy (ADR 0120): each part given replaces that part.
async fn goal_strategy(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<StrategyAsked>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let edit = endora_team::domain::areas::StrategyEdit {
        purpose: req.purpose,
        success: req.success,
        measures: req.measures,
        bets: req.bets,
        not_now: req.not_now,
    };
    let written = blocking(move || hq.set_strategy(GoalId(id), &edit)).await?;
    Ok(Json(json!({ "goal": id, "by": written.by })))
}

#[derive(Deserialize)]
struct BrainAsked {
    brain: String,
}

/// The owner sets what a member thinks with: `local`, or a Claude its role may use.
async fn set_brain(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<BrainAsked>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let brain = blocking(move || hq.set_brain(MemberId(id), &req.brain)).await?;
    Ok(Json(json!({ "member": id, "brain": brain.name() })))
}

#[derive(Deserialize)]
struct RuntimeAsked {
    runtime: String,
}

/// The owner sets what runs a member's turns: `worker`, or `claude-code` for a developer on
/// Claude (ADR 0117).
async fn set_runtime(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<RuntimeAsked>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let runtime = blocking(move || hq.set_runtime(MemberId(id), &req.runtime)).await?;
    Ok(Json(json!({ "member": id, "runtime": runtime.name() })))
}

#[derive(Deserialize)]
struct Why {
    #[serde(default)]
    why: String,
}

async fn stop(
    Extract(state): Extract<State>,
    Json(req): Json<Why>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.stop_all(&req.why)).await?;
    Ok(Json(json!({ "stopped": true })))
}

async fn resume(Extract(state): Extract<State>) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.resume()).await?;
    Ok(Json(json!({ "stopped": false })))
}

#[derive(Deserialize)]
struct RoutineRequest {
    title: String,
    #[serde(default)]
    description: String,
    goal: i64,
    assignee: i64,
    every_ms: i64,
    #[serde(default)]
    on_lab_trouble: bool,
    #[serde(default)]
    project: Option<i64>,
}

/// The owner makes a routine: work that comes round again, as a task for one member.
async fn add_routine(
    Extract(state): Extract<State>,
    Json(req): Json<RoutineRequest>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let id = blocking(move || {
        hq.add_routine(
            Author::Owner,
            NewRoutine {
                title: req.title,
                description: req.description.trim().to_owned(),
                goal: GoalId(req.goal),
                project: req.project.map(ProjectId),
                assignee: MemberId(req.assignee),
                every_ms: req.every_ms,
                on_lab_trouble: req.on_lab_trouble,
            },
        )
    })
    .await?;
    Ok(Json(json!({ "routine": id.0 })))
}

#[derive(Deserialize)]
struct ReportsTo {
    to: i64,
}

/// The owner sets whom a member reports to.
async fn reports_to(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<ReportsTo>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.set_reports_to(MemberId(id), MemberId(req.to))).await?;
    Ok(Json(json!({ "member": id, "reports_to": req.to })))
}

/// The owner puts a member on furlough: never woken, nothing held, nothing deleted.
async fn furlough(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.furlough(MemberId(id))).await?;
    Ok(Json(json!({ "member": id, "furloughed": true })))
}

/// The owner brings a furloughed member back to work.
async fn recall(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.recall(MemberId(id))).await?;
    Ok(Json(json!({ "member": id, "furloughed": false })))
}

/// The owner undoes the last change to the office; nothing happens if there is none.
async fn office_undo(Extract(state): Extract<State>) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let undone = blocking(move || hq.undo_office()).await?;
    Ok(Json(json!({ "undone": undone })))
}

/// The owner puts the office back as it began.
async fn office_reset(Extract(state): Extract<State>) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.reset_office()).await?;
    Ok(Json(json!({ "reset": true })))
}

/// The owner vetoes an open proposal for the office.
async fn office_veto(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.veto_office(id)).await?;
    Ok(Json(json!({ "proposal": id, "vetoed": true })))
}

#[derive(Deserialize)]
struct DowntimeAllowance {
    cents: i64,
}

/// The owner says what paid members' downtime turns may spend together in a day, in cents.
async fn office_downtime(
    Extract(state): Extract<State>,
    Json(req): Json<DowntimeAllowance>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.set_downtime_paid_cents(req.cents)).await?;
    Ok(Json(json!({ "downtime_paid_cents_per_day": req.cents })))
}

#[derive(Deserialize)]
struct NoteText {
    text: String,
}

/// A member's notebook, for the owner to read.
async fn notebook(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    Ok(Json(
        blocking(move || hq.notebook_shown(MemberId(id))).await?,
    ))
}

/// The owner adds a note to a member's notebook: no evidence, and it says it is theirs.
async fn notebook_add(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<NoteText>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let note = blocking(move || hq.owner_note(MemberId(id), &req.text)).await?;
    Ok(Json(json!({ "member": id, "note": note })))
}

/// The owner rewrites a note in a member's notebook.
async fn notebook_edit(
    Extract(state): Extract<State>,
    Path((id, note)): Path<(i64, i64)>,
    Json(req): Json<NoteText>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.owner_edit_note(MemberId(id), note, &req.text)).await?;
    Ok(Json(json!({ "member": id, "note": note })))
}

/// The owner deletes a note from a member's notebook.
async fn notebook_delete(
    Extract(state): Extract<State>,
    Path((id, note)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.owner_delete_note(MemberId(id), note)).await?;
    Ok(Json(json!({ "member": id, "note": note, "deleted": true })))
}

/// A monthly cap as the screen sends it: dollars, as a number or as text; nothing for none.
fn cap_of(cap: Option<&serde_json::Value>) -> Result<Option<i64>, Failed> {
    let said = match cap {
        None | Some(serde_json::Value::Null) => return Ok(None),
        Some(serde_json::Value::String(s)) if s.trim().is_empty() => return Ok(None),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(_) => String::new(),
    };
    endora_team::domain::projects::read_dollars(&said)
        .map(Some)
        .ok_or(Failed(EngineError::Unknown("cap, in dollars")))
}

#[derive(Deserialize)]
struct ProjectRequest {
    goal: i64,
    title: String,
    #[serde(default)]
    reason: String,
    lead: i64,
    #[serde(default)]
    cap: Option<serde_json::Value>,
}

/// The owner makes a project, active at once.
async fn add_project(
    Extract(state): Extract<State>,
    Json(req): Json<ProjectRequest>,
) -> Result<Json<serde_json::Value>, Failed> {
    let cap = cap_of(req.cap.as_ref())?;
    let hq = state.hq.clone();
    let id = blocking(move || {
        hq.add_project(
            GoalId(req.goal),
            &req.title,
            &req.reason,
            MemberId(req.lead),
            cap,
        )
    })
    .await?;
    Ok(Json(json!({ "project": id.0 })))
}

#[derive(Deserialize)]
struct ProjectAction {
    action: String,
}

/// The owner pauses, resumes, drops, reopens or finishes a project.
async fn change_project(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<ProjectAction>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let now = blocking(move || hq.change_project(ProjectId(id), &req.action)).await?;
    Ok(Json(json!({ "project": id, "status": now.name() })))
}

#[derive(Deserialize)]
struct ProjectCap {
    #[serde(default)]
    cap: Option<serde_json::Value>,
}

/// The owner sets, raises or takes away a project's cap.
async fn project_cap(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<ProjectCap>,
) -> Result<Json<serde_json::Value>, Failed> {
    let cap = cap_of(req.cap.as_ref())?;
    let hq = state.hq.clone();
    blocking(move || hq.set_project_cap(ProjectId(id), cap)).await?;
    Ok(Json(json!({ "project": id, "cap": cap })))
}

#[derive(Deserialize)]
struct Switch {
    enabled: bool,
}

/// The owner switches a routine on or off.
async fn switch_routine(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<Switch>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.switch_routine(RoutineId(id), req.enabled)).await?;
    Ok(Json(json!({ "routine": id, "enabled": req.enabled })))
}

/// What a firing did, as the node reads it.
fn fired(fired: &Fired) -> serde_json::Value {
    match fired {
        Fired::Created(task) => json!({ "fired": "created", "task": task.0 }),
        Fired::Posted(task) => json!({ "fired": "posted", "task": task.0 }),
        Fired::Skipped(why) => json!({ "fired": "skipped", "why": why }),
    }
}

/// The owner runs a routine now.
async fn fire_routine(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let done = blocking(move || hq.fire_routine(RoutineId(id))).await?;
    Ok(Json(fired(&done)))
}

#[derive(Deserialize)]
struct AlarmRequest {
    text: String,
}

/// Endora's node raises an alarm about the home lab: the routine that takes them fires. The
/// demo's scripted team takes none: nothing in it is real.
async fn alarm(
    Extract(state): Extract<State>,
    Json(req): Json<AlarmRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    if state.demo {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "the demo is on: it takes no alarms" })),
        )
            .into_response());
    }
    let hq = state.hq.clone();
    let done = blocking(move || hq.alarm(&req.text))
        .await
        .map_err(IntoResponse::into_response)?;
    let Some((routine, done)) = done else {
        return Ok(Json(json!({ "fired": "nobody" })));
    };
    let mut said = fired(&done);
    said["routine"] = json!(routine.0);
    Ok(Json(said))
}

/// Endora's node says something in the home lab that alarmed is back up: said in the open
/// task the alarm went to, waking whoever has it. It hands out no work (`"posted": null`
/// when no alarm's task is open), and the demo takes none.
async fn recovered(
    Extract(state): Extract<State>,
    Json(req): Json<AlarmRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    if state.demo {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "the demo is on: it takes no alarms" })),
        )
            .into_response());
    }
    let hq = state.hq.clone();
    let posted = blocking(move || hq.recovered(&req.text))
        .await
        .map_err(IntoResponse::into_response)?;
    Ok(Json(json!({ "posted": posted.map(|t| t.0) })))
}

#[derive(Deserialize)]
struct LabNames {
    names: Vec<String>,
}

/// The node says which containers the home lab has now. Kept in memory: the node says them
/// again every ten minutes, so a restarted HQ learns them soon.
async fn lab_names(
    Extract(state): Extract<State>,
    Json(req): Json<LabNames>,
) -> Json<serde_json::Value> {
    let names: Vec<String> = req
        .names
        .into_iter()
        .map(|n| n.trim().chars().take(128).collect::<String>())
        .filter(|n| !n.is_empty())
        .take(1000)
        .collect();
    let count = names.len();
    state.hq.set_lab_names(names);
    Json(json!({ "ok": true, "names": count }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use endora_team::application::engine::Settings;
    use endora_team::application::ports::TeamStore as _;
    use endora_team::domain::{Role, RunId, RunStatus, TaskId};
    use endora_team::infrastructure::{RemoteRunner, Runner, SimulatedRunner, SqliteTeamStore};
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    const TOKEN: &str = "a-token-of-at-least-thirty-two-characters";
    const RUNNER: &str = "a-runner-token-of-at-least-thirty-two-chars";

    /// HQ as it runs: the demo team on the simulated runner, or no demo and the remote one.
    fn state(demo: bool) -> State {
        let runner = if demo {
            Runner::Simulated(SimulatedRunner::new(4))
        } else {
            Runner::Remote(RemoteRunner::default())
        };
        let hq = Arc::new(endora_team::application::engine::Engine::new(
            SqliteTeamStore::in_memory().unwrap(),
            runner,
            SystemClock,
            Settings::default(),
        ));
        if demo {
            demo_goal(&hq).unwrap();
        }
        State {
            hq,
            token: Arc::new(TOKEN.to_owned()),
            runner_token: Some(Arc::new(RUNNER.to_owned())),
            demo,
            tick_seconds: 30,
        }
    }

    fn scripted(st: &State) -> &SimulatedRunner {
        st.hq.runner().simulated().unwrap()
    }

    /// A developer is handed a task, and the tick starts their turn: it waits for a runner.
    fn a_turn_starts(st: &State) -> (RunId, TaskId) {
        let dev = st.hq.hire("John Doe", Role::Developer, 40, 2, 0).unwrap();
        let goal = st
            .hq
            .set_goal("Keep the lab tidy", "a test", Author::Owner)
            .unwrap();
        let task = st
            .hq
            .add_task(
                Author::Owner,
                NewTask {
                    goal,
                    project: None,
                    parent: None,
                    title: "Sort the cables".to_owned(),
                    description: String::new(),
                    assignee: Some(dev),
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
            )
            .unwrap();
        assert_eq!(st.hq.tick().unwrap().started, 1);
        let run = st.hq.store().runs().unwrap()[0].id;
        (run, task)
    }

    fn status_of(st: &State, run: RunId) -> RunStatus {
        st.hq.store().run(run).unwrap().unwrap().status
    }

    fn call(method: &str, uri: &str, token: Option<&str>, body: &str) -> HttpRequest<Body> {
        let mut b = HttpRequest::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(token) = token {
            b = b.header("authorization", format!("Bearer {token}"));
        }
        b.body(Body::from(body.to_owned())).unwrap()
    }

    async fn json_of(res: Response) -> serde_json::Value {
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn only_the_node_is_answered() {
        let app = app(state(true));
        for token in [
            None,
            Some("wrong"),
            Some("a-token-of-at-least-thirty-two-characterz"),
        ] {
            let res = app
                .clone()
                .oneshot(call("GET", "/v1/snapshot", token, ""))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{token:?}");
        }
        let res = app
            .clone()
            .oneshot(call("GET", "/health", None, ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "health needs no token");
    }

    #[tokio::test]
    async fn the_events_before_an_id_are_the_past_the_log_scrolls_back_into() {
        let app = app(state(true));
        let snap = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        let last = snap["last_event"].as_i64().unwrap();
        assert!(last > 3, "the demo has a past");
        let page = json_of(
            app.clone()
                .oneshot(call(
                    "GET",
                    &format!("/v1/events?before={last}&limit=2"),
                    Some(TOKEN),
                    "",
                ))
                .await
                .unwrap(),
        )
        .await;
        let ids: Vec<i64> = page["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_i64().unwrap())
            .collect();
        assert_eq!(
            ids,
            [last - 2, last - 1],
            "the two just before, oldest first"
        );
        let none = json_of(
            app.oneshot(call("GET", "/v1/events?before=1&limit=50", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        assert!(none["events"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_snapshot_and_the_events_after_it_are_the_contract() {
        let app = app(state(true));
        let snap = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(snap["demo"], true);
        assert_eq!(snap["members"].as_array().unwrap().len(), 4);
        let last = snap["last_event"].as_i64().unwrap();
        let res = app
            .clone()
            .oneshot(call(
                "POST",
                "/v1/goals",
                Some(TOKEN),
                r#"{"title":"Learn the new lab layout","reason":"you moved the rack"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let after = json_of(
            app.oneshot(call(
                "GET",
                &format!("/v1/events?after={last}"),
                Some(TOKEN),
                "",
            ))
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(after["events"][0]["kind"], "goal.changed");
        assert_eq!(
            after["events"][0]["data"]["title"],
            "Learn the new lab layout"
        );
    }

    #[tokio::test]
    async fn the_owner_decides_and_pulls_the_kill_switch_through_the_node() {
        let app = app(state(true));
        let snap = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        let proposal = snap["desk"][0]["id"].as_i64().unwrap();
        let decided = app
            .clone()
            .oneshot(call(
                "POST",
                &format!("/v1/desk/{proposal}"),
                Some(TOKEN),
                r#"{"decision":"approve"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(decided.status(), StatusCode::OK);
        let again = app
            .clone()
            .oneshot(call(
                "POST",
                &format!("/v1/desk/{proposal}"),
                Some(TOKEN),
                r#"{"decision":"approve"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(again.status(), StatusCode::CONFLICT, "decided once");
        let stop = app
            .clone()
            .oneshot(call(
                "POST",
                "/v1/stop",
                Some(TOKEN),
                r#"{"why":"testing"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(stop.status(), StatusCode::OK);
        let snap = json_of(
            app.oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(snap["stopped"], true);
    }

    #[tokio::test]
    async fn the_owner_answers_a_question_from_the_desk_into_its_thread() {
        let st = state(false);
        let (run, task) = a_turn_starts(&st);
        let endora_team::application::Asked::Desk(asked) =
            st.hq.ask_owner(run, "Which cables first?").unwrap()
        else {
            panic!("no reviewer: straight to the desk");
        };
        let app = app(st.clone());
        let decide = |body: &str| call("POST", &format!("/v1/desk/{}", asked.0), Some(TOKEN), body);
        let res = app
            .clone()
            .oneshot(decide(
                r#"{"decision":"seen","answer":"The network ones."}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let thread = st.hq.store().thread(task).unwrap();
        let said = thread.last().unwrap();
        assert_eq!(
            (said.author.as_str(), said.text.as_str()),
            ("owner", "The network ones.")
        );
        let again = app.oneshot(decide(r#"{"decision":"seen"}"#)).await.unwrap();
        assert_eq!(again.status(), StatusCode::CONFLICT, "decided once");
    }

    #[tokio::test]
    async fn mcp_answers_batches_and_never_answers_a_notification() {
        let st = state(true);
        st.hq.tick().unwrap();
        let turn = scripted(&st).started()[0].clone();
        let app = app(st.clone());
        let speak = |body: serde_json::Value| {
            call("POST", "/v1/team/mcp", Some(&turn.token), &body.to_string())
        };
        let batch = json_of(
            app.clone()
                .oneshot(speak(json!([
                    { "jsonrpc": "2.0", "id": 7, "method": "tools/list" },
                    { "jsonrpc": "2.0", "method": "notifications/initialized" },
                    { "jsonrpc": "2.0", "id": "b", "method": "ping" },
                ])))
                .await
                .unwrap(),
        )
        .await;
        let replies = batch.as_array().expect("a batch is answered with a batch");
        assert_eq!(replies.len(), 2, "the notification has no answer: {batch}");
        assert_eq!(replies[0]["id"], 7);
        assert!(replies[0]["result"]["tools"].is_array());
        assert_eq!(replies[1]["id"], "b");
        for quiet in [
            json!({ "jsonrpc": "2.0", "method": "ping" }),
            json!({ "jsonrpc": "2.0", "id": 3, "result": {} }),
            json!([{ "jsonrpc": "2.0", "method": "notifications/cancelled" }]),
        ] {
            let res = app.clone().oneshot(speak(quiet.clone())).await.unwrap();
            assert_eq!(res.status(), StatusCode::ACCEPTED, "{quiet}");
            let body = res.into_body().collect().await.unwrap().to_bytes();
            assert!(body.is_empty(), "{quiet}");
        }
        let asked = json_of(
            app.clone()
                .oneshot(speak(
                    json!({ "jsonrpc": "2.0", "id": 4, "method": "notifications/odd" }),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            (asked["id"].clone(), asked["error"]["code"].clone()),
            (json!(4), json!(-32601)),
            "a request is answered, whatever its method"
        );
        let empty = json_of(app.oneshot(speak(json!([]))).await.unwrap()).await;
        assert_eq!(empty["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn a_worker_speaks_mcp_with_its_turns_token_and_nothing_else_opens_the_door() {
        let st = state(true);
        st.hq.tick().unwrap();
        let turn = scripted(&st).started()[0].clone();
        let app = app(st.clone());
        let rpc = |id: i64, method: &str, params: serde_json::Value| {
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
        };
        // The node's token is not a worker's, and neither is a guess.
        for token in [Some(TOKEN), Some("not-a-turn"), None] {
            let res = app
                .clone()
                .oneshot(call(
                    "POST",
                    "/v1/team/mcp",
                    token,
                    &rpc(1, "tools/list", json!({})),
                ))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{token:?}");
        }
        let speak = |body: String| call("POST", "/v1/team/mcp", Some(&turn.token), &body);
        let hello = json_of(
            app.clone()
                .oneshot(speak(rpc(
                    1,
                    "initialize",
                    json!({ "protocolVersion": "2025-06-18" }),
                )))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(hello["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(hello["result"]["serverInfo"]["name"], "endora-hq");
        let noted = app
            .clone()
            .oneshot(speak(
                json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(noted.status(), StatusCode::ACCEPTED);
        let listed = json_of(
            app.clone()
                .oneshot(speak(rpc(2, "tools/list", json!({}))))
                .await
                .unwrap(),
        )
        .await;
        let names: Vec<&str> = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        // The demo's first turn is its lead's, who keeps the team's lessons and proposes
        // projects, accepts tested work, hands a subordinate's task on, asks the owner to hire
        // and reads a task's documents and the team's library, too, and asks for staff when a
        // project has a backlog. Every member keeps a notebook of its own, too (ADR 0118):
        // note, forget, read and check. Every member shapes the office (ADR 0119). And it
        // reports on its area, decides its area's stuck work, and gives its view in a huddle
        // (ADR 0120).
        assert_eq!(names.len(), 25, "{names:?}");
        assert!(names.contains(&"office"), "every member shapes the office");
        assert!(names.contains(&"note_lesson") && names.contains(&"propose_project"));
        for added in ["report_area", "decide_stuck", "huddle_view"] {
            assert!(names.contains(&added), "{added}");
        }
        assert!(names.contains(&"request_staff") && names.contains(&"note"));
        assert!(
            names.contains(&"read_doc"),
            "every member reads the library"
        );
        assert!(names.contains(&"accept") && names.contains(&"hand_to"));
        assert!(names.contains(&"request_hire") && names.contains(&"read_document"));
        assert!(
            !names.contains(&"report_usage"),
            "the relay charges for model use, not the worker"
        );
        let mine = json_of(
            app.clone()
                .oneshot(speak(rpc(
                    3,
                    "tools/call",
                    json!({ "name": "my_turn", "arguments": {} }),
                )))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(mine["result"]["isError"], false);
        assert!(
            !mine["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        let refused = json_of(
            app.clone()
                .oneshot(speak(rpc(
                    4,
                    "tools/call",
                    json!({ "name": "run_gate", "arguments": { "task": 1 } }),
                )))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(refused["result"]["isError"], true);
        let unknown = json_of(
            app.clone()
                .oneshot(speak(rpc(5, "resources/list", json!({}))))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(unknown["error"]["code"], -32601);
        // The turn ends: its token opens nothing any more.
        scripted(&st).finish(turn.run, true);
        st.hq.tick().unwrap();
        let after = app
            .oneshot(speak(rpc(6, "tools/list", json!({}))))
            .await
            .unwrap();
        assert_eq!(after.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn only_the_node_records_a_receipt_and_only_for_a_turn_that_is_running() {
        let st = state(true);
        st.hq.tick().unwrap();
        let turn = scripted(&st).started()[0].clone();
        let app = app(st.clone());
        let tell = |signed: Option<&str>, token: &str| {
            call(
                "POST",
                "/v1/team/receipt",
                signed,
                &json!({ "token": token, "tool": "homelab_status", "args": "",
                         "excerpt": "sonarr: running", "ok": true })
                .to_string(),
            )
        };
        let res = app
            .clone()
            .oneshot(tell(Some(&turn.token), &turn.token))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "a worker cannot write its own receipts"
        );
        let forged = app
            .clone()
            .oneshot(tell(Some(TOKEN), "not-a-turns-token"))
            .await
            .unwrap();
        assert_eq!(forged.status(), StatusCode::NOT_FOUND);
        let kept = json_of(
            app.clone()
                .oneshot(tell(Some(TOKEN), &turn.token))
                .await
                .unwrap(),
        )
        .await;
        let id = kept["receipt"].as_i64().expect("a receipt");
        let found = st.hq.store().call_receipt(id).unwrap().unwrap();
        assert_eq!(
            (found.run, found.tool.as_str(), found.excerpt.as_str()),
            (turn.run, "homelab_status", "sonarr: running")
        );
        assert!(!found.sandbox, "the node witnessed it");
        // What a Claude Code sandbox reported (ADR 0117), told by the node as such.
        let reported = json_of(
            app.clone()
                .oneshot(call(
                    "POST",
                    "/v1/team/receipt",
                    Some(TOKEN),
                    &json!({ "token": turn.token, "tool": "Bash", "args": "git status",
                             "excerpt": "clean", "ok": true, "sandbox": true })
                    .to_string(),
                ))
                .await
                .unwrap(),
        )
        .await;
        let id = reported["receipt"].as_i64().expect("a receipt");
        assert!(st.hq.store().call_receipt(id).unwrap().unwrap().sandbox);
        // Once the turn has ended, its token records nothing more.
        scripted(&st).finish(turn.run, true);
        st.hq.tick().unwrap();
        let late = app.oneshot(tell(Some(TOKEN), &turn.token)).await.unwrap();
        assert_eq!(late.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn the_owner_replays_a_turn_and_a_tasks_turns_through_the_node_only() {
        let st = state(true);
        st.hq.tick().unwrap();
        let turn = scripted(&st).started()[0].clone();
        let app = app(st.clone());
        let receipt = json!({ "token": turn.token, "tool": "container_logs",
                              "args": "container: sonarr", "excerpt": "<b>500</b>",
                              "ok": true, "confidential": true })
        .to_string();
        app.clone()
            .oneshot(call("POST", "/v1/team/receipt", Some(TOKEN), &receipt))
            .await
            .unwrap();
        let run = format!("/v1/runs/{}/replay", turn.run.0);
        for token in [None, Some("wrong"), Some(turn.token.as_str()), Some(RUNNER)] {
            let res = app
                .clone()
                .oneshot(call("GET", &run, token, ""))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{token:?}");
        }
        let res = app
            .clone()
            .oneshot(call("GET", &run, Some(TOKEN), ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let replay = json_of(res).await["replay"].clone();
        assert_eq!(replay["run"], turn.run.0);
        let call_made = replay["moments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["kind"] == "call")
            .cloned()
            .expect("the call it made");
        assert_eq!(
            (
                call_made["tool"].clone(),
                call_made["excerpt"].clone(),
                call_made["confidential"].clone()
            ),
            (json!("container_logs"), json!("<b>500</b>"), json!(true)),
            "the owner's screen is given the Confidential excerpt, and escapes it"
        );
        let task = turn.task.expect("a turn on a task").0;
        let all = json_of(
            app.clone()
                .oneshot(call(
                    "GET",
                    &format!("/v1/tasks/{task}/replay"),
                    Some(TOKEN),
                    "",
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(all["runs"][0]["run"], turn.run.0);
        for missing in ["/v1/runs/999999/replay", "/v1/tasks/999999/replay"] {
            let res = app
                .clone()
                .oneshot(call("GET", missing, Some(TOKEN), ""))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::NOT_FOUND, "{missing}");
        }
    }

    #[tokio::test]
    async fn the_relay_learns_whose_a_token_is_and_reports_what_each_call_cost() {
        let st = state(true);
        // Ada, the demo's lead, thinks with Claude: her turns are paid from the pool. (One on
        // the house's model reserves nothing, ADR 0111.)
        let ada = st.hq.store().members().unwrap()[0].id;
        st.hq.set_brain(ada, "claude-sonnet-5-5").unwrap();
        st.hq.tick().unwrap();
        let turn = scripted(&st).started()[0].clone();
        let app = app(st.clone());
        let ask = |token: Option<&str>, body: String| call("POST", "/v1/team/turn", token, &body);
        let body = json!({ "token": turn.token }).to_string();
        let res = app
            .clone()
            .oneshot(ask(Some(&turn.token), body.clone()))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "only the node asks, not the worker"
        );
        let found = json_of(app.clone().oneshot(ask(Some(TOKEN), body)).await.unwrap()).await;
        assert_eq!(found["run"], turn.run.0);
        assert_eq!(found["cloud"], true);
        assert_eq!(found["brain"], "claude-sonnet-5-5");
        let left = found["left"].as_i64().unwrap();
        assert_eq!(left, 400_000, "{found}");
        let spend = |micros: i64| {
            call(
                "POST",
                "/v1/team/usage",
                Some(TOKEN),
                &json!({ "run": turn.run.0, "micros": micros }).to_string(),
            )
        };
        let ok = json_of(app.clone().oneshot(spend(10)).await.unwrap()).await;
        assert_eq!(ok["may_go_on"], true);
        let cached = call(
            "POST",
            "/v1/team/usage",
            Some(TOKEN),
            &json!({ "run": turn.run.0, "micros": 10, "input": 100,
                     "cache_read": 9_000, "cache_write": 900, "output": 250 })
            .to_string(),
        );
        let ok = json_of(app.clone().oneshot(cached).await.unwrap()).await;
        assert_eq!(ok["may_go_on"], true);
        let kept = st.hq.store().run(turn.run).unwrap().unwrap();
        assert_eq!(
            (
                kept.input_tokens,
                kept.cache_read_tokens,
                kept.cache_write_tokens,
                kept.output_tokens
            ),
            (100, 9_000, 900, 250),
            "the turn's record keeps the tokens the relay reported, in and out"
        );
        let over = json_of(app.clone().oneshot(spend(left)).await.unwrap()).await;
        assert_eq!(over["may_go_on"], false, "past its budget, the turn stops");
        let gone = app
            .oneshot(ask(Some(TOKEN), json!({ "token": turn.token }).to_string()))
            .await
            .unwrap();
        assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn the_node_tells_the_owner_of_a_contracts_fix_as_a_notice_on_the_desk() {
        let st = state(true);
        st.hq.tick().unwrap();
        let turn = scripted(&st).started()[0].clone();
        let app = app(st.clone());
        let body = json!({ "run": turn.run.0, "text": "Ada restarted sonarr: it was unhealthy. It is now running." })
            .to_string();
        let res = app
            .clone()
            .oneshot(call("POST", "/v1/team/notice", Some(&turn.token), &body))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "only the node tells"
        );
        let told = json_of(
            app.clone()
                .oneshot(call("POST", "/v1/team/notice", Some(TOKEN), &body))
                .await
                .unwrap(),
        )
        .await;
        let item = told["item"].as_i64().unwrap();
        let snap = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        let shown = snap["desk"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == item)
            .cloned()
            .unwrap();
        assert_eq!(shown["kind"], "notice", "{shown}");
        assert!(shown["text"].as_str().unwrap().contains("restarted sonarr"));
        let seen = app
            .oneshot(call(
                "POST",
                &format!("/v1/desk/{item}"),
                Some(TOKEN),
                r#"{"decision":"seen"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(seen.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn the_node_asks_an_approval_and_tells_what_came_of_it_once_approved() {
        let st = state(true);
        st.hq.tick().unwrap();
        let turn = scripted(&st).started()[0].clone();
        let app = app(st.clone());
        let body = json!({ "run": turn.run.0, "text": "Ada asks to run sonarr_retry_import. Why: stuck." })
            .to_string();
        let res = app
            .clone()
            .oneshot(call("POST", "/v1/team/approval", Some(&turn.token), &body))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "only the node asks");
        let asked = json_of(
            app.clone()
                .oneshot(call("POST", "/v1/team/approval", Some(TOKEN), &body))
                .await
                .unwrap(),
        )
        .await;
        let item = asked["item"].as_i64().unwrap();
        // Asked again for the same turn: it has ended, and the refusal says so.
        let again = app
            .clone()
            .oneshot(call("POST", "/v1/team/approval", Some(TOKEN), &body))
            .await
            .unwrap();
        assert_eq!(again.status(), StatusCode::BAD_REQUEST);
        let outcome = |text: &str| {
            call(
                "POST",
                &format!("/v1/team/approval/{item}/outcome"),
                Some(TOKEN),
                &json!({ "text": text }).to_string(),
            )
        };
        let early = app.clone().oneshot(outcome("Done.")).await.unwrap();
        assert_eq!(early.status(), StatusCode::NOT_FOUND, "not approved yet");
        let approved = app
            .clone()
            .oneshot(call(
                "POST",
                &format!("/v1/desk/{item}"),
                Some(TOKEN),
                r#"{"decision":"approve"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(approved.status(), StatusCode::OK);
        let told = app.clone().oneshot(outcome("Done.")).await.unwrap();
        assert_eq!(told.status(), StatusCode::OK);
        let twice = app.oneshot(outcome("Done.")).await.unwrap();
        assert_eq!(twice.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn the_owner_replies_on_a_task_and_writes_a_goals_notes() {
        let st = state(false);
        let app = app(st.clone());
        let goal = json_of(
            app.clone()
                .oneshot(call(
                    "POST",
                    "/v1/goals",
                    Some(TOKEN),
                    r#"{"title":"Keep the home lab healthy"}"#,
                ))
                .await
                .unwrap(),
        )
        .await["goal"]
            .as_i64()
            .unwrap();
        let task = json_of(
            app.clone()
                .oneshot(call(
                    "POST",
                    "/v1/tasks",
                    Some(TOKEN),
                    &json!({ "goal": goal, "title": "Look at the music stack" }).to_string(),
                ))
                .await
                .unwrap(),
        )
        .await["task"]
            .as_i64()
            .unwrap();
        let reply = |text: &str| {
            call(
                "POST",
                &format!("/v1/tasks/{task}/post"),
                Some(TOKEN),
                &json!({ "text": text }).to_string(),
            )
        };
        let res = app.clone().oneshot(reply("Leave it.")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        for bad in ["  ", &"x".repeat(4_001)] {
            let res = app.clone().oneshot(reply(bad)).await.unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{}", bad.len());
        }
        let note = |text: &str| {
            call(
                "POST",
                &format!("/v1/goals/{goal}/note"),
                Some(TOKEN),
                &json!({ "note": text }).to_string(),
            )
        };
        let res = app
            .clone()
            .oneshot(note("The SSD is the owner's."))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let res = app.clone().oneshot(note(&"x".repeat(2_001))).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let snap = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(snap["goals"][0]["note"], "The SSD is the owner's.");
        let thread = &snap["tasks"][0]["thread"];
        assert_eq!(thread[0]["author"], "owner", "{snap}");
        let res = app.oneshot(note("x")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    /// The runner's three routes, each with a body it might send.
    const RUNNER_ROUTES: [(&str, &str, &str); 3] = [
        ("POST", "/v1/runner/next", r#"{"free":1}"#),
        (
            "POST",
            "/v1/runner/1/ended",
            r#"{"succeeded":true,"lease":1}"#,
        ),
        ("GET", "/v1/runner/stops", ""),
    ];

    #[tokio::test]
    async fn only_the_runners_token_opens_the_runners_door() {
        let st = state(false);
        let app = app(st.clone());
        for (method, uri, body) in RUNNER_ROUTES {
            for token in [
                None,
                Some(""),
                Some(TOKEN),
                Some("a-runner-token-of-at-least-thirty-two-chaz"),
            ] {
                let res = app
                    .clone()
                    .oneshot(call(method, uri, token, body))
                    .await
                    .unwrap();
                assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{uri} {token:?}");
            }
            let res = app
                .clone()
                .oneshot(call(method, uri, Some(RUNNER), body))
                .await
                .unwrap();
            // In, though run 1 was never handed out, so its end is refused as not held.
            assert_ne!(res.status(), StatusCode::UNAUTHORIZED, "{uri}");
            assert!(
                res.status().is_success() || res.status() == StatusCode::CONFLICT,
                "{uri}: {}",
                res.status()
            );
        }
        // The runner's token opens nothing of the node's, nor a worker's door.
        let rpc = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }).to_string();
        for (method, uri, body) in [
            ("GET", "/v1/snapshot", ""),
            ("POST", "/v1/stop", r#"{"why":"a runner"}"#),
            ("POST", "/v1/team/turn", r#"{"token":"x"}"#),
            ("POST", "/v1/team/mcp", rpc.as_str()),
        ] {
            let res = app
                .clone()
                .oneshot(call(method, uri, Some(RUNNER), body))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{uri}");
        }
        // With no runner's token set, no runner is answered, whatever it offers.
        let shut = super::app(State {
            runner_token: None,
            ..st
        });
        for token in [Some(""), Some(RUNNER)] {
            let res = shut
                .clone()
                .oneshot(call("GET", "/v1/runner/stops", token, ""))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{token:?}");
        }
    }

    #[tokio::test]
    async fn the_runner_is_handed_a_started_turn_once_and_its_end_ends_the_run() {
        let st = state(false);
        let (run, task) = a_turn_starts(&st);
        let app = app(st.clone());
        let next = |free: u32| {
            call(
                "POST",
                "/v1/runner/next",
                Some(RUNNER),
                &json!({ "free": free }).to_string(),
            )
        };
        let full = app.clone().oneshot(next(0)).await.unwrap();
        assert_eq!(full.status(), StatusCode::NO_CONTENT, "no room, no turn");
        let res = app.clone().oneshot(next(2)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let turn = json_of(res).await;
        assert_eq!(turn["run"], run.0);
        assert_eq!(turn["member"], "John Doe");
        assert_eq!(turn["role"], "developer");
        assert_eq!(turn["task"], task.0);
        assert_eq!(
            turn["brain"], "local",
            "the house's model unless the owner says"
        );
        assert_eq!(turn["effort"], json!(null), "the house's model takes none");
        let token = turn["token"].as_str().unwrap();
        assert_eq!(
            st.hq.turn_of(token).unwrap().unwrap().0.id,
            run,
            "the turn's own token"
        );
        let again = app.clone().oneshot(next(1)).await.unwrap();
        assert_eq!(again.status(), StatusCode::NO_CONTENT, "handed out once");
        let lease = turn["lease"].as_u64().unwrap();
        let end = |lease: u64| {
            call(
                "POST",
                &format!("/v1/runner/{}/ended", run.0),
                Some(RUNNER),
                &json!({ "succeeded": true, "lease": lease }).to_string(),
            )
        };
        let stale = app.clone().oneshot(end(lease + 1)).await.unwrap();
        assert_eq!(
            stale.status(),
            StatusCode::CONFLICT,
            "another attempt's lease"
        );
        assert_eq!(status_of(&st, run), RunStatus::Running);
        let ended = app.oneshot(end(lease)).await.unwrap();
        assert_eq!(ended.status(), StatusCode::NO_CONTENT);
        assert_eq!(status_of(&st, run), RunStatus::Running, "until the tick");
        assert_eq!(st.hq.tick().unwrap().ended, 1);
        assert_eq!(status_of(&st, run), RunStatus::Succeeded);
        assert!(
            st.hq.turn_of(token).unwrap().is_none(),
            "its token is spent"
        );
    }

    #[tokio::test]
    async fn the_runners_reason_for_a_failed_turn_is_kept_on_the_run_and_an_old_report_still_taken()
    {
        let st = state(false);
        let (run, _) = a_turn_starts(&st);
        let app = app(st.clone());
        let next = || {
            call(
                "POST",
                "/v1/runner/next",
                Some(RUNNER),
                &json!({ "free": 1 }).to_string(),
            )
        };
        let handed = json_of(app.clone().oneshot(next()).await.unwrap()).await;
        let lease = handed["lease"].as_u64().unwrap();
        let end = |body: serde_json::Value| {
            call(
                "POST",
                &format!("/v1/runner/{}/ended", run.0),
                Some(RUNNER),
                &body.to_string(),
            )
        };
        // The worker exited 3: HQ or the relay had ended the turn under it.
        let over = app
            .clone()
            .oneshot(end(json!({
                "succeeded": false, "lease": lease, "code": 3,
                "why": "exit 3: worker: the turn is over: HQ answered 401"
            })))
            .await
            .unwrap();
        assert_eq!(over.status(), StatusCode::NO_CONTENT);
        assert_eq!(st.hq.tick().unwrap().retrying, 1);
        let kept = st.hq.store().run(run).unwrap().unwrap();
        assert_eq!(kept.status, RunStatus::RetryWaiting);
        assert_eq!(
            kept.end_reason,
            "the turn was over: exit 3: worker: the turn is over: HQ answered 401"
        );
        let said = st
            .hq
            .store()
            .events_after(0)
            .unwrap()
            .into_iter()
            .rfind(|e| e.kind == "run.ended")
            .unwrap();
        assert_eq!(
            said.data["why"], kept.end_reason,
            "the activity says it too"
        );

        // The retry waits a minute, which the system clock cannot skip: it is made due by hand.
        let due_now = || {
            let mut waiting = st.hq.store().run(run).unwrap().unwrap();
            waiting.retry_at = Some(0);
            st.hq.store().save_run(&waiting).unwrap();
            st.hq.tick().unwrap();
        };
        // The second attempt's worker failed, and the runner that took it is an older one,
        // saying only that it did not succeed.
        due_now();
        let again = json_of(app.clone().oneshot(next()).await.unwrap()).await;
        let lease = again["lease"].as_u64().unwrap();
        let old = app
            .clone()
            .oneshot(end(json!({ "succeeded": false, "lease": lease })))
            .await
            .unwrap();
        assert_eq!(
            old.status(),
            StatusCode::NO_CONTENT,
            "a report without code or why"
        );
        st.hq.tick().unwrap();
        let kept = st.hq.store().run(run).unwrap().unwrap();
        assert_eq!(kept.end_reason, "the turn failed", "as before");

        // The third attempt's worker failed and said why.
        due_now();
        let third = json_of(app.clone().oneshot(next()).await.unwrap()).await;
        let lease = third["lease"].as_u64().unwrap();
        let failed = app
            .oneshot(end(json!({
                "succeeded": false, "lease": lease, "code": 1,
                "why": "exit 1: worker: the model answered 500"
            })))
            .await
            .unwrap();
        assert_eq!(failed.status(), StatusCode::NO_CONTENT);
        st.hq.tick().unwrap();
        let kept = st.hq.store().run(run).unwrap().unwrap();
        assert_eq!(
            kept.end_reason,
            "the turn failed: exit 1: worker: the model answered 500"
        );
    }

    #[tokio::test]
    async fn the_owner_gives_a_member_a_cloud_brain_and_the_runner_and_the_node_hear_it() {
        let st = state(false);
        let lead = st.hq.hire("Jane Doe", Role::Lead, 25, 2, 0).unwrap();
        let app = app(st.clone());
        let set = |token: Option<&str>, member: i64, brain: &str| {
            call(
                "POST",
                &format!("/v1/members/{member}/brain"),
                token,
                &json!({ "brain": brain }).to_string(),
            )
        };
        let stranger = app
            .clone()
            .oneshot(set(Some(RUNNER), lead.0, "claude-sonnet-5-5"))
            .await
            .unwrap();
        assert_eq!(
            stranger.status(),
            StatusCode::UNAUTHORIZED,
            "the node's alone"
        );
        let opus = app
            .clone()
            .oneshot(set(Some(TOKEN), lead.0, "claude-opus-5-5"))
            .await
            .unwrap();
        assert_eq!(opus.status(), StatusCode::BAD_REQUEST);
        assert!(
            json_of(opus).await["error"]
                .as_str()
                .unwrap()
                .contains("Opus")
        );
        let nobody = app
            .clone()
            .oneshot(set(Some(TOKEN), 99, "local"))
            .await
            .unwrap();
        assert_eq!(nobody.status(), StatusCode::NOT_FOUND);
        let res = app
            .clone()
            .oneshot(set(Some(TOKEN), lead.0, "claude-sonnet-5-5"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_of(res).await["brain"], "claude-sonnet-5-5");

        let goal = st
            .hq
            .set_goal("Keep the lab tidy", "a test", Author::Owner)
            .unwrap();
        st.hq
            .add_task(
                Author::Owner,
                NewTask {
                    goal,
                    project: None,
                    parent: None,
                    title: "Look round".to_owned(),
                    description: String::new(),
                    assignee: Some(lead),
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
            )
            .unwrap();
        st.hq.tick().unwrap();
        let handed = json_of(
            app.clone()
                .oneshot(call(
                    "POST",
                    "/v1/runner/next",
                    Some(RUNNER),
                    &json!({ "free": 1 }).to_string(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(handed["brain"], "claude-sonnet-5-5");
        assert_eq!(handed["effort"], "medium", "the lead's");
        let token = handed["token"].as_str().unwrap();
        let found = json_of(
            app.oneshot(call(
                "POST",
                "/v1/team/turn",
                Some(TOKEN),
                &json!({ "token": token }).to_string(),
            ))
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(found["cloud"], true, "the node's check point is told");
        assert_eq!(found["brain"], "claude-sonnet-5-5");
        assert_eq!(found["role"], "lead");
    }

    #[tokio::test]
    async fn the_owner_puts_a_developer_on_claude_code_and_the_runner_and_the_relay_hear_it() {
        let st = state(false);
        let dev = st.hq.hire("Jane Doe", Role::Developer, 40, 2, 0).unwrap();
        let lead = st.hq.hire("John Doe", Role::Lead, 25, 2, 1).unwrap();
        st.hq.set_brain(lead, "claude-sonnet-5-5").unwrap();
        let app = app(st.clone());
        let set = |token: Option<&str>, member: i64, runtime: &str| {
            call(
                "POST",
                &format!("/v1/members/{member}/runtime"),
                token,
                &json!({ "runtime": runtime }).to_string(),
            )
        };
        let stranger = app
            .clone()
            .oneshot(set(Some(RUNNER), dev.0, "claude-code"))
            .await
            .unwrap();
        assert_eq!(
            stranger.status(),
            StatusCode::UNAUTHORIZED,
            "the node's alone"
        );
        for (member, why) in [(dev, "a developer at home"), (lead, "a lead")] {
            let refused = app
                .clone()
                .oneshot(set(Some(TOKEN), member.0, "claude-code"))
                .await
                .unwrap();
            assert_eq!(refused.status(), StatusCode::BAD_REQUEST, "{why}");
        }
        st.hq.set_brain(dev, "claude-haiku-5-5").unwrap();
        let res = app
            .clone()
            .oneshot(set(Some(TOKEN), dev.0, "claude-code"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_of(res).await["runtime"], "claude-code");
        let shown = st.hq.snapshot().unwrap();
        let card = shown["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == dev.0)
            .unwrap()
            .clone();
        assert_eq!(
            (card["runtime"].clone(), card["claude_code"].clone()),
            (json!("claude-code"), json!(true))
        );

        let goal = st
            .hq
            .set_goal("Tidy the fork", "a test", Author::Owner)
            .unwrap();
        st.hq
            .add_task(
                Author::Owner,
                NewTask {
                    goal,
                    project: None,
                    parent: None,
                    title: "Tidy a role".to_owned(),
                    description: String::new(),
                    assignee: Some(dev),
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
            )
            .unwrap();
        st.hq.tick().unwrap();
        let handed = json_of(
            app.clone()
                .oneshot(call(
                    "POST",
                    "/v1/runner/next",
                    Some(RUNNER),
                    &json!({ "free": 1 }).to_string(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(handed["runtime"], "claude-code");
        assert_eq!(handed["brain"], "claude-haiku-5-5");
        let found = json_of(
            app.oneshot(call(
                "POST",
                "/v1/team/turn",
                Some(TOKEN),
                &json!({ "token": handed["token"] }).to_string(),
            ))
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(found["runtime"], "claude-code", "the relay is told");
    }

    #[tokio::test]
    async fn the_runner_hears_once_of_a_turn_the_kill_switch_stopped() {
        let st = state(false);
        let (run, _) = a_turn_starts(&st);
        let app = app(st.clone());
        let handed = app
            .clone()
            .oneshot(call(
                "POST",
                "/v1/runner/next",
                Some(RUNNER),
                r#"{"free":1}"#,
            ))
            .await
            .unwrap();
        assert_eq!(handed.status(), StatusCode::OK);
        let stops = || call("GET", "/v1/runner/stops", Some(RUNNER), "");
        let none = json_of(app.clone().oneshot(stops()).await.unwrap()).await;
        assert_eq!(none["runs"], json!([]));
        let stop = app
            .clone()
            .oneshot(call(
                "POST",
                "/v1/stop",
                Some(TOKEN),
                r#"{"why":"testing"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(stop.status(), StatusCode::OK);
        assert_eq!(status_of(&st, run), RunStatus::Stopped);
        let heard = json_of(app.clone().oneshot(stops()).await.unwrap()).await;
        assert_eq!(heard["runs"], json!([run.0]));
        let after = json_of(app.oneshot(stops()).await.unwrap()).await;
        assert_eq!(after["runs"], json!([]), "told once");
    }

    #[tokio::test]
    async fn the_owner_makes_switches_and_runs_a_routine_through_the_node() {
        let st = state(false);
        let lead = st.hq.hire("John Doe", Role::Lead, 25, 2, 0).unwrap();
        let goal = st
            .hq
            .set_goal("Keep the home lab healthy", "", Author::Owner)
            .unwrap();
        let app = app(st.clone());
        let send = |method: &str, uri: &str, token: Option<&str>, body: serde_json::Value| {
            call(method, uri, token, &body.to_string())
        };
        let made = json!({ "title": "Check the home lab", "description": "Look.", "goal": goal.0,
                           "assignee": lead.0, "every_ms": 6 * 3_600_000 });
        let res = app
            .clone()
            .oneshot(send("POST", "/v1/routines", None, made.clone()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "only the node");
        let res = app
            .clone()
            .oneshot(send("POST", "/v1/routines", Some(TOKEN), made))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let id = json_of(res).await["routine"].as_i64().unwrap();
        let too_fast =
            json!({ "title": "Spin", "goal": goal.0, "assignee": lead.0, "every_ms": 1000 });
        let res = app
            .clone()
            .oneshot(send("POST", "/v1/routines", Some(TOKEN), too_fast))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        let off = app
            .clone()
            .oneshot(send(
                "POST",
                &format!("/v1/routines/{id}"),
                Some(TOKEN),
                json!({ "enabled": false }),
            ))
            .await
            .unwrap();
        assert_eq!(off.status(), StatusCode::OK);
        let fired = json_of(
            app.clone()
                .oneshot(send(
                    "POST",
                    &format!("/v1/routines/{id}/fire"),
                    Some(TOKEN),
                    json!({}),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(fired["fired"], "created", "{fired}");
        assert!(fired["task"].is_i64());
        let again = json_of(
            app.clone()
                .oneshot(send(
                    "POST",
                    &format!("/v1/routines/{id}/fire"),
                    Some(TOKEN),
                    json!({}),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(again["fired"], "skipped");
        assert!(again["why"].as_str().unwrap().contains("still open"));
        let snap = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(snap["routines"][0]["enabled"], false);
        assert_eq!(snap["routines"][0]["rhythm"], "every 6 hours");
        let missing = app
            .oneshot(send("POST", "/v1/routines/99/fire", Some(TOKEN), json!({})))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_alarm_from_the_node_reaches_the_routine_that_takes_it_and_never_the_demo() {
        let alarm = |token: Option<&str>| {
            call(
                "POST",
                "/v1/alarms",
                token,
                &json!({ "text": "sonarr became unhealthy at 14:02" }).to_string(),
            )
        };
        let demo = app(state(true));
        let res = demo.oneshot(alarm(Some(TOKEN))).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::CONFLICT,
            "the demo takes no alarms"
        );

        let st = state(false);
        let app = app(st.clone());
        let res = app.clone().oneshot(alarm(None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let none = json_of(app.clone().oneshot(alarm(Some(TOKEN))).await.unwrap()).await;
        assert_eq!(none["fired"], "nobody", "{none}");
        st.hq.hire("John Doe", Role::Lead, 25, 2, 0).unwrap();
        st.hq.set_goal(crate::LAB_GOAL, "", Author::Owner).unwrap();
        crate::seed_routine(&st.hq).unwrap();
        let took = json_of(app.oneshot(alarm(Some(TOKEN))).await.unwrap()).await;
        assert_eq!(took["fired"], "created", "{took}");
        let task = st
            .hq
            .store()
            .task(TaskId(took["task"].as_i64().unwrap()))
            .unwrap()
            .unwrap();
        assert!(
            task.description
                .contains("sonarr became unhealthy at 14:02")
        );
    }

    #[tokio::test]
    async fn a_recovery_from_the_node_is_said_in_the_alarms_open_task() {
        let back = |token: Option<&str>| {
            call(
                "POST",
                "/v1/alarms/recovered",
                token,
                &json!({ "text": "deluge is back up at 13:43" }).to_string(),
            )
        };
        let demo = app(state(true));
        let res = demo.oneshot(back(Some(TOKEN))).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::CONFLICT,
            "nothing in the demo is real"
        );

        let st = state(false);
        let app = app(st.clone());
        let res = app.clone().oneshot(back(None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        st.hq.hire("John Doe", Role::Lead, 25, 2, 0).unwrap();
        st.hq.set_goal(crate::LAB_GOAL, "", Author::Owner).unwrap();
        crate::seed_routine(&st.hq).unwrap();
        let none = json_of(app.clone().oneshot(back(Some(TOKEN))).await.unwrap()).await;
        assert_eq!(
            none["posted"],
            json!(null),
            "no alarm's task is open: {none}"
        );
        let fired = st
            .hq
            .alarm("deluge became exited at 13:42")
            .unwrap()
            .unwrap()
            .1;
        let endora_team::application::Fired::Created(task) = fired else {
            panic!("a task: {fired:?}");
        };
        let took = json_of(app.oneshot(back(Some(TOKEN))).await.unwrap()).await;
        assert_eq!(took["posted"], task.0, "{took}");
        let said = st.hq.store().thread(task).unwrap().pop().unwrap();
        assert_eq!(said.text, "Recovered: deluge is back up at 13:43");
    }

    #[tokio::test]
    async fn the_node_tells_the_labs_names_and_a_question_is_checked_against_them() {
        let st = state(false);
        let app = app(st.clone());
        let names = |token: Option<&str>| {
            call(
                "POST",
                "/v1/lab/names",
                token,
                &json!({ "names": ["sonarr", "radarr"] }).to_string(),
            )
        };
        let res = app.clone().oneshot(names(None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "only the node");
        let res = app.oneshot(names(Some(TOKEN))).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let (run, _) = a_turn_starts(&st);
        let endora_team::application::Asked::Desk(asked) =
            st.hq.ask_owner(run, "Restart `Crappy_Cray`?").unwrap()
        else {
            panic!("no reviewer: straight to the desk");
        };
        let item = st
            .hq
            .store()
            .desk()
            .unwrap()
            .into_iter()
            .find(|d| d.id == asked)
            .unwrap();
        assert!(
            item.text
                .ends_with("\nNot found in Endora's home lab: Crappy_Cray"),
            "{}",
            item.text
        );
    }

    #[test]
    fn with_the_demo_off_the_home_labs_routine_is_made_once_when_its_goal_is_there() {
        let st = state(false);
        assert_eq!(crate::seed_routine(&st.hq).unwrap(), None, "no goal yet");
        let lead = st.hq.hire("John Doe", Role::Lead, 25, 2, 0).unwrap();
        st.hq
            .set_goal("Ship the tracker", "", Author::Owner)
            .unwrap();
        assert_eq!(crate::seed_routine(&st.hq).unwrap(), None, "not that goal");
        let goal = st
            .hq
            .set_goal("keep the home lab healthy ", "", Author::Owner)
            .unwrap();
        let made = crate::seed_routine(&st.hq).unwrap().unwrap();
        let routines = st.hq.store().routines().unwrap();
        assert_eq!(routines.len(), 1);
        let r = &routines[0];
        assert_eq!(r.id, made);
        assert_eq!(r.title, "Check the home lab");
        assert_eq!((r.goal, r.assignee), (goal, lead));
        assert_eq!(r.every_ms, 6 * 3_600_000);
        assert!(r.on_lab_trouble && r.enabled);
        assert!(
            r.description
                .starts_with("Look at the home lab with homelab_status")
        );
        let changed = st
            .hq
            .events_after(0)
            .unwrap()
            .into_iter()
            .rfind(|e| e.kind == "routine.changed")
            .unwrap();
        assert_eq!(changed.data["by"], "endora", "recorded, as Endora's");
        st.hq.switch_routine(made, false).unwrap();
        assert_eq!(crate::seed_routine(&st.hq).unwrap(), None, "once");
        assert_eq!(st.hq.store().routines().unwrap().len(), 1);
    }

    #[test]
    fn with_the_demo_off_the_ceo_is_seated_once_and_its_daily_review_made_once() {
        let st = state(false);
        let lead = st.hq.hire("John Doe", Role::Lead, 25, 2, 0).unwrap();
        let (seated, routine, planning) = crate::seed_ceo(&st.hq).unwrap();
        let ceo = seated.expect("seated");
        assert_eq!((routine, planning), (None, None), "no goal yet");
        let goal = st
            .hq
            .set_goal("Keep the home lab healthy", "", Author::Owner)
            .unwrap();
        // The home lab's routine is still made, though the CEO's comes first here.
        let (again, made, planning) = crate::seed_ceo(&st.hq).unwrap();
        assert_eq!(again, None, "seated once");
        let made = made.expect("its daily review");
        let planning = planning.expect("its weekly planning");
        assert!(crate::seed_routine(&st.hq).unwrap().is_some());
        let routines = st.hq.store().routines().unwrap();
        let review = routines.iter().find(|r| r.id == made).unwrap();
        assert_eq!(review.title, "Daily review");
        assert_eq!((review.goal, review.assignee), (goal, ceo));
        assert_eq!(review.every_ms, 24 * 3_600_000);
        assert!(!review.on_lab_trouble);
        assert!(routines.iter().any(|r| r.assignee == lead));
        let plan = routines.iter().find(|r| r.id == planning).unwrap();
        assert_eq!(
            (plan.title.as_str(), plan.assignee, plan.every_ms),
            ("Planning", ceo, 7 * 24 * 3_600_000)
        );
        assert_eq!(crate::seed_ceo(&st.hq).unwrap(), (None, None, None), "once");
        let members = st.hq.store().members().unwrap();
        let mara = members.iter().find(|m| m.id == ceo).unwrap();
        assert_eq!(
            (mara.name.as_str(), mara.role, mara.brain.name()),
            ("Mara", Role::Ceo, "claude-opus-5-5")
        );
    }

    #[tokio::test]
    async fn the_owner_renames_a_member_and_edits_the_house_log_through_the_node() {
        let st = state(false);
        let lead = st.hq.hire("John Doe", Role::Lead, 25, 2, 0).unwrap();
        let app = app(st.clone());
        let res = app
            .clone()
            .oneshot(call(
                "POST",
                &format!("/v1/members/{}/name", lead.0),
                Some(TOKEN),
                r#"{"name":"Jane Doe"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_of(res).await["name"], "Jane Doe");
        let res = app
            .clone()
            .oneshot(call(
                "POST",
                "/v1/house",
                Some(TOKEN),
                r#"{"log":"- 17 Oct: the NAS has 16 GB."}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let snap = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(snap["house_log"], "- 17 Oct: the NAS has 16 GB.");
        assert_eq!(snap["members"][0]["name"], "Jane Doe");
        let unsigned = app
            .oneshot(call("POST", "/v1/house", None, r#"{"log":""}"#))
            .await
            .unwrap();
        assert_eq!(unsigned.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_owner_writes_a_goals_strategy_and_the_snapshot_shows_it() {
        let st = state(false);
        let goal = st
            .hq
            .set_goal("Keep the home lab healthy", "", Author::Owner)
            .unwrap();
        let app = app(st);
        let res = app
            .clone()
            .oneshot(call(
                "POST",
                &format!("/v1/goals/{}/strategy", goal.0),
                Some(TOKEN),
                r#"{"purpose":"The house runs itself.","not_now":"<b>No new containers</b>"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let snap = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        let strategy = &snap["goals"][0]["strategy"];
        assert_eq!(strategy["purpose"], "The house runs itself.");
        assert_eq!(
            strategy["not_now"], "<b>No new containers</b>",
            "kept as text"
        );
        assert_eq!(strategy["by"], "owner");
        let empty = app
            .clone()
            .oneshot(call(
                "POST",
                &format!("/v1/goals/{}/strategy", goal.0),
                Some(TOKEN),
                "{}",
            ))
            .await
            .unwrap();
        assert_ne!(empty.status(), StatusCode::OK, "an edit gives a part");
        let unsigned = app
            .oneshot(call(
                "POST",
                &format!("/v1/goals/{}/strategy", goal.0),
                None,
                r#"{"purpose":"x"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(unsigned.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_owner_makes_caps_and_pauses_a_project_and_sets_whom_a_member_reports_to() {
        let st = state(false);
        let lead = st.hq.hire("John Doe", Role::Lead, 25, 2, 0).unwrap();
        let other = st.hq.hire("Jane Doe", Role::Lead, 25, 2, 0).unwrap();
        let dev = st
            .hq
            .hire("Richard Roe", Role::Developer, 40, 2, 0)
            .unwrap();
        let goal = st
            .hq
            .set_goal("Keep the home lab healthy", "", Author::Owner)
            .unwrap();
        let app = app(st.clone());
        let post = |uri: String, body: String| {
            let app = app.clone();
            async move {
                let res = app
                    .oneshot(call("POST", &uri, Some(TOKEN), &body))
                    .await
                    .unwrap();
                (res.status(), json_of(res).await)
            }
        };
        let (status, made) = post(
            "/v1/projects".to_owned(),
            json!({ "goal": goal.0, "title": "Upkeep", "lead": lead.0, "cap": 10 }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{made}");
        let id = made["project"].as_i64().unwrap();
        let (status, refused) = post(
            "/v1/projects".to_owned(),
            json!({ "goal": goal.0, "title": "Media", "lead": lead.0, "cap": "61" }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            refused["error"]
                .as_str()
                .unwrap()
                .contains("at most $60.00"),
            "{refused}"
        );
        let (status, _) = post(
            format!("/v1/projects/{id}/cap"),
            json!({ "cap": "12.50" }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, paused) = post(
            format!("/v1/projects/{id}"),
            json!({ "action": "pause" }).to_string(),
        )
        .await;
        assert_eq!(
            (status, paused["status"].clone()),
            (StatusCode::OK, json!("paused"))
        );
        let (status, _) = post(
            "/v1/tasks".to_owned(),
            json!({ "goal": goal.0, "title": "Fix sonarr", "project": id }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = post(
            format!("/v1/members/{}/reports_to", dev.0),
            json!({ "to": other.0 }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = post(
            format!("/v1/members/{}/reports_to", dev.0),
            json!({ "to": dev.0 }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "never itself");
        let snap = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        let project = &snap["projects"][0];
        assert_eq!(
            (
                project["title"].clone(),
                project["status"].clone(),
                project["pause"].clone(),
                project["cap"].clone()
            ),
            (
                json!("Upkeep"),
                json!("paused"),
                json!("owner"),
                json!(12_500_000)
            )
        );
        assert_eq!(snap["tasks"][0]["project"], id);
        assert_eq!(snap["members"][2]["reports_to"], other.0);
    }

    #[tokio::test]
    async fn the_owner_is_the_offices_landlord_and_the_snapshot_carries_the_office() {
        use endora_team::domain::office::{Change, Kind};
        let st = state(false);
        st.hq.seed_office().unwrap();
        let seeded = st.hq.store().office_items().unwrap().len();
        let app = app(st.clone());
        let send = |uri: String, body: String, signed: Option<&'static str>| {
            let app = app.clone();
            async move {
                let res = app
                    .oneshot(call("POST", &uri, signed, &body))
                    .await
                    .unwrap();
                (res.status(), json_of(res).await)
            }
        };
        // The node signs; nobody else may.
        let (status, _) = send("/v1/office/reset".to_owned(), "{}".to_owned(), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        // Nothing to undo yet.
        let (status, said) = send("/v1/office/undo".to_owned(), "{}".to_owned(), Some(TOKEN)).await;
        assert_eq!(
            (status, said["undone"].clone()),
            (StatusCode::OK, json!(null))
        );
        // A proposal open, and the owner vetoes it.
        let id = st
            .hq
            .store()
            .add_office_proposal(
                "Jane Doe",
                &[Change::Place {
                    kind: Kind::FishTank,
                    x: 10,
                    y: 3,
                }],
                "life",
                1,
            )
            .unwrap();
        let (status, said) = send(
            format!("/v1/office/proposals/{id}/veto"),
            "{}".to_owned(),
            Some(TOKEN),
        )
        .await;
        assert_eq!(
            (status, said["vetoed"].clone()),
            (StatusCode::OK, json!(true))
        );
        let (status, _) = send(
            format!("/v1/office/proposals/{id}/veto"),
            "{}".to_owned(),
            Some(TOKEN),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "a closed proposal");
        let (status, _) = send(
            "/v1/office/proposals/99/veto".to_owned(),
            "{}".to_owned(),
            Some(TOKEN),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // Reset puts the seeded layout back, and the allowance is set within the pool's day.
        st.hq.store().remove_office_item(1).unwrap();
        let (status, _) = send("/v1/office/reset".to_owned(), "{}".to_owned(), Some(TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(st.hq.store().office_items().unwrap().len(), seeded);
        let (status, said) = send(
            "/v1/office/downtime".to_owned(),
            json!({ "cents": 25 }).to_string(),
            Some(TOKEN),
        )
        .await;
        assert_eq!(
            (status, said["downtime_paid_cents_per_day"].clone()),
            (StatusCode::OK, json!(25))
        );
        let (status, _) = send(
            "/v1/office/downtime".to_owned(),
            json!({ "cents": -1 }).to_string(),
            Some(TOKEN),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let snap = st.hq.snapshot().unwrap();
        assert_eq!(snap["office"]["items"].as_array().unwrap().len(), seeded);
        assert_eq!(snap["office"]["downtime_paid_cents_per_day"], 25);
        assert_eq!(snap["office"]["can_undo"], true, "a reset can be undone");
        let (status, said) = send("/v1/office/undo".to_owned(), "{}".to_owned(), Some(TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(said["undone"].as_str().unwrap().contains("reset"), "{said}");
    }

    #[tokio::test]
    async fn the_owner_furloughs_and_recalls_a_member_but_never_the_ceo_and_nobody_is_deleted() {
        let st = state(false);
        let ceo = st.hq.seat_ceo("Jane Doe").unwrap().unwrap();
        let dev = st
            .hq
            .hire("Richard Roe", Role::Developer, 40, 2, 0)
            .unwrap();
        let app = app(st.clone());
        let post = |uri: String, signed: Option<&'static str>| {
            let app = app.clone();
            async move {
                let res = app.oneshot(call("POST", &uri, signed, "{}")).await.unwrap();
                (res.status(), json_of(res).await)
            }
        };
        let unsigned = app
            .clone()
            .oneshot(call(
                "POST",
                &format!("/v1/members/{}/furlough", dev.0),
                None,
                "{}",
            ))
            .await
            .unwrap();
        assert_eq!(
            unsigned.status(),
            StatusCode::UNAUTHORIZED,
            "the node signs"
        );
        let (status, said) = post(format!("/v1/members/{}/furlough", dev.0), Some(TOKEN)).await;
        assert_eq!(
            (status, said["furloughed"].clone()),
            (StatusCode::OK, json!(true))
        );
        let (status, _) = post(format!("/v1/members/{}/furlough", dev.0), Some(TOKEN)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "already on furlough");
        let (status, refused) = post(format!("/v1/members/{}/furlough", ceo.0), Some(TOKEN)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            refused["error"].as_str().unwrap().contains("CEO"),
            "{refused}"
        );
        let snap = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(snap["members"][1]["furloughed"], true);
        assert_eq!(snap["members"][1]["name"], "Richard Roe");
        let (status, said) = post(format!("/v1/members/{}/recall", dev.0), Some(TOKEN)).await;
        assert_eq!(
            (status, said["furloughed"].clone()),
            (StatusCode::OK, json!(false))
        );
        let (status, _) = post(format!("/v1/members/{}/recall", dev.0), Some(TOKEN)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "not on furlough");
        assert_eq!(st.hq.store().members().unwrap().len(), 2);
        // No route removes a member: the router names none that does.
        let source = include_str!("api.rs");
        let removes = ["delete(", "remove_member", "fire_member"].map(str::to_owned);
        assert!(
            !source
                .lines()
                .filter(|l| l.contains(".route(") && l.contains("members"))
                .any(|l| removes.iter().any(|r| l.contains(r.as_str()))),
            "a route that removes a member"
        );
    }

    #[tokio::test]
    async fn the_owner_reads_adds_edits_and_deletes_a_members_notes_through_the_node() {
        let st = state(false);
        let dev = st
            .hq
            .hire("Richard Roe", Role::Developer, 40, 2, 0)
            .unwrap();
        let other = st.hq.hire("Jane Doe", Role::Developer, 40, 2, 0).unwrap();
        let app = app(st.clone());
        let send = |method: &'static str, uri: String, body: String| {
            let app = app.clone();
            async move {
                let res = app
                    .oneshot(call(method, &uri, Some(TOKEN), &body))
                    .await
                    .unwrap();
                (res.status(), json_of(res).await)
            }
        };
        let book = format!("/v1/members/{}/notebook", dev.0);
        let (status, empty) = send("GET", book.clone(), String::new()).await;
        assert_eq!(
            (status, empty["notes"].clone()),
            (StatusCode::OK, json!([]))
        );
        let (status, made) = send(
            "POST",
            book.clone(),
            json!({ "text": "Never at night." }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{made}");
        let note = made["note"].as_i64().unwrap();
        let (status, _) = send("POST", book.clone(), json!({ "text": "  " }).to_string()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = send(
            "POST",
            format!("{book}/{note}"),
            json!({ "text": "Never at night, ever." }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, shown) = send("GET", book.clone(), String::new()).await;
        let first = &shown["notes"][0];
        assert_eq!(
            (
                first["text"].clone(),
                first["from_owner"].clone(),
                first["stale"].clone(),
                first["evidence"].clone()
            ),
            (
                json!("Never at night, ever."),
                json!(true),
                json!(false),
                json!([])
            )
        );
        // A note is reached through its own member's notebook only.
        let (status, _) = send(
            "POST",
            format!("/v1/members/{}/notebook/{note}/delete", other.0),
            "{}".to_owned(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = send("POST", format!("{book}/{note}/delete"), "{}".to_owned()).await;
        assert_eq!(status, StatusCode::OK);
        let (_, gone) = send("GET", book, String::new()).await;
        assert_eq!(gone["notes"], json!([]));
        let (status, _) = send("GET", "/v1/members/999/notebook".to_owned(), String::new()).await;
        assert_ne!(status, StatusCode::OK);
        assert_eq!(st.hq.store().members().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn the_owner_hires_through_the_node_and_one_ceo_is_enough() {
        let st = state(false);
        let app = app(st.clone());
        let hire = |signed: Option<&str>, body: serde_json::Value| {
            call("POST", "/v1/members", signed, &body.to_string())
        };
        let res = app
            .clone()
            .oneshot(hire(None, json!({ "name": "Kit", "role": "lead" })))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "the node signs");
        let res = app
            .clone()
            .oneshot(hire(
                Some(TOKEN),
                json!({ "name": "Kit", "role": "lead", "brain": "claude-sonnet-5-5" }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let kit = json_of(res).await["member"].as_i64().unwrap();
        let res = app
            .clone()
            .oneshot(hire(
                Some(TOKEN),
                json!({ "name": "Rae", "role": "developer", "brain": "claude-haiku-5-5", "reports_to": kit }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let members = st.hq.store().members().unwrap();
        let rae = members.iter().find(|m| m.name == "Rae").unwrap();
        assert_eq!(
            (rae.role, rae.brain.name(), rae.reports_to),
            (Role::Developer, "claude-haiku-5-5", Some(MemberId(kit)))
        );
        st.hq.seat_ceo("Mara").unwrap();
        let res = app
            .clone()
            .oneshot(hire(
                Some(TOKEN),
                json!({ "name": "Second", "role": "ceo", "brain": "claude-opus-5-5" }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert!(
            json_of(res).await["error"]
                .as_str()
                .unwrap()
                .contains("one CEO"),
        );
        let res = app
            .oneshot(hire(Some(TOKEN), json!({ "name": "X", "role": "wizard" })))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_branch_the_node_took_is_checked_on_the_runner_and_reported_with_the_jobs_token() {
        let st = state(false);
        let (run, task) = a_turn_starts(&st);
        let app = app(st.clone());
        let handed = json_of(
            app.clone()
                .oneshot(call(
                    "POST",
                    "/v1/runner/next",
                    Some(RUNNER),
                    r#"{"free":2}"#,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(handed["run"], run.0, "a turn, not a check: none waits");
        let token = handed["token"].as_str().unwrap().to_owned();
        // The relay learns the turn's task too (ADR 0115).
        let whose = json_of(
            app.clone()
                .oneshot(call(
                    "POST",
                    "/v1/team/turn",
                    Some(TOKEN),
                    &json!({ "token": token }).to_string(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(whose["task"], task.0);
        let branch = |signed: Option<&str>, token: &str| {
            call(
                "POST",
                "/v1/team/branch",
                signed,
                &json!({ "token": token, "branch": "hq/task-1-sort-the-cables",
                         "head": "a1b2c3d4e5f60718", "base": "0123456789abcdef", "files": 1,
                         "plus": 2, "minus": 0, "stat": " a | 2 ++", "diff": "+x\n+y",
                         "can_push": true })
                .to_string(),
            )
        };
        let res = app
            .clone()
            .oneshot(branch(Some(&token), &token))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "the node tells it, never a worker"
        );
        let res = app
            .clone()
            .oneshot(branch(Some(TOKEN), "forged"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let recorded = json_of(
            app.clone()
                .oneshot(branch(Some(TOKEN), &token))
                .await
                .unwrap(),
        )
        .await;
        let check = recorded["check"].as_i64().expect("a check is queued");
        assert!(recorded["document"].as_i64().is_some());
        assert_eq!(st.hq.store().documents_of(task).unwrap().len(), 1);
        // The runner is handed the check before any turn, with the job's own token.
        let job = json_of(
            app.clone()
                .oneshot(call(
                    "POST",
                    "/v1/runner/next",
                    Some(RUNNER),
                    r#"{"free":1}"#,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            (job["kind"].as_str(), job["check"].as_i64()),
            (Some("check"), Some(check))
        );
        assert_eq!(job["branch"], "hq/task-1-sort-the-cables");
        let job_token = job["token"].as_str().unwrap().to_owned();
        assert_ne!(job_token, token);
        // The node asks whose a job token is before it hands out the bundle.
        let named = app
            .clone()
            .oneshot(call(
                "POST",
                "/v1/team/check",
                Some(TOKEN),
                &json!({ "token": job_token }).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(named.status(), StatusCode::OK);
        assert_eq!(json_of(named).await["head"], "a1b2c3d4e5f60718");
        let unknown = app
            .clone()
            .oneshot(call(
                "POST",
                "/v1/team/check",
                Some(TOKEN),
                &json!({ "token": token }).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(
            unknown.status(),
            StatusCode::NOT_FOUND,
            "a turn's token names no check"
        );
        // The report comes with the job's token: once; the runner's or a turn's token opens nothing.
        let done = |bearer: Option<&str>, body: serde_json::Value| {
            call(
                "POST",
                &format!("/v1/runner/check/{check}/done"),
                bearer,
                &body.to_string(),
            )
        };
        let report = json!({ "ok": true, "summary": "syntax ok, ansible-lint ok, yamllint ok", "report": "all good" });
        for wrong in [Some(RUNNER), Some(TOKEN), Some(token.as_str())] {
            let res = app
                .clone()
                .oneshot(done(wrong, report.clone()))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::NOT_FOUND, "{wrong:?}");
        }
        let res = app
            .clone()
            .oneshot(done(None, report.clone()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "no token at all");
        let res = app
            .clone()
            .oneshot(done(Some(&job_token), report.clone()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let res = app
            .clone()
            .oneshot(done(Some(&job_token), report))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT, "reported once");
        let receipt = st.hq.store().call_receipts_of(run).unwrap().pop().unwrap();
        assert_eq!((receipt.tool.as_str(), receipt.ok), ("check", true));
        assert_eq!(
            st.hq.store().gate(task).unwrap(),
            Some(true),
            "the check is the gate"
        );
        // Nothing more waits for the runner.
        let res = app
            .oneshot(call(
                "POST",
                "/v1/runner/next",
                Some(RUNNER),
                r#"{"free":1}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn an_approved_push_names_its_branch_for_the_node_and_its_outcome_is_told_once() {
        let st = state(false);
        let (run, task) = a_turn_starts(&st);
        let token = st.hq.store().runs().unwrap()[0].id;
        assert_eq!(token, run);
        let context = {
            let remote = st.hq.runner().remote().unwrap();
            endora_team::infrastructure::RemoteRunner::next(remote).expect("a turn waits")
        };
        st.hq
            .record_branch(
                &context.turn.token,
                &endora_team::application::Submission {
                    branch: "hq/task-1-sort-the-cables".to_owned(),
                    head: "a1b2c3d4e5f60718".to_owned(),
                    base: "0123456789abcdef".to_owned(),
                    files: 1,
                    plus: 2,
                    minus: 0,
                    stat: String::new(),
                    diff: String::new(),
                    can_push: true,
                },
            )
            .unwrap();
        st.hq
            .store()
            .set_task_status(task, endora_team::domain::TaskStatus::Done)
            .unwrap();
        st.hq.offer_push(task).unwrap();
        let item = st
            .hq
            .store()
            .desk()
            .unwrap()
            .into_iter()
            .find(|d| d.kind == "push")
            .unwrap();
        let app = app(st.clone());
        let decided = json_of(
            app.clone()
                .oneshot(call(
                    "POST",
                    &format!("/v1/desk/{}", item.id.0),
                    Some(TOKEN),
                    r#"{"decision":"approve"}"#,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(decided["push"]["branch"], "hq/task-1-sort-the-cables");
        assert_eq!(decided["push"]["head"], "a1b2c3d4e5f60718");
        assert_eq!(decided["push"]["task"], task.0);
        let outcome = |body: &str| {
            call(
                "POST",
                &format!("/v1/team/push/{}/outcome", item.id.0),
                Some(TOKEN),
                body,
            )
        };
        let res = app
            .clone()
            .oneshot(outcome(
                r#"{"ok":true,"text":"Pushed hq/task-1-sort-the-cables to the fork."}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let res = app
            .oneshot(outcome(r#"{"ok":true,"text":"again"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT, "told once");
        assert_eq!(st.hq.store().branches().unwrap()[0].status, "pushed");
        assert!(
            st.hq
                .store()
                .thread(task)
                .unwrap()
                .pop()
                .unwrap()
                .text
                .contains("Pushed hq/task-1")
        );
    }

    #[tokio::test]
    async fn with_the_demo_on_there_is_no_runner_to_serve() {
        let app = app(state(true));
        for (method, uri, body) in RUNNER_ROUTES {
            let res = app
                .clone()
                .oneshot(call(method, uri, Some(RUNNER), body))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::NOT_FOUND, "{uri}");
        }
    }
}
