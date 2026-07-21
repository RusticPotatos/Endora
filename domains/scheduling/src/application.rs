//! Scheduling application layer — repository ports for the two schedules.

use endora_kernel::RepositoryError;

use crate::domain::{BriefSchedule, CheckinSchedule};

/// Persists the single [`BriefSchedule`].
pub trait BriefScheduleRepository {
    /// Returns the stored schedule, or `None` if never set.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn get(&self) -> Result<Option<BriefSchedule>, RepositoryError>;

    /// Stores the schedule (replacing any previous one).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, schedule: &BriefSchedule) -> Result<(), RepositoryError>;
}

/// Persists the single [`CheckinSchedule`].
pub trait CheckinRepository {
    /// Returns the stored schedule, or `None` if never set.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self) -> Result<Option<CheckinSchedule>, RepositoryError>;

    /// Stores the schedule (replacing any previous one).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, schedule: &CheckinSchedule) -> Result<(), RepositoryError>;
}
