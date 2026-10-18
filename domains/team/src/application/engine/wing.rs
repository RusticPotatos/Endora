//! The engineering wing and the git flow (ADR 0115): a submitted branch recorded with its
//! document and a check job queued for the dev node; the check's result as a receipt that
//! wakes the submitter and the tester (or sends failed work back); the owner's push offered on
//! the desk once the lead accepts, and what came of it; hiring by the owner, a lead's request
//! to hire within the wing's seats; the wing seeded once; and documents members read. Every
//! rule is code: branch names, seats, roles, who is woken, what closes.

use serde_json::json;

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, DeskItem, NewTask, TeamStore, WorkerAdapter};
use crate::domain::wing::{self, HireRequest, WING_GOAL, WING_NOTE, WING_PROJECT};
use crate::domain::{
    Brain, Branch, CallReceipt, Check, Criterion, Document, EventKind, GoalId, GoalStatus,
    MemberId, Priority, ProjectId, Ref, Refusal, Role, RunId, TaskId, TaskStatus, WakeReason,
};

/// Set once HQ has seeded the wing, so it is never seeded again.
const WING_SEEDED: &str = "wing_seeded";

/// The most a document holds: the diff shown (64 KB) and its stat.
pub const DOCUMENT_MOST: usize = 72 * 1024;

/// How much of a document a turn reads at a time.
pub const DOCUMENT_PART: usize = 16_000;

/// The wing's first task, for its lead.
pub const WING_FIRST_TASK: &str =
    "Plan the clean-up: read the fork's docs and propose the first three branches";

/// A branch as the node took it (ADR 0115), told to HQ with the turn's token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    /// The branch, under `hq/`.
    pub branch: String,
    /// Its head.
    pub head: String,
    /// The default branch's head it sits on.
    pub base: String,
    /// How many files it changes.
    pub files: usize,
    /// Lines added.
    pub plus: i64,
    /// Lines removed.
    pub minus: i64,
    /// `git diff --stat`.
    pub stat: String,
    /// The diff, as the node shows it (at most 64 KB).
    pub diff: String,
    /// Whether the node can push it (a deploy key is set up).
    pub can_push: bool,
}

/// What recording a submission made: the branch, its document, and its check job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recorded {
    /// The branch's id.
    pub branch: i64,
    /// The document's id, `d<id>`.
    pub document: i64,
    /// The check's id, `c<id>`.
    pub check: i64,
}

/// What seeding the wing made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WingSeeded {
    /// The wing's goal.
    pub goal: GoalId,
    /// Its first project.
    pub project: ProjectId,
    /// Its lead.
    pub lead: MemberId,
    /// Its developers.
    pub developers: Vec<MemberId>,
    /// Its first task, for the lead.
    pub task: TaskId,
}

/// A short hash, as the thread shows one.
fn short(sha: &str) -> &str {
    &sha[..12.min(sha.len())]
}

/// Text held to `most` characters, saying so when cut.
fn held_to(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let kept: String = text.chars().take(most).collect();
    format!("{kept}\n[cut]")
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Records a branch the node took from a turn (ADR 0115): the branch, a document with its
    /// stat and diff, a line in the task's thread, and a check job queued for the dev node's
    /// runner. Named by the turn's own token, while it runs.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a token that opens no running turn, a turn on no task, or a
    /// branch not under `hq/`; a store failure.
    pub fn record_branch(&self, token: &str, s: &Submission) -> Done<Recorded> {
        let _held = self.held();
        let Some(id) = self.store.run_of_token(token)? else {
            return Err(EngineError::Unknown("running turn with that token"));
        };
        let run = self
            .running(id)
            .map_err(|_| EngineError::Unknown("running turn with that token"))?;
        let task = run.task.ok_or(EngineError::Unknown("task"))?;
        if !s.branch.starts_with(wing::BRANCH_PREFIX) || s.head.len() < 7 {
            return Err(EngineError::Unknown("branch under hq/"));
        }
        let now = self.clock.now();
        let title = format!(
            "branch {} ({} file{}, +{} −{})",
            s.branch,
            s.files,
            if s.files == 1 { "" } else { "s" },
            s.plus,
            s.minus
        );
        let document = self.store.add_document(&Document {
            id: 0,
            task,
            run: Some(run.id),
            title: title.clone(),
            text: held_to(&format!("{}\n\n{}", s.stat.trim(), s.diff), DOCUMENT_MOST),
            at: now,
        })?;
        let branch = self.store.add_branch(&Branch {
            id: 0,
            task,
            run: run.id,
            member: run.member,
            name: s.branch.clone(),
            head: s.head.clone(),
            base: s.base.clone(),
            files: s.files,
            plus: s.plus,
            minus: s.minus,
            can_push: s.can_push,
            status: "submitted".to_owned(),
            at: now,
        })?;
        let (check, _) = self.store.add_check(&Check {
            id: 0,
            task,
            branch,
            run: run.id,
            token: String::new(),
            status: "waiting".to_owned(),
            ok: None,
            summary: String::new(),
            queued_at: now,
            started_at: None,
            done_at: None,
        })?;
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "Submitted {title} at {}: document d{document}. Check c{check} is queued on the \
                 dev node (ansible syntax, ansible-lint, yamllint); its result comes back as a \
                 receipt.",
                short(&s.head)
            ),
        )?;
        self.emit(
            EventKind::BranchSubmitted,
            json!({ "task": task.0, "branch": branch, "name": s.branch, "head": s.head,
                    "files": s.files, "member": run.member.0, "run": run.id.0,
                    "document": document, "check": check }),
        )?;
        Ok(Recorded {
            branch,
            document,
            check,
        })
    }

    /// The next check job for the runner: the oldest waiting, now running. A check running
    /// longer than [`wing::CHECK_LOST_AFTER_MS`] is given up first, as failed. Nothing while
    /// the kill switch is pulled.
    ///
    /// # Errors
    /// A store failure.
    pub fn next_check(&self) -> Done<Option<Check>> {
        let _held = self.held();
        if self.store.stopped()? {
            return Ok(None);
        }
        let now = self.clock.now();
        let checks = self.store.checks()?;
        for lost in checks.iter().filter(|c| {
            c.status == "running"
                && c.started_at
                    .is_some_and(|at| now - at > wing::CHECK_LOST_AFTER_MS)
        }) {
            self.finish_check(
                lost,
                false,
                "the check never reported: the dev node may be down",
                "",
            )?;
        }
        let Some(mut next) = checks.into_iter().find(|c| c.status == "waiting") else {
            return Ok(None);
        };
        next.status = "running".to_owned();
        next.started_at = Some(now);
        self.store.save_check(&next)?;
        Ok(Some(next))
    }

    /// The check a job token names, while it waits or runs: how the node knows which bundle
    /// to hand the check's sandbox.
    ///
    /// # Errors
    /// A store failure.
    pub fn check_by_token(&self, token: &str) -> Done<Option<(Check, Branch)>> {
        let _held = self.held();
        if token.len() < 32 {
            return Ok(None);
        }
        let Some(check) = self
            .store
            .checks()?
            .into_iter()
            .find(|c| c.token == token && c.status != "done")
        else {
            return Ok(None);
        };
        let branch = self
            .store
            .branches()?
            .into_iter()
            .find(|b| b.id == check.branch)
            .ok_or(EngineError::Unknown("branch"))?;
        Ok(Some((check, branch)))
    }

    /// A check's report, from the runner with the job's token: pass or fail, a line and the
    /// commands' last words. Taken once.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a check or token that is not one; [`Refusal::Finished`]
    /// for one reported already.
    pub fn check_done(
        &self,
        id: i64,
        token: &str,
        ok: bool,
        summary: &str,
        report: &str,
    ) -> Done<()> {
        let _held = self.held();
        let check = self
            .store
            .checks()?
            .into_iter()
            .find(|c| c.id == id && c.token == token)
            .ok_or(EngineError::Unknown("check with that token"))?;
        if check.status == "done" {
            return Err(EngineError::Refused(Refusal::Finished));
        }
        self.finish_check(&check, ok, summary, report)
    }

    /// Records a check's result (the lock held): a receipt on the task, a document with the
    /// report, a line in the thread; failed work in review goes back to its member, and a pass
    /// wakes the submitter and the tester.
    fn finish_check(&self, check: &Check, ok: bool, summary: &str, report: &str) -> Done<()> {
        let now = self.clock.now();
        let summary = super::one_line(summary, 300);
        let summary = if summary.is_empty() {
            if ok { "passed" } else { "failed" }.to_owned()
        } else {
            summary
        };
        let branch = self
            .store
            .branches()?
            .into_iter()
            .find(|b| b.id == check.branch)
            .ok_or(EngineError::Unknown("branch"))?;
        let mut done = check.clone();
        done.status = "done".to_owned();
        done.ok = Some(ok);
        done.summary.clone_from(&summary);
        done.done_at = Some(now);
        self.store.save_check(&done)?;
        // The check is the wing's gate (tests, lint and build, ADR 0114): code work closes only
        // once it has passed, and the lab's own checks are what passes it.
        self.store.set_gate(check.task, ok)?;
        self.emit(
            EventKind::GateResult,
            json!({ "task": check.task.0, "passed": ok, "failed": if ok { "" } else { summary.as_str() },
                    "check": check.id }),
        )?;
        let receipt = self.store.add_call_receipt(&CallReceipt {
            id: 0,
            run: check.run,
            member: branch.member,
            task: Some(check.task),
            tool: "check".to_owned(),
            args: format!("c{} {}@{}", check.id, branch.name, short(&branch.head)),
            excerpt: summary.clone(),
            ok,
            act: false,
            confidential: false,
            sandbox: false,
            at: now,
        })?;
        let document = self.store.add_document(&Document {
            id: 0,
            task: check.task,
            run: Some(check.run),
            title: format!("check c{}: {summary}", check.id),
            text: held_to(report, DOCUMENT_MOST),
            at: now,
        })?;
        self.post_held_lock(
            Author::Endora,
            check.task,
            &format!(
                "Check c{} on {}: {summary} (receipt r{receipt}, document d{document}).",
                check.id, branch.name
            ),
        )?;
        self.emit(
            EventKind::CheckDone,
            json!({ "task": check.task.0, "check": check.id, "branch": branch.name, "ok": ok,
                    "summary": summary, "receipt": receipt, "document": document,
                    "member": branch.member.0 }),
        )?;
        let task = self.task(check.task)?;
        if !ok && task.status == TaskStatus::InReview {
            // A branch that fails its checks is sent back (ADR 0115): the tester need not read
            // what the lab's own checks already refused.
            self.store.set_task_status(task.id, TaskStatus::Doing)?;
            self.post_held_lock(
                Author::Endora,
                task.id,
                &format!(
                    "Sent back: check c{} failed ({summary}). Read document d{document}, put it \
                     right, submit the branch again and then the task.",
                    check.id
                ),
            )?;
            self.emit_status(&task, TaskStatus::Doing)?;
            self.wake_held_lock(branch.member, Some(task.id), WakeReason::SentBack)?;
            return Ok(());
        }
        if task.status.finished() {
            return Ok(());
        }
        self.wake_held_lock(branch.member, Some(task.id), WakeReason::CheckDone)?;
        for tester in self
            .store
            .members()?
            .iter()
            .filter(|m| m.role == Role::Reviewer && !m.paused && m.id != branch.member)
        {
            self.wake_held_lock(tester.id, Some(task.id), WakeReason::CheckDone)?;
        }
        Ok(())
    }

    /// Once a task with a submitted branch is done: the push is offered on the owner's desk,
    /// saying the branch, its size and how its checks went; or, when the node has no deploy
    /// key, that pushing is not set up and where the bundle is. Called when a task reaches
    /// done; nothing for a task with no branch, or one offered already.
    ///
    /// # Errors
    /// A store failure.
    pub fn offer_push(&self, task: TaskId) -> Done<()> {
        let Some(branch) = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.task == task && b.status == "submitted")
            .max_by_key(|b| b.id)
        else {
            return Ok(());
        };
        let checks = self.checks_said(branch.id)?;
        let files = format!(
            "{} file{}",
            branch.files,
            if branch.files == 1 { "" } else { "s" }
        );
        let text = if branch.can_push {
            format!(
                "Push {} to your fork ({files}, {checks}). It is {} on top of {}. Approve, and \
                 Endora pushes that branch to the fork over SSH with the deploy key, nothing \
                 else: you merge it on GitHub. Decline, and it stays on the node.",
                branch.name,
                short(&branch.head),
                short(&branch.base)
            )
        } else {
            format!(
                "Branch {} is accepted ({files}, {checks}), but pushing is not set up on the \
                 node (ENDORA_GIT_PUSH_KEY_FILE names no deploy key). Download its bundle from \
                 the node at /v1/hq/bundles/{}/{} and push it yourself.",
                branch.name,
                task.0,
                branch.name.trim_start_matches(wing::BRANCH_PREFIX)
            )
        };
        let item = self.store.add_desk(
            "push",
            Some(task),
            None,
            Some(branch.member),
            &text,
            self.clock.now(),
        )?;
        let mut offered = branch;
        offered.status = "offered".to_owned();
        self.store.save_branch(&offered)?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": item.0, "kind": "push", "task": task.0, "branch": offered.name }),
        )?;
        Ok(())
    }

    /// How a branch's checks went, in a few words.
    fn checks_said(&self, branch: i64) -> Done<String> {
        let latest = self
            .store
            .checks()?
            .into_iter()
            .filter(|c| c.branch == branch)
            .max_by_key(|c| c.id);
        Ok(match latest {
            Some(c) if c.status == "done" && c.ok == Some(true) => "checks passed".to_owned(),
            Some(c) if c.status == "done" => format!("checks failed: {}", c.summary),
            Some(_) => "checks not finished".to_owned(),
            None => "no check ran".to_owned(),
        })
    }

    /// The owner's decision on a push (the lock held): approved, the branch waits for the
    /// node to push it; declined, it stays on the node and its member hears.
    pub(super) fn decide_push(&self, found: &DeskItem, approve: bool) -> Done<()> {
        let task = found.task.ok_or(EngineError::Unknown("task"))?;
        let Some(mut branch) = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.task == task && b.status == "offered")
            .max_by_key(|b| b.id)
        else {
            return Err(EngineError::Unknown("offered branch"));
        };
        if approve && !branch.can_push {
            return Err(EngineError::Unknown("decision for this item"));
        }
        branch.status = if approve { "approved" } else { "declined" }.to_owned();
        self.store.save_branch(&branch)?;
        if !approve {
            self.post_held_lock(
                Author::Owner,
                task,
                &format!(
                    "The owner declined pushing {}: it stays stored on the node.",
                    branch.name
                ),
            )?;
            self.wake_held_lock(branch.member, Some(task), WakeReason::ProposalDecided)?;
        }
        Ok(())
    }

    /// The branch a push item the owner approved asks the node to push, if that is what the
    /// item is.
    ///
    /// # Errors
    /// A store failure.
    pub fn push_asked(&self, item: crate::domain::DeskId) -> Done<Option<Branch>> {
        let _held = self.held();
        let Some(found) = self
            .store
            .desk()?
            .into_iter()
            .find(|d| d.id == item && d.kind == "push" && d.status == "approve")
        else {
            return Ok(None);
        };
        Ok(self
            .store
            .branches()?
            .into_iter()
            .filter(|b| Some(b.task) == found.task && b.status == "approved")
            .max_by_key(|b| b.id))
    }

    /// What came of a push the owner approved, from the node (ADR 0115): said in the thread,
    /// kept as a receipt on the task, and the branch's member woken. Told once.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for an item that is not an approved push;
    /// [`Refusal::Finished`] once told.
    pub fn push_outcome(&self, item: crate::domain::DeskId, ok: bool, text: &str) -> Done<()> {
        let _held = self.held();
        let found = self
            .store
            .desk()?
            .into_iter()
            .find(|d| d.id == item && d.kind == "push")
            .ok_or(EngineError::Unknown("push"))?;
        match found.status.as_str() {
            "approve" => {}
            "pushed" | "failed" => return Err(EngineError::Refused(Refusal::Finished)),
            _ => return Err(EngineError::Unknown("approved push")),
        }
        let task = found.task.ok_or(EngineError::Unknown("task"))?;
        let mut branch = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.task == task && b.status == "approved")
            .max_by_key(|b| b.id)
            .ok_or(EngineError::Unknown("approved branch"))?;
        let status = if ok { "pushed" } else { "failed" };
        self.store.set_desk_status(item, status)?;
        branch.status = status.to_owned();
        self.store.save_branch(&branch)?;
        let text = super::one_line(text, 1000);
        let receipt = self.store.add_call_receipt(&CallReceipt {
            id: 0,
            run: branch.run,
            member: branch.member,
            task: Some(task),
            tool: "push".to_owned(),
            args: format!("{}@{}", branch.name, short(&branch.head)),
            excerpt: text.clone(),
            ok,
            act: true,
            confidential: false,
            sandbox: false,
            at: self.clock.now(),
        })?;
        self.post_held_lock(
            Author::Endora,
            task,
            &format!("{text} (receipt r{receipt})"),
        )?;
        self.emit(
            EventKind::BranchPushed,
            json!({ "task": task.0, "branch": branch.name, "head": branch.head, "ok": ok,
                    "receipt": receipt, "member": branch.member.0 }),
        )?;
        self.wake_held_lock(branch.member, Some(task), WakeReason::ProposalDecided)?;
        Ok(())
    }

    // ---- hiring (ADR 0115) ---------------------------------------------------------------

    /// The owner hires a member: a name, a role, a brain its role may think with, and whom it
    /// reports to (its role's default when `None`). The team has one CEO. Said in the feed as
    /// the owner's.
    ///
    /// # Errors
    /// [`Refusal::OnlyOneCeo`], [`Refusal::NotAName`], [`Refusal::NotABrain`],
    /// [`EngineError::Unknown`] for a manager that is no member; a store failure.
    pub fn hire_by_owner(
        &self,
        name: &str,
        role: Role,
        brain: &str,
        reports_to: Option<MemberId>,
    ) -> Done<MemberId> {
        let _held = self.held();
        self.hire_held_lock(name, role, brain, reports_to, "owner")
    }

    fn hire_held_lock(
        &self,
        name: &str,
        role: Role,
        brain: &str,
        reports_to: Option<MemberId>,
        by: &str,
    ) -> Done<MemberId> {
        let members = self.store.members()?;
        if role == Role::Ceo && members.iter().any(|m| m.role == Role::Ceo) {
            return Err(EngineError::Refused(Refusal::OnlyOneCeo));
        }
        let name = super::one_line(name, 41);
        let taken = members.iter().any(|m| m.name.eq_ignore_ascii_case(&name));
        if name.is_empty() || name.chars().count() > 40 || taken {
            return Err(EngineError::Refused(Refusal::NotAName));
        }
        let brain = Brain::for_role(role, brain).map_err(EngineError::Refused)?;
        if let Some(to) = reports_to {
            if role == Role::Ceo || !members.iter().any(|m| m.id == to) {
                return Err(EngineError::Refused(Refusal::NotAManager));
            }
        }
        let manager = match reports_to {
            Some(to) => Some(to),
            None => crate::domain::chain::manager_at_hire(role, &members),
        };
        let look = u8::try_from(members.len() % 6).unwrap_or(0);
        let id = self.store.add_member(&name, role, role.share(), 2, look)?;
        self.store.set_brain(id, brain)?;
        self.store.set_reports_to(id, manager)?;
        self.emit(
            EventKind::MemberHired,
            json!({ "member": id.0, "name": name, "role": role.name(), "brain": brain.name(),
                    "reports_to": manager.map(|m| m.0), "by": by }),
        )?;
        Ok(id)
    }

    /// The members of the wing: whoever leads one of its projects, and whoever reports up to
    /// them.
    ///
    /// # Errors
    /// A store failure.
    pub fn wing_members(&self) -> Done<Vec<MemberId>> {
        let members = self.store.members()?;
        let leads = self.wing_leads()?;
        Ok(members
            .iter()
            .filter(|m| wing::in_wing(m, &leads, &members))
            .map(|m| m.id)
            .collect())
    }

    fn wing_leads(&self) -> Done<Vec<MemberId>> {
        let goals: Vec<GoalId> = self
            .store
            .goals()?
            .into_iter()
            .filter(|g| g.title.trim().eq_ignore_ascii_case(WING_GOAL))
            .map(|g| g.id)
            .collect();
        Ok(self
            .store
            .projects()?
            .into_iter()
            .filter(|p| goals.contains(&p.goal))
            .map(|p| p.lead)
            .collect())
    }

    /// A lead asks the owner to hire (ADR 0115): a member in a role below its own, on a brain
    /// that role may think with, reporting to the lead, for a reason and on evidence from
    /// HQ's record. In the wing the seats are a rule: one lead, at most five developers, on
    /// the brains the owner agreed. The request waits on the owner's desk, one at a time per
    /// lead; approving hires.
    ///
    /// # Errors
    /// [`Refusal::OnlyLeadsHire`], [`Refusal::NotBelowAsker`], [`Refusal::NotABrain`],
    /// [`Refusal::NoEvidence`], [`Refusal::NoSuchEvidence`], [`Refusal::NoSeat`],
    /// [`Refusal::WingBrain`], [`Refusal::AlreadyAsked`], [`Refusal::NotAName`].
    pub fn request_hire(
        &self,
        run: RunId,
        name: Option<&str>,
        role: Role,
        brain: &str,
        why: &str,
        cited: &[Ref],
    ) -> Done<crate::domain::DeskId> {
        let _held = self.held();
        let turn = self.running(run)?;
        let asker = self.member(turn.member)?;
        if asker.role != Role::Lead {
            return Err(EngineError::Refused(Refusal::OnlyLeadsHire));
        }
        if role.rank() <= asker.role.rank() {
            return Err(EngineError::Refused(Refusal::NotBelowAsker));
        }
        let brain = Brain::for_role(role, brain).map_err(EngineError::Refused)?;
        if cited.is_empty() {
            return Err(EngineError::Refused(Refusal::NoEvidence));
        }
        for group in self.look_up(None, &[cited.to_vec()])? {
            for (r, found) in group {
                if found == crate::domain::evidence::Found::Missing {
                    return Err(EngineError::Refused(Refusal::NoSuchEvidence(r.said())));
                }
            }
        }
        let members = self.store.members()?;
        let leads = self.wing_leads()?;
        if wing::in_wing(&asker, &leads, &members) {
            let wing: Vec<&crate::domain::Member> = members
                .iter()
                .filter(|m| wing::in_wing(m, &leads, &members))
                .collect();
            wing::may_seat(role, brain, &wing).map_err(EngineError::Refused)?;
        }
        if self
            .store
            .desk()?
            .iter()
            .any(|d| d.kind == "hire" && d.status == "waiting" && d.member == Some(asker.id))
        {
            return Err(EngineError::Refused(Refusal::AlreadyAsked));
        }
        let name = match name
            .map(|n| super::one_line(n, 41))
            .filter(|n| !n.is_empty())
        {
            Some(name) => {
                if name.chars().count() > 40
                    || members.iter().any(|m| m.name.eq_ignore_ascii_case(&name))
                {
                    return Err(EngineError::Refused(Refusal::NotAName));
                }
                name
            }
            None => wing::spare_name(&members),
        };
        let request = HireRequest {
            name,
            role,
            brain,
            reports_to: asker.id,
            why: super::one_line(why, 400),
        };
        let evidence = self.evidence_said(cited)?;
        let text = request.write(&asker.name, &evidence);
        let item = self.store.add_desk(
            "hire",
            turn.task,
            None,
            Some(asker.id),
            &text,
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": item.0, "kind": "hire", "task": turn.task.map(|t| t.0),
                    "member": asker.id.0, "name": request.name, "role": role.name() }),
        )?;
        Ok(item)
    }

    /// The owner's decision on a hire request (the lock held): approved, the member joins,
    /// reporting to the lead who asked; either way the lead hears in the task's thread.
    pub(super) fn decide_hire(&self, found: &DeskItem, approve: bool) -> Done<()> {
        let request = HireRequest::read(&found.text).ok_or(EngineError::Unknown("hire request"))?;
        let said = if approve {
            let id = self.hire_held_lock(
                &request.name,
                request.role,
                request.brain.name(),
                Some(request.reports_to),
                "owner, asked by a lead",
            )?;
            format!(
                "The owner approved the hire: {} ({}) joins as {} on {}, reporting to you.",
                request.name,
                id,
                request.role.name(),
                request.brain.label()
            )
        } else {
            format!("The owner declined hiring {}.", request.name)
        };
        if let Some(task) = found.task {
            self.post_held_lock(Author::Owner, task, &said)?;
            if let Some(asker) = found.member {
                self.wake_held_lock(asker, Some(task), WakeReason::ProposalDecided)?;
            }
        }
        Ok(())
    }

    // ---- the wing, seeded once (ADR 0115) --------------------------------------------------

    /// Seeds the wing, once (a setting remembers it), for HQ started with the demo off: the
    /// owner's goal "Improve the lab" with the wing's notes for the team, the project
    /// "ansible-nas clean-up" led by Kit, Kit (a lead on Claude Sonnet, reporting to the
    /// CEO), Rae and Zed (developers on Claude Haiku, reporting to Kit), and Kit's first task,
    /// to plan the clean-up. What is already there (by name or title) is kept, not made again.
    ///
    /// # Errors
    /// What the engine refuses, or a store failure.
    pub fn seed_wing(&self) -> Done<Option<WingSeeded>> {
        if self.store.setting(WING_SEEDED)?.is_some() {
            return Ok(None);
        }
        let goal = match self
            .store
            .goals()?
            .into_iter()
            .find(|g| g.title.trim().eq_ignore_ascii_case(WING_GOAL) && g.status != GoalStatus::Dropped)
        {
            Some(goal) => goal.id,
            None => self.set_goal(
                WING_GOAL,
                "The wing cleans up the lab's ansible-nas fork, one reviewed branch at a time; the owner merges.",
                Author::Owner,
            )?,
        };
        if self.store.goal_note(goal)?.trim().is_empty() {
            self.set_goal_note(goal, WING_NOTE)?;
        }
        let named = |name: &str| -> Done<Option<MemberId>> {
            Ok(self
                .store
                .members()?
                .into_iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
                .map(|m| m.id))
        };
        let lead = match named("Kit")? {
            Some(id) => id,
            None => {
                let id = self.hire("Kit", Role::Lead, Role::Lead.share(), 2, 5)?;
                self.set_brain(id, Brain::Sonnet.name())?;
                id
            }
        };
        let mut developers = Vec::new();
        for (name, look) in [("Rae", 2), ("Zed", 4)] {
            let id = match named(name)? {
                Some(id) => id,
                None => {
                    let id = self.hire(name, Role::Developer, Role::Developer.share(), 2, look)?;
                    self.set_brain(id, Brain::Haiku.name())?;
                    self.set_reports_to(id, lead)?;
                    id
                }
            };
            developers.push(id);
        }
        let project = match self
            .store
            .projects()?
            .into_iter()
            .find(|p| p.goal == goal && p.title.eq_ignore_ascii_case(WING_PROJECT))
        {
            Some(p) => p.id,
            None => self.add_project(
                goal,
                WING_PROJECT,
                "The fork's roles and docs, tidied one branch at a time; the owner merges each.",
                lead,
                None,
            )?,
        };
        let task = match self
            .store
            .tasks()?
            .into_iter()
            .find(|t| t.project == Some(project) && t.title == WING_FIRST_TASK)
        {
            Some(t) => t.id,
            None => self.add_task_with(
                Author::Owner,
                NewTask {
                    goal,
                    project: Some(project),
                    parent: None,
                    title: WING_FIRST_TASK.to_owned(),
                    description: "Read the fork's docs (ansible_nas_doc: roles, website/docs) and \
                        check out its default branch (repo_checkout) to see how it is laid out. \
                        Then post a plan: the first three branches, each named hq/task-N-<slug>, \
                        what each changes and why it is safe. Hand each to a developer as a \
                        sub-task with criteria once the owner has read the plan."
                        .to_owned(),
                    assignee: Some(lead),
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
                &[
                    Criterion::new(
                        "The plan cites the fork's files it read, by receipt (ansible_nas_doc or repo_checkout)",
                        false,
                    ),
                    Criterion::new(
                        "Three branches are proposed, each named hq/task-N-<slug>, with what each changes and why",
                        false,
                    ),
                ],
            )?,
        };
        self.store.set_setting(WING_SEEDED, "1")?;
        Ok(Some(WingSeeded {
            goal,
            project,
            lead,
            developers,
            task,
        }))
    }

    // ---- documents ---------------------------------------------------------------------

    /// A document of the turn's own work (its task, the tasks under it, or the one above),
    /// [`DOCUMENT_PART`] characters a part.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`], [`Refusal::NotADocument`].
    pub fn read_document(&self, run: RunId, id: i64, part: usize) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        let Some(document) = self.store.document(id)? else {
            return Err(EngineError::Refused(Refusal::NotADocument));
        };
        let mut reach = match turn.task {
            Some(task) => self.work_of(task)?,
            None => Vec::new(),
        };
        if let Some(parent) = turn
            .task
            .and_then(|t| self.store.task(t).ok().flatten())
            .and_then(|t| t.parent)
        {
            reach.push(parent);
        }
        // The CEO and a lead read any document: they accept the work.
        let member = self.member(turn.member)?;
        if !member.role.leads() && !reach.contains(&document.task) {
            return Err(EngineError::Refused(Refusal::NotADocument));
        }
        let chars: Vec<char> = document.text.chars().collect();
        let parts = chars.len().div_ceil(DOCUMENT_PART).max(1);
        let part = part.clamp(1, parts);
        let from = (part - 1) * DOCUMENT_PART;
        let text: String = chars[from..(from + DOCUMENT_PART).min(chars.len())]
            .iter()
            .collect();
        Ok(format!(
            "Document d{} on task #{}: {} (part {part} of {parts})\n{text}",
            document.id, document.task, document.title
        ))
    }

    /// A task's documents and branches, for its briefing: a line each, or nothing.
    pub(super) fn documents_said(&self, task: TaskId) -> Done<String> {
        let documents = self.store.documents_of(task)?;
        let branches: Vec<Branch> = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.task == task)
            .collect();
        if documents.is_empty() && branches.is_empty() {
            return Ok(String::new());
        }
        let mut told = String::new();
        if !documents.is_empty() {
            let list: Vec<String> = documents
                .iter()
                .map(|d| format!("d{} {}", d.id, d.title))
                .collect();
            told.push_str(&format!(
                "Documents on this task (read one with read_document): {}\n",
                list.join("; ")
            ));
        }
        for b in branches {
            told.push_str(&format!(
                "Branch {} at {} is {}.\n",
                b.name,
                short(&b.head),
                b.status
            ));
        }
        Ok(told)
    }

    /// A task's documents and branches, as the HQ screen shows them.
    pub(super) fn documents_shown(
        &self,
        task: TaskId,
    ) -> Done<(serde_json::Value, serde_json::Value)> {
        let documents: Vec<serde_json::Value> = self
            .store
            .documents_of(task)?
            .iter()
            .map(|d| json!({ "id": d.id, "title": d.title, "at": d.at, "text": d.text }))
            .collect();
        let checks = self.store.checks()?;
        let branches: Vec<serde_json::Value> = self
            .store
            .branches()?
            .iter()
            .filter(|b| b.task == task)
            .map(|b| {
                let check = checks
                    .iter()
                    .filter(|c| c.branch == b.id)
                    .max_by_key(|c| c.id);
                json!({ "id": b.id, "name": b.name, "head": b.head, "files": b.files,
                        "plus": b.plus, "minus": b.minus, "status": b.status, "at": b.at,
                        "can_push": b.can_push,
                        "check": check.map(|c| json!({ "id": c.id, "status": c.status,
                                                        "ok": c.ok, "summary": c.summary })) })
            })
            .collect();
        Ok((json!(documents), json!(branches)))
    }

    /// The wing as the HQ screen shows it: its goal, its projects' leads, its members and the
    /// agreed seats.
    pub(super) fn wing_shown(&self) -> Done<serde_json::Value> {
        let members = self.store.members()?;
        let leads = self.wing_leads()?;
        let wing: Vec<&crate::domain::Member> = members
            .iter()
            .filter(|m| wing::in_wing(m, &leads, &members))
            .collect();
        Ok(json!({
            "goal": WING_GOAL,
            "leads": leads.iter().map(|l| l.0).collect::<Vec<_>>(),
            "members": wing.iter().map(|m| m.id.0).collect::<Vec<_>>(),
            "developers": wing.iter().filter(|m| m.role == Role::Developer).count(),
            "most_developers": wing::MOST_DEVELOPERS,
            "most_leads": wing::MOST_LEADS,
        }))
    }
}
