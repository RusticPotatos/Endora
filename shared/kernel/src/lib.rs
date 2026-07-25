//! # Endora shared kernel
//!
//! The small set of primitives every bounded context shares: typed identifiers
//! and time ([`ids`]), domain errors ([`error`]), the [`Clock`] and [`IdSource`]
//! ports through which time and identity enter the pure layers, and
//! the shared autonomy vocabulary: [`AutonomyLevel`] (how much standing authority
//! a component has) and [`Reversibility`] with its [`Decision`] (how undoable an
//! action is, and what policy does with it — ADR 0024). These are the vocabulary
//! that two contexts — direction's policy and capabilities' envelope — genuinely
//! share.
//!
//! The kernel depends on nothing. It is `shared/`, not a bounded context: it
//! holds no use cases and no policy, only the vocabulary contexts speak in
//! common. See `docs/adr/0026-package-by-bounded-context.md`.

#![forbid(unsafe_code)]

pub mod autonomy;
pub mod error;
pub mod ids;
pub mod reversibility;
pub mod traits;

pub use autonomy::AutonomyLevel;
pub use error::{AppError, DomainError, RepositoryError};
pub use ids::{AuditId, BeliefId, MessageId, OutcomeId, PreferenceId, Timestamp};
pub use reversibility::{Decision, Reversibility};
pub use traits::{Clock, IdSource};
