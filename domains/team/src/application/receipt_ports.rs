//! What the engine needs from the store for what turns did and cited: call receipts, scenario
//! actions and the evidence cited on tasks (ADR 0114).

use super::ports::{Cited, ScenarioAction, Stored, ThreadPost};
use crate::domain::{CallReceipt, RunId, TaskId};

/// The receipts' and citations' part of HQ's store.
pub trait ReceiptStore {
    /// Records a receipt the node wrote (its `id` is ignored) and returns its id.
    fn add_call_receipt(&self, receipt: &CallReceipt) -> Stored<i64>;
    /// A receipt, by its id.
    fn call_receipt(&self, id: i64) -> Stored<Option<CallReceipt>>;
    /// A turn's receipts, oldest first.
    fn call_receipts_of(&self, run: RunId) -> Stored<Vec<CallReceipt>>;
    /// Adds an action to a scenario's log (its `id` is ignored) and returns its id.
    fn add_scenario_action(&self, action: &ScenarioAction) -> Stored<i64>;
    /// A scenario's log, oldest first.
    fn scenario_actions(&self, task: TaskId) -> Stored<Vec<ScenarioAction>>;
    /// Records evidence cited for a criterion.
    fn add_cited(&self, cited: &Cited) -> Stored<()>;
    /// The evidence cited on a task, oldest first.
    fn cited(&self, task: TaskId) -> Stored<Vec<Cited>>;

    /// Every post in a task's thread with the evidence it cited, oldest first: what Replay
    /// reads back.
    fn thread_cited(&self, task: TaskId) -> Stored<Vec<ThreadPost>>;
}
