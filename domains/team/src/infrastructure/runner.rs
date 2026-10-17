//! The runner HQ uses: the simulated one for its labelled demo, else the remote one, which a
//! real runner asks for work.

use std::time::Duration;

use crate::application::ports::{RunContext, StartError, WorkerAdapter};
use crate::domain::{Liveness, RunId};
use crate::infrastructure::{RemoteRunner, SimulatedRunner};

/// Either runner, chosen when HQ starts.
pub enum Runner {
    /// Scripted turns: the demo team, no model, no cost.
    Simulated(SimulatedRunner),
    /// Real turns, on a runner that asks HQ for them.
    Remote(RemoteRunner),
}

impl Runner {
    /// The simulated runner, while the demo is on.
    #[must_use]
    pub fn simulated(&self) -> Option<&SimulatedRunner> {
        let Self::Simulated(runner) = self else {
            return None;
        };
        Some(runner)
    }

    /// The remote runner, while the demo is off.
    #[must_use]
    pub fn remote(&self) -> Option<&RemoteRunner> {
        let Self::Remote(runner) = self else {
            return None;
        };
        Some(runner)
    }

    fn adapter(&self) -> &dyn WorkerAdapter {
        match self {
            Self::Simulated(runner) => runner,
            Self::Remote(runner) => runner,
        }
    }
}

impl WorkerAdapter for Runner {
    fn start(&self, run: &RunContext) -> Result<(), StartError> {
        self.adapter().start(run)
    }

    fn status(&self, run: RunId) -> Liveness {
        self.adapter().status(run)
    }

    fn stop(&self, run: RunId, grace: Duration) {
        self.adapter().stop(run, grace);
    }

    fn capacity(&self) -> usize {
        self.adapter().capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MemberId;
    use crate::infrastructure::Script;

    fn turn(run: i64) -> RunContext {
        RunContext {
            run: RunId(run),
            member: MemberId(1),
            task: None,
            told: String::new(),
            token: "t".to_owned(),
            effort: None,
        }
    }

    #[test]
    fn the_simulated_runner_takes_the_turns_while_the_demo_is_on() {
        let runner = Runner::Simulated(SimulatedRunner::new(4));
        assert!(runner.remote().is_none());
        let scripted = runner.simulated().unwrap();
        scripted.script(MemberId(1), Script::Succeeds);
        runner.start(&turn(1)).unwrap();
        assert_eq!(scripted.started().len(), 1);
        assert_eq!(
            runner.status(RunId(1)),
            Liveness::Finished { succeeded: true }
        );
        runner.stop(RunId(1), Duration::ZERO);
        assert_eq!(scripted.stopped(), vec![RunId(1)]);
        assert_eq!(runner.capacity(), 4);
    }

    #[test]
    fn the_remote_runner_takes_them_otherwise() {
        let runner = Runner::Remote(RemoteRunner::new(3));
        assert!(runner.simulated().is_none());
        let remote = runner.remote().unwrap();
        runner.start(&turn(1)).unwrap();
        assert_eq!(runner.status(RunId(1)), Liveness::Running);
        assert_eq!(remote.next().unwrap().turn.run, RunId(1));
        runner.stop(RunId(1), Duration::ZERO);
        assert_eq!(remote.stops(), vec![RunId(1)]);
        assert_eq!(runner.capacity(), 3);
    }
}
