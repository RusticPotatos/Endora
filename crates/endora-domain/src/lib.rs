//! # Endora domain layer
//!
//! This crate holds the **Domain** layer of the Endora modular monolith. It
//! contains only pure concepts and rules of the platform. By construction it
//! has no dependencies and must never depend on:
//!
//! - HTTP, transports, or serialization formats
//! - databases or storage engines
//! - AI vendors or model-specific concepts
//! - user-interface frameworks
//! - operating-system integrations (including the system clock)
//!
//! Higher layers (Application, Infrastructure, Interface) depend inward on this
//! crate; this crate depends on nothing. See `docs/architecture.md`.
//!
//! ## What lives here
//!
//! - [`autonomy`] — how much authority a component has before a human is
//!   involved ([`AutonomyLevel`]).
//! - [`targets`] — the Direction & Targets context: [`Direction`], [`Target`],
//!   [`Assumption`].
//! - [`experiments`] — the Experiments & Learning context: [`Experiment`],
//!   [`ExperimentStatus`], [`Observation`].
//! - [`reflection`] — the Reflection context: [`Reflection`],
//!   [`ProposedProcessChange`], [`ApprovalState`].
//! - [`policy`] — the Policy & Consent boundary: deterministic authorization
//!   ([`PolicyDecision`], [`authorize_process_change`]).
//! - [`ids`] — typed identifiers and [`Timestamp`], both supplied by callers.
//! - [`error`] — [`DomainError`].
//!
//! Together these model the first vertical slice — the learning loop for a
//! single target (see `docs/adr/0006-first-vertical-slice.md`). Identifiers and
//! time are always supplied by outer layers, so the domain is fully
//! deterministic and testable.

#![forbid(unsafe_code)]

pub mod autonomy;
pub mod conversation;
pub mod error;
pub mod ids;

// `AuditRecord` moved to the platform context (ADR 0026); re-exported so
// `endora_domain::AuditRecord` paths are unchanged during the migration.
pub use autonomy::AutonomyLevel;
pub use conversation::{ChatMessage, MessageRole};
pub use endora_platform::AuditRecord;
// Beliefs and preferences moved to the understanding context (ADR 0026);
// the aims + learning loop (values, targets, experiments, reflection, policy)
// moved to the direction context. Both re-exported so existing `endora_domain::…`
// paths are unchanged.
pub use endora_direction::{
    ApprovalState, Assumption, Direction, Experiment, ExperimentStatus, LifecycleStatus,
    Observation, PolicyDecision, ProposedProcessChange, Reflection, Target, Value,
    authorize_process_change,
};
pub use endora_understanding::{
    Belief, BeliefKind, BeliefStatus, Confidence, Preference, PreferenceKind,
};
pub use error::DomainError;
pub use ids::{
    AssumptionId, AuditId, BeliefId, DirectionId, ExperimentId, MessageId, ObservationId,
    PreferenceId, ProcessChangeId, ReflectionId, SuggestionId, TargetId, Timestamp, ValueId,
};
