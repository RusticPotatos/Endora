//! # Endora shared kernel
//!
//! The small set of primitives every bounded context shares: typed identifiers
//! and time ([`ids`]), domain errors ([`error`]), the [`Clock`] and [`IdSource`]
//! ports through which time and identity enter the pure layers, and
//! [`AutonomyLevel`] — the one vocabulary two contexts (direction's policy and
//! capabilities' envelope) genuinely share.
//!
//! The kernel depends on nothing. It is `shared/`, not a bounded context: it
//! holds no use cases and no policy, only the vocabulary contexts speak in
//! common. See `docs/adr/0026-package-by-bounded-context.md`.

#![forbid(unsafe_code)]

pub mod autonomy;
pub mod error;
pub mod ids;
pub mod traits;

pub use autonomy::AutonomyLevel;
pub use error::{AppError, DomainError, RepositoryError};
pub use ids::{
    AssumptionId, AuditId, BeliefId, DirectionId, ExperimentId, MessageId, ObservationId,
    PreferenceId, ProcessChangeId, ReflectionId, SuggestionId, TargetId, Timestamp, ValueId,
};
pub use traits::{Clock, IdSource};
