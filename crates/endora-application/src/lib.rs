//! # Endora application layer
//!
//! This crate is the thin **orchestration layer** (ADR 0026): it holds the
//! butler-turn contract and the use cases that compose several bounded contexts,
//! and defines the abstractions (ports) that infrastructure implements. It
//! depends inward on the context crates under `domains/` and the shared kernel,
//! and must not depend on concrete infrastructure, transports, or UI frameworks.
//!
//! It also re-exports each context's domain vocabulary so the adapter layers
//! (infrastructure, the node) have a single import surface above the domain.
//! See `docs/architecture.md`.

#![forbid(unsafe_code)]

pub mod error;
pub mod ports;
pub mod usecases;

pub use error::AppError;
// The application layer re-exports the domain vocabulary of each context (models
// + ids), so the adapter layers (infrastructure, the node) have one place to
// import the types they translate to and from — the orchestration layer is the
// surface above the domain (ADR 0026).
pub use endora_kernel::{
    AuditId, AutonomyLevel, BeliefId, DomainError, MessageId, PreferenceId, Timestamp,
};
// The audit trail and event log live in the platform context now (ADR 0026);
// re-exported so `endora_application::{AuditLog, EventLog, ActivityEvent}` hold.
pub use endora_platform::{ActivityEvent, AuditLog, AuditRecord, EventLog};
// The capabilities ports live in the capabilities context (ADR 0026); re-exported
// so their `endora_application::…` paths are unchanged.
pub use endora_capabilities::{
    AutonomyEnvelope, AutonomyEnvelopeRepository, ButlerModelConfig, ButlerModelConfigRepository,
    CapabilityConfigRepository, CapabilityRunner, CapabilitySettingsRepository, CapabilitySpec,
    CapabilityUse, DeepModel, DeepModelRepository, ModelSlot, ModelTuneSchedule,
    ModelTuneScheduleRepository, Sampling,
};
// The chat repository lives in the conversation context (ADR 0026); re-exported
// so `endora_application::ChatRepository` is unchanged.
pub use endora_conversation::{ChatMessage, ChatRepository, MessageRole};
// The schedules live in the scheduling context (ADR 0026); re-exported so their
// `endora_application::…` paths are unchanged.
pub use endora_scheduling::{
    BriefSchedule, BriefScheduleRepository, CheckinRepository, CheckinSchedule,
};
// Belief/preference repositories live in the understanding context (ADR 0026);
// re-exported so their `endora_application::…` paths are unchanged.
pub use endora_understanding::{
    Belief, BeliefKind, BeliefRepository, BeliefStatus, Confidence, Preference, PreferenceKind,
    PreferenceRepository,
};
pub use ports::{
    Butler, ButlerContext, ButlerReply, CapabilityTool, Clock, ConversationSummary,
    ConversationSummaryStore, DeepAsker, FormedBelief, IdSource, MemorySnapshot, MemoryStore,
    ProposalError, RepositoryError, ToolCall, TurnMessage,
};
pub use usecases::{ActivityItem, ActivityKind};

/// Human-readable identity of this build, suitable for a node/CLI banner.
///
/// Kept in the application layer so every interface (the node, the CLI, and
/// later clients) reports the platform identically.
#[must_use]
pub fn platform_identity() -> String {
    format!(
        "Endora {} — an open platform for continuous growth",
        version()
    )
}

/// The workspace version string for this build.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A short identifier for *this build* — the git short SHA stamped in at image
/// build time (`ENDORA_BUILD`), so a client can tell one deploy from the next
/// even when the version number hasn't changed. `"dev"` for a local run where the
/// stamp isn't set.
#[must_use]
pub fn build_id() -> String {
    std::env::var("ENDORA_BUILD")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "dev".to_owned())
}

/// The default autonomy level a freshly configured component starts at.
///
/// Endora defaults to the most conservative posture: observe only. Any greater
/// autonomy is an explicit, human-granted decision — never an implicit default.
#[must_use]
pub const fn default_autonomy_level() -> AutonomyLevel {
    AutonomyLevel::Observe
}

#[cfg(test)]
mod tests {
    use super::{default_autonomy_level, platform_identity, version};
    use endora_kernel::AutonomyLevel;

    #[test]
    fn identity_names_the_project() {
        assert!(platform_identity().contains("Endora"));
    }

    #[test]
    fn version_is_populated() {
        assert!(!version().is_empty());
    }

    #[test]
    fn default_autonomy_is_the_most_conservative() {
        assert_eq!(default_autonomy_level(), AutonomyLevel::Observe);
    }
}
