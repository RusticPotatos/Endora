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
use endora_application::Clock;
use endora_application::{
    ActivityItem, AppError, AutonomyEnvelope, AutonomyEnvelopeRepository, BriefSchedule, Butler,
    ButlerModelConfig, ButlerModelConfigRepository, CapabilityConfigRepository,
    CapabilitySettingsRepository, CheckinSchedule, DeepModelRepository, MemorySnapshot, ModelSlot,
    ModelTuneScheduleRepository, RepositoryError, Sampling, usecases,
};
use endora_application::{
    AuditRecord, Belief, BeliefId, ChatMessage, Preference, PreferenceId, PreferenceKind,
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
    /// The persistence adapter (implements the repository ports not yet moved to
    /// their bounded context — ADR 0026).
    pub store: Arc<SqliteStore>,
    /// The conversation context's chat repository, over the shared connection.
    pub chat: Arc<endora_conversation::ChatStore>,
    /// The scheduling context's schedule repositories, over the shared connection.
    pub schedules: Arc<endora_scheduling::ScheduleStore>,
    /// The understanding context's belief + preference repositories.
    pub understanding: Arc<endora_understanding::UnderstandingStore>,
    /// The capabilities context's config store (settings/config/envelope/deep-model).
    pub config: Arc<endora_capabilities::ConfigStore>,
    /// The platform context's audit trail store.
    pub audit: Arc<endora_platform::AuditStore>,
    /// The platform context's event-log store.
    pub events: Arc<endora_platform::EventStore>,
    /// The identifier source.
    pub ids: Arc<RandomIdSource>,
    /// The system clock.
    pub clock: Arc<SystemClock>,
    /// The butler brain (proposes; never acts).
    pub butler: Arc<dyn Butler + Send + Sync>,
    /// Broadcasts a signal whenever a write succeeds, so activity-stream
    /// subscribers know to refresh. Carries no payload — it is a "something
    /// changed" nudge, and clients re-read the authoritative state.
    pub changes: broadcast::Sender<()>,
    /// The butler's skills (weather, web, …) — declared modules the butler can
    /// reach for, each gated by its autonomy level (ADR 0019).
    pub capabilities: Arc<Vec<Arc<dyn Capability>>>,
    /// Connected MCP servers, as a runner (ADR 0021). Long-lived (subprocesses),
    /// so it is connected once at startup and shared across turns; a registry
    /// change rebuilds it and swaps it in behind the lock. Merged with the built-in
    /// registry per turn (see [`build_runner`]).
    pub mcp: Arc<std::sync::RwLock<Arc<endora_capabilities::McpRunner>>>,
    /// Serializes butler *turns* (a chat reply, a proactive brief/check-in). The
    /// agentic loop reads history, calls the model several times, then appends —
    /// running two at once on one local model both thrashes it and lets turns
    /// cross-talk (one turn answering with another's context). A single lock makes
    /// every turn atomic, so a heartbeat brief can't interleave with a chat.
    pub turn_lock: Arc<tokio::sync::Mutex<()>>,
    /// The running conversation summary (ADR 0028 context compaction), which keeps the
    /// chat prompt bounded on a long conversation without dropping the day's thread.
    /// Persisted (SQLite), so a restart doesn't re-summarise the whole backlog — on a
    /// slow local model that catch-up degraded the first turns after every deploy — and
    /// the butler keeps the day's thread across restarts.
    pub summary: PersistentSummary,
}

/// Persistent [`ConversationSummaryStore`](endora_application::ConversationSummaryStore):
/// the single running summary, stored in SQLite via the conversation
/// [`ChatStore`](endora_conversation::ChatStore) (Endora is single-conversation).
#[derive(Clone)]
pub struct PersistentSummary(Arc<endora_conversation::ChatStore>);

impl endora_application::ConversationSummaryStore for PersistentSummary {
    fn get(&self) -> Option<endora_application::ConversationSummary> {
        self.0
            .load_summary()
            .ok()
            .flatten()
            .map(|(text, covered)| endora_application::ConversationSummary { text, covered })
    }
    fn set(&self, summary: endora_application::ConversationSummary) {
        // Best-effort: a failed summary write just means the next turn re-summarises;
        // it must never break the turn.
        let _ = self.0.save_summary(&summary.text, summary.covered);
    }
}

impl AppState {
    /// Creates the shared state, wiring up the change-broadcast channel.
    #[must_use]
    pub fn new(
        store: Arc<SqliteStore>,
        ids: Arc<RandomIdSource>,
        clock: Arc<SystemClock>,
        butler: Arc<dyn Butler + Send + Sync>,
    ) -> Self {
        // A small buffer is plenty: subscribers coalesce to a single refresh,
        // and a lagged receiver still gets one "changed" signal.
        let (changes, _) = broadcast::channel(16);
        // Context stores share the one connection the store opened (ADR 0026).
        let chat = Arc::new(endora_conversation::ChatStore::new(store.db()));
        // The running summary is persisted through the same chat store (SQLite).
        let summary = PersistentSummary(chat.clone());
        let schedules = Arc::new(endora_scheduling::ScheduleStore::new(store.db()));
        let understanding = Arc::new(endora_understanding::UnderstandingStore::new(store.db()));
        let config = Arc::new(endora_capabilities::ConfigStore::new(store.db()));
        let audit = Arc::new(endora_platform::AuditStore::new(store.db()));
        let events = Arc::new(endora_platform::EventStore::new(store.db()));
        // Deployment policy: if a skills config file is set, apply its per-skill modes
        // at startup as the baseline (ADR 0024) — before MCP connects, so MCP-tool
        // modes are in place too.
        if let Ok(path) = std::env::var("ENDORA_SKILLS_CONFIG") {
            if !path.trim().is_empty() {
                apply_skills_config(config.as_ref(), path.trim());
            }
        }
        // Connect any registered MCP servers up front (subprocesses persist across
        // turns). A server that fails to start is skipped, so startup never blocks
        // on a bad one (ADR 0021).
        let mcp = Arc::new(std::sync::RwLock::new(Arc::new(connect_mcp(
            config.as_ref(),
        ))));
        Self {
            store,
            chat,
            schedules,
            understanding,
            config,
            audit,
            events,
            ids,
            clock,
            butler,
            changes,
            capabilities: Arc::new(endora_infrastructure::default_capabilities()),
            mcp,
            turn_lock: Arc::new(tokio::sync::Mutex::new(())),
            summary,
        }
    }
}

/// Applies a deployment **skills config file** (ADR 0024): a JSON object mapping a
/// skill id (built-in, or an MCP tool `server.tool`) to a mode — `"off"`, `"auto"`,
/// or `"ask"`. Applied at startup as the baseline for the listed skills; skills not
/// listed are left to the person's choices. `auto` = enabled, runs per its band;
/// `ask` = enabled but confirmed each use (and, for the un-undoable, allowed to run
/// with confirmation); `off` = disabled. Missing/invalid files are logged and
/// skipped — a bad config never stops the node from starting.
fn apply_skills_config(config: &endora_capabilities::ConfigStore, path: &str) {
    use endora_capabilities::CapabilityConfigRepository;
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("skills config: can't read {path}: {e} — skipping");
            return;
        }
    };
    let map: std::collections::BTreeMap<String, String> = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("skills config: {path} is not a JSON object of id→mode ({e}) — skipping");
            return;
        }
    };
    for (id, mode) in map {
        match mode.as_str() {
            "off" => {
                let _ = config.set_enabled(&id, false);
            }
            "auto" => {
                let _ = config.set_enabled(&id, true);
                let _ = config.set_confirm(&id, false);
                let _ = config.set_open_irreversible(&id, false);
            }
            // Ask first, band-agnostic: confirm downgrades an autonomous read, and
            // opening lets an un-undoable run with confirmation. Both together give
            // "on with user input" for any band.
            "ask" => {
                let _ = config.set_enabled(&id, true);
                let _ = config.set_confirm(&id, true);
                let _ = config.set_open_irreversible(&id, true);
            }
            other => {
                eprintln!("skills config: unknown mode '{other}' for '{id}' (use off|auto|ask)");
            }
        }
    }
}

/// Connects to every **enabled stdio** MCP server in the registry, returning a
/// runner over the ones that came up (ADR 0021). A server whose process fails to
/// start or handshake is skipped — its tools simply don't appear — so one bad server
/// can't break startup or a turn. HTTP transport is a later slice.
fn connect_mcp(config: &endora_capabilities::ConfigStore) -> endora_capabilities::McpRunner {
    use endora_capabilities::{
        CapabilityConfigRepository, CapabilityRunner, HttpMcpClient, McpClient, McpServerRegistry,
        McpTransport, StdioMcpClient,
    };
    let servers = config.list().unwrap_or_default();
    // Namespacing prefixes of servers whose tools should be auto-allowed on connect.
    let trusted: Vec<String> = servers
        .iter()
        .filter(|s| s.enabled && s.trust_all)
        .map(|s| format!("{}.", s.name))
        .collect();
    // Each server carries the tool the person nominated as its state reader (ADR 0038),
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
    // Auto-allow: for a server marked trust_all, open every tool it exposes so the
    // butler can use them without per-tool clicking. Opened MCP tools remain
    // Block→Confirm — it still asks before each use (ADR 0024). This is deterministic
    // policy set in code from a stored flag, never routed from model output.
    if !trusted.is_empty() {
        for spec in runner.available() {
            if trusted.iter().any(|prefix| spec.id.starts_with(prefix)) {
                let _ = config.set_open_irreversible(&spec.id, true);
            }
        }
    }
    runner
}

/// Rebuilds the connected-MCP runner from the current registry and swaps it into
/// `slot`. Called after a registry change so new/removed servers take effect without
/// a restart. Blocking (spawns subprocesses) — run it off the async path.
fn reconnect_mcp(
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
fn reconnect_empty_mcp_servers(state: &AppState) {
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
fn mcp_snapshot(state: &AppState) -> Arc<endora_capabilities::McpRunner> {
    match state.mcp.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

#[derive(Deserialize)]
struct CatalogQuery {
    #[serde(default)]
    q: String,
}

/// Searches the MCP catalog: the curated entries shipped with Endora, plus — best
/// effort — the community registry (ADR 0021). Results prefill the "Add a server"
/// form and stay editable, so a stale launch command can be corrected before it is
/// registered. Registry lookup is opportunistic: if it is unreachable, misconfigured,
/// or replies in a shape we don't recognise, the curated results still come back and
/// `registry_ok` says what happened.
async fn search_mcp_catalog(Query(q): Query<CatalogQuery>) -> Json<serde_json::Value> {
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
/// currently live (ADR 0021). A server with 0 live tools is registered but didn't
/// connect (bad command, unreachable, or disabled).
async fn list_mcp_servers(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::{
        CapabilityConfigRepository, CapabilityRunner, McpServerRegistry, McpTransport,
    };
    let config = state.config.clone();
    let (servers, opened, enabled) = blocking(move || {
        Ok((
            config.list().map_err(AppError::Repository)?,
            config.opened_overrides().map_err(AppError::Repository)?,
            config.enabled_overrides().map_err(AppError::Repository)?,
        ))
    })
    .await?;
    // Tools the person has turned off entirely (ADR 0040) — not offered to the butler at
    // all, which is a different state from blocked and must be visible as such.
    let withdrawn: std::collections::HashSet<String> = enabled
        .into_iter()
        .filter(|(_, on)| !*on)
        .map(|(id, _)| id)
        .collect();
    let opened: std::collections::HashSet<String> = opened
        .into_iter()
        .filter(|(_, o)| *o)
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
            // confirm-each-use). Un-opened MCP tools are visible but blocked (ADR 0024).
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

fn default_enabled() -> bool {
    true
}

#[derive(Deserialize)]
struct McpServerRequest {
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
/// band-classified before they can run (ADR 0021/0024).
async fn register_mcp_server(
    State(state): State<AppState>,
    Json(req): Json<McpServerRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use endora_capabilities::{McpServer, McpServerRegistry, McpTransport};
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

/// Removes an MCP server by name, then reconnects.
async fn remove_mcp_server(
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
async fn reconnect_mcp_server(
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
struct TrustRequest {
    trust_all: bool,
}

/// Sets a server's auto-allow flag, then reconnects so it takes effect. With it on,
/// the reconnect opens every tool the server exposes (still Block→Confirm — the butler
/// asks before each use). Turning it off stops auto-opening future tools; tools already
/// allowed stay allowed until blocked individually, so the change is never a silent
/// widening of access.
#[derive(serde::Deserialize)]
struct ReaderRequest {
    /// The tool on this server that reads its state. Blank clears the nomination.
    reader_tool: String,
}

/// Nominates which of a server's tools **reads its state** (ADR 0038).
///
/// One answer settles two things: that tool's own result becomes an observation rather
/// than a receipt, and every other tool on the server is verified through it. It comes
/// from the person, never from the server — a server's self-report is not evidence, and
/// policy must not take an unvetted third party's word (ADR 0005).
///
/// The nomination is validated against the tools the server actually exposes, so a typo
/// cannot quietly disable read-back.
async fn set_mcp_reader(
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

async fn set_mcp_trust(
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

/// Builds the router for the node's HTTP API.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/styles.css", get(console_css))
        .route("/app.js", get(console_js))
        .route("/health", get(health))
        .route("/v1/chat", post(send_chat).get(chat_history))
        .route("/v1/chat/stream", post(stream_chat))
        .route("/v1/checkin", get(get_checkin).post(set_checkin))
        .route("/v1/understanding", get(list_understanding))
        .route("/v1/understanding/{id}/affirm", post(affirm_belief))
        .route("/v1/understanding/{id}/correct", post(correct_belief))
        .route("/v1/outcomes", get(list_outcomes))
        .route("/v1/repairs", get(list_repairs))
        .route("/v1/aliases", get(list_aliases).post(set_alias))
        .route("/v1/aliases/upstream", post(push_aliases_upstream))
        .route("/v1/outcomes/{id}/reaction", post(react_to_outcome))
        // Read and drop only. There is deliberately NO create or edit route: Endora
        // forms its own intentions, and the person's whole side of the interface is
        // "stop doing that" (ADR 0036).
        .route("/v1/intentions", get(list_intentions))
        .route("/v1/intentions/{id}/drop", post(drop_intention))
        .route("/v1/capabilities", get(list_capabilities))
        .route("/v1/capabilities/{id}/invoke", post(invoke_capability))
        .route("/v1/capabilities/{id}/enable", post(set_capability_enabled))
        .route("/v1/capabilities/{id}/open", post(set_capability_open))
        .route(
            "/v1/capabilities/{id}/confirm",
            post(set_capability_confirm),
        )
        .route("/v1/mcp/catalog", get(search_mcp_catalog))
        .route(
            "/v1/mcp/servers",
            get(list_mcp_servers).post(register_mcp_server),
        )
        .route(
            "/v1/mcp/servers/{name}",
            axum::routing::delete(remove_mcp_server),
        )
        .route(
            "/v1/mcp/servers/{name}/reconnect",
            post(reconnect_mcp_server),
        )
        .route("/v1/mcp/servers/{name}/trust", post(set_mcp_trust))
        .route("/v1/mcp/servers/{name}/reader", post(set_mcp_reader))
        .route(
            "/v1/capabilities/{id}/config",
            post(set_capability_settings),
        )
        .route("/v1/autonomy", get(get_autonomy).post(set_autonomy))
        .route("/v1/brief", post(brief))
        .route(
            "/v1/brief/schedule",
            get(get_brief_schedule).post(set_brief_schedule),
        )
        .route(
            "/v1/nightly-loop/schedule",
            get(get_nightly_loop_schedule).post(set_nightly_loop_schedule),
        )
        .route("/v1/deep-model", get(get_deep_model).post(set_deep_model))
        .route(
            "/v1/model-config",
            get(get_model_config).post(set_model_config),
        )
        .route("/v1/models/discover", post(discover_models))
        .route("/v1/models/test", post(test_model_connection))
        .route("/v1/model-layer/run", post(run_model_layer_now))
        .route(
            "/v1/model-tune/schedule",
            get(get_tune_schedule).post(set_tune_schedule),
        )
        .route("/v1/transcribe", post(transcribe))
        .route("/v1/deep-ask", post(deep_ask))
        .route(
            "/v1/preferences",
            post(create_preference).get(list_preferences),
        )
        .route(
            "/v1/preferences/{id}",
            axum::routing::delete(delete_preference),
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

/// Serves the web console's HTML shell (embedded in the binary; see ADR 0009).
/// The styles and script are separate files (`/styles.css`, `/app.js`) so the
/// console is organized by responsibility, not one giant file — still embedded,
/// still no build step.
async fn index() -> impl axum::response::IntoResponse {
    // Cache-bust the assets per build: the shell is always revalidated (no-cache),
    // and it points at `/app.js?v=<build>` / `/styles.css?v=<build>` so a deploy
    // serves fresh script/styles even to a browser that heuristically cached the
    // old ones (the assets themselves are then immutable per build, below).
    let v = endora_application::build_id();
    let html = include_str!("web/index.html")
        .replace("/app.js", &format!("/app.js?v={v}"))
        .replace("/styles.css", &format!("/styles.css?v={v}"));
    (
        [(axum::http::header::CACHE_CONTROL, "no-cache")],
        Html(html),
    )
}

/// The console's stylesheet (embedded). Immutable per build — the shell versions
/// the URL, so a new build fetches a new URL rather than a stale cached one.
async fn console_css() -> impl axum::response::IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable",
            ),
        ],
        include_str!("web/styles.css"),
    )
}

/// The console's script (embedded). Immutable per build (see [`console_css`]).
async fn console_js() -> impl axum::response::IntoResponse {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable",
            ),
        ],
        include_str!("web/app.js"),
    )
}

async fn health() -> Json<serde_json::Value> {
    let stt = std::env::var("ENDORA_STT_URL")
        .ok()
        .is_some_and(|s| !s.trim().is_empty());
    Json(json!({
        "status": "ok",
        "service": endora_application::platform_identity(),
        "version": endora_application::version(),
        "build": endora_application::build_id(),
        // Whether a speech-to-text server is configured, so the console can use
        // real transcription for push-to-talk instead of the browser's flaky one.
        "stt": stt,
    }))
}

/// Whisper (and similar STT) hallucinate a stock phrase repeated many times when
/// handed near-silent or non-speech audio — e.g. "Torsdagsfotografi" twenty times in
/// a row. Real speech varies; a hallucinated loop is one or two tokens over and over.
/// We treat such a transcript as nothing said (so conversation mode keeps listening
/// and push-to-talk inserts nothing) rather than send it as if the person spoke it.
fn looks_like_stt_hallucination(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 6 {
        return false;
    }
    let distinct: std::collections::HashSet<String> =
        words.iter().map(|w| w.to_lowercase()).collect();
    (distinct.len() as f64) / (words.len() as f64) <= 0.34
}

/// Transcribes a recording (raw audio bytes in the body) via the configured
/// speech-to-text server (`ENDORA_STT_URL`, OpenAI-compatible). The node proxies
/// it so the STT host is never exposed to the page; 503 when none is configured.
async fn transcribe(body: axum::body::Bytes) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(url) = std::env::var("ENDORA_STT_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
    else {
        return Err(ApiError(AppError::Model {
            message: "no speech-to-text server configured".to_owned(),
        }));
    };
    let text = blocking(move || {
        endora_infrastructure::transcribe_audio(url.trim(), &body)
            .map_err(|e| AppError::Model { message: e })
    })
    .await?;
    // Drop the classic silence-hallucination so it never reaches the conversation.
    let text = if looks_like_stt_hallucination(&text) {
        String::new()
    } else {
        text
    };
    Ok(Json(json!({ "text": text })))
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
    let audit = state.audit.clone();
    let records = blocking(move || usecases::recent_audit(audit.as_ref(), limit)).await?;
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
    let audit = state.audit.clone();
    let events = state.events.clone();
    let items =
        blocking(move || usecases::recent_activity(audit.as_ref(), events.as_ref(), limit)).await?;
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

/// Builds a capability runner that honours the person's enable/disable choices
/// (ADR 0021) and their autonomy envelope (ADR 0022). Reading config is
/// best-effort: on failure it falls back to defaults, so a glitch never breaks the
/// butler.
/// Records one line to the butler's action log (best-effort — transparency, not
/// the critical path, so a failure never breaks the turn).
fn record_event(events: &endora_platform::EventStore, clock: &SystemClock, summary: &str) {
    use endora_application::{Clock, EventLog};
    let _ = events.record(clock.now(), summary);
}

fn build_runner(
    config: &endora_capabilities::ConfigStore,
    capabilities: Arc<Vec<Arc<dyn Capability>>>,
    mcp: Arc<endora_capabilities::McpRunner>,
) -> endora_capabilities::WithdrawnRunner {
    let overrides = config.enabled_overrides().unwrap_or_default();
    // Everything the person has turned off, whatever kind of capability it is. The
    // built-in registry applies its own flag below; an MCP tool had no equivalent, so
    // "off" silently did nothing to it (ADR 0040).
    let withdrawn: std::collections::HashSet<String> = overrides
        .iter()
        .filter(|(_, enabled)| !*enabled)
        .map(|(id, _)| id.clone())
        .collect();
    let opened = config.opened_overrides().unwrap_or_default();
    let confirm = config.confirm_overrides().unwrap_or_default();
    let envelope = AutonomyEnvelopeRepository::get(config).unwrap_or_default();
    // Whether the person allowed acting on consequential things on its own — an opened
    // MCP tool may then run in the loop rather than only confirm-each-use.
    let auto_consequential = envelope.auto_consequential;
    // The tools the person has opened this turn (ADR 0024) — shared by the built-in
    // registry and the MCP overlay below.
    let mcp_opened: std::collections::HashSet<String> = opened
        .iter()
        .filter(|(_, open)| *open)
        .map(|(id, _)| id.clone())
        .collect();
    // Fresh per turn so config/envelope/opener/ask-first changes take effect at once.
    let registry = endora_infrastructure::RegistryRunner::with_config(
        capabilities,
        overrides,
        opened,
        confirm,
        envelope,
        settings_map(config),
    );
    // Apply the same openers to the shared MCP runner's deny-by-default: an opened
    // MCP tool becomes confirm-each-use this turn, without rebuilding the connection.
    let mcp_source = endora_capabilities::OpenerRunner::new(
        mcp as Arc<dyn endora_capabilities::CapabilityRunner + Send + Sync>,
        mcp_opened,
        auto_consequential,
    );
    // Confirmed target aliases (ADR 0039), so a call that fails on a name the person has
    // already explained gets one retry with their answer — see `AliasRunner`.
    let aliases: Vec<(String, String, String)> =
        endora_capabilities::TargetAliasRepository::aliases(config)
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.server, a.said, a.means))
            .collect();
    // Built-in skills + connected MCP servers, behind one runner. The application
    // never learns a tool's origin (ADR 0021).
    // Confirmed answers first, then observed ones — ADR 0038's trust ranking, expressed
    // as the order of recovery: the alias the person gave is tried before Endora goes
    // looking through the server's own reading for a name that resembles the request.
    let recovers = endora_capabilities::TargetSearchRunner::with_channels(
        Arc::new(endora_capabilities::AliasRunner::new(
            Arc::new(mcp_source) as Arc<dyn endora_capabilities::CapabilityRunner + Send + Sync>,
            aliases,
        )) as Arc<dyn endora_capabilities::CapabilityRunner + Send + Sync>,
        native_channels(config),
    );
    let composite = endora_capabilities::CompositeRunner::new(vec![
        Arc::new(registry) as Arc<dyn endora_capabilities::CapabilityRunner + Send + Sync>,
        Arc::new(recovers) as Arc<dyn endora_capabilities::CapabilityRunner + Send + Sync>,
    ]);
    // Outermost, so a withdrawn capability is off the menu regardless of which source
    // offered it or what any inner layer would have decided.
    endora_capabilities::WithdrawnRunner::new(
        Arc::new(composite) as Arc<dyn endora_capabilities::CapabilityRunner + Send + Sync>,
        withdrawn,
    )
}

/// The runner for turns that are only ever allowed to *gather* — the heartbeat's
/// check-in, brief, and nightly loop, plus an on-demand brief.
///
/// Same catalog as [`build_runner`], with autonomy clamped to the reversible bands. The
/// person's openers and a widened envelope clear an actuator to run while they are
/// *present*, watching the activity trail and able to say stop; none of that is true at
/// 03:00. Keeps the nightly loop's guarantee — "nothing it could do that it couldn't
/// undo" — enforced rather than merely documented.
fn build_reversible_only_runner(
    config: &endora_capabilities::ConfigStore,
    capabilities: Arc<Vec<Arc<dyn Capability>>>,
    mcp: Arc<endora_capabilities::McpRunner>,
) -> endora_capabilities::ReversibleOnlyRunner {
    endora_capabilities::ReversibleOnlyRunner::new(Arc::new(build_runner(
        config,
        capabilities,
        mcp,
    )))
}

/// The servers Endora has **direct reach** into (ADR 0042) — its own connection to a
/// service, alongside whatever tool surface that service exposes to the model.
///
/// Home Assistant's is built from the URL and long-lived token already stored against the
/// `home_assistant` skill, so nothing has to be entered twice. Absent either, there is no
/// channel and everything behaves exactly as it did before.
///
/// The server name must match the MCP server's, since that is how a failing tool id is
/// traced back to the service that owns it.
fn native_channels(
    config: &endora_capabilities::ConfigStore,
) -> Vec<(String, Arc<dyn endora_capabilities::NativeChannel>)> {
    let settings = settings_map(config);
    let Some(home_settings) = settings.get("home_assistant") else {
        return Vec::new();
    };
    let Some(home) = endora_capabilities::HomeAssistant::from_settings(home_settings) else {
        return Vec::new();
    };
    vec![(
        endora_capabilities::paired_server(home_settings),
        Arc::new(home) as Arc<dyn endora_capabilities::NativeChannel>,
    )]
}

/// Loads all capability settings, grouped by capability id, for the runner.
fn settings_map(
    config: &endora_capabilities::ConfigStore,
) -> std::collections::HashMap<String, endora_infrastructure::CapabilitySettings> {
    let mut map: std::collections::HashMap<String, endora_infrastructure::CapabilitySettings> =
        std::collections::HashMap::new();
    for (id, key, value) in config.all_settings().unwrap_or_default() {
        map.entry(id).or_default().insert(key, value);
    }
    map
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
}

/// Sends a message to the butler; returns its reply and what it did this turn.
async fn send_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chat = state.chat.clone();
    let understanding = state.understanding.clone();
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
        let runner = build_runner(config.as_ref(), capabilities, mcp);
        // Ground the butler in the person's current life before it answers.
        let context = usecases::butler_context(
            understanding.as_ref(),
            understanding.as_ref(),
            config.as_ref(),
            &runner,
            clock.as_ref(),
        )?;
        let out = usecases::send_to_butler(
            chat.as_ref(),
            understanding.as_ref(),
            understanding.as_ref(),
            understanding.as_ref(),
            &runner,
            butler.as_ref(),
            audit.as_ref(),
            ids.as_ref(),
            clock.as_ref(),
            &context,
            &req.message,
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
/// writes it from what it actually gathered (ADR 0028). Returns the posted message,
/// or a note if it had nothing worth saying.
async fn brief(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let chat = state.chat.clone();
    let understanding = state.understanding.clone();
    let config = state.config.clone();
    let events = state.events.clone();
    let audit = state.audit.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let capabilities = state.capabilities.clone();
    let butler = state.butler.clone();
    let mcp = mcp_snapshot(&state);
    let result = blocking(move || {
        // A brief is an act of service, defined as reversible — it gathers and writes,
        // it never actuates. Enforced, not just documented above.
        let runner = build_reversible_only_runner(config.as_ref(), capabilities, mcp);
        let context = usecases::butler_context(
            understanding.as_ref(),
            understanding.as_ref(),
            config.as_ref(),
            &runner,
            clock.as_ref(),
        )?;
        let out = usecases::daily_brief(
            chat.as_ref(),
            understanding.as_ref(),
            understanding.as_ref(),
            &runner,
            butler.as_ref(),
            audit.as_ref(),
            ids.as_ref(),
            clock.as_ref(),
            &context,
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
            json!({ "briefed": false, "note": "Set your home location, and enable weather/news/safety, to get a brief." }),
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
async fn stream_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let chat = state.chat.clone();
    let understanding = state.understanding.clone();
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
    let message = req.message;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Event>();

    // Serialize this turn against any other butler turn (a concurrent chat, or a
    // heartbeat brief). The guard is held on the async task across the whole
    // blocking turn — so a second turn waits here until this one finishes.
    let turn_lock = state.turn_lock.clone();
    tokio::task::spawn(async move {
        let _turn = turn_lock.lock_owned().await;
        tokio::task::spawn_blocking(move || {
            let runner = build_runner(config.as_ref(), capabilities, mcp);
            // The deeper (bigger/cloud) rung of the capability ladder, if the person
            // configured one — the turn escalates to it only when the local model
            // comes up empty (ADR 0027).
            let deep = DeepModelRepository::get(config.as_ref())
                .ok()
                .flatten()
                .map(|d| endora_infrastructure::DeepModelAsker::new(d.url, d.model, d.api_key));
            let event = |v: serde_json::Value| Event::default().data(v.to_string());
            let context = match usecases::butler_context(
                understanding.as_ref(),
                understanding.as_ref(),
                config.as_ref(),
                &runner,
                clock.as_ref(),
            ) {
                Ok(c) => c,
                Err(e) => {
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
            // reply ends up claiming (ADR 0037).
            let mut disclosed: Vec<usecases::ActionDisclosure> = Vec::new();
            let result = {
                let mut on_token = |chunk: &str| {
                    let _ = tx.send(event(json!({ "type": "token", "text": chunk })));
                };
                // Structured action steps (router → skill → result), surfaced live so
                // the console can show an expandable trail of what the butler is doing,
                // separate from the reply prose.
                let mut on_step = |step: usecases::ButlerStep| {
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
                usecases::send_to_butler_streaming(
                    chat.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    &runner,
                    butler.as_ref(),
                    audit.as_ref(),
                    deep.as_ref()
                        .map(|d| d as &dyn endora_application::DeepAsker),
                    Some(&summary as &dyn endora_application::ConversationSummaryStore),
                    ids.as_ref(),
                    clock.as_ref(),
                    &context,
                    &message,
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
                    // The deterministic half of honesty about actions (ADR 0037): the
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
                            })
                        })
                        .collect();
                    let actions =
                        json!({ "steps": steps, "sources": sources, "actions_taken": disclosures });
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
fn fold_step(trail: &mut Vec<serde_json::Value>, step: serde_json::Value) {
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
fn sources_from_steps(steps: &[serde_json::Value]) -> Vec<String> {
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
async fn chat_history(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let chat = state.chat.clone();
    let (messages, actions) = blocking(move || {
        let msgs = usecases::chat_history(chat.as_ref())?;
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
    let schedules = state.schedules.clone();
    let clock = state.clock.clone();
    let schedule =
        blocking(move || usecases::checkin_schedule(schedules.as_ref(), clock.as_ref())).await?;
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
    let schedules = state.schedules.clone();
    let clock = state.clock.clone();
    let schedule = blocking(move || {
        usecases::set_checkin_schedule(
            schedules.as_ref(),
            clock.as_ref(),
            req.enabled,
            req.interval_ms,
        )
    })
    .await?;
    Ok(Json(schedule.into()))
}

/// Serializes a belief for the console. `confidence` is the **decayed** value — how
/// sure Endora is right now, given how long since anything reinforced it (ADR 0032) —
/// not the value frozen at the moment it was formed. Showing the stored one would
/// present a year-old guess as current.
fn belief_json(b: &Belief, now: endora_application::Timestamp) -> serde_json::Value {
    let confidence = b.confidence_at(now).unwrap_or(b.confidence());
    json!({
        "id": b.id().value().to_string(),
        "statement": b.statement(),
        "kind": b.kind().name(),
        "confidence": confidence.name(),
        "evidence": b.evidence(),
        "last_affirmed_ms": b.last_affirmed_at().unix_millis(),
    })
}

/// Serializes a belief **exactly as stored**, for the export. The memory right is to
/// see what Endora actually holds, so the export must not reinterpret it: it reports
/// the recorded confidence, where the live view reports the decayed one.
fn stored_belief_json(b: &Belief) -> serde_json::Value {
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
    let understanding = state.understanding.clone();
    let clock = state.clock.clone();
    let now = clock.now();
    let items =
        blocking(move || usecases::understanding(understanding.as_ref(), clock.as_ref())).await?;
    Ok(Json(items.iter().map(|b| belief_json(b, now)).collect()))
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
    let understanding = state.understanding.clone();
    let clock = state.clock.clone();
    let now = clock.now();
    let b = blocking(move || usecases::affirm_belief(understanding.as_ref(), clock.as_ref(), bid))
        .await?;
    Ok(Json(belief_json(&b, now)))
}

/// The person says a belief is wrong — drop it from understanding.
async fn correct_belief(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let bid = parse_belief_id(&id)?;
    let understanding = state.understanding.clone();
    blocking(move || usecases::correct_belief(understanding.as_ref(), bid)).await?;
    Ok(Json(json!({ "corrected": true })))
}

fn intention_json(i: &endora_application::Intention) -> serde_json::Value {
    json!({
        "id": i.id().value().to_string(),
        "statement": i.statement(),
        // Never null: an intention that cannot be explained cannot exist (ADR 0036).
        "motivating_belief": i.motivating_belief().value().to_string(),
        "note": i.note(),
        "state": i.state().name(),
        "active": i.is_active(),
        "steps_taken": i.steps_taken(),
        "created_ms": i.created_at().unix_millis(),
        "last_progressed_ms": i.last_progressed_at().unix_millis(),
    })
}

/// What Endora is pursuing, and what it has pursued before (ADR 0036).
async fn list_intentions(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let understanding = state.understanding.clone();
    let items = blocking(move || usecases::intentions(understanding.as_ref())).await?;
    Ok(Json(items.iter().map(intention_json).collect()))
}

/// The person tells Endora to stop working on something — their whole authority over
/// an intention, and the only verb they have (ADR 0036).
async fn drop_intention(
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

/// How many outcomes the console shows — recent history, not an archive to manage.
const OUTCOMES_SHOWN: usize = 30;

fn outcome_json(o: &endora_application::Outcome) -> serde_json::Value {
    json!({
        "id": o.id().value().to_string(),
        "capability": o.capability(),
        "input": o.input(),
        // The tool's own account and what Endora saw are kept apart here exactly as
        // they are in storage (ADR 0035) — the console must not merge them either.
        "claim": o.claim(),
        "observation": o.observation(),
        "observed": o.was_observed(),
        // Whether the world actually moved (ADR 0039). `null` means there was nothing
        // to compare — no reader, or the action never ran.
        "changed": o.changed(),
        "at_ms": o.at().unix_millis(),
        "motivating_belief": o.motivating_belief().map(|b| b.value().to_string()),
        "reaction": o.reaction().map(endora_application::Reaction::name),
    })
}

#[derive(serde::Deserialize)]
struct AliasRequest {
    server: String,
    said: String,
    means: String,
}

/// What the person has told Endora its tools' targets are really called (ADR 0039).
async fn list_aliases(
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

/// The person answers what Endora asked: this target is really called that (ADR 0039).
///
/// The **confirmed** source in ADR 0038's ranking, and the only one policy trusts.
/// Endora never fills this in from a server's text — that is the per-integration
/// parsing 0038 exists to stop.
async fn set_alias(
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

/// Writes the names the person has confirmed back into the service that owns them
/// (ADR 0043).
///
/// A confirmed alias currently helps only Endora. The same fact written into Home
/// Assistant's own registry helps **everything** that talks to that house — its app, its
/// voice assistants, anything else — and stops the failure at the source rather than
/// recovering from it every time.
///
/// Deliberately only what the person already confirmed. Endora does not invent names here.
async fn push_aliases_upstream(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let taught = blocking(move || {
        let channels = native_channels(config.as_ref());
        let aliases = endora_capabilities::TargetAliasRepository::aliases(config.as_ref())
            .map_err(AppError::Repository)?;
        let mut said: Vec<serde_json::Value> = Vec::new();
        for alias in aliases {
            let Some((_, channel)) = channels.iter().find(|(name, _)| *name == alias.server) else {
                continue;
            };
            let result = channel.teach(&alias.means, &alias.said);
            said.push(match result {
                Some(Ok(what)) => json!({ "alias": alias.said, "of": alias.means, "done": what }),
                Some(Err(why)) => {
                    json!({ "alias": alias.said, "of": alias.means, "failed": why })
                }
                None => json!({
                    "alias": alias.said,
                    "of": alias.means,
                    "skipped": "Endora is not allowed to write names into this service",
                }),
            });
        }
        Ok(said)
    })
    .await?;
    Ok(Json(json!({ "taught": taught })))
}

/// What Endora has noticed is wrong with its own tooling (ADR 0039).
///
/// Derived on read, never stored — there is nothing here to dismiss or process.
async fn list_repairs(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let understanding = state.understanding.clone();
    let config = state.config.clone();
    let (found, withdrawn) = blocking(move || {
        use endora_capabilities::CapabilityConfigRepository;
        let found = usecases::repairs(understanding.as_ref())?;
        let withdrawn: std::collections::HashSet<String> = config
            .enabled_overrides()
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, on)| !*on)
            .map(|(id, _)| id)
            .collect();
        Ok((found, withdrawn))
    })
    .await?;
    Ok(Json(
        found
            .iter()
            // A tool that is already turned off can never produce new evidence, so its
            // finding would sit there forever asking for something already done. The
            // derivation stays pure and unaware of config (ADR 0039); this is the one
            // place that knows both, and answering the question is what retires the card.
            .filter(|r| {
                r.remedy != endora_understanding::Remedy::StopOfferingIt
                    || !withdrawn.contains(&r.capability)
            })
            .map(|r| {
                json!({
                    "capability": r.capability,
                    "target": r.target,
                    "attempts": r.attempts,
                    // What would actually fix it (ADR 0040). The console offers a
                    // different control for each, because "what is it really called?"
                    // is the wrong question about a tool that has never worked at all.
                    "remedy": match r.remedy {
                        endora_understanding::Remedy::NameTheTarget => "name_the_target",
                        endora_understanding::Remedy::StopOfferingIt => "stop_offering_it",
                    },
                })
            })
            .collect(),
    ))
}

/// What Endora has done lately, and what it saw afterwards (ADR 0035) — the memory
/// right to *see* its actions, next to the beliefs it holds.
async fn list_outcomes(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let understanding = state.understanding.clone();
    let items =
        blocking(move || usecases::recent_outcomes(understanding.as_ref(), OUTCOMES_SHOWN)).await?;
    Ok(Json(items.iter().map(outcome_json).collect()))
}

#[derive(serde::Deserialize)]
struct ReactionBody {
    reaction: String,
}

/// The person says how an action landed. Offered where the action already appears —
/// they are never asked for it (ADR 0035).
async fn react_to_outcome(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReactionBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let oid = id
        .parse::<u128>()
        .map(endora_application::OutcomeId::new)
        .map_err(|_| ApiError(AppError::NotFound { entity: "outcome" }))?;
    let reaction =
        endora_application::Reaction::from_name(body.reaction.trim()).ok_or_else(|| {
            ApiError(AppError::BadRequest {
                message: "reaction must be helped, did_not_help, or no_reaction".to_owned(),
            })
        })?;
    let understanding = state.understanding.clone();
    let out =
        blocking(move || usecases::react_to_outcome(understanding.as_ref(), oid, reaction)).await?;
    Ok(Json(outcome_json(&out)))
}

fn capability_json(
    info: &endora_infrastructure::CapabilityInfo,
    enabled: bool,
    opened: bool,
    confirm: bool,
    settings: &endora_infrastructure::CapabilitySettings,
) -> serde_json::Value {
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
    let settings_complete = info
        .settings
        .iter()
        .all(|s| settings.get(s.key).is_some_and(|v| !v.trim().is_empty()));
    json!({
        "id": info.id,
        "name": info.name,
        "description": info.description,
        "category": info.category,
        "reaches_external": info.reaches_external,
        "reversibility": info.reversibility.name(),
        // Whether the person has opened this capability's irreversible band, and
        // whether it is therefore currently blocked deny-by-default (ADR 0024). Only
        // an irreversible skill can be blocked or opened.
        "open_irreversible": opened,
        "blocked": info.reversibility.name() == "irreversible" && !opened,
        // Whether the person set this skill to ask first ("on with user input"): it
        // runs only after they confirm each use, never on its own.
        "confirm": confirm,
        // `configured` = code ready + settings filled; `enabled` = the person's on/off
        // switch; a skill is usable only when both hold (ADR 0021).
        "configured": info.configured && settings_complete,
        "enabled": enabled,
        "usable": info.configured && settings_complete && enabled,
        "needs": info.needs,
        "settings": settings_schema,
    })
}

/// Lists the butler's skills (capabilities/modules), their status, and whether the
/// person has each turned on.
async fn list_capabilities(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let enabled: std::collections::HashMap<String, bool> = state
        .config
        .enabled_overrides()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let opened: std::collections::HashMap<String, bool> = state
        .config
        .opened_overrides()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let confirm: std::collections::HashMap<String, bool> = state
        .config
        .confirm_overrides()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let settings = settings_map(state.config.as_ref());
    let empty = endora_infrastructure::CapabilitySettings::new();
    Json(
        state
            .capabilities
            .iter()
            .map(|c| c.info())
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
            .filter(|info| info.configured || !info.settings.is_empty())
            .map(|info| {
                let on = enabled.get(info.id).copied().unwrap_or(true);
                let is_open = opened.get(info.id).copied().unwrap_or(false);
                let is_confirm = confirm.get(info.id).copied().unwrap_or(false);
                capability_json(
                    &info,
                    on,
                    is_open,
                    is_confirm,
                    settings.get(info.id).unwrap_or(&empty),
                )
            })
            .collect(),
    )
}

#[derive(Deserialize)]
struct SettingsRequest {
    settings: std::collections::HashMap<String, String>,
}

/// Sets one or more settings for a capability (ADR 0021). Validated against the
/// registry and its declared setting keys, so only known keys are stored.
async fn set_capability_settings(
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
struct EnableRequest {
    enabled: bool,
}

/// Turns a capability on or off for the person (ADR 0021). Validated against the
/// registry so only real skill ids are stored; nudges the change stream so open
/// consoles refresh.
async fn set_capability_enabled(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<EnableRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Any capability the butler actually offers, not just the built-in registry: an MCP
    // tool is exactly the kind of thing worth turning off (ADR 0040), and this route
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
        config
            .set_enabled(&cap_id, enabled)
            .map_err(AppError::Repository)?;
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
struct OpenRequest {
    open: bool,
}

/// Opens or re-blocks a capability's **irreversible band** for the person (ADR
/// 0024). Opening only ever moves the un-undoable from *blocked* to
/// *confirm-each-use* — the butler still never runs it on its own. Validated
/// against the registry; nudges the change stream so open consoles refresh.
async fn set_capability_open(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<OpenRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Accept a built-in capability id or a connected MCP tool id (server.tool) —
    // both are opened the same way (ADR 0024), keyed by id in the config store.
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
        config
            .set_open_irreversible(&cap_id, open)
            .map_err(AppError::Repository)?;
        record_event(
            events.as_ref(),
            clock.as_ref(),
            &if open {
                format!(
                    "Opened the {cap_id} skill's irreversible actions (still confirmed each time)"
                )
            } else {
                format!("Re-blocked the {cap_id} skill's irreversible actions")
            },
        );
        Ok::<_, AppError>(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "id": id, "open_irreversible": req.open })))
}

#[derive(Deserialize)]
struct ConfirmRequest {
    confirm: bool,
}

/// Sets whether a skill must **ask first** ("on with user input"): when on, the
/// butler proposes and runs it only after the person confirms each use, never on its
/// own — whatever the skill's band. Validated against the registry (built-in or a
/// connected MCP tool); nudges the change stream so open consoles refresh.
async fn set_capability_confirm(
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
        config
            .set_confirm(&cap_id, confirm)
            .map_err(AppError::Repository)?;
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

fn envelope_json(e: &AutonomyEnvelope) -> serde_json::Value {
    json!({ "auto_external": e.auto_external, "auto_consequential": e.auto_consequential })
}

/// Returns the person's autonomy envelope — the boundary the butler acts within
/// (ADR 0022).
async fn get_autonomy(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let envelope = blocking(move || {
        AutonomyEnvelopeRepository::get(config.as_ref()).map_err(AppError::Repository)
    })
    .await?;
    Ok(Json(envelope_json(&envelope)))
}

#[derive(Deserialize)]
struct AutonomyRequest {
    auto_external: bool,
    auto_consequential: bool,
}

/// Sets the autonomy envelope (ADR 0022). Widening it grants the butler more
/// independence; the deterministic policy layer still enforces the edges.
async fn set_autonomy(
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

fn brief_schedule_json(s: &BriefSchedule) -> serde_json::Value {
    json!({ "enabled": s.enabled, "hour_utc": s.hour_utc })
}

/// The deep-model config (a bigger AI for hard questions). The key is NEVER
/// returned — only whether one is set.
async fn get_deep_model(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let cfg =
        blocking(move || DeepModelRepository::get(config.as_ref()).map_err(AppError::Repository))
            .await?
            .unwrap_or_default();
    Ok(Json(json!({
        "url": cfg.url,
        "model": cfg.model,
        "configured": !cfg.url.is_empty() && !cfg.model.is_empty(),
        "key_set": !cfg.api_key.is_empty(),
    })))
}

#[derive(Deserialize)]
struct DeepModelRequest {
    url: String,
    model: String,
    #[serde(default)]
    api_key: Option<String>,
}

/// Configures the deep model. A blank/omitted `api_key` keeps any existing key, so
/// the secret is never round-tripped through the client.
async fn set_deep_model(
    State(state): State<AppState>,
    Json(req): Json<DeepModelRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    blocking(move || {
        let existing = DeepModelRepository::get(config.as_ref())
            .map_err(AppError::Repository)?
            .unwrap_or_default();
        let api_key = match req.api_key {
            Some(k) if !k.trim().is_empty() => k.trim().to_owned(),
            _ => existing.api_key, // keep the stored key when none is supplied
        };
        DeepModelRepository::set(
            config.as_ref(),
            &endora_application::DeepModel {
                url: req.url.trim().to_owned(),
                model: req.model.trim().to_owned(),
                api_key,
            },
        )
        .map_err(AppError::Repository)?;
        record_event(events.as_ref(), clock.as_ref(), "Configured the deep model");
        Ok::<_, AppError>(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

/// Renders one model slot for the console (sampling params flattened; `null`
/// means "use the endpoint default").
fn slot_json(slot: &ModelSlot) -> serde_json::Value {
    json!({
        "model": slot.model,
        "temperature": slot.sampling.temperature,
        "top_p": slot.sampling.top_p,
        "top_k": slot.sampling.top_k,
        "repeat_penalty": slot.sampling.repeat_penalty,
    })
}

/// The butler model configuration (ADR 0027), editable from the console. The API
/// key is NEVER returned — only whether one is set.
async fn get_model_config(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let cfg = blocking(move || {
        ButlerModelConfigRepository::get(config.as_ref()).map_err(AppError::Repository)
    })
    .await?
    .unwrap_or_default();
    let configured = !cfg.base_url.is_empty()
        && if cfg.mixture {
            !cfg.router.model.is_empty() && !cfg.synth.model.is_empty()
        } else {
            !cfg.single.model.is_empty()
        };
    // The effective deployment default (the env brain the butler falls back to when
    // nothing is saved). Surfaced so the Everyday card can name the model actually
    // running instead of a bare "using deployment default" — same values main.rs uses.
    let default_base_url = std::env::var("ENDORA_MODEL_URL")
        .unwrap_or_else(|_| "http://localhost:11434/v1".to_owned());
    let default_model = std::env::var("ENDORA_MODEL").unwrap_or_else(|_| "qwen2.5:7b".to_owned());
    let default_router = std::env::var("ENDORA_ROUTER_MODEL").unwrap_or_default();
    let default_synth = std::env::var("ENDORA_SYNTH_MODEL").unwrap_or_default();
    let default_mixture = !default_router.is_empty() && !default_synth.is_empty();
    Ok(Json(json!({
        "base_url": cfg.base_url,
        "mixture": cfg.mixture,
        "key_set": !cfg.api_key.is_empty(),
        "configured": configured,
        "single": slot_json(&cfg.single),
        "router": slot_json(&cfg.router),
        "synth": slot_json(&cfg.synth),
        "default_base_url": default_base_url,
        "default_model": default_model,
        "default_mixture": default_mixture,
        "default_router": default_router,
        "default_synth": default_synth,
    })))
}

#[derive(Deserialize, Default)]
struct SlotRequest {
    #[serde(default)]
    model: String,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    top_p: Option<f64>,
    #[serde(default)]
    top_k: Option<u32>,
    #[serde(default)]
    repeat_penalty: Option<f64>,
}

impl SlotRequest {
    fn into_slot(self) -> ModelSlot {
        ModelSlot {
            model: self.model.trim().to_owned(),
            sampling: Sampling {
                temperature: self.temperature,
                top_p: self.top_p,
                top_k: self.top_k,
                repeat_penalty: self.repeat_penalty,
            },
        }
    }
}

#[derive(Deserialize)]
struct ModelConfigRequest {
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    mixture: bool,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    single: SlotRequest,
    #[serde(default)]
    router: SlotRequest,
    #[serde(default)]
    synth: SlotRequest,
}

/// Saves the butler model configuration. A blank/omitted `api_key` keeps any
/// existing key, so the secret is never round-tripped through the client. The
/// change takes effect on the next turn — the [`ConfigurableButler`] rereads it.
async fn set_model_config(
    State(state): State<AppState>,
    Json(req): Json<ModelConfigRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    blocking(move || {
        let existing = ButlerModelConfigRepository::get(config.as_ref())
            .map_err(AppError::Repository)?
            .unwrap_or_default();
        let api_key = match req.api_key {
            Some(k) if !k.trim().is_empty() => k.trim().to_owned(),
            _ => existing.api_key, // keep the stored key when none is supplied
        };
        ButlerModelConfigRepository::set(
            config.as_ref(),
            &ButlerModelConfig {
                base_url: req.base_url.trim().to_owned(),
                api_key,
                mixture: req.mixture,
                single: req.single.into_slot(),
                router: req.router.into_slot(),
                synth: req.synth.into_slot(),
            },
        )
        .map_err(AppError::Repository)?;
        record_event(events.as_ref(), clock.as_ref(), "Updated the butler models");
        Ok::<_, AppError>(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct DiscoverModelsRequest {
    #[serde(default)]
    base_url: String,
    /// The key to use — falls back to the stored key for `role` when blank, so the
    /// person can discover with the already-saved key without re-entering it.
    #[serde(default)]
    api_key: Option<String>,
    /// Which stored key to fall back to: `deep` or `everyday` (default).
    #[serde(default)]
    role: Option<String>,
}

/// Lists the models an OpenAI-compatible endpoint offers, so the console can
/// populate a picker after the person enters the endpoint + key. Uses the key in
/// the request, or the stored key for the role when the field is left blank.
async fn discover_models(
    State(state): State<AppState>,
    Json(req): Json<DiscoverModelsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let base = req.base_url.trim().to_owned();
    let models = blocking(move || {
        if base.is_empty() {
            return Err(AppError::Model {
                message: "enter the endpoint first".to_owned(),
            });
        }
        let key = match req.api_key {
            Some(k) if !k.trim().is_empty() => k.trim().to_owned(),
            _ if req.role.as_deref() == Some("deep") => DeepModelRepository::get(config.as_ref())
                .map_err(AppError::Repository)?
                .map(|m| m.api_key)
                .unwrap_or_default(),
            _ => ButlerModelConfigRepository::get(config.as_ref())
                .map_err(AppError::Repository)?
                .map(|c| c.api_key)
                .unwrap_or_default(),
        };
        endora_infrastructure::list_models(&base, &key).map_err(|e| AppError::Model { message: e })
    })
    .await?;
    Ok(Json(json!({ "models": models })))
}

#[derive(Deserialize)]
struct TestConnectionRequest {
    #[serde(default)]
    base_url: String,
    /// The key to test — falls back to the stored key for `role` when blank, so the
    /// person can test the already-saved key without re-entering it.
    #[serde(default)]
    api_key: Option<String>,
    /// The model to exercise (a minimal completion). Blank ⇒ reachability check only.
    #[serde(default)]
    model: String,
    /// Which stored key to fall back to: `deep` or `everyday` (default).
    #[serde(default)]
    role: Option<String>,
}

/// Tests that a model endpoint + API key actually work, for the settings "Test
/// connection" button. Sends a minimal completion with the given (or stored) key —
/// a real auth check, not just a `/models` listing — and returns `{ok, detail}`
/// with a human-readable result either way. Never persists anything.
async fn test_model_connection(
    State(state): State<AppState>,
    Json(req): Json<TestConnectionRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let base = req.base_url.trim().to_owned();
    let model = req.model.trim().to_owned();
    let result = blocking(move || {
        let key = match req.api_key {
            Some(k) if !k.trim().is_empty() => k.trim().to_owned(),
            _ if req.role.as_deref() == Some("deep") => DeepModelRepository::get(config.as_ref())
                .map_err(AppError::Repository)?
                .map(|m| m.api_key)
                .unwrap_or_default(),
            _ => ButlerModelConfigRepository::get(config.as_ref())
                .map_err(AppError::Repository)?
                .map(|c| c.api_key)
                .unwrap_or_default(),
        };
        Ok::<_, AppError>(endora_infrastructure::test_connection(&base, &key, &model))
    })
    .await?;
    // A failed test is a normal, expected outcome (bad key, wrong model) — report it
    // as data with `ok: false`, not an HTTP error.
    Ok(Json(match result {
        Ok(detail) => json!({ "ok": true, "detail": detail }),
        Err(detail) => json!({ "ok": false, "detail": detail }),
    }))
}

/// Kicks off the self-improving model layer (ADR 0027) in the background:
/// discover the models on the local endpoint, score each with the fitness
/// function, and gate-adopt the best **local** one (auto-adopt local, propose
/// cloud). Slow (it runs the eval per candidate), so it runs detached and records
/// its progress + result to the activity log; the response returns immediately.
/// Runs the model layer synchronously (discover local models → evaluate each →
/// gated adoption), recording progress + the result to the activity log. Slow
/// (it runs the eval per candidate). Shared by the manual `/v1/model-layer/run`
/// and the scheduled nightly tune.
fn run_model_tune(
    butler: &(dyn Butler + Send + Sync),
    config: &endora_capabilities::ConfigStore,
    events: &endora_platform::EventStore,
    clock: &SystemClock,
    model_url: &str,
) {
    use endora_infrastructure::model_layer::{AdoptionOutcome, ModelCandidate, Scorecard};

    record_event(
        events,
        clock,
        "Model layer: evaluating available local models",
    );
    // Discover local models → single-model local candidates (keyless).
    let ids = endora_infrastructure::list_models(model_url, "").unwrap_or_default();
    let candidates: Vec<ModelCandidate> = ids
        .into_iter()
        .map(|id| ModelCandidate {
            name: id.clone(),
            config: ButlerModelConfig {
                base_url: model_url.to_owned(),
                api_key: String::new(),
                mixture: false,
                single: ModelSlot {
                    model: id,
                    sampling: Sampling::default(),
                },
                ..ButlerModelConfig::default()
            },
        })
        .collect();

    let mut on_propose = |c: &ModelCandidate, s: &Scorecard, incumbent: usize| {
        record_event(
            events,
            clock,
            &format!(
                "Model layer: a cloud model ({}) scored {}/{} vs {} — proposing (needs your ok)",
                c.name,
                s.total(),
                s.max(),
                incumbent
            ),
        );
    };

    match endora_infrastructure::run_model_layer(butler, candidates, config, &mut on_propose) {
        Ok((outcome, scored)) => {
            for sc in &scored {
                record_event(
                    events,
                    clock,
                    &format!(
                        "Model eval: {} scored {}/{}",
                        sc.candidate.name,
                        sc.score.total(),
                        sc.score.max()
                    ),
                );
            }
            let msg = match outcome {
                AdoptionOutcome::Adopted { name, score } => {
                    format!("adopted a better local model: {name} ({score}/15)")
                }
                AdoptionOutcome::Proposed { name, score } => {
                    format!("proposes cloud model {name} ({score}/15) — awaiting your ok")
                }
                AdoptionOutcome::Kept { incumbent } => {
                    format!("kept the current model (still best at {incumbent}/15)")
                }
            };
            record_event(events, clock, &format!("Model layer: {msg}"));
        }
        Err(e) => record_event(events, clock, &format!("Model layer run failed: {e}")),
    }
}

/// The model endpoint the tune discovers local candidates from.
fn tune_model_url() -> String {
    std::env::var("ENDORA_MODEL_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_owned())
}

async fn run_model_layer_now(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let butler = state.butler.clone();
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    let model_url = tune_model_url();
    tokio::task::spawn_blocking(move || {
        run_model_tune(
            butler.as_ref(),
            config.as_ref(),
            events.as_ref(),
            clock.as_ref(),
            &model_url,
        );
    });
    Ok(Json(json!({ "started": true })))
}

/// The nightly model-tune schedule (ADR 0027) — off by default.
async fn get_tune_schedule(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let s = blocking(move || {
        ModelTuneScheduleRepository::get(config.as_ref()).map_err(AppError::Repository)
    })
    .await?;
    Ok(Json(
        json!({ "enabled": s.enabled, "hour_utc": s.hour_utc }),
    ))
}

#[derive(Deserialize)]
struct TuneScheduleRequest {
    enabled: bool,
    hour_utc: u8,
}

async fn set_tune_schedule(
    State(state): State<AppState>,
    Json(req): Json<TuneScheduleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    blocking(move || {
        let current =
            ModelTuneScheduleRepository::get(config.as_ref()).map_err(AppError::Repository)?;
        ModelTuneScheduleRepository::set(
            config.as_ref(),
            &endora_application::ModelTuneSchedule {
                enabled: req.enabled,
                hour_utc: req.hour_utc.min(23),
                last_ms: current.last_ms, // keep last-run so toggling doesn't re-fire
            },
        )
        .map_err(AppError::Repository)
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct DeepAskRequest {
    question: String,
}

/// Escalates one question to the configured deep model and posts its answer to the
/// chat. The person opts in per question (the everyday stays local). The outbound
/// question passes the egress guard — a secret blocks it, PII is redacted (ADR 0023).
async fn deep_ask(
    State(state): State<AppState>,
    Json(req): Json<DeepAskRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chat = state.chat.clone();
    let config = state.config.clone();
    let events = state.events.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let question = req.question;
    let posted = blocking(move || {
        let Some(cfg) = DeepModelRepository::get(config.as_ref()).map_err(AppError::Repository)?
        else {
            return Ok::<_, AppError>(None);
        };
        if cfg.url.is_empty() || cfg.model.is_empty() {
            return Ok(None);
        }
        // Egress guard on the outbound question (it leaves the device to the deep model).
        if let Some(kind) = endora_infrastructure::scan_outbound_secret(&question) {
            return Err(AppError::BadRequest {
                message: format!(
                    "won't send that to the deep model — it looks like it contains {kind}"
                ),
            });
        }
        let mut v = serde_json::Value::String(question.clone());
        endora_infrastructure::redact_pii_in_value(&mut v);
        let safe_question = v.as_str().unwrap_or(&question).to_owned();
        // Persist the person's question (what they typed, kept local) so the chat
        // shows both sides after a reload — this path otherwise stored only the
        // answer, so the question vanished and it looked like nothing happened.
        usecases::post_user_message(chat.as_ref(), ids.as_ref(), clock.as_ref(), &question)?;
        // Record the person's question, then the deep answer.
        record_event(events.as_ref(), clock.as_ref(), "Asked the deep model");
        match endora_infrastructure::ask_deep_model(
            &cfg.url,
            &cfg.model,
            &cfg.api_key,
            &safe_question,
        ) {
            Ok(answer) => {
                let msg = usecases::post_butler_message(
                    chat.as_ref(),
                    ids.as_ref(),
                    clock.as_ref(),
                    &answer,
                )?;
                Ok(Some(msg))
            }
            Err(e) => Err(AppError::BadRequest {
                message: format!("the deep model couldn't answer: {e}"),
            }),
        }
    })
    .await?;
    let _ = state.changes.send(());
    match posted {
        Some(msg) => Ok(Json(
            json!({ "answered": true, "message": MessageResponse::from(&msg) }),
        )),
        None => Ok(Json(
            json!({ "answered": false, "note": "Configure a deep model in Settings first." }),
        )),
    }
}

/// The daily-brief schedule (when the butler prepares a brief on its own).
async fn get_brief_schedule(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let schedules = state.schedules.clone();
    let s = blocking(move || usecases::brief_schedule(schedules.as_ref())).await?;
    Ok(Json(brief_schedule_json(&s)))
}

#[derive(Deserialize)]
struct BriefScheduleRequest {
    enabled: bool,
    hour_utc: u8,
}

/// Sets the daily-brief schedule. `hour_utc` is the UTC hour (the console converts
/// the person's local hour). Only ever prepares a brief from reversible skills.
async fn set_brief_schedule(
    State(state): State<AppState>,
    Json(req): Json<BriefScheduleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let schedules = state.schedules.clone();
    let s = blocking(move || {
        usecases::set_brief_schedule(schedules.as_ref(), req.enabled, req.hour_utc)
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(brief_schedule_json(&s)))
}

fn nightly_loop_schedule_json(s: &endora_scheduling::NightlyLoopSchedule) -> serde_json::Value {
    json!({ "enabled": s.enabled, "hour_utc": s.hour_utc })
}

/// The nightly self-improvement loop schedule (ADR 0024) — when the butler reviews
/// the day and reflects, overnight, within the reversible band.
async fn get_nightly_loop_schedule(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let schedules = state.schedules.clone();
    let s = blocking(move || usecases::nightly_loop_schedule(schedules.as_ref())).await?;
    Ok(Json(nightly_loop_schedule_json(&s)))
}

/// Sets the nightly-loop schedule. `hour_utc` is the UTC hour (the console converts
/// the person's local hour). The loop only ever reflects/drafts — never anything
/// irreversible.
async fn set_nightly_loop_schedule(
    State(state): State<AppState>,
    Json(req): Json<BriefScheduleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let schedules = state.schedules.clone();
    let s = blocking(move || {
        usecases::set_nightly_loop_schedule(schedules.as_ref(), req.enabled, req.hour_utc)
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(nightly_loop_schedule_json(&s)))
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
    // Data-loss tripwire + query minimization (ADR 0023), on the explicit-invoke
    // path too: refuse a request carrying a secret, and redact personal identifiers
    // before it leaves.
    let mut input = input;
    if cap.info().reaches_external {
        if let Some(kind) = endora_infrastructure::scan_outbound_secret(&input.to_string()) {
            return Err(ApiError(AppError::BadRequest {
                message: format!("refusing to send this out — it looks like it contains {kind}"),
            }));
        }
        endora_infrastructure::redact_pii_in_value(&mut input);
    }
    let config = state.config.clone();
    let result = tokio::task::spawn_blocking(move || {
        let settings = settings_map(config.as_ref())
            .remove(&id)
            .unwrap_or_default();
        cap.invoke(&input, &settings)
    })
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
        let mut ticks: u64 = 0;
        loop {
            ticker.tick().await;
            ticks += 1;
            // An enabled server that came up with NO tools connected to nothing, so try
            // again. Observed after a reboot: every container started at once, Endora
            // reached the Home Assistant server before it was listening, got an empty
            // catalogue, and never retried — so for twenty minutes the butler had no way
            // to act and narrated "turning on the kitchen light" with nothing behind it.
            //
            // `connect_mcp` runs once at startup, which is exactly wrong on a machine
            // where everything boots together. Every second minute is often enough to
            // ride out a boot race and cheap enough to leave running for a server that
            // is genuinely gone.
            if ticks.is_multiple_of(4) {
                reconnect_empty_mcp_servers(&state);
            }
            let events = state.events.clone();
            let chat = state.chat.clone();
            let schedules = state.schedules.clone();
            let understanding = state.understanding.clone();
            let config = state.config.clone();
            let audit = state.audit.clone();
            let ids = state.ids.clone();
            let clock = state.clock.clone();
            let capabilities = state.capabilities.clone();
            let butler = state.butler.clone();
            let mcp = mcp_snapshot(&state);
            // Take the turn lock so a proactive brief/check-in never interleaves
            // with a chat reply; held across the blocking work below.
            let _turn = state.turn_lock.clone().lock_owned().await;
            let posted = tokio::task::spawn_blocking(move || {
                // Unattended: nobody is here to confirm anything, so only the reversible
                // bands may act — regardless of what the person opened for chat turns.
                let runner = build_reversible_only_runner(config.as_ref(), capabilities, mcp);
                let context = usecases::butler_context(
                    understanding.as_ref(),
                    understanding.as_ref(),
                    config.as_ref(),
                    &runner,
                    clock.as_ref(),
                )?;
                // The butler decides whether it has a reason to speak; the schedule
                // only bounds how often it may (ADR 0031).
                let posted = usecases::consider_reaching_out(
                    chat.as_ref(),
                    schedules.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    &runner,
                    butler.as_ref(),
                    audit.as_ref(),
                    ids.as_ref(),
                    clock.as_ref(),
                    &context,
                )?;
                if let Some((_, activity)) = &posted {
                    for item in activity {
                        record_event(events.as_ref(), clock.as_ref(), item);
                    }
                }
                // A daily brief, if one is due (reversible skills only).
                let briefed = usecases::run_due_brief(
                    chat.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    schedules.as_ref(),
                    &runner,
                    butler.as_ref(),
                    audit.as_ref(),
                    ids.as_ref(),
                    clock.as_ref(),
                    &context,
                )?;
                if let Some((_, activity)) = &briefed {
                    for item in activity {
                        record_event(events.as_ref(), clock.as_ref(), item);
                    }
                    record_event(events.as_ref(), clock.as_ref(), "Prepared your daily brief");
                }
                // The nightly self-improvement loop (ADR 0024), if due: review the
                // day and reflect, within the reversible band — never anything
                // irreversible. Serialized under the turn lock like the brief.
                let reflected = usecases::run_due_nightly_loop(
                    chat.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    schedules.as_ref(),
                    &runner,
                    butler.as_ref(),
                    audit.as_ref(),
                    ids.as_ref(),
                    clock.as_ref(),
                    &context,
                )?;
                if let Some((_, activity)) = &reflected {
                    for item in activity {
                        record_event(events.as_ref(), clock.as_ref(), item);
                    }
                    record_event(
                        events.as_ref(),
                        clock.as_ref(),
                        "Ran the nightly self-improvement loop",
                    );
                }
                Ok::<_, AppError>(posted.is_some() || briefed.is_some() || reflected.is_some())
            })
            .await;
            if let Ok(Ok(true)) = posted {
                let _ = state.changes.send(());
            }

            // Nightly self-improving model tune (ADR 0027), if scheduled + due.
            // Marked fired first (so it can't double-run), then run DETACHED and
            // without the turn lock — it's long and competes on the GPU, which is
            // why the schedule points at an off-hour.
            let due = {
                let config = state.config.clone();
                let clock = state.clock.clone();
                tokio::task::spawn_blocking(move || {
                    let now = clock.now().unix_millis();
                    match ModelTuneScheduleRepository::get(config.as_ref()) {
                        Ok(mut s) if s.is_due(now) => {
                            s.last_ms = now;
                            let _ = ModelTuneScheduleRepository::set(config.as_ref(), &s);
                            true
                        }
                        _ => false,
                    }
                })
                .await
                .unwrap_or(false)
            };
            if due {
                let config = state.config.clone();
                let events = state.events.clone();
                let clock = state.clock.clone();
                let butler = state.butler.clone();
                let model_url = tune_model_url();
                tokio::task::spawn_blocking(move || {
                    run_model_tune(
                        butler.as_ref(),
                        config.as_ref(),
                        events.as_ref(),
                        clock.as_ref(),
                        &model_url,
                    );
                });
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
                message: format!(
                    "unknown preference kind {k:?}; expected taste, authority, or context"
                ),
            })
        })?,
        None => PreferenceKind::Taste,
    };
    let understanding = state.understanding.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let preference = blocking(move || {
        usecases::create_preference(
            understanding.as_ref(),
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
    let understanding = state.understanding.clone();
    let prefs = blocking(move || usecases::list_preferences(understanding.as_ref())).await?;
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
    let understanding = state.understanding.clone();
    blocking(move || usecases::delete_preference(understanding.as_ref(), pref_id)).await?;
    Ok(Json(json!({ "deleted": true })))
}

/// The full export of the user's data — the "exportable" memory right.
#[derive(Serialize)]
struct ExportResponse {
    audit: Vec<AuditResponse>,
    messages: Vec<MessageResponse>,
    preferences: Vec<PreferenceResponse>,
    beliefs: Vec<serde_json::Value>,
}

impl From<&MemorySnapshot> for ExportResponse {
    fn from(s: &MemorySnapshot) -> Self {
        Self {
            audit: s.audit.iter().map(AuditResponse::from).collect(),
            messages: s.messages.iter().map(MessageResponse::from).collect(),
            preferences: s.preferences.iter().map(PreferenceResponse::from).collect(),
            beliefs: s.beliefs.iter().map(stored_belief_json).collect(),
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

    fn test_state() -> AppState {
        AppState::new(
            Arc::new(SqliteStore::open_in_memory().unwrap()),
            Arc::new(RandomIdSource),
            Arc::new(SystemClock),
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
    async fn there_is_no_way_for_the_person_to_create_an_intention() {
        // ADR 0036's first constraint, as a test rather than a promise. Endora forms
        // its own intentions; a console with an "add" button would mean the ADR failed,
        // and the API is where that would have to start.
        let app = app(test_state());
        for (uri, body) in [
            ("/v1/intentions", r#"{"statement":"do a thing"}"#),
            ("/v1/intentions/1", r#"{"statement":"edit a thing"}"#),
        ] {
            let res = app.clone().oneshot(post(uri, body)).await.unwrap();
            assert!(
                res.status() == StatusCode::METHOD_NOT_ALLOWED
                    || res.status() == StatusCode::NOT_FOUND,
                "POST {uri} answered {} — the person must not be able to file work",
                res.status()
            );
        }
    }

    #[tokio::test]
    async fn an_intention_is_visible_and_can_be_dropped_but_only_dropped() {
        use endora_application::{
            BeliefId, Intention, IntentionId, IntentionRepository, Timestamp,
        };
        let state = test_state();
        IntentionRepository::save(
            state.understanding.as_ref(),
            &Intention::form(
                IntentionId::new(5),
                "learn what helps them sleep",
                BeliefId::new(7),
                Timestamp::from_unix_millis(10),
            )
            .unwrap(),
        )
        .unwrap();
        let app = app(state);

        let listed = json_body(app.clone().oneshot(get("/v1/intentions")).await.unwrap()).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["statement"], "learn what helps them sleep");
        assert_eq!(listed[0]["active"], true);
        // Never null — an intention that can't be explained can't exist.
        assert_eq!(listed[0]["motivating_belief"], "7");

        let dropped = json_body(
            app.clone()
                .oneshot(post("/v1/intentions/5/drop", "{}"))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(dropped["state"], "abandoned");
        assert_eq!(dropped["active"], false);

        // It stays visible as something Endora once pursued, but is no longer current.
        let listed = json_body(app.oneshot(get("/v1/intentions")).await.unwrap()).await;
        assert_eq!(listed[0]["active"], false);
    }

    #[test]
    fn the_persisted_step_trail_counts_calls_not_events() {
        use super::fold_step;
        let step = |skill: &str, status: &str| serde_json::json!({ "skill": skill, "status": status, "label": "x", "output": null });

        // The live trail from a real turn: one failed action, then two reads. Each
        // reports twice — running, then its outcome.
        let mut trail = Vec::new();
        for s in [
            step("home-assistant.HassTurnOff", "running"),
            step("home-assistant.HassTurnOff", "failed"),
            step("home-assistant.GetLiveContext", "running"),
            step("home-assistant.GetLiveContext", "done"),
            step("home-assistant.GetLiveContext", "running"),
            step("home-assistant.GetLiveContext", "done"),
        ] {
            fold_step(&mut trail, s);
        }

        // Three calls, not six events — the console said "6 actions" for this turn.
        assert_eq!(trail.len(), 3, "trail was {trail:?}");
        let statuses: Vec<&str> = trail
            .iter()
            .map(|s| s["status"].as_str().unwrap())
            .collect();
        assert_eq!(statuses, vec!["failed", "done", "done"]);
    }

    #[test]
    fn a_blocked_step_keeps_its_own_row() {
        use super::fold_step;
        // Policy refusing a call never reports "running" first. Folding it into an
        // unrelated in-flight row would hide the most interesting step there is.
        let mut trail = Vec::new();
        fold_step(
            &mut trail,
            serde_json::json!({ "skill": "a", "status": "running", "label": "x", "output": null }),
        );
        fold_step(
            &mut trail,
            serde_json::json!({ "skill": "b", "status": "blocked", "label": "x", "output": null }),
        );
        assert_eq!(trail.len(), 2, "the blocked call was swallowed: {trail:?}");
        assert_eq!(trail[1]["skill"], "b");
    }

    #[tokio::test]
    async fn an_alias_is_confirmed_by_the_person_and_grounds_later_turns() {
        // ADR 0039's answer path: Endora asks what a target is really called, and the
        // person's answer is the CONFIRMED source — the only one policy trusts.
        let app = app(test_state());
        let stored = json_body(
            app.clone()
                .oneshot(post(
                    "/v1/aliases",
                    r#"{"server":"home-assistant","said":"kitchen main","means":"Kitchen Main"}"#,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(stored["means"], "Kitchen Main");

        let listed = json_body(app.oneshot(get("/v1/aliases")).await.unwrap()).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["said"], "kitchen main");
    }

    #[tokio::test]
    async fn an_alias_that_names_nothing_is_refused() {
        // A blank side grounds nothing and would quietly do nothing at all.
        let res = app(test_state())
            .oneshot(post(
                "/v1/aliases",
                r#"{"server":"home-assistant","said":"kitchen main","means":"  "}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn an_enabled_server_with_no_tools_is_the_state_worth_retrying() {
        // The condition the heartbeat looks for, stated as a test because the bug it
        // fixes was silent: after a reboot the Home Assistant server was registered,
        // enabled, and exposing zero tools, so the butler narrated actions it had no
        // way to take. Zero tools on an ENABLED server never makes sense — it means the
        // connection found nothing — and nothing retried it.
        use endora_application::CapabilitySpec;
        let has_tools = |server: &str, live: &[CapabilitySpec]| {
            let prefix = format!("{server}.");
            live.iter().any(|c| c.id.starts_with(&prefix))
        };
        let spec = |id: &str| CapabilitySpec {
            id: id.to_owned(),
            description: String::new(),
            configured: true,
            autonomous: false,
            input_schema: None,
            reversibility: endora_application::Reversibility::Irreversible,
        };

        // A healthy server is left alone.
        let live = vec![spec("home-assistant.HassTurnOff")];
        assert!(has_tools("home-assistant", &live));

        // One that came up empty is detected — including when another server is fine,
        // which is the case a naive "any tools at all?" check would miss.
        assert!(!has_tools("calendar", &live));
    }

    #[tokio::test]
    async fn repairs_are_derived_from_what_was_observed_not_stored() {
        use endora_application::{Outcome, OutcomeId, OutcomeRepository, Timestamp};
        let state = test_state();
        // Two actions that reported success and moved nothing — the live kitchen case.
        for id in [1_u128, 2] {
            OutcomeRepository::save(
                state.understanding.as_ref(),
                &Outcome::record(
                    OutcomeId::new(id),
                    "home-assistant.HassTurnOff",
                    r#"{"area":"kitchen"}"#,
                    "The action completed successfully on: Kitchen (area).",
                    Some("Kitchen Main | switch | on"),
                    Timestamp::from_unix_millis(id as i64),
                    None,
                    Some(false),
                )
                .unwrap(),
            )
            .unwrap();
        }
        let app = app(state);

        let found = json_body(app.clone().oneshot(get("/v1/repairs")).await.unwrap()).await;
        assert_eq!(found.as_array().unwrap().len(), 1, "{found}");
        assert_eq!(found[0]["capability"], "home-assistant.HassTurnOff");
        assert_eq!(found[0]["attempts"], 2);

        // Nothing was stored to make that happen, and nothing can be dismissed: there
        // is deliberately no way to write a repair (ADR 0039/0029).
        let res = app
            .oneshot(post("/v1/repairs", r#"{"capability":"x"}"#))
            .await
            .unwrap();
        assert!(
            res.status() == StatusCode::METHOD_NOT_ALLOWED || res.status() == StatusCode::NOT_FOUND,
            "repairs became writable — that is a queue: {}",
            res.status()
        );
    }

    #[tokio::test]
    async fn outcomes_are_visible_and_start_empty() {
        // Nothing has acted yet, so there is nothing to show — and the endpoint must
        // still answer, since the console asks for it on every load.
        let listed = json_body(
            app(test_state())
                .oneshot(get("/v1/outcomes"))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(listed, serde_json::json!([]));
    }

    #[tokio::test]
    async fn an_outcome_surfaces_its_claim_and_observation_separately_and_takes_a_reaction() {
        use endora_application::{Outcome, OutcomeId, OutcomeRepository, Timestamp};
        let state = test_state();
        // The kitchen light: the tool claimed success, the world disagreed. The console
        // must show BOTH, unmerged (ADR 0035).
        OutcomeRepository::save(
            state.understanding.as_ref(),
            &Outcome::record(
                OutcomeId::new(42),
                "home.HassTurnOff",
                r#"{"name":"kitchen"}"#,
                "action_done",
                Some("kitchen switch: on"),
                Timestamp::from_unix_millis(1_000),
                None,
                None,
            )
            .unwrap(),
        )
        .unwrap();
        let app = app(state);

        let listed = json_body(app.clone().oneshot(get("/v1/outcomes")).await.unwrap()).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["capability"], "home.HassTurnOff");
        assert_eq!(listed[0]["claim"], "action_done");
        assert_eq!(listed[0]["observation"], "kitchen switch: on");
        assert_eq!(listed[0]["observed"], true);
        assert!(listed[0]["reaction"].is_null(), "nobody was asked");

        let reacted = json_body(
            app.clone()
                .oneshot(post(
                    "/v1/outcomes/42/reaction",
                    r#"{"reaction":"did_not_help"}"#,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(reacted["reaction"], "did_not_help");

        // And it stuck.
        let listed = json_body(app.oneshot(get("/v1/outcomes")).await.unwrap()).await;
        assert_eq!(listed[0]["reaction"], "did_not_help");
    }

    #[tokio::test]
    async fn reacting_to_an_unknown_outcome_is_a_clean_not_found() {
        let res = app(test_state())
            .oneshot(post(
                "/v1/outcomes/12345/reaction",
                r#"{"reaction":"helped"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_unrecognized_reaction_is_rejected_rather_than_stored() {
        // The stored vocabulary is closed (ADR 0035); a typo must not become a value
        // that later reads back as corrupt.
        let res = app(test_state())
            .oneshot(post(
                "/v1/outcomes/1/reaction",
                r#"{"reaction":"loved it"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
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
    async fn chat_records_the_exchange() {
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

        // The history holds both turns.
        let hist = json_body(app.clone().oneshot(get("/v1/chat")).await.unwrap()).await;
        assert_eq!(hist.as_array().unwrap().len(), 2);
        assert_eq!(hist[0]["role"], "user");
        assert_eq!(hist[1]["role"], "butler");
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
            .oneshot(post(
                "/v1/preferences",
                r#"{"text":"Likes tea","kind":"taste"}"#,
            ))
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

        // A domain-invalid request (blank text) is a 400 and must not signal.
        let res = app
            .clone()
            .oneshot(post("/v1/preferences", r#"{"text":"  ","kind":"taste"}"#))
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
    async fn export_then_purge_clears_all_data() {
        let app = app(test_state());

        // Seed something exportable: a preference and a chat turn.
        let res = app
            .clone()
            .oneshot(post(
                "/v1/preferences",
                r#"{"text":"Likes tea","kind":"taste"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let res = app
            .clone()
            .oneshot(post("/v1/chat", r#"{"message":"hello"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app.clone().oneshot(get("/v1/export")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let export = json_body(res).await;
        assert_eq!(export["preferences"].as_array().unwrap().len(), 1);
        assert_eq!(export["messages"].as_array().unwrap().len(), 2);

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

        let res = app.clone().oneshot(get("/v1/export")).await.unwrap();
        let empty = json_body(res).await;
        assert!(empty["preferences"].as_array().unwrap().is_empty());
        assert!(empty["messages"].as_array().unwrap().is_empty());
        assert!(empty["beliefs"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn editing_a_server_with_a_blank_token_keeps_the_saved_one() {
        // Registered `disabled` so no real connection is attempted — this exercises the
        // config merge, not networking.
        let router = app(test_state());
        let reg = router
            .clone()
            .oneshot(post(
                "/v1/mcp/servers",
                r#"{"name":"ha","transport":"http","url":"http://old/sse","auth":"secret-token","command":"","args":[],"env":{},"enabled":false}"#,
            ))
            .await
            .unwrap();
        assert_eq!(reg.status(), StatusCode::OK);

        // Edit: change only the URL, leave the token blank (the UI can't echo secrets).
        let edit = router
            .clone()
            .oneshot(post(
                "/v1/mcp/servers",
                r#"{"name":"ha","transport":"http","url":"http://new/sse","auth":"","command":"","args":[],"env":{},"enabled":false}"#,
            ))
            .await
            .unwrap();
        assert_eq!(edit.status(), StatusCode::OK);

        let list = json_body(router.oneshot(get("/v1/mcp/servers")).await.unwrap()).await;
        let ha = list["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "ha")
            .expect("ha server present");
        // URL updated, but the token was preserved rather than wiped.
        assert_eq!(ha["url"], "http://new/sse");
        assert_eq!(ha["auth_set"], true);
    }

    #[test]
    fn stt_hallucination_filter_drops_repeats_but_keeps_real_speech() {
        use super::looks_like_stt_hallucination;
        // The exact silence-hallucination signature: one token over and over.
        let junk = "Torsdagsfotografi ".repeat(20);
        assert!(looks_like_stt_hallucination(junk.trim()));
        assert!(looks_like_stt_hallucination(
            "thank you thank you thank you thank you thank you thank you"
        ));
        // Real sentences vary and must pass through untouched.
        assert!(!looks_like_stt_hallucination(
            "turn on the kitchen lights and check if the back door is locked"
        ));
        assert!(!looks_like_stt_hallucination(
            "what's the weather like today"
        ));
        // Too short to judge — never dropped.
        assert!(!looks_like_stt_hallucination("no no no"));
    }

    #[tokio::test]
    async fn trust_all_defaults_on_and_can_be_toggled() {
        let router = app(test_state());
        // Registered without a trust_all field → defaults on. Disabled so no connect.
        let reg = router
            .clone()
            .oneshot(post(
                "/v1/mcp/servers",
                r#"{"name":"ha","transport":"http","url":"http://ha/sse","auth":"t","enabled":false}"#,
            ))
            .await
            .unwrap();
        assert_eq!(reg.status(), StatusCode::OK);
        let list = json_body(
            router
                .clone()
                .oneshot(get("/v1/mcp/servers"))
                .await
                .unwrap(),
        )
        .await;
        let ha = list["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "ha")
            .unwrap();
        assert_eq!(ha["trust_all"], true);

        // Turn it off via the endpoint.
        let off = router
            .clone()
            .oneshot(post("/v1/mcp/servers/ha/trust", r#"{"trust_all":false}"#))
            .await
            .unwrap();
        assert_eq!(off.status(), StatusCode::OK);
        let list = json_body(router.oneshot(get("/v1/mcp/servers")).await.unwrap()).await;
        let ha = list["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "ha")
            .unwrap();
        assert_eq!(ha["trust_all"], false);
    }

    #[tokio::test]
    async fn nominating_a_reader_on_an_unknown_server_is_404() {
        let res = app(test_state())
            .oneshot(post(
                "/v1/mcp/servers/nope/reader",
                r#"{"reader_tool":"list_events"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_reader_nomination_must_name_a_tool_the_server_really_has() {
        // A typo must not quietly disable read-back — it would fail by simply never
        // verifying anything, which is the worst way for a safety mechanism to break.
        let state = test_state();
        let app = app(state);
        let res = app
            .clone()
            .oneshot(post(
                "/v1/mcp/servers/whatever/reader",
                r#"{"reader_tool":"GetLiveContxt"}"#,
            ))
            .await
            .unwrap();
        assert!(
            res.status() == StatusCode::BAD_REQUEST || res.status() == StatusCode::NOT_FOUND,
            "a tool the server doesn't expose was accepted: {}",
            res.status()
        );
    }

    #[tokio::test]
    async fn setting_trust_on_an_unknown_server_is_404() {
        let res = app(test_state())
            .oneshot(post("/v1/mcp/servers/nope/trust", r#"{"trust_all":true}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn reconnecting_an_unknown_mcp_server_is_404() {
        let res = app(test_state())
            .oneshot(post("/v1/mcp/servers/nope/reconnect", ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn reconnecting_a_registered_server_reports_its_tool_count() {
        let state = test_state();
        // Register a stdio server whose command won't produce tools; the point is that
        // reconnect answers for a real, known server rather than erroring.
        let router = app(state);
        let reg = router
            .clone()
            .oneshot(post(
                "/v1/mcp/servers",
                r#"{"name":"probe","transport":"stdio","command":"true","args":[],"env":{},"url":"","auth":"","enabled":true}"#,
            ))
            .await
            .unwrap();
        assert_eq!(reg.status(), StatusCode::OK);

        let res = router
            .oneshot(post("/v1/mcp/servers/probe/reconnect", ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = json_body(res).await;
        assert_eq!(body["ok"], true);
        // It didn't connect (no real MCP server behind `true`), and the endpoint says
        // so honestly instead of failing.
        assert_eq!(body["connected"], false);
        assert_eq!(body["tools_live"], 0);
    }
}
