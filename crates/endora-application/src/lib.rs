//! # Endora application layer
//!
//! This crate holds the **Application** layer. It orchestrates domain concepts
//! into use cases and defines the abstractions (ports) that infrastructure
//! implements. It depends inward on [`endora_domain`] and must not depend on
//! concrete infrastructure, transports, or UI frameworks.
//!
//! It fixes the dependency direction (`Interface -> Application -> Domain`) and
//! defines the [`ports`] that infrastructure implements. See
//! `docs/architecture.md`.

#![forbid(unsafe_code)]

pub mod error;
pub mod ports;
pub mod usecases;

pub use error::AppError;
// The audit trail and event log live in the platform context now (ADR 0026);
// re-exported so `endora_application::{AuditLog, EventLog, ActivityEvent}` hold.
pub use endora_platform::{ActivityEvent, AuditLog, EventLog};
// The capabilities ports live in the capabilities context (ADR 0026); re-exported
// so their `endora_application::…` paths are unchanged.
pub use endora_capabilities::{
    AutonomyEnvelope, AutonomyEnvelopeRepository, CapabilityConfigRepository, CapabilityRunner,
    CapabilitySettingsRepository, CapabilitySpec, CapabilityUse, DeepModel, DeepModelRepository,
};
// The chat repository lives in the conversation context (ADR 0026); re-exported
// so `endora_application::ChatRepository` is unchanged.
pub use endora_conversation::ChatRepository;
// Belief/preference repositories live in the understanding context (ADR 0026);
// re-exported so their `endora_application::…` paths are unchanged.
pub use endora_understanding::{BeliefRepository, PreferenceRepository};
// The aims + learning-loop repositories live in the direction context (ADR 0026);
// re-exported so their `endora_application::…` paths are unchanged.
pub use endora_direction::{
    AssumptionRepository, DirectionRepository, ExperimentRepository, ObservationRepository,
    ProcessChangeRepository, ReflectionRepository, TargetRepository, ValueRepository,
};
pub use ports::{
    AttentionItem, AttentionKind, BriefSchedule, BriefScheduleRepository, Butler, ButlerContext,
    ButlerProposal, ButlerReply, CheckinRepository, CheckinSchedule, Clock, FormedBelief, IdSource,
    MemorySnapshot, MemoryStore, NorthStarBrief, ProposalError, Proposer, RepositoryError, Snooze,
    SnoozeRepository, Suggestion, SuggestionRepository, SuggestionStatus,
};
pub use usecases::{ActivityItem, ActivityKind};

use endora_domain::AutonomyLevel;

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
    use endora_domain::AutonomyLevel;

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
