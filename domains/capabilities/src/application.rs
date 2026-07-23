//! Capabilities application layer — ports for running and configuring skills.

use endora_kernel::{Decision, RepositoryError};

pub use crate::domain::{AutonomyEnvelope, McpServer, McpTransport};

/// An optional **deep model** — a bigger/cloud AI the person configures for hard
/// questions the local model can't handle well (like a phone escalating to a bigger
/// brain). Off unless configured. The key is a secret, stored server-side and never
/// returned to a client.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeepModel {
    /// OpenAI-compatible base URL (`.../v1`).
    pub url: String,
    /// Model name to request.
    pub model: String,
    /// API key sent as a bearer token (empty for keyless/local endpoints).
    pub api_key: String,
}

/// Persists the single [`DeepModel`] configuration.
pub trait DeepModelRepository {
    /// Returns the configured deep model, or `None` if unset.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn get(&self) -> Result<Option<DeepModel>, RepositoryError>;

    /// Stores the deep model configuration.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, model: &DeepModel) -> Result<(), RepositoryError>;
}

/// Sampling parameters for one model call. Every field is optional — a `None`
/// leaves that knob to the endpoint's own default. `temperature` and `top_p` are
/// standard OpenAI-compatible fields honoured everywhere; `top_k` and
/// `repeat_penalty` are non-standard extensions honoured by local runtimes
/// (Ollama) but rejected by strict cloud endpoints, so providers that need them
/// off leave them unset. See ADR 0027 — the discovery loop tunes these per slot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sampling {
    /// Randomness. Lower = more deterministic. Router wants this cold (~0.0–0.2)
    /// for reliable skill selection; the synthesizer wants it warmer for prose.
    pub temperature: Option<f64>,
    /// Nucleus sampling cutoff.
    pub top_p: Option<f64>,
    /// Top-k cutoff (Ollama/local only).
    pub top_k: Option<u32>,
    /// Repetition penalty (Ollama/local only).
    pub repeat_penalty: Option<f64>,
}

/// One model "slot": the model name plus its sampling parameters. The base URL
/// and API key live once on the parent [`ButlerModelConfig`] (all slots share an
/// endpoint).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelSlot {
    /// Model name to request (e.g. `qwen2.5:7b`, `gpt-4o-mini`).
    pub model: String,
    /// Sampling parameters for this slot.
    pub sampling: Sampling,
}

/// The butler's model configuration, editable at runtime from the console
/// (ADR 0027). Shared endpoint + key; either a single model or the router +
/// synthesizer mixture. The key is a secret stored server-side and never
/// returned to a client. When unset, the node falls back to its environment
/// configuration.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ButlerModelConfig {
    /// OpenAI-compatible base URL (`.../v1`) shared by every slot.
    pub base_url: String,
    /// API key sent as a bearer token (empty for keyless/local endpoints).
    pub api_key: String,
    /// `true` runs the router + synthesizer mixture; `false` a single model.
    pub mixture: bool,
    /// The single-model slot (used when `mixture` is false).
    pub single: ModelSlot,
    /// The router slot — a tool-tuned specialist that picks skills.
    pub router: ModelSlot,
    /// The synthesizer slot — a generalist that writes the reply.
    pub synth: ModelSlot,
}

/// A schedule for the self-improving model tune (ADR 0027) — off by default.
/// When on, the heartbeat runs the local-model evaluation + gated adoption once a
/// day at `hour_utc`; pick an off-hour so the eval doesn't contend with chat on
/// the GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTuneSchedule {
    /// Whether the nightly tune is on.
    pub enabled: bool,
    /// The UTC hour (0–23) to run it.
    pub hour_utc: u8,
    /// When it last ran (so it fires once per day).
    pub last_ms: i64,
}

impl ModelTuneSchedule {
    /// Off, defaulting to 4am UTC — a quiet hour.
    #[must_use]
    pub const fn disabled_default() -> Self {
        Self {
            enabled: false,
            hour_utc: 4,
            last_ms: 0,
        }
    }

    /// Whether the tune is due: enabled, the current UTC hour matches, and it
    /// hasn't run in the last ~20h (so it fires once per day).
    #[must_use]
    pub fn is_due(&self, now_ms: i64) -> bool {
        if !self.enabled {
            return false;
        }
        let hour = (now_ms.div_euclid(3_600_000) % 24) as u8;
        hour == self.hour_utc && (now_ms - self.last_ms) >= 20 * 60 * 60 * 1_000
    }
}

/// Persists the single [`ModelTuneSchedule`].
pub trait ModelTuneScheduleRepository {
    /// Returns the schedule, defaulting to off when unset.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn get(&self) -> Result<ModelTuneSchedule, RepositoryError>;

    /// Stores the schedule.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, schedule: &ModelTuneSchedule) -> Result<(), RepositoryError>;
}

/// Persists the single [`ButlerModelConfig`].
pub trait ButlerModelConfigRepository {
    /// Returns the configured butler models, or `None` if unset (use the
    /// environment configuration).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn get(&self) -> Result<Option<ButlerModelConfig>, RepositoryError>;

    /// Stores the butler model configuration.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, config: &ButlerModelConfig) -> Result<(), RepositoryError>;
}

/// Persists the person's [`AutonomyEnvelope`] (ADR 0022).
pub trait AutonomyEnvelopeRepository {
    /// The stored envelope, or the default if never set.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn get(&self) -> Result<AutonomyEnvelope, RepositoryError>;

    /// Stores the envelope (replacing any previous one).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, envelope: &AutonomyEnvelope) -> Result<(), RepositoryError>;
}

/// Persists per-capability **settings** — the values a skill needs to run (a model
/// name, an API key, a URL), keyed by capability id then setting key (ADR 0021).
/// Secrets live only here and are never echoed back to clients.
pub trait CapabilitySettingsRepository {
    /// All stored settings, as `(capability_id, key, value)` triples.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn all_settings(&self) -> Result<Vec<(String, String, String)>, RepositoryError>;

    /// Sets one setting value for a capability (upsert).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set_setting(
        &self,
        capability_id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), RepositoryError>;
}

/// Persists per-capability configuration the person controls from the Skills view
/// (ADR 0021). Stores the **enabled** flag and, per ADR 0024, whether the person
/// has **opened the irreversible band** for this capability. Only overrides are
/// stored — a capability with no row keeps its built-in defaults (enabled, and the
/// irreversible band closed).
pub trait CapabilityConfigRepository {
    /// The stored enabled/disabled overrides, as `(id, enabled)` pairs. Ids not
    /// present here have never been toggled and use their default.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn enabled_overrides(&self) -> Result<Vec<(String, bool)>, RepositoryError>;

    /// Sets whether a capability is enabled (upsert by id, leaving the
    /// irreversible-opener flag untouched).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), RepositoryError>;

    /// The capabilities whose **irreversible band the person has opened** (ADR
    /// 0024), as `(id, opened)` pairs. Ids not present default to closed — the
    /// irreversible band stays blocked until deliberately opened, per capability.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn opened_overrides(&self) -> Result<Vec<(String, bool)>, RepositoryError>;

    /// Opens or re-closes a capability's irreversible band (upsert by id, leaving
    /// the enabled flag untouched). Opening only ever moves the un-undoable from
    /// *blocked* to *confirm-each-use* — never to autonomous (ADR 0024).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set_open_irreversible(&self, id: &str, opened: bool) -> Result<(), RepositoryError>;
}

/// Persists the **MCP servers** the catalog draws tools from (ADR 0021). The stored
/// rows are plain configuration; adding one is a gated capability (deny-by-default),
/// and every tool a server exposes is still band-classified before it can run — an
/// unknown tool is treated as irreversible and blocked (ADR 0024). Servers are keyed
/// by [`McpServer::name`], which also namespaces their tools (`name.tool`).
pub trait McpServerRegistry {
    /// All registered servers, enabled or not.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn list(&self) -> Result<Vec<McpServer>, RepositoryError>;

    /// Adds a server or replaces the one with the same name (upsert), persisting its
    /// transport and enabled flag.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn register(&self, server: &McpServer) -> Result<(), RepositoryError>;

    /// Switches a server on or off by name, leaving its transport untouched. A no-op
    /// if no server has that name.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), RepositoryError>;

    /// Removes a server by name (idempotent — removing an absent name is fine).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn remove(&self, name: &str) -> Result<(), RepositoryError>;
}

/// A capability the butler asked to use this turn (parsed from its reply). The
/// policy layer decides whether to run it; the model never executes directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityUse {
    /// The capability id, e.g. `"weather"`.
    pub capability: String,
    /// The JSON input for it, as a string.
    pub input_json: String,
}

/// What a capability the butler could use looks like to the application layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySpec {
    /// Stable id, e.g. `"weather"`.
    pub id: String,
    /// One-line description of what it does.
    pub description: String,
    /// Ready to use (vs awaiting setup).
    pub configured: bool,
    /// May it run on its own (read-only/low-stakes), or must the person authorize?
    pub autonomous: bool,
}

/// Runs the butler's skills. The application asks this port to execute a
/// capability the butler proposed, keeping the model out of the execution path
/// (models propose, policy authorizes, capabilities execute — ADRs 0019/0020).
pub trait CapabilityRunner {
    /// The skills currently available (for grounding the butler).
    fn available(&self) -> Vec<CapabilitySpec>;

    /// Runs a capability with JSON input, returning its JSON output or an error
    /// message. Only ever called for capabilities the policy layer has cleared.
    fn run(&self, id: &str, input_json: &str) -> Result<String, String>;

    /// The deterministic policy [`Decision`] for a capability by id (ADRs
    /// 0005/0024) — what policy does with it: [`Act`](Decision::Act) on its own,
    /// [`Confirm`](Decision::Confirm) first, or [`Block`](Decision::Block) outright.
    /// `None` if there is no such skill.
    ///
    /// The default derives a coarse verdict from [`CapabilitySpec::autonomous`]
    /// (act, else confirm); a runner that classifies reversibility bands overrides
    /// this to report [`Block`](Decision::Block) for the un-undoable.
    fn decision(&self, id: &str) -> Option<Decision> {
        self.available().into_iter().find(|s| s.id == id).map(|s| {
            if s.autonomous {
                Decision::Act
            } else {
                Decision::Confirm
            }
        })
    }
}
