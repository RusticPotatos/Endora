use super::*;

/// What the next message would send the model, section by section, and how full the window
/// would be (ADR 0088). Estimates, labelled as such by the console.
pub(super) async fn chat_context(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chat = state.chat.clone();
    let understanding = state.understanding.clone();
    let knowledge = state.knowledge.clone();
    let config = state.config.clone();
    let clock = state.clock.clone();
    let capabilities = state.capabilities.clone();
    let summary = state.summary.clone();
    let mcp = mcp_snapshot(&state);
    let report = blocking(move || {
        use endora_application::ConversationSummaryStore as _;
        let runner = build_runner(
            config.as_ref(),
            capabilities,
            mcp,
            proven_now(understanding.as_ref()),
        );
        let mut context = usecases::butler_context(
            knowledge.as_ref(),
            knowledge.as_ref(),
            understanding.as_ref(),
            config.as_ref(),
            chat.as_ref(),
            &runner,
            config.as_ref(),
            clock.as_ref(),
        )?;
        let from = topic_start(config.as_ref());
        context.context_from_ms = from;
        // Not part of an ordinary message: it is only sent when the person asks what Endora did.
        context.did_lately.clear();
        let (history, summary_text) =
            usecases::history_the_next_turn_would_send(chat.as_ref(), summary.get(), from)?;
        let prefs = endora_application::PreferenceRepository::list_all(knowledge.as_ref())
            .map_err(AppError::Repository)?;
        let report = endora_application::context_report::context_report(
            &context,
            &history,
            &prefs,
            summary_text.as_deref(),
            endora_infrastructure::butler_instructions(),
            context_window_tokens(),
        );
        Ok::<_, AppError>((report, from))
    })
    .await?;
    let (report, from) = report;
    let sections: Vec<serde_json::Value> = report
        .sections
        .iter()
        .map(|s| {
            json!({
                "key": s.key,
                "label": s.label,
                "tokens": s.tokens,
                "count": s.count,
                "to_deep": s.to_deep,
                "note": s.note,
                "items": s.items.iter().map(|i| json!({"text": i.text, "tokens": i.tokens})).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(json!({
        "window_tokens": report.window_tokens,
        "total_tokens": report.total_tokens,
        "context_from_ms": from,
        "estimate": true,
        "sections": sections,
    })))
}

/// Starts a new topic: from now on the model reads only what comes after this moment, and
/// the running summary of what came before is dropped. Nothing is deleted — the earlier chat
/// stays on screen — and what Endora has learned about the person is untouched (ADR 0088).
pub(super) async fn reset_chat_context(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_application::ConversationSummaryStore as _;
    let config = state.config.clone();
    let clock = state.clock.clone();
    let summary = state.summary.clone();
    let at = blocking(move || {
        use endora_application::Clock as _;
        let at = clock.now().unix_millis();
        config
            .set_setting("chat", "context_from_ms", &at.to_string())
            .map_err(AppError::Repository)?;
        summary.set(endora_application::ConversationSummary::default());
        Ok::<_, AppError>(at)
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true, "context_from_ms": at })))
}

#[derive(Deserialize)]
pub(super) struct ChatRequest {
    message: String,
    #[serde(default)]
    request_id: Option<String>,
    /// Sent with 🔒 (ADR 0105): Restricted, so this turn, and any while it is in view, is
    /// answered on the house's own network and never leaves it, disguised or not.
    #[serde(default)]
    private: bool,
}

impl ChatRequest {
    /// The message as it is kept: a private one carries the mark the one door refuses.
    fn text(&self) -> String {
        let mark = endora_application::egress::RESTRICTED_MARK;
        if self.private && !self.message.starts_with(mark) {
            return format!("{mark}{}", self.message);
        }
        self.message.clone()
    }
}

/// Sends a message to the butler; returns its reply and what it did this turn.
pub(super) async fn send_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chat = state.chat.clone();
    let understanding = state.understanding.clone();
    let knowledge = state.knowledge.clone();
    let config = state.config.clone();
    let events = state.events.clone();
    let audit = state.audit.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let butler = state.butler.clone();
    let capabilities = state.capabilities.clone();
    let mcp = mcp_snapshot(&state);
    // Serialize this turn against any other butler turn (chat or heartbeat brief).
    let _turn = state.turn_lock.clone().lock_owned().await;
    let (reply, activity) = blocking(move || {
        let runner = build_runner(
            config.as_ref(),
            capabilities,
            mcp,
            proven_now(understanding.as_ref()),
        );
        // Ground the butler in the person's current life before it answers.
        let mut context = usecases::butler_context(
            knowledge.as_ref(),
            knowledge.as_ref(),
            understanding.as_ref(),
            config.as_ref(),
            chat.as_ref(),
            &runner,
            config.as_ref(),
            clock.as_ref(),
        )?;
        context.context_from_ms = topic_start(config.as_ref());
        let out = usecases::send_to_butler_streaming_for_request(
            chat.as_ref(),
            knowledge.as_ref(),
            understanding.as_ref(),
            &runner,
            butler.as_ref(),
            audit.as_ref(),
            None,
            ids.as_ref(),
            clock.as_ref(),
            &context,
            &req.text(),
            req.request_id.as_deref(),
            &mut |_| {},
            &mut |_| {},
            &mut Vec::new(),
        )?;
        // Persist what the butler did/learned this turn to its action log, so the
        // activity view is a durable record, not just an in-page note.
        for item in &out.1 {
            record_event(events.as_ref(), clock.as_ref(), item);
        }
        Ok(out)
    })
    .await?;
    Ok(Json(json!({
        "reply": MessageResponse::from(&reply),
        "activity": activity,
    })))
}

/// Composes and posts a daily briefing — an act of service using only reversible,
/// autonomous skills (ADRs 0024/0025). The butler decides what the brief needs and
/// writes it from what it actually gathered (ADR 0053). Returns the posted message,
/// or a note if it had nothing worth saying.
pub(super) async fn brief(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chat = state.chat.clone();
    let understanding = state.understanding.clone();
    let knowledge = state.knowledge.clone();
    let config = state.config.clone();
    let events = state.events.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let capabilities = state.capabilities.clone();
    let mail_lines = super::workers::brief_lines(&state);
    let butler = state.butler.clone();
    let mcp = mcp_snapshot(&state);
    let result = blocking(move || {
        // A brief is an act of service, defined as reversible — it gathers and writes,
        // it never actuates. Enforced, not just documented above.
        let runner = build_reversible_only_runner(
            config.as_ref(),
            capabilities,
            mcp,
            proven_now(understanding.as_ref()),
        );
        let context = usecases::butler_context(
            knowledge.as_ref(),
            knowledge.as_ref(),
            understanding.as_ref(),
            config.as_ref(),
            chat.as_ref(),
            &runner,
            config.as_ref(),
            clock.as_ref(),
        )?;
        let out = usecases::daily_brief(
            chat.as_ref(),
            &runner,
            butler.as_ref(),
            ids.as_ref(),
            clock.as_ref(),
            &context,
            &mail_lines,
        )?;
        if let Some((_, activity)) = &out {
            for item in activity {
                record_event(events.as_ref(), clock.as_ref(), item);
            }
            record_event(events.as_ref(), clock.as_ref(), "Prepared your daily brief");
        }
        Ok::<_, AppError>(out)
    })
    .await?;
    let _ = state.changes.send(());
    match result {
        Some((msg, _)) => Ok(Json(
            json!({ "briefed": true, "message": MessageResponse::from(&msg) }),
        )),
        None => Ok(Json(
            json!({ "briefed": false, "note": "Set your home location and enable some skills — weather, news, the home — to get a brief." }),
        )),
    }
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
pub(super) async fn stream_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let chat = state.chat.clone();
    let understanding = state.understanding.clone();
    let knowledge = state.knowledge.clone();
    let config = state.config.clone();
    let events = state.events.clone();
    let audit = state.audit.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let butler = state.butler.clone();
    let changes = state.changes.clone();
    let capabilities = state.capabilities.clone();
    let summary = state.summary.clone();
    let mcp = mcp_snapshot(&state);
    let message = req.text();
    let request_id = req.request_id;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Event>();

    // Serialize this turn against any other butler turn (a concurrent chat, or a
    // heartbeat brief). The guard is held on the async task across the whole
    // blocking turn — so a second turn waits here until this one finishes.
    let turn_lock = state.turn_lock.clone();
    tokio::task::spawn(async move {
        let _turn = turn_lock.lock_owned().await;
        tokio::task::spawn_blocking(move || {
            let runner = build_runner(
                config.as_ref(),
                capabilities,
                mcp,
                proven_now(understanding.as_ref()),
            );
            let event = |v: serde_json::Value| Event::default().data(v.to_string());
            let context = match usecases::butler_context(
                knowledge.as_ref(),
                knowledge.as_ref(),
                understanding.as_ref(),
                config.as_ref(),
                chat.as_ref(),
                &runner,
                config.as_ref(),
                clock.as_ref(),
            ) {
                Ok(mut c) => {
                    c.context_from_ms = topic_start(config.as_ref());
                    c
                }
                Err(e) => {
                    // A failure that evaporates can be neither trusted nor improved: this
                    // turn used to die leaving the last stored message as the person's,
                    // nothing after it, and nothing anywhere saying why.
                    record_event(
                        events.as_ref(),
                        clock.as_ref(),
                        &format!("A turn failed before it could reply: {e}"),
                    );
                    let _ = tx.send(event(json!({ "type": "error", "message": e.to_string() })));
                    return;
                }
            };
            // Collect the turn's steps so they can be PERSISTED with the reply (so a
            // past answer keeps its expandable actions + Sources after a reload), in
            // addition to streaming them live.
            let collected =
                std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
            let collected_step = collected.clone();
            // Scope the token closure so its borrow of `tx` ends before the `done`
            // send below.
            // Every action this turn took, recorded deterministically whatever the
            // reply ends up claiming (ADR 0053).
            let mut disclosed: Vec<usecases::ActionDisclosure> = Vec::new();
            // Words streamed since the last step. If the turn then does something else (calls
            // a skill, asks the deep model), those words were working notes and not the
            // answer, so they are moved into the action trail as a "Thinking" row rather
            // than left in the reply bubble to be overwritten later.
            let spoken = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
            let spoken_token = spoken.clone();
            let spoken_step = spoken.clone();
            let result = {
                let mut on_token = |chunk: &str| {
                    if let Ok(mut g) = spoken_token.lock() {
                        g.push_str(chunk);
                    }
                    let _ = tx.send(event(json!({ "type": "token", "text": chunk })));
                };
                // Structured action steps (router → skill → result), surfaced live so
                // the console can show an expandable trail of what the butler is doing,
                // separate from the reply prose.
                let mut on_step = |step: usecases::ButlerStep| {
                    let notes = spoken_step
                        .lock()
                        .map(|mut g| std::mem::take(&mut *g))
                        .unwrap_or_default();
                    if !notes.trim().is_empty() {
                        let thought = json!({
                            "skill": "thinking",
                            "status": "done",
                            "label": "Thinking",
                            "output": notes.trim(),
                        });
                        if let Ok(mut g) = collected_step.lock() {
                            g.push(thought.clone());
                        }
                        let mut e = thought;
                        e["type"] = json!("step");
                        let _ = tx.send(event(e));
                    }
                    let v = json!({
                        "skill": step.skill,
                        "status": step.status.as_str(),
                        "label": step.label,
                        "output": step.output,
                    });
                    // Every call reports twice — once running, once with its outcome — and the
                    // persisted trail keeps one row per call (see `fold_step`).
                    if let Ok(mut g) = collected_step.lock() {
                        fold_step(&mut g, v.clone());
                    }
                    let mut e = v;
                    e["type"] = json!("step");
                    let _ = tx.send(event(e));
                };
                usecases::send_to_butler_streaming_for_request(
                    chat.as_ref(),
                    knowledge.as_ref(),
                    understanding.as_ref(),
                    &runner,
                    butler.as_ref(),
                    audit.as_ref(),
                    Some(&summary as &dyn endora_application::ConversationSummaryStore),
                    ids.as_ref(),
                    clock.as_ref(),
                    &context,
                    &message,
                    request_id.as_deref(),
                    &mut on_token,
                    &mut on_step,
                    &mut disclosed,
                )
            };
            match result {
                Ok((reply, activity)) => {
                    // Persist the turn's actions/learnings to the butler's action log.
                    for item in &activity {
                        record_event(events.as_ref(), clock.as_ref(), item);
                    }
                    // Persist this reply's action trail (steps + real sources) so it
                    // survives a reload — the client renders it under the message.
                    let steps = collected.lock().map(|g| g.clone()).unwrap_or_default();
                    let sources = sources_from_steps(&steps);
                    // The deterministic half of honesty about actions (ADR 0053): the
                    // model ignores the read-back roughly two runs in three, so the
                    // person is shown what ran and whether it was confirmed regardless
                    // of what the prose says. This never edits the reply.
                    let disclosures: Vec<serde_json::Value> = disclosed
                        .iter()
                        .map(|d| {
                            json!({
                                "skill": d.skill,
                                "claimed": d.claimed,
                                "observed": d.observed,
                                "confirmed": d.was_observed(),
                                "outcome": d.outcome,
                            })
                        })
                        .collect();
                    // `activity` is stored with the rest, not only streamed. It was sent
                    // over the live stream and nowhere else, so the note of what Endora
                    // learned existed for exactly as long as the person stayed on the
                    // screen and vanished on any reload or view change — the one part of
                    // the trail that did not survive, and the part that says what the turn
                    // changed about its understanding.
                    let actions = json!({
                        "steps": steps,
                        "sources": sources,
                        "actions_taken": disclosures,
                        "activity": activity,
                    });
                    let _ =
                        chat.save_actions(&reply.id().value().to_string(), &actions.to_string());
                    // A successful write nudges the change stream, like other writes.
                    let _ = changes.send(());
                    let _ = tx.send(event(json!({
                        "type": "done",
                        "reply": MessageResponse::from(&reply),
                        "activity": activity,
                        "actions": actions,
                    })));
                }
                Err(e) => {
                    record_event(
                        events.as_ref(),
                        clock.as_ref(),
                        &format!("A turn failed before it could reply: {e}"),
                    );
                    let _ = tx.send(event(json!({ "type": "error", "message": e.to_string() })));
                }
            }
        })
        .await
        .ok(); // release the turn lock once the blocking turn completes
    });

    let stream = unfold(rx, |mut rx| async move {
        rx.recv().await.map(|ev| (Ok(ev), rx))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Folds a step into the trail **persisted** with the reply.
///
/// Every tool call reports twice — once running, once with its outcome. The live view
/// finalises the running row in place; the persisted trail kept both, so reopening a
/// past reply showed three tool calls as "6 actions". This collapses the same way, so
/// what is stored matches what was watched.
///
/// A terminal step with nothing in flight starts its own row: a blocked call never
/// reports running, and folding it away would hide the most interesting kind of step
/// there is — one that policy refused.
pub(super) fn fold_step(trail: &mut Vec<serde_json::Value>, step: serde_json::Value) {
    let field = |v: &serde_json::Value, k: &str| {
        v.get(k)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    if field(&step, "status") == "running" {
        trail.push(step);
        return;
    }
    // Finish the in-flight row **for this same skill**. Matching on skill matters: a
    // blocked call reports no `running` at all, and folding it into whatever happened to
    // be in flight would overwrite an unrelated call and hide the refusal.
    let skill = field(&step, "skill");
    let in_flight = trail
        .iter()
        .rposition(|s| field(s, "status") == "running" && field(s, "skill") == skill);
    match in_flight {
        Some(i) => trail[i] = step,
        None => trail.push(step),
    }
}

/// Extracts the http(s) URLs a turn's steps returned — the real sources — from
/// the step outputs, deduped, in order.
pub(super) fn sources_from_steps(steps: &[serde_json::Value]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for s in steps {
        let Some(text) = s["output"].as_str() else {
            continue;
        };
        for token in text.split_whitespace() {
            let Some(idx) = token.find("http") else {
                continue;
            };
            let url = &token[idx..];
            if url.starts_with("http://") || url.starts_with("https://") {
                let url = url.trim_end_matches(['.', ',', ';', ':', ')', '"', '\'']);
                if seen.insert(url.to_owned()) {
                    out.push(url.to_owned());
                }
            }
        }
    }
    out
}

/// The whole conversation with the butler, oldest first — each butler message
/// carrying its persisted action trail (steps + sources) when it has one, so the
/// console can render the expandable actions + Sources for past replies too.
#[derive(Deserialize)]
pub(super) struct HistoryWindow {
    /// Inclusive start, in milliseconds. Omitted means everything.
    from: Option<i64>,
    /// Exclusive end, in milliseconds.
    to: Option<i64>,
}

/// The local days that have conversation, so the console can offer a way back without
/// holding every message it might need.
///
/// `offset_minutes` is the caller's distance from UTC. The server stores instants and is
/// told where the caller's midnights fall rather than guessing — which also means it stays
/// right when they travel.
pub(super) async fn chat_days(
    State(state): State<AppState>,
    Query(req): Query<DaysRequest>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let chat = state.chat.clone();
    let offset = req.offset_minutes.unwrap_or(0);
    let days = blocking(move || {
        endora_conversation::ChatRepository::days(chat.as_ref(), offset)
            .map_err(AppError::Repository)
    })
    .await?;
    Ok(Json(
        days.into_iter()
            .map(|(day, messages)| json!({ "day": day, "messages": messages }))
            .collect(),
    ))
}

#[derive(Deserialize)]
pub(super) struct DaysRequest {
    offset_minutes: Option<i64>,
}

pub(super) async fn chat_history(
    State(state): State<AppState>,
    Query(window): Query<HistoryWindow>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let chat = state.chat.clone();
    let (messages, actions) = blocking(move || {
        // A console asks for the stretch it is showing. Everything, only when nobody
        // said — which keeps every existing caller working and stops being the default
        // the moment the browser knows what day it is.
        let msgs = match (window.from, window.to) {
            (Some(from), Some(to)) => {
                endora_conversation::ChatRepository::between(chat.as_ref(), from, to)
                    .map_err(AppError::Repository)?
            }
            _ => usecases::chat_history(chat.as_ref())?,
        };
        let acts = chat.all_actions().map_err(AppError::Repository)?;
        Ok::<_, AppError>((msgs, acts))
    })
    .await?;
    let action_map: std::collections::HashMap<String, serde_json::Value> = actions
        .into_iter()
        .filter_map(|(id, json)| serde_json::from_str(&json).ok().map(|v| (id, v)))
        .collect();
    let out: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            let r = MessageResponse::from(m);
            let mut v = serde_json::to_value(&r).unwrap_or_else(|_| json!({}));
            if let Some(a) = action_map.get(&r.id) {
                v["actions"] = a.clone();
            }
            v
        })
        .collect();
    Ok(Json(out))
}
