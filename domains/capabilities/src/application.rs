//! Capabilities application layer — ports for running and configuring skills.

use endora_kernel::RepositoryError;

pub use crate::domain::AutonomyEnvelope;

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
/// (ADR 0021). This first slice stores only the **enabled** flag; only overrides
/// are stored — a capability with no row keeps its built-in default (enabled).
pub trait CapabilityConfigRepository {
    /// The stored enabled/disabled overrides, as `(id, enabled)` pairs. Ids not
    /// present here have never been toggled and use their default.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn enabled_overrides(&self) -> Result<Vec<(String, bool)>, RepositoryError>;

    /// Sets whether a capability is enabled (upsert by id).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), RepositoryError>;
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
}
