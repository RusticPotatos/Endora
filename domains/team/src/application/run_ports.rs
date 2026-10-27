//! What the engine needs from the store for turns and the money they spend: each turn, its
//! token, and the ledger it reserves, uses and settles against.

use super::ports::{LedgerRow, Run, Stored};
use crate::domain::money::Micros;
use crate::domain::{MemberId, Millis, ProjectId, RunId, TaskId, WakeId};

/// The turns' and money's part of HQ's store.
pub trait RunStore {
    /// Queues a turn for a wake.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_run(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        wake: WakeId,
        at: Millis,
    ) -> Stored<RunId>;
    /// A turn.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn run(&self, id: RunId) -> Stored<Option<Run>>;
    /// Every turn, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn runs(&self) -> Stored<Vec<Run>>;
    /// The turns queued, running or waiting to retry, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn live_runs(&self) -> Stored<Vec<Run>>;
    /// A task's turns, oldest first.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn runs_of(&self, task: TaskId) -> Stored<Vec<Run>>;
    /// Saves a turn's changes.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn save_run(&self, run: &Run) -> Stored<()>;

    /// Adds a model call's tokens to a turn's: input (fresh, read from the cache, written to
    /// it) and output. Only this adds to them; [`Self::save_run`] leaves them as they are.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn add_tokens(
        &self,
        run: RunId,
        fresh: i64,
        read: i64,
        written: i64,
        output: i64,
    ) -> Stored<()>;

    /// Issues a fresh token for a turn, replacing any it had: what its worker presents to the
    /// team protocol, good for this run only (spec section 3, invariant 3).
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn issue_token(&self, run: RunId) -> Stored<String>;

    /// The run a token was issued for, if any.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn run_of_token(&self, token: &str) -> Stored<Option<RunId>>;

    /// Reserves `amount` for a turn of `member` on `day`, before it starts.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn reserve(
        &self,
        run: RunId,
        member: MemberId,
        day: i64,
        amount: Micros,
        at: Millis,
    ) -> Stored<()>;
    /// Records use reported while a turn runs.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn record_use(
        &self,
        run: RunId,
        member: MemberId,
        day: i64,
        amount: Micros,
        at: Millis,
    ) -> Stored<()>;
    /// Settles a turn: charges what it spent and releases what it reserved.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn settle(&self, run: RunId, member: MemberId, day: i64, at: Millis) -> Stored<()>;
    /// What `member` has committed on `day`: charged, plus reserved and not yet released.
    /// `None` for the whole pool.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn committed(&self, member: Option<MemberId>, day: i64) -> Stored<Micros>;
    /// What `member` has committed on `day` against its own share: as [`Self::committed`],
    /// less its turns the owner asked for, which are the pool's alone (ADR 0113, amended).
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn share_committed(&self, member: MemberId, day: i64) -> Stored<Micros>;
    /// What `member` has really spent on `day`: the use its turns reported, not what they
    /// hold. `None` for the whole pool.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn spent(&self, member: Option<MemberId>, day: i64) -> Stored<Micros>;
    /// What turns on a project's tasks committed from `from` until `until` (charged, plus
    /// reserved and not yet released), by the moments the ledger's rows were written. A
    /// ledger row is a project's when its turn's task was in the project as it was written.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn project_committed(&self, project: ProjectId, from: Millis, until: Millis) -> Stored<Micros>;
    /// What turns on a project's tasks really spent from `from` until `until`.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn project_spent(&self, project: ProjectId, from: Millis, until: Millis) -> Stored<Micros>;
    /// A turn's rows of the ledger, oldest first: what Replay says it held, used, was charged
    /// and released.
    ///
    /// # Errors
    /// [`StoreError`] if the store fails.
    fn ledger_of(&self, run: RunId) -> Stored<Vec<LedgerRow>>;
}
