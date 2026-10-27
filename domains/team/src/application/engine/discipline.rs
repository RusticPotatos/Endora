//! Discipline (ADR 0124): work split no wider or deeper than it can be followed, each part
//! saying which of its parent's criteria it serves; and proof that is already enough ends in a
//! submission, not more proof.
//!
//! Live, scope grew sub-task by sub-task, each defensible on its own, and members kept
//! re-reading what they had already shown. Neither is a model's to judge: the counts, the
//! criteria and the evidence rule are HQ's record.

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::guards::{self, GuardRefusal};
use crate::domain::{DeskId, Refusal, TaskId};

/// Where HQ keeps which criterion of its task an approval on the desk serves, as it was asked
/// (ADR 0124, amended): its number and its words, as JSON.
pub(super) fn approval_serves_key(desk: DeskId) -> String {
    format!("approval_serves:{}", desk.0)
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- sub-task discipline ------------------------------------------------------------------

    /// Which of `task`'s criteria a request for the owner's yes serves (ADR 0124, amended): its
    /// number, counted from 1, and its words as they are now. A request that names none, or on a
    /// task with none, is refused with the reason.
    pub(super) fn approval_serves(
        &self,
        task: TaskId,
        said: Option<&str>,
    ) -> Done<(usize, String)> {
        let criteria = self.task(task)?.criteria;
        let n = guards::approval_serves(task, &criteria, said).map_err(refused)?;
        let words = criteria
            .get(n.saturating_sub(1))
            .map(|c| c.text.clone())
            .unwrap_or_default();
        Ok((n, words))
    }

    // ---- evidence that is already enough ------------------------------------------------------
}

fn refused(r: GuardRefusal) -> EngineError {
    EngineError::Refused(Refusal::Guard(r))
}
