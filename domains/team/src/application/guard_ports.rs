//! What the engine needs from the store for its guards (ADR 0124): the receipts of recovery
//! wakes, kept before each is sent, and the guards' findings with their verdicts. A nudge's
//! receipt is never changed or removed; a finding only gains its clearing and its verdict.

use super::ports::Stored;
use crate::domain::closure::Closed;
use crate::domain::guards::{Finding, Verdict};
use crate::domain::retraction::Retraction;
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

    /// Keeps a retraction, returning its id. A retraction is never changed or removed.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn add_retraction(&self, retraction: &Retraction) -> Stored<i64>;

    /// Every retraction, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn retractions(&self) -> Stored<Vec<Retraction>>;

    /// Records that note `note` was built on note `parent` (it cites it).
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn add_note_parent(&self, note: i64, parent: i64) -> Stored<()>;

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

    /// The notes built on `parent`, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn note_children(&self, parent: i64) -> Stored<Vec<i64>>;
}
