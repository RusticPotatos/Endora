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
    CapabilitySettingsRepository, CheckinSchedule, DeepModelRepository, ExecutionLeaseRepository,
    GoalDelivery, GoalRepository, MemorySnapshot, ModelSlot, ModelTuneScheduleRepository,
    RepositoryError, Sampling, usecases,
};
use endora_application::{
    AuditRecord, Belief, BeliefId, ChatMessage, IdSource, IntentionRepository, Preference,
    PreferenceId, PreferenceKind, ResearchAssignment, ResearchRepository, ResearchStatus,
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
    /// Explicit handheld administration, with server-owned credentials and journal.
    pub console: Arc<dyn endora_capabilities::application::HomeConsole>,
    /// Bounded dashboard observations, never added to model context.
    pub homelab: Arc<std::sync::Mutex<serde_json::Value>>,
    /// The persistence adapter (implements the repository ports not yet moved to
    /// their bounded context — ADR 0050).
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
    /// The same brain, concretely — for facts only the composition can answer, like
    /// which rung is serving (ADR 0072). `None` in tests that hand in a bare fake.
    pub brain: Option<Arc<endora_infrastructure::ConfigurableButler>>,
    /// Broadcasts a signal whenever a write succeeds, so activity-stream
    /// subscribers know to refresh. Carries no payload — it is a "something
    /// changed" nudge, and clients re-read the authoritative state.
    pub changes: broadcast::Sender<()>,
    /// When this process started, so a never-claimed node can be claimed without the
    /// bootstrap token for a short while after it comes up (ADR 0058 / `crate::signin`).
    pub started_ms: i64,
    /// The token every `/v1` request must carry.
    ///
    /// This node shipped with no inbound authentication at all, on `0.0.0.0`, which made
    /// `/v1/export` and every write available to anyone who could reach the port. Empty here
    /// refuses every API route rather than opening them — see [`crate::auth::may_pass`].
    pub token: Arc<String>,
    /// The butler's skills (weather, web, …) — declared modules the butler can
    /// reach for, each gated by its autonomy level (ADR 0056).
    pub capabilities: Arc<Vec<Arc<dyn Capability>>>,
    /// Connected MCP servers, as a runner (ADR 0054). Long-lived (subprocesses),
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
    /// The running conversation summary (ADR 0053 context compaction), which keeps the
    /// chat prompt bounded on a long conversation without dropping the day's thread.
    /// Persisted (SQLite), so a restart doesn't re-summarise the whole backlog — on a
    /// slow local model that catch-up degraded the first turns after every deploy — and
    /// the butler keeps the day's thread across restarts.
    pub summary: PersistentSummary,
    /// Whether the node looks into things on its own (`ENDORA_OWN_WORK`, ADR 0094/0095).
    /// Off unless the owner turned it on. When on, the nightly loop leaves the stumpers and
    /// the long-running intention to that lane.
    pub own_work: bool,
    /// The person's distance from UTC in minutes (`ENDORA_UTC_OFFSET_MINUTES`), for patterns
    /// that depend on the time of day. The node stores instants and does not otherwise know
    /// where the person is. Zero unless set.
    pub utc_offset_minutes: i64,
    /// What Endora has made of the mail (ADR 0097): an index of every message and a digest of
    /// those it has read. Always present; empty until there is mail.
    pub mail_store: Arc<endora_mail::MailStore>,
    /// The read-only copy of the mail, when one is mounted.
    pub mail_copy: Option<Arc<endora_mail::MaildirCopy>>,
    /// The quarantined reader: the local model, with no tools and none of the person's context.
    pub mail_reader: Option<Arc<dyn endora_application::UntrustedReader + Send + Sync>>,
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
        // Both resolved before the struct takes ownership of the store and the clock.
        let started_ms = {
            use endora_application::Clock as _;
            clock.now().unix_millis()
        };
        let token = Arc::new(the_nodes_token(&store, &ids));
        // The authenticator secret is made alongside, so the enrolment QR is ready the first
        // time anybody opens the console rather than needing a separate step to bring into
        // existence. Best-effort: a node that cannot store one simply cannot be enrolled, and
        // the bootstrap token still works.
        let _ = ensure_enrolment_secret(&store, &ids);
        say_the_token_if_it_is_still_needed(&store, &token);
        // A small buffer is plenty: subscribers coalesce to a single refresh,
        // and a lagged receiver still gets one "changed" signal.
        let (changes, _) = broadcast::channel(16);
        // Context stores share the one connection the store opened (ADR 0050).
        let chat = Arc::new(endora_conversation::ChatStore::new(store.db()));
        // The running summary is persisted through the same chat store (SQLite).
        let summary = PersistentSummary(chat.clone());
        let schedules = Arc::new(endora_scheduling::ScheduleStore::new(store.db()));
        let understanding = Arc::new(endora_understanding::UnderstandingStore::new(store.db()));
        let config = Arc::new(endora_capabilities::ConfigStore::new(store.db()));
        let audit = Arc::new(endora_platform::AuditStore::new(store.db()));
        let events = Arc::new(endora_platform::EventStore::new(store.db()));
        // Best-effort like the enrolment secret: a node whose mail tables cannot be made still
        // runs, and simply has nothing to say about mail.
        let _ = endora_mail::migrate(&store.db());
        let mail_store = Arc::new(endora_mail::MailStore::new(store.db()));
        // Deployment policy: if a skills config file is set, apply its per-skill modes
        // at startup as the baseline (ADR 0051) — before MCP connects, so MCP-tool
        // modes are in place too.
        if let Ok(path) = std::env::var("ENDORA_SKILLS_CONFIG") {
            if !path.trim().is_empty() {
                apply_skills_config(config.as_ref(), path.trim());
            }
        }
        // Connect any registered MCP servers up front (subprocesses persist across
        // turns). A server that fails to start is skipped, so startup never blocks
        // on a bad one (ADR 0054).
        let mcp = Arc::new(std::sync::RwLock::new(Arc::new(connect_mcp(
            config.as_ref(),
        ))));
        // Built before the struct literal takes ownership of the stores.
        // Endora's own record, as a skill it can reach for. Defined HERE because it
        // reads across four contexts — chat, outcomes, config writes, standing
        // trouble — and the composition root is the only place allowed to know all of
        // them (ADR 0050). The digest itself is a use case in the orchestration layer;
        // this is only the wiring.
        let base_caps = Arc::new({
            let mut all = endora_infrastructure::default_capabilities();
            all.push(Arc::new(WhatIHaveBeenDoing {
                chat: chat.clone(),
                understanding: understanding.clone(),
                config: config.clone(),
                clock: clock.clone(),
            }) as Arc<dyn endora_capabilities::Capability>);
            all
        });
        // The brief skill gathers FROM `base_caps` — a list that does not contain it,
        // so it can never recurse into itself by construction (ADR 0074).
        let capabilities = Arc::new({
            let mut all = (*base_caps).clone();
            all.push(Arc::new(TheWholeBrief {
                chat: chat.clone(),
                understanding: understanding.clone(),
                config: config.clone(),
                clock: clock.clone(),
                base: base_caps,
                mcp: mcp.clone(),
            }) as Arc<dyn endora_capabilities::Capability>);
            all
        });
        Self {
            console: Arc::new(endora_capabilities::infrastructure::terminal::Adapter::new(
                config.clone(),
                store.db(),
            )),
            homelab: Arc::new(std::sync::Mutex::new(
                json!({"observed_at_ms":0,"services":[
                    {"name":"docker","state":"unconfigured","items":[]},
                    {"name":"sonarr","state":"unconfigured","items":[]},
                    {"name":"radarr","state":"unconfigured","items":[]}
                ]}),
            )),
            brain: None,
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
            capabilities,
            mcp,
            turn_lock: Arc::new(tokio::sync::Mutex::new(())),
            summary,
            token,
            started_ms,
            own_work: false,
            utc_offset_minutes: 0,
            mail_store,
            mail_copy: None,
            mail_reader: None,
        }
    }

    /// Gives the node the mail: the read-only copy, and the reader that may look at it.
    #[must_use]
    pub fn with_mail(
        mut self,
        copy: endora_mail::MaildirCopy,
        reader: Arc<dyn endora_application::UntrustedReader + Send + Sync>,
    ) -> Self {
        self.mail_copy = Some(Arc::new(copy));
        self.mail_reader = Some(reader);
        self
    }

    /// Sets the person's distance from UTC, in minutes (negative to the west).
    #[must_use]
    pub fn with_utc_offset_minutes(mut self, minutes: i64) -> Self {
        self.utc_offset_minutes = minutes;
        self
    }

    /// Turns on the record lane: the node starts looking into things on its own.
    #[must_use]
    pub fn with_own_work(mut self, on: bool) -> Self {
        self.own_work = on;
        self
    }

    /// Attaches the concrete brain, for facts only the composition can answer —
    /// which rung is serving (ADR 0072). The `dyn` handle stays the one turns use.
    #[must_use]
    pub fn with_brain(mut self, brain: Arc<endora_infrastructure::ConfigurableButler>) -> Self {
        self.brain = Some(brain);
        self
    }
}

/// The token this node will accept, made once and kept.
///
/// `ENDORA_TOKEN` wins when set, so a deployment can pin it. Otherwise one is generated on
/// first run, stored, and **printed to the log** — the operator has a shell on the machine
/// this runs on, which is the one channel that needs no bootstrapping of its own.
///
/// Two identifiers from the source the rest of the node already uses (v4 UUIDs, so 256 bits
/// of the same randomness) — and **no new dependency** for something a dependency would not
/// make safer.
///
/// If the store cannot be read or written, this returns **empty**, and empty refuses every
/// API route. A node that cannot remember its own credential is not a node that should be
/// answering — failing open here would undo the entire point.
fn the_nodes_token(store: &SqliteStore, ids: &RandomIdSource) -> String {
    if let Ok(pinned) = std::env::var("ENDORA_TOKEN") {
        let pinned = pinned.trim().to_owned();
        if !pinned.is_empty() {
            return pinned;
        }
    }
    match store.node_token() {
        Ok(Some(kept)) => kept,
        Ok(None) => {
            use endora_application::IdSource as _;
            let made = format!("{:032x}{:032x}", ids.new_id(), ids.new_id());
            if store.set_node_token(&made).is_err() {
                eprintln!("endora: could not store a token, so nothing will be let in");
                return String::new();
            }
            made
        }
        Err(e) => {
            eprintln!("endora: could not read the stored token ({e}); refusing everything");
            String::new()
        }
    }
}

/// Applies a deployment **skills config file** (ADR 0051): a JSON object mapping a
/// skill id (built-in, or an MCP tool `server.tool`) to a mode — `"off"`, `"auto"`,
/// or `"ask"`. Applied at startup as the baseline for the listed skills; skills not
/// listed are left to the person's choices. `auto` = enabled, runs per its band;
/// `ask` = enabled but confirmed each use (and, for the un-undoable, allowed to run
/// with confirmation); `off` = disabled. Missing/invalid files are logged and
/// skipped — a bad config never stops the node from starting.
fn apply_skills_config(config: &endora_capabilities::ConfigStore, path: &str) {
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
    // The file's vocabulary was always the real model (ADR 0062); it used to be
    // translated into three flags, and now it is stored as itself.
    for (id, mode) in map {
        match endora_capabilities::Stance::from_word(&mode) {
            Some(stance) => {
                let _ = endora_capabilities::CapabilityConfigRepository::set_stance(
                    config, &id, stance,
                );
            }
            None => {
                eprintln!("skills config: unknown mode '{mode}' for '{id}' (use off|ask|auto)");
            }
        }
    }
}
mod mcp;
use mcp::*;

/// Builds the router for the node's HTTP API.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/styles.css", get(console_css))
        .route("/app.js", get(console_js))
        .route("/health", get(health))
        .route("/v1/homelab", get(homelab::dashboard))
        .route("/v1/chat", post(send_chat).get(chat_history))
        .route("/v1/chat/days", get(chat_days))
        .route("/v1/chat/context", get(chat_context))
        .route("/v1/chat/context/reset", post(reset_chat_context))
        .route("/v1/chat/stream", post(stream_chat))
        .route("/v1/checkin", get(get_checkin).post(set_checkin))
        .route("/v1/understanding", get(list_understanding))
        .route("/v1/understanding/{id}/affirm", post(affirm_belief))
        .route("/v1/understanding/{id}/correct", post(correct_belief))
        .route("/v1/outcomes", get(list_outcomes))
        .route("/v1/repairs", get(list_repairs))
        .route(
            "/v1/aliases",
            get(list_aliases)
                .post(set_alias)
                .delete(forget_alias_everywhere),
        )
        .route("/v1/aliases/upstream", post(push_aliases_upstream))
        .route("/v1/collections", post(make_a_collection))
        .route("/v1/models/worth-knowing", get(models_worth_knowing))
        .route("/v1/context", get(what_the_butler_is_told))
        .route("/v1/reliability", get(how_it_has_been_landing))
        // What is already set up, so the screen can say so instead of offering to do it
        // again — a Connect button beside a working calendar reads as a failed attempt.
        // Signing in. The POST answers unsigned by design (see `sign_in`); the setup pair
        // sits behind the bootstrap token, because enrolling is claiming the account.
        .route("/v1/session", get(whether_sign_in_exists).post(sign_in))
        .route("/v1/session/setup", get(sign_in_setup).post(set_sign_in))
        // A read-only credential for `make smoke`, behind a full one.
        .route("/v1/session/checks", post(mint_a_checking_token))
        .route("/v1/session/dashboard", post(mint_a_dashboard_token))
        .route("/v1/homelab/overview", get(homelab::overview))
        .route("/v1/terminal/home", get(terminal::home))
        .route("/v1/terminal/home/catalog", get(terminal::catalog))
        .route("/v1/terminal/media", get(terminal::media))
        .route("/v1/terminal/media/requests", get(terminal::requests))
        .route("/v1/terminal/containers", get(terminal::containers))
        .route("/v1/terminal/dashboard", get(terminal::dashboard))
        .route("/v1/terminal/logs/{id}", get(terminal::logs))
        .route("/v1/terminal/jobs", get(terminal::jobs))
        .route("/v1/terminal/jobs/{id}", get(terminal::job))
        .route("/v1/terminal/action", post(terminal::action))
        .route("/v1/terminal/operations/{id}", get(terminal::operation))
        .route(
            "/v1/repairs/{server}/{thing}/leave",
            post(leave_this_one_alone),
        )
        // Managing your own credentials. Every one of these needs a full session, and the two
        // that change something need the current password as well.
        .route("/v1/session/password", post(change_password))
        .route("/v1/session/authenticator", post(begin_new_authenticator))
        .route(
            "/v1/session/authenticator/confirm",
            post(confirm_new_authenticator),
        )
        .route("/v1/session/bootstrap", get(reveal_the_bootstrap_token))
        .route("/v1/connect/connected", get(what_is_connected))
        .route("/v1/connect/begin", post(begin_connecting))
        .route("/v1/connect/finish", post(finish_connecting))
        .route("/v1/standing-trouble", get(list_standing_trouble))
        .route("/v1/standing-trouble/answer", post(answer_standing_trouble))
        .route("/v1/config-writes", get(list_config_writes))
        .route("/v1/config-writes/{id}/undo", post(undo_config_write))
        .route("/v1/outcomes/{id}/reaction", post(react_to_outcome))
        // Read and drop only. There is deliberately NO create or edit route: Endora
        // forms its own intentions, and the person's whole side of the interface is
        // "stop doing that" (ADR 0052).
        .route("/v1/intentions", get(list_intentions))
        // Readable, never actionable — no companion POST exists (ADR 0057).
        .route("/v1/notions", get(list_notions))
        .route("/v1/intentions/{id}/drop", post(drop_intention))
        .route("/v1/mail", get(mail::mail_digest))
        .route("/v1/goals", get(list_goals))
        .route("/v1/goals/{request_key}", get(get_goal))
        .route("/v1/goals/{request_key}/cancel", post(cancel_goal))
        .route("/v1/research/readiness", get(research_readiness))
        .route("/v1/research", get(list_research).post(commission_research))
        .route("/v1/research/{id}", get(get_research))
        .route("/v1/research/{id}/cancel", post(cancel_research))
        .route("/v1/research/{id}/report", get(research_report))
        .route("/v1/capabilities", get(list_capabilities))
        .route("/v1/capabilities/{id}/invoke", post(invoke_capability))
        .route("/v1/capabilities/{id}/test", post(test_capability))
        .route("/v1/capabilities/{id}/enable", post(set_capability_enabled))
        .route("/v1/capabilities/{id}/open", post(set_capability_open))
        .route(
            "/v1/mcp/servers/{name}/tools/open",
            post(set_every_tool_open),
        )
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
        .route("/v1/mcp/servers/{name}/test", post(test_mcp_server))
        .route(
            "/v1/capabilities/{id}/config",
            post(set_capability_settings),
        )
        .route("/v1/recipes", get(list_recipes).post(create_recipe))
        .route("/v1/recipes/try", post(try_recipe))
        .route("/v1/recipes/{id}", axum::routing::delete(remove_recipe))
        .route("/v1/recipes/{id}/enable", post(set_recipe_enabled))
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
        .route("/v1/deep-model/usage", get(deep_usage))
        .route("/v1/deep-model/balance", get(deep_balance))
        .route("/v1/deep-model/prices", post(set_deep_prices))
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
        // Outermost, so nothing behind it runs for a request that has no business being here.
        .layer(from_fn_with_state(state.clone(), require_the_token))
        .with_state(state)
}
mod auth;
mod homelab;
mod terminal;
use auth::*;
pub(crate) use homelab::spawn as spawn_homelab;

mod platform;
use platform::*;

fn build_runner(
    config: &endora_capabilities::ConfigStore,
    capabilities: Arc<Vec<Arc<dyn Capability>>>,
    mcp: Arc<endora_capabilities::McpRunner>,
    proven: std::collections::HashSet<String>,
) -> endora_capabilities::WithdrawnRunner {
    let stances =
        endora_capabilities::CapabilityConfigRepository::stances(config).unwrap_or_default();
    // `off` is off for every kind of capability, whichever source offers it (ADR 0062).
    let withdrawn: std::collections::HashSet<String> = stances
        .iter()
        .filter(|(_, st)| *st == endora_capabilities::Stance::Off)
        .map(|(id, _)| id.clone())
        .collect();
    let envelope = AutonomyEnvelopeRepository::get(config).unwrap_or_default();
    let auto_consequential = envelope.auto_consequential;
    // Fresh per turn so a stance, envelope or settings change takes effect at once.
    let registry = endora_infrastructure::RegistryRunner::with_config(
        capabilities,
        stances.clone(),
        proven.clone(),
        envelope,
        settings_map(config),
    );
    // The person's own recipes (ADR 0071) — read fresh, same as the built-in
    // registry above, so authoring or enabling one takes effect on the very next
    // turn. `RecipeRunner` holds the ONE difference from every other Observe-band
    // skill (absence defaults to `off`, not `Auto`) entirely inside itself; the
    // stances handed to it are the same stored rows every other source reads.
    let recipe_source = endora_capabilities::RecipeRunner::new(
        endora_capabilities::RecipeRepository::recipes(config).unwrap_or_default(),
        stances.clone(),
    );
    // The same stances overlaid on the shared MCP runner, without rebuilding the
    // connection: `ask` confirms each use and graduates when proven (ADR 0062).
    // The servers Endora has a relationship with — their readings are the house, not a
    // stranger's words (ADR 0064, drawing 0058's line).
    let the_house: std::collections::HashSet<String> = native_channels(config)
        .into_iter()
        .map(|(server, _)| server)
        .collect();
    let mcp_source = endora_capabilities::OpenerRunner::new(
        mcp as Arc<dyn endora_capabilities::CapabilityRunner + Send + Sync>,
        the_house,
        stances.into_iter().collect(),
        proven,
        auto_consequential,
    );
    // Confirmed target aliases (ADR 0054), so a call that fails on a name the person has
    // already explained gets one retry with their answer — see `AliasRunner`.
    let aliases: Vec<(String, String, String)> =
        endora_capabilities::TargetAliasRepository::aliases(config)
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.server, a.said, a.means))
            .collect();
    // Built-in skills + connected MCP servers, behind one runner. The application
    // never learns a tool's origin (ADR 0054).
    // Confirmed answers first, then observed ones — ADR 0054's trust ranking, expressed
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
        Arc::new(recipe_source) as Arc<dyn endora_capabilities::CapabilityRunner + Send + Sync>,
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
    proven: std::collections::HashSet<String>,
) -> endora_capabilities::ReversibleOnlyRunner {
    endora_capabilities::ReversibleOnlyRunner::new(Arc::new(build_runner(
        config,
        capabilities,
        mcp,
        proven,
    )))
}

/// What the record has proven, read from outcomes (ADR 0062). Derived at composition,
/// never stored; an unreadable store proves nothing, which is the safe direction.
fn proven_now(
    understanding: &endora_understanding::UnderstandingStore,
) -> std::collections::HashSet<String> {
    endora_understanding::OutcomeRepository::list(understanding)
        .map(|all| usecases::proven_by_the_record(&all))
        .unwrap_or_default()
}

/// The servers Endora has **direct reach** into (ADR 0054) — its own connection to a
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
    // Every declared skill is asked whether it speaks for a service Endora needs to watch;
    // almost all of them say no. Nothing here knows an integration by name.
    //
    // It used to: this function reached for Home Assistant's settings directly and
    // `return`ed an empty list if they were missing, so a second local integration could
    // never have been reached even once it existed. Registration now runs off the same list
    // every skill is already declared in, which makes adding one a single line there rather
    // than a change to the composition root (ADR 0050 — the node composes, it does not know).
    endora_capabilities::channels_of(
        &endora_infrastructure::default_capabilities(),
        &settings_map(config),
        &endora_capabilities::TargetAliasRepository::aliases(config).unwrap_or_default(),
    )
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

/// Where the person's current topic begins (ms), or 0 for the whole chat (ADR 0088). Kept as
/// one stored setting rather than a column, because it is a choice about what the model
/// reads, not part of any message.
fn topic_start(config: &endora_capabilities::ConfigStore) -> i64 {
    settings_map(config)
        .get("chat")
        .and_then(|m| m.get("context_from_ms"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// The model's window in tokens. The node cannot ask the model server, so it is told:
/// `ENDORA_CONTEXT_TOKENS`, defaulting to what the Ollama role configures.
fn context_window_tokens() -> usize {
    std::env::var("ENDORA_CONTEXT_TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n >= 1024)
        .unwrap_or(16_384)
}
mod chat;
use chat::*;

mod life;

mod mail;
use life::*;

mod trouble;
use trouble::*;

mod skills;
use skills::*;

mod models;
use models::*;

mod workers;
pub use workers::*;

mod misc;
use misc::*;

#[cfg(test)]
mod goal_lanes;
#[cfg(test)]
mod mail_reading;
#[cfg(test)]
mod own_work;
#[cfg(test)]
mod tests;
