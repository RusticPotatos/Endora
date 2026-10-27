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
use axum::routing::{delete, get, post, put};
use endora_application::Clock;
use endora_application::{
    ActivityItem, AppError, AuditRecord, CapabilityConfigRepository, CapabilitySettingsRepository,
    IdSource, MemorySnapshot, RepositoryError, usecases,
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
    /// The capabilities context's config store (settings, stances, model lists, tool servers).
    pub config: Arc<endora_capabilities::ConfigStore>,
    /// The platform context's audit trail store.
    pub audit: Arc<endora_platform::AuditStore>,
    /// The platform context's event-log store.
    pub events: Arc<endora_platform::EventStore>,
    /// The identifier source.
    pub ids: Arc<RandomIdSource>,
    /// The system clock.
    pub clock: Arc<SystemClock>,
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
    /// The house's skills (weather, web, the lab, …), which the team's bridge offers its turns
    /// with each one's stance (ADR 0108).
    pub capabilities: Arc<Vec<Arc<dyn Capability>>>,
    /// Connected MCP servers, as a runner (ADR 0054). Long-lived (subprocesses),
    /// so it is connected once at startup and shared; a registry change rebuilds it and swaps
    /// it in behind the lock. The bridge offers their tools too (ADR 0108).
    pub mcp: Arc<std::sync::RwLock<Arc<endora_capabilities::McpRunner>>>,
    /// The person's time zone (`ENDORA_TIME_ZONE`, or the fixed `ENDORA_UTC_OFFSET_MINUTES`),
    /// for the nightly copies' hour. The node stores instants and does not otherwise know where
    /// the person is. UTC unless set.
    pub time_zone: endora_infrastructure::TimeZone,
    /// Endora HQ, when this node has one (ADR 0102): the team service, which answers the node
    /// only. `None` until `ENDORA_HQ_URL` and `ENDORA_HQ_TOKEN` are set.
    pub hq: Option<Arc<endora_infrastructure::HqClient>>,
    /// endora-sound, when this node has one (ADR 0104): the sound hub, and the listen
    /// tickets handed out for it. `None` until `ENDORA_SOUND_URL` and a token are set.
    pub sound: Option<Arc<SoundRemote>>,
    /// Where HQ's model relay sends a call: Anthropic, always, outside tests (ADR 0102). A
    /// field rather than a setting, so nothing can point the owner's key anywhere else.
    pub relay_upstream: Arc<String>,
    /// The model the node was installed with, as `(url, model)` from `ENDORA_MODEL_URL` and
    /// `ENDORA_MODEL`: where HQ's local relay sends a turn's call when no Everyday entry is on
    /// the house's network (ADR 0102). A field, like `relay_upstream`, so a test can point it
    /// at a fake.
    pub installed_model: Arc<(String, String)>,
    /// The turns with a call to Anthropic in flight through HQ's model relay: a turn's calls
    /// go one at a time, so each is capped by what is left once the one before was charged.
    pub(crate) relay_calls: Arc<relay::TurnCalls>,
    /// The bench's credential (ADR 0123), when `ENDORA_BENCH_TOKEN_FILE` names one: a token
    /// that opens the model relay alone, for measuring an external agent harness, metered as
    /// `bench` and capped by the day. `None`: no bench.
    pub(crate) bench: Option<Arc<bench::Bench>>,
    /// How the home lab is built (ADR 0112): the lab's ansible-nas docs, the owner's retired
    /// list, and the map the team reads, kept a minute. From the environment
    /// (`ENDORA_ANSIBLE_NAS_REPO`, `ENDORA_ANSIBLE_NAS_REF`, `ENDORA_LAB_RETIRED`), read by
    /// [`with_lab_from_env`](Self::with_lab_from_env); none until then.
    pub(crate) lab: Arc<contracts::LabKnowledge>,
    /// The house's vault and the passkeys that unlock it (ADR 0127). Not configured until
    /// `ENDORA_VAULT_URL` is set ([`with_vault`](Self::with_vault)).
    pub vault: Arc<crate::vault::Vault>,
    /// What the node last heard from the owner's Mac, and what HQ was last told of it
    /// (ADR 0130). In memory: the Mac says it again every thirty seconds.
    pub(crate) mac: Arc<mac::MacBrain>,
}

impl AppState {
    /// Creates the shared state, wiring up the change-broadcast channel.
    #[must_use]
    pub fn new(store: Arc<SqliteStore>, ids: Arc<RandomIdSource>, clock: Arc<SystemClock>) -> Self {
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
        let config = Arc::new(endora_capabilities::ConfigStore::new(store.db()));
        let audit = Arc::new(endora_platform::AuditStore::new(store.db()));
        let events = Arc::new(endora_platform::EventStore::new(store.db()));
        // Deployment policy: if a skills config file is set, apply its per-skill modes
        // at startup as the baseline (ADR 0051) — before MCP connects, so MCP-tool
        // modes are in place too.
        if let Ok(path) = std::env::var("ENDORA_SKILLS_CONFIG") {
            if !path.trim().is_empty() {
                apply_skills_config(config.as_ref(), path.trim());
            }
        }
        // Connect any registered MCP servers up front (subprocesses persist). A server that
        // fails to start is skipped, so startup never blocks on a bad one (ADR 0054).
        let mcp = Arc::new(std::sync::RwLock::new(Arc::new(connect_mcp(
            config.as_ref(),
        ))));
        let capabilities = Arc::new(endora_infrastructure::default_capabilities());
        let vault = Arc::new(crate::vault::Vault::new(
            None,
            store.clone(),
            None,
            crate::vault::DEFAULT_RP_ID,
        ));
        Self {
            vault,
            mac: Arc::default(),
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
            store,
            config,
            audit,
            events,
            ids,
            clock,
            changes,
            capabilities,
            mcp,
            token,
            started_ms,
            time_zone: endora_infrastructure::TimeZone::fixed(0),
            hq: None,
            sound: None,
            relay_upstream: Arc::new(endora_infrastructure::relay::ANTHROPIC.to_owned()),
            relay_calls: Arc::default(),
            bench: None,
            lab: Arc::new(contracts::LabKnowledge::unconfigured()),
            // Read as `main` reads them, with its defaults.
            installed_model: Arc::new((
                std::env::var("ENDORA_MODEL_URL")
                    .unwrap_or_else(|_| "http://localhost:11434/v1".to_owned()),
                std::env::var("ENDORA_MODEL").unwrap_or_else(|_| "qwen2.5:7b".to_owned()),
            )),
        }
    }

    /// Sets the person's time zone.
    #[must_use]
    pub fn with_time_zone(mut self, time_zone: endora_infrastructure::TimeZone) -> Self {
        self.time_zone = time_zone;
        self
    }

    /// Connects Endora HQ (ADR 0102).
    #[must_use]
    pub fn with_hq(mut self, hq: endora_infrastructure::HqClient) -> Self {
        self.hq = Some(Arc::new(hq));
        self
    }

    /// Opens the bench (ADR 0123) from `ENDORA_BENCH_TOKEN_FILE` and `ENDORA_BENCH_CAP_CENTS`,
    /// when a file is named; says so, never the token.
    #[must_use]
    pub fn with_bench_from_env(self) -> Self {
        let Some(bench) = bench::Bench::from_env() else {
            return self;
        };
        #[allow(clippy::cast_precision_loss)]
        let dollars = bench.cap_micros() as f64 / 1_000_000.0;
        println!(
            "bench: a credential for the model relay alone, Haiku or Sonnet, ${dollars:.2} a day"
        );
        self.with_bench(bench)
    }

    /// Reads how the home lab is built (ADR 0112) from the environment: the ansible-nas fork,
    /// the retired list and the deploy key. Until then the node has no fork.
    #[must_use]
    pub fn with_lab_from_env(mut self) -> Self {
        self.lab = Arc::new(contracts::LabKnowledge::from_env());
        self
    }

    pub(crate) fn with_bench(mut self, bench: bench::Bench) -> Self {
        self.bench = Some(Arc::new(bench));
        self
    }

    /// Gives the node its vault (ADR 0127).
    #[must_use]
    pub fn with_vault(mut self, vault: crate::vault::Vault) -> Self {
        self.vault = Arc::new(vault);
        self
    }

    /// Connects the sound hub (ADR 0104).
    #[must_use]
    pub fn with_sound(mut self, sound: endora_infrastructure::SoundClient) -> Self {
        self.sound = Some(Arc::new(SoundRemote::new(sound)));
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
        // Endora HQ, through the node (ADR 0102), and the office's art.
        .route("/hq-art/{*file}", get(hq_art))
        .route("/v1/hq", get(hq_snapshot))
        .route("/v1/hq/events", get(hq_events))
        // Replay: a turn, or a task's turns, read back in full (the owner's only).
        .route("/v1/hq/runs/{id}/replay", get(hq_run_replay))
        .route("/v1/hq/tasks/{id}/replay", get(hq_task_replay))
        .route(
            "/v1/hq/tasks/{id}/consultations",
            get(hq_task_consultations),
        )
        .route("/v1/hq/goals", post(hq_goal))
        .route("/v1/hq/tasks", post(hq_task))
        .route("/v1/hq/desk/{id}", post(hq_decide))
        // The owner's reply on a task, and a goal's notes for the team.
        .route("/v1/hq/tasks/{id}/post", post(hq_reply))
        .route("/v1/hq/tasks/{id}/project", post(hq_task_project))
        .route("/v1/hq/goals/{id}/note", post(hq_goal_note))
        // A goal's strategy, the owner's edit of the CEO's (ADR 0120).
        .route("/v1/hq/goals/{id}/strategy", post(hq_goal_strategy))
        // What a member thinks with (ADR 0109).
        .route("/v1/hq/members/{id}/brain", post(hq_set_brain))
        .route("/v1/hq/members/{id}/model", post(hq_set_model))
        .route("/v1/hq/members/{id}/work-brain", post(hq_set_work_brain))
        .route(
            "/v1/hq/members/{id}/judgment-brain",
            post(hq_set_judgment_brain),
        )
        // What runs a member's turns (ADR 0117).
        .route("/v1/hq/members/{id}/runtime", post(hq_set_runtime))
        // The owner renames a member, and keeps the CEO's house log with it (ADR 0111).
        .route("/v1/hq/members/{id}/name", post(hq_rename))
        .route("/v1/hq/house", post(hq_house_log))
        // The team's world (ADR 0122): the owner reads it, downloads it, and alone makes
        // something canon or demotes it.
        .route("/v1/hq/world", get(hq_world))
        .route("/v1/hq/world/export.md", get(hq_world_export))
        .route("/v1/hq/world/section", post(hq_world_section))
        .route("/v1/hq/world/timeline", post(hq_world_timeline))
        .route("/v1/hq/world/promote", post(hq_world_promote))
        .route("/v1/hq/world/demote", post(hq_world_demote))
        .route("/v1/hq/world/proposals/decide", post(hq_world_proposal))
        // The guards (ADR 0124): the owner's verdict on a finding.
        .route(
            "/v1/hq/guards/findings/{id}/verdict",
            post(hq_finding_verdict),
        )
        // A guard that is tried in shadow before it acts (ADR 0133): on, or back in shadow.
        .route("/v1/hq/guards/{guard}/mode", post(hq_guard_mode))
        // Retract and sweep (ADR 0124): "Not true", what else says it, and forgetting it.
        .route("/v1/hq/retractions", post(hq_retract))
        .route("/v1/hq/retractions/{id}/sweep", get(hq_sweep))
        .route("/v1/hq/retractions/{id}/forget", post(hq_forget_swept))
        .route("/v1/hq/stop", post(hq_stop))
        .route("/v1/hq/resume", post(hq_resume))
        // The owner's routines (ADR 0107).
        .route("/v1/hq/routines", post(hq_add_routine))
        .route("/v1/hq/routines/{id}", post(hq_switch_routine))
        .route("/v1/hq/routines/{id}/fire", post(hq_fire_routine))
        .route("/v1/hq/routines/{id}/project", post(hq_routine_project))
        // The owner's projects, and whom members report to (ADR 0113).
        .route("/v1/hq/projects", post(hq_add_project))
        .route("/v1/hq/projects/{id}", post(hq_change_project))
        .route("/v1/hq/projects/{id}/cap", post(hq_project_cap))
        .route("/v1/hq/projects/{id}/tools", post(hq_project_tools))
        .route("/v1/hq/projects/{id}/rank", post(hq_loop_rank))
        .route("/v1/hq/one-team", post(hq_one_team))
        .route("/v1/hq/scenarios", post(hq_start_scenario))
        .route("/v1/hq/scenarios/rerun", post(hq_rerun_holodeck))
        .route("/v1/hq/league", post(hq_start_league))
        .route("/v1/hq/scenarios/{task}", get(hq_scenario))
        .route("/v1/hq/trials", post(hq_start_trial))
        .route("/v1/hq/trials/{name}", get(hq_trial))
        .route("/v1/hq/games/{id}/replay", get(hq_game_replay))
        .route("/v1/hq/members/{id}/reports_to", post(hq_reports_to))
        // Elastic staffing (ADR 0118): the owner furloughs and recalls; there is no route that
        // removes a member.
        .route("/v1/hq/members/{id}/furlough", post(hq_furlough))
        .route("/v1/hq/members/{id}/recall", post(hq_recall))
        // The owner as landlord of the office (ADR 0119): only these three routes. (Downtime is
        // work on the house's own model and costs nothing, so there is no allowance, ADR 0131.)
        .route("/v1/hq/office/undo", post(hq_office_undo))
        .route("/v1/hq/office/reset", post(hq_office_reset))
        .route("/v1/hq/office/proposals/{id}/veto", post(hq_office_veto))
        // The owner tops the money pool up for one day (ADR 0113 rule 6).
        .route("/v1/hq/pool/top-up", post(hq_pool_top_up))
        // A member's notebook (ADR 0118): the owner reads it and writes, edits and deletes notes.
        .route(
            "/v1/hq/members/{id}/notebook",
            get(hq_notebook).post(hq_note_add),
        )
        .route(
            "/v1/hq/members/{id}/notebook/{note}",
            post(hq_note_edit).delete(hq_note_delete),
        )
        // The owner hires (ADR 0115), and downloads a branch's bundle when the node cannot push.
        .route("/v1/hq/members", post(hq_hire))
        .route(
            "/v1/hq/bundles/{task}/{slug}",
            get(contracts::bundle_download),
        )
        // The owner's switches for the team's contracts (ADR 0106).
        .route(
            "/v1/hq/contracts",
            get(contracts::switches).post(contracts::switch),
        )
        // endora-sound, through the node (ADR 0104): only these routes, never a path passed on.
        .route("/v1/sound", get(sound_state))
        .route("/v1/sound/sources", post(sound_add_source))
        .route("/v1/sound/sources/{id}", delete(sound_remove_source))
        .route("/v1/sound/sources/{id}/level", put(sound_set_all))
        .route("/v1/sound/speakers", post(sound_add_speaker))
        .route("/v1/sound/speakers/{id}", delete(sound_remove_speaker))
        .route("/v1/sound/speakers/{id}/volume", put(sound_set_volume))
        .route("/v1/sound/speakers/{id}/sync", put(sound_set_sync))
        .route("/v1/sound/speakers/{id}/free", post(sound_free))
        .route("/v1/sound/found", get(sound_found))
        .route("/v1/sound/wires", put(sound_wire))
        .route("/v1/sound/wires/{source}/{speaker}", delete(sound_unwire))
        .route("/v1/sound/setups", post(sound_save_setup))
        .route("/v1/sound/setups/{name}", delete(sound_remove_setup))
        .route("/v1/sound/setups/{name}/apply", post(sound_apply_setup))
        .route("/v1/sound/listen/{id}/ticket", post(sound_listen_ticket))
        .route("/v1/sound/listen/{id}", get(sound_listen))
        .route("/v1/sound/live/{id}", get(sound_live))
        .route("/v1/sound/speakers/{id}/probe", post(sound_probe))
        // The owner's Mac as a brain for the team (ADR 0130). The heartbeat answers the
        // Mac's helper alone, by a token of its own (`crate::auth::Scope::Mac`).
        .route("/v1/mac", get(mac::status))
        .route("/v1/mac/heartbeat", post(mac::heartbeat))
        .route("/v1/mac/settings", post(mac::change))
        .route("/v1/mac/token", post(mac::make_token))
        .route("/v1/collections", post(make_a_collection))
        .route("/v1/models/worth-knowing", get(models_worth_knowing))
        // What is already set up, so the screen can say so instead of offering to do it
        // again — a Connect button beside a working calendar reads as a failed attempt.
        // Signing in. The POST answers unsigned by design (see `sign_in`); the setup pair
        // sits behind the bootstrap token, because enrolling is claiming the account.
        .route("/v1/session", get(whether_sign_in_exists).post(sign_in))
        .route("/v1/session/setup", get(sign_in_setup).post(set_sign_in))
        // A read-only credential for `make smoke`, behind a full one.
        // The vault and its passkey unlock (ADR 0127). Only the status and the unlock answer
        // without a sign-in (`crate::auth::may_pass`); the passkey is the unlock's credential.
        .route("/v1/vault", get(vault::status))
        .route("/v1/vault/unlock/begin", post(vault::begin_unlock))
        .route("/v1/vault/unlock", post(vault::unlock))
        .route("/v1/vault/passkeys", get(vault::passkeys))
        .route("/v1/vault/passkeys/begin", post(vault::begin_registration))
        .route(
            "/v1/vault/passkeys/finish",
            post(vault::finish_registration),
        )
        .route(
            "/v1/vault/passkeys/{id}/remove",
            post(vault::remove_passkey),
        )
        .route("/v1/vault/ceremony", post(vault::begin_ceremony))
        .route("/v1/vault/initialise", post(vault::initialise))
        .route("/v1/vault/enrol", post(vault::enrol))
        .route("/v1/vault/adopt", post(vault::adopt))
        .route("/v1/vault/recover", post(vault::recover))
        .route("/v1/vault/keys-into-vault", post(vault::keys_into_vault))
        .route("/v1/vault/start-over", post(vault::start_over))
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
        .route("/v1/config-writes", get(list_config_writes))
        .route("/v1/config-writes/{id}/undo", post(undo_config_write))
        .route("/v1/ai", get(ai::ai_overview))
        // Spotify, held here and lent to every device; its callback is outside /v1 because a
        // browser redirect carries no token (the waiting sign-in's state authorizes it).
        .route("/v1/connections/spotify", get(spotify::status))
        .route("/v1/connections/spotify/begin", post(spotify::begin))
        .route("/v1/connections/spotify/finish", post(spotify::finish))
        .route("/v1/connections/spotify/token", post(spotify::token))
        .route(
            "/v1/connections/spotify/take-over",
            post(spotify::take_over),
        )
        .route(
            "/v1/connections/spotify/disconnect",
            post(spotify::disconnect),
        )
        .route("/connect/spotify/callback", get(spotify::callback))
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
        .route("/v1/model-lists", get(model_lists::lists))
        .route("/v1/model-lists/providers", post(model_lists::add_provider))
        .route(
            "/v1/model-lists/providers/{id}",
            post(model_lists::update_provider),
        )
        .route(
            "/v1/model-lists/providers/{id}/remove",
            post(model_lists::remove_provider),
        )
        .route(
            "/v1/model-lists/providers/{id}/discover",
            post(model_lists::discover),
        )
        .route("/v1/model-lists/entries", post(model_lists::add_entry))
        .route(
            "/v1/model-lists/entries/{id}/{action}",
            post(model_lists::entry_action),
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
        // HQ's model relays and the team's contracts are not here: workers reach only HQ and
        // the team listener ([`team_relay`]), never the node's main port.
        .with_state(state)
}

/// HQ's model relays (ADR 0102, phase 4), the team's contracts (ADR 0106) and the team gateway
/// (ADR 0110), and nothing else of the node: what the plain-HTTP team listener serves
/// (`ENDORA_TEAM_ADDR`), and the only place they are served. A worker has no way to trust the
/// node's self-signed certificate, and a turn's own token is its credential, not the owner's
/// sign-in. Every route here needs a token: a turn's, or the runner's for its routes.
pub fn team_relay(state: AppState) -> Router {
    // The bench's token (ADR 0123) opens the model relay and nothing else here.
    let fenced = relay_routes().route_layer(from_fn_with_state(state.clone(), bench::fenced));
    Router::new()
        .route("/team-relay/v1/messages", post(relay::messages))
        .route(
            "/team-relay/v1/messages/count_tokens",
            post(relay::count_tokens),
        )
        .route("/team-relay/bench", get(bench::today_so_far))
        .merge(fenced)
        .with_state(state)
}

/// The most a contract call may carry: a submitted bundle (8 MB) as base64, and its words.
const CONTRACT_CALL_MOST: usize = 12 * 1024 * 1024;

/// The most a worker's account of its turn may carry: the node holds what it keeps to a
/// megabyte, and refuses what is far past it.
const TRANSCRIPT_CALL_MOST: usize = 6 * 1024 * 1024;

/// The team listener's routes but the model relay's own two.
fn relay_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/team-relay/local/v1/chat/completions",
            post(relay::chat_completions),
        )
        // The team's contracts (ADR 0106): what Endora offers a turn on the home lab.
        .route("/team-relay/contracts", get(contracts::list))
        // A submitted bundle rides in the call (ADR 0115): room for it, and not much more.
        .route(
            "/team-relay/contracts/{name}",
            post(contracts::call).layer(axum::extract::DefaultBodyLimit::max(CONTRACT_CALL_MOST)),
        )
        // What a Claude Code turn's sandbox says its own tools did (ADR 0117), by its hook.
        .route("/team-relay/receipt", post(reported::sandbox_receipt))
        // What a turn said, told by its worker at the turn's end (ADR 0129): scrubbed, capped,
        // and passed to HQ, which keeps it for thirty days. Room for a megabyte of text, and
        // a worker that did not hold it to that.
        .route(
            "/team-relay/transcript",
            post(reported::turn_transcript)
                .layer(axum::extract::DefaultBodyLimit::max(TRANSCRIPT_CALL_MOST)),
        )
        // A check job's bundle (ADR 0115), opened by the job's own token.
        .route(
            "/team-relay/check/{id}/bundle",
            get(contracts::check_bundle),
        )
        // The team gateway (ADR 0110): HQ's two doors for callers on the house's network,
        // passed on untouched.
        .route("/team-gateway/mcp", axum::routing::any(gateway::mcp))
        .route("/team-gateway/v2/mcp", axum::routing::any(gateway::mcp_v2))
        .route(
            "/team-gateway/runner/{*route}",
            axum::routing::any(gateway::runner),
        )
}
mod auth;
mod homelab;
mod mac;
mod terminal;
mod vault;
use auth::*;
pub(crate) use homelab::spawn as spawn_homelab;
pub(crate) use mac::spawn as spawn_mac;

mod platform;
use platform::*;

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

mod hq;
// HQ's model relay: a team member's only way to a model (ADR 0102, phase 4).
mod relay;
// The bench's credential: the model relay alone, for an external agent harness (ADR 0123).
mod bench;
// The team's contracts: what Endora offers HQ's workers on the home lab (ADR 0106).
mod contracts;
// The bridge: Endora's capabilities as the team's tools (ADR 0108).
mod bridge;
// What a Claude Code turn's sandbox reports its own tools did (ADR 0117).
mod reported;
// The team gateway: HQ's doors for the Pi, passed on (ADR 0110).
mod gateway;
// The providers and ordered model lists on the AI screen (ADR 0105).
mod model_lists;
// The agenda loop (ADR 0103): signals become items, and the owner's answers to them.
use hq::*;
mod hq_art;
use hq_art::*;
mod sound;
use sound::*;

mod trouble;
use trouble::*;

mod skills;
use skills::*;

mod workers;
// The AI screen: every model Endora uses, where it runs and what it sees.
mod ai;
// Spotify, held by the node and lent to every device.
mod spotify;
// Endora's nightly copies of its own database.
mod snapshots;
pub use snapshots::spawn_snapshots;
pub use workers::*;

mod misc;
use misc::*;

#[cfg(test)]
mod tests;
