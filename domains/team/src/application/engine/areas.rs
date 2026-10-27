//! The leads' old report and patterns turns (ADR 0120): kept as tasks so they stay off the board
//! and the scorecard. The leads, their reports and the CEO's strategy went with the company
//! (ADR 0142).

use std::collections::HashSet;

use super::{Done, Engine};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::TaskId;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The tasks that are a lead's report or patterns turn, not work on the board.
    pub(super) fn report_task_ids(&self) -> Done<HashSet<TaskId>> {
        Ok(self
            .store
            .report_tasks()?
            .into_iter()
            .map(|(t, _, _)| t)
            .collect())
    }
}
