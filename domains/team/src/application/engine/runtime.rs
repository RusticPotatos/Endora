//! What runs a member's turns (ADR 0117): the owner's setting, refused unless the member may
//! run on it, and what a turn is handed out with; and the pilot, seeded once: one wing
//! developer on Claude Code, measured against the other on the worker by the scorecard.

use serde_json::json;

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::{EventKind, MemberId, Runtime};

/// Set once HQ has put the pilot's developer on Claude Code, so the owner's later choice
/// stands.
const PILOT_SEEDED: &str = "claude_code_pilot_seeded";

/// The wing's developer the pilot runs on Claude Code; the other stays on the worker.
pub const PILOT_DEVELOPER: &str = "Rae";

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
        let runtime =
            Runtime::for_member(found.role, found.brain, runtime).map_err(EngineError::Refused)?;
        self.store.set_runtime(member, runtime)?;
        self.emit(
            EventKind::RuntimeSet,
            json!({ "member": member.0, "name": found.name, "runtime": runtime.name() }),
        )?;
        Ok(runtime)
    }

    /// What runs a member's turn now: the owner's setting while the member may still run on
    /// it, the worker otherwise (its brain gone home, its role changed).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; a store failure.
    pub fn runtime_of(&self, member: MemberId) -> Done<Runtime> {
        let found = self.member(member)?;
        let set = self.store.runtime_of(member)?;
        Ok(set.effective(found.role, found.brain))
    }

    /// Puts the pilot's developer on Claude Code, once: nothing when HQ has done so before
    /// (the owner may have changed it since), or there is no such developer yet. Returns the
    /// member it put there.
    ///
    /// # Errors
    /// A store failure. A developer who may not run on Claude Code (not on Claude) is left on
    /// the worker, and the pilot counts as seeded.
    pub fn seed_claude_code_pilot(&self) -> Done<Option<MemberId>> {
        if self.store.setting(PILOT_SEEDED)?.is_some() {
            return Ok(None);
        }
        let Some(pilot) = self
            .store
            .members()?
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(PILOT_DEVELOPER))
        else {
            return Ok(None);
        };
        let set = match self.set_runtime(pilot.id, Runtime::ClaudeCode.name()) {
            Ok(_) => Some(pilot.id),
            Err(EngineError::Refused(_)) => None,
            Err(e) => return Err(e),
        };
        self.store.set_setting(PILOT_SEEDED, "1")?;
        Ok(set)
    }
}
