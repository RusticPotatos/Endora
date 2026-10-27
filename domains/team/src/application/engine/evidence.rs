//! Evidence, acceptance criteria and the tester (ADR 0114).
//!
//! The node writes a receipt for every call a turn makes of Endora's tools, with the turn's
//! token; HQ keeps it only while that turn runs. Work submitted for checking cites evidence
//! per criterion, and goes member → tester → lead → done; every claim (a submission, the
//! tester's pass, the lead's acceptance, a routine's check closed, the CEO's decision on a
//! project, anything sent to the owner's desk or up to the CEO) is judged by one rule,
//! [`evidence::judge`], from what is in HQ's record. Each turn ends with a label worked out from
//! what it did; a member whose turns show nothing is nudged twice, then its task goes up.

use serde_json::json;

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Cited, Clock, NewTask, Run, TeamStore, WorkerAdapter};
use crate::domain::chain;
use crate::domain::evidence::{
    self, AfterNothing, CallReceipt, Criterion, Found, Judged, Label, Ref, TurnFacts,
};
use crate::domain::history::InputOrigin;
use crate::domain::record;
use crate::domain::review::SendBackReason;
use crate::domain::{
    EventKind, Member, MemberId, Millis, Refusal, RunId, RunStatus, Task, TaskId, TaskStatus,
    WakeReason,
};

/// What the node says of one call a turn made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Called {
    /// The tool.
    pub tool: String,
    /// Its arguments, in short.
    pub args: String,
    /// The start of what it gave back.
    pub excerpt: String,
    /// Whether it did what was asked.
    pub ok: bool,
    /// Whether the tool changes something.
    pub act: bool,
    /// Whether what it gave back is Confidential.
    pub confidential: bool,
}

/// The stages evidence is cited at.
pub(super) const SUBMITTED: &str = "submitted";
const TESTED: &str = "tested";
const CLOSED: &str = "closed";

/// What the thread says when work is sent back, by the tester or the lead: what counts the
/// rounds.
pub(super) const SENT_BACK: &str = "Sent back: ";

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- receipts ---------------------------------------------------------------------------

    /// The node records one call a turn made of Endora's tools: its receipt, kept for good,
    /// returned as its id. Only a turn that is running now, known by its token, has receipts:
    /// a forged token, or one of a turn that has ended, records nothing.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a token that opens no running turn; a store failure.
    pub fn record_receipt(&self, token: &str, called: &Called) -> Done<i64> {
        self.record(token, called, false)
    }

    /// The node records what a turn's sandbox reported its own tools did (ADR 0117): Claude
    /// Code's Bash, Edit and the rest, posted by its hook with the turn's token. Kept as a
    /// receipt marked sandbox-reported: it may be cited, but no live criterion rests on it.
    ///
    /// # Errors
    /// As [`Self::record_receipt`].
    pub fn record_sandbox_receipt(&self, token: &str, called: &Called) -> Done<i64> {
        self.record(token, called, true)
    }

    fn record(&self, token: &str, called: &Called, sandbox: bool) -> Done<i64> {
        let _held = self.held();
        let Some(id) = self.store.run_of_token(token)? else {
            return Err(EngineError::Unknown("running turn with that token"));
        };
        let run = self
            .running(id)
            .map_err(|_| EngineError::Unknown("running turn with that token"))?;
        let tool = evidence::short(&called.tool, 80);
        if tool.is_empty() {
            return Err(EngineError::Unknown("tool"));
        }
        let receipt = CallReceipt {
            id: 0,
            run: run.id,
            member: run.member,
            task: run.task,
            tool,
            args: evidence::short(&called.args, evidence::ARGS_MOST),
            excerpt: evidence::short(&called.excerpt, evidence::EXCERPT_MOST),
            ok: called.ok,
            act: called.act,
            confidential: called.confidential,
            sandbox,
            at: self.clock.now(),
        };
        let id = self.store.add_call_receipt(&receipt)?;
        self.emit(
            EventKind::ReceiptRecorded,
            json!({ "receipt": id, "run": run.id.0, "member": run.member.0,
                    "task": run.task.map(|t| t.0), "tool": receipt.tool, "ok": receipt.ok,
                    "act": receipt.act, "sandbox": sandbox }),
        )?;
        Ok(id)
    }

    // ---- what evidence is ---------------------------------------------------------------------

    /// The task and every task under it, at any depth: one piece of work.
    pub(super) fn work_of(&self, task: TaskId) -> Done<Vec<TaskId>> {
        let tasks = self.store.tasks()?;
        let mut work = vec![task];
        let mut i = 0;
        while i < work.len() {
            let at = work[i];
            work.extend(tasks.iter().filter(|t| t.parent == Some(at)).map(|t| t.id));
            i += 1;
        }
        Ok(work)
    }

    /// What a reference is in HQ's record, and whether it is about `work` (`None`: anything
    /// in the record counts, as for a decision).
    pub(super) fn found(&self, r: Ref, work: Option<&[TaskId]>, waits: &[TaskId]) -> Done<Found> {
        let about = |task: Option<TaskId>| match work {
            None => true,
            Some(work) => task.is_some_and(|t| work.contains(&t)),
        };
        Ok(match r {
            Ref::Receipt(id) => match self.store.call_receipt(id)? {
                None => Found::Missing,
                Some(found) => Found::Receipt {
                    run: found.run,
                    ok: found.ok,
                    in_work: about(found.task),
                    witnessed: !found.sandbox,
                },
            },
            Ref::Task(id) => match self.store.task(id)? {
                None => Found::Missing,
                Some(_) => Found::Other {
                    in_work: about(Some(id)) || waits.contains(&id),
                },
            },
            Ref::Event(id) => match self.store.event(id)? {
                None => Found::Missing,
                Some(event) => Found::Other {
                    in_work: about(event.data["task"].as_i64().map(TaskId)),
                },
            },
        })
    }

    /// Each list of references, looked up: about `task`'s work, or (`None`) about anything.
    pub(super) fn look_up(
        &self,
        task: Option<&Task>,
        cited: &[Vec<Ref>],
    ) -> Done<Vec<Vec<(Ref, Found)>>> {
        let work = match task {
            Some(task) => Some(self.work_of(task.id)?),
            None => None,
        };
        let waits = task.map(|t| t.waits_on.clone()).unwrap_or_default();
        if cited.iter().map(Vec::len).sum::<usize>() > evidence::MOST_REFS {
            return Err(EngineError::Refused(Refusal::TooLong(evidence::MOST_REFS)));
        }
        let mut all = Vec::new();
        for group in cited {
            let mut found = Vec::new();
            for r in group {
                found.push((*r, self.found(*r, work.as_deref(), &waits)?));
            }
            all.push(found);
        }
        Ok(all)
    }

    /// Judges a claim about `task`'s work by the one rule: per criterion, from the record.
    pub(super) fn judge_work(&self, task: &Task, cited: &[Vec<Ref>], how: &Judged<'_>) -> Done<()> {
        let found = self.look_up(Some(task), cited)?;
        evidence::judge(&task.criteria, &found, how).map_err(EngineError::Refused)
    }

    /// The work's last change, which a time-bound criterion waits from (ADR 0120): its latest
    /// act that succeeded (a restart's receipt) on the task or under it, or the latest alarm
    /// about the task, whichever is later; `None` when neither happened.
    fn last_change(&self, task: TaskId) -> Done<Option<Millis>> {
        let work = self.work_of(task)?;
        let mut latest: Option<Millis> = None;
        for run in self.store.runs()? {
            if !run.task.is_some_and(|t| work.contains(&t)) {
                continue;
            }
            for receipt in self.store.call_receipts_of(run.id)? {
                if receipt.ok && receipt.act && receipt.tool != "push" {
                    latest = latest.max(Some(receipt.at));
                }
            }
        }
        for event in self.store.events_after(0)? {
            let alarm = (event.kind == "routine.fired" && event.data["firing"] == "alarm")
                || (event.kind == "routine.posted" && event.data["recovered"] != true);
            if alarm && event.data["task"].as_i64() == Some(task.0) {
                latest = latest.max(Some(event.at));
            }
        }
        Ok(latest)
    }

    /// Whether `task`'s time-bound criteria have waited their time, now.
    pub(super) fn waited_for(&self, task: &Task) -> Done<()> {
        evidence::waited(
            &task.criteria,
            self.last_change(task.id)?,
            self.clock.now(),
            self.settings.offset_minutes,
        )
        .map_err(EngineError::Refused)
    }

    /// Judges evidence for a decision or a message up: at least one reference, each in HQ's
    /// record.
    pub(super) fn judge_decision(&self, cited: &[Ref], turn: RunId) -> Done<()> {
        let found = self.look_up(None, &[cited.to_vec()])?;
        evidence::judge(
            &[],
            &found,
            &Judged {
                fresh: &[turn],
                own_reading: None,
            },
        )
        .map_err(EngineError::Refused)
    }

    /// References in HQ's record, each looked up: a post's, which need only be there.
    pub(super) fn all_there(&self, cited: &[Ref]) -> Done<()> {
        for r in cited {
            if self.found(*r, None, &[])? == Found::Missing {
                return Err(EngineError::Refused(Refusal::NoSuchEvidence(r.said())));
            }
        }
        Ok(())
    }

    /// Evidence as the thread and the desk say it: the ids, and each receipt's tool, never
    /// its excerpt (the excerpts are shown where the reader's model is known).
    pub(super) fn evidence_said(&self, cited: &[Ref]) -> Done<String> {
        let mut said = Vec::new();
        for r in cited {
            let text = match r {
                Ref::Receipt(id) => match self.store.call_receipt(*id)? {
                    Some(found) => format!("{} ({})", r.said(), found.tool),
                    None => r.said(),
                },
                _ => r.said(),
            };
            said.push(text);
        }
        Ok(said.join(", "))
    }

    /// Records what a turn cited for each criterion, at a stage.
    pub(super) fn keep_cited(
        &self,
        task: TaskId,
        run: &Run,
        stage: &str,
        cited: &[Vec<Ref>],
    ) -> Done<()> {
        let now = self.clock.now();
        for (i, group) in cited.iter().enumerate() {
            if group.is_empty() {
                continue;
            }
            let criterion = if self.task(task)?.criteria.is_empty() {
                0
            } else {
                i + 1
            };
            self.store.add_cited(&Cited {
                task,
                run: run.id,
                member: run.member,
                stage: stage.to_owned(),
                criterion,
                refs: group.clone(),
                at: now,
            })?;
        }
        Ok(())
    }

    /// Where HQ keeps the hash of the criteria a tester's pass judged.
    fn passed_key(task: TaskId, run: RunId) -> String {
        format!("passed:{}:{}", task.0, run.0)
    }

    /// Whether the tester's pass in `run` judged criteria other than `task`'s now. A pass from
    /// before HQ kept the hash cannot tell, and stands as it did.
    fn needs_recheck(&self, task: TaskId, run: RunId) -> Done<bool> {
        let Some(then) = self.store.setting(&Self::passed_key(task, run))? else {
            return Ok(false);
        };
        Ok(then != evidence::criteria_hash(&self.task(task)?.criteria))
    }

    /// The owner rewrites a task's criteria (ADR 0124): a tester's pass on the old ones then
    /// reads "needs re-check" and does not count.
    ///
    /// # Errors
    /// [`Refusal::TooLong`] past [`evidence::MOST_CRITERIA`]; [`EngineError::Unknown`].
    pub fn set_criteria(&self, task: TaskId, criteria: &[Criterion]) -> Done<()> {
        let _held = self.held();
        self.set_criteria_held_lock(task, criteria)
    }

    /// [`Self::set_criteria`], with the engine's lock already held.
    fn set_criteria_held_lock(&self, task: TaskId, criteria: &[Criterion]) -> Done<()> {
        self.task(task)?;
        evidence::may_hand_on(criteria, false).map_err(EngineError::Refused)?;
        self.store.set_criteria(task, criteria)?;
        Ok(())
    }

    /// How many times a task's work was sent back, by the tester or the lead.
    pub(super) fn sent_back(&self, task: TaskId) -> Done<usize> {
        let members = self.store.members()?;
        Ok(self
            .store
            .thread(task)?
            .iter()
            .filter(|p| p.text.starts_with(SENT_BACK) && members.iter().any(|m| m.name == p.author))
            .count())
    }

    // ---- who checks ---------------------------------------------------------------------------

    // ---- what a turn does with evidence -----------------------------------------------------

    /// A turn moves its task on (see [`Engine::set_status_in`]), with evidence: submitting work
    /// for checking (`in_review`) cites it per criterion, as closing a routine's check that has
    /// criteria does; a live criterion needs a reading from this turn.
    ///
    /// # Errors
    /// What [`evidence::judge`] refuses, and what [`Engine::set_status_in`] does.
    pub fn set_status_citing(
        &self,
        run: RunId,
        task: TaskId,
        to: TaskStatus,
        reason: &str,
        cited: &[Vec<Ref>],
    ) -> Done<()> {
        let _held = self.held();
        let turn = self.running(run)?;
        self.reach(&turn, task, false)?;
        let current = self.task(task)?;
        let check = to == TaskStatus::Done && self.is_a_check(task)?;
        let judged = to == TaskStatus::InReview || (check && !current.criteria.is_empty());
        let holds = current.owner == Some(turn.member);
        if judged && holds {
            self.judge_work(
                &current,
                cited,
                &Judged {
                    fresh: &[turn.id],
                    own_reading: None,
                },
            )?;
        }
        if check && holds {
            self.waited_for(&current)?;
        }
        self.set_status_held_lock(turn.member, task, to, reason)?;
        if judged {
            let stage = if check { CLOSED } else { SUBMITTED };
            self.keep_cited(task, &turn, stage, cited)?;
            let said = self.evidence_said(&cited.concat())?;
            if !said.is_empty() {
                self.post_system_held_lock(task, &format!("Evidence cited: {said}"))?;
            }
        }
        Ok(())
    }

    /// A turn hands work on (see [`Engine::add_task`]): work handed to another member says what
    /// done means, and a sub-task of a task with criteria, the owner's and a routine's too, which
    /// of them it serves (`serves`, ADR 0124, amended); no task holds more than five open
    /// sub-tasks, nor work go deeper than three. Creating it is a change.
    ///
    /// # Errors
    /// [`Refusal::NoCriteria`], [`Refusal::TooLong`], what [`Engine::add_task`] refuses, and
    /// the sub-task discipline's refusals (with their counts).
    pub fn create_task_in(
        &self,
        run: RunId,
        task: NewTask,
        criteria: &[Criterion],
        serves: Option<&str>,
    ) -> Done<TaskId> {
        let _held = self.held();
        let turn = self.running(run)?;
        // Who may hand what to whom comes first: a member who may not make the task at all is
        // told so, not asked what it serves.
        let to_another = task.assignee.filter(|a| *a != turn.member);
        if let Some(to) = to_another {
            let (from, to) = (self.member(turn.member)?, self.member(to)?);
            crate::domain::work::may_delegate(from.role, to.role).map_err(EngineError::Refused)?;
        }
        let served = match task.parent {
            Some(parent) => self.subtask_discipline(parent, serves)?,
            None => None,
        };
        evidence::may_hand_on(criteria, to_another.is_some()).map_err(EngineError::Refused)?;
        let id = self.add_task_authored_held_lock(
            Author::Member(turn.member),
            task,
            InputOrigin::Run(run),
        )?;
        self.store
            .set_criteria_from(id, criteria, InputOrigin::Run(run))?;
        if let Some(n) = served {
            self.store
                .set_setting(&super::discipline::serves_key(id), &n.to_string())?;
        }
        if let Some(own) = turn.task {
            self.mark_progress(turn.member, own)?;
        }
        Ok(id)
    }

    /// The owner hands the team work, saying what done means: at least one line (ADR 0124,
    /// amended), since a team handed "fix the backups" has nothing to check a part against. This
    /// is the owner's front door, the one Give work and the API use; [`Engine::add_task`] is the
    /// primitive under it, for the tasks HQ makes itself and for work already open, which stays
    /// as it is.
    ///
    /// # Errors
    /// [`Refusal::OwnerNeedsCriteria`] when the owner gives none, [`Refusal::TooLong`] past
    /// [`evidence::MOST_CRITERIA`]; and as [`Engine::add_task`].
    pub fn add_task_with(&self, by: Author, task: NewTask, criteria: &[Criterion]) -> Done<TaskId> {
        let _held = self.held();
        let _asked = (by == Author::Owner).then(|| self.for_owner());
        let may = match by {
            Author::Owner => evidence::owner_may_hand_out(criteria),
            _ => evidence::may_hand_on(criteria, false),
        };
        may.map_err(EngineError::Refused)?;
        let origin = if by == Author::Owner {
            InputOrigin::Owner
        } else {
            InputOrigin::Unknown
        };
        let id = self.add_task_authored_held_lock(by, task, origin)?;
        self.store.set_criteria_from(id, criteria, origin)?;
        Ok(id)
    }

    /// The tester's verdict on work in review (ADR 0114), in her turn. She re-checks it
    /// herself: a pass cites, per criterion, evidence that holds, with at least one reading of
    /// her own this turn, and goes to the work's lead to accept (or closes it, when no other
    /// lead is there). Sending it back is a finished test too, and keeps the review it had
    /// ([`Engine::review_in`]), a draft for the owner included. A draft is never passed: it is
    /// forwarded with `ask_owner`, or sent back. A CEO's draft puts the work task itself in
    /// review, and a pass through the draft's path closed it as done, with the gate true, and
    /// offered its push, with no evidence judged and no lead's acceptance.
    ///
    /// A send-back may name its `reason` when it is one HQ counts (ADR 0132).
    ///
    /// # Errors
    /// What [`Engine::review_in`] refuses, [`Refusal::ForwardTheDraft`], and what
    /// [`evidence::judge`] refuses for a pass.
    pub fn test_in(
        &self,
        run: RunId,
        task: TaskId,
        pass: bool,
        notes: &str,
        cited: &[Vec<Ref>],
        reason: Option<SendBackReason>,
    ) -> Done<()> {
        let current = self.task(task)?;
        if !pass {
            return self.review_in(run, task, false, notes, reason);
        }
        let _held = self.held();
        let turn = self.running(run)?;
        self.reach(&turn, task, true)?;
        let tester = self.member(turn.member)?;
        // In the loops the member one rank up passes it (ADR 0142); otherwise, the tester.
        let one_rank_up = self
            .loop_reviewer_of(&current)?
            .is_some_and(|r| r.id == tester.id);
        if !one_rank_up {
            return Err(EngineError::Refused(Refusal::NotYours));
        }
        if current.status != TaskStatus::InReview {
            return Err(EngineError::Refused(Refusal::NotAStep {
                from: current.status,
                to: TaskStatus::Done,
            }));
        }
        if self.is_draft(&current)? {
            return Err(EngineError::Refused(Refusal::ForwardTheDraft));
        }
        let worker = current.owner.or(current.assignee);
        if worker == Some(tester.id) {
            return Err(EngineError::Refused(Refusal::OwnWork));
        }
        self.judge_work(
            &current,
            cited,
            &Judged {
                fresh: &[turn.id],
                own_reading: Some(turn.id),
            },
        )?;
        self.waited_for(&current)?;
        self.keep_cited(task, &turn, TESTED, cited)?;
        // The pass is bound to the criteria it judged (ADR 0124).
        self.store.set_setting(
            &Self::passed_key(task, turn.id),
            &evidence::criteria_hash(&current.criteria),
        )?;
        let said = self.evidence_said(&cited.concat())?;
        self.post_own_held_lock(
            tester.id,
            task,
            &format!("Tested and passed: {notes}\nEvidence: {said}"),
            &[],
        )?;
        self.mark_progress(tester.id, task)?;
        self.close_checked(&current, tester.id)
    }

    /// Closes checked work as done: gated work through the gate's rule, the rest at once.
    fn close_checked(&self, task: &Task, by: MemberId) -> Done<()> {
        let gated = crate::domain::review::needs_gate(
            self.routine_of(task.id)?.is_some(),
            self.the_home_labs(task)?,
        );
        if gated {
            return self.approve_held_lock(by, task.id);
        }
        self.store.set_task_status(task.id, TaskStatus::Done)?;
        self.emit_status(task, TaskStatus::Done)?;
        self.after_finishing(task)
    }

    /// A turn writes in a thread, citing evidence if it has any (see [`Engine::post_in`]). A
    /// member's post that cites nothing, in a turn with no receipt, is marked unsupported
    /// for the tester and the lead to see: allowed, never taken for a finding.
    ///
    /// # Errors
    /// [`Refusal::NoSuchEvidence`] for a reference not in the record, and what
    /// [`Engine::post_in`] refuses.
    pub fn post_citing(&self, run: RunId, task: TaskId, text: &str, cited: &[Ref]) -> Done<()> {
        {
            let _held = self.held();
            self.all_there(cited)?;
        }
        self.post_in_marked(run, task, text, true, cited)
    }

    /// Whether a turn has any receipt in HQ's record, held or refused: a post in such a turn
    /// rests on something the node wrote, so it is never unsupported (ADR 0114, amended).
    pub(super) fn has_receipts(&self, run: RunId) -> Done<bool> {
        Ok(!self.store.call_receipts_of(run)?.is_empty())
    }

    /// Whether what waits in a task's review is a draft for the owner, not work.
    pub(super) fn is_draft(&self, task: &Task) -> Done<bool> {
        let newest = self.store.thread(task.id)?.into_iter().rev().find(|p| {
            p.text.starts_with(crate::domain::review::DRAFT) || p.text.contains(" → in_review")
        });
        Ok(newest.is_some_and(|p| p.text.starts_with(crate::domain::review::DRAFT)))
    }

    // ---- what a turn came to ------------------------------------------------------------------

    /// A turn's label, from its record: what it changed, the receipts it has, whether it
    /// posted or was blocked.
    pub(super) fn label_of(&self, run: &Run) -> Done<Label> {
        let receipts = self.store.call_receipts_of(run.id)?;
        Ok(evidence::label(TurnFacts {
            changed: run.moved_forward,
            acted: receipts.iter().any(|r| r.ok && r.act),
            readings: receipts.iter().filter(|r| r.ok).count(),
            posted: run.posted,
            blocked: run.blocked,
        }))
    }

    /// After a member's turn on its task that showed nothing: a nudge for its next turn (the
    /// briefing says it) for the first two, then, once, the same member a model tier up (ADR
    /// 0139). When its work already thinks with Claude or the task has escalated twice today,
    /// a task in a lead's area is the ladder's; any other goes to whom the member reports to,
    /// with a post saying so.
    pub(super) fn after_nothing_shown(&self, run: &Run) -> Done<()> {
        let Some(task_id) = run.task else {
            return Ok(());
        };
        let task = self.task(task_id)?;
        let mine = task.owner.or(task.assignee) == Some(run.member);
        if !mine || task.status.finished() || task.status == TaskStatus::InReview {
            return Ok(());
        }
        let shown = self.shown_nothing(run.member, task_id)?;
        if shown
            .last()
            .is_none_or(|(id, nothing)| *id != run.id || !nothing)
        {
            return Ok(());
        }
        let count = shown.iter().filter(|(_, nothing)| *nothing).count();
        if evidence::after_nothing(count) != AfterNothing::HandUp {
            return Ok(());
        }
        let key = format!("handed_up:{}:{}", task_id.0, run.member.0);
        if self.store.setting(&key)?.is_some() {
            return Ok(());
        }
        let member = self.member(run.member)?;
        // A model tier up before a person (ADR 0139): the same member, on its Claude.
        if self.on_work_turn(run, &member)? && self.take_a_tier_up(task_id)? {
            self.store.set_setting(&key, "1")?;
            self.post_held_lock(
                Author::Endora,
                task_id,
                &format!(
                    "{} took {count} turns on this that showed nothing (talk only or empty), \
                     after {} nudges: {} takes it a model tier up, on its Claude.",
                    member.name,
                    evidence::MOST_NUDGES,
                    member.name
                ),
            )?;
            self.wake_held_lock(member.id, Some(task_id), WakeReason::Escalated)?;
            return Ok(());
        }
        let members = self.store.members()?;
        let Some(manager) = chain::manager(&member, &members)
            .and_then(|id| members.iter().find(|m| m.id == id).cloned())
            .filter(|m| m.id != member.id)
        else {
            return Ok(());
        };
        self.store.set_setting(&key, "1")?;
        self.post_held_lock(
            Author::Endora,
            task_id,
            &format!(
                "{} took {count} turns on this that showed nothing (talk only or empty), after \
                 {} nudges: it goes to {}, whom {} reports to.",
                member.name,
                evidence::MOST_NUDGES,
                manager.name,
                member.name
            ),
        )?;
        self.store.release(task_id)?;
        self.store.set_assignee(task_id, Some(manager.id))?;
        if task.status != TaskStatus::Todo {
            self.store.set_task_status(task_id, TaskStatus::Todo)?;
            self.emit_status(&task, TaskStatus::Todo)?;
        }
        self.emit(
            EventKind::TaskReleased,
            json!({ "task": task_id.0, "member": member.id.0, "to": manager.id.0,
                    "why": "showed nothing" }),
        )?;
        self.wake_held_lock(manager.id, Some(task_id), WakeReason::Assigned)?;
        Ok(())
    }

    /// A member's ended turns on a task, oldest first, each with whether it showed nothing:
    /// talk only or empty by its label, or readings that its turn before had already taken
    /// (the same tools with the same arguments, as their arguments read once spaces and an
    /// empty `{}` are put aside), which changed nothing (ADR 0114: live, a member read the
    /// lab's state fourteen turns in a row to no effect; and on #667 a lead re-read only the
    /// status its turn before had read, four times). A first reading always shows something.
    pub(super) fn shown_nothing(&self, member: MemberId, task: TaskId) -> Done<Vec<(RunId, bool)>> {
        // A huddle's short turn is the member's view, not its work on the task (ADR 0120).
        let huddles: Vec<crate::domain::WakeId> = self
            .store
            .wakes()?
            .into_iter()
            .filter(|w| w.reason == WakeReason::Huddle)
            .map(|w| w.id)
            .collect();
        let mut runs: Vec<Run> = self
            .store
            .runs()?
            .into_iter()
            .filter(|r| r.member == member && r.task == Some(task) && r.label.is_some())
            .filter(|r| !huddles.contains(&r.wake))
            .collect();
        runs.sort_by_key(|r| r.id);
        let mut shown = Vec::new();
        let mut before: Option<Vec<(String, String)>> = None;
        for run in runs {
            let mut read: Vec<(String, String)> = self
                .store
                .call_receipts_of(run.id)?
                .into_iter()
                .filter(|r| r.ok)
                .map(|r| (r.tool, evidence::same_args(&r.args)))
                .collect();
            read.sort();
            read.dedup();
            let nothing = match run.label {
                Some(Label::Checked) => before
                    .as_ref()
                    .is_some_and(|b| !read.is_empty() && read.iter().all(|r| b.contains(r))),
                Some(label) => !label.productive(),
                None => false,
            };
            shown.push((run.id, nothing));
            before = Some(read);
        }
        Ok(shown)
    }

    /// The nudge for a member's turn on a task, when its last turn on it showed nothing and it
    /// has had fewer than its nudges.
    pub(super) fn nudge_for(&self, member: MemberId, run: &Run) -> Done<Option<&'static str>> {
        let Some(task) = run.task else {
            return Ok(None);
        };
        // A check-back is the member's own plan to look again: no nudge for keeping it.
        if self.reason_for(run)? == WakeReason::CheckBack.name() {
            return Ok(None);
        }
        let shown: Vec<(RunId, bool)> = self
            .shown_nothing(member, task)?
            .into_iter()
            .filter(|(id, _)| *id != run.id)
            .collect();
        if !shown.last().is_some_and(|(_, nothing)| *nothing) {
            return Ok(None);
        }
        let count = shown.iter().filter(|(_, nothing)| *nothing).count();
        Ok((count <= evidence::MOST_NUDGES).then_some(evidence::NUDGE))
    }

    // ---- what members are told ----------------------------------------------------------------

    /// A task's criteria for a briefing; empty when it has none.
    pub(super) fn criteria_said(&self, task: &Task) -> String {
        if task.criteria.is_empty() {
            return String::new();
        }
        let mut told = String::from("What done means (cite evidence for each, in order):\n");
        for (i, c) in task.criteria.iter().enumerate() {
            let live = if c.live {
                " [live: a reading from this turn]"
            } else {
                ""
            };
            let after = c
                .after
                .map(|m| format!(" [shown only {m} minutes after the work's last change]"))
                .unwrap_or_default();
            told.push_str(&format!("{}. {}{live}{after}\n", i + 1, c.text));
        }
        told
    }

    /// What was cited on a task at `stage`'s latest, per criterion, each receipt with its
    /// excerpt, whatever the reader thinks on (ADR 0141).
    pub(super) fn cited_said(&self, task: &Task, stage: &str, cloud: bool) -> Done<String> {
        let cited = self.store.cited(task.id)?;
        let Some(last) = cited.iter().rev().find(|c| c.stage == stage) else {
            return Ok(String::new());
        };
        let mut told = String::new();
        for c in cited
            .iter()
            .filter(|c| c.stage == stage && c.run == last.run)
        {
            let about = match c.criterion {
                0 => "The work".to_owned(),
                n => format!(
                    "{n}. {}",
                    task.criteria.get(n - 1).map_or("", |k| k.text.as_str())
                ),
            };
            told.push_str(&format!("{about}:\n"));
            for r in &c.refs {
                told.push_str(&format!("  - {}\n", self.ref_line(*r, cloud)?));
            }
        }
        Ok(told)
    }

    /// One reference as a reader is told it.
    pub(super) fn ref_line(&self, r: Ref, cloud: bool) -> Done<String> {
        if let Ref::Receipt(id) = r {
            if let Some(found) = self.store.call_receipt(id)? {
                return Ok(found.line(cloud));
            }
        }
        Ok(r.said())
    }

    /// Who a briefing's turn on a task is, for the checking path: what the tester is told on
    /// work submitted, and the lead on work to accept.
    pub(super) fn checking_said(&self, member: &Member, run: &Run, task: &Task) -> Done<String> {
        let reason = self.reason_for(run)?;
        let cloud = member.brain.cloud();
        if reason == WakeReason::AcceptanceAsked.name() {
            let tested = self.cited_said(task, TESTED, cloud)?;
            let mut told = String::new();
            if tested.is_empty() {
                told.push_str("Nobody else tests this work: you accept it alone.\n");
            } else {
                told.push_str(&format!("The tester passed it on this evidence:\n{tested}"));
            }
            let submitted = self.cited_said(task, SUBMITTED, cloud)?;
            if !submitted.is_empty() {
                told.push_str(&format!("Its member submitted it on:\n{submitted}"));
            }
            told.push_str(
                "Accept it against its criteria with accept (verdict accept, evidence one list \
                 per criterion: receipts, the tester's among them, count as fresh), or send it \
                 back with exactly what does not hold.\n",
            );
            return Ok(told);
        }
        if reason == WakeReason::ReviewRequested.name() && !self.is_draft(task)? {
            let submitted = self.cited_said(task, SUBMITTED, cloud)?;
            if submitted.is_empty() {
                return Ok(String::new());
            }
            return Ok(format!("It was submitted on this evidence:\n{submitted}"));
        }
        Ok(String::new())
    }

    /// A post of a thread as another reads it, in a briefing, in `my_turn` or on a card: under its
    /// author's name, unsupported ones marked. What a member wrote is cleaned of what could turn
    /// text around or hide in it, of a line break that could forge a line, and of an `@` that
    /// could wake someone ([`record::clean`]); what Endora and the owner wrote is as it was
    /// written. The post itself is kept as written (ADR 0129).
    pub(crate) fn post_line(
        members: &[Member],
        author: &str,
        text: &str,
        unsupported: bool,
    ) -> String {
        let text = if members.iter().any(|m| m.name == author) {
            record::clean(text)
        } else {
            text.to_owned()
        };
        if unsupported {
            return format!("{author} [unsupported]: {text}");
        }
        format!("{author}: {text}")
    }

    /// The runs that ended last, newest first, with their labels: the screen's run lines.
    pub(super) fn recent_runs(&self, most: usize) -> Done<serde_json::Value> {
        let mut ended: Vec<Run> = self
            .store
            .runs()?
            .into_iter()
            .filter(|r| r.ended_at.is_some() && r.started_at.is_some())
            .collect();
        ended.sort_by_key(|r| core::cmp::Reverse(r.id));
        Ok(serde_json::Value::Array(
            ended
                .iter()
                .take(most)
                .map(|r| {
                    json!({ "id": r.id.0, "member": r.member.0, "task": r.task.map(|t| t.0),
                            "status": r.status.name(), "label": r.label.map(Label::name),
                            "ended_at": r.ended_at })
                })
                .collect(),
        ))
    }

    /// What a task's evidence is, for the screen: its criteria, and each stage's citations
    /// with every receipt's tool, time, excerpt and whether it held (the owner's screen shows
    /// Confidential excerpts: the owner sees the house).
    pub(super) fn evidence_shown(&self, task: &Task) -> Done<serde_json::Value> {
        let mut cited = Vec::new();
        for c in self.store.cited(task.id)? {
            let mut refs = Vec::new();
            for r in &c.refs {
                let mut shown = json!({ "ref": r.said() });
                if let Ref::Receipt(id) = r {
                    if let Some(found) = self.store.call_receipt(*id)? {
                        shown["tool"] = json!(found.tool);
                        shown["at"] = json!(found.at);
                        shown["excerpt"] = json!(found.excerpt);
                        shown["ok"] = json!(found.ok);
                    }
                }
                refs.push(shown);
            }
            // A pass on criteria that changed since reads "needs re-check" (ADR 0124).
            let recheck = c.stage == TESTED && self.needs_recheck(task.id, c.run)?;
            cited.push(
                json!({ "stage": c.stage, "member": c.member.0, "criterion": c.criterion,
                               "at": c.at, "refs": refs, "needs_recheck": recheck }),
            );
        }
        Ok(json!({
            "criteria": task.criteria.iter()
                .map(|c| json!({ "text": c.text, "live": c.live, "after": c.after }))
                .collect::<Vec<_>>(),
            "cited": cited,
        }))
    }

    /// A turn that ended: its facts settled into its label, kept and returned.
    pub(super) fn settle_label(&self, run: &mut Run) -> Done<Option<Label>> {
        if run.started_at.is_none() {
            return Ok(None);
        }
        if let Some(stored) = self.store.run(run.id)? {
            run.moved_forward |= stored.moved_forward;
            run.posted |= stored.posted;
            run.blocked |= stored.blocked;
        }
        let label = self.label_of(run)?;
        // Re-checking evidence already enough is talk only (ADR 0124).
        let label = self.after_enough_label(run, label)?;
        run.label = Some(label);
        Ok(Some(label))
    }

    /// Marks a turn of `member` on `task` that is running.
    pub(super) fn mark_turn(
        &self,
        member: MemberId,
        task: TaskId,
        mark: &dyn Fn(&mut Run),
    ) -> Done<()> {
        for mut run in self.store.runs()? {
            if run.member == member && run.task == Some(task) && run.status == RunStatus::Running {
                mark(&mut run);
                self.store.save_run(&run)?;
            }
        }
        Ok(())
    }
}
