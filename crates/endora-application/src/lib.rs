//! # Endora application layer
//!
//! The thin **orchestration layer** (ADR 0050): the use cases the interfaces call that are not
//! one context's own, and the ports infrastructure implements for them. Since the chat was
//! retired (ADR 0136) that is the activity feed, the owner's export and delete, and the usage
//! log the team's relay records into. It depends inward on the context crates and the shared
//! kernel, never on infrastructure, transports or UI.
//!
//! It also re-exports the contexts' vocabulary the adapter layers translate, so the node has a
//! single import surface above the domain. See `docs/architecture.md`.

#![forbid(unsafe_code)]

pub mod error;
pub mod ports;
pub mod usecases;

pub use error::AppError;
// The application layer re-exports the domain vocabulary of each context (models
// + ids), so the adapter layers (infrastructure, the node) have one place to
// import the types they translate to and from (ADR 0050).
pub use endora_kernel::{AuditId, AutonomyLevel, Decision, DomainError, Reversibility, Timestamp};
// The audit trail and event log live in the platform context (ADR 0050).
pub use endora_platform::{ActivityEvent, AuditLog, AuditRecord, EventLog};
// The capabilities ports live in the capabilities context (ADR 0050).
pub use endora_capabilities::{
    CapabilityConfigRepository, CapabilityRead, CapabilityRunner, CapabilitySettingsRepository,
    CapabilitySpec, CapabilityUse,
};
pub use ports::{
    Clock, IdSource, MemorySnapshot, MemoryStore, ModelUsage, RepositoryError, UsageLog,
    UsageTotals,
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
