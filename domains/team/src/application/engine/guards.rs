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
    self, Diagnosis, Finding, Guard, Raised, Stopped, THRESHOLDS, TaskFacts, Verdict,
};
use crate::domain::turns;
use crate::domain::{
    CheckBackStatus, EventKind, Label, MemberId, RunStatus, Task, TaskId, TaskStatus, WakeReason,
};

/// The wakes that are HQ's own recovery, left out of what a nudge "saw": a nudge must not make
/// itself new.
const OWN_WAKES: [WakeReason; 3] = [
    WakeReason::Recovery,
    WakeReason::AssignmentRecovery,
    WakeReason::Guard,
];

/// What a guard's detection came to (ADR 0132): the finding it concerned, and what was done to
/// it.
pub(super) struct Found {
    /// The finding.
    pub(super) id: i64,
    /// What the detection did to it.
    pub(super) how: Raised,
}

/// The key of a task's stuck finding: one is kept for the task.
pub(super) fn stuck_key(task: TaskId) -> String {
    format!("stuck:{}", task.0)
}

/// The desk notice a finding puts up: the guard, the finding and what it fired on, the rule, and
/// what the owner is asked.
pub(super) fn guard_card(guard: Guard, finding: i64, evidence: &str) -> String {
    format!(
        "{} (finding {finding}): {evidence}\nThe rule: {}. Say whether it was real.",
        guard.plainly(),
        guard.rule()
    )
}

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
        // A turn that waits for budget or for the Mac is queued and live, but it is a wait, not
        // a turn under way (ADR 0131): the work it is on is held, and starts when it can.
        let waits =
            |r: &&Run| r.status == RunStatus::Queued && turns::is_waiting_words(&r.end_reason);
        Ok(TaskFacts {
            status: task.status,
            holder_at_work,
            live_turn: on_it.iter().any(|r| r.status.live() && !waits(r)),
            waiting_turn: on_it.iter().any(waits),
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
    ///
    /// # Errors
    /// A store failure.
    pub fn diagnosis(&self, task: TaskId) -> Done<Option<Diagnosis>> {
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

    /// A guard's detection, weighed against the finding for its key (ADR 0132): one finding for a
    /// guard and what it is about, however often it sees it. `mark` is the evidence the detection
    /// rests on, a count that only grows with new evidence (see [`Guard::step`]). The finding is
    /// opened, opened again, renewed, refreshed or left alone as [`guards::reconcile`] says, and
    /// what it did, with the finding's id, is returned: only news (`Open`, `Renew`, `Reopen`) is
    /// for the callers to tell anyone of.
    pub(super) fn find(
        &self,
        guard: Guard,
        key: &str,
        task: Option<TaskId>,
        project: Option<i64>,
        evidence: &str,
        mark: i64,
    ) -> Done<Found> {
        let latest = self.store.latest_finding(key)?;
        let how = guards::reconcile(latest.as_ref(), mark, guard.step());
        let Some(latest) = latest else {
            let id = self.open(guard, key, task, project, evidence, mark)?;
            return Ok(Found { id, how });
        };
        match how {
            Raised::Hold => {}
            Raised::Refresh => self.see_again(&latest, evidence, mark)?,
            Raised::Reopen => {
                self.store
                    .reopen_finding(latest.id, evidence, self.clock.now(), mark)?;
                self.told_found(guard, latest.id, task, project, evidence, "reopened")?;
            }
            Raised::Renew | Raised::Open => {
                // A judged finding that is still open closes before its successor opens.
                if latest.cleared_at.is_none() {
                    self.clear(&latest)?;
                }
                let id = self.open(guard, key, task, project, evidence, mark)?;
                return Ok(Found { id, how });
            }
        }
        Ok(Found { id: latest.id, how })
    }

    /// A new finding for a key: kept, and told in the feed.
    fn open(
        &self,
        guard: Guard,
        key: &str,
        task: Option<TaskId>,
        project: Option<i64>,
        evidence: &str,
        mark: i64,
    ) -> Done<i64> {
        let now = self.clock.now();
        let finding = Finding {
            id: 0,
            guard,
            key: key.to_owned(),
            task: task.map(|t| t.0),
            project,
            rule: guard.rule(),
            evidence: evidence.to_owned(),
            opened_at: now,
            cleared_at: None,
            verdict: None,
            fix_worked: None,
            judged_by: String::new(),
            desk: None,
            seen_at: now,
            episodes: 1,
            mark,
        };
        let id = self
            .store
            .open_finding(&finding)?
            .ok_or(EngineError::Unknown("a finding to open"))?;
        self.told_found(guard, id, task, project, evidence, "opened")?;
        Ok(id)
    }

    /// Says in the feed that a finding opened (or opened again).
    fn told_found(
        &self,
        guard: Guard,
        id: i64,
        task: Option<TaskId>,
        project: Option<i64>,
        evidence: &str,
        how: &str,
    ) -> Done<()> {
        self.emit(
            EventKind::GuardFound,
            json!({ "finding": id, "guard": guard.name(), "task": task.map(|t| t.0),
                    "project": project, "evidence": evidence, "how": how }),
        )
    }

    /// An open finding seen again (ADR 0132): what it fired on and when are refreshed, no oftener
    /// than HQ looks unless the evidence changed, and the desk notice it put up, if it still
    /// waits, says the new words. A finding nobody has judged rests on the most it has seen; one
    /// that was judged keeps resting on what was judged, so that evidence that has grown since is
    /// told from evidence that has not.
    fn see_again(&self, open: &Finding, evidence: &str, mark: i64) -> Done<()> {
        let now = self.clock.now();
        let changed = open.evidence != evidence;
        if !changed && now - open.seen_at < THRESHOLDS.look_every_ms {
            return Ok(());
        }
        let mark = open.verdict.is_none().then_some(mark);
        self.store.refresh_finding(open.id, evidence, now, mark)?;
        // A stuck finding's card is the stuck work's own, in its own words.
        if changed && open.guard != Guard::Stuck {
            if let Some(desk) = open.desk {
                self.store.set_desk_text(
                    crate::domain::DeskId(desk),
                    &guard_card(open.guard, open.id, evidence),
                )?;
            }
        }
        Ok(())
    }

    /// A finding whose condition no longer holds clears, and so does its desk notice if the
    /// owner had not decided it.
    pub(super) fn clear(&self, finding: &Finding) -> Done<()> {
        self.store.clear_finding(finding.id, self.clock.now())?;
        self.emit(
            EventKind::GuardCleared,
            json!({ "finding": finding.id, "guard": finding.guard.name(), "task": finding.task }),
        )?;
        self.settle_card_of(finding, "cleared")
    }

    /// Puts away the desk notice a finding put up, if it still waits, as `decision`: cleared by
    /// HQ when the condition went, or the owner's own verdict (ADR 0132).
    fn settle_card_of(&self, finding: &Finding, decision: &str) -> Done<()> {
        let Some(desk) = finding.desk else {
            return Ok(());
        };
        let Some(card) = self
            .store
            .desk()?
            .into_iter()
            .find(|d| d.id.0 == desk && d.status == "waiting")
        else {
            return Ok(());
        };
        self.settle_desk_item(card.id, &card.kind, decision)
    }

    /// Puts a desk item away, saying how in the feed.
    pub(super) fn settle_desk_item(
        &self,
        item: crate::domain::DeskId,
        kind: &str,
        decision: &str,
    ) -> Done<()> {
        self.store.set_desk_status(item, decision)?;
        self.emit(
            EventKind::DeskDecided,
            json!({ "item": item.0, "kind": kind, "decision": decision }),
        )
    }

    fn close_desk_item(&self, item: crate::domain::DeskId, kind: &str) -> Done<()> {
        self.settle_desk_item(item, kind, "cleared")
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

    /// The owner gives a finding its verdict, from the screen. The owner has acted on it, so the
    /// desk notice it put up, if it still waits, is put away as the verdict (ADR 0132); a lead's
    /// verdict is not the owner's and leaves the notice for them.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a finding that does not exist.
    pub fn judge_finding(&self, id: i64, verdict: Verdict, fix_worked: Option<bool>) -> Done<()> {
        let _held = self.held();
        self.judge_held_lock(id, verdict, fix_worked, "owner")?;
        let Some(finding) = self.store.findings()?.into_iter().find(|f| f.id == id) else {
            return Ok(());
        };
        self.settle_card_of(&finding, verdict.name())
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

    /// Whether stuck work is moving again: finished, out of blocked, or with a turn on it (one
    /// that waits for budget or the Mac is one: it starts when it can, ADR 0131).
    fn unstuck(&self, task: TaskId) -> Done<bool> {
        let Some(found) = self.store.task(task)? else {
            return Ok(true);
        };
        if found.status != TaskStatus::Blocked {
            return Ok(true);
        }
        let facts = self.task_facts(&found)?;
        Ok(facts.live_turn
            || facts.waiting_turn
            || guards::diagnose(&facts).is_none_or(|d| d.stopped == Stopped::Working))
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

    /// Stuck work, as a finding (ADR 0124): opened when the tick marks it stuck, refreshed each
    /// time it is seen so, and raised again only when it has moved since (ADR 0132). What it
    /// rests on is how many of the task's turns advanced it: work that stops again with no turn
    /// having moved it is the same stuck work.
    pub(super) fn found_stuck(&self, task: &Task, why: &str) -> Done<Found> {
        let advanced = self
            .store
            .runs()?
            .iter()
            .filter(|r| r.task == Some(task.id) && r.label == Some(Label::Advanced))
            .count();
        self.find(
            Guard::Stuck,
            &stuck_key(task.id),
            Some(task.id),
            task.project.map(|p| p.0),
            &format!("#{} {}: {why}", task.id.0, task.title),
            i64::try_from(advanced).unwrap_or(i64::MAX),
        )
    }
}
