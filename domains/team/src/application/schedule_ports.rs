//! What the engine needs from the store for what comes back on time: routines and the
//! check-backs members set.

use super::ports::{NewRoutine, Stored};
use crate::domain::{CheckBack, CheckBackStatus, MemberId, Millis, Routine, RoutineId, TaskId};

/// The routines' and check-backs' part of HQ's store.
pub trait ScheduleStore {
    /// Adds a routine, on, first due at `next_due_at`.
    fn add_routine(&self, routine: &NewRoutine, next_due_at: Millis) -> Stored<RoutineId>;
    /// Every routine, oldest first.
    fn routines(&self) -> Stored<Vec<Routine>>;
    /// Saves a routine's changes: on or off, when it is next due, when it last fired, its
    /// last task, and its project.
    fn save_routine(&self, routine: &Routine) -> Stored<()>;

    /// Adds a pending check-back.
    fn add_check_back(
        &self,
        member: MemberId,
        task: TaskId,
        why: &str,
        due_at: Millis,
        at: Millis,
    ) -> Stored<i64>;
    /// Every check-back, oldest first.
    fn check_backs(&self) -> Stored<Vec<CheckBack>>;
    /// Records what became of a check-back, and when.
    fn set_check_back(&self, id: i64, status: CheckBackStatus, at: Millis) -> Stored<()>;
}
