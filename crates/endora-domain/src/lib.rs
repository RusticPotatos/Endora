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

pub mod audit;
pub mod autonomy;
pub mod conversation;
pub mod error;
pub mod experiments;
pub mod ids;
pub mod policy;
pub mod preferences;
pub mod reflection;
pub mod targets;
pub mod values;

pub use audit::AuditRecord;
pub use autonomy::AutonomyLevel;
pub use conversation::{ChatMessage, MessageRole};
pub use error::DomainError;
pub use experiments::{Experiment, ExperimentStatus, Observation};
pub use ids::{
    AssumptionId, AuditId, DirectionId, ExperimentId, MessageId, ObservationId, PreferenceId,
    ProcessChangeId, ReflectionId, TargetId, Timestamp, ValueId,
};
pub use policy::{PolicyDecision, authorize_process_change};
pub use preferences::{Preference, PreferenceKind};
pub use reflection::{ApprovalState, ProposedProcessChange, Reflection};
pub use targets::{Assumption, Direction, LifecycleStatus, Target};
pub use values::Value;
