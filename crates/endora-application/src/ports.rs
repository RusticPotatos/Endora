//! Ports the application defines and the infrastructure layer implements.
//!
//! These are the abstractions the application depends on for persistence. The
//! application never names a concrete storage engine; infrastructure provides
//! adapters (e.g. SQLite — see `docs/adr/0004-sqlite-first.md`). This keeps the
//! dependency direction pointing inward: `Infrastructure -> Application/Domain`.

use core::fmt;

use endora_domain::{
    Assumption, AssumptionId, AuditRecord, Direction, DirectionId, Experiment, ExperimentId, Goal,
    GoalId, Observation, ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId,
    Timestamp,
};

/// A failure from a storage backend behind a repository port.
///
/// Deliberately free of any engine-specific type: adapters translate their own
/// errors (driver, I/O, corrupt rows) into these variants so the application
/// stays independent of the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryError {
    /// The backend itself failed (connection, I/O, driver error, lock).
    Backend(String),
    /// Stored data could not be reconstituted into a valid domain value.
    Corrupt(String),
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(msg) => write!(f, "storage backend error: {msg}"),
            Self::Corrupt(msg) => write!(f, "corrupt stored data: {msg}"),
        }
    }
}

impl core::error::Error for RepositoryError {}

/// Persists and retrieves [`Direction`]s.
pub trait DirectionRepository {
    /// Inserts a direction, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, direction: &Direction) -> Result<(), RepositoryError>;

    /// Fetches a direction by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: DirectionId) -> Result<Option<Direction>, RepositoryError>;
}

/// Persists and retrieves [`Goal`]s.
pub trait GoalRepository {
    /// Inserts a goal, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, goal: &Goal) -> Result<(), RepositoryError>;

    /// Fetches a goal by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: GoalId) -> Result<Option<Goal>, RepositoryError>;

    /// Lists the goals belonging to a direction, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_direction(&self, direction: DirectionId) -> Result<Vec<Goal>, RepositoryError>;
}

/// Persists and retrieves [`Assumption`]s.
pub trait AssumptionRepository {
    /// Inserts an assumption, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, assumption: &Assumption) -> Result<(), RepositoryError>;

    /// Fetches an assumption by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: AssumptionId) -> Result<Option<Assumption>, RepositoryError>;

    /// Lists the assumptions belonging to a goal, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_goal(&self, goal: GoalId) -> Result<Vec<Assumption>, RepositoryError>;
}

/// Persists and retrieves [`Experiment`]s.
pub trait ExperimentRepository {
    /// Inserts an experiment, or replaces the existing one with the same id
    /// (used both to create and to persist lifecycle transitions).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, experiment: &Experiment) -> Result<(), RepositoryError>;

    /// Fetches an experiment by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: ExperimentId) -> Result<Option<Experiment>, RepositoryError>;

    /// Lists the experiments testing an assumption, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_assumption(
        &self,
        assumption: AssumptionId,
    ) -> Result<Vec<Experiment>, RepositoryError>;
}

/// Persists and retrieves [`Observation`]s.
pub trait ObservationRepository {
    /// Inserts an observation, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, observation: &Observation) -> Result<(), RepositoryError>;

    /// Lists the observations recorded for an experiment, oldest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_experiment(
        &self,
        experiment: ExperimentId,
    ) -> Result<Vec<Observation>, RepositoryError>;
}

/// A failure from a reasoning model behind the [`Proposer`] port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalError {
    /// The model could not be reached, or its response could not be understood.
    Unavailable(String),
    /// The model returned an empty proposal.
    Empty,
}

impl fmt::Display for ProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(msg) => write!(f, "model unavailable: {msg}"),
            Self::Empty => write!(f, "model returned an empty proposal"),
        }
    }
}

impl core::error::Error for ProposalError {}

/// Produces proposals from a reasoning model (see
/// `docs/adr/0008-local-model-adapter.md`).
///
/// A proposer is a reasoning component, not an authority: whatever it returns is
/// only *input* to the deterministic policy boundary. The domain never depends
/// on it; infrastructure implements it (e.g. a local, OpenAI-compatible model).
pub trait Proposer {
    /// Proposes a one-line process-change description for a reflection, given
    /// its summary and how many observations it cites.
    ///
    /// # Errors
    /// [`ProposalError`] if the model is unreachable or returns nothing usable.
    fn propose_process_change(
        &self,
        reflection_summary: &str,
        evidence_count: usize,
    ) -> Result<String, ProposalError>;
}

/// Supplies the current time to use cases.
///
/// The domain never reads the clock, so time enters through this port. The node
/// wires a real system clock; tests wire a fixed one.
pub trait Clock {
    /// The current instant, as a domain [`Timestamp`].
    fn now(&self) -> Timestamp;
}

/// Supplies fresh, unique identifier values to use cases.
///
/// The domain never generates identifiers, so they enter through this port. Use
/// cases wrap the raw value in the appropriate typed id. The node wires a random
/// source; tests wire a deterministic one.
pub trait IdSource {
    /// Returns a fresh identifier value, unique within this store's lifetime.
    fn new_id(&self) -> u128;
}

/// Persists and retrieves [`Reflection`]s (including their evidence).
pub trait ReflectionRepository {
    /// Inserts a reflection and its evidence, or replaces the one with the same
    /// id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, reflection: &Reflection) -> Result<(), RepositoryError>;

    /// Fetches a reflection by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: ReflectionId) -> Result<Option<Reflection>, RepositoryError>;

    /// Lists the reflections for a goal, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_goal(&self, goal: GoalId) -> Result<Vec<Reflection>, RepositoryError>;
}

/// Persists and retrieves [`ProposedProcessChange`]s.
pub trait ProcessChangeRepository {
    /// Inserts a proposed change, or replaces the one with the same id (used to
    /// create and to persist approval/rejection).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, change: &ProposedProcessChange) -> Result<(), RepositoryError>;

    /// Fetches a proposed change by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: ProcessChangeId) -> Result<Option<ProposedProcessChange>, RepositoryError>;

    /// Lists the proposed changes from a reflection, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_reflection(
        &self,
        reflection: ReflectionId,
    ) -> Result<Vec<ProposedProcessChange>, RepositoryError>;
}

/// Appends to and reads the audit trail.
///
/// The audit log is append-only from the application's point of view: records
/// are added, never mutated. It is subject to the same memory rights as other
/// stored data (visible, exportable, deletable).
pub trait AuditLog {
    /// Appends a record to the trail.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn append(&self, record: &AuditRecord) -> Result<(), RepositoryError>;

    /// Returns the most recent records, newest first, up to `limit`.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn recent(&self, limit: usize) -> Result<Vec<AuditRecord>, RepositoryError>;
}
