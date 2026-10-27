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

    /// The newest finding with this key, open or cleared, judged or not (ADR 0132).
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn latest_finding(&self, key: &str) -> Stored<Option<Finding>>;

    /// Says an open finding was seen again at `seen_at`, with what it fired on now. Its mark
    /// rises to `mark` if one is given and it is higher, and is left alone otherwise: what a
    /// judged finding rests on is what was judged (ADR 0132).
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn refresh_finding(
        &self,
        id: i64,
        evidence: &str,
        seen_at: Millis,
        mark: Option<i64>,
    ) -> Stored<()>;

    /// Opens a cleared finding again, as itself (ADR 0132): its episode begins at `at`, it counts
    /// one more, and its mark rises to `mark` if that is higher. Returns whether it was cleared
    /// to open.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn reopen_finding(&self, id: i64, evidence: &str, at: Millis, mark: i64) -> Stored<bool>;

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
