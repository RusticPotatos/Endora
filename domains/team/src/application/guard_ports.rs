//! What the engine needs from the store for its guards (ADR 0124): the receipts of recovery
//! wakes, kept before each is sent, and the guards' findings with their verdicts. A nudge's
//! receipt is never changed or removed; a finding only gains its clearing and its verdict.

use super::ports::Stored;
use crate::domain::closure::Closed;
use crate::domain::guards::{Finding, Verdict};
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

    /// Every finding, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn findings(&self) -> Stored<Vec<Finding>>;

    /// Keeps a new finding, returning its id; `None`, keeping nothing, while a finding with
    /// the same key is still open.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn open_finding(&self, finding: &Finding) -> Stored<Option<i64>>;

    /// Says a finding's condition cleared at `at`.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn clear_finding(&self, id: i64, at: Millis) -> Stored<()>;

    /// Records the verdict on a finding, and whether its fix worked if that was said.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn judge_finding(
        &self,
        id: i64,
        verdict: Verdict,
        fix_worked: Option<bool>,
        by: &str,
        at: Millis,
    ) -> Stored<()>;

    /// Records the desk notice a finding put up.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn set_finding_desk(&self, id: i64, desk: i64) -> Stored<()>;

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
}
