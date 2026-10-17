//! Endora HQ through the node (ADR 0102). The console talks to the node only; the node signs
//! every request to HQ, and writes every action of the owner's into Endora's own audit log,
//! which is the record. HQ's own history is for display.

use super::*;
use endora_infrastructure::HqError;

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
}

/// The events after an id: the office's stream, resumed from the last id it saw.
pub(super) async fn hq_events(State(state): State<AppState>, Query(q): Query<HqAfter>) -> Answer {
    ask(&state, format!("/v1/events?after={}", q.after), None).await
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
    let answer = ask(
        &state,
        format!("/v1/desk/{id}"),
        Some(json!({ "decision": req.decision, "answer": req.answer })),
    )
    .await?;
    record(
        &state,
        &format!("you decided desk item {id}: {}", req.decision),
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

#[cfg(test)]
mod tests {
    use super::super::relay::tests::{fake, node};
    use super::*;

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
}
