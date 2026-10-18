//! The guards (ADR 0124), in the engine: the stopped diagnosis read from the record for each
//! open task, recovery wakes recorded before they are sent, guard findings that open once a
//! key and clear themselves, and the owner's or a lead's verdict on each.
//!
//! The diagnosis replaces "did its last turn move it forward" in every stuck decision: only
//! work that is **parked** (stopped by accident) is nudged and climbs the ladder; work that is
//! **held** (stopped on purpose, with something live to bring it back) is left alone; and work
//! HQ **cannot tell** about is never taken for fine: the lead of its area looks at it once.

use serde_json::json;

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::closure::{Closed, Closure, DoneWhy, DropWhy, not_a_closure};
use crate::domain::guards::{
    self, Diagnosis, Finding, Guard, Stopped, THRESHOLDS, TaskFacts, Verdict,
};
use crate::domain::{
    CheckBackStatus, EventKind, MemberId, RunStatus, Task, TaskId, TaskStatus, WakeReason,
};

/// The wakes that are HQ's own recovery, left out of what a nudge "saw": a nudge must not make
/// itself new.
const OWN_WAKES: [WakeReason; 3] = [
    WakeReason::Recovery,
    WakeReason::AssignmentRecovery,
    WakeReason::Guard,
];

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- the stopped diagnosis ----------------------------------------------------------------

    /// What the diagnosis reads of `task`, from the record now.
    fn task_facts(&self, task: &Task) -> Done<TaskFacts> {
        let runs = self.store.runs()?;
        let on_it: Vec<&Run> = runs.iter().filter(|r| r.task == Some(task.id)).collect();
        let holder = task.owner.or(task.assignee);
        let members = self.store.members()?;
        let holder_at_work = holder.map(|h| members.iter().any(|m| m.id == h && m.at_work()));
        let last_turn_failed = on_it
            .iter()
            .filter(|r| !r.status.live())
            .max_by_key(|r| r.id)
            .is_some_and(|r| r.status == RunStatus::Failed);
        let live_wake = match holder {
            None => false,
            Some(h) => {
                self.store.check_backs()?.iter().any(|c| {
                    c.task == task.id && c.member == h && c.status == CheckBackStatus::Pending
                }) || self.waiting_question(h, task.id)?
            }
        };
        let woken_for_nothing = match holder {
            None => false,
            Some(h) => self.woken_for_nothing(h, task.id)?,
        };
        Ok(TaskFacts {
            status: task.status,
            holder_at_work,
            live_turn: on_it.iter().any(|r| r.status.live()),
            held_wake: !self.store.held_wakes(task.id)?.is_empty(),
            waits_on_open_work: !self.waiting_on(task)?.is_empty(),
            wait_dropped: self.dropped_wait(&task.waits_on)?.is_some(),
            blocked_reason: self.blocked_reason(task.id)?,
            live_wake,
            last_turn_failed,
            woken_for_nothing,
        })
    }

    /// Whether `member`'s last [`THRESHOLDS`]`.redundant_wakes` turns on `task`, in a row, each
    /// showed nothing (ADR 0124).
    pub(super) fn woken_for_nothing(&self, member: MemberId, task: TaskId) -> Done<bool> {
        Ok(self
            .shown_nothing(member, task)?
            .iter()
            .rev()
            .take_while(|(_, nothing)| *nothing)
            .count()
            >= THRESHOLDS.redundant_wakes)
    }

    /// Whether a member said why the task is blocked: its newest move by a member is to
    /// blocked, with a reason.
    fn blocked_reason(&self, task: TaskId) -> Done<bool> {
        let members = self.store.members()?;
        let newest_move = self
            .store
            .thread(task)?
            .into_iter()
            .rev()
            .find(|p| p.text.contains(" → ") && members.iter().any(|m| m.name == p.author));
        Ok(newest_move.is_some_and(|p| {
            p.text
                .split_once(" → blocked: ")
                .is_some_and(|(_, why)| !why.trim().is_empty())
        }))
    }

    /// The stopped diagnosis of `task` as the record stands now (ADR 0124); `None` for a task
    /// that is finished, or gone.
    pub(crate) fn diagnosis(&self, task: TaskId) -> Done<Option<Diagnosis>> {
        let Some(task) = self.store.task(task)? else {
            return Ok(None);
        };
        Ok(guards::diagnose(&self.task_facts(&task)?))
    }

    // ---- receipted recovery wakes -------------------------------------------------------------

    /// What HQ sees of a task, for a nudge's key: where it is, and the newest of what anyone
    /// but HQ did on it (posts, evidence cited, receipts, wakes that were not HQ's own nudges).
    fn task_saw(&self, task: &Task) -> Done<String> {
        let posts = self
            .store
            .thread(task.id)?
            .iter()
            .filter(|p| p.author != "endora")
            .count();
        let cited = self.store.cited(task.id)?.len();
        let mut receipt = 0;
        for run in self
            .store
            .runs()?
            .iter()
            .filter(|r| r.task == Some(task.id))
        {
            for r in self.store.call_receipts_of(run.id)? {
                receipt = receipt.max(r.id);
            }
        }
        let wake = self
            .store
            .wakes()?
            .iter()
            .filter(|w| w.task == Some(task.id) && !OWN_WAKES.contains(&w.reason))
            .map(|w| w.id.0)
            .max()
            .unwrap_or(0);
        Ok(format!(
            "{}|posts:{posts}|cited:{cited}|receipt:{receipt}|wake:{wake}",
            task.status.name()
        ))
    }

    /// A recovery wake for stuck work (ADR 0124): recorded, under what it saw, before it is
    /// sent; a key recorded before is never sent again. Returns whether it was sent.
    pub(super) fn nudge_once(
        &self,
        task: &Task,
        member: MemberId,
        reason: WakeReason,
    ) -> Done<bool> {
        let saw = self.task_saw(task)?;
        let key = guards::nudge_key(task.id.0, member.0, reason.name(), &saw);
        let now = self.clock.now();
        if !self
            .store
            .record_nudge(&key, task.id, member, reason.name(), now)?
        {
            return Ok(false);
        }
        self.wake_held_lock(member, Some(task.id), reason)?;
        Ok(true)
    }

    /// Work HQ cannot tell about gets one look from the lead of its area (ADR 0124): said in
    /// its thread, and the lead woken, once for what HQ saw of it.
    pub(super) fn look_once(&self, task: &Task, found: Diagnosis) -> Done<()> {
        let Some(lead) = self.area_lead(task)? else {
            return Ok(());
        };
        let saw = self.task_saw(task)?;
        let key = guards::nudge_key(task.id.0, lead.id.0, "look", &saw);
        if !self
            .store
            .record_nudge(&key, task.id, lead.id, "look", self.clock.now())?
        {
            return Ok(());
        }
        self.post_held_lock(
            Author::Endora,
            task.id,
            &format!(
                "HQ cannot tell whether this stopped by accident: {}. {}, look at it once and \
                 decide: carry on with it, hand it on, or drop it.",
                found.why, lead.name
            ),
        )?;
        self.wake_held_lock(lead.id, Some(task.id), WakeReason::Guard)?;
        Ok(())
    }

    // ---- findings -----------------------------------------------------------------------------

    /// A guard's finding, opened once a key (ADR 0124): kept, in the feed, and returned as its
    /// id; `None` while one with its key is open.
    pub(super) fn find(
        &self,
        guard: Guard,
        key: &str,
        task: Option<TaskId>,
        project: Option<i64>,
        evidence: &str,
    ) -> Done<Option<i64>> {
        let finding = Finding {
            id: 0,
            guard,
            key: key.to_owned(),
            task: task.map(|t| t.0),
            project,
            rule: guard.rule(),
            evidence: evidence.to_owned(),
            opened_at: self.clock.now(),
            cleared_at: None,
            verdict: None,
            fix_worked: None,
            judged_by: String::new(),
            desk: None,
        };
        let Some(id) = self.store.open_finding(&finding)? else {
            return Ok(None);
        };
        self.emit(
            EventKind::GuardFound,
            json!({ "finding": id, "guard": guard.name(), "task": task.map(|t| t.0),
                    "project": project, "evidence": evidence }),
        )?;
        Ok(Some(id))
    }

    /// A finding whose condition no longer holds clears, and so does its desk notice if the
    /// owner had not decided it.
    pub(super) fn clear(&self, finding: &Finding) -> Done<()> {
        self.store.clear_finding(finding.id, self.clock.now())?;
        self.emit(
            EventKind::GuardCleared,
            json!({ "finding": finding.id, "guard": finding.guard.name(), "task": finding.task }),
        )?;
        let Some(desk) = finding.desk else {
            return Ok(());
        };
        let waiting = self
            .store
            .desk()?
            .iter()
            .any(|d| d.id.0 == desk && d.status == "waiting");
        if waiting {
            self.close_desk_item(crate::domain::DeskId(desk), "guard")?;
        }
        Ok(())
    }

    fn close_desk_item(&self, item: crate::domain::DeskId, kind: &str) -> Done<()> {
        self.store.set_desk_status(item, "cleared")?;
        self.emit(
            EventKind::DeskDecided,
            json!({ "item": item.0, "kind": kind, "decision": "cleared" }),
        )
    }

    /// The owner or a lead gives a finding its verdict (ADR 0124), and says whether its fix
    /// worked if they know.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a finding that does not exist.
    pub(super) fn judge_held_lock(
        &self,
        id: i64,
        verdict: Verdict,
        fix_worked: Option<bool>,
        by: &str,
    ) -> Done<()> {
        if !self.store.findings()?.iter().any(|f| f.id == id) {
            return Err(EngineError::Unknown("finding"));
        }
        self.store
            .judge_finding(id, verdict, fix_worked, by, self.clock.now())?;
        self.emit(
            EventKind::GuardJudged,
            json!({ "finding": id, "verdict": verdict.name(), "fix_worked": fix_worked, "by": by }),
        )
    }

    /// The owner gives a finding its verdict, from the screen.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a finding that does not exist.
    pub fn judge_finding(&self, id: i64, verdict: Verdict, fix_worked: Option<bool>) -> Done<()> {
        let _held = self.held();
        self.judge_held_lock(id, verdict, fix_worked, "owner")
    }

    // ---- the tick's look ----------------------------------------------------------------------

    /// The guards' part of the tick: stuck items and findings whose condition cleared close
    /// themselves, every tick.
    pub(super) fn guards_tick(&self) -> Done<()> {
        for item in self.store.desk()? {
            if item.kind != "stuck" || item.status != "waiting" {
                continue;
            }
            let Some(task) = item.task else {
                continue;
            };
            if self.unstuck(task)? {
                self.close_desk_item(item.id, "stuck")?;
            }
        }
        for finding in self.store.findings()? {
            if finding.cleared_at.is_some() || finding.guard != Guard::Stuck {
                continue;
            }
            let Some(task) = finding.task else {
                continue;
            };
            if self.unstuck(TaskId(task))? {
                self.clear(&finding)?;
            }
        }
        Ok(())
    }

    /// Whether stuck work is moving again: finished, out of blocked, or with a turn on it.
    fn unstuck(&self, task: TaskId) -> Done<bool> {
        let Some(found) = self.store.task(task)? else {
            return Ok(true);
        };
        if found.status != TaskStatus::Blocked {
            return Ok(true);
        }
        Ok(self
            .diagnosis(task)?
            .is_none_or(|d| d.stopped == Stopped::Working))
    }

    // ---- why work closed -----------------------------------------------------------------------

    /// Records why `task` closed, and what carries it on (ADR 0124).
    pub(super) fn closed(
        &self,
        task: TaskId,
        closure: Closure,
        successor: Option<TaskId>,
        by: &str,
    ) -> Done<()> {
        if let Some(next) = successor {
            self.task(next)?;
        }
        self.store.record_closure(&Closed {
            task,
            closure,
            successor,
            by: by.to_owned(),
            at: self.clock.now(),
        })?;
        Ok(())
    }

    /// Work that just closed: it carries a reason (its own if it was given one, else what its
    /// path means: done on evidence met its criteria, a plain drop will not be done); and the
    /// open work that waited on a dropped task is told at once, and stays blocked.
    pub(super) fn after_closing(&self, task: TaskId) -> Done<()> {
        let Some(now) = self.store.task(task)? else {
            return Ok(());
        };
        if !now.status.finished() {
            return Ok(());
        }
        if self.store.closure(task)?.is_none() {
            let default = if now.status == TaskStatus::Done {
                Closure::Done(DoneWhy::MetCriteria)
            } else {
                Closure::Dropped(DropWhy::WontDo)
            };
            self.closed(task, default, None, "endora")?;
        }
        if now.status != TaskStatus::Dropped {
            return Ok(());
        }
        let Some(closed) = self.store.closure(task)? else {
            return Ok(());
        };
        for waiting in self.store.tasks()? {
            if waiting.status.finished() || !waiting.waits_on.contains(&task) {
                continue;
            }
            self.post_held_lock(
                Author::Endora,
                waiting.id,
                &closed.told_to_waiters(&now.title),
            )?;
            if matches!(waiting.status, TaskStatus::Todo | TaskStatus::Doing) {
                self.store
                    .set_task_status(waiting.id, TaskStatus::Blocked)?;
                self.emit_status(&waiting, TaskStatus::Blocked)?;
            }
            if let Some(holder) = waiting.owner.or(waiting.assignee) {
                self.wake_held_lock(holder, Some(waiting.id), WakeReason::WaitDropped)?;
            }
        }
        Ok(())
    }

    /// The owner drops a task, saying why from the set, and what carries it on if anything.
    ///
    /// # Errors
    /// [`crate::domain::closure::not_a_closure`] for a reason that is not a drop's;
    /// [`EngineError::Unknown`] for a task, or a successor, that does not exist.
    pub fn drop_task_because(
        &self,
        task: TaskId,
        closure: Closure,
        successor: Option<TaskId>,
    ) -> Done<()> {
        let _held = self.held();
        if closure.status() != TaskStatus::Dropped {
            return Err(EngineError::Refused(not_a_closure(TaskStatus::Dropped)));
        }
        let current = self.task(task)?;
        if let Some(next) = successor {
            self.task(next)?;
        }
        self.store.set_task_status(task, TaskStatus::Dropped)?;
        self.emit_status(&current, TaskStatus::Dropped)?;
        self.closed(task, closure, successor, "owner")?;
        self.after_finishing(&current)
    }

    /// A member that closed its task as done says it was overtaken or not needed, rather than
    /// that it met its criteria (ADR 0124): in the same turn, once it is done.
    ///
    /// # Errors
    /// [`super::Refusal::TurnOver`]; [`super::Refusal::NotYours`] for work the turn did not
    /// close; [`crate::domain::closure::not_a_closure`] for a reason that is not done's.
    pub fn done_because_in(
        &self,
        run: crate::domain::RunId,
        task: TaskId,
        closure: Closure,
        successor: Option<TaskId>,
    ) -> Done<()> {
        let _held = self.held();
        let turn = self.running(run)?;
        if closure.status() != TaskStatus::Done {
            return Err(EngineError::Refused(not_a_closure(TaskStatus::Done)));
        }
        let current = self.task(task)?;
        let mine = current.owner == Some(turn.member) || turn.task == Some(task);
        if current.status != TaskStatus::Done || !mine {
            return Err(EngineError::Refused(super::Refusal::NotYours));
        }
        let by = self.member(turn.member)?.name;
        self.closed(task, closure, successor, &by)
    }

    /// Stuck work, as a finding (ADR 0124): opened when the tick marks it stuck.
    pub(super) fn found_stuck(&self, task: &Task, why: &str) -> Done<()> {
        self.find(
            Guard::Stuck,
            &format!("stuck:{}", task.id.0),
            Some(task.id),
            task.project.map(|p| p.0),
            &format!("#{} {}: {why}", task.id.0, task.title),
        )?;
        Ok(())
    }
}
