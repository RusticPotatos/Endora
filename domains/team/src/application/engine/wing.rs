//! The engineering wing and the git flow (ADR 0115): a submitted branch recorded with its
//! document and a check job queued for the dev node; the check's result as a receipt that
//! wakes the submitter (or sends failed work back); the owner's push offered on the desk once
//! the work passes review, and what came of it; hiring by the owner; and documents members read. Every rule is code: branch names, who is woken, what closes.

use serde_json::json;

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, DeskItem, Run, TeamStore, WorkerAdapter};
use crate::domain::wing::{self, IN_PROGRESS, WING_GOAL};
use crate::domain::{
    A_MEMBERS_SHARE, Brain, Branch, CallReceipt, Check, Document, EventKind, GoalId, MemberId,
    Refusal, RunId, TaskId, TaskStatus, WakeReason,
};

/// The most a document holds: the diff shown (64 KB) and its stat.
pub const DOCUMENT_MOST: usize = 72 * 1024;

/// How much of a document a turn reads at a time.
pub const DOCUMENT_PART: usize = 16_000;

fn document_part(document: &Document, part: usize) -> String {
    let chars: Vec<char> = document.text.chars().collect();
    let parts = chars.len().div_ceil(DOCUMENT_PART).max(1);
    let part = part.clamp(1, parts);
    let from = (part - 1) * DOCUMENT_PART;
    let text: String = chars[from..(from + DOCUMENT_PART).min(chars.len())]
        .iter()
        .collect();
    format!(
        "Document d{} on task #{}: {} (part {part} of {parts})\n{text}",
        document.id, document.task, document.title
    )
}

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
    /// Every file it changes (`git diff --name-only`), for HQ's check of its scope; empty from
    /// a node that does not tell them, when they are read from the diff.
    pub changed: Vec<String>,
    /// Files as they are at its head that HQ's mechanical checks read, as `(path, text)`: each
    /// role README it changes and the role's `defaults/main.yml`.
    pub texts: Vec<(String, String)>,
    /// `(path, text)` of each GitHub Actions workflow at its head, for HQ's check of a CI
    /// criterion (ADR 0142, amended 2026-10-31); `None` from a node that does not tell them.
    pub workflows: Option<Vec<(String, String)>>,
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

/// A kept branch a turn may check out (ADR 0115, amended): the head HQ recorded for it, the
/// base it sits on, and the task it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptBranch {
    /// The head HQ recorded: the node hands out the kept submission only at this head.
    pub head: String,
    /// The default branch's head it sits on.
    pub base: String,
    /// The task it was submitted for.
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
    /// The running turn whose `token` submits `s`, and its task, for a branch under `hq/`.
    fn submitting(&self, token: &str, s: &Submission) -> Done<(Run, TaskId)> {
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
        Ok((run, task))
    }

    /// The newest submission of `task`'s work offered as finished: work kept in progress, which
    /// may share its head, never hides it.
    pub(super) fn newest_finished(&self, task: TaskId) -> Done<Option<crate::domain::Branch>> {
        let work = self.work_of(task)?;
        Ok(self
            .store
            .branches()?
            .into_iter()
            .filter(|b| work.contains(&b.task) && b.status != IN_PROGRESS)
            .max_by_key(|b| b.id))
    }

    /// What HQ recorded when the newest finished submission of `task`'s work is already `s`'s
    /// branch at `s`'s full head: that branch, its document and its newest check.
    fn already_recorded(&self, task: TaskId, s: &Submission) -> Done<Option<Recorded>> {
        let Some(newest) = self.newest_finished(task)? else {
            return Ok(None);
        };
        if newest.name != s.branch || newest.head != s.head {
            return Ok(None);
        }
        let title = format!("branch {} (", s.branch);
        let document = self
            .store
            .documents_of(newest.task)?
            .into_iter()
            .filter(|d| d.title.starts_with(&title))
            .map(|d| d.id)
            .max()
            .unwrap_or(0);
        let check = self
            .store
            .checks()?
            .into_iter()
            .filter(|c| c.branch == newest.id)
            .map(|c| c.id)
            .max()
            .unwrap_or(0);
        Ok(Some(Recorded {
            branch: newest.id,
            document,
            check,
        }))
    }

    /// Keeps the branch `s` of `run`'s on `task` as `status`, with its files.
    fn keep_branch(
        &self,
        run: &Run,
        task: TaskId,
        s: &Submission,
        status: &str,
        at: crate::domain::Millis,
    ) -> Done<i64> {
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
            status: status.to_owned(),
            at,
        })?;
        self.keep_branch_files(branch, &s.changed, &s.texts, s.workflows.as_deref())?;
        Ok(branch)
    }

    /// Records a branch the node took from a turn (ADR 0115): the branch, a document with its
    /// stat and diff, a line in the task's thread, and a check job queued for the dev node's
    /// runner. Named by the turn's own token, while it runs.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a token that opens no running turn, a turn on no task, or a
    /// branch not under `hq/`; a store failure.
    pub fn record_branch(&self, token: &str, s: &Submission) -> Done<Recorded> {
        let _held = self.held();
        let (run, task) = self.submitting(token, s)?;
        // One revision is one submission (defect remediation, C): a turn unsure whether its
        // submission arrived (interrupted after the node took it, or before the answer came
        // back) may submit it again, and that records nothing new.
        if let Some(kept) = self.already_recorded(task, s)? {
            return Ok(kept);
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
        let branch = self.keep_branch(&run, task, s, wing::SUBMITTED, now)?;
        if let Some(repo) = self.outside_repo_of_task(task)? {
            return self.record_outside_branch(&run, task, s, &repo, branch, document, &title);
        }
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
                 dev node (ansible syntax, ansible-lint, yamllint), judged against develop: it \
                 passes when no check is worse than on develop. Its result comes back as a \
                 receipt.",
                short(&s.head)
            ),
        )?;
        // The push the owner is offered is of the newest head, and the tester and the lead
        // judge one head: a new head voids the review, and the gate waits for its own check
        // (ADR 0115, amended).
        self.store.set_gate(task, false)?;
        let reviewed = self.task(task)?;
        if reviewed.status == TaskStatus::InReview {
            self.store.set_task_status(task, TaskStatus::Doing)?;
            self.post_held_lock(
                Author::Endora,
                task,
                &format!(
                    "Sent back: {} was submitted again at {} while this was in review. A new \
                     head needs a new review: submit the task for review again once its check \
                     has passed.",
                    s.branch,
                    short(&s.head)
                ),
            )?;
            self.emit_status(&reviewed, TaskStatus::Doing)?;
        }
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

    /// A branch of the repository outside the lab its item names (ADR 0115, amended 2026-11-01:
    /// round 4's F4), recorded with its document (the lock held). The dev node's check is
    /// ansible's and says nothing of another repository, so none is queued: the item's criteria
    /// are its gate, judged by its reviewer, who checks the branch out and runs its tests. The
    /// branch stays on the node and is never pushed (see [`Self::accept_branch`]).
    #[allow(clippy::too_many_arguments)]
    fn record_outside_branch(
        &self,
        run: &Run,
        task: TaskId,
        s: &Submission,
        repo: &str,
        branch: i64,
        document: i64,
        title: &str,
    ) -> Done<Recorded> {
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "Submitted {title} at {} in {repo}: document d{document}. It is kept on the node \
                 for judging and never pushed anywhere. No check of the lab's applies to it: the \
                 item's criteria are its gate, so check it out and run its own tests.",
                short(&s.head)
            ),
        )?;
        self.store.set_gate(task, true)?;
        self.emit(
            EventKind::BranchSubmitted,
            json!({ "task": task.0, "branch": branch, "name": s.branch, "head": s.head,
                    "files": s.files, "member": run.member.0, "run": run.id.0,
                    "document": document, "repo": repo }),
        )?;
        Ok(Recorded {
            branch,
            document,
            check: 0,
        })
    }

    /// The node kept a turn's work in progress (ADR 0142, amended 2026-10-31): the worker
    /// committed what the sandbox had to the item's branch as the turn ran out of room, and the
    /// node took it by the rules of a submission and keeps it. HQ records the branch as
    /// [`IN_PROGRESS`], so the next turn on the item may check it out and is told to; it writes
    /// no document, queues no check and posts nothing, since the work is not offered as done.
    /// The item's gate is the newest head's, so it is shut until a check of a later head passes;
    /// a head the newest branch already holds leaves it as it is (D23, amended 2026-11-01).
    /// Work in review is refused: a new head would void its review. Returns the branch's id.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a token that opens no running turn on a task, or a branch
    /// not under `hq/`; [`Refusal::InReview`] for work in review; a store failure.
    pub fn keep_work_in_progress(&self, token: &str, s: &Submission) -> Done<i64> {
        let _held = self.held();
        let (run, task) = self.submitting(token, s)?;
        if self.task(task)?.status == TaskStatus::InReview {
            return Err(EngineError::Refused(Refusal::InReview));
        }
        // The gate belongs to the head (D23): keeping the head the newest branch already holds
        // leaves its check's word standing, which no other route could reopen.
        let same_head = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.task == task)
            .max_by_key(|b| b.id)
            .is_some_and(|b| b.head == s.head);
        let branch = self.keep_branch(&run, task, s, IN_PROGRESS, self.clock.now())?;
        if same_head {
            return Ok(branch);
        }
        self.store.set_gate(task, false)?;
        Ok(branch)
    }

    /// A member asks for the newest kept branch of a piece of work to be checked
    /// again (ADR 0115, amended), without submitting it again: a check queued for the branch the
    /// node still keeps, with no sandbox and no new submission, whose result comes back as the
    /// first's did (a receipt, a document, the task's gate). A check was queued only by
    /// submitting from a sandbox that held the branch, which a later turn never does, so a
    /// check that failed for the check box's own reasons cost a recreated branch each time.
    /// One at a time: refused while a check of that branch waits or runs. Returns the check.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`], [`Refusal::OutOfReach`]; [`EngineError::Unknown`] for work with
    /// no submitted branch; [`Refusal::NoLabCheck`] for a repository outside the lab;
    /// [`Refusal::CheckPending`].
    pub fn recheck_in(&self, run: RunId, task: TaskId, why: &str) -> Done<i64> {
        let _held = self.held();
        let turn = self.running(run)?;
        self.reach(&turn, task, false)?;
        let member = self.member(turn.member)?;
        let work = self.work_of(task)?;
        let Some(branch) = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| work.contains(&b.task) && b.status == wing::SUBMITTED)
            .max_by_key(|b| b.id)
        else {
            return Err(EngineError::Unknown("submitted branch on that work"));
        };
        // The lab's check is ansible's, and its box fetches the lab's branches: one queued on
        // an outside branch could only fail, shutting the gate its submission opened (#1048).
        if let Some(repo) = self.outside_repo_of_task(branch.task)? {
            return Err(EngineError::Refused(Refusal::NoLabCheck(repo)));
        }
        if let Some(pending) = self
            .store
            .checks()?
            .into_iter()
            .find(|c| c.branch == branch.id && c.status != "done")
        {
            return Err(EngineError::Refused(Refusal::CheckPending(pending.id)));
        }
        let (check, _) = self.store.add_check(&Check {
            id: 0,
            task: branch.task,
            branch: branch.id,
            run: turn.id,
            token: String::new(),
            status: "waiting".to_owned(),
            ok: None,
            summary: String::new(),
            queued_at: self.clock.now(),
            started_at: None,
            done_at: None,
        })?;
        self.post_held_lock(
            Author::Endora,
            branch.task,
            &format!(
                "Check c{check} is queued again on {} at {}, asked for by {}: {}. No new \
                 submission: the branch the node keeps is checked, judged against develop as \
                 before. Its result comes back as a receipt.",
                branch.name,
                short(&branch.head),
                member.name,
                super::one_line(why, 300)
            ),
        )?;
        // Asking is a step the turn took: it is not a turn that showed nothing.
        if let Some(own) = turn.task {
            self.mark_progress(member.id, own)?;
        }
        Ok(check)
    }

    /// Whether a turn may have a branch the team submitted and the node keeps (ADR 0115,
    /// amended): the node asks, naming the turn by its own token, before it hands a kept
    /// submission out. HQ's record decides, never the name (a name is the model's): the newest
    /// branch recorded under it must be for the turn's own work (its task, the tasks under it,
    /// the tasks above it) or have been submitted by the turn's member. What is handed back is
    /// the head HQ recorded, the base it sits on, and its task: the node checks out that head
    /// only, and refuses it if the kept branch has moved. Two risks are fenced: half-done work
    /// leaking across areas, and one member laundering another's unreviewed branch under a task
    /// of its own.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a token that opens no running turn, or a name HQ never
    /// recorded; [`Refusal::NotYourBranch`] for another task's.
    pub fn checkout_branch(&self, token: &str, name: &str) -> Done<KeptBranch> {
        let _held = self.held();
        let Some(id) = self.store.run_of_token(token)? else {
            return Err(EngineError::Unknown("running turn with that token"));
        };
        let run = self
            .running(id)
            .map_err(|_| EngineError::Unknown("running turn with that token"))?;
        let Some(branch) = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.name == name)
            .max_by_key(|b| b.id)
        else {
            return Err(EngineError::Unknown("branch of that name"));
        };
        if branch.member != run.member && !self.in_the_work_of(&run, branch.task)? {
            return Err(EngineError::Refused(Refusal::NotYourBranch {
                branch: branch.name,
                task: branch.task,
            }));
        }
        Ok(KeptBranch {
            head: branch.head,
            base: branch.base,
            task: branch.task,
        })
    }

    /// Whether `task` is in the work of a turn: the task the turn is on, a task under it at any
    /// depth, or one of the tasks above it. A turn on no task has no work.
    fn in_the_work_of(&self, run: &crate::application::ports::Run, task: TaskId) -> Done<bool> {
        let Some(own) = run.task else {
            return Ok(false);
        };
        if self.work_of(own)?.contains(&task) {
            return Ok(true);
        }
        let tasks = self.store.tasks()?;
        let parent_of = |id: TaskId| tasks.iter().find(|t| t.id == id).and_then(|t| t.parent);
        let (mut above, mut seen) = (parent_of(own), vec![own]);
        while let Some(id) = above {
            if id == task {
                return Ok(true);
            }
            if seen.contains(&id) {
                break;
            }
            seen.push(id);
            above = parent_of(id);
        }
        Ok(false)
    }

    /// The next check job for the runner: the oldest waiting, now running. A check running
    /// longer than [`wing::CHECK_LOST_AFTER_MS`] is given up first, as failed. Nothing while
    /// the kill switch is pulled.
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
        let newer = self.gate_from(check, &branch, ok, &summary)?;
        // The turn that asked for the check: the member who submitted the branch, unless a lead
        // or another developer asked for it to be checked again.
        let asked = self
            .store
            .run(check.run)?
            .map_or(branch.member, |r| r.member);
        let receipt = self.store.add_call_receipt(&CallReceipt {
            id: 0,
            run: check.run,
            member: asked,
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
        let older = newer.as_ref().map_or_else(String::new, |newest| {
            format!(
                " It is the check of an older head: {} was submitted since, and its check is \
                 the gate.",
                short(&newest.head)
            )
        });
        self.post_held_lock(
            Author::Endora,
            check.task,
            &format!(
                "Check c{} on {}: {summary} (receipt r{receipt}, document d{document}).{older}",
                check.id, branch.name
            ),
        )?;
        self.emit(
            EventKind::CheckDone,
            json!({ "task": check.task.0, "check": check.id, "branch": branch.name, "ok": ok,
                    "summary": summary, "receipt": receipt, "document": document,
                    "member": branch.member.0 }),
        )?;
        // Nobody is sent back or woken for a head that has been replaced.
        if newer.is_some() {
            return Ok(());
        }
        let task = self.task(check.task)?;
        if !ok && task.status == TaskStatus::InReview {
            return self.send_back_failed(&task, check, &branch, &summary, document);
        }
        if task.status.finished() {
            return Ok(());
        }
        self.wake_held_lock(branch.member, Some(task.id), WakeReason::CheckDone)?;
        // A lead or another developer who asked for the branch to be checked again hears too.
        if asked != branch.member && self.member(asked)?.at_work() {
            self.wake_held_lock(asked, Some(task.id), WakeReason::CheckDone)?;
        }
        Ok(())
    }

    /// The check is the wing's gate (tests, lint and build, ADR 0114): code work closes only
    /// once it has passed, and the lab's own checks are what passes it. The gate is the newest
    /// head's: a check of an older head that reports late (two heads in flight) is recorded,
    /// and sets nothing (ADR 0115, amended). Returns the newer branch, if there is one.
    fn gate_from(
        &self,
        check: &Check,
        branch: &Branch,
        ok: bool,
        summary: &str,
    ) -> Done<Option<Branch>> {
        let newer = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.task == check.task)
            .max_by_key(|b| b.id)
            .filter(|newest| newest.head != branch.head);
        if newer.is_some() {
            return Ok(newer);
        }
        // In the loops a failed check is one of the item's failures (ADR 0142).
        if !ok {
            self.loop_failed(check.task)?;
        }
        self.store.set_gate(check.task, ok)?;
        self.emit(
            EventKind::GateResult,
            json!({ "task": check.task.0, "passed": ok, "failed": if ok { "" } else { summary },
                    "check": check.id }),
        )?;
        Ok(None)
    }

    /// A branch that fails its checks is sent back (ADR 0115): the tester need not read what
    /// the lab's own checks already refused.
    fn send_back_failed(
        &self,
        task: &crate::domain::Task,
        check: &Check,
        branch: &Branch,
        summary: &str,
        document: i64,
    ) -> Done<()> {
        self.store.set_task_status(task.id, TaskStatus::Doing)?;
        self.post_held_lock(
            Author::Endora,
            task.id,
            &format!(
                "Sent back: check c{} failed ({summary}). Read document d{document}, put it \
                 right, submit the branch again and then the task. If the line says the \
                 check box could not run, ask for the same branch to be checked again with \
                 recheck, and submit nothing new.",
                check.id
            ),
        )?;
        self.emit_status(task, TaskStatus::Doing)?;
        self.wake_held_lock(branch.member, Some(task.id), WakeReason::SentBack)?;
        Ok(())
    }

    /// Once a task with a submitted branch is done: the push is offered on the owner's desk,
    /// saying the branch, its size and how its checks went; or, when the node has no deploy
    /// key, that pushing is not set up and where the bundle is. Called when a task reaches
    /// done; nothing for a task with no branch, or one offered already.
    pub fn offer_push(&self, task: TaskId) -> Done<()> {
        let Some(branch) = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.task == task && b.status == wing::SUBMITTED)
            .max_by_key(|b| b.id)
        else {
            return Ok(());
        };
        // No card unless the owner put them back: listed, and pushed by rule (ADR 0115,
        // amended 2026-10-31). A branch of a repository outside the lab is listed, never carded.
        if !self.cards_back()? || self.outside_repo_of_task(task)?.is_some() {
            return self.accept_branch(branch);
        }
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

    /// The owner hires a member: a name and its model (ADR 0142). Nobody reports to anyone. Said in the feed as the owner's.
    ///
    /// # Errors
    /// [`Refusal::NotAName`], [`Refusal::NotABrain`]; a store failure.
    pub fn hire_by_owner(&self, name: &str, brain: &str) -> Done<MemberId> {
        self.hire_by_owner_with(name, Some(brain))
    }

    /// As [`Self::hire_by_owner`], the brain named or not: one not named is the house's model.
    ///
    /// # Errors
    /// As [`Self::hire_by_owner`].
    pub fn hire_by_owner_with(&self, name: &str, brain: Option<&str>) -> Done<MemberId> {
        let _held = self.held();
        self.hire_held_lock(name, brain, "owner")
    }

    /// Hires a member (the lock held), on the brain named. A blank name or brain is none.
    fn hire_held_lock(&self, name: &str, brain: Option<&str>, by: &str) -> Done<MemberId> {
        let members = self.store.members()?;
        let name = super::one_line(name, 41);
        let taken = members.iter().any(|m| m.name.eq_ignore_ascii_case(&name));
        if name.is_empty() || name.chars().count() > 40 || taken {
            return Err(EngineError::Refused(Refusal::NotAName));
        }
        let named = brain
            .filter(|b| !b.trim().is_empty())
            .map(Brain::named)
            .transpose()
            .map_err(EngineError::Refused)?;
        let look = u8::try_from(members.len() % 6).unwrap_or(0);
        let id = self.store.add_member(&name, A_MEMBERS_SHARE, 2, look)?;
        if let Some(brain) = named {
            self.store.set_brain(id, brain)?;
        }
        let hired = self.member(id)?;
        self.emit(
            EventKind::MemberHired,
            json!({ "member": id.0, "name": name, "brain": hired.brain.name(), "by": by }),
        )?;
        Ok(id)
    }

    /// The members of the wing: whoever leads one of its projects, and whoever reports up to
    /// them.
    pub fn wing_members(&self) -> Done<Vec<MemberId>> {
        // With nobody reporting to anyone (ADR 0142), the wing is its projects' leads.
        let leads = self.wing_leads()?;
        Ok(self
            .store
            .members()?
            .iter()
            .filter(|m| leads.contains(&m.id))
            .map(|m| m.id)
            .collect())
    }

    /// What a member of the wing is told about the fork's own rules: its `AGENTS.md` is the
    /// repository's rules when it exists, and one is proposed when it is missing; its lead is
    /// told the draft in Endora's library is the first branch to propose (ADR 0116). Nothing
    /// for anyone outside the wing, nor on an item in a repository outside the lab (ADR 0115,
    /// amended 2026-11-05), whose briefing names that repository instead.
    pub(super) fn wing_rules(
        &self,
        member: &crate::domain::Member,
        task: Option<TaskId>,
    ) -> Done<&'static str> {
        if !self.wing_members()?.contains(&member.id) {
            return Ok("");
        }
        if let Some(task) = task {
            if self.outside_repo_of_task(task)?.is_some() {
                return Ok("");
            }
        }
        Ok(wing::FORK_RULES)
    }

    /// Whether a member changes the lab (ADR 0120): a lead or a developer in the engineering
    /// wing, who may submit a branch and ask for it to be checked again. What the node asks
    /// before it offers or runs a submission, so nothing is kept for anyone else.
    pub fn changes_the_lab(&self, member: MemberId) -> Done<bool> {
        // Any member at work may change the lab (ADR 0142): checks and review stop bad work.
        Ok(self.member(member)?.at_work())
    }

    pub(super) fn wing_leads(&self) -> Done<Vec<MemberId>> {
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
        if !reach.contains(&document.task) {
            return Err(EngineError::Refused(Refusal::NotADocument));
        }
        self.inherit_input_from(run, document.run)?;
        Ok(document_part(&document, part))
    }

    /// A task's documents and branches, for its briefing: a line each, or nothing.
    /// Carries each task document's writer's mark to a turn shown their titles (ADR 0138 A2c):
    /// a document no turn wrote fails closed.
    pub(super) fn documents_reached(&self, run: RunId, task: TaskId) -> Done<()> {
        for document in self.store.documents_of(task)? {
            self.inherit_input_from(run, document.run)?;
        }
        Ok(())
    }

    /// The receipt that wrote document `id`, for a member who cited the document: the submit
    /// whose result names it (`document d241`), or the check it reports (`check c7: …`), both
    /// kept by the turn the document came from. None for a document no receipt names.
    ///
    /// # Errors
    /// A store failure.
    pub fn receipt_of_document(&self, id: i64) -> Done<Option<i64>> {
        let Some(document) = self.store.document(id)? else {
            return Ok(None);
        };
        let Some(run) = document.run else {
            return Ok(None);
        };
        let named = format!("document d{id}");
        let names = |text: &str| {
            text.match_indices(&named)
                .any(|(at, _)| !text[at + named.len()..].starts_with(|c: char| c.is_ascii_digit()))
        };
        let check = document
            .title
            .strip_prefix("check ")
            .and_then(|t| t.split(':').next())
            .map(|c| format!("{c} "));
        Ok(self
            .store
            .call_receipts_of(run)?
            .into_iter()
            .filter(|r| r.task == Some(document.task))
            .find(|r| {
                names(&r.excerpt)
                    || check
                        .as_deref()
                        .is_some_and(|c| r.tool == "check" && r.args.starts_with(c))
            })
            .map(|r| r.id))
    }

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

    /// A task's documents, as the HQ screen shows them.
    pub(super) fn documents_shown(&self, task: TaskId) -> Done<serde_json::Value> {
        let documents: Vec<serde_json::Value> = self
            .store
            .documents_of(task)?
            .iter()
            .map(|d| json!({ "id": d.id, "title": d.title, "at": d.at, "text": d.text }))
            .collect();
        Ok(json!(documents))
    }

    /// The wing as the HQ screen shows it: its goal, its projects' leads and its members.
    pub(super) fn wing_shown(&self) -> Done<serde_json::Value> {
        let members = self.store.members()?;
        let leads = self.wing_leads()?;
        let ids = self.wing_members()?;
        let wing: Vec<&crate::domain::Member> =
            members.iter().filter(|m| ids.contains(&m.id)).collect();
        Ok(json!({
            "goal": WING_GOAL,
            "leads": leads.iter().map(|l| l.0).collect::<Vec<_>>(),
            "members": wing.iter().map(|m| m.id.0).collect::<Vec<_>>(),
        }))
    }
}

/// A task's branches as the HQ screen shows them, each with its latest check, from the
/// branches and checks read once for the whole board.
pub(super) fn branches_shown(
    task: TaskId,
    branches: &[wing::Branch],
    checks: &[wing::Check],
) -> serde_json::Value {
    let shown: Vec<serde_json::Value> = branches
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
    json!(shown)
}
