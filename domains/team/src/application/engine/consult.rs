//! Refusal of retired expert turns (ADR 0138); `consult` itself is canonical assistance.

use super::{Done, Engine};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::{RunId, WakeReason};

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Recognize an obsolete taskless expert turn without claiming any old request.
    /// Canonical expert and requester-result associations take precedence.
    /// # Errors
    /// A store failure or unknown run.
    pub(crate) fn retired_peer_turn(&self, run: RunId) -> Done<bool> {
        let Some(turn) = self.store.run(run)? else {
            return Ok(true);
        };
        Ok(turn.task.is_none()
            && self.reason_for(&turn)? == WakeReason::Consulted.name()
            && self.store.assistance_for_run(run)?.is_none()
            && self.store.assistance_notice_for_run(run)?.is_none())
    }
}
