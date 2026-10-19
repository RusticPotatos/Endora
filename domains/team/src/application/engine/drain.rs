//! A drain before a restart (ADR 0134). Deploys, and the NAS's own security reboots, restart
//! HQ and the runner under turns of ten or fifteen minutes, and what such a turn had done since
//! its last note was lost. A restart is now announced first. While the drain lasts nothing new
//! starts (what is queued waits, nothing is dropped), every answer a running turn gets says a
//! restart is coming, and a member may end its turn with a handover note and keep its task. The
//! drain does not kill anything: whoever restarts waits for the turns to end, or for the cap, and
//! what the restart still cuts is retried by the path every interrupted turn takes, counted, and
//! told to the owner once, in the digest, only when it cut something. HQ starting again lifts the
//! drain; so does anyone saying resume, and so does half an hour, so that a drain nobody ended
//! cannot stall the team.

use serde_json::{Value, json};

use super::{Done, Engine};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::cut::cut;
use crate::domain::drain::{Drain, Lifted, Outcome, cut_said};
use crate::domain::world::HandoverNote;
use crate::domain::{EventKind, Fault, Refusal, RunId, RunStatus, TaskId};

/// The setting that keeps a drain while one lasts, and is empty when none does.
const KEPT: &str = "drain";
/// The setting that keeps what the last drain came to.
const LAST: &str = "drain_last";
/// The most characters of what a restart cut that the owner is told.
const TOLD_MOST: usize = 600;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The drain the store keeps, if one is kept.
    fn kept_drain(&self) -> Done<Option<Drain>> {
        Ok(self.store.setting(KEPT)?.as_deref().and_then(Drain::read))
    }

    /// The turns running now.
    fn running_turns(&self) -> Done<Vec<Run>> {
        Ok(self
            .store
            .runs()?
            .into_iter()
            .filter(|r| r.status == RunStatus::Running)
            .collect())
    }

    /// The drain that holds now: one that is kept and has not outlasted its age. One that has is
    /// lifted at the next tick, but never holds in the meantime.
    pub(super) fn live_drain(&self) -> Done<Option<Drain>> {
        let now = self.clock.now();
        Ok(self.kept_drain()?.filter(|d| !d.forgotten(now)))
    }

    /// Whether a drain holds now. A store that cannot say is no drain: it never keeps the team
    /// from its work.
    #[must_use]
    pub fn draining(&self) -> bool {
        self.live_drain().ok().flatten().is_some()
    }

    /// Begins a drain, with `cap_seconds` for a deploy to wait for the turns under way (the
    /// default when none). One that already lasts is left as it began: asking again changes
    /// nothing. Returns [`Self::drain_shown`].
    ///
    /// # Errors
    /// A store failure.
    pub fn begin_drain(&self, cap_seconds: Option<i64>) -> Done<Value> {
        let _held = self.held();
        let now = self.clock.now();
        self.drain_tick_held_lock(now)?;
        if self.kept_drain()?.is_some() {
            return self.drain_shown();
        }
        let running: Vec<RunId> = self.running_turns()?.iter().map(|r| r.id).collect();
        let drain = Drain::begin(now, cap_seconds, running);
        self.store.set_setting(KEPT, &drain.keep())?;
        self.emit(
            EventKind::DrainBegan,
            json!({ "cap_seconds": drain.cap_seconds, "running": drain.holding.len() }),
        )?;
        self.drain_shown()
    }

    /// Lifts the drain, if one lasts: what a deploy says once it is done, in case HQ was not
    /// restarted. Returns [`Self::drain_shown`].
    ///
    /// # Errors
    /// A store failure.
    pub fn end_drain(&self) -> Done<Value> {
        let _held = self.held();
        self.drain_tick_held_lock(self.clock.now())?;
        if let Some(drain) = self.kept_drain()? {
            self.lift_held_lock(&drain, Lifted::Resumed)?;
        }
        self.drain_shown()
    }

    /// The drain as the API tells it: whether one lasts, since when and with what cap, how many
    /// turns are running and how many wait, and what the last one came to.
    ///
    /// # Errors
    /// A store failure.
    pub fn drain_shown(&self) -> Done<Value> {
        let drain = self.live_drain()?;
        let last = self.store.setting(LAST)?.as_deref().and_then(Outcome::read);
        let runs = self.store.runs()?;
        Ok(json!({
            "draining": drain.is_some(),
            "since": drain.as_ref().map(|d| d.since),
            "cap_seconds": drain.as_ref().map(|d| d.cap_seconds),
            "running": runs.iter().filter(|r| r.status == RunStatus::Running).count(),
            "waiting": runs.iter().filter(|r| r.status == RunStatus::Queued).count(),
            "last": last.map(|o| json!({
                "since": o.since, "by": o.by.name(), "held": o.held,
                "clean": o.clean, "cut": o.cut })),
        }))
    }

    /// What a tick does about a drain, before anything else: one kept by an HQ that is gone was
    /// for the restart that has now happened, and a drain nobody ended outlasts its age. The lock
    /// is held.
    pub(super) fn drain_tick_held_lock(&self, now: crate::domain::Millis) -> Done<()> {
        let Some(drain) = self.kept_drain()? else {
            self.drain_checked
                .store(true, std::sync::atomic::Ordering::SeqCst);
            return Ok(());
        };
        // The first look this HQ takes: a drain it did not begin is the last one's.
        if !self
            .drain_checked
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return self.lift_held_lock(&drain, Lifted::Restarted);
        }
        if drain.forgotten(now) {
            return self.lift_held_lock(&drain, Lifted::Forgotten);
        }
        Ok(())
    }

    /// Lifts `drain`, `by` the way it ended: what it came to is kept and told in the feed, and a
    /// restart that cut turns is one item in the owner's digest, naming them. A drain that cut
    /// none says nothing but its feed line. The lock is held.
    fn lift_held_lock(&self, drain: &Drain, by: Lifted) -> Done<()> {
        let running = self.running_turns()?;
        let ids: Vec<RunId> = running.iter().map(|r| r.id).collect();
        let outcome = Outcome::of(drain, &ids, by);
        self.store.set_setting(KEPT, "")?;
        self.store.set_setting(LAST, &outcome.keep())?;
        self.emit(
            EventKind::DrainEnded,
            json!({ "by": by.name(), "since": drain.since, "held": outcome.held,
                    "clean": outcome.clean, "cut": outcome.cut }),
        )?;
        // What waited for the restart waits for nothing now: a turn that is still held back for
        // another reason says that when it is next looked at.
        let said = crate::domain::turns::Wait::Restart.words();
        for mut run in self.store.runs()? {
            if run.status == RunStatus::Queued && run.end_reason == said {
                run.end_reason.clear();
                self.store.save_run(&run)?;
            }
        }
        if outcome.cut == 0 {
            return Ok(());
        }
        let mut who = Vec::new();
        for turn in &running {
            let member = self.member(turn.member)?.name;
            who.push(match turn.task {
                Some(task) => format!("{member} on #{} {}", task.0, self.task(task)?.title),
                None => format!("{member}, on no task"),
            });
        }
        let item = self.store.add_desk(
            "notice",
            None,
            None,
            None,
            &cut(&cut_said(&who), TOLD_MOST),
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": item.0, "kind": "notice", "task": null }),
        )
    }

    /// Holds every queued turn back while a drain lasts, each saying so, and starts none. Nothing
    /// is dropped: the turn is as it was, and starts when the drain is over. Returns whether a
    /// drain holds. The lock is held.
    pub(super) fn hold_for_restart_held_lock(&self) -> Done<bool> {
        if self.live_drain()?.is_none() {
            return Ok(false);
        }
        let said = crate::domain::turns::Wait::Restart.words();
        for mut run in self.store.runs()? {
            if run.status != RunStatus::Queued || run.end_reason == said {
                continue;
            }
            run.end_reason.clone_from(&said);
            self.store.save_run(&run)?;
        }
        Ok(true)
    }

    /// A member ends its turn with a handover note while a drain lasts, and keeps its task: the
    /// note is kept for its own next turn on the task, which comes as soon as the drain is over,
    /// and the start of this one does not count against its retries. The task is not handed on,
    /// blocked or stuck.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::NoRestartComing`] when no drain lasts;
    /// [`Refusal::OutOfReach`] for a task that is not the turn's own; a store failure.
    pub fn wrap_up_in(&self, run: RunId, task: TaskId, note: &HandoverNote) -> Done<()> {
        let _held = self.held();
        let mut turn = self.running(run)?;
        if self.live_drain()?.is_none() {
            return Err(super::EngineError::Refused(Refusal::NoRestartComing));
        }
        if turn.task != Some(task) {
            return Err(super::EngineError::Refused(Refusal::OutOfReach));
        }
        self.keep_handover(&turn, task, Some(turn.member), Some(turn.member), note)?;
        // Not the member's failing, nor the platform's: it ends here, and starts again.
        turn.attempts = turn.attempts.saturating_sub(1);
        self.fail(turn, "wrapped up for a restart", Fault::Worker)?;
        Ok(())
    }
}
