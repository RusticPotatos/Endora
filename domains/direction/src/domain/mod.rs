//! Direction domain model — the person's aims and the learning loop that refines
//! how the butler pursues them.
//!
//! [`values`] (what matters), [`targets`] (North Stars, Targets, Assumptions),
//! [`experiments`] (bounded tests + Observations), [`reflection`] (learning from
//! evidence into proposed process changes), and [`policy`] — the deterministic
//! authorization boundary (models propose; policy authorizes — ADR 0005).

pub mod experiments;
pub mod policy;
pub mod reflection;
pub mod targets;
pub mod values;

pub use experiments::{Experiment, ExperimentStatus, Observation};
pub use policy::{PolicyDecision, authorize_process_change};
pub use reflection::{ApprovalState, ProposedProcessChange, Reflection};
pub use targets::{Assumption, Direction, LifecycleStatus, Target};
pub use values::Value;
