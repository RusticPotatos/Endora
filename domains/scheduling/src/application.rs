//! Scheduling application layer — repository ports for the two schedules.

use endora_kernel::RepositoryError;

use crate::domain::BriefSchedule;

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
