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

use super::{Author, Done, Engine, EngineError, one_line};
use crate::application::ports::{Cited, Clock, NewTask, Run, TeamStore, WorkerAdapter};
use crate::domain::chain;
use crate::domain::evidence::{
    self, AfterNothing, CallReceipt, Checkers, Criterion, Found, Judged, Label, Ref, TurnFacts,
};
use crate::domain::review::SendBackReason;
use crate::domain::staffing;
use crate::domain::{
    EventKind, Member, MemberId, Millis, Refusal, Role, RunId, RunStatus, Task, TaskId, TaskStatus,
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
const ACCEPTED: &str = "accepted";
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
        // A refused act may be the team pushing at the fence (ADR 0124).
        if receipt.act && !receipt.ok {
            self.after_refused_act(run.id, &receipt.tool)?;
        }
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

    /// The turns of the tester's latest pass on a task since it was last submitted, if she
    /// passed it.
    fn tested_since_submitted(&self, task: TaskId) -> Done<Option<Vec<RunId>>> {
        let cited = self.store.cited(task)?;
        let last = cited
            .iter()
            .rev()
            .find(|c| c.stage == SUBMITTED || c.stage == TESTED);
        let Some(last) = last.filter(|c| c.stage == TESTED) else {
            return Ok(None);
        };
        // A pass on criteria that changed since needs re-checking, and does not count (ADR
        // 0124).
        if self.needs_recheck(task, last.run)? {
            return Ok(None);
        }
        Ok(Some(vec![last.run]))
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

    /// The lead of a task's area narrows or rewords what done means for it, in its turn (ADR
    /// 0133): the ceremony finding tells a lead to, and the engine had the means and no tool. As
    /// many criteria as the task has, or fewer, and at least one; it is posted in the thread and
    /// recorded in the feed, and a tester's pass on the old ones needs checking again, as it does
    /// for the owner's rewrite.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::Finished`]; [`Refusal::OnlyTheAreasLead`] for anyone but
    /// the lead of the task's area; [`Refusal::NarrowedToNothing`]; [`Refusal::NotNarrower`];
    /// [`Refusal::TooLong`]; [`EngineError::Unknown`] for a task that does not exist.
    pub fn narrow_criteria_in(
        &self,
        run: RunId,
        task: TaskId,
        criteria: &[Criterion],
        why: &str,
    ) -> Done<()> {
        let _held = self.held();
        let turn = self.running(run)?;
        let current = self.task(task)?;
        if current.status.finished() {
            return Err(EngineError::Refused(Refusal::Finished));
        }
        let lead = self.member(turn.member)?;
        if self.area_lead(&current)?.is_none_or(|l| l.id != lead.id) {
            return Err(EngineError::Refused(Refusal::OnlyTheAreasLead));
        }
        if criteria.is_empty() {
            return Err(EngineError::Refused(Refusal::NarrowedToNothing));
        }
        let had = current.criteria.len();
        if had > 0 && criteria.len() > had {
            return Err(EngineError::Refused(Refusal::NotNarrower {
                had,
                gave: criteria.len(),
            }));
        }
        self.set_criteria_held_lock(task, criteria)?;
        // Narrowed, its talk is counted afresh, and the question put to its lead is answered
        // (ADR 0133).
        self.renew_coordination_budget(task)?;
        let texts = |all: &[Criterion]| all.iter().map(|c| c.text.clone()).collect::<Vec<_>>();
        let said = |all: &[Criterion]| {
            if all.is_empty() {
                return "none".to_owned();
            }
            all.iter()
                .enumerate()
                .map(|(i, c)| format!("{}. {}", i + 1, c.text))
                .collect::<Vec<_>>()
                .join("; ")
        };
        let why = one_line(why, 300);
        let because = if why.is_empty() {
            String::new()
        } else {
            format!(": {why}")
        };
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "{} narrowed what done means here{because}.\nWas: {}\nNow: {}",
                lead.name,
                said(&current.criteria),
                said(criteria)
            ),
        )?;
        self.emit(
            EventKind::CriteriaChanged,
            json!({ "task": task.0, "by": lead.id.0, "before": texts(&current.criteria),
                    "after": texts(criteria), "why": why }),
        )?;
        self.mark_progress(lead.id, task)
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

    /// Whether `member` is above `holder` in the chain of command: whom the holder reports to,
    /// and so on up, and the CEO above everyone (ADR 0114: the manager chain may act on a
    /// subordinate's task).
    pub(super) fn above_in_chain(&self, member: MemberId, holder: Option<MemberId>) -> Done<bool> {
        let Some(holder) = holder.filter(|h| *h != member) else {
            return Ok(false);
        };
        let members = self.store.members()?;
        if members
            .iter()
            .any(|m| m.id == member && m.role == Role::Ceo)
        {
            return Ok(true);
        }
        let mut seen = vec![holder];
        let mut at = members.iter().find(|m| m.id == holder);
        while let Some(m) = at {
            let Some(up) = chain::manager(m, &members) else {
                return Ok(false);
            };
            if up == member {
                return Ok(true);
            }
            if seen.contains(&up) {
                return Ok(false);
            }
            seen.push(up);
            at = members.iter().find(|x| x.id == up);
        }
        Ok(false)
    }

    /// A turn of a manager's hands a subordinate's task to another member (itself, or one
    /// below it), saying why, with its handover note (ADR 0122): let go, handed, `todo`, the
    /// note kept for the new member's first turn, and the new member woken. A task in a lead's
    /// area goes to someone outside it only by that lead's hand (ADR 0120, amended).
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::NotYours`] for a task held by nobody below the turn's
    /// member; [`Refusal::Finished`]; [`Refusal::NotBelow`]; [`Refusal::OutsideTheArea`] for a
    /// member outside the task's area, from anyone but that area's lead; [`EngineError::Unknown`].
    pub fn hand_to_in(
        &self,
        run: RunId,
        task: TaskId,
        to: MemberId,
        why: &str,
        handover: &crate::domain::world::HandoverNote,
    ) -> Done<()> {
        let _held = self.held();
        let turn = self.running(run)?;
        let current = self.task(task)?;
        let holder = current.owner.or(current.assignee);
        if !self.above_in_chain(turn.member, holder)? {
            return Err(EngineError::Refused(Refusal::NotYours));
        }
        if current.status.finished() {
            return Err(EngineError::Refused(Refusal::Finished));
        }
        let (by, target) = (self.member(turn.member)?, self.member(to)?);
        if target.furloughed() {
            return Err(EngineError::Refused(Refusal::Furloughed(target.name)));
        }
        // The CEO hands work to a lead only (ADR 0120).
        if by.role == Role::Ceo && target.role != Role::Lead {
            return Err(EngineError::Refused(Refusal::CeoToLeads));
        }
        if target.id != by.id {
            crate::domain::work::may_delegate(by.role, target.role)
                .map_err(EngineError::Refused)?;
        }
        // A task in a lead's area leaves it only by that lead's hand, or the owner's, who hands
        // out work another way (ADR 0120, amended): the CEO directs, and any other manager above
        // the holder asks. Handing it to someone inside the area is not leaving it.
        if let Some(area) = self.area_lead(&current)? {
            let members = self.store.members()?;
            let inside = staffing::lead_of(&target, &members) == Some(area.id);
            if by.id != area.id && !inside {
                return Err(EngineError::Refused(Refusal::OutsideTheArea {
                    lead: area.name,
                    to: target.name,
                }));
            }
        }
        let from = holder.map(|h| self.member(h)).transpose()?;
        self.store.release(task)?;
        self.store.set_assignee(task, Some(target.id))?;
        if current.status != TaskStatus::Todo {
            self.store.set_task_status(task, TaskStatus::Todo)?;
            self.emit_status(&current, TaskStatus::Todo)?;
        }
        self.post_held_lock(
            Author::Member(by.id),
            task,
            &format!(
                "Handed from {} to {} by {}, above them: {why}",
                from.map_or_else(|| "nobody".to_owned(), |m| m.name),
                target.name,
                by.name
            ),
        )?;
        self.mark_progress(by.id, task)?;
        self.keep_handover(&turn, task, holder, Some(target.id), handover)?;
        self.wake_held_lock(target.id, Some(task), WakeReason::Assigned)?;
        Ok(())
    }

    /// Who accepts `task`'s work: the first at work of its project's lead, the worker's
    /// manager when that one leads, the earliest lead, and the CEO; never the worker itself.
    pub(super) fn lead_for(&self, task: &Task) -> Done<Option<Member>> {
        let members = self.store.members()?;
        let worker = task.owner.or(task.assignee);
        let at_work = |id: Option<MemberId>| {
            id.and_then(|id| members.iter().find(|m| m.id == id))
                .filter(|m| m.at_work() && m.role.leads() && Some(m.id) != worker)
                .cloned()
        };
        let first = |role: Role| {
            members
                .iter()
                .filter(|m| m.role == role && m.at_work() && Some(m.id) != worker)
                .min_by_key(|m| m.id)
                .cloned()
        };
        let project_lead = match task.project {
            Some(project) => Some(self.project(project)?.lead),
            None => None,
        };
        let manager = worker
            .and_then(|w| members.iter().find(|m| m.id == w))
            .and_then(|m| chain::manager(m, &members));
        // The CEO accepts only its own work: in a lead's area the lead does (ADR 0120).
        let ceos_own = self.area_lead(task)?.is_none();
        Ok(at_work(project_lead)
            .filter(|m| ceos_own || m.role != Role::Ceo)
            .or_else(|| at_work(manager).filter(|m| ceos_own || m.role != Role::Ceo))
            .or_else(|| first(Role::Lead))
            .or_else(|| first(Role::Ceo).filter(|_| ceos_own)))
    }

    /// Who checks `task`'s work now.
    pub(super) fn checkers_of(&self, task: &Task) -> Done<Checkers<MemberId>> {
        let worker = task.owner.or(task.assignee);
        let tester = self.reviewer()?.filter(|r| r.at_work()).map(|r| r.id);
        let lead = self.lead_for(task)?.map(|l| l.id);
        Ok(evidence::checkers(worker, tester, lead))
    }

    /// Work sent back three times: its next submission is the owner's, with why each time, and
    /// only the owner advances it (ADR 0114). Once a submission.
    pub(super) fn rounds_to_the_owner(&self, task: &Task) -> Done<()> {
        let waiting = self
            .store
            .desk()?
            .iter()
            .any(|d| d.kind == "rounds" && d.status == "waiting" && d.task == Some(task.id));
        if waiting {
            return Ok(());
        }
        let members = self.store.members()?;
        let why: Vec<String> = self
            .store
            .thread(task.id)?
            .iter()
            .filter(|p| p.text.starts_with(SENT_BACK) && members.iter().any(|m| m.name == p.author))
            .map(|p| format!("- {}: {}", p.author, p.text.trim_start_matches(SENT_BACK)))
            .collect();
        let desk = self.store.add_desk(
            "rounds",
            Some(task.id),
            None,
            None,
            &format!(
                "Sent back {} times: {}. Its next round is yours: accept it as done, or send it \
                 back to its member.\n{}",
                why.len(),
                task.title,
                why.join("\n")
            ),
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": "rounds", "task": task.id.0 }),
        )
    }

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
        self.ceo_keeps_out(turn.member, &current)?;
        let check = to == TaskStatus::Done && self.routine_of(task)?.is_some();
        if to == TaskStatus::Done && !check && self.leads_own(&current, turn.member)? {
            return self.lead_closes_own(&current, &turn, reason, cited);
        }
        let judged = to == TaskStatus::InReview || (check && !current.criteria.is_empty());
        let holds = current.owner == Some(turn.member)
            || self.above_in_chain(turn.member, current.owner.or(current.assignee))?;
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
                self.post_held_lock(Author::Endora, task, &format!("Evidence cited: {said}"))?;
            }
        }
        Ok(())
    }

    /// Whether `task` is its project's lead's own work, held by `member` who leads it, with no
    /// branch on it (a branch goes through the wing's gate and the tester): a plan, a decision.
    fn leads_own(&self, task: &Task, member: MemberId) -> Done<bool> {
        if task.owner != Some(member) || task.project.is_none() {
            return Ok(false);
        }
        if self.area_lead(task)?.is_none_or(|l| l.id != member) {
            return Ok(false);
        }
        Ok(!self.store.branches()?.iter().any(|b| b.task == task.id))
    }

    /// The project's lead closes its own work as done, on its own acceptance with evidence
    /// (ADR 0120): no tester's round, nobody's leave. Judged by the one rule, its time-bound
    /// criteria waited.
    fn lead_closes_own(
        &self,
        task: &Task,
        turn: &Run,
        reason: &str,
        cited: &[Vec<Ref>],
    ) -> Done<()> {
        self.judge_work(
            task,
            cited,
            &Judged {
                fresh: &[turn.id],
                own_reading: None,
            },
        )?;
        self.waited_for(task)?;
        self.keep_cited(task.id, turn, ACCEPTED, cited)?;
        self.store.set_task_status(task.id, TaskStatus::Done)?;
        let said = self.evidence_said(&cited.concat())?;
        self.post_held_lock(
            Author::Member(turn.member),
            task.id,
            &format!(
                "{} → done: {reason} (the project's lead decides its own work)\nEvidence: {said}",
                task.status.name()
            ),
        )?;
        self.mark_progress(turn.member, task.id)?;
        self.emit_status(task, TaskStatus::Done)?;
        self.after_finishing(task)
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
        let task_parent = task.parent;
        // Who may hand what to whom comes first: a member who may not make the task at all is
        // told so, not asked what it serves.
        let to_another = task.assignee.filter(|a| *a != turn.member);
        if let Some(to) = to_another {
            let (from, to) = (self.member(turn.member)?, self.member(to)?);
            crate::domain::work::may_delegate(from.role, to.role).map_err(EngineError::Refused)?;
        }
        // The CEO delegates to leads, from its own work; in a lead's area it directs
        // (ADR 0120).
        if self.member(turn.member)?.role == Role::Ceo {
            if let Some(parent) = task.parent {
                self.ceo_keeps_out(turn.member, &self.task(parent)?)?;
            }
            let to_a_lead = match task.assignee {
                Some(to) => self.member(to)?.role == Role::Lead,
                None => false,
            };
            if !to_a_lead {
                return Err(EngineError::Refused(Refusal::CeoToLeads));
            }
        }
        let served = match task.parent {
            Some(parent) => self.subtask_discipline(parent, serves)?,
            None => None,
        };
        evidence::may_hand_on(criteria, to_another.is_some()).map_err(EngineError::Refused)?;
        let id = self.add_task_held_lock(Author::Member(turn.member), task)?;
        self.store.set_criteria(id, criteria)?;
        if let Some(n) = served {
            self.store
                .set_setting(&super::discipline::serves_key(id), &n.to_string())?;
        }
        if let Some(own) = turn.task {
            self.mark_progress(turn.member, own)?;
        }
        // A lead splitting the task it was asked about has decided it: the talk is counted afresh
        // (ADR 0133).
        if let Some(parent) =
            task_parent.filter(|p| self.asked_about_talk(*p, turn.member).unwrap_or(false))
        {
            self.renew_coordination_budget(parent)?;
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
        let id = self.add_task_held_lock(by, task)?;
        self.store.set_criteria(id, criteria)?;
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
        if tester.role != Role::Reviewer {
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
        self.post_held_lock(
            Author::Member(tester.id),
            task,
            &format!("Tested and passed: {notes}\nEvidence: {said}"),
        )?;
        self.mark_progress(tester.id, task)?;
        let lead = self.lead_for(&current)?.filter(|l| l.id != tester.id);
        if let Some(lead) = lead {
            self.wake_held_lock(lead.id, Some(task), WakeReason::AcceptanceAsked)?;
            return Ok(());
        }
        self.post_held_lock(
            Author::Endora,
            task,
            "No lead but the one who did this work: the tester's pass closes it.",
        )?;
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

    /// The lead's word on work in review (ADR 0114), in its turn: accepted against its
    /// criteria (citing evidence; the tester's readings count as fresh) it is done; sent back,
    /// its member puts it right. Never on work of the lead's own, and, while the team has a
    /// tester who did not do the work, only once she has passed it.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`], [`Refusal::NotTheLead`], [`Refusal::NotAStep`] for work not in
    /// review, [`Refusal::NotTested`], what [`evidence::judge`] refuses, and
    /// [`Refusal::GateNotPassed`] for gated work.
    pub fn accept_in(
        &self,
        run: RunId,
        task: TaskId,
        accept: bool,
        notes: &str,
        cited: &[Vec<Ref>],
        reason: Option<SendBackReason>,
    ) -> Done<()> {
        let _held = self.held();
        let turn = self.running(run)?;
        let lead = self.member(turn.member)?;
        let current = self.task(task)?;
        let worker = current.owner.or(current.assignee);
        if !lead.role.leads() || worker == Some(lead.id) {
            return Err(EngineError::Refused(Refusal::NotTheLead));
        }
        self.ceo_keeps_out(lead.id, &current)?;
        if current.status != TaskStatus::InReview {
            return Err(EngineError::Refused(Refusal::NotAStep {
                from: current.status,
                to: TaskStatus::Done,
            }));
        }
        if !accept {
            self.store.set_task_status(task, TaskStatus::Doing)?;
            self.post_held_lock(
                Author::Member(lead.id),
                task,
                &format!("{SENT_BACK}{notes}"),
            )?;
            self.emit_status(&current, TaskStatus::Doing)?;
            self.emit_sent_back(task, lead.id, "lead", reason)?;
            self.mark_progress(lead.id, task)?;
            if let Some(owner) = worker {
                self.wake_held_lock(owner, Some(task), WakeReason::SentBack)?;
            }
            // Bounced with nothing new between: a huddle, not another round (ADR 0124).
            return self.after_sent_back(task);
        }
        let tested = self.tested_since_submitted(task)?;
        let tester_there = matches!(
            self.checkers_of(&current)?,
            Checkers::Both { .. } | Checkers::TesterAlone(_)
        );
        if tester_there && tested.is_none() {
            return Err(EngineError::Refused(Refusal::NotTested));
        }
        let mut fresh = vec![turn.id];
        fresh.extend(tested.unwrap_or_default());
        self.judge_work(
            &current,
            cited,
            &Judged {
                fresh: &fresh,
                own_reading: None,
            },
        )?;
        self.waited_for(&current)?;
        self.keep_cited(task, &turn, ACCEPTED, cited)?;
        let said = self.evidence_said(&cited.concat())?;
        self.post_held_lock(
            Author::Member(lead.id),
            task,
            &format!("Accepted: {notes}\nEvidence: {said}"),
        )?;
        self.mark_progress(lead.id, task)?;
        self.close_checked(&current, lead.id)
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
        self.post_in_marked(run, task, text, true, cited)?;
        // A note the post cites by number, or by what it rests on, was used (ADR 0118).
        self.notes_used(run, text, cited)
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
    /// briefing says it) for the first two, then, once, the task goes to whom the member reports
    /// to, with a post saying so.
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
        let members = self.store.members()?;
        let member = self.member(run.member)?;
        let Some(manager) = chain::manager(&member, &members)
            .and_then(|id| members.iter().find(|m| m.id == id).cloned())
            .filter(|m| m.id != member.id)
        else {
            return Ok(());
        };
        self.store.set_setting(&key, "1")?;
        // The CEO never takes an area's work (ADR 0120): live, #667 went up from Ada to Mara,
        // who closed it two minutes after a restart. It stays with the area's lead, said so.
        if manager.role == Role::Ceo {
            if let Some(lead) = self.area_lead(&task)? {
                self.post_held_lock(
                    Author::Endora,
                    task_id,
                    &format!(
                        "{} took {count} turns on this that showed nothing (talk only, empty or \
                         the same reading again), after {} nudges. It stays in {}'s area: the \
                         CEO directs, and does not take the area's work. Take one concrete step, \
                         or mark it blocked and say why.",
                        member.name,
                        evidence::MOST_NUDGES,
                        lead.name
                    ),
                )?;
                return Ok(());
            }
        }
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
    /// excerpt (left out of a Confidential one for a reader on a cloud model).
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

    /// The receipts a draft cites, as its checker is told them.
    pub(super) fn draft_receipts(&self, draft: &str, cloud: bool) -> Done<String> {
        let mut told = String::new();
        let mut seen = Vec::new();
        for word in draft.split(|c: char| !(c.is_ascii_alphanumeric())) {
            let Some(Ref::Receipt(id)) = Ref::parse(word) else {
                continue;
            };
            let r = Ref::Receipt(id);
            if seen.contains(&r) || self.store.call_receipt(id)?.is_none() {
                continue;
            }
            seen.push(r);
            told.push_str(&format!("- {}\n", self.ref_line(r, cloud)?));
        }
        if told.is_empty() {
            return Ok(told);
        }
        Ok(format!(
            "The receipts it cites, as the node recorded them:\n{told}"
        ))
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

    /// The members' posts that are unsupported, marked in a thread line.
    pub(crate) fn post_line(author: &str, text: &str, unsupported: bool) -> String {
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
