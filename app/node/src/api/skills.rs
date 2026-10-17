use super::*;

pub(super) fn capability_json(
    info: &endora_infrastructure::CapabilityInfo,
    stance: endora_capabilities::Stance,
    settings: &endora_infrastructure::CapabilitySettings,
) -> serde_json::Value {
    use endora_capabilities::Stance;
    // One word (ADR 0062). The older field names are kept for the console and derived,
    // so a stale page keeps rendering while it catches up.
    let enabled = stance != Stance::Off;
    let opened = stance != Stance::Off && info.reversibility.name() == "irreversible";
    let confirm = stance == Stance::Ask;
    // The settings schema, each flagged whether it's been set — but NEVER the value
    // (secrets stay server-side).
    let settings_schema: Vec<serde_json::Value> = info
        .settings
        .iter()
        .map(|s| {
            json!({
                "key": s.key,
                "label": s.label,
                "secret": s.secret,
                "set": settings.get(s.key).is_some_and(|v| !v.trim().is_empty()),
            })
        })
        .collect();
    // The same question policy asks, answered by the same code. This used to be a second
    // copy here, and when settings gained an "optional" flag only one of the two learned
    // about it — so the console said "needs setup" for a skill policy considered ready.
    let settings_complete = endora_capabilities::settings_complete(info, settings);
    // The same readiness chat uses; for a borrower, `settings` are its connection's.
    let settings_ready = endora_capabilities::settings_ready(info, settings);
    // An entry that only holds settings (for the handheld, and the chat skills that borrow
    // them) is never "configured" and its on/off does nothing; shown as on/off and "needs
    // setup", every one of them said "needs setup" forever, set up or not.
    let settings_only = endora_capabilities::infrastructure::terminal::settings_only(info);
    json!({
        "id": info.id,
        "name": info.name,
        "description": info.description,
        "category": info.category,
        "reaches_external": info.reaches_external,
        "reversibility": info.reversibility.name(),
        // Whether the person has opened this capability's irreversible band, and
        // whether it is therefore currently blocked deny-by-default (ADR 0051). Only
        // an irreversible skill can be blocked or opened.
        "open_irreversible": opened,
        "blocked": info.reversibility.name() == "irreversible" && !opened,
        // Whether the person set this skill to ask first ("on with user input"): it
        // runs only after they confirm each use, never on its own.
        "confirm": confirm,
        // `configured` = code ready + settings filled; `enabled` = the person's on/off
        // switch; a skill is usable only when both hold (ADR 0054).
        "configured": info.configured && settings_ready,
        "enabled": enabled,
        "usable": info.configured && settings_ready && enabled,
        "needs": info.needs,
        "settings_only": settings_only,
        // The connection whose settings this skill rides, if any, so the console can say
        // "needs a connection" and point at it rather than a bare "needs setup".
        "borrows_from": info.borrows_from,
        "settings_complete": settings_complete,
        // The one word the model of this is actually made of now (ADR 0062).
        "stance": stance.word(),
        "settings": settings_schema,
    })
}

/// Lists the butler's skills (capabilities/modules), their status, and whether the
/// person has each turned on.
pub(super) async fn list_capabilities(
    State(state): State<AppState>,
) -> Json<Vec<serde_json::Value>> {
    let stances: std::collections::HashMap<String, endora_capabilities::Stance> = state
        .config
        .stances()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let settings = settings_map(state.config.as_ref());
    let empty = endora_infrastructure::CapabilitySettings::new();
    Json(
        state
            .capabilities
            .iter()
            .map(|c| (c, c.info()))
            // Show a skill the person can do something about; hide one they cannot.
            //
            // A scaffold is declared with full metadata and no data source behind it —
            // unconfigured, and with no settings to fill in. There is no action
            // available, so listing it only advertises something Endora cannot do and
            // invites the fair question "what is an incident scanner and why can't I
            // configure it?". Unconfigured skills that DO have settings still show,
            // because those are a task rather than a dead end.
            //
            // The rule is a property, not a list: give a scaffold settings, or
            // implement it, and it appears here with no further change.
            .filter(|(_, info)| info.configured || !info.settings.is_empty())
            .map(|(c, info)| {
                let stance = stances
                    .get(info.id)
                    .copied()
                    .unwrap_or_else(|| endora_capabilities::default_stance(info.reversibility));
                // A skill that rides a connection is judged by the connection's settings.
                let own = settings.get(info.borrows_from.unwrap_or(info.id));
                let mut shown = capability_json(&info, stance, own.unwrap_or(&empty));
                // What the settings screen says the skill does: a line for people where the
                // description is written for the model.
                shown["summary"] = json!(c.for_people().unwrap_or(info.description));
                shown
            })
            .collect(),
    )
}

#[derive(Deserialize)]
pub(super) struct SettingsRequest {
    settings: std::collections::HashMap<String, String>,
}

/// Sets one or more settings for a capability (ADR 0054). Validated against the
/// registry and its declared setting keys, so only known keys are stored.
pub(super) async fn set_capability_settings(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SettingsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(cap) = state.capabilities.iter().find(|c| c.info().id == id) else {
        return Err(ApiError(AppError::NotFound {
            entity: "capability",
        }));
    };
    let allowed: std::collections::HashSet<&str> =
        cap.info().settings.iter().map(|s| s.key).collect();
    // Keep only keys this capability actually declares.
    let to_set: Vec<(String, String)> = req
        .settings
        .into_iter()
        .filter(|(k, _)| allowed.contains(k.as_str()))
        .collect();
    let events = state.events.clone();
    let config = state.config.clone();
    let clock = state.clock.clone();
    let cap_id = id.clone();
    blocking(move || {
        for (k, v) in &to_set {
            config
                .set_setting(&cap_id, k, v)
                .map_err(AppError::Repository)?;
        }
        record_event(
            events.as_ref(),
            clock.as_ref(),
            &format!("Updated settings for the {cap_id} skill"),
        );
        Ok::<_, AppError>(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "id": id, "ok": true })))
}

#[derive(Deserialize)]
pub(super) struct EnableRequest {
    enabled: bool,
}

/// Turns a capability on or off for the person (ADR 0054). Validated against the
/// registry so only real skill ids are stored; nudges the change stream so open
/// consoles refresh.
pub(super) async fn set_capability_enabled(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<EnableRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Any capability the butler actually offers, not just the built-in registry: an MCP
    // tool is exactly the kind of thing worth turning off (ADR 0054), and this route
    // used to 404 for every one of them.
    let known = state.capabilities.iter().any(|c| c.info().id == id)
        || state.mcp.read().ok().is_some_and(|r| {
            use endora_capabilities::CapabilityRunner;
            r.available().iter().any(|c| c.id == id)
        });
    if !known {
        return Err(ApiError(AppError::NotFound {
            entity: "capability",
        }));
    }
    let events = state.events.clone();
    let config = state.config.clone();
    let clock = state.clock.clone();
    let (cap_id, enabled) = (id.clone(), req.enabled);
    blocking(move || {
        // On = back to the band's default; off = off. One word (ADR 0062).
        if enabled {
            config.clear_stance(&cap_id).map_err(AppError::Repository)?;
        } else {
            config
                .set_stance(&cap_id, endora_capabilities::Stance::Off)
                .map_err(AppError::Repository)?;
        }
        record_event(
            events.as_ref(),
            clock.as_ref(),
            &format!(
                "Turned the {cap_id} skill {}",
                if enabled { "on" } else { "off" }
            ),
        );
        Ok(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "id": id, "enabled": req.enabled })))
}

#[derive(Deserialize)]
pub(super) struct OpenRequest {
    open: bool,
}

/// Opens or re-blocks a capability's **irreversible band** for the person (ADR
/// 0024). Opening only ever moves the un-undoable from *blocked* to
/// *confirm-each-use* — the butler still never runs it on its own. Validated
/// against the registry; nudges the change stream so open consoles refresh.
/// Opens or blocks **every tool a server exposes right now**, in one act.
///
/// Not the same thing as auto-allow, and the difference is the whole reason this exists
/// separately. Auto-allow is a **standing policy**: it opens whatever the server exposes,
/// including tools it gains later, every time it connects — which is how a tool that plays
/// audio through the house ended up open without anyone choosing it. This is a **deliberate
/// act on the tools that exist now**, and it grants nothing in future.
///
/// Twenty tools is not a thing to tap twenty times, and the answer to that is a bulk action,
/// not a default that decides for people (ADR 0054).
pub(super) async fn set_every_tool_open(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<OpenRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::CapabilityRunner;
    let prefix = format!("{name}.");
    let ids: Vec<String> = mcp_snapshot(&state)
        .available()
        .into_iter()
        .map(|s| s.id)
        .filter(|id| id.starts_with(&prefix))
        .collect();
    if ids.is_empty() {
        return Err(ApiError(AppError::NotFound {
            entity: "MCP server",
        }));
    }
    let events = state.events.clone();
    let config = state.config.clone();
    let clock = state.clock.clone();
    let open = req.open;
    let changed = blocking(move || {
        let mut changed = 0;
        for id in &ids {
            let stance = if open {
                endora_capabilities::Stance::Ask
            } else {
                endora_capabilities::Stance::Off
            };
            config
                .set_stance(id, stance)
                .map_err(AppError::Repository)?;
            changed += 1;
        }
        record_event(
            events.as_ref(),
            clock.as_ref(),
            &format!(
                "{} all {changed} tools on {name}",
                if open { "Allowed" } else { "Blocked" }
            ),
        );
        Ok::<_, AppError>(changed)
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(
        json!({ "ok": true, "changed": changed, "open": open }),
    ))
}

pub(super) async fn set_capability_open(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<OpenRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Accept a built-in capability id or a connected MCP tool id (server.tool) —
    // both are opened the same way (ADR 0051), keyed by id in the config store.
    let is_builtin = state.capabilities.iter().any(|c| c.info().id == id);
    let is_mcp = {
        use endora_capabilities::CapabilityRunner;
        mcp_snapshot(&state).available().iter().any(|s| s.id == id)
    };
    if !is_builtin && !is_mcp {
        return Err(ApiError(AppError::NotFound {
            entity: "capability",
        }));
    }
    let events = state.events.clone();
    let config = state.config.clone();
    let clock = state.clock.clone();
    let (cap_id, open) = (id.clone(), req.open);
    blocking(move || {
        let stance = if open {
            endora_capabilities::Stance::Ask
        } else {
            endora_capabilities::Stance::Off
        };
        config
            .set_stance(&cap_id, stance)
            .map_err(AppError::Repository)?;
        record_event(
            events.as_ref(),
            clock.as_ref(),
            &if open {
                format!("Allowed the {cap_id} skill — it asks each time until it has proven itself")
            } else {
                format!("Blocked the {cap_id} skill")
            },
        );
        Ok::<_, AppError>(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "id": id, "open_irreversible": req.open })))
}

#[derive(Deserialize)]
pub(super) struct ConfirmRequest {
    confirm: bool,
}

/// Sets whether a skill must **ask first** ("on with user input"): when on, the
/// butler proposes and runs it only after the person confirms each use, never on its
/// own — whatever the skill's band. Validated against the registry (built-in or a
/// connected MCP tool); nudges the change stream so open consoles refresh.
pub(super) async fn set_capability_confirm(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ConfirmRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let is_builtin = state.capabilities.iter().any(|c| c.info().id == id);
    let is_mcp = {
        use endora_capabilities::CapabilityRunner;
        mcp_snapshot(&state).available().iter().any(|s| s.id == id)
    };
    if !is_builtin && !is_mcp {
        return Err(ApiError(AppError::NotFound {
            entity: "capability",
        }));
    }
    let events = state.events.clone();
    let config = state.config.clone();
    let clock = state.clock.clone();
    let (cap_id, confirm) = (id.clone(), req.confirm);
    blocking(move || {
        if confirm {
            config
                .set_stance(&cap_id, endora_capabilities::Stance::Ask)
                .map_err(AppError::Repository)?;
        } else {
            config.clear_stance(&cap_id).map_err(AppError::Repository)?;
        }
        record_event(
            events.as_ref(),
            clock.as_ref(),
            &if confirm {
                format!("Set the {cap_id} skill to ask first before each use")
            } else {
                format!("Set the {cap_id} skill to run automatically")
            },
        );
        Ok::<_, AppError>(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "id": id, "confirm": req.confirm })))
}

pub(super) fn envelope_json(e: &AutonomyEnvelope) -> serde_json::Value {
    json!({ "auto_external": e.auto_external, "auto_consequential": e.auto_consequential })
}

/// Returns the person's autonomy envelope — the boundary the butler acts within
/// (ADR 0051).
pub(super) async fn get_autonomy(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let envelope = blocking(move || {
        AutonomyEnvelopeRepository::get(config.as_ref()).map_err(AppError::Repository)
    })
    .await?;
    Ok(Json(envelope_json(&envelope)))
}

#[derive(Deserialize)]
pub(super) struct AutonomyRequest {
    auto_external: bool,
    auto_consequential: bool,
}

/// Sets the autonomy envelope (ADR 0051). Widening it grants the butler more
/// independence; the deterministic policy layer still enforces the edges.
pub(super) async fn set_autonomy(
    State(state): State<AppState>,
    Json(req): Json<AutonomyRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let envelope = AutonomyEnvelope {
        auto_external: req.auto_external,
        auto_consequential: req.auto_consequential,
    };
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    blocking(move || {
        AutonomyEnvelopeRepository::set(config.as_ref(), &envelope)
            .map_err(AppError::Repository)?;
        record_event(
            events.as_ref(),
            clock.as_ref(),
            &format!(
                "Adjusted autonomy: read-only skills {}, consequential actions {}",
                if envelope.auto_external {
                    "on their own"
                } else {
                    "ask first"
                },
                if envelope.auto_consequential {
                    "on their own"
                } else {
                    "ask first"
                },
            ),
        );
        Ok(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(envelope_json(&envelope)))
}

/// Invokes a capability by id with a JSON body. Read-only skills run directly;
/// this is the `act` path of the autonomy model. (Consequential skills will be
/// routed through propose→confirm as they are wired.)
pub(super) async fn invoke_capability(
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
    let config = state.config.clone();
    let result =
        tokio::task::spawn_blocking(move || invoke_now(config.as_ref(), cap.as_ref(), input))
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

/// Runs a built-in capability as an explicit invoke runs it, for the owner's invoke and the
/// team's bridge (ADR 0108) alike: the data-loss tripwire and query minimization (ADR 0051)
/// for one that reaches outside (a request carrying a secret is refused, and personal
/// identifiers are redacted before it leaves), then the skill with its settings, which are
/// its connection's for a skill that rides one.
///
/// # Errors
/// [`CapabilityError`] if the input is refused or bad, or the skill cannot run.
pub(super) fn invoke_now(
    config: &endora_capabilities::ConfigStore,
    cap: &dyn Capability,
    input: serde_json::Value,
) -> Result<serde_json::Value, CapabilityError> {
    let info = cap.info();
    let mut input = input;
    if info.reaches_external {
        if let Some(kind) = endora_infrastructure::scan_outbound_secret(&input.to_string()) {
            return Err(CapabilityError::BadInput(format!(
                "refusing to send this out — it looks like it contains {kind}"
            )));
        }
        endora_infrastructure::redact_pii_in_value(&mut input);
    }
    let settings = settings_map(config)
        .remove(info.borrows_from.unwrap_or(info.id))
        .unwrap_or_default();
    cap.invoke(&input, &settings)
}

/// Proves an MCP server works with the credential it has, right now.
///
/// Built-in skills have had a Test button and both model endpoints a *Test connection*;
/// somebody else's server had **Reconnect**, which proves a process started and listed its
/// tools. That is not the question anybody is asking.
///
/// Live, and the reason this exists: a Brave server connected cleanly, advertised eight
/// tools, reported no error, and was subscribed to the wrong Brave API. Every indicator on
/// the card was green because **a handshake never calls the service behind the server**. The
/// first anyone would have known is a search quietly returning nothing.
///
/// It calls the tool the person nominated as this server's state reader (ADR 0054). That
/// nomination already means *"this one only looks"*, so the safety question is settled by a
/// decision they made rather than by a guess here — the same rule as `test_capability`, that
/// "press this to find out" must never be how someone learns what a skill does. A server
/// with no reader nominated is not tested; it says so and says what to do about it.
///
/// A refusal from the far end is a **result, not an error**: `401` is the most useful thing
/// this endpoint can report, so it comes back as prose with `ok: false` rather than as a
/// failed request.
pub(super) async fn test_mcp_server(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::{CapabilityRunner, McpServerRegistry};
    let config = state.config.clone();
    let lookup = name.clone();
    let known = blocking(move || {
        Ok(McpServerRegistry::list(config.as_ref())
            .map_err(AppError::Repository)?
            .into_iter()
            .find(|s| s.name == lookup))
    })
    .await?;
    let Some(server) = known else {
        return Err(ApiError(AppError::NotFound { entity: "server" }));
    };
    if server.reader_tool.trim().is_empty() {
        return Ok(Json(json!({
            "ok": false,
            "said": "Nobody has said which of its tools reads this server's state, so there \
                    is nothing safe to call. Choose one above and press Test again.",
        })));
    }

    let id = format!("{}.{}", server.name, server.reader_tool.trim());
    let runner = mcp_snapshot(&state);
    let Some(spec) = runner.available().into_iter().find(|s| s.id == id) else {
        return Ok(Json(json!({
            "ok": false,
            "said": format!(
                "It is not offering '{}' at the moment. Press Reconnect, then Test again.",
                server.reader_tool.trim()
            ),
        })));
    };

    let args = endora_capabilities::arguments_for_a_test_call(spec.input_schema.as_deref());
    let events = state.events.clone();
    let clock = state.clock.clone();
    let called = tokio::task::spawn_blocking(move || runner.run(&id, &args))
        .await
        .map_err(|_| {
            ApiError(AppError::Repository(RepositoryError::Backend(
                "worker task failed".to_owned(),
            )))
        })?;
    match called {
        Ok(said) => {
            record_event(
                events.as_ref(),
                clock.as_ref(),
                &format!("Tested the {name} server: it answered."),
            );
            // Enough to show it really answered, not so much that a page of search results
            // lands in a toast.
            let shown: String = said.chars().take(300).collect();
            Ok(Json(json!({ "ok": true, "said": shown })))
        }
        Err(why) => Ok(Json(json!({ "ok": false, "said": why }))),
    }
}

/// Proves a skill works with the settings it has, right now.
///
/// Both model endpoints have had a *Test connection* button since they existed; no skill
/// ever has. So the only way to find out whether a URL, a token — or a newly nominated
/// notify service — is right was to save it and wait for something that might not arrive
/// for hours.
///
/// Read-only skills prove themselves by running, which is the default. A skill that can
/// actuate refuses: "press this to find out" must never be how someone discovers what a
/// skill does.
pub(super) async fn test_capability(
    State(state): State<AppState>,
    Path(id): Path<String>,
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
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    let told = id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let settings = settings_map(config.as_ref())
            .remove(&told)
            .unwrap_or_default();
        cap.self_test(&settings)
    })
    .await
    .map_err(|_| {
        ApiError(AppError::Repository(RepositoryError::Backend(
            "worker task failed".to_owned(),
        )))
    })?;
    match result {
        Ok(said) => {
            record_event(
                events.as_ref(),
                clock.as_ref(),
                &format!("Tested the {id} skill: {said}"),
            );
            Ok(Json(json!({ "ok": true, "said": said })))
        }
        // Not something the person did wrong — a soft result they can read and act on.
        Err(CapabilityError::BadInput(m) | CapabilityError::Unavailable(m)) => {
            Ok(Json(json!({ "ok": false, "said": m })))
        }
    }
}
