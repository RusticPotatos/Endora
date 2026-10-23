//! Thin canonical consultation adapter and refusal of retired expert turns (ADR 0138).

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::{Refusal, RunId, TaskId, WakeReason};

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Familiar application entry point; it owns no separate consultation lifecycle.
    /// # Errors
    /// A retired task-routing argument or canonical assistance policy refusal.
    pub fn consult_in(
        &self,
        run: RunId,
        expert: &str,
        question: &str,
        task: Option<TaskId>,
    ) -> Done<String> {
        if task.is_some() {
            return Err(EngineError::Refused(Refusal::Conversation(
                "task-thread consultation is retired; use authorized assistance",
            )));
        }
        self.consult_peer_in(run, expert, question, None, None, &[])
    }

    /// Recognize an obsolete taskless expert turn without claiming any old request.
    /// Canonical expert and requester-result associations take precedence.
    /// # Errors
    /// A store failure or unknown run.
    pub fn in_consult(&self, run: RunId) -> Done<bool> {
        let Some(turn) = self.store.run(run)? else {
            return Ok(true);
        };
        Ok(turn.task.is_none()
            && self.reason_for(&turn)? == WakeReason::Consulted.name()
            && self.store.assistance_for_run(run)?.is_none()
            && self.store.assistance_notice_for_run(run)?.is_none())
    }

    /// Retired answer API: it can never post an answer or schedule work.
    /// # Errors
    /// Always refuses; assigned canonical experts use `answer_assistance`.
    pub fn answer_consult_in(&self, _run: RunId, _answer: &str) -> Done<()> {
        Err(EngineError::Refused(Refusal::Conversation(
            "answer_consult is retired; use assigned answer_assistance",
        )))
    }

    /// Refuse any surviving obsolete expert token, rather than offering a fallback.
    /// # Errors
    /// A store failure or unknown run.
    pub fn consulted_turn_refuses(&self, run: RunId, _tool: &str) -> Done<Option<String>> {
        Ok(self
            .in_consult(run)?
            .then(|| "legacy expert turn retired; no tools are authorized".into()))
    }
}
