//! What runs a member's turns (ADR 0117): the owner's setting, refused unless the member may
//! run on it, and what a turn is handed out with.

use serde_json::json;

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::{EventKind, MemberId, RunId, Runtime};

/// What the owner's change of a member's model did (ADR 0117, amended 2026-10-31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSet {
    /// The model set.
    pub brain: crate::domain::Brain,
    /// What the owner is told beside it, when the member's runtime cannot run the new model and
    /// the member was put back on Endora's worker; `None` when nothing else changed.
    pub said: Option<String>,
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The owner sets what runs a member's turns, by name: `worker`, or `claude-code` for a
    /// developer who thinks with Claude. The next turn starts with it.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist;
    /// [`crate::domain::Refusal::NotARuntime`] for one it may not run on; a store failure.
    pub fn set_runtime(&self, member: MemberId, runtime: &str) -> Done<Runtime> {
        let _held = self.held();
        let found = self.member(member)?;
        let runtime = Runtime::for_member(found.brain, runtime).map_err(EngineError::Refused)?;
        self.store.set_runtime(member, runtime)?;
        self.emit(
            EventKind::RuntimeSet,
            json!({ "member": member.0, "name": found.name, "runtime": runtime.name() }),
        )?;
        Ok(runtime)
    }

    /// After the owner set `member`'s model to `brain`, under the engine's hold: a runtime that
    /// cannot run it is moved to Endora's worker, which runs every model, said in the feed, and
    /// what the owner is told is returned. The owner chooses the model and the runtime; the
    /// worker is every member's default (ADR 0117), so the move takes nothing the owner chose
    /// that could still run. `None` when the runtime stands.
    pub(super) fn runtime_after_model(
        &self,
        member: &crate::domain::Member,
        brain: crate::domain::Brain,
    ) -> Done<Option<String>> {
        let set = self.store.runtime_of(member.id)?;
        if set.allowed_for(brain) {
            return Ok(None);
        }
        self.store.set_runtime(member.id, Runtime::Worker)?;
        self.emit(
            EventKind::RuntimeSet,
            json!({ "member": member.id.0, "name": member.name, "runtime": Runtime::Worker.name(),
                    "was": set.name(), "why": "model" }),
        )?;
        Ok(Some(format!(
            "{} now runs on {}: {} runs only Claude",
            member.name,
            Runtime::Worker.label(),
            set.label()
        )))
    }

    /// What runs a member's turn now: the owner's setting while the member may still run on
    /// it, the worker otherwise (its brain gone home, its role changed).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; a store failure.
    pub fn runtime_of(&self, member: MemberId) -> Done<Runtime> {
        let found = self.member(member)?;
        let set = self.store.runtime_of(member)?;
        Ok(set.effective(found.brain))
    }

    /// What runs one turn (ADR 0131): the member's setting while the brain the turn was started
    /// on may run on it. A turn of work on the house's models runs on the worker, whatever the
    /// member's setting: Claude Code speaks only Anthropic's API.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a turn that does not exist; a store failure.
    pub fn runtime_of_turn(&self, run: RunId) -> Done<Runtime> {
        let found = self.store.run(run)?.ok_or(EngineError::Unknown("run"))?;
        let member = self.member(found.member)?;
        let set = self.store.runtime_of(member.id)?;
        Ok(set.effective(found.brain.unwrap_or(member.brain)))
    }
}
