use super::*;

/// Serializes a belief for the console. `confidence` is the **decayed** value — how
/// sure Endora is right now, given how long since anything reinforced it (ADR 0052) —
/// not the value frozen at the moment it was formed. Showing the stored one would
/// present a year-old guess as current.
pub(super) fn belief_json(b: &Belief, now: endora_application::Timestamp) -> serde_json::Value {
    belief_json_contested(b, now, None)
}

/// As [`belief_json`], but told whether this belief **contradicts another one Endora
/// holds** — which is not a property of a belief on its own, so it cannot be answered in
/// the domain (ADR 0052).
///
/// Contradiction defeats settledness. Two beliefs, both high-confidence, both repeatedly
/// affirmed, saying opposite things — observed live, Fahrenheit against Celsius — is the
/// one situation where a confirmation prompt has an obvious payoff: Endora is definitely
/// wrong about one of them, and only the person knows which. Suppressing the question
/// there would be the settledness rule eating the case it should care most about.
pub(super) fn belief_json_contested(
    b: &Belief,
    now: endora_application::Timestamp,
    contradicts: Option<&str>,
) -> serde_json::Value {
    let confidence = b.confidence_at(now).unwrap_or(b.confidence());
    let mut value = json!({
        "id": b.id().value().to_string(),
        "statement": b.statement(),
        "kind": b.kind().name(),
        "confidence": confidence.name(),
        "evidence": b.evidence(),
        "last_affirmed_ms": b.last_affirmed_at().unix_millis(),
        "settled": b.is_settled(now) && contradicts.is_none(),
    });
    if let Some(other) = contradicts {
        value["contradicts"] = json!(other);
    }
    value
}

/// Serializes a belief **exactly as stored**, for the export. The memory right is to
/// see what Endora actually holds, so the export must not reinterpret it: it reports
/// the recorded confidence, where the live view reports the decayed one.
pub(super) fn stored_belief_json(b: &Belief) -> serde_json::Value {
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
pub(super) async fn list_understanding(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let understanding = state.understanding.clone();
    let clock = state.clock.clone();
    let now = clock.now();
    let items =
        blocking(move || usecases::understanding(understanding.as_ref(), clock.as_ref())).await?;
    // Endora holding two opposite beliefs means it is wrong about one of them, which is
    // the most useful thing understanding can say — and which one is the person's call,
    // not the butler's (ADR 0052). Shown on both cards, so neither is quietly the winner.
    Ok(Json(
        items
            .iter()
            .map(|b| {
                let against = items
                    .iter()
                    .find(|o| {
                        o.id() != b.id()
                            && usecases::statements_disagree(o.statement(), b.statement())
                    })
                    .map(Belief::statement);
                belief_json_contested(b, now, against)
            })
            .collect(),
    ))
}

pub(super) fn parse_belief_id(id: &str) -> Result<BeliefId, ApiError> {
    id.parse::<u128>()
        .map(BeliefId::new)
        .map_err(|_| ApiError(AppError::NotFound { entity: "belief" }))
}

/// The person confirms a belief is right — raise its confidence.
pub(super) async fn affirm_belief(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let bid = parse_belief_id(&id)?;
    let understanding = state.understanding.clone();
    let clock = state.clock.clone();
    let now = clock.now();
    let b = blocking(move || usecases::affirm_belief(understanding.as_ref(), clock.as_ref(), bid))
        .await?;
    Ok(Json(belief_json(&b, now)))
}

/// The person says a belief is wrong — drop it from understanding.
pub(super) async fn correct_belief(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let bid = parse_belief_id(&id)?;
    let understanding = state.understanding.clone();
    blocking(move || usecases::correct_belief(understanding.as_ref(), bid)).await?;
    Ok(Json(json!({ "corrected": true })))
}

#[derive(Deserialize)]
pub(super) struct ForgetRequest {
    #[serde(default)]
    confirm: bool,
}

/// Deletes everything Endora learned about the person, keeping what they told it themselves
/// (their preferences, where they are, how to address them). The owner's "start fresh",
/// 2026-10-16. Needs `{"confirm": true}`, like the full purge.
pub(super) async fn forget_what_it_learned(
    State(state): State<AppState>,
    Json(req): Json<ForgetRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !req.confirm {
        return Err(ApiError(AppError::BadRequest {
            message: r#"send {"confirm": true} to delete everything Endora learned"#.to_owned(),
        }));
    }
    let understanding = state.understanding.clone();
    let forgot = blocking(move || {
        endora_application::BeliefRepository::forget_all(understanding.as_ref())
            .map_err(AppError::Repository)
    })
    .await?;
    Ok(Json(json!({ "forgot": forgot })))
}

pub(super) fn intention_json(i: &endora_application::Intention) -> serde_json::Value {
    json!({
        "id": i.id().value().to_string(),
        "statement": i.statement(),
        // User-request origins have no motivating belief; belief-origin intentions do.
        "motivating_belief": i.motivating_belief().map(|id| id.value().to_string()),
        "note": i.note(),
        "state": i.state().name(),
        "active": i.is_active(),
        "steps_taken": i.steps_taken(),
        "created_ms": i.created_at().unix_millis(),
        "last_progressed_ms": i.last_progressed_at().unix_millis(),
    })
}

pub(super) fn goal_json(record: &endora_application::GoalRecord) -> serde_json::Value {
    let goal = record.goal();
    json!({
        "request_key": record.request_key(),
        "objective": goal.objective(),
        "status": goal.status().name(),
        // Which lane it runs in (ADR 0094): `person` for what was asked for, `record` for what
        // Endora started itself. Additive, so existing clients are unaffected.
        "origin": record.origin().name(),
        "phase": goal.phase(),
        "next_step": goal.next_step(),
        "reason": goal.reason(),
        // The words the chat uses for how it ended, so the console shows the same text.
        // Only for an ended goal: before then there is no outcome to describe.
        "outcome": goal.status().terminal().then(|| goal.outcome_notice()),
        "criteria": goal.criteria().map(endora_application::goal_request::GoalCriteria::to_json),
        "evidence": goal.evidence().iter().map(|e| json!({"step": e.step, "output": e.output})).collect::<Vec<_>>(),
        "observations": goal.observations().iter().map(|o| json!({"tool":o.tool,"input":o.input,"output":o.output,"data":o.data,"error":o.error,"retrieved_ms":o.at.unix_millis()})).collect::<Vec<_>>(),
        "created_ms": record.created_at().unix_millis(),
        "updated_ms": record.updated_at().unix_millis(),
    })
}

pub(super) async fn list_goals(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let goals = blocking(move || store.active_goals().map_err(AppError::from)).await?;
    Ok(Json(json!({
        "items": goals.iter().map(goal_json).collect::<Vec<_>>(),
    })))
}

pub(super) async fn get_goal(
    State(state): State<AppState>,
    Path(request_key): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let goal = blocking(move || store.get_goal(&request_key).map_err(AppError::from)).await?;
    let Some(goal) = goal else {
        return Err(ApiError(AppError::NotFound { entity: "goal" }));
    };
    Ok(Json(goal_json(&goal)))
}

#[derive(Deserialize)]
pub(super) struct CancelGoalRequest {
    #[serde(default = "default_goal_cancel_reason")]
    reason: String,
}

pub(super) fn default_goal_cancel_reason() -> String {
    "cancelled by the person".to_owned()
}

pub(super) async fn cancel_goal(
    State(state): State<AppState>,
    Path(request_key): Path<String>,
    Json(req): Json<CancelGoalRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let clock = state.clock.clone();
    let key = request_key.clone();
    let changed = blocking(move || {
        store
            .cancel_goal(&key, &req.reason, clock.now())
            .map_err(AppError::from)
    })
    .await?;
    if !changed {
        return Err(ApiError(AppError::Conflict {
            code: "goal_not_active",
            message: "the goal is missing or already terminal".to_owned(),
        }));
    }
    let _ = state.changes.send(());
    let store = state.store.clone();
    let goal = blocking(move || store.get_goal(&request_key).map_err(AppError::from)).await?;
    let Some(goal) = goal else {
        return Err(ApiError(AppError::NotFound { entity: "goal" }));
    };
    Ok(Json(goal_json(&goal)))
}

/// What Endora is pursuing, and what it has pursued before (ADR 0052).
pub(super) async fn list_intentions(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let understanding = state.understanding.clone();
    let items = blocking(move || usecases::intentions(understanding.as_ref())).await?;
    Ok(Json(
        items
            .iter()
            .filter(|item| item.motivating_belief().is_some())
            .map(intention_json)
            .collect(),
    ))
}

/// The person tells Endora to stop working on something — their whole authority over
/// an intention, and the only verb they have (ADR 0052).
pub(super) async fn drop_intention(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let iid = id
        .parse::<u128>()
        .map(endora_application::IntentionId::new)
        .map_err(|_| {
            ApiError(AppError::NotFound {
                entity: "intention",
            })
        })?;
    let understanding = state.understanding.clone();
    let out = blocking(move || usecases::drop_intention(understanding.as_ref(), iid)).await?;
    Ok(Json(intention_json(&out)))
}

#[derive(Deserialize)]
pub(super) struct CommissionResearchRequest {
    request_key: String,
    assignment: String,
    #[serde(default)]
    source_urls: Vec<String>,
    #[serde(default)]
    search_capability_id: Option<String>,
}

pub(super) fn research_capabilities(state: &AppState) -> (bool, Vec<String>) {
    let runner = build_reversible_only_runner(
        state.config.as_ref(),
        state.capabilities.clone(),
        mcp_snapshot(state),
        proven_now(state.understanding.as_ref()),
    );
    let mut search = Vec::new();
    let mut fetch = false;
    for spec in endora_application::CapabilityRunner::available(&runner) {
        if spec.id == "web_fetch" && spec.autonomous {
            fetch = true;
        }
        let brave_tool = spec.id == "web_search"
            || spec.id == "brave-search.brave_web_search"
            || spec.id == "brave-search.brave_news_search";
        if spec.autonomous && spec.third_party && brave_tool {
            search.push(spec.id);
        }
    }
    (fetch, search)
}

pub(super) async fn research_readiness(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (fetch, search) = research_capabilities(&state);
    let mut modes = Vec::new();
    if fetch {
        modes.push("sources");
    }
    if fetch && !search.is_empty() {
        modes.push("search");
    }
    Json(json!({
        "ready": fetch,
        "modes": modes,
        "search_tools": search.iter().map(|id| json!({"id":id,"name":id})).collect::<Vec<_>>(),
        "reason_code": if fetch { serde_json::Value::Null } else { json!("web_fetch_unavailable") },
    }))
}

pub(super) async fn commission_research(
    State(state): State<AppState>,
    Json(req): Json<CommissionResearchRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    if req.source_urls.len() > 5
        || req
            .source_urls
            .iter()
            .any(|u| u.len() > 2048 || !(u.starts_with("https://") || u.starts_with("http://")))
    {
        return Err(ApiError(AppError::BadRequest {
            message: "supply at most five http(s) source URLs".to_owned(),
        }));
    }
    let (fetch, search) = research_capabilities(&state);
    if !fetch || (req.source_urls.is_empty() && req.search_capability_id.is_none()) {
        return Err(ApiError(AppError::Conflict {
            code: "setup_required",
            message: "Connect web search or supply a source URL.".to_owned(),
        }));
    }
    if let Some(id) = req.search_capability_id.as_deref() {
        if !search.iter().any(|candidate| candidate == id) {
            return Err(ApiError(AppError::Conflict {
                code: "setup_required",
                message: "That search capability is not available for unattended research."
                    .to_owned(),
            }));
        }
    }
    let store = state.store.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let assignment = req.assignment;
    let key = req.request_key;
    let urls = serde_json::to_string(&req.source_urls).map_err(|_| {
        ApiError(AppError::BadRequest {
            message: "invalid source URLs".to_owned(),
        })
    })?;
    let search_id = req.search_capability_id;
    let created = blocking(move || {
        store
            .commission_research(
                endora_application::IntentionId::new(ids.new_id()),
                endora_application::MessageId::new(ids.new_id()),
                &key,
                &assignment,
                &urls,
                search_id.as_deref(),
                clock.now(),
            )
            .map_err(|e| match &e {
                RepositoryError::Backend(message)
                    if message.contains("active") || message.contains("exists") =>
                {
                    AppError::Conflict {
                        code: "occupied_slot",
                        message: message.clone(),
                    }
                }
                RepositoryError::Backend(message) if message.contains("request key") => {
                    AppError::Conflict {
                        code: "request_key_reused",
                        message: message.clone(),
                    }
                }
                _ => AppError::Repository(e),
            })
    })
    .await?;
    let _ = state.changes.send(());
    Ok((StatusCode::ACCEPTED, Json(research_json(&created))))
}

pub(super) fn research_json(work: &ResearchAssignment) -> serde_json::Value {
    let checkpoint = serde_json::from_str::<serde_json::Value>(work.checkpoint())
        .unwrap_or_else(|_| json!({"version": 1}));
    json!({
        "id": work.intention_id().value().to_string(), "assignment": work.assignment(),
        "request_key": work.request_key(), "checkpoint": checkpoint,
        "status": work.status().name(), "phase": work.phase(),
        "reason_code": work.reason_code(), "reason": work.reason(),
        "created_ms": work.created_at().unix_millis(), "updated_ms": work.updated_at().unix_millis(),
        "expires_ms": work.expires_at().unix_millis(),
        "budget": {"model_calls_used":work.model_calls_used(),"model_calls_limit":16,
                   "read_attempts_used":work.read_attempts_used(),"read_attempts_limit":16},
        "can_cancel": !work.status().terminal(), "report_available": work.report().is_some(),
        "partial": work.report().is_some_and(endora_application::ResearchReport::partial),
        "report_markdown": work.report().map(endora_application::ResearchReport::markdown),
        "report_path": work.report().map(|_| format!("/v1/research/{}/report", work.intention_id().value())),
    })
}

pub(super) async fn list_research(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let understanding = state.understanding.clone();
    let items = blocking(move || understanding.list_research().map_err(AppError::from)).await?;
    Ok(Json(
        json!({"items":items.iter().take(20).map(research_json).collect::<Vec<_>>() }),
    ))
}

pub(super) async fn get_research(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = id
        .parse::<u128>()
        .map(endora_application::IntentionId::new)
        .map_err(|_| ApiError(AppError::NotFound { entity: "research" }))?;
    let understanding = state.understanding.clone();
    let work = blocking(move || understanding.research(id).map_err(AppError::from))
        .await?
        .ok_or(ApiError(AppError::NotFound { entity: "research" }))?;
    Ok(Json(research_json(&work)))
}

pub(super) async fn cancel_research(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = id
        .parse::<u128>()
        .map(endora_application::IntentionId::new)
        .map_err(|_| ApiError(AppError::NotFound { entity: "research" }))?;
    let store = state.store.clone();
    let clock = state.clock.clone();
    let work = blocking(move || {
        store
            .cancel_research(id, clock.now())
            .map_err(|error| match &error {
                RepositoryError::Backend(message) if message.contains("not found") => {
                    AppError::NotFound { entity: "research" }
                }
                RepositoryError::Backend(message) if message.contains("terminal") => {
                    AppError::Conflict {
                        code: "already_terminal",
                        message: message.clone(),
                    }
                }
                _ => AppError::Repository(error),
            })
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(research_json(&work)))
}

pub(super) async fn research_report(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = id
        .parse::<u128>()
        .map(endora_application::IntentionId::new)
        .map_err(|_| ApiError(AppError::NotFound { entity: "research" }))?;
    let understanding = state.understanding.clone();
    let work = blocking(move || understanding.research(id).map_err(AppError::from))
        .await?
        .ok_or(ApiError(AppError::NotFound { entity: "research" }))?;
    let report = work.report().ok_or(ApiError(AppError::Conflict {
        code: "report_not_ready",
        message: "The report is not ready.".to_owned(),
    }))?;
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/markdown; charset=utf-8",
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"endora-research.md\"",
            ),
        ],
        report.markdown().to_owned(),
    )
        .into_response())
}

/// How many outcomes the console shows — recent history, not an archive to manage.
pub(super) const CONFIG_WRITES_SHOWN: usize = 50;
pub(super) const OUTCOMES_SHOWN: usize = 30;

pub(super) fn outcome_json(o: &endora_application::Outcome) -> serde_json::Value {
    json!({
        "id": o.id().value().to_string(),
        "capability": o.capability(),
        "input": o.input(),
        // The tool's own account and what Endora saw are kept apart here exactly as
        // they are in storage (ADR 0053) — the console must not merge them either.
        "claim": o.claim(),
        "observation": o.observation(),
        "observed": o.was_observed(),
        // Whether the world actually moved (ADR 0054). `null` means there was nothing
        // to compare — no reader, or the action never ran.
        "changed": o.changed(),
        "at_ms": o.at().unix_millis(),
        "motivating_belief": o.motivating_belief().map(|b| b.value().to_string()),
        "reaction": o.reaction().map(endora_application::Reaction::name),
    })
}

#[derive(serde::Deserialize)]
pub(super) struct AliasRequest {
    server: String,
    said: String,
    means: String,
}

/// What the person has told Endora its tools' targets are really called (ADR 0054).
pub(super) async fn list_aliases(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    use endora_capabilities::TargetAliasRepository;
    let config = state.config.clone();
    let found = blocking(move || config.aliases().map_err(AppError::Repository)).await?;
    Ok(Json(
        found
            .iter()
            .map(|a| json!({ "server": a.server, "said": a.said, "means": a.means }))
            .collect(),
    ))
}

/// The person answers what Endora asked: this target is really called that (ADR 0054).
///
/// The **confirmed** source in ADR 0054's ranking, and the only one policy trusts.
/// Endora never fills this in from a server's text — that is the per-integration
/// parsing 0038 exists to stop.
pub(super) async fn set_alias(
    State(state): State<AppState>,
    Json(req): Json<AliasRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::{TargetAlias, TargetAliasRepository};
    let alias = TargetAlias::new(&req.server, &req.said, &req.means)
        .map_err(|e| ApiError(AppError::Domain(e)))?;
    let config = state.config.clone();
    let stored = alias.clone();
    blocking(move || config.set_alias(&stored).map_err(AppError::Repository)).await?;
    Ok(Json(
        json!({ "server": alias.server, "said": alias.said, "means": alias.means }),
    ))
}

/// Forgets a name — here, and in the service if it was taught there (ADR 0054).
///
/// The other half of teaching. A name Endora was told and wrote upstream could be added
/// and never taken away, which left a person's own configuration carrying a word they had
/// changed their mind about and no way to say so from here.
///
/// The upstream removal is logged like any other change, so it too can be put back.
pub(super) async fn forget_alias_everywhere(
    State(state): State<AppState>,
    Json(req): Json<AliasRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let (server, said, means) = (req.server.clone(), req.said.clone(), req.means.clone());
    let upstream = blocking(move || {
        // Local first: forgetting here must not depend on a service being reachable.
        endora_capabilities::TargetAliasRepository::forget_alias(config.as_ref(), &server, &said)
            .map_err(AppError::Repository)?;
        let channels = native_channels(config.as_ref());
        let Some((_, channel)) = channels.iter().find(|(name, _)| *name == server) else {
            return Ok(String::new());
        };
        match channel.forget(&means, &said) {
            Some(Ok(mut write)) => {
                write.id = endora_application::IdSource::new_id(ids.as_ref());
                write.at_ms = clock.now().unix_millis();
                write.server = server;
                let described = write.describe();
                endora_capabilities::ConfigWriteLog::record(config.as_ref(), &write)
                    .map_err(AppError::Repository)?;
                Ok(described)
            }
            // It was never taught upstream, or cannot be — forgetting it here is still
            // done, and saying nothing more is honest.
            Some(Err(_)) | None => Ok(String::new()),
        }
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true, "upstream": upstream })))
}
