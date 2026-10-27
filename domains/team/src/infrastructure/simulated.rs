//! The simulated runner (spec section 13): scripted turns, no model, no cost. It drives the
//! engine's tests and HQ's labelled demo mode, so the office is built and tested before any
//! real worker exists.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use crate::application::ports::{RunContext, StartError, WorkerAdapter};
use crate::domain::{Exit, Liveness, MemberId, RunId};

/// How a member's scripted turn goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// It stays running until the test ends it.
    StaysRunning,
    /// It ends at once, well.
    Succeeds,
    /// It ends at once, in failure.
    Fails,
    /// Its container disappears without a word.
    Vanishes,
    /// It is killed from outside, as Docker does to a container it stops (SIGKILL, exit 137),
    /// and the runner says so.
    Killed,
    /// It cannot even start.
    WillNotStart,
}

#[derive(Default)]
struct State {
    scripts: HashMap<MemberId, Script>,
    turns: HashMap<RunId, Liveness>,
    exits: HashMap<RunId, Exit>,
    started: Vec<RunContext>,
    stopped: Vec<RunId>,
}

/// A runner whose turns follow a script per member.
pub struct SimulatedRunner {
    capacity: usize,
    state: Mutex<State>,
}

impl SimulatedRunner {
    /// A runner that can take `capacity` turns at once; every member's turn stays running
    /// until told otherwise.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Sets how `member`'s turns go from now on.
    pub fn script(&self, member: MemberId, script: Script) {
        self.state().scripts.insert(member, script);
    }

    /// Ends a running turn, well or not.
    pub fn finish(&self, run: RunId, succeeded: bool) {
        self.state()
            .turns
            .insert(run, Liveness::Finished { succeeded });
    }

    /// Ends a running turn badly, saying how its worker's container ended, as a runner does.
    pub fn finish_badly(&self, run: RunId, exit: Exit) {
        let mut state = self.state();
        state
            .turns
            .insert(run, Liveness::Finished { succeeded: false });
        state.exits.insert(run, exit);
    }

    /// Every turn started, in order, with what the member was told.
    #[must_use]
    pub fn started(&self) -> Vec<RunContext> {
        self.state().started.clone()
    }

    /// Every turn asked to stop.
    #[must_use]
    pub fn stopped(&self) -> Vec<RunId> {
        self.state().stopped.clone()
    }
}

impl WorkerAdapter for SimulatedRunner {
    fn start(&self, run: &RunContext) -> Result<(), StartError> {
        let mut state = self.state();
        let script = state
            .scripts
            .get(&run.member)
            .copied()
            .unwrap_or(Script::StaysRunning);
        let liveness = match script {
            Script::WillNotStart => return Err(StartError("the runner said no".to_owned())),
            Script::StaysRunning => Liveness::Running,
            Script::Succeeds => Liveness::Finished { succeeded: true },
            Script::Fails | Script::Killed => Liveness::Finished { succeeded: false },
            Script::Vanishes => Liveness::Gone,
        };
        state.turns.insert(run.run, liveness);
        // A new attempt starts afresh: nothing of how the last one ended is kept.
        state.exits.remove(&run.run);
        if script == Script::Killed {
            state.exits.insert(run.run, Exit::Code(137));
        }
        state.started.push(run.clone());
        Ok(())
    }

    fn status(&self, run: RunId) -> Liveness {
        self.state()
            .turns
            .get(&run)
            .copied()
            .unwrap_or(Liveness::Gone)
    }

    fn exit_of(&self, run: RunId) -> Exit {
        self.state()
            .exits
            .get(&run)
            .copied()
            .unwrap_or(Exit::Unsaid)
    }

    fn stop(&self, run: RunId, _grace: Duration) {
        let mut state = self.state();
        state.stopped.push(run);
        state
            .turns
            .insert(run, Liveness::Finished { succeeded: false });
    }

    fn capacity(&self) -> usize {
        self.capacity
    }
}
