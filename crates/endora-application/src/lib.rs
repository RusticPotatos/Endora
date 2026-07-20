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
pub use ports::{
    AssumptionRepository, AttentionItem, AttentionKind, AuditLog, Butler, ButlerContext,
    ButlerProposal, ButlerReply, ChatRepository, CheckinRepository, CheckinSchedule, Clock,
    DirectionRepository, ExperimentRepository, IdSource, MemorySnapshot, MemoryStore,
    NorthStarBrief, ObservationRepository, PreferenceRepository, ProcessChangeRepository,
    ProposalError, Proposer, ReflectionRepository, RepositoryError, Snooze, SnoozeRepository,
    Suggestion, SuggestionRepository, SuggestionStatus, TargetRepository, ValueRepository,
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
