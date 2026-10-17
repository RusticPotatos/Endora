//! The remote runner (spec section 15, phase 4): HQ's side of `endora-runner`. HQ never
//! reaches out to a runner; the runner asks HQ for work. So this adapter only keeps, in
//! memory, the turns waiting to start, the ones handed out, how each ended, and the stops
//! asked for, and HQ's runner routes read and write them. A turn the runner never reports is
//! ended by the engine's longest-turn limit.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

use crate::application::ports::{RunContext, StartError, WorkerAdapter};
use crate::domain::{Liveness, RunId};

/// How many turns a runner takes at once unless HQ is told otherwise.
pub const DEFAULT_CAPACITY: usize = 2;

/// The most reported ends, and the most stops not yet passed on, kept at once. A tick reads
/// an end within seconds, and a runner asks for stops every few; the limit only keeps a
/// long-lived HQ's memory from growing with every turn it ever ran, or with stops no runner
/// came for (a runner kills a turn past its longest anyway).
const KEPT: usize = 1_000;

/// A turn handed out to the runner, with its lease: the number that names this attempt of
/// the run, and no other. A run tried again (after a failure, or after HQ restarted and lost
/// track of it) is handed out with a new lease, so a report from the earlier attempt's
/// container cannot end the new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handed {
    /// The turn.
    pub turn: RunContext,
    /// This attempt's lease.
    pub lease: u64,
}

#[derive(Default)]
struct State {
    waiting: VecDeque<RunContext>,
    /// The runs handed out, each with its current lease.
    handed: HashMap<RunId, u64>,
    ended: VecDeque<(RunId, bool)>,
    stops: VecDeque<RunId>,
    /// The next lease to give. Seeded from the clock, so an HQ that restarts never hands out
    /// a lease an earlier one already gave.
    next_lease: u64,
}

/// A runner on another host, which asks HQ for the turns to start.
pub struct RemoteRunner {
    capacity: usize,
    state: Mutex<State>,
}

impl Default for RemoteRunner {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl RemoteRunner {
    /// A runner that can take `capacity` turns at once.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        // A thousand leases a millisecond: far more turns than a runner could ever take.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| {
                u64::try_from(d.as_millis()).unwrap_or(u64::MAX / 1000) * 1000
            });
        Self {
            capacity,
            state: Mutex::new(State {
                next_lease: seed,
                ..State::default()
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The oldest turn waiting to start, handed out now with its lease; `None` when none
    /// waits.
    pub fn next(&self) -> Option<Handed> {
        let mut state = self.state();
        let turn = state.waiting.pop_front()?;
        let lease = state.next_lease;
        state.next_lease = state.next_lease.wrapping_add(1);
        state.handed.insert(turn.run, lease);
        Some(Handed { turn, lease })
    }

    /// The runner reports how a turn it was handed ended, quoting its lease. The engine's
    /// next tick reads it. A report about a turn it was not handed, one the engine has
    /// already stopped, or one from an earlier attempt (its lease is not the current one) is
    /// not wanted, changes nothing, and says so.
    pub fn ended(&self, run: RunId, lease: u64, succeeded: bool) -> bool {
        let mut state = self.state();
        if state.handed.get(&run) != Some(&lease) {
            return false;
        }
        state.handed.remove(&run);
        state.ended.push_back((run, succeeded));
        if state.ended.len() > KEPT {
            state.ended.pop_front();
        }
        true
    }

    /// The turns asked to stop since the runner last asked, oldest first.
    pub fn stops(&self) -> Vec<RunId> {
        self.state().stops.drain(..).collect()
    }
}

impl WorkerAdapter for RemoteRunner {
    /// Queues the turn until the runner asks for work. A turn tried again starts afresh,
    /// with its new token.
    fn start(&self, run: &RunContext) -> Result<(), StartError> {
        let mut state = self.state();
        state.waiting.retain(|t| t.run != run.run);
        state.handed.remove(&run.run);
        state.ended.retain(|(r, _)| *r != run.run);
        state.waiting.push_back(run.clone());
        Ok(())
    }

    fn status(&self, run: RunId) -> Liveness {
        let state = self.state();
        if let Some((_, succeeded)) = state.ended.iter().find(|(r, _)| *r == run) {
            return Liveness::Finished {
                succeeded: *succeeded,
            };
        }
        if state.handed.contains_key(&run) || state.waiting.iter().any(|t| t.run == run) {
            return Liveness::Running;
        }
        Liveness::Gone
    }

    /// A turn still waiting is dropped: the runner never had it. Any other is passed on for
    /// the runner to kill, including one HQ does not know, which may still be on the runner
    /// from before HQ restarted. The runner keeps its own grace.
    fn stop(&self, run: RunId, _grace: Duration) {
        let mut state = self.state();
        let waited = state.waiting.len();
        state.waiting.retain(|t| t.run != run);
        if state.waiting.len() < waited {
            return;
        }
        state.handed.remove(&run);
        state.ended.retain(|(r, _)| *r != run);
        state.stops.push_back(run);
        if state.stops.len() > KEPT {
            state.stops.pop_front();
        }
    }

    fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::engine::{Author, Engine, Settings};
    use crate::application::ports::{Clock, NewTask, TeamStore};
    use crate::domain::{MemberId, Priority, Role, RunStatus};
    use crate::infrastructure::SqliteTeamStore;

    const GRACE: Duration = Duration::from_secs(10);

    fn turn(run: i64, token: &str) -> RunContext {
        RunContext {
            run: RunId(run),
            member: MemberId(1),
            task: None,
            told: "You are the developer.".to_owned(),
            token: token.to_owned(),
        }
    }

    #[test]
    fn turns_wait_and_are_handed_out_oldest_first_once_each() {
        let runner = RemoteRunner::default();
        runner.start(&turn(1, "first")).unwrap();
        runner.start(&turn(2, "second")).unwrap();
        assert_eq!(runner.status(RunId(1)), Liveness::Running, "waiting");
        let first = runner.next().unwrap();
        assert_eq!(
            (first.turn.run, first.turn.token.as_str()),
            (RunId(1), "first")
        );
        assert_eq!(runner.status(RunId(1)), Liveness::Running, "handed out");
        let second = runner.next().unwrap();
        assert_eq!(second.turn.run, RunId(2));
        assert_ne!(first.lease, second.lease, "each attempt its own lease");
        assert_eq!(runner.next(), None, "nothing left to hand out");
    }

    #[test]
    fn a_reported_end_is_how_the_turn_ended() {
        let runner = RemoteRunner::default();
        runner.start(&turn(1, "a")).unwrap();
        runner.start(&turn(2, "b")).unwrap();
        let a = runner.next().unwrap();
        let b = runner.next().unwrap();
        assert!(runner.ended(RunId(1), a.lease, true));
        assert!(runner.ended(RunId(2), b.lease, false));
        assert_eq!(
            runner.status(RunId(1)),
            Liveness::Finished { succeeded: true }
        );
        assert_eq!(
            runner.status(RunId(2)),
            Liveness::Finished { succeeded: false }
        );
    }

    #[test]
    fn a_report_about_a_turn_never_handed_out_changes_nothing() {
        let runner = RemoteRunner::default();
        runner.start(&turn(1, "a")).unwrap();
        assert!(!runner.ended(RunId(1), 0, true));
        assert!(!runner.ended(RunId(9), 0, true));
        assert_eq!(runner.status(RunId(1)), Liveness::Running, "still waiting");
        assert_eq!(runner.status(RunId(9)), Liveness::Gone);
    }

    #[test]
    fn a_run_it_never_had_is_gone() {
        assert_eq!(RemoteRunner::default().status(RunId(7)), Liveness::Gone);
    }

    #[test]
    fn a_waiting_turn_asked_to_stop_is_dropped_and_the_runner_is_not_bothered() {
        let runner = RemoteRunner::default();
        runner.start(&turn(1, "a")).unwrap();
        runner.stop(RunId(1), GRACE);
        assert_eq!(runner.next(), None);
        assert!(runner.stops().is_empty(), "the runner never had it");
    }

    #[test]
    fn a_handed_out_turn_asked_to_stop_is_passed_on_once() {
        let runner = RemoteRunner::default();
        runner.start(&turn(1, "a")).unwrap();
        runner.start(&turn(2, "b")).unwrap();
        let a = runner.next().unwrap();
        runner.next().unwrap();
        runner.stop(RunId(2), GRACE);
        runner.stop(RunId(1), GRACE);
        // One HQ forgot on restarting may still be on the runner: it is passed on too.
        runner.stop(RunId(9), GRACE);
        assert_eq!(runner.stops(), vec![RunId(2), RunId(1), RunId(9)]);
        assert!(runner.stops().is_empty(), "cleared once given");
        // The engine ended it when it asked; the runner's word after that is not wanted.
        assert!(!runner.ended(RunId(1), a.lease, false));
        assert_eq!(runner.status(RunId(1)), Liveness::Gone);
    }

    #[test]
    fn a_turn_tried_again_starts_afresh_with_its_new_token() {
        let runner = RemoteRunner::default();
        runner.start(&turn(1, "old")).unwrap();
        let old = runner.next().unwrap();
        assert!(runner.ended(RunId(1), old.lease, false));
        runner.start(&turn(1, "new")).unwrap();
        assert_eq!(runner.status(RunId(1)), Liveness::Running);
        assert_eq!(runner.next().unwrap().turn.token, "new");
    }

    #[test]
    fn a_report_from_an_earlier_attempt_cannot_end_the_new_one() {
        // HQ restarted and lost track of run 1; the engine tried it again, and the runner was
        // handed the new attempt while the old container still ran.
        let runner = RemoteRunner::default();
        runner.start(&turn(1, "old")).unwrap();
        let old = runner.next().unwrap();
        runner.start(&turn(1, "new")).unwrap();
        let new = runner.next().unwrap();
        // The old container, its token refused, exits and is reported failed.
        assert!(!runner.ended(RunId(1), old.lease, false), "a stale report");
        assert_eq!(
            runner.status(RunId(1)),
            Liveness::Running,
            "the new attempt runs on"
        );
        assert!(runner.ended(RunId(1), new.lease, true));
        assert_eq!(
            runner.status(RunId(1)),
            Liveness::Finished { succeeded: true }
        );
    }

    #[test]
    fn its_capacity_is_two_unless_set() {
        assert_eq!(RemoteRunner::default().capacity(), 2);
        assert_eq!(RemoteRunner::new(5).capacity(), 5);
    }

    #[test]
    fn a_long_lived_hq_keeps_only_the_latest_reports() {
        let runner = RemoteRunner::default();
        let most = i64::try_from(KEPT).unwrap();
        // One more end than it keeps: the oldest goes, the newest stays.
        for run in 0..=most {
            runner.start(&turn(run, "t")).unwrap();
            let handed = runner.next().unwrap();
            assert!(runner.ended(RunId(run), handed.lease, true));
        }
        assert_eq!(runner.state.lock().unwrap().ended.len(), KEPT);
        assert_eq!(
            runner.status(RunId(0)),
            Liveness::Gone,
            "the oldest end let go"
        );
        assert_eq!(
            runner.status(RunId(most)),
            Liveness::Finished { succeeded: true }
        );
        // One more stop than it keeps, for turns it no longer holds.
        for run in 0..=most {
            runner.stop(RunId(run + most + 1), GRACE);
        }
        let stops = runner.stops();
        assert_eq!(stops.len(), KEPT);
        assert_eq!(
            stops.first(),
            Some(&RunId(most + 2)),
            "the oldest stop let go"
        );
        assert_eq!(stops.last(), Some(&RunId(2 * most + 1)), "newest kept");
    }

    struct Noon;
    impl Clock for Noon {
        fn now(&self) -> i64 {
            1_792_166_400_000 // 2026-10-16 12:00 UTC
        }
    }

    #[test]
    fn the_next_tick_ends_a_turn_the_runner_reported() {
        let hq = Engine::new(
            SqliteTeamStore::in_memory().unwrap(),
            RemoteRunner::default(),
            Noon,
            Settings::default(),
        );
        let dev = hq.hire("John Doe", Role::Developer, 40, 2, 0).unwrap();
        let goal = hq
            .set_goal("Keep the lab tidy", "a test", Author::Owner)
            .unwrap();
        hq.add_task(
            Author::Owner,
            NewTask {
                goal,
                parent: None,
                title: "Sort the cables".to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(hq.tick().unwrap().started, 1);
        let handed = hq.runner().next().unwrap();
        assert_eq!(
            hq.turn_of(&handed.turn.token).unwrap().unwrap().0.id,
            handed.turn.run
        );
        assert_eq!(hq.tick().unwrap().ended, 0, "still running on the runner");
        assert!(hq.runner().ended(handed.turn.run, handed.lease, true));
        assert_eq!(hq.tick().unwrap().ended, 1);
        let run = hq.store().run(handed.turn.run).unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Succeeded);
    }
}
