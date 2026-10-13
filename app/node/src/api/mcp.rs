use super::*;

/// Connects to every **enabled stdio** MCP server in the registry, returning a
/// runner over the ones that came up (ADR 0054). A server whose process fails to
/// start or handshake is skipped — its tools simply don't appear — so one bad server
/// can't break startup or a turn. HTTP transport is a later slice.
pub(super) fn connect_mcp(
    config: &endora_capabilities::ConfigStore,
) -> endora_capabilities::McpRunner {
    use endora_capabilities::{
        CapabilityRunner, HttpMcpClient, McpClient, McpServerRegistry, McpTransport, StdioMcpClient,
    };
    let servers = config.list().unwrap_or_default();
    // Namespacing prefixes of servers whose tools should be auto-allowed on connect.
    let trusted: Vec<String> = servers
        .iter()
        .filter(|s| s.enabled && s.trust_all)
        .map(|s| format!("{}.", s.name))
        .collect();
    // Each server carries the tool the person nominated as its state reader (ADR 0054),
    // so read-back is data rather than a name in Endora's source.
    let clients: Vec<(String, Box<dyn McpClient>, String)> = servers
        .into_iter()
        .filter(|s| s.enabled)
        .filter_map(|s| {
            let reader = s.reader_tool.clone();
            match &s.transport {
                McpTransport::Stdio { command, args, env } => {
                    StdioMcpClient::spawn_with_env(command, args, env)
                        .ok()
                        .map(|c| (s.name, Box::new(c) as Box<dyn McpClient>, reader))
                }
                McpTransport::Http { url, auth } => HttpMcpClient::connect_with_auth(url, auth)
                    .ok()
                    .map(|c| (s.name, Box::new(c) as Box<dyn McpClient>, reader)),
            }
        })
        .collect();
    let runner = endora_capabilities::McpRunner::connect_with_readers(clients);
    // trust_all's one remaining meaning (ADR 0062): a tool arriving on that server gets
    // `ask` instead of `off`. Only tools nobody has ruled on — a stored stance is a
    // decision, and connect is a default that must never overwrite one.
    if !trusted.is_empty() {
        let decided: Vec<String> = endora_capabilities::CapabilityConfigRepository::stances(config)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let available: Vec<String> = runner.available().iter().map(|s| s.id.clone()).collect();
        for id in endora_capabilities::tools_to_open_on_connect(&available, &trusted, &decided) {
            let _ = endora_capabilities::CapabilityConfigRepository::set_stance(
                config,
                &id,
                endora_capabilities::Stance::Ask,
            );
        }
    }
    runner
}

/// Rebuilds the connected-MCP runner from the current registry and swaps it into
/// `slot`. Called after a registry change so new/removed servers take effect without
/// a restart. Blocking (spawns subprocesses) — run it off the async path.
pub(super) fn reconnect_mcp(
    config: &endora_capabilities::ConfigStore,
    slot: &std::sync::RwLock<Arc<endora_capabilities::McpRunner>>,
) {
    let runner = Arc::new(connect_mcp(config));
    // Recover a poisoned lock rather than panicking the handler.
    match slot.write() {
        Ok(mut guard) => *guard = runner,
        Err(poisoned) => *poisoned.into_inner() = runner,
    }
}

/// Reconnects any **enabled server that is exposing no tools**.
///
/// Zero tools on an enabled server is not a state that ever makes sense: it means the
/// connection was attempted and found nothing there. Left alone it is permanent, because
/// tools are discovered once at connect time — and it fails silently, since the server
/// still looks registered and switched on.
///
/// Reconnecting rebuilds every connection, so this only fires when something is actually
/// wrong rather than on a healthy tick.
pub(super) fn reconnect_empty_mcp_servers(state: &AppState) {
    use endora_capabilities::{CapabilityRunner, McpServerRegistry};
    let enabled: Vec<String> = McpServerRegistry::list(state.config.as_ref())
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| s.name)
        .collect();
    if enabled.is_empty() {
        return;
    }
    // Which prefixes actually have tools right now.
    let live = mcp_snapshot(state).available();
    let empty: Vec<&String> = enabled
        .iter()
        .filter(|name| {
            let prefix = format!("{name}.");
            !live.iter().any(|c| c.id.starts_with(&prefix))
        })
        .collect();
    if empty.is_empty() {
        return;
    }
    // NOTE (ADR 0073): "exposing no tools" is a weak health check and it is deliberately
    // kept weak. It catches the boot race it was written for — a server not up yet when
    // the node started. It cannot catch a session that died *after* a good connect,
    // because the tool list is cached from that connect and keeps the server looking
    // healthy while every call 404s. That failure heals inside the transport, where the
    // dead session is actually visible; this stays the coarse net for the coarse case.
    println!(
        "mcp: {} enabled server(s) exposing no tools ({}) — reconnecting",
        empty.len(),
        empty
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    reconnect_mcp(state.config.as_ref(), state.mcp.as_ref());
}

/// A snapshot of the shared MCP runner (cheap `Arc` clone), for composing this turn.
pub(super) fn mcp_snapshot(state: &AppState) -> Arc<endora_capabilities::McpRunner> {
    match state.mcp.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

#[derive(Deserialize)]
pub(super) struct CatalogQuery {
    #[serde(default)]
    q: String,
}

/// Searches the MCP catalog: the curated entries shipped with Endora, plus — best
/// effort — the community registry (ADR 0054). Results prefill the "Add a server"
/// form and stay editable, so a stale launch command can be corrected before it is
/// registered. Registry lookup is opportunistic: if it is unreachable, misconfigured,
/// or replies in a shape we don't recognise, the curated results still come back and
/// `registry_ok` says what happened.
pub(super) async fn search_mcp_catalog(Query(q): Query<CatalogQuery>) -> Json<serde_json::Value> {
    let needle = q.q.clone();
    let mut servers = crate::mcp_catalog::search(&needle);
    // The registry lives outside the process; do the blocking HTTP off the async path.
    let found = tokio::task::spawn_blocking(move || {
        let base = std::env::var("ENDORA_MCP_REGISTRY_URL")
            .unwrap_or_else(|_| endora_capabilities::mcp_registry::DEFAULT_REGISTRY_URL.to_owned());
        endora_capabilities::mcp_registry::search(&base, &needle)
    })
    .await
    .unwrap_or(None);
    let registry_ok = found.is_some();
    for e in found.unwrap_or_default() {
        // The registry tells us either a hosted endpoint or how to launch a package;
        // either way this prefills the form, and the person reviews before adding.
        let mut fields: Vec<serde_json::Value> = e
            .env_keys
            .iter()
            .map(|k| {
                json!({
                    "key": k, "label": k, "placeholder": "",
                    "secret": true, "target": "env",
                })
            })
            .collect();
        if e.transport == "http" && e.url.is_empty() {
            fields.push(json!({
                "key": "url", "label": "Endpoint URL", "placeholder": "",
                "secret": false, "target": "url",
            }));
        }
        servers.push(json!({
            "id": e.name,
            "name": e.name,
            "description": if e.description.is_empty() {
                "From the community registry — see its docs for how to run it.".to_owned()
            } else { e.description },
            "category": "registry",
            "transport": e.transport,
            "command": e.command,
            "args": e.args,
            "url": e.url,
            "docs": e.docs,
            "source": "registry",
            // When the registry last recorded a change. It publishes no popularity
            // signal — no downloads, no stars — so this is the only thing that
            // distinguishes a maintained server from an abandoned one.
            "updated": e.updated,
            "fields": fields,
        }));
    }
    Json(json!({ "servers": servers, "registry_ok": registry_ok }))
}

/// Lists the registered MCP servers and, for each, how many of its tools are
/// currently live (ADR 0054). A server with 0 live tools is registered but didn't
/// connect (bad command, unreachable, or disabled).
pub(super) async fn list_mcp_servers(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::{
        CapabilityConfigRepository, CapabilityRunner, McpServerRegistry, McpTransport,
    };
    let config = state.config.clone();
    let (servers, stances) = blocking(move || {
        Ok((
            config.list().map_err(AppError::Repository)?,
            CapabilityConfigRepository::stances(config.as_ref()).map_err(AppError::Repository)?,
        ))
    })
    .await?;
    // One word per tool (ADR 0062). "Withdrawn" and "blocked" both read as `off` now —
    // that collapse is the point, not a loss.
    let withdrawn: std::collections::HashSet<String> = stances
        .iter()
        .filter(|(_, st)| *st == endora_capabilities::Stance::Off)
        .map(|(id, _)| id.clone())
        .collect();
    let opened: std::collections::HashSet<String> = stances
        .into_iter()
        .filter(|(_, st)| *st != endora_capabilities::Stance::Off)
        .map(|(id, _)| id)
        .collect();
    // The live tools (namespaced server.tool) each connected server exposes.
    let live = mcp_snapshot(&state).available();
    let out: Vec<_> = servers
        .into_iter()
        .map(|s| {
            // NEVER return the secrets (a stdio env's values, an http bearer token) —
            // only their names / whether one is set, like capability settings do.
            let (transport, command, args, url, env_keys, auth_set) = match &s.transport {
                McpTransport::Stdio { command, args, env } => (
                    "stdio",
                    command.clone(),
                    args.clone(),
                    String::new(),
                    env.keys().cloned().collect::<Vec<_>>(),
                    false,
                ),
                McpTransport::Http { url, auth } => (
                    "http",
                    String::new(),
                    Vec::new(),
                    url.clone(),
                    Vec::new(),
                    !auth.is_empty(),
                ),
            };
            let prefix = format!("{}.", s.name);
            // Each tool, with whether the person has opened it (allowed it to run,
            // confirm-each-use). Un-opened MCP tools are visible but blocked (ADR 0051).
            let tools: Vec<_> = live
                .iter()
                .filter(|spec| spec.id.starts_with(&prefix))
                .map(|spec| {
                    json!({
                        "id": spec.id,
                        "description": spec.description,
                        "opened": opened.contains(&spec.id),
                        "enabled": !withdrawn.contains(&spec.id),
                    })
                })
                .collect();
            json!({
                "name": s.name,
                "transport": transport,
                "command": command,
                "args": args,
                "url": url,
                // Secret-safe: the env variable NAMES only, and whether a token is set.
                "env_keys": env_keys,
                "auth_set": auth_set,
                "enabled": s.enabled,
                "trust_all": s.trust_all,
                "reader_tool": s.reader_tool,
                "tools_live": tools.len(),
                "tools": tools,
            })
        })
        .collect();
    Ok(Json(json!({ "servers": out })))
}

pub(super) fn default_enabled() -> bool {
    true
}

#[derive(Deserialize)]
pub(super) struct McpServerRequest {
    name: String,
    /// `"stdio"` (default) or `"http"`.
    #[serde(default)]
    transport: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    url: String,
    /// Environment for a stdio server's child process — where most servers take
    /// their credentials. A secret: stored, never returned.
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    /// Bearer token for an http server (e.g. a Home Assistant long-lived token).
    /// A secret: stored, never returned.
    #[serde(default)]
    auth: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
    /// Auto-allow all of this server's tools on connect (default on). Opened tools are
    /// still confirmed each use — this never removes the ask-before-acting net.
    #[serde(default = "default_enabled")]
    trust_all: bool,
}

/// Registers (or replaces) an MCP server, then reconnects so its tools appear
/// without a restart. Registration is deliberately a plain, network-trusted config
/// write here (like the other 0.x config endpoints); the tools it exposes are still
/// band-classified before they can run (ADR 0054/0024).
pub(super) async fn register_mcp_server(
    State(state): State<AppState>,
    Json(req): Json<McpServerRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::{McpServer, McpServerRegistry, McpTransport};
    // A server's name is the namespace for every tool it exposes (`server.tool`), and a
    // tool id is resolved on its FIRST separator. A name containing one therefore points
    // at a server that does not exist, and every tool from it silently disappears —
    // observed with a registry id, `io.github.XavierFabregat/spotify-mcp`, which resolves
    // to a server called "io".
    //
    // Checked here rather than only in the console, because the console is not the only
    // way in.
    if req.name.contains(['.', ':']) {
        return Err(ApiError(AppError::BadRequest {
            message: format!(
                "'{}' can't be a server name: it is the prefix for this server's tools, \
                 and a dot or colon in it hides every one of them. Try something short, \
                 like 'spotify-mcp'.",
                req.name
            ),
        }));
    }
    let config = state.config.clone();
    let mcp = state.mcp.clone();
    blocking(move || {
        let enabled = req.enabled;
        // Editing an existing server: a blank secret means "keep what's stored", so
        // changing (say) just the URL doesn't wipe a token the person didn't retype —
        // the same rule capability settings use. Secrets are never sent back to the
        // client, so a blank field is the only value it can offer for an unchanged one.
        let existing = McpServerRegistry::list(config.as_ref())
            .map_err(AppError::Repository)?
            .into_iter()
            .find(|s| s.name == req.name);
        let mut server = match req.transport.as_str() {
            "http" => {
                let mut auth = req.auth.clone();
                if auth.is_empty() {
                    if let Some(McpTransport::Http { auth: old, .. }) =
                        existing.as_ref().map(|s| &s.transport)
                    {
                        auth = old.clone();
                    }
                }
                McpServer::http_with_auth(&req.name, &req.url, &auth)
            }
            _ => {
                let mut env = req.env.clone();
                if let Some(McpTransport::Stdio { env: old, .. }) =
                    existing.as_ref().map(|s| &s.transport)
                {
                    // Fill blanks from the stored env; a supplied value overrides.
                    for (k, v) in env.iter_mut() {
                        if v.is_empty() {
                            if let Some(prev) = old.get(k) {
                                *v = prev.clone();
                            }
                        }
                    }
                }
                McpServer::stdio_with_env(&req.name, &req.command, req.args, env)
            }
        }
        .map_err(AppError::Domain)?;
        server.enabled = enabled;
        server.trust_all = req.trust_all;
        McpServerRegistry::register(config.as_ref(), &server).map_err(AppError::Repository)?;
        reconnect_mcp(config.as_ref(), mcp.as_ref());
        Ok(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

// ---- Recipes: the person's own capabilities (ADR 0071) ---------------------

#[derive(Deserialize)]
pub(super) struct RecipeInputRequest {
    name: String,
    /// "string" or "number" — the vocabulary `RecipeInputKind` stores.
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
pub(super) struct RecipeRequest {
    id: String,
    description: String,
    #[serde(default)]
    inputs: Vec<RecipeInputRequest>,
    get: String,
    say: String,
}

/// A draft being tried, before it is anything (ADR 0071). No id or description:
/// what a trial can tell you about is the address and the sentence, and asking
/// for a name before you know the thing works is the wrong order.
#[derive(serde::Deserialize)]
pub(super) struct RecipeTrialRequest {
    #[serde(default)]
    inputs: Vec<RecipeInputRequest>,
    get: String,
    say: String,
    /// A sample value per declared input, so the address can actually be filled.
    #[serde(default)]
    values: serde_json::Value,
}

pub(super) fn recipe_json(
    recipe: &endora_capabilities::Recipe,
    stance: endora_capabilities::Stance,
) -> serde_json::Value {
    json!({
        "id": recipe.id,
        "capability_id": recipe.capability_id(),
        "description": recipe.description,
        "inputs": recipe.inputs.iter()
            .map(|i| json!({ "name": i.name, "type": i.kind.as_str() }))
            .collect::<Vec<_>>(),
        "get": recipe.get,
        "say": recipe.say,
        "enabled": stance != endora_capabilities::Stance::Off,
    })
}

/// Every recipe the person has authored, with whether each is enabled.
pub(super) async fn list_recipes(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    use endora_capabilities::{CapabilityConfigRepository, RecipeRepository, Stance};
    let config = state.config.clone();
    let out = blocking(move || {
        let recipes = RecipeRepository::recipes(config.as_ref()).map_err(AppError::Repository)?;
        let stances: std::collections::HashMap<String, Stance> =
            CapabilityConfigRepository::stances(config.as_ref())
                .unwrap_or_default()
                .into_iter()
                .collect();
        Ok::<_, AppError>(
            recipes
                .iter()
                .map(|r| {
                    let stance = stances
                        .get(&r.capability_id())
                        .copied()
                        .unwrap_or(Stance::Off);
                    recipe_json(r, stance)
                })
                .collect::<Vec<_>>(),
        )
    })
    .await?;
    Ok(Json(out))
}

/// Authors (or edits) a recipe — the whole self-authoring sandbox's only way in
/// (ADR 0071). Validated at the domain layer (`Recipe::new`): a bad id, a
/// non-http(s) `get`, or a `get` slot naming no declared input is refused here,
/// before anything is stored, not discovered as a 400 the first time it runs.
///
/// A NEW recipe's stance is stamped `off` explicitly. The observe band's own
/// default is `Auto` — recipes override that once, at the one moment doing so is
/// unambiguous (ADR 0062's stored-beats-default, the same mechanism `trust_all`
/// already uses for MCP tools). Editing an EXISTING recipe leaves its stance
/// exactly as it was: enabling is the person's, never a side effect of a save.
pub(super) async fn create_recipe(
    State(state): State<AppState>,
    Json(req): Json<RecipeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::{
        CapabilityConfigRepository, Recipe, RecipeInput, RecipeInputKind, RecipeRepository, Stance,
    };
    let inputs = req
        .inputs
        .iter()
        .map(|i| {
            RecipeInputKind::from_name(&i.kind)
                .map(|kind| RecipeInput {
                    name: i.name.clone(),
                    kind,
                })
                .ok_or_else(|| {
                    ApiError(AppError::BadRequest {
                        message: format!(
                            "'{}' is not a recipe input type — use 'string' or 'number'",
                            i.kind
                        ),
                    })
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let recipe = Recipe::new(&req.id, &req.description, inputs, &req.get, &req.say)
        .map_err(|e| ApiError(AppError::Domain(e)))?;
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    blocking(move || {
        let capability_id = recipe.capability_id();
        let already_decided = CapabilityConfigRepository::stances(config.as_ref())
            .unwrap_or_default()
            .iter()
            .any(|(id, _)| *id == capability_id);
        RecipeRepository::save_recipe(config.as_ref(), &recipe).map_err(AppError::Repository)?;
        if !already_decided {
            CapabilityConfigRepository::set_stance(config.as_ref(), &capability_id, Stance::Off)
                .map_err(AppError::Repository)?;
        }
        record_event(
            events.as_ref(),
            clock.as_ref(),
            &format!("Authored the '{}' recipe", recipe.id),
        );
        Ok::<_, AppError>(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

/// Runs a draft recipe once and reports what came back (ADR 0071).
///
/// Nothing is stored and nothing is enabled: this is the authoring loop, where
/// the person sees the real answer and picks the field they want out of it
/// instead of guessing a JSON path. It runs the same guarded fetch a saved
/// recipe runs, so a trial cannot reach anywhere the real thing could not.
///
/// A failed fetch is a **200 with the failure described**, not a 4xx: "that
/// address didn't answer" is the trial's result, not a broken request.
pub(super) async fn try_recipe(
    Json(req): Json<RecipeTrialRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::{RecipeInput, RecipeInputKind};
    let inputs = req
        .inputs
        .iter()
        .map(|i| {
            RecipeInputKind::from_name(&i.kind)
                .map(|kind| RecipeInput {
                    name: i.name.clone(),
                    kind,
                })
                .ok_or_else(|| {
                    ApiError(AppError::BadRequest {
                        message: format!(
                            "'{}' is not a recipe input type — use 'string' or 'number'",
                            i.kind
                        ),
                    })
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let values = if req.values.is_object() {
        req.values.clone()
    } else {
        json!({})
    };
    let tried = blocking(move || {
        Ok::<_, AppError>(endora_capabilities::try_recipe(
            inputs, &req.get, &req.say, &values,
        ))
    })
    .await?;
    Ok(Json(match tried {
        Ok(trial) => json!({
            "ok": true,
            "url": trial.url,
            "fields": trial.fields.iter()
                .map(|(path, sample)| json!({ "path": path, "sample": sample }))
                .collect::<Vec<_>>(),
            "preview": trial.preview,
            "preview_error": trial.preview_error,
            "skipped_a_list": trial.skipped_a_list,
        }),
        Err(why) => json!({ "ok": false, "error": why }),
    }))
}

#[derive(Deserialize)]
pub(super) struct RecipeEnableRequest {
    enabled: bool,
}

/// Turns a recipe on or off — the ADR's one affirmative "enable" action, and the
/// one place recipes differ from the generic capability toggle: ON stores an
/// explicit `Auto` rather than clearing back to the band default, because the
/// band default (Observe) is `Auto` too — clearing would enable a recipe nobody
/// had turned on yet the moment anyone next cleared an unrelated stance.
pub(super) async fn set_recipe_enabled(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RecipeEnableRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::{CapabilityConfigRepository, RecipeRepository, Stance};
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    blocking(move || {
        let Some(recipe) =
            RecipeRepository::recipe(config.as_ref(), &id).map_err(AppError::Repository)?
        else {
            return Err(AppError::NotFound { entity: "recipe" });
        };
        let stance = if req.enabled {
            Stance::Auto
        } else {
            Stance::Off
        };
        CapabilityConfigRepository::set_stance(config.as_ref(), &recipe.capability_id(), stance)
            .map_err(AppError::Repository)?;
        record_event(
            events.as_ref(),
            clock.as_ref(),
            &format!(
                "{} the '{}' recipe",
                if req.enabled { "Enabled" } else { "Disabled" },
                recipe.id
            ),
        );
        Ok::<_, AppError>(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

/// Removes a recipe. Its stance goes with it in storage (`delete_recipe` already
/// cascades that), so a same-named recipe authored later starts at the same
/// `off` default as any other new one, never inheriting one it never earned.
pub(super) async fn remove_recipe(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::RecipeRepository;
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    blocking(move || {
        RecipeRepository::delete_recipe(config.as_ref(), &id).map_err(AppError::Repository)?;
        record_event(
            events.as_ref(),
            clock.as_ref(),
            &format!("Removed the '{id}' recipe"),
        );
        Ok::<_, AppError>(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

/// Removes an MCP server by name, then reconnects.
pub(super) async fn remove_mcp_server(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::McpServerRegistry;
    let config = state.config.clone();
    let mcp = state.mcp.clone();
    blocking(move || {
        McpServerRegistry::remove(config.as_ref(), &name).map_err(AppError::Repository)?;
        reconnect_mcp(config.as_ref(), mcp.as_ref());
        Ok(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

/// Reconnects a single already-registered MCP server: re-runs the handshake and
/// re-lists its tools, without touching its stored config. Useful when the server
/// was unreachable when it was added (or its backing service has only just come up)
/// and you want to retry without re-entering the URL and token. Reconnecting rebuilds
/// the whole MCP runner (cheap), then reports honestly how many tools this server now
/// exposes — `connected` is false, not an error, when it still didn't come up.
pub(super) async fn reconnect_mcp_server(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::{CapabilityRunner, McpServerRegistry};
    let config = state.config.clone();
    let mcp = state.mcp.clone();
    let lookup = name.clone();
    let known = blocking(move || {
        let known = McpServerRegistry::list(config.as_ref())
            .map_err(AppError::Repository)?
            .iter()
            .any(|s| s.name == lookup);
        // Only reconnect for a real server, so a typo'd name is a clean 404 rather
        // than a silent no-op that rebuilds the runner for nothing.
        if known {
            reconnect_mcp(config.as_ref(), mcp.as_ref());
        }
        Ok(known)
    })
    .await?;
    if !known {
        return Err(ApiError(AppError::NotFound {
            entity: "MCP server",
        }));
    }
    // How many tools this server exposes after the fresh handshake.
    let prefix = format!("{name}.");
    let tools_live = mcp_snapshot(&state)
        .available()
        .iter()
        .filter(|spec| spec.id.starts_with(&prefix))
        .count();
    let _ = state.changes.send(());
    Ok(Json(json!({
        "ok": true,
        "connected": tools_live > 0,
        "tools_live": tools_live,
    })))
}

#[derive(Deserialize)]
pub(super) struct TrustRequest {
    trust_all: bool,
}

/// Sets a server's auto-allow flag, then reconnects so it takes effect. With it on,
/// the reconnect opens every tool the server exposes (still Block→Confirm — the butler
/// asks before each use). Turning it off stops auto-opening future tools; tools already
/// allowed stay allowed until blocked individually, so the change is never a silent
/// widening of access.
#[derive(serde::Deserialize)]
pub(super) struct ReaderRequest {
    /// The tool on this server that reads its state. Blank clears the nomination.
    reader_tool: String,
}

/// Nominates which of a server's tools **reads its state** (ADR 0054).
///
/// One answer settles two things: that tool's own result becomes an observation rather
/// than a receipt, and every other tool on the server is verified through it. It comes
/// from the person, never from the server — a server's self-report is not evidence, and
/// policy must not take an unvetted third party's word (ADR 0051).
///
/// The nomination is validated against the tools the server actually exposes, so a typo
/// cannot quietly disable read-back.
pub(super) async fn set_mcp_reader(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<ReaderRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::{CapabilityRunner, McpServerRegistry};
    let config = state.config.clone();
    let mcp = state.mcp.clone();
    let lookup = name.clone();
    let reader = req.reader_tool.trim().to_owned();

    // Does this server exist at all? Answered first, so an unknown server reads as a
    // 404 rather than being judged on the tool name it could never have.
    let known_config = config.clone();
    let known_name = lookup.clone();
    let known = blocking(move || {
        Ok(McpServerRegistry::list(known_config.as_ref())
            .map_err(AppError::Repository)?
            .iter()
            .any(|s| s.name == known_name))
    })
    .await?;
    if !known {
        return Err(ApiError(AppError::NotFound {
            entity: "MCP server",
        }));
    }

    // A blank clears the nomination; anything else must be a tool this server really
    // exposes. A typo would fail by simply never verifying anything, which is the worst
    // way for a safety mechanism to break.
    if !reader.is_empty() {
        let offered = mcp.read().ok().is_some_and(|r| {
            r.available()
                .into_iter()
                .any(|c| c.id == format!("{lookup}.{reader}"))
        });
        if !offered {
            return Err(ApiError(AppError::BadRequest {
                message: format!("'{reader}' is not a tool this server exposes"),
            }));
        }
    }

    let stored = reader.clone();
    blocking(move || {
        let mut servers = McpServerRegistry::list(config.as_ref()).map_err(AppError::Repository)?;
        if let Some(server) = servers.iter_mut().find(|s| s.name == lookup) {
            server.reader_tool = stored;
            McpServerRegistry::register(config.as_ref(), server).map_err(AppError::Repository)?;
            reconnect_mcp(config.as_ref(), mcp.as_ref());
        }
        Ok(())
    })
    .await?;
    Ok(Json(json!({ "ok": true, "reader_tool": reader })))
}

pub(super) async fn set_mcp_trust(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<TrustRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::McpServerRegistry;
    let config = state.config.clone();
    let mcp = state.mcp.clone();
    let lookup = name.clone();
    let known = blocking(move || {
        let known = McpServerRegistry::list(config.as_ref())
            .map_err(AppError::Repository)?
            .iter()
            .any(|s| s.name == lookup);
        if known {
            McpServerRegistry::set_trust_all(config.as_ref(), &lookup, req.trust_all)
                .map_err(AppError::Repository)?;
            // The toggle moves every tool on the server, both ways.
            //
            // Off was already like this, for a good reason: auto-allow writes a per-tool
            // flag for every tool the server exposes and nothing reversed them, so switching
            // back to deny-by-default left twenty tools open, one of which plays audio
            // through the house. Every tool is closed rather than only the ones auto-allow
            // opened, because nothing records which was which.
            //
            // On did nothing but store the flag, leaving the opening to connect — which
            // deliberately skips any tool somebody has ruled on, and after an off that is
            // every one of them. A server sat reading "Allow all its tools: On" with all
            // eight of them blocked, and the only way back was eight separate taps: the same
            // one-way door the paragraph above closed, facing the other way.
            //
            // Connect is a **default** and must not overwrite a decision. This is a
            // **decision**, made by the person, about this server, just now.
            let available: Vec<String> =
                endora_capabilities::CapabilityRunner::available(&*mcp.read().unwrap().clone())
                    .into_iter()
                    .map(|spec| spec.id)
                    .collect();
            for id in endora_capabilities::tools_the_toggle_governs(&available, &lookup) {
                let stance = if req.trust_all {
                    endora_capabilities::Stance::Ask
                } else {
                    endora_capabilities::Stance::Off
                };
                let _ = endora_capabilities::CapabilityConfigRepository::set_stance(
                    config.as_ref(),
                    &id,
                    stance,
                );
            }
            reconnect_mcp(config.as_ref(), mcp.as_ref());
        }
        Ok(known)
    })
    .await?;
    if !known {
        return Err(ApiError(AppError::NotFound {
            entity: "MCP server",
        }));
    }
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true, "trust_all": req.trust_all })))
}
