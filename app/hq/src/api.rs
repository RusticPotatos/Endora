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

use crate::Hq;

mod drain;
mod world;

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
    /// How often the engine ticks, for the office's clock.
    pub tick_seconds: u64,
    /// When the last tick finished, in milliseconds since the epoch; 0 before the first
    /// (ADR 0124: `/health` says its age, so a wedged tick shows).
    pub last_tick: Arc<std::sync::atomic::AtomicI64>,
}

/// HQ's routes.
pub fn app(state: State) -> Router {
    let signed = Router::new()
        .route("/v1/snapshot", get(snapshot))
        .route("/v1/events", get(events))
        // Replay: a turn, or a task's turns, read back in full for the owner's screen.
        .route("/v1/runs/{id}/replay", get(run_replay))
        .route("/v1/tasks/{id}", get(task_shown))
        .route("/v1/tasks/{id}/replay", get(task_replay))
        .route("/v1/tasks/{id}/consultations", get(task_consultations))
        .route("/v1/goals", post(set_goal))
        .route("/v1/tasks", post(add_task))
        .route("/v1/desk/{id}", post(decide))
        // The owner's reply on a task, and a goal's notes for the team.
        .route("/v1/tasks/{id}/post", post(reply))
        .route("/v1/tasks/{id}/project", post(task_project))
        .route("/v1/goals/{id}/note", post(goal_note))
        // What a member thinks with (ADR 0109): the owner's, through the node.
        .route("/v1/members/{id}/brain", post(set_brain))
        .route("/v1/members/{id}/model", post(set_model))
        .route("/v1/members/{id}/work-brain", post(set_work_brain))
        .route("/v1/members/{id}/judgment-brain", post(set_judgment_brain))
        // What runs a member's turns (ADR 0117): the worker, or Claude Code for a developer.
        .route("/v1/members/{id}/runtime", post(set_runtime))
        // The owner renames a member, and reads and edits the CEO's house log (ADR 0111).
        .route("/v1/members/{id}/name", post(rename))
        .route("/v1/house", post(house_log))
        // The owner as landlord of the office (ADR 0119): undo the last change, reset it to the
        // layout it began with, and veto an open proposal. The team's own proposals and votes
        // come through the worker's door.
        // The owner tops the money pool up for one day (ADR 0113 rule 6).
        .route("/v1/pool/top-up", post(pool_top_up))
        // The owner hires (ADR 0115): a name, a role, a brain, whom it reports to.
        .route("/v1/members", post(hire))
        .route("/v1/projects", post(add_project))
        .route("/v1/projects/{id}", post(change_project))
        .route("/v1/projects/{id}/cap", post(project_cap))
        .route("/v1/projects/{id}/tools", post(project_tools))
        .route("/v1/projects/{id}/rank", post(loop_rank))
        .route("/v1/projects/{id}/items", post(loop_items))
        .route("/v1/one-team", post(one_team))
        // Scenarios (ADR 0135): the owner starts a chess match, and reads it.
        .route("/v1/scenarios", post(start_scenario))
        .route("/v1/league", post(start_league))
        .route("/v1/league/reset", post(reset_league))
        .route("/v1/scenarios/{task}", get(scenario_shown))
        // A match of ping-pong or pool played again, step by step.
        .route("/v1/stop", post(stop))
        .route("/v1/resume", post(resume))
        // Routines (ADR 0107): the owner's, through the node; and Endora's alarms.
        .route("/v1/routines", post(add_routine))
        .route("/v1/routines/{id}", post(switch_routine))
        .route("/v1/routines/{id}/fire", post(fire_routine))
        .route("/v1/routines/{id}/project", post(routine_project))
        .route("/v1/alarms", post(alarm))
        .route("/v1/alarms/recovered", post(recovered))
        // The home lab's containers, as the node sees them: what a question to the owner is
        // checked against before it reaches the desk.
        .route("/v1/lab/names", post(lab_names))
        .route("/v1/mac", post(mac_state))
        // The node's model relay: whose a turn's token is, and what each call cost.
        .route("/v1/team/turn", post(crate::team::turn))
        .route("/v1/team/usage", post(crate::team::usage))
        // The node's receipts (ADR 0114): each call a turn made of Endora's tools, with the
        // turn's own token, written by the node and never by the model.
        .route("/v1/team/receipt", post(crate::team::receipt))
        // What a turn said, kept for a while (ADR 0129): scrubbed by the node, told with the
        // turn's own token while it runs. Room for a megabyte of text and its escapes.
        .route(
            "/v1/team/transcript",
            post(crate::team::transcript).layer(axum::extract::DefaultBodyLimit::max(4 << 20)),
        )
        // The node's contracts: a safe fix a member made, told to the owner (ADR 0106).
        .route("/v1/team/notice", post(crate::team::notice))
        // The bridge (ADR 0108): an action waiting for the owner's yes, and what came of it.
        .route("/v1/team/approval", post(crate::team::approval))
        .route(
            "/v1/team/approval/{id}/outcome",
            post(crate::team::approval_outcome),
        )
        // The wing's git flow (ADR 0115): a branch the node took, which kept branch a turn may
        // check out, the check a job token names, and what came of a push the owner approved.
        .route("/v1/team/branch", post(crate::team::branch))
        .route(
            "/v1/team/branch/checkout",
            post(crate::team::branch_checkout),
        )
        .route("/v1/team/check", post(crate::team::check))
        .route(
            "/v1/team/push/{id}/outcome",
            post(crate::team::push_outcome),
        )
        // The team's world (ADR 0122): the owner reads and downloads it, and alone makes
        // something canon or demotes it.
        .merge(world::routes())
        // The drain before a restart (ADR 0134): HQ's own token, for a deploy and the NAS's
        // shutdown; the node does not proxy it.
        .merge(drain::routes())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            signed_by_the_node,
        ));
    Router::new()
        .route("/health", get(health))
        // A worker's door: its turn's token, not the node's (spec section 6).
        .route("/v1/team/mcp", post(crate::team::mcp))
        .route("/v2/team/mcp", post(crate::team::mcp_v2))
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

/// HQ is up, and how long ago its tick last finished (ADR 0124): a process that answers while
/// its tick is wedged is not a working HQ, and the age says so. `ok` stays the process's alone,
/// so nothing restarts it for a slow tick; `ticking` says whether the last tick is within
/// three of its periods.
async fn health(Extract(state): Extract<State>) -> Json<serde_json::Value> {
    let last = state.last_tick.load(std::sync::atomic::Ordering::SeqCst);
    let age = (last > 0).then(|| SystemClock.now().saturating_sub(last));
    let period = i64::try_from(state.tick_seconds.saturating_mul(1_000)).unwrap_or(i64::MAX);
    Json(json!({
        "ok": true,
        "last_tick_at": (last > 0).then_some(last), "tick_age_ms": age,
        "tick_seconds": state.tick_seconds,
        "ticking": age.is_some_and(|a| a <= period.saturating_mul(3)),
    }))
}

async fn snapshot(Extract(state): Extract<State>) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let mut snap = blocking(move || hq.snapshot()).await?;
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

/// Retained task questions, paged for the owner without peer execution or grants.
async fn task_consultations(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Query(q): Query<After>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let page = blocking(move || hq.legacy_task_conversations(TaskId(id), q.after)).await?;
    Ok(Json(page))
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
    /// (ADR 0114). At least one: the owner's work says what done means too (ADR 0124, amended),
    /// and is refused, with the way to give one, when it does not.
    #[serde(default)]
    criteria: Vec<String>,
}

/// The owner hands the team a piece of work, and says what done means.
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
    /// One brain, to the slot it belongs to (ADR 0131): the way a hire was always asked.
    #[serde(default)]
    brain: Option<String>,
    /// What it works with, the Mac or the house's model; the Mac if not said.
    #[serde(default)]
    work_brain: Option<String>,
    /// The Claude it judges with; Haiku if not said (the CEO's, Sonnet).
    #[serde(default)]
    judgment_brain: Option<String>,
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
    let id = blocking(move || {
        hq.hire_by_owner_with(
            &req.name,
            role,
            req.brain.as_deref(),
            req.work_brain.as_deref(),
            req.judgment_brain.as_deref(),
            req.reports_to.map(MemberId),
        )
    })
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
struct League {
    project: i64,
    brains: Vec<String>,
}

/// The owner starts the model league: the brains play chess against each other in a project
/// that holds scenarios (ADR 0135, amended 2026-10-25).
async fn start_league(
    Extract(state): Extract<State>,
    Json(req): Json<League>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let brains = req.brains.clone();
    blocking(move || {
        let named: Vec<&str> = brains.iter().map(String::as_str).collect();
        hq.start_league(ProjectId(req.project), &named)
    })
    .await?;
    Ok(Json(json!({ "started": true, "brains": req.brains })))
}

#[derive(Deserialize)]
struct TaskProject {
    project: i64,
}

/// The owner puts an open task into an active project (ADR 0139).
async fn task_project(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<TaskProject>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.move_task_to_project(TaskId(id), ProjectId(req.project))).await?;
    Ok(Json(json!({ "task": id, "project": req.project })))
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
struct ModelAsked {
    model: String,
}

/// The owner sets a member's model (ADR 0142): every turn of its runs on it.
async fn set_model(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<ModelAsked>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let brain = blocking(move || hq.set_model(MemberId(id), &req.model)).await?;
    Ok(Json(json!({ "member": id, "model": brain.name() })))
}

#[derive(Deserialize)]
struct WorkBrainAsked {
    work_brain: String,
}

/// The owner sets what a member does its work with (ADR 0131): `mac` or `local`.
async fn set_work_brain(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<WorkBrainAsked>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let brain = blocking(move || hq.set_work_brain(MemberId(id), &req.work_brain)).await?;
    Ok(Json(json!({ "member": id, "work_brain": brain.name() })))
}

#[derive(Deserialize)]
struct JudgmentBrainAsked {
    judgment_brain: String,
}

/// The owner sets the Claude a member judges with (ADR 0131).
async fn set_judgment_brain(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<JudgmentBrainAsked>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let brain = blocking(move || hq.set_judgment_brain(MemberId(id), &req.judgment_brain)).await?;
    Ok(Json(
        json!({ "member": id, "judgment_brain": brain.name() }),
    ))
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
struct PoolTopUp {
    amount_micros: i64,
}

/// The owner adds to today's pool, and only today's (ADR 0113 rule 6): above nothing, and at
/// most three pool days a day in all.
async fn pool_top_up(
    Extract(state): Extract<State>,
    Json(req): Json<PoolTopUp>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let total = blocking(move || hq.top_up_pool(req.amount_micros)).await?;
    Ok(Json(json!({ "topped_up_today": total })))
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
struct StartScenario {
    project: i64,
    /// `chess`, the one world left (ADR 0135, amended 2026-10-30).
    #[serde(default)]
    kind: Option<String>,
    /// A chess match's players: white, then black.
    #[serde(default)]
    players: Option<[i64; 2]>,
    /// Which game it is. Today's clock when not given.
    #[serde(default)]
    seed: Option<u64>,
}

/// The owner starts a chess match, in a project that gives its work the scenario tool alone.
/// The lighthouse and Silent Station were retired (2026-10-30), and are refused.
async fn start_scenario(
    Extract(state): Extract<State>,
    Json(req): Json<StartScenario>,
) -> Result<Json<serde_json::Value>, Failed> {
    let seed = req.seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
    });
    let (Some("chess"), Some(players)) = (req.kind.as_deref(), req.players) else {
        return Err(Failed(EngineError::Refused(Refusal::Scenario(
            "the Holodeck plays chess alone: kind \"chess\" names its players: [white, black] \
             (the lighthouse and Silent Station were retired)"
                .to_owned(),
        ))));
    };
    let hq = state.hq.clone();
    let project = ProjectId(req.project);
    let task = blocking(move || hq.start_chess(project, players.map(MemberId), seed)).await?;
    Ok(Json(json!({ "task": task.0, "seed": seed })))
}

/// A scenario, its state and its log step by step.
/// One task whole, as the owner opens it.
async fn task_shown(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    Ok(Json(blocking(move || hq.task_shown(TaskId(id))).await?))
}

async fn scenario_shown(
    Extract(state): Extract<State>,
    Path(task): Path<i64>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    Ok(Json(
        blocking(move || hq.scenario_shown(TaskId(task))).await?,
    ))
}

#[derive(Deserialize)]
struct ProjectTools {
    #[serde(default)]
    tools: Option<Vec<String>>,
}

/// The owner sets the tools a project's work is given (the Holodeck plan's phase 4), or with
/// `null` takes the list away.
/// The owner resets the model league: its games set aside, every brain rated afresh.
async fn reset_league(Extract(state): Extract<State>) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    Ok(Json(blocking(move || hq.reset_league()).await?))
}

/// The owner's cut-over to one team of loops (ADR 0142): what it did.
async fn one_team(Extract(state): Extract<State>) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    Ok(Json(blocking(move || hq.become_one_team()).await?))
}

#[derive(Deserialize)]
struct LoopRank {
    rank: u8,
}

/// The owner sets the rank a loop's items start at (ADR 0142).
async fn loop_rank(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<LoopRank>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.set_loop_rank(ProjectId(id), req.rank)).await?;
    Ok(Json(json!({ "project": id, "rank": req.rank })))
}

#[derive(Deserialize)]
struct LoopItems {
    items: u8,
}

/// The owner sets how many items a loop has in hand at once, 1 to 4 (ADR 0142).
async fn loop_items(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<LoopItems>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.set_loop_items(ProjectId(id), req.items)).await?;
    Ok(Json(json!({ "project": id, "items": req.items })))
}

async fn project_tools(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<ProjectTools>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    let tools = req.tools;
    let set = tools.clone();
    blocking(move || hq.set_project_tools(ProjectId(id), set)).await?;
    let hq = state.hq.clone();
    let kept = blocking(move || hq.project_tools(ProjectId(id))).await?;
    Ok(Json(json!({ "project": id, "tools": kept })))
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

#[derive(Deserialize)]
struct RoutineProject {
    #[serde(default)]
    project: Option<i64>,
}

/// The owner moves a routine into a project under its goal, or out of one (D2).
async fn routine_project(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<RoutineProject>,
) -> Result<Json<serde_json::Value>, Failed> {
    let hq = state.hq.clone();
    blocking(move || hq.set_routine_project(RoutineId(id), req.project.map(ProjectId))).await?;
    Ok(Json(json!({ "routine": id, "project": req.project })))
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

/// Endora's node raises an alarm about the home lab: the routine that takes them fires.
async fn alarm(
    Extract(state): Extract<State>,
    Json(req): Json<AlarmRequest>,
) -> Result<Json<serde_json::Value>, Response> {
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
/// when no alarm's task is open).
async fn recovered(
    Extract(state): Extract<State>,
    Json(req): Json<AlarmRequest>,
) -> Result<Json<serde_json::Value>, Response> {
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

#[derive(Deserialize)]
struct MacState {
    ready: bool,
    #[serde(default)]
    why: String,
}

/// The node says whether the owner's Mac is free for a team turn now, and why not if it is not
/// (ADR 0130). Kept in memory and trusted for a couple of minutes: the node says it every thirty
/// seconds, so a restarted HQ learns it soon, and a node gone quiet counts as the Mac away.
async fn mac_state(
    Extract(state): Extract<State>,
    Json(req): Json<MacState>,
) -> Json<serde_json::Value> {
    state.hq.tell_mac(req.ready, &req.why);
    Json(json!({ "ok": true }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use endora_team::application::desk_ports::DeskStore;
    use endora_team::application::engine::Settings;
    use endora_team::application::event_ports::EventStore;
    use endora_team::application::office_ports::OfficeStore;
    use endora_team::application::ports::TeamStore as _;
    use endora_team::application::receipt_ports::ReceiptStore;
    use endora_team::application::run_ports::RunStore;
    use endora_team::application::schedule_ports::ScheduleStore;
    use endora_team::application::wing_ports::WingStore;
    use endora_team::domain::{Role, RunId, RunStatus, TaskId};
    use endora_team::infrastructure::{RemoteRunner, Runner, SimulatedRunner, SqliteTeamStore};
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    const TOKEN: &str = "a-token-of-at-least-thirty-two-characters";
    const RUNNER: &str = "a-runner-token-of-at-least-thirty-two-chars";

    /// HQ as it runs on the remote runner, or, `simulated`, a small team on the simulated
    /// runner: Ada, Bo, Cy and Dee, the home lab's goal, a goal Endora proposes, and Ada's first
    /// task.
    fn state(simulated: bool) -> State {
        let runner = if simulated {
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
        if simulated {
            let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
            hq.hire("Bo", Role::Developer, 40, 2, 4).unwrap();
            hq.hire("Cy", Role::Researcher, 15, 2, 2).unwrap();
            hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
            let goal = hq
                .set_goal("Keep the home lab healthy", "", Author::Owner)
                .unwrap();
            hq.set_goal(
                "Make Endora handle most things itself",
                "A proposal, waiting on your desk",
                Author::Endora,
            )
            .unwrap();
            hq.add_task(
                Author::Owner,
                endora_team::application::ports::NewTask {
                    goal,
                    project: None,
                    parent: None,
                    title: "Check the backups ran last night".to_owned(),
                    description: String::new(),
                    assignee: Some(ada),
                    priority: endora_team::domain::Priority::Normal,
                    waits_on: Vec::new(),
                },
            )
            .unwrap();
        }
        State {
            hq,
            token: Arc::new(TOKEN.to_owned()),
            runner_token: Some(Arc::new(RUNNER.to_owned())),
            tick_seconds: 30,
            last_tick: Arc::default(),
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

    /// As [`a_turn_starts`], for a developer in the engineering wing: it reports to the lead of
    /// a wing project, and only the wing changes the lab (ADR 0120), so a branch is submitted
    /// from its turns alone.
    fn a_wing_turn_starts(st: &State) -> (RunId, TaskId) {
        let (run, task) = a_turn_starts(st);
        let lead = st.hq.hire("Jane Doe", Role::Lead, 25, 2, 1).unwrap();
        let wing = st
            .hq
            .set_goal(endora_team::domain::wing::WING_GOAL, "", Author::Owner)
            .unwrap();
        st.hq
            .add_project(
                wing,
                endora_team::domain::wing::WING_PROJECT,
                "",
                lead,
                None,
            )
            .unwrap();
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

    /// The owner retired the Holodeck's other worlds (ADR 0135, amended 2026-10-30): nothing
    /// starts a lighthouse, Silent Station, a trial or a re-run any more.
    #[tokio::test]
    async fn only_chess_starts_on_the_holodeck() {
        let app = app(state(true));
        for (path, body) in [
            ("/v1/scenarios", r#"{"project":1,"member":2,"seed":7}"#),
            (
                "/v1/scenarios",
                r#"{"project":1,"kind":"silent_station","crew":[2,3,4]}"#,
            ),
            ("/v1/scenarios/rerun", r#"{"round":"round 1"}"#),
            (
                "/v1/trials",
                r#"{"name":"t","project":1,"kind":"lighthouse"}"#,
            ),
        ] {
            let res = app
                .clone()
                .oneshot(call("POST", path, Some(TOKEN), body))
                .await
                .unwrap();
            assert!(
                !res.status().is_success(),
                "{path} {body}: {}",
                res.status()
            );
        }
        let refused = app
            .clone()
            .oneshot(call(
                "POST",
                "/v1/scenarios",
                Some(TOKEN),
                r#"{"project":1,"kind":"lighthouse","member":2}"#,
            ))
            .await
            .unwrap();
        assert!(
            json_of(refused).await.to_string().contains("chess alone"),
            "the refusal says why"
        );
    }

    /// `/health` says how long ago the tick last finished (ADR 0124): none before the first,
    /// and a wedged tick shows as an age past three periods.
    #[tokio::test]
    async fn health_says_the_ticks_age() {
        let st = state(false);
        let health = |st: &State| {
            let app = app(st.clone());
            async move { json_of(app.oneshot(call("GET", "/health", None, "")).await.unwrap()).await }
        };
        let before = health(&st).await;
        assert_eq!(
            (before["last_tick_at"].clone(), before["ticking"].clone()),
            (json!(null), json!(false))
        );
        let now = SystemClock.now();
        st.last_tick
            .store(now - 5_000, std::sync::atomic::Ordering::SeqCst);
        let fresh = health(&st).await;
        assert!(fresh["tick_age_ms"].as_i64().unwrap() >= 5_000);
        assert_eq!(fresh["ticking"], json!(true));
        st.last_tick
            .store(now - 120_000, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(health(&st).await["ticking"], json!(false), "wedged");
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
    async fn retired_team_v1_requires_a_live_token_and_never_dispatches_a_batch() {
        let st = state(true);
        st.hq.tick().unwrap();
        let turn = scripted(&st).started()[0].clone();
        let app = app(st.clone());
        let body = json!([
            {"jsonrpc":"2.0","id":1,"method":"initialize"},
            {"jsonrpc":"2.0","id":2,"method":"tools/list"},
            {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"consult","arguments":{"member":"Member 2","question":"Fixture?"}}},
            {"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"answer_consult","arguments":{"answer":"Fixture answer"}}}
        ]).to_string();
        let before = (
            st.hq.store().runs().unwrap().len(),
            st.hq.wakes().unwrap().len(),
        );
        for token in [None, Some(TOKEN), Some("not-a-turn")] {
            let res = app
                .clone()
                .oneshot(call("POST", "/v1/team/mcp", token, &body))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }
        let res = app
            .clone()
            .oneshot(call("POST", "/v1/team/mcp", Some(&turn.token), &body))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::GONE);
        assert!(
            json_of(res).await["error"]
                .as_str()
                .unwrap()
                .contains("/v2/team/mcp")
        );
        assert_eq!(
            before,
            (
                st.hq.store().runs().unwrap().len(),
                st.hq.wakes().unwrap().len()
            )
        );
        scripted(&st).finish(turn.run, true);
        st.hq.tick().unwrap();
        let res = app
            .oneshot(call("POST", "/v1/team/mcp", Some(&turn.token), &body))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn mcp_answers_batches_and_never_answers_a_notification() {
        let st = state(true);
        st.hq.tick().unwrap();
        let turn = scripted(&st).started()[0].clone();
        let app = app(st.clone());
        let speak = |body: serde_json::Value| {
            call("POST", "/v2/team/mcp", Some(&turn.token), &body.to_string())
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
                    "/v2/team/mcp",
                    token,
                    &rpc(1, "tools/list", json!({})),
                ))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{token:?}");
        }
        let speak = |body: String| call("POST", "/v2/team/mcp", Some(&turn.token), &body);
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
        // Every member is offered the same tools, and none of the company's (ADR 0142): the
        // work's, the record's, chess, and the map.
        assert_eq!(
            names,
            [
                "my_turn",
                "claim",
                "post",
                "set_status",
                "create_task",
                "ask_owner",
                "check_back",
                "recheck",
                "read_document",
                "read_doc",
                "chess",
                "read_world",
                "ask_record",
                "add_to_map"
            ]
        );
        assert!(
            !names.contains(&"report_usage"),
            "the relay charges for model use, not the worker"
        );
        // A tool only a quiz turn or a consulted turn can use is offered in those turns alone.
        for special in ["answer_quiz", "answer_consult"] {
            assert!(
                !names.contains(&special),
                "{special} is for its own turn: {names:?}"
            );
        }
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
                    json!({ "name": "format_the_disk", "arguments": {} }),
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
    async fn only_the_node_keeps_a_turns_conversation_and_the_replay_shows_it() {
        let st = state(true);
        st.hq.tick().unwrap();
        let turn = scripted(&st).started()[0].clone();
        let app = app(st.clone());
        let tell = |signed: Option<&str>, token: &str, text: &str| {
            call(
                "POST",
                "/v1/team/transcript",
                signed,
                &json!({ "token": token, "runtime": "worker", "text": text }).to_string(),
            )
        };
        let said = "--- assistant ---\nI will cap sonarr at 512M.";
        let res = app
            .clone()
            .oneshot(tell(Some(&turn.token), &turn.token, said))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "a worker cannot write its own history"
        );
        let forged = app
            .clone()
            .oneshot(tell(Some(TOKEN), "not-a-turns-token", said))
            .await
            .unwrap();
        assert_eq!(forged.status(), StatusCode::NOT_FOUND);
        let kept = json_of(
            app.clone()
                .oneshot(tell(Some(TOKEN), &turn.token, said))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            (&kept["kept"], &kept["tier"]),
            (&json!(true), &json!("internal"))
        );
        let again = app
            .clone()
            .oneshot(tell(Some(TOKEN), &turn.token, said))
            .await
            .unwrap();
        assert_eq!(again.status(), StatusCode::BAD_REQUEST, "a turn has one");
        // The owner's replay carries it.
        let replay = json_of(
            app.clone()
                .oneshot(call(
                    "GET",
                    &format!("/v1/runs/{}/replay", turn.run.0),
                    Some(TOKEN),
                    "",
                ))
                .await
                .unwrap(),
        )
        .await;
        assert!(
            replay["replay"]["transcript"]["text"]
                .as_str()
                .is_some_and(|t| t.contains("cap sonarr at 512M")),
            "{replay}"
        );
    }

    #[tokio::test]
    async fn retained_task_questions_are_owner_only_and_read_only() {
        let st = state(true);
        st.hq.tick().unwrap();
        let turn = scripted(&st).started()[0].clone();
        let task = turn.task.unwrap();
        let path = format!("/v1/tasks/{}/consultations?after=-1", task.0);
        let before = (
            st.hq.wakes().unwrap().len(),
            st.hq.store().runs().unwrap().len(),
        );
        let app = app(st.clone());
        for token in [None, Some("wrong"), Some(turn.token.as_str()), Some(RUNNER)] {
            let response = app
                .clone()
                .oneshot(call("GET", &path, token, ""))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        let response = app
            .clone()
            .oneshot(call("GET", &path, Some(TOKEN), ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let page = json_of(response).await;
        assert_eq!(page["source"], "legacy-task-consultation");
        assert_eq!(page["task"], task.0);
        assert!(page["records"].as_array().unwrap().is_empty());
        assert_eq!(page["after"], 0);
        let response = app
            .clone()
            .oneshot(call(
                "GET",
                "/v1/tasks/999999/consultations",
                Some(TOKEN),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = app
            .oneshot(call("POST", &path, Some(TOKEN), "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            before,
            (
                st.hq.wakes().unwrap().len(),
                st.hq.store().runs().unwrap().len()
            )
        );
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
        // The owner opens the task whole.
        let opened = json_of(
            app.clone()
                .oneshot(call("GET", &format!("/v1/tasks/{task}"), Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(opened["id"], task);
        assert!(opened["thread"].is_array(), "{opened}");
        for missing in [
            "/v1/runs/999999/replay",
            "/v1/tasks/999999/replay",
            "/v1/tasks/999999",
        ] {
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
        // Ada, the demo's lead, judges with Claude: her turns of judgment are paid from the
        // pool. (A turn of work reserves nothing, ADR 0131.) She is asked to decide the task
        // she was handed.
        let ada = st.hq.store().members().unwrap()[0].id;
        st.hq.set_brain(ada, "claude-sonnet-5-5").unwrap();
        let hers = st
            .hq
            .store()
            .tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.assignee == Some(ada))
            .expect("the demo hands Ada a task")
            .id;
        st.hq
            .wake(ada, Some(hers), endora_team::domain::WakeReason::Decide)
            .unwrap();
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
        // Any member at work may change the lab (ADR 0142).
        assert_eq!(found["wing"], true, "{found}");
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
    async fn the_node_tells_the_owner_of_a_contracts_fix_as_a_notice_in_the_digest() {
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
        // A notice needs no decision, so it waits in the digest and does not interrupt (ADR 0132).
        assert!(
            snap["desk"]
                .as_array()
                .unwrap()
                .iter()
                .all(|d| d["id"] != item),
            "a notice is not on the desk: {}",
            snap["desk"]
        );
        let shown = snap["digest"]
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
        let text = "Ada asks to run sonarr_retry_import. Why: stuck.";
        let body = json!({ "run": turn.run.0, "text": text, "serves": "1" }).to_string();
        let res = app
            .clone()
            .oneshot(call("POST", "/v1/team/approval", Some(&turn.token), &body))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "only the node asks");
        // A request serves one of its task's criteria (ADR 0124, amended). The demo's task has
        // none: refused with the way out, and nothing reaches the desk.
        let task = turn.task.expect("the turn is on a task");
        let res = app
            .clone()
            .oneshot(call("POST", "/v1/team/approval", Some(TOKEN), &body))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let said = json_of(res).await;
        assert!(
            said["error"].as_str().unwrap().contains("has no criteria"),
            "{said}"
        );
        st.hq
            .set_criteria(
                task,
                &[endora_team::domain::Criterion::new(
                    "The queue is clear",
                    false,
                )],
            )
            .unwrap();
        // It serves none of them: refused with the criteria listed, the desk still empty.
        let without = json!({ "run": turn.run.0, "text": text }).to_string();
        let res = app
            .clone()
            .oneshot(call("POST", "/v1/team/approval", Some(TOKEN), &without))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let said = json_of(res).await;
        assert!(
            said["error"]
                .as_str()
                .unwrap()
                .contains("does asking the owner serve? Give serves (its number or its words): 1. The queue is clear"),
            "{said}"
        );
        assert!(
            st.hq
                .store()
                .desk()
                .unwrap()
                .iter()
                .all(|d| d.kind != "approval"),
            "it never reached the desk"
        );
        let asked = json_of(
            app.clone()
                .oneshot(call("POST", "/v1/team/approval", Some(TOKEN), &body))
                .await
                .unwrap(),
        )
        .await;
        let item = asked["item"].as_i64().unwrap();
        // The owner's card says which criterion the request serves.
        let snap = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/snapshot", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        let card = snap["desk"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == item)
            .cloned()
            .expect("the approval is on the desk");
        assert_eq!(
            card["serves"],
            json!({ "n": 1, "text": "The queue is clear" }),
            "{card}"
        );
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
                    &json!({ "goal": goal, "title": "Look at the music stack",
                        "criteria": ["The stack's state is in the thread"] })
                    .to_string(),
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

    #[tokio::test]
    async fn the_owner_hands_out_work_only_with_what_done_means() {
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
        let give =
            |body: serde_json::Value| call("POST", "/v1/tasks", Some(TOKEN), &body.to_string());
        // Give work with no line, or only blank ones, is refused with the way to give one.
        for none in [
            json!({ "goal": goal, "title": "Fix the backups" }),
            json!({ "goal": goal, "title": "Fix the backups", "criteria": [] }),
            json!({ "goal": goal, "title": "Fix the backups", "criteria": ["  ", ""] }),
        ] {
            let res = app.clone().oneshot(give(none.clone())).await.unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{none}");
            let said = json_of(res).await;
            let words = said["error"].as_str().unwrap();
            assert!(
                words.contains("what done means") && words.contains("live:"),
                "{said}"
            );
        }
        assert!(
            st.hq.store().tasks().unwrap().is_empty(),
            "a refusal makes nothing"
        );
        // With one line it is handed out, and `live:` marks a line about the lab now.
        let res = app
            .clone()
            .oneshot(give(json!({ "goal": goal, "title": "Fix the backups",
                "criteria": ["The backups finish by 6 am", "live: the disk is under 80% full"] })))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let task = json_of(res).await["task"].as_i64().unwrap();
        let kept = st.hq.store().task(TaskId(task)).unwrap().unwrap().criteria;
        assert_eq!(
            kept.iter()
                .map(|c| (c.text.as_str(), c.live))
                .collect::<Vec<_>>(),
            [
                ("The backups finish by 6 am", false),
                ("the disk is under 80% full", true)
            ]
        );
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

    /// Explicit assistance and its result verification do not inherit execution capabilities.
    #[tokio::test]
    async fn a_deploy_checks_turn_is_handed_without_a_shell_and_with_the_lab_reads_alone() {
        let st = state(false);
        let (run, task) = a_turn_starts(&st);
        let app = app(st.clone());
        let next = || call("POST", "/v1/runner/next", Some(RUNNER), r#"{"free":2}"#);
        let whose = |token: &str| {
            call(
                "POST",
                "/v1/team/turn",
                Some(TOKEN),
                &json!({ "token": token }).to_string(),
            )
        };
        // An ordinary turn first: a shell where the runner has one, and no list.
        let other = st.hq.store().runs().unwrap()[0].clone();
        assert_eq!(other.id, run);
        st.hq
            .store()
            .set_setting(&format!("deploy_check:{}", task.0), "1")
            .unwrap();
        let handed = json_of(app.clone().oneshot(next()).await.unwrap()).await;
        assert_eq!(handed["run"], run.0);
        assert_eq!(handed["shell"], false, "{handed}");
        let token = handed["token"].as_str().unwrap().to_owned();
        let found = json_of(app.clone().oneshot(whose(&token)).await.unwrap()).await;
        assert_eq!(
            found["only"],
            json!(["homelab_status", "container_logs", "nas_status"]),
            "{found}"
        );
        // Its task does not say to leave the lab alone: nothing is refused beside the list.
        assert_eq!(found["never"], json!([]), "{found}");
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
        assert_eq!(
            turn["shell"], true,
            "a turn has a shell where the runner gives one"
        );
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
    async fn a_container_the_runner_found_gone_or_killed_is_the_platforms_and_a_workers_exit_is_its_own()
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
        let report = |lease: u64, body: serde_json::Value| {
            let mut body = body;
            body["lease"] = json!(lease);
            call(
                "POST",
                &format!("/v1/runner/{}/ended", run.0),
                Some(RUNNER),
                &body.to_string(),
            )
        };
        // The retry waits a minute, which the system clock cannot skip: it is made due by hand.
        let due_now = || {
            let mut waiting = st.hq.store().run(run).unwrap().unwrap();
            waiting.retry_at = Some(0);
            st.hq.store().save_run(&waiting).unwrap();
            st.hq.tick().unwrap();
        };
        let lease = |app: &axum::Router| {
            let app = app.clone();
            async move {
                json_of(app.oneshot(next()).await.unwrap()).await["lease"]
                    .as_u64()
                    .unwrap()
            }
        };

        // The runner found the worker's container gone before it saw it end: a runner says why
        // and no exit code. HQ takes that for the platform's, and the member loses nothing.
        let first = lease(&app).await;
        let said = app
            .clone()
            .oneshot(report(
                first,
                json!({ "succeeded": false, "code": null,
                        "why": "its container was gone before it was seen to end" }),
            ))
            .await
            .unwrap();
        assert_eq!(said.status(), StatusCode::NO_CONTENT);
        assert_eq!(st.hq.tick().unwrap().retrying, 1);
        let kept = st.hq.store().run(run).unwrap().unwrap();
        assert_eq!(
            (kept.status, kept.attempts, kept.interrupted),
            (RunStatus::RetryWaiting, 1, 1)
        );
        assert!(
            kept.end_reason.starts_with("interrupted: "),
            "{}",
            kept.end_reason
        );

        // Killed from outside (Docker stopping it): the same, the second time without the
        // allowance, which was the first's.
        due_now();
        let second = lease(&app).await;
        let said = app
            .clone()
            .oneshot(report(
                second,
                json!({ "succeeded": false, "code": 137, "why": "killed (exit 137)" }),
            ))
            .await
            .unwrap();
        assert_eq!(said.status(), StatusCode::NO_CONTENT);
        assert_eq!(st.hq.tick().unwrap().retrying, 1);
        let kept = st.hq.store().run(run).unwrap().unwrap();
        assert_eq!((kept.attempts, kept.interrupted), (2, 2));

        // Its worker's own exit, or the runner saying nothing at all, is not an interruption.
        due_now();
        let third = lease(&app).await;
        let said = app
            .clone()
            .oneshot(report(
                third,
                json!({ "succeeded": false, "code": 1, "why": "exit 1: worker: the model answered 500" }),
            ))
            .await
            .unwrap();
        assert_eq!(said.status(), StatusCode::NO_CONTENT);
        st.hq.tick().unwrap();
        let kept = st.hq.store().run(run).unwrap().unwrap();
        assert_eq!(
            (kept.attempts, kept.interrupted),
            (3, 2),
            "its worker's failure"
        );
        assert!(
            !kept.end_reason.starts_with("interrupted: "),
            "{}",
            kept.end_reason
        );
        due_now();
        let fourth = lease(&app).await;
        let said = app
            .clone()
            .oneshot(report(fourth, json!({ "succeeded": false })))
            .await
            .unwrap();
        assert_eq!(said.status(), StatusCode::NO_CONTENT);
        st.hq.tick().unwrap();
        let kept = st.hq.store().run(run).unwrap().unwrap();
        assert_eq!(
            (kept.status, kept.attempts, kept.interrupted),
            (RunStatus::Failed, 4, 2),
            "an older runner's bare report is a failure, as ever"
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
        // Since one team (ADR 0142) a lead may think with Opus; a model Endora lacks is refused.
        let unknown = app
            .clone()
            .oneshot(set(Some(TOKEN), lead.0, "gpt-5"))
            .await
            .unwrap();
        assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
        assert!(
            json_of(unknown).await["error"]
                .as_str()
                .unwrap()
                .contains("thinks with")
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

        // A task handed to the lead, and the lead asked to decide it: a turn of judgment, which
        // is the one that runs on Claude (ADR 0131).
        let goal = st
            .hq
            .set_goal("Keep the lab tidy", "a test", Author::Owner)
            .unwrap();
        let look = st
            .hq
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
        st.hq
            .wake(lead, Some(look), endora_team::domain::WakeReason::Decide)
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
    async fn the_owner_sets_both_brains_and_a_turn_of_work_is_handed_out_on_the_work_brain() {
        let engine = endora_team::application::engine::Engine::new(
            SqliteTeamStore::in_memory().unwrap(),
            Runner::Remote(RemoteRunner::default()),
            SystemClock,
            Settings::default(),
        );
        let st = State {
            hq: Arc::new(engine),
            token: Arc::new(TOKEN.to_owned()),
            runner_token: Some(Arc::new(RUNNER.to_owned())),
            tick_seconds: 30,
            last_tick: Arc::default(),
        };
        let lead = st.hq.hire("Jane Doe", Role::Lead, 25, 2, 0).unwrap();
        let app = app(st.clone());
        let set = |path: &str, token: Option<&str>, body: serde_json::Value| {
            call(
                "POST",
                &format!("/v1/members/{}/{path}", lead.0),
                token,
                &body.to_string(),
            )
        };
        let stranger = app
            .clone()
            .oneshot(set(
                "work-brain",
                Some(RUNNER),
                json!({ "work_brain": "mac" }),
            ))
            .await
            .unwrap();
        assert_eq!(
            stranger.status(),
            StatusCode::UNAUTHORIZED,
            "the node's alone"
        );
        // Claude never does the work; the house's models do.
        let claude = app
            .clone()
            .oneshot(set(
                "work-brain",
                Some(TOKEN),
                json!({ "work_brain": "claude-sonnet-5-5" }),
            ))
            .await
            .unwrap();
        assert_eq!(claude.status(), StatusCode::BAD_REQUEST);
        let mac = json_of(
            app.clone()
                .oneshot(set(
                    "work-brain",
                    Some(TOKEN),
                    json!({ "work_brain": "mac" }),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(mac["work_brain"], "mac");
        // Judgment is a cloud model's, and a member cannot be left with none.
        for refused in ["gpt-5", "local", "none"] {
            let res = app
                .clone()
                .oneshot(set(
                    "judgment-brain",
                    Some(TOKEN),
                    json!({ "judgment_brain": refused }),
                ))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{refused}");
        }
        let sonnet = json_of(
            app.clone()
                .oneshot(set(
                    "judgment-brain",
                    Some(TOKEN),
                    json!({ "judgment_brain": "claude-sonnet-5-5" }),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(sonnet["judgment_brain"], "claude-sonnet-5-5");

        // A task handed to the lead is work, and the Mac has not said it is free: the house's
        // own model does it, on the worker, and the node's relay is told so.
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
        assert_eq!(handed["brain"], "local", "work, and the Mac is not free");
        assert_eq!(handed["runtime"], "worker");
        let token = handed["token"].as_str().unwrap();
        let found = json_of(
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
        assert_eq!(
            found["cloud"], false,
            "nothing of a work turn reaches Anthropic"
        );
        assert_eq!(found["brain"], "local");
        let shown = st.hq.snapshot().unwrap();
        let jane = &shown["members"][0];
        assert_eq!(jane["work_brain"], "mac");
        assert_eq!(jane["judgment_brain"], "claude-sonnet-5-5");
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
        // Every member judges with a Claude (ADR 0131), so a developer may be given Claude Code
        // to do that judging with; a lead keeps Endora's worker.
        let refused = app
            .clone()
            .oneshot(set(Some(TOKEN), lead.0, "claude-code"))
            .await
            .unwrap();
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST, "a lead");
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

        // A task handed to the developer, and the developer asked to decide it: a turn of
        // judgment, which is the one Claude Code runs (work is the worker's, on a house model).
        let goal = st
            .hq
            .set_goal("Tidy the fork", "a test", Author::Owner)
            .unwrap();
        let tidy = st
            .hq
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
        st.hq
            .wake(dev, Some(tidy), endora_team::domain::WakeReason::Decide)
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
    async fn an_alarm_from_the_node_reaches_the_routine_that_takes_it() {
        let alarm = |token: Option<&str>| {
            call(
                "POST",
                "/v1/alarms",
                token,
                &json!({ "text": "sonarr became unhealthy at 14:02" }).to_string(),
            )
        };

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

    #[tokio::test]
    async fn the_node_tells_whether_the_mac_is_free_and_the_snapshot_shows_it() {
        let st = state(false);
        let app = app(st.clone());
        let said = |token: Option<&str>, ready: bool, why: &str| {
            call(
                "POST",
                "/v1/mac",
                token,
                &json!({ "ready": ready, "why": why }).to_string(),
            )
        };
        let res = app
            .clone()
            .oneshot(said(None, true, "ready"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "only the node");
        assert_eq!(st.hq.snapshot().unwrap()["mac"]["ready"], false);
        let res = app
            .clone()
            .oneshot(said(Some(TOKEN), false, "owner active"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let shown = st.hq.snapshot().unwrap();
        assert_eq!(shown["mac"]["ready"], false);
        assert_eq!(shown["mac"]["why"], "owner active");
        app.oneshot(said(Some(TOKEN), true, "ready")).await.unwrap();
        assert_eq!(st.hq.snapshot().unwrap()["mac"]["ready"], true);
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
    async fn the_owner_makes_caps_and_pauses_a_project() {
        let st = state(false);
        let lead = st.hq.hire("John Doe", Role::Lead, 25, 2, 0).unwrap();
        st.hq.hire("Jane Doe", Role::Lead, 25, 2, 0).unwrap();
        st.hq
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
            json!({ "goal": goal.0, "title": "Fix sonarr", "project": id,
                "criteria": ["Sonarr is running and healthy"] })
            .to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
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
    }

    #[tokio::test]
    async fn the_snapshot_carries_the_office_and_the_office_routes_are_gone() {
        let st = state(false);
        st.hq.seed_office().unwrap();
        let seeded = st.hq.store().office_items().unwrap().len();
        let res = app(st.clone())
            .oneshot(call("POST", "/v1/office/reset", Some(TOKEN), "{}"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "the office is not run");
        let snap = st.hq.snapshot().unwrap();
        assert_eq!(snap["office"]["items"].as_array().unwrap().len(), seeded);
    }

    #[tokio::test]
    async fn only_the_node_tops_the_pool_up_and_the_limit_is_told() {
        let st = state(false);
        let app = app(st.clone());
        let send = |body: serde_json::Value, signed: Option<&'static str>| {
            let app = app.clone();
            async move {
                let res = app
                    .oneshot(call("POST", "/v1/pool/top-up", signed, &body.to_string()))
                    .await
                    .unwrap();
                (res.status(), json_of(res).await)
            }
        };
        let ask = json!({ "amount_micros": 1_000_000 });
        let (status, _) = send(ask.clone(), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "no token");
        let (status, _) = send(ask.clone(), Some("wrong")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "the wrong token");
        assert_eq!(st.hq.snapshot().unwrap()["pool"]["topped_up"], 0);
        let (status, said) = send(ask, Some(TOKEN)).await;
        assert_eq!(
            (status, said["topped_up_today"].clone()),
            (StatusCode::OK, json!(1_000_000))
        );
        let (status, said) = send(json!({ "amount_micros": 99_000_000 }), Some(TOKEN)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(said.to_string().contains("three"), "{said}");
        assert_eq!(st.hq.snapshot().unwrap()["pool"]["topped_up"], 1_000_000);
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
        {
            let id = st.hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
            st.hq.set_brain(id, "claude-opus-5-5").unwrap();
            id
        };
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
        let (run, task) = a_wing_turn_starts(&st);
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
        let named = json_of(named).await;
        assert_eq!(named["head"], "a1b2c3d4e5f60718");
        assert_eq!(named["base"], "0123456789abcdef", "the base it sits on");
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
    async fn the_node_asks_hq_which_head_of_a_kept_branch_a_turn_may_check_out() {
        let st = state(false);
        let (_, task) = a_wing_turn_starts(&st);
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
        let token = handed["token"].as_str().unwrap().to_owned();
        let branch = json!({ "token": token, "branch": "hq/task-1-sort-the-cables",
                             "head": "a1b2c3d4e5f60718", "base": "0123456789abcdef", "files": 1,
                             "plus": 2, "minus": 0, "stat": " a | 2 ++", "diff": "+x\n+y",
                             "can_push": true });
        let res = app
            .clone()
            .oneshot(call(
                "POST",
                "/v1/team/branch",
                Some(TOKEN),
                &branch.to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let checkout = |signed: Option<&str>, token: &str, name: &str| {
            call(
                "POST",
                "/v1/team/branch/checkout",
                signed,
                &json!({ "token": token, "branch": name }).to_string(),
            )
        };
        // Signed by the node, naming the turn by its own token: the head HQ recorded, the base
        // it sits on, and the task it is for.
        let res = app
            .clone()
            .oneshot(checkout(Some(TOKEN), &token, "hq/task-1-sort-the-cables"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            json_of(res).await,
            json!({ "head": "a1b2c3d4e5f60718", "base": "0123456789abcdef", "task": task.0 })
        );
        // Not by a worker, and not for a branch HQ never recorded or a turn that is not running.
        for (signed, who, name, status) in [
            (
                Some(token.as_str()),
                token.as_str(),
                "hq/task-1-sort-the-cables",
                StatusCode::UNAUTHORIZED,
            ),
            (
                None,
                token.as_str(),
                "hq/task-1-sort-the-cables",
                StatusCode::UNAUTHORIZED,
            ),
            (
                Some(TOKEN),
                token.as_str(),
                "hq/task-1-none",
                StatusCode::NOT_FOUND,
            ),
            (
                Some(TOKEN),
                "forged",
                "hq/task-1-sort-the-cables",
                StatusCode::NOT_FOUND,
            ),
        ] {
            let res = app
                .clone()
                .oneshot(checkout(signed, who, name))
                .await
                .unwrap();
            assert_eq!(res.status(), status, "{signed:?} {who} {name}");
        }
    }

    #[tokio::test]
    async fn an_approved_push_names_its_branch_for_the_node_and_its_outcome_is_told_once() {
        let st = state(false);
        let (run, task) = a_wing_turn_starts(&st);
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

    /// The drain's door (ADR 0134) is the node's token's alone, as the owner's actions are: not
    /// a runner's, not a worker's, not nobody's.
    #[tokio::test]
    async fn only_the_hq_token_opens_the_drain() {
        let st = state(false);
        let app = app(st.clone());
        for (method, uri, body) in [
            ("GET", "/v1/team/drain", ""),
            ("POST", "/v1/team/drain", "{}"),
            ("POST", "/v1/team/resume", ""),
        ] {
            for token in [None, Some(""), Some("wrong"), Some(RUNNER)] {
                let res = app
                    .clone()
                    .oneshot(call(method, uri, token, body))
                    .await
                    .unwrap();
                assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{uri} {token:?}");
            }
        }
        assert!(!st.hq.draining(), "nothing was started by the refused");
    }

    #[tokio::test]
    async fn a_deploy_drains_reads_the_drain_and_resumes() {
        let st = state(false);
        let (run, _) = a_turn_starts(&st);
        let app = app(st.clone());

        let before = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/team/drain", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(before["draining"], false);
        assert_eq!(
            before["running"], 1,
            "a turn HQ started is running, held or not"
        );

        let res = app
            .clone()
            .oneshot(call(
                "POST",
                "/v1/team/drain",
                Some(TOKEN),
                r#"{"cap_seconds":120}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let began = json_of(res).await;
        assert_eq!(began["draining"], true);
        assert_eq!(began["cap_seconds"], 120);
        assert!(began["since"].as_i64().unwrap() > 0, "{began}");
        assert!(st.hq.draining());
        assert!(status_of(&st, run) != RunStatus::Failed);

        // No cap given: the default of five minutes. Asking again does not begin another.
        let again = json_of(
            app.clone()
                .oneshot(call("POST", "/v1/team/drain", Some(TOKEN), "{}"))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(again["since"], began["since"]);
        assert_eq!(again["cap_seconds"], 120);

        let read = json_of(
            app.clone()
                .oneshot(call("GET", "/v1/team/drain", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(read["draining"], true);

        let resumed = json_of(
            app.clone()
                .oneshot(call("POST", "/v1/team/resume", Some(TOKEN), ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(resumed["draining"], false);
        assert!(!st.hq.draining());
        assert_eq!(resumed["last"]["by"], "resume");
    }

    /// The command a deploy runs in the container, against a real HQ over a real socket: it
    /// announces the restart, finds nothing running, and lifts it again.
    #[tokio::test]
    async fn the_drain_command_drains_a_real_hq_over_tcp_and_resumes_it() {
        use crate::drain_cli::{Command, Door as _, Tcp, World, run};
        let st = state(false);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let served = app(st.clone());
        tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });

        let command = |door: Tcp, command: Command| {
            let now = || 1_i64;
            let sleep = |_: u64| {};
            let mut out = Vec::new();
            let code = run(
                &command,
                &World {
                    door: &door,
                    now: &now,
                    sleep: &sleep,
                },
                &mut out,
            );
            (code, String::from_utf8(out).unwrap())
        };
        let (a, t) = (addr.clone(), TOKEN.to_owned());
        let (code, said) = tokio::task::spawn_blocking(move || {
            command(
                Tcp { addr: a, token: t },
                Command::Drain {
                    cap: Some(30),
                    wait: true,
                },
            )
        })
        .await
        .unwrap();
        assert_eq!(code, 0, "{said}");
        assert!(said.contains("no turn is running"), "{said}");
        assert!(st.hq.draining(), "the real HQ was told");

        let (a, t) = (addr.clone(), TOKEN.to_owned());
        let (code, said) = tokio::task::spawn_blocking(move || {
            command(Tcp { addr: a, token: t }, Command::Resume)
        })
        .await
        .unwrap();
        assert_eq!(code, 0, "{said}");
        assert!(!st.hq.draining());

        // A wrong token is refused, said, and changes nothing.
        let (a, wrong) = (addr.clone(), RUNNER.to_owned());
        let (code, said) = tokio::task::spawn_blocking(move || {
            command(
                Tcp {
                    addr: a,
                    token: wrong,
                },
                Command::Drain {
                    cap: None,
                    wait: false,
                },
            )
        })
        .await
        .unwrap();
        assert_eq!(code, 1, "{said}");
        assert!(said.contains("401"), "{said}");
        assert!(!st.hq.draining());

        // And a door that nobody listens at is said, not waited for.
        let gone = tokio::task::spawn_blocking(move || {
            Tcp {
                addr: "127.0.0.1:1".to_owned(),
                token: TOKEN.to_owned(),
            }
            .send("GET", "/v1/team/drain", "")
        })
        .await
        .unwrap();
        assert!(gone.is_err());
    }

    #[tokio::test]
    async fn a_drain_with_no_cap_waits_five_minutes() {
        let app = app(state(false));
        let began = json_of(
            app.oneshot(call("POST", "/v1/team/drain", Some(TOKEN), "{}"))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(began["cap_seconds"], 300);
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
