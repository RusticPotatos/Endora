//! What the engine needs from the store for its guards (ADR 0124): the receipts of recovery
//! wakes, kept before each is sent, and the guards' findings with their verdicts. A nudge's
//! receipt is never changed or removed; a finding only gains its clearing and its verdict.

use super::ports::Stored;
use crate::domain::closure::Closed;
use crate::domain::{MemberId, Millis, TaskId};

/// The guards' part of HQ's store.
pub trait GuardStore {
    /// Records a recovery wake under its key before it is sent. Returns `false`, recording
    /// nothing, when that key was recorded before: the same wake is never sent twice.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn record_nudge(
        &self,
        key: &str,
        task: TaskId,
        member: MemberId,
        reason: &str,
        at: Millis,
    ) -> Stored<bool>;

    /// Records why a task closed, and what carries it on (its latest closing, if it closed
    /// before).
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn record_closure(&self, closed: &Closed) -> Stored<()>;

    /// Why a task closed, if it did and HQ was told.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn closure(&self, task: TaskId) -> Stored<Option<Closed>>;

    /// Attaches the same ask, from `task` (asked by `member`), to the desk item `desk`.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn attach_to_desk(&self, desk: i64, task: TaskId, member: Option<MemberId>) -> Stored<()>;

    /// The tasks (and who asked on each) attached to a desk item, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn attached(&self, desk: i64) -> Stored<Vec<(TaskId, Option<MemberId>)>>;
}
