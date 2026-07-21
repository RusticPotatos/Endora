//! # Direction context
//!
//! The person's aims and the learning loop that refines how the butler pursues
//! them: **values**, **North Stars/Targets/Assumptions**, **experiments** and
//! their observations, **reflections** that turn evidence into proposed process
//! changes, and the deterministic **policy** boundary that authorizes those
//! changes (models propose; policy authorizes — ADR 0005).
//!
//! Layered inward: [`domain`] (pure model + policy) and [`application`] (the
//! repository ports). Cross-domain orchestration that applies a butler proposal
//! lives in the orchestration layer. Depends only on the shared kernel — a leaf
//! context. See ADR 0026.

#![forbid(unsafe_code)]

pub mod application;
pub mod domain;

pub use application::{
    AssumptionRepository, DirectionRepository, ExperimentRepository, ObservationRepository,
    ProcessChangeRepository, ReflectionRepository, TargetRepository, ValueRepository,
};
pub use domain::{
    ApprovalState, Assumption, Direction, Experiment, ExperimentStatus, LifecycleStatus,
    Observation, PolicyDecision, ProposedProcessChange, Reflection, Target, Value,
    authorize_process_change,
};
