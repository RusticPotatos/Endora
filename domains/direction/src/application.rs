//! Direction application layer — repository ports for the aims + learning loop.

use endora_kernel::RepositoryError;
use endora_kernel::ids::{
    AssumptionId, DirectionId, ExperimentId, ProcessChangeId, ReflectionId, TargetId, Timestamp,
    ValueId,
};

use crate::domain::{
    Assumption, Direction, Experiment, Observation, ProposedProcessChange, Reflection, Target,
    Value,
};

/// Persists and retrieves [`Value`]s (the Identity & Values context).
pub trait ValueRepository {
    /// Inserts a value, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, value: &Value) -> Result<(), RepositoryError>;

    /// Fetches a value by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: ValueId) -> Result<Option<Value>, RepositoryError>;

    /// Lists all values, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_all(&self) -> Result<Vec<Value>, RepositoryError>;

    /// Permanently removes a value. Callers are responsible for ensuring no
    /// North Star still references it first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn delete(&self, id: ValueId) -> Result<(), RepositoryError>;
}

/// Persists and retrieves [`Direction`]s (North Stars).
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

    /// Lists all directions, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_all(&self) -> Result<Vec<Direction>, RepositoryError>;

    /// Permanently removes a direction. Callers are responsible for ensuring it
    /// has no dependents first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn delete(&self, id: DirectionId) -> Result<(), RepositoryError>;
}

/// Persists and retrieves [`Target`]s.
pub trait TargetRepository {
    /// Inserts a target, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, target: &Target) -> Result<(), RepositoryError>;

    /// Fetches a target by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: TargetId) -> Result<Option<Target>, RepositoryError>;

    /// Lists the targets belonging to a direction, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_direction(&self, direction: DirectionId) -> Result<Vec<Target>, RepositoryError>;

    /// Permanently removes a target. Callers are responsible for ensuring it has
    /// no dependents first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn delete(&self, id: TargetId) -> Result<(), RepositoryError>;
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

    /// Lists the assumptions belonging to a target, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_target(&self, target: TargetId) -> Result<Vec<Assumption>, RepositoryError>;
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

    /// Lists experiments whose scheduled review is due as of `now` — a review
    /// was scheduled for at or before `now` and the experiment is not concluded.
    /// Ordered by review time, soonest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_due_reviews(&self, now: Timestamp) -> Result<Vec<Experiment>, RepositoryError>;
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

    /// Lists the most recently recorded observations across all experiments,
    /// newest first, up to `limit`. Used to build the activity feed.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn recent(&self, limit: usize) -> Result<Vec<Observation>, RepositoryError>;
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

    /// Lists the reflections for a target, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_target(&self, target: TargetId) -> Result<Vec<Reflection>, RepositoryError>;
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
