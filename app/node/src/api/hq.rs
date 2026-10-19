//! Endora HQ through the node (ADR 0102). The console talks to the node only; the node signs
//! every request to HQ, and writes every action of the owner's into Endora's own audit log,
//! which is the record. HQ's own history is for display.

use super::*;
use endora_infrastructure::HqError;

/// The team's world (ADR 0122).
mod world;
pub(super) use world::*;

/// The guards (ADR 0124).
mod guards;
pub(super) use guards::*;

fn client(
    state: &AppState,
) -> Result<Arc<endora_infrastructure::HqClient>, (StatusCode, Json<serde_json::Value>)> {
    state.hq.clone().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "configured": false, "error": "HQ is not set up on this node" })),
    ))
}

fn failed(error: HqError) -> (StatusCode, Json<serde_json::Value>) {
    let status = match &error {
        HqError::Unreachable(_) => StatusCode::BAD_GATEWAY,
        HqError::Answered { status, .. } => {
            StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
        }
    };
    (status, Json(json!({ "error": error.to_string() })))
}

type Answer = Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>;

async fn ask(state: &AppState, path: String, body: Option<serde_json::Value>) -> Answer {
    let hq = client(state)?;
    let answer = tokio::task::spawn_blocking(move || match body {
        Some(body) => hq.post(&path, &body),
        None => hq.get(&path),
    })
    .await
    .map_err(|_| failed(HqError::Unreachable("a worker thread failed".to_owned())))?;
    answer.map(Json).map_err(failed)
}

/// Writes an owner's action in HQ, or a team member's use of a contract, to Endora's audit
/// log, and to Activity.
pub(super) fn record(state: &AppState, what: &str) {
    let now = state.clock.now();
    if let Ok(entry) = endora_application::AuditRecord::new(
        endora_application::AuditId::new(state.ids.new_id()),
        now,
        &format!("HQ: {what}"),
    ) {
        let _ = endora_application::AuditLog::append(state.audit.as_ref(), &entry);
    }
    let _ =
        endora_application::EventLog::record(state.events.as_ref(), now, &format!("HQ: {what}"));
    let _ = state.changes.send(());
}

/// Everything the office shows now, and the owner's switches for the team's contracts.
pub(super) async fn hq_snapshot(State(state): State<AppState>) -> Answer {
    let mut answer = ask(&state, "/v1/snapshot".to_owned(), None).await?;
    answer.0["contracts"] = super::contracts::switches_said(&state);
    // Each approval's card shows what will run, from what the node stored (ADR 0108).
    let looked = state.clone();
    let mut desk = answer.0["desk"].take();
    let desk = tokio::task::spawn_blocking(move || {
        for item in desk.as_array_mut().into_iter().flatten() {
            if item["kind"] != "approval" {
                continue;
            }
            if let Some(approval) = item["id"]
                .as_i64()
                .and_then(|id| super::bridge::approval_at(&looked, id))
                .filter(|a| item["text"] == super::bridge::desk_text(a).as_str())
            {
                item["approval"] = super::bridge::shown(&approval);
            }
        }
        desk
    })
    .await
    .unwrap_or_default();
    answer.0["desk"] = desk;
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqAfter {
    #[serde(default)]
    after: i64,
    /// With `before`, the past instead: the events just below this id.
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    limit: Option<u32>,
}

/// The events after an id: the office's stream, resumed from the last id it saw. With
/// `before` (and a `limit`), the events just below that id instead: the activity log going
/// further back. Numbers only reach HQ, rebuilt here, never the caller's own text.
pub(super) async fn hq_events(State(state): State<AppState>, Query(q): Query<HqAfter>) -> Answer {
    let Some(before) = q.before else {
        return ask(&state, format!("/v1/events?after={}", q.after), None).await;
    };
    let limit = q.limit.unwrap_or(50).min(200);
    ask(
        &state,
        format!("/v1/events?before={before}&limit={limit}"),
        None,
    )
    .await
}

/// Replay: one turn read back in full, for the owner's screen. Read only, so nothing is
/// recorded; a checking credential never reaches it (`crate::auth`), as it carries what the
/// member was told and Confidential excerpts.
pub(super) async fn hq_run_replay(State(state): State<AppState>, Path(id): Path<i64>) -> Answer {
    ask(&state, format!("/v1/runs/{id}/replay"), None).await
}

/// Replay: a task's turns read back, oldest first.
pub(super) async fn hq_task_replay(State(state): State<AppState>, Path(id): Path<i64>) -> Answer {
    ask(&state, format!("/v1/tasks/{id}/replay"), None).await
}

#[derive(Deserialize)]
pub(super) struct HqGoal {
    title: String,
    #[serde(default)]
    reason: String,
}

/// The owner sets a goal.
pub(super) async fn hq_goal(State(state): State<AppState>, Json(req): Json<HqGoal>) -> Answer {
    let answer = ask(
        &state,
        "/v1/goals".to_owned(),
        Some(json!({ "title": req.title, "reason": req.reason })),
    )
    .await?;
    record(&state, &format!("you set a goal: {}", req.title.trim()));
    Ok(answer)
}

/// The owner hands the team a piece of work.
pub(super) async fn hq_task(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Answer {
    let title = req["title"].as_str().unwrap_or_default().trim().to_owned();
    let answer = ask(&state, "/v1/tasks".to_owned(), Some(req)).await?;
    record(&state, &format!("you handed the team: {title}"));
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqDecision {
    pub(super) decision: String,
    /// For a member's question: the owner's answer, which HQ writes in the task's thread.
    #[serde(default)]
    pub(super) answer: String,
}

/// The owner decides an item on their desk. An approval of the team's (ADR 0108) is decided
/// here, on the node, which runs exactly what it stored when the owner says yes.
pub(super) async fn hq_decide(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqDecision>,
) -> Answer {
    client(&state)?;
    let looked = state.clone();
    let approval = tokio::task::spawn_blocking(move || super::bridge::approval_at(&looked, id))
        .await
        .unwrap_or(None);
    if let Some(approval) = approval {
        let decided = state.clone();
        let decision = req.decision.clone();
        return tokio::task::spawn_blocking(move || {
            super::bridge::decide(&decided, &approval, &decision)
        })
        .await
        .map_err(|_| failed(HqError::Unreachable("a worker thread failed".to_owned())))?
        .map(Json)
        .map_err(|(status, why)| (status, Json(json!({ "error": why }))));
    }
    let mut answer = ask(
        &state,
        format!("/v1/desk/{id}"),
        Some(json!({ "decision": req.decision, "answer": req.answer })),
    )
    .await?;
    record(
        &state,
        &format!("you decided desk item {id}: {}", req.decision),
    );
    // A push the owner approved (ADR 0115): the node pushes the branch HQ named, now, and
    // tells HQ what came of it.
    if answer.0["push"].is_object() {
        let pushing = state.clone();
        let push = answer.0["push"].clone();
        let came =
            tokio::task::spawn_blocking(move || super::contracts::push_approved(&pushing, &push))
                .await
                .unwrap_or_else(|_| Err("the push failed".to_owned()));
        answer.0["pushed"] = json!(came.is_ok());
        answer.0["said"] = json!(came.unwrap_or_else(|why| why));
    }
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqHire {
    name: String,
    role: String,
    #[serde(default)]
    brain: Option<String>,
    #[serde(default)]
    reports_to: Option<i64>,
}

/// The owner hires a member (ADR 0115): HQ decides the rules (one CEO, a brain the role may
/// use); the node records who joined.
pub(super) async fn hq_hire(State(state): State<AppState>, Json(req): Json<HqHire>) -> Answer {
    let answer = ask(
        &state,
        "/v1/members".to_owned(),
        Some(json!({ "name": req.name, "role": req.role, "brain": req.brain, "reports_to": req.reports_to })),
    )
    .await?;
    record(
        &state,
        &format!(
            "you hired {} as {} (member {})",
            req.name.trim(),
            req.role.trim(),
            answer.0["member"]
        ),
    );
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqReply {
    text: String,
}

/// The owner replies on a task: HQ writes it in the thread and wakes whoever has the task.
pub(super) async fn hq_reply(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqReply>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/tasks/{id}/post"),
        Some(json!({ "text": req.text })),
    )
    .await?;
    record(&state, &format!("you replied on task {id}"));
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqNote {
    #[serde(default)]
    note: String,
}

/// The owner writes a goal's notes for the team, which every briefing under it carries.
pub(super) async fn hq_goal_note(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqNote>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/goals/{id}/note"),
        Some(json!({ "note": req.note })),
    )
    .await?;
    record(&state, &format!("you wrote the team's notes for goal {id}"));
    Ok(answer)
}

/// The owner writes a goal's strategy (ADR 0120): the parts given, passed on as they are; HQ
/// holds them to their limits.
pub(super) async fn hq_goal_strategy(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<serde_json::Value>,
) -> Answer {
    let mut parts = serde_json::Map::new();
    for key in ["purpose", "success", "measures", "bets", "not_now"] {
        if let Some(text) = req[key].as_str() {
            parts.insert(key.to_owned(), json!(text));
        }
    }
    let answer = ask(
        &state,
        format!("/v1/goals/{id}/strategy"),
        Some(serde_json::Value::Object(parts)),
    )
    .await?;
    record(&state, &format!("you wrote the strategy for goal {id}"));
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqBrain {
    brain: String,
}

/// The owner sets what a member thinks with (ADR 0109): the house's model, or a Claude its
/// role may use. HQ decides what the role may use; the node records what was set.
pub(super) async fn hq_set_brain(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqBrain>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/members/{id}/brain"),
        Some(json!({ "brain": req.brain })),
    )
    .await?;
    // HQ answers with one of its own brains' names, never what was asked.
    let brain = answer.0["brain"].as_str().unwrap_or("").to_owned();
    record(
        &state,
        &format!("you set member {id} to think with {brain}"),
    );
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqWorkBrain {
    work_brain: String,
}

/// The owner sets what a member does its work with (ADR 0131): the Mac or the house's model.
/// HQ decides what is allowed; the node records what was set.
pub(super) async fn hq_set_work_brain(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqWorkBrain>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/members/{id}/work-brain"),
        Some(json!({ "work_brain": req.work_brain })),
    )
    .await?;
    // HQ answers with one of its own brains' names, never what was asked.
    let brain = answer.0["work_brain"].as_str().unwrap_or("").to_owned();
    record(&state, &format!("you set member {id} to work with {brain}"));
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqJudgmentBrain {
    judgment_brain: String,
}

/// The owner sets the Claude a member judges with (ADR 0131), or `none` to take it away.
pub(super) async fn hq_set_judgment_brain(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqJudgmentBrain>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/members/{id}/judgment-brain"),
        Some(json!({ "judgment_brain": req.judgment_brain })),
    )
    .await?;
    let brain = answer.0["judgment_brain"]
        .as_str()
        .unwrap_or("no one")
        .to_owned();
    record(
        &state,
        &format!("you set member {id} to judge with {brain}"),
    );
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqRuntime {
    runtime: String,
}

/// The owner sets what runs a member's turns (ADR 0117): Endora's worker, or Claude Code for a
/// developer on Claude. HQ decides who may; the node records what was set.
pub(super) async fn hq_set_runtime(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqRuntime>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/members/{id}/runtime"),
        Some(json!({ "runtime": req.runtime })),
    )
    .await?;
    let runtime = answer.0["runtime"].as_str().unwrap_or("").to_owned();
    record(&state, &format!("you set member {id} to run on {runtime}"));
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqName {
    name: String,
}

/// The owner renames a member (the CEO HQ seated as Mara, say). HQ keeps names apart; the node
/// records the name HQ kept.
pub(super) async fn hq_rename(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqName>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/members/{id}/name"),
        Some(json!({ "name": req.name })),
    )
    .await?;
    let name = answer.0["name"].as_str().unwrap_or("").to_owned();
    record(&state, &format!("you renamed member {id} to {name}"));
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqHouseLog {
    #[serde(default)]
    log: String,
}

/// The owner rewrites the house log, which the CEO keeps and the CEO and the lead read every
/// turn (ADR 0111).
pub(super) async fn hq_house_log(
    State(state): State<AppState>,
    Json(req): Json<HqHouseLog>,
) -> Answer {
    let answer = ask(
        &state,
        "/v1/house".to_owned(),
        Some(json!({ "log": req.log })),
    )
    .await?;
    record(&state, "you rewrote the house log");
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqWhy {
    #[serde(default)]
    why: String,
}

/// The kill switch: every turn stops, and every wake is turned away until the owner resumes.
/// (When the gateway exists, the switch closes it to the team too.)
pub(super) async fn hq_stop(State(state): State<AppState>, Json(req): Json<HqWhy>) -> Answer {
    let answer = ask(
        &state,
        "/v1/stop".to_owned(),
        Some(json!({ "why": req.why })),
    )
    .await?;
    record(&state, "you pulled the kill switch");
    Ok(answer)
}

/// The owner lets the team work again.
pub(super) async fn hq_resume(State(state): State<AppState>) -> Answer {
    let answer = ask(&state, "/v1/resume".to_owned(), Some(json!({}))).await?;
    record(&state, "you let the team work again");
    Ok(answer)
}

/// The owner makes a routine (ADR 0107): work that comes round again, as a task for one
/// member.
pub(super) async fn hq_add_routine(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Answer {
    let title = req["title"].as_str().unwrap_or_default().trim().to_owned();
    let answer = ask(&state, "/v1/routines".to_owned(), Some(req)).await?;
    record(&state, &format!("you made a routine: {title}"));
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqSwitch {
    enabled: bool,
}

/// The owner switches a routine on or off.
pub(super) async fn hq_switch_routine(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqSwitch>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/routines/{id}"),
        Some(json!({ "enabled": req.enabled })),
    )
    .await?;
    let now = if req.enabled { "on" } else { "off" };
    record(&state, &format!("you switched routine {id} {now}"));
    Ok(answer)
}

/// The owner runs a routine now.
pub(super) async fn hq_fire_routine(State(state): State<AppState>, Path(id): Path<i64>) -> Answer {
    let answer = ask(&state, format!("/v1/routines/{id}/fire"), Some(json!({}))).await?;
    record(&state, &format!("you ran routine {id} now"));
    Ok(answer)
}

/// The owner makes a project (ADR 0113): HQ checks it and its cap.
pub(super) async fn hq_add_project(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Answer {
    let title = req["title"].as_str().unwrap_or_default().trim().to_owned();
    let answer = ask(&state, "/v1/projects".to_owned(), Some(req)).await?;
    record(&state, &format!("you made a project: {title}"));
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqProjectAction {
    action: String,
}

/// The owner pauses, resumes, drops, reopens or finishes a project.
pub(super) async fn hq_change_project(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqProjectAction>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/projects/{id}"),
        Some(json!({ "action": req.action })),
    )
    .await?;
    // HQ answers with one of its own statuses, never what was asked.
    let now = answer.0["status"].as_str().unwrap_or("").to_owned();
    record(&state, &format!("you set project {id} {now}"));
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqCap {
    #[serde(default)]
    cap: Option<serde_json::Value>,
}

/// The owner sets, raises or takes away a project's monthly cap.
pub(super) async fn hq_project_cap(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqCap>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/projects/{id}/cap"),
        Some(json!({ "cap": req.cap })),
    )
    .await?;
    let cap = answer.0["cap"].as_i64();
    record(
        &state,
        &match cap {
            Some(micros) => format!(
                "you capped project {id} at ${}.{:02} a month",
                micros / 1_000_000,
                (micros % 1_000_000) / 10_000
            ),
            None => format!("you took project {id}'s cap away"),
        },
    );
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqReportsTo {
    to: i64,
}

/// The owner sets whom a member reports to; HQ refuses itself and circles.
pub(super) async fn hq_reports_to(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqReportsTo>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/members/{id}/reports_to"),
        Some(json!({ "to": req.to })),
    )
    .await?;
    record(
        &state,
        &format!("you set member {id} to report to member {}", req.to),
    );
    Ok(answer)
}

/// The owner puts a member on furlough (ADR 0118); HQ refuses the CEO and a member with open
/// work.
pub(super) async fn hq_furlough(State(state): State<AppState>, Path(id): Path<i64>) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/members/{id}/furlough"),
        Some(json!({})),
    )
    .await?;
    record(&state, &format!("you put member {id} on furlough"));
    Ok(answer)
}

/// The owner brings a furloughed member back to work.
pub(super) async fn hq_recall(State(state): State<AppState>, Path(id): Path<i64>) -> Answer {
    let answer = ask(&state, format!("/v1/members/{id}/recall"), Some(json!({}))).await?;
    record(
        &state,
        &format!("you brought member {id} back from furlough"),
    );
    Ok(answer)
}

/// The owner undoes the last change to the office (ADR 0119).
pub(super) async fn hq_office_undo(State(state): State<AppState>) -> Answer {
    let answer = ask(&state, "/v1/office/undo".to_owned(), Some(json!({}))).await?;
    record(&state, "you undid the last change to the office");
    Ok(answer)
}

/// The owner puts the office back as it began.
pub(super) async fn hq_office_reset(State(state): State<AppState>) -> Answer {
    let answer = ask(&state, "/v1/office/reset".to_owned(), Some(json!({}))).await?;
    record(&state, "you reset the office");
    Ok(answer)
}

/// The owner vetoes an open proposal for the office.
pub(super) async fn hq_office_veto(State(state): State<AppState>, Path(id): Path<i64>) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/office/proposals/{id}/veto"),
        Some(json!({})),
    )
    .await?;
    record(&state, &format!("you vetoed office proposal {id}"));
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqDowntime {
    cents: i64,
}

/// The owner says what paid members' downtime turns may spend together in a day, in cents.
pub(super) async fn hq_office_downtime(
    State(state): State<AppState>,
    Json(req): Json<HqDowntime>,
) -> Answer {
    let answer = ask(
        &state,
        "/v1/office/downtime".to_owned(),
        Some(json!({ "cents": req.cents })),
    )
    .await?;
    record(
        &state,
        &format!(
            "you set paid members' downtime to {} cents a day",
            req.cents
        ),
    );
    Ok(answer)
}

#[derive(Deserialize)]
pub(super) struct HqPoolTopUp {
    amount_micros: i64,
}

/// The owner tops today's pool up (ADR 0113 rule 6): audited, with the amount.
pub(super) async fn hq_pool_top_up(
    State(state): State<AppState>,
    Json(req): Json<HqPoolTopUp>,
) -> Answer {
    let answer = ask(
        &state,
        "/v1/pool/top-up".to_owned(),
        Some(json!({ "amount_micros": req.amount_micros })),
    )
    .await?;
    record(
        &state,
        &format!(
            "you topped the pool up by ${}.{:02} for today",
            req.amount_micros / 1_000_000,
            req.amount_micros % 1_000_000 / 10_000
        ),
    );
    Ok(answer)
}

/// A member's notebook, for the owner to read (ADR 0118).
pub(super) async fn hq_notebook(State(state): State<AppState>, Path(id): Path<i64>) -> Answer {
    ask(&state, format!("/v1/members/{id}/notebook"), None).await
}

#[derive(Deserialize)]
pub(super) struct HqMemberNote {
    text: String,
}

/// The owner adds a note to a member's notebook: no evidence, and it says it is theirs.
pub(super) async fn hq_note_add(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqMemberNote>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/members/{id}/notebook"),
        Some(json!({ "text": req.text })),
    )
    .await?;
    record(
        &state,
        &format!("you added a note to member {id}'s notebook"),
    );
    Ok(answer)
}

/// The owner rewrites a note in a member's notebook.
pub(super) async fn hq_note_edit(
    State(state): State<AppState>,
    Path((id, note)): Path<(i64, i64)>,
    Json(req): Json<HqMemberNote>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/members/{id}/notebook/{note}"),
        Some(json!({ "text": req.text })),
    )
    .await?;
    record(
        &state,
        &format!("you edited note {note} in member {id}'s notebook"),
    );
    Ok(answer)
}

/// The owner deletes a note from a member's notebook (HQ takes it as a POST: its client has
/// no DELETE).
pub(super) async fn hq_note_delete(
    State(state): State<AppState>,
    Path((id, note)): Path<(i64, i64)>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/members/{id}/notebook/{note}/delete"),
        Some(json!({})),
    )
    .await?;
    record(
        &state,
        &format!("you deleted note {note} from member {id}'s notebook"),
    );
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::super::relay::tests::{fake, node};
    use super::*;

    #[tokio::test]
    async fn a_replay_is_read_from_hq_as_it_answers_and_records_nothing() {
        let (url, seen) = fake(Arc::new(|path, _| {
            (
                200,
                "application/json",
                json!({ "asked": path }).to_string(),
            )
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let run = hq_run_replay(State(state.clone()), Path(1599))
            .await
            .unwrap();
        let task = hq_task_replay(State(state.clone()), Path(7)).await.unwrap();
        assert_eq!(run.0, json!({ "asked": "/v1/runs/1599/replay" }));
        assert_eq!(task.0, json!({ "asked": "/v1/tasks/7/replay" }));
        let paths: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, _)| p.clone())
            .collect();
        assert_eq!(paths, ["/v1/runs/1599/replay", "/v1/tasks/7/replay"]);
        let audit = endora_application::AuditLog::recent(state.audit.as_ref(), 10).unwrap();
        assert!(audit.is_empty(), "reading is not an action");
    }

    #[tokio::test]
    async fn the_owners_reply_and_a_goals_notes_pass_through_signed_and_are_recorded() {
        let (url, seen) = fake(Arc::new(|_, _| {
            (200, "application/json", json!({ "ok": true }).to_string())
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let _ = hq_reply(
            State(state.clone()),
            Path(7),
            Json(HqReply {
                text: "Leave the old player alone.".to_owned(),
            }),
        )
        .await
        .unwrap();
        let _ = hq_goal_note(
            State(state.clone()),
            Path(1),
            Json(HqNote {
                note: "The SSD is the owner's.".to_owned(),
            }),
        )
        .await
        .unwrap();
        // A goal's strategy: only its parts pass, whatever else the screen sent.
        let _ = hq_goal_strategy(
            State(state.clone()),
            Path(1),
            Json(json!({ "purpose": "The house runs itself.", "by": "someone else" })),
        )
        .await
        .unwrap();
        let sent: Vec<(String, serde_json::Value)> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, b)| (p.clone(), serde_json::from_str(b).unwrap()))
            .collect();
        assert_eq!(
            sent,
            [
                (
                    "/v1/tasks/7/post".to_owned(),
                    json!({ "text": "Leave the old player alone." })
                ),
                (
                    "/v1/goals/1/note".to_owned(),
                    json!({ "note": "The SSD is the owner's." })
                ),
                (
                    "/v1/goals/1/strategy".to_owned(),
                    json!({ "purpose": "The house runs itself." })
                ),
            ]
        );
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        for line in [
            "HQ: you replied on task 7",
            "HQ: you wrote the team's notes for goal 1",
            "HQ: you wrote the strategy for goal 1",
        ] {
            assert!(audit.iter().any(|a| a == line), "{line}: {audit:?}");
        }
    }

    #[tokio::test]
    async fn the_owner_renames_a_member_and_rewrites_the_house_log_through_the_node() {
        let (url, seen) = fake(Arc::new(|path, _| {
            if path.ends_with("/name") {
                return (
                    200,
                    "application/json",
                    json!({ "member": 5, "name": "Jane Doe" }).to_string(),
                );
            }
            (200, "application/json", json!({ "kept": true }).to_string())
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let renamed = hq_rename(
            State(state.clone()),
            Path(5),
            Json(HqName {
                name: "Jane Doe".to_owned(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(renamed.0["name"], "Jane Doe");
        let _ = hq_house_log(
            State(state.clone()),
            Json(HqHouseLog {
                log: "- 17 Oct: the NAS has 16 GB.".to_owned(),
            }),
        )
        .await
        .unwrap();
        let sent: Vec<(String, serde_json::Value)> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, b)| (p.clone(), serde_json::from_str(b).unwrap()))
            .collect();
        assert_eq!(
            sent,
            [
                (
                    "/v1/members/5/name".to_owned(),
                    json!({ "name": "Jane Doe" })
                ),
                (
                    "/v1/house".to_owned(),
                    json!({ "log": "- 17 Oct: the NAS has 16 GB." })
                ),
            ]
        );
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        for line in [
            "HQ: you renamed member 5 to Jane Doe",
            "HQ: you rewrote the house log",
        ] {
            assert!(audit.iter().any(|a| a == line), "{line}: {audit:?}");
        }
    }

    #[tokio::test]
    async fn the_owner_sets_a_members_runtime_through_the_node_and_it_is_recorded() {
        let (url, seen) = fake(Arc::new(|_, body| {
            if body.contains("lead") {
                return (
                    400,
                    "application/json",
                    json!({ "error": "no" }).to_string(),
                );
            }
            (
                200,
                "application/json",
                json!({ "member": 5, "runtime": "claude-code" }).to_string(),
            )
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let answer = hq_set_runtime(
            State(state.clone()),
            Path(5),
            Json(HqRuntime {
                runtime: "claude-code".to_owned(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(answer.0["runtime"], "claude-code");
        let sent = seen.lock().unwrap()[0].clone();
        assert_eq!(sent.0, "/v1/members/5/runtime");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&sent.2).unwrap(),
            json!({ "runtime": "claude-code" })
        );
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        assert!(
            audit
                .iter()
                .any(|a| a == "HQ: you set member 5 to run on claude-code"),
            "{audit:?}"
        );
    }

    #[tokio::test]
    async fn the_owner_sets_both_of_a_members_brains_through_the_node_and_both_are_recorded() {
        let (url, seen) = fake(Arc::new(|path, _| {
            if path.ends_with("/work-brain") {
                return (
                    200,
                    "application/json",
                    json!({ "member": 1, "work_brain": "mac" }).to_string(),
                );
            }
            (
                200,
                "application/json",
                json!({ "member": 1, "judgment_brain": null }).to_string(),
            )
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let work = hq_set_work_brain(
            State(state.clone()),
            Path(1),
            Json(HqWorkBrain {
                work_brain: "mac".to_owned(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(work.0["work_brain"], "mac");
        let judges = hq_set_judgment_brain(
            State(state.clone()),
            Path(1),
            Json(HqJudgmentBrain {
                judgment_brain: "none".to_owned(),
            }),
        )
        .await
        .unwrap();
        assert!(judges.0["judgment_brain"].is_null());
        let paths: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, _)| p.clone())
            .collect();
        assert_eq!(
            paths,
            ["/v1/members/1/work-brain", "/v1/members/1/judgment-brain"]
        );
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        assert!(
            audit.contains(&"HQ: you set member 1 to work with mac".to_owned()),
            "{audit:?}"
        );
        assert!(
            audit.contains(&"HQ: you set member 1 to judge with no one".to_owned()),
            "{audit:?}"
        );
    }

    #[tokio::test]
    async fn the_owner_sets_a_members_brain_through_the_node_and_it_is_recorded() {
        let (url, seen) = fake(Arc::new(|_, body| {
            if body.contains("opus") {
                return (
                    400,
                    "application/json",
                    json!({ "error": "a lead thinks with local, claude-sonnet-5-5, claude-haiku-5-5 only" })
                        .to_string(),
                );
            }
            (
                200,
                "application/json",
                json!({ "member": 1, "brain": "claude-sonnet-5-5" }).to_string(),
            )
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let set = |brain: &str| {
            hq_set_brain(
                State(state.clone()),
                Path(1),
                Json(HqBrain {
                    brain: brain.to_owned(),
                }),
            )
        };
        let answer = set("claude-sonnet-5-5").await.unwrap();
        assert_eq!(answer.0["brain"], "claude-sonnet-5-5");
        let refused = set("claude-opus-5-5").await.unwrap_err();
        assert_eq!(
            refused.0,
            StatusCode::BAD_REQUEST,
            "HQ's refusal, passed on"
        );
        let sent: Vec<(String, serde_json::Value)> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, b)| (p.clone(), serde_json::from_str(b).unwrap()))
            .collect();
        assert_eq!(
            sent[0],
            (
                "/v1/members/1/brain".to_owned(),
                json!({ "brain": "claude-sonnet-5-5" })
            )
        );
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        assert!(
            audit
                .iter()
                .any(|a| a == "HQ: you set member 1 to think with claude-sonnet-5-5"),
            "{audit:?}"
        );
        assert_eq!(
            audit.len(),
            1,
            "a refusal changes nothing, and is not recorded as done"
        );
    }

    #[tokio::test]
    async fn the_owners_routines_pass_through_signed_and_are_recorded() {
        let (url, seen) = fake(Arc::new(|path, _| {
            let body = if path.ends_with("/fire") {
                json!({ "fired": "created", "task": 4 })
            } else {
                json!({ "routine": 2 })
            };
            (200, "application/json", body.to_string())
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let made = json!({ "title": "Check the home lab", "goal": 1, "assignee": 1,
                           "every_ms": 21_600_000 });
        let answer = hq_add_routine(State(state.clone()), Json(made.clone()))
            .await
            .unwrap();
        assert_eq!(answer.0["routine"], 2);
        let switched = hq_switch_routine(
            State(state.clone()),
            Path(2),
            Json(HqSwitch { enabled: false }),
        )
        .await
        .unwrap();
        assert_eq!(switched.0["routine"], 2);
        let fired = hq_fire_routine(State(state.clone()), Path(2))
            .await
            .unwrap();
        assert_eq!(fired.0["fired"], "created");
        let sent: Vec<(String, String)> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, b)| (p.clone(), b.clone()))
            .collect();
        assert_eq!(sent[0].0, "/v1/routines");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&sent[0].1).unwrap(),
            made
        );
        assert_eq!(sent[1].0, "/v1/routines/2");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&sent[1].1).unwrap(),
            json!({ "enabled": false })
        );
        assert_eq!(sent[2].0, "/v1/routines/2/fire");
        let signed = seen.lock().unwrap()[0]
            .1
            .iter()
            .any(|(k, v)| k == "authorization" && v == &format!("Bearer {}", "n".repeat(32)));
        assert!(signed, "signed with HQ's token");
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        for line in [
            "HQ: you made a routine: Check the home lab",
            "HQ: you switched routine 2 off",
            "HQ: you ran routine 2 now",
        ] {
            assert!(audit.iter().any(|a| a == line), "{line}: {audit:?}");
        }
    }

    #[tokio::test]
    async fn the_owner_furloughs_and_recalls_through_the_node_and_it_is_recorded() {
        let (url, seen) = fake(Arc::new(|path, _| {
            let furloughed = path.ends_with("/furlough");
            (
                200,
                "application/json",
                json!({ "member": 5, "furloughed": furloughed }).to_string(),
            )
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let answer = hq_furlough(State(state.clone()), Path(5)).await.unwrap();
        assert_eq!(answer.0["furloughed"], true);
        let answer = hq_recall(State(state.clone()), Path(5)).await.unwrap();
        assert_eq!(answer.0["furloughed"], false);
        let sent: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, _)| p.clone())
            .collect();
        assert_eq!(sent, ["/v1/members/5/furlough", "/v1/members/5/recall"]);
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        for line in [
            "HQ: you put member 5 on furlough",
            "HQ: you brought member 5 back from furlough",
        ] {
            assert!(audit.iter().any(|a| a == line), "{line}: {audit:?}");
        }
    }

    #[tokio::test]
    async fn the_owner_undoes_resets_vetoes_and_sets_downtime_through_the_node_and_it_is_recorded()
    {
        let (url, seen) = fake(Arc::new(|_, _| {
            (200, "application/json", json!({}).to_string())
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let _ = hq_office_undo(State(state.clone())).await.unwrap();
        let _ = hq_office_reset(State(state.clone())).await.unwrap();
        let _ = hq_office_veto(State(state.clone()), Path(4)).await.unwrap();
        let _ = hq_office_downtime(State(state.clone()), Json(HqDowntime { cents: 25 }))
            .await
            .unwrap();
        let sent: Vec<(String, String)> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, b)| (p.clone(), b.clone()))
            .collect();
        assert_eq!(
            sent.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            [
                "/v1/office/undo",
                "/v1/office/reset",
                "/v1/office/proposals/4/veto",
                "/v1/office/downtime"
            ]
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&sent[3].1).unwrap(),
            json!({ "cents": 25 })
        );
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        for line in [
            "HQ: you undid the last change to the office",
            "HQ: you reset the office",
            "HQ: you vetoed office proposal 4",
            "HQ: you set paid members' downtime to 25 cents a day",
        ] {
            assert!(audit.iter().any(|a| a == line), "{line}: {audit:?}");
        }
    }

    #[tokio::test]
    async fn the_owner_tops_the_pool_up_through_the_node_and_it_is_recorded() {
        let (url, seen) = fake(Arc::new(|_, _| {
            (200, "application/json", json!({}).to_string())
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let _ = hq_pool_top_up(
            State(state.clone()),
            Json(HqPoolTopUp {
                amount_micros: 2_333_333,
            }),
        )
        .await
        .unwrap();
        let sent: Vec<(String, String)> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, b)| (p.clone(), b.clone()))
            .collect();
        assert_eq!(sent[0].0, "/v1/pool/top-up");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&sent[0].1).unwrap(),
            json!({ "amount_micros": 2_333_333 })
        );
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        assert!(
            audit
                .iter()
                .any(|a| a == "HQ: you topped the pool up by $2.33 for today"),
            "{audit:?}"
        );
    }

    #[tokio::test]
    async fn the_owner_reads_and_writes_a_members_notebook_through_the_node_and_it_is_recorded() {
        let (url, seen) = fake(Arc::new(|path, _| {
            let body = if path.ends_with("/notebook") {
                json!({ "member": 5, "notes": [] })
            } else {
                json!({ "member": 5, "note": 2 })
            };
            (200, "application/json", body.to_string())
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let answer = hq_notebook(State(state.clone()), Path(5)).await.unwrap();
        assert_eq!(answer.0["notes"], json!([]));
        let text = |t: &str| Json(HqMemberNote { text: t.to_owned() });
        let _ = hq_note_add(State(state.clone()), Path(5), text("Never at night."))
            .await
            .unwrap();
        let _ = hq_note_edit(
            State(state.clone()),
            Path((5, 2)),
            text("Never at night, ever."),
        )
        .await
        .unwrap();
        let _ = hq_note_delete(State(state.clone()), Path((5, 2)))
            .await
            .unwrap();
        let sent: Vec<(String, String)> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, b)| (p.clone(), b.clone()))
            .collect();
        assert_eq!(
            sent.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            [
                "/v1/members/5/notebook",
                "/v1/members/5/notebook",
                "/v1/members/5/notebook/2",
                "/v1/members/5/notebook/2/delete"
            ]
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&sent[1].1).unwrap(),
            json!({ "text": "Never at night." })
        );
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        for line in [
            "HQ: you added a note to member 5's notebook",
            "HQ: you edited note 2 in member 5's notebook",
            "HQ: you deleted note 2 from member 5's notebook",
        ] {
            assert!(audit.iter().any(|a| a == line), "{line}: {audit:?}");
        }
    }

    #[tokio::test]
    async fn the_owners_projects_and_reports_pass_through_signed_and_are_recorded() {
        let (url, seen) = fake(Arc::new(|path, _| {
            let body = if path.ends_with("/cap") {
                json!({ "project": 3, "cap": 12_500_000 })
            } else if path.ends_with("/reports_to") {
                json!({ "member": 5, "reports_to": 2 })
            } else if path == "/v1/projects" {
                json!({ "project": 3 })
            } else {
                json!({ "project": 3, "status": "paused" })
            };
            (200, "application/json", body.to_string())
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let made = json!({ "goal": 1, "title": "Upkeep", "lead": 2, "cap": 10 });
        let answer = hq_add_project(State(state.clone()), Json(made.clone()))
            .await
            .unwrap();
        assert_eq!(answer.0["project"], 3);
        let answer = hq_change_project(
            State(state.clone()),
            Path(3),
            Json(HqProjectAction {
                action: "pause".to_owned(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(answer.0["status"], "paused");
        let answer = hq_project_cap(
            State(state.clone()),
            Path(3),
            Json(HqCap {
                cap: Some(json!("12.50")),
            }),
        )
        .await
        .unwrap();
        assert_eq!(answer.0["cap"], 12_500_000);
        let answer = hq_reports_to(State(state.clone()), Path(5), Json(HqReportsTo { to: 2 }))
            .await
            .unwrap();
        assert_eq!(answer.0["reports_to"], 2);
        let sent: Vec<(String, serde_json::Value)> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, b)| (p.clone(), serde_json::from_str(b).unwrap()))
            .collect();
        assert_eq!(
            sent,
            [
                ("/v1/projects".to_owned(), made),
                ("/v1/projects/3".to_owned(), json!({ "action": "pause" })),
                ("/v1/projects/3/cap".to_owned(), json!({ "cap": "12.50" })),
                ("/v1/members/5/reports_to".to_owned(), json!({ "to": 2 })),
            ]
        );
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        for line in [
            "HQ: you made a project: Upkeep",
            "HQ: you set project 3 paused",
            "HQ: you capped project 3 at $12.50 a month",
            "HQ: you set member 5 to report to member 2",
        ] {
            assert!(audit.iter().any(|a| a == line), "{line}: {audit:?}");
        }
    }
}
