//! Wakes: who is woken, about what, and whether a turn already covers it.

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::routines;
use crate::domain::turns;
use crate::domain::wakes::{self, LiveTurn};
use crate::domain::{
    Brain, EventKind, Member, MemberId, Millis, Receipt, Refusal, Role, RunId, RunStatus, TaskId,
    TaskStatus, WakeDecision, WakeReason, WakeSituation,
};
use serde_json::json;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Asks `member` to take a turn, about `task` if any. Every wake leaves a receipt.
    ///
    /// # Errors
    /// [`EngineError::Store`] if the store fails; [`EngineError::Unknown`] for a member that
    /// does not exist.
    pub fn wake(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<Receipt> {
        let _held = self.held();
        self.wake_held_lock(member, task, reason)
    }

    /// The live turns a wake about `task` meets. A wake of judgment (`judgment`) may join any
    /// of them but a work turn already running on the house's models: it would be judged by a
    /// brain that must not judge (ADR 0131).
    pub(super) fn live_turns(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        judgment: bool,
    ) -> Done<Vec<LiveTurn>> {
        Ok(self
            .store
            .live_runs()?
            .into_iter()
            .filter(|run| match task {
                Some(_) => run.task == task,
                // A wake about no task joins only the member's own turn about no task.
                None => run.task.is_none() && run.member == member,
            })
            .map(|run| LiveTurn {
                run: run.id,
                member: run.member,
                foldable: !(judgment
                    && run.status == RunStatus::Running
                    && run.brain.is_some_and(Brain::house)),
            })
            .collect())
    }

    /// Whether a member has a task of its own that is not over: handed to it, or taken.
    pub(super) fn has_open_task(&self, member: MemberId) -> Done<bool> {
        Ok(self.store.tasks()?.iter().any(|t| {
            !t.status.finished() && (t.assignee == Some(member) || t.owner == Some(member))
        }))
    }

    pub(super) fn situation(
        &self,
        member: &Member,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<WakeSituation> {
        let nothing_to_do =
            task.is_none() && reason.on_a_clock() && !self.has_open_task(member.id)?;
        let already_held = match task {
            Some(task) => self
                .store
                .held_wakes(task)?
                .iter()
                .any(|w| w.member == member.id),
            None => false,
        };
        let waits_for_review = match task {
            Some(task) if !reason.reaches_work_in_review() => {
                let task = self.task(task)?;
                task.status == TaskStatus::InReview
                    && task.owner.or(task.assignee) == Some(member.id)
            }
            _ => false,
        };
        let project = self.project_of(task)?;
        Ok(WakeSituation {
            waits_for_review,
            not_for_the_ceo: member.role == Role::Ceo && !reason.reaches_the_ceo(),
            project_paused: self.held_by_owner(project)?,
            stopped: self.store.stopped()?,
            paused: member.paused,
            furloughed: member.furloughed(),
            // Whether the money is there is not asked of a wake (ADR 0131): the turn is queued,
            // and waits for budget where turns start.
            about_a_task: task.is_some(),
            live: self.live_turns(
                member.id,
                task,
                self.need_of_wake(member, reason)?.kind == turns::TurnKind::Judgment,
            )?,
            already_held,
            nothing_to_do,
            // A wake that brings nothing new, to a member its last turns showed nothing for,
            // again and again (ADR 0124): parked, not woken once more.
            woken_for_nothing: match task {
                Some(task) if !reason.brings_news() => self.woken_for_nothing(member.id, task)?,
                _ => false,
            },
        })
    }

    pub(super) fn decide_wake(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<WakeDecision> {
        let who = self.member(member)?;
        // News is never kept waiting: a turn on its task held back by the back-off, or waiting
        // to try again, is brought forward to take it.
        if let (Some(task), true) = (task, reason.brings_news()) {
            self.bring_forward(task)?;
        }
        let situation = self.situation(&who, task, reason)?;
        let decision = match task {
            // The check after a deploy has one turn (ADR 0115, amended): once its verdict is
            // given, nothing wakes it again.
            Some(task) if self.deploy_check_given(task)? => {
                WakeDecision::Refuse(wakes::Refused::Checked)
            }
            _ => wakes::decide(member, &situation),
        };
        Ok(decision)
    }

    pub(super) fn wake_held_lock(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<Receipt> {
        if reason == WakeReason::Consulted {
            return Err(EngineError::Refused(Refusal::Conversation(
                "legacy consultation wakes are retired; use authorized assistance",
            )));
        }
        let decision = self.decide_wake(member, task, reason)?;
        let now = self.clock.now();
        let mut about = json!({
            "member": member.0, "task": task.map(|t| t.0), "reason": reason.name(),
        });
        match decision {
            WakeDecision::Queue => {
                let wake = self
                    .store
                    .add_wake(member, task, reason, Receipt::Queued, None, now)?;
                self.mark_owners(wake, None)?;
                let (run, delayed) =
                    self.queue_turn(member, task, wake, reason, self.asked_by_owner())?;
                self.store.set_wake(wake, Receipt::Queued, Some(run))?;
                if let Some(until) = delayed {
                    about["delayed_until"] = json!(until);
                }
                self.emit(EventKind::WakeQueued, about)?;
                Ok(Receipt::Queued)
            }
            WakeDecision::Fold(run) => {
                let wake =
                    self.store
                        .add_wake(member, task, reason, Receipt::Folded, Some(run), now)?;
                self.mark_owners(wake, Some(run))?;
                self.emit(EventKind::WakeFolded, about)?;
                Ok(Receipt::Folded)
            }
            WakeDecision::Hold => {
                let wake = self
                    .store
                    .add_wake(member, task, reason, Receipt::Held, None, now)?;
                self.mark_owners(wake, None)?;
                self.emit(EventKind::WakeHeld, about)?;
                Ok(Receipt::Held)
            }
            WakeDecision::Refuse(why) => {
                self.store
                    .add_wake(member, task, reason, Receipt::Refused(why), None, now)?;
                about["why"] = json!(why.name());
                self.emit(EventKind::WakeRefused, about)?;
                Ok(Receipt::Refused(why))
            }
        }
    }

    /// Marks a wake the owner's asking, while the engine does what they asked; and the turn it
    /// folds into, if that has not started yet (its reservation is still to be made).
    pub(super) fn mark_owners(
        &self,
        wake: crate::domain::WakeId,
        folded: Option<RunId>,
    ) -> Done<()> {
        if !self.asked_by_owner() {
            return Ok(());
        }
        self.store.set_wake_by_owner(wake)?;
        let Some(run) = folded else {
            return Ok(());
        };
        let Some(mut run) = self.store.run(run)? else {
            return Ok(());
        };
        if run.started_at.is_none() && !run.asked_by_owner {
            run.asked_by_owner = true;
            self.store.save_run(&run)?;
        }
        Ok(())
    }

    /// How many turns in a row on a task, newest first, moved nothing forward.
    pub(super) fn empty_turns(&self, task: TaskId) -> Done<usize> {
        let mut ended: Vec<Run> = self
            .store
            .runs_of(task)?
            .into_iter()
            .filter(|r| !r.status.live() && r.started_at.is_some())
            .collect();
        ended.sort_by_key(|r| core::cmp::Reverse(r.id));
        // A turn's label says whether it did something (ADR 0114). A turn with none (from
        // before labels, or that never settled) cannot be told, so the count stops there
        // rather than guess from whether it moved its task (ADR 0124).
        Ok(ended
            .iter()
            .take_while(|r| r.label.is_some_and(|l| !l.productive()))
            .count())
    }

    /// How long a wake that brings nothing new to `task` waits now (ADR 0107): its back-off,
    /// and at least until [`wakes::AGAIN_AFTER_MS`] after `member`'s last turn on the task ended.
    pub(super) fn back_off(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<Millis> {
        let Some(task) = task else {
            return Ok(0);
        };
        if reason.brings_news() {
            return Ok(0);
        }
        let last_ended = self
            .store
            .runs_of(task)?
            .iter()
            .filter(|r| r.member == member)
            .filter_map(|r| r.ended_at)
            .max();
        let gap = last_ended.map_or(0, |at| {
            (at + wakes::AGAIN_AFTER_MS - self.clock.now()).max(0)
        });
        Ok(routines::back_off(self.empty_turns(task)?).max(gap))
    }

    /// Queues a turn for a wake: at once, or, for a stale wake on a task whose turns keep
    /// moving nothing, waiting out its back-off. Returns the turn, and until when it waits. A
    /// wake the owner's asking raised starts a turn the owner asked for.
    pub(super) fn queue_turn(
        &self,
        member: MemberId,
        task: Option<TaskId>,
        wake: crate::domain::WakeId,
        reason: WakeReason,
        by_owner: bool,
    ) -> Done<(RunId, Option<Millis>)> {
        let now = self.clock.now();
        let id = self.store.add_run(member, task, wake, now)?;
        let mut run = self.store.run(id)?.ok_or(EngineError::Unknown("run"))?;
        if by_owner {
            run.asked_by_owner = true;
            self.store.save_run(&run)?;
        }
        let wait = self.back_off(member, task, reason)?;
        if wait == 0 {
            return Ok((id, None));
        }
        run.status = RunStatus::RetryWaiting;
        run.retry_at = Some(now + wait);
        run.end_reason =
            "backing off: its last turns moved nothing, or it took one on this moments ago"
                .to_owned();
        self.store.save_run(&run)?;
        Ok((id, Some(now + wait)))
    }

    /// Brings a task's waiting turn forward, to start on the next tick.
    pub(super) fn bring_forward(&self, task: TaskId) -> Done<()> {
        for mut run in self.store.live_runs()? {
            if run.task != Some(task) || run.status != RunStatus::RetryWaiting {
                continue;
            }
            run.status = RunStatus::Queued;
            run.retry_at = None;
            self.store.save_run(&run)?;
        }
        Ok(())
    }

    /// Releases the oldest wake held for a task, now that its turn has ended.
    pub(super) fn release_held(&self, task: TaskId) -> Done<()> {
        let Some(oldest) = self.store.held_wakes(task)?.into_iter().next() else {
            return Ok(());
        };
        let who = self.member(oldest.member)?;
        let decision = wakes::decide(
            oldest.member,
            &self.situation(&who, Some(task), oldest.reason)?,
        );
        match decision {
            WakeDecision::Queue => {
                let (run, _) = self.queue_turn(
                    oldest.member,
                    Some(task),
                    oldest.id,
                    oldest.reason,
                    oldest.by_owner,
                )?;
                self.store.set_wake(oldest.id, Receipt::Queued, Some(run))?;
                self.emit(
                    EventKind::WakeQueued,
                    json!({ "member": oldest.member.0, "task": task.0, "reason": oldest.reason.name() }),
                )?;
            }
            WakeDecision::Fold(run) => {
                self.store.set_wake(oldest.id, Receipt::Folded, Some(run))?
            }
            // Still held behind another turn; it waits on.
            WakeDecision::Hold | WakeDecision::Refuse(wakes::Refused::Duplicate) => {}
            WakeDecision::Refuse(why) => {
                self.store
                    .set_wake(oldest.id, Receipt::Refused(why), None)?;
            }
        }
        Ok(())
    }
}
