//! # Endora domain facade (transitional)
//!
//! Under the Responsibility-Oriented reorg (ADR 0026) the domain models moved
//! into their bounded-context crates under `domains/`: the aims + learning loop
//! (values, targets, experiments, reflection, policy) into `endora-direction`,
//! beliefs and preferences into `endora-understanding`, the chat model into
//! `endora-conversation`, the audit record into `endora-platform`, and the
//! autonomy envelope into `endora-capabilities`. Shared primitives (typed ids,
//! `Timestamp`, `DomainError`, `AutonomyLevel`) live in `endora-kernel`.
//!
//! This crate now only **re-exports** those types so existing `endora_domain::…`
//! paths keep working while callers are repointed; it holds no models of its own
//! and is slated for removal. New code should depend on the context crates and
//! the kernel directly. See `docs/architecture.md`.

#![forbid(unsafe_code)]

pub mod autonomy;
pub mod error;
pub mod ids;

// `AuditRecord` moved to the platform context (ADR 0026); re-exported so
// `endora_domain::AuditRecord` paths are unchanged during the migration.
pub use autonomy::AutonomyLevel;
pub use endora_conversation::{ChatMessage, MessageRole};
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
