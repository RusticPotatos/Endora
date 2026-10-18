//! Replay for the owner's screen: a turn, or a task's turns, read back from the record
//! ([`crate::application::replay`]); and the events a turn's calls emit, named with its run so
//! that a turn's replay takes its own events by id rather than by time.

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::application::replay::{self, Record, RunReplay, TAGGED_FROM};
use crate::domain::{RunId, TaskId};

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Says which turn's call the engine is taking now: `None` as the lock is taken, the run
    /// once [`Self::running`] has found it running.
    pub(super) fn act_for(&self, run: Option<RunId>) {
        *self
            .acting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = run;
    }

    /// An event's data, naming the turn whose call emitted it unless it names a run already.
    /// The first time any does, the store keeps when ([`TAGGED_FROM`]): a turn queued since
    /// has every event of its own named.
    pub(super) fn name_the_turn(&self, mut data: serde_json::Value) -> Done<serde_json::Value> {
        let acting = *self
            .acting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(run) = acting else {
            return Ok(data);
        };
        let Some(fields) = data.as_object_mut() else {
            return Ok(data);
        };
        fields
            .entry("run")
            .or_insert_with(|| serde_json::json!(run.0));
        if self.store.setting(TAGGED_FROM)?.is_none() {
            self.store
                .set_setting(TAGGED_FROM, &self.clock.now().to_string())?;
        }
        Ok(data)
    }

    /// A turn, read back: everything that happened in it, in order, for the owner's screen.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a run that does not exist; a store failure.
    pub fn replay_run(&self, run: RunId) -> Done<RunReplay> {
        let _held = self.held();
        let found = self.store.run(run)?.ok_or(EngineError::Unknown("run"))?;
        Ok(self.replayed(&[found])?.remove(0))
    }

    /// A task's turns, read back, oldest first.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that does not exist; a store failure.
    pub fn replay_task(&self, task: TaskId) -> Done<Vec<RunReplay>> {
        let _held = self.held();
        self.task(task)?;
        let runs: Vec<_> = self
            .store
            .runs()?
            .into_iter()
            .filter(|r| r.task == Some(task))
            .collect();
        self.replayed(&runs)
    }

    /// The record the turns are read from, gathered once for them all.
    fn replayed(&self, runs: &[crate::application::ports::Run]) -> Done<Vec<RunReplay>> {
        let members = self.store.members()?;
        let wakes = self.store.wakes()?;
        let events = self.store.events_after(0)?;
        let tagged_from = self
            .store
            .setting(TAGGED_FROM)?
            .and_then(|at| at.parse().ok());
        let (mut receipts, mut ledger, mut posts, mut cited) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut read: Vec<TaskId> = Vec::new();
        for run in runs {
            receipts.extend(self.store.call_receipts_of(run.id)?);
            ledger.extend(self.store.ledger_of(run.id)?);
            for task in replay::tasks_touched(run, &events) {
                if read.contains(&task) {
                    continue;
                }
                read.push(task);
                posts.extend(self.store.thread_cited(task)?);
                cited.extend(self.store.cited(task)?);
            }
        }
        let record = Record {
            members: &members,
            wakes: &wakes,
            receipts: &receipts,
            posts: &posts,
            cited: &cited,
            events: &events,
            ledger: &ledger,
            tagged_from,
            now: self.clock.now(),
        };
        let mut all = replay::replay_all(&record, runs);
        for turn in &mut all {
            turn.briefed = self.store.briefed(turn.run.id)?;
            turn.transcript = self.store.transcript(turn.run.id)?;
        }
        Ok(all)
    }
}
