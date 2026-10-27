//! The check after a deploy (ADR 0115, amended): a card on the owner's desk once a branch is
//! pushed; the owner's Deployed hands the routine that takes the home lab's alarms its member a
//! check, in that routine's goal and project; that check's turn reads the lab and nothing more;
//! and when its one turn is over, its verdict goes to the change's thread and the owner's desk.

use serde_json::{Value, json};

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, DeskItem, NewTask, Run, TeamStore, WorkerAdapter};
use crate::domain::closure::{Closure, DropWhy};
use crate::domain::deploy::{self, Verdict};
use crate::domain::{Branch, EventKind, Priority, RunId, TaskId, TaskStatus};

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The card that asks the owner to say when a pushed branch is deployed.
    pub(super) fn offer_deploy(&self, task: TaskId, branch: &Branch) -> Done<()> {
        // In the loops there is no Deployed card (ADR 0142): the owner merges and runs the
        // playbook, and the lab's recurring health check is the check.
        if !self.loop_members()?.is_empty() {
            return Ok(());
        }
        let item = self.store.add_desk(
            "deploy",
            Some(task),
            None,
            Some(branch.member),
            &deploy::card(&branch.name),
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": item.0, "kind": "deploy", "task": task.0, "branch": branch.name }),
        )?;
        Ok(())
    }

    /// The owner's word on a deploy card. Deployed: the routine that takes the home lab's alarms
    /// hands its member one check, in its goal and project, without touching the routine's own
    /// task, which stays its alarms'. Won't deploy: the branch says so, and nothing is made. The
    /// card is answered once: the desk takes no second word on it.
    pub(super) fn decide_deploy(&self, found: &DeskItem, deployed: bool) -> Done<()> {
        let task = found.task.ok_or(EngineError::Unknown("task"))?;
        let mut branch = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.task == task && b.status == "pushed")
            .max_by_key(|b| b.id)
            .ok_or(EngineError::Unknown("pushed branch"))?;
        if !deployed {
            branch.status = "not_deployed".to_owned();
            self.store.save_branch(&branch)?;
            return self.post_held_lock(
                Author::Endora,
                task,
                &format!(
                    "The owner will not deploy {}: no check is made.",
                    branch.name
                ),
            );
        }
        let routine = self
            .store
            .routines()?
            .into_iter()
            .find(|r| r.enabled && r.on_lab_trouble)
            .ok_or(EngineError::Unknown(
                "routine that takes the home lab's alarms",
            ))?;
        let change = self.task(task)?;
        let check = self.add_task_held_lock(
            Author::Endora,
            NewTask {
                goal: routine.goal,
                project: routine.project,
                parent: None,
                title: deploy::title(&branch.name),
                description: deploy::description(&branch.name, task.0, &change.criteria),
                assignee: Some(routine.assignee),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )?;
        self.store
            .set_setting(&mark(deploy::MARK, check), &branch.id.to_string())?;
        self.store.set_criteria(check, &[deploy::criterion()])?;
        branch.status = "deployed".to_owned();
        self.store.save_branch(&branch)?;
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "The owner deployed {}: the lab is checked once, in task #{}.",
                branch.name, check.0
            ),
        )?;
        self.emit(
            EventKind::BranchDeployed,
            json!({ "task": task.0, "check": check.0, "branch": branch.name,
                    "routine": routine.id.0, "member": routine.assignee.0 }),
        )?;
        Ok(())
    }

    /// The branch a task checks after its deploy, if it is a deploy check.
    pub(super) fn deploy_check_of(&self, task: TaskId) -> Done<Option<i64>> {
        Ok(self
            .store
            .setting(&mark(deploy::MARK, task))?
            .and_then(|v| v.parse().ok()))
    }

    /// Whether a deploy check has had its turn and given its verdict.
    pub(super) fn deploy_check_given(&self, task: TaskId) -> Done<bool> {
        Ok(self.store.setting(&mark(deploy::GIVEN, task))?.is_some())
    }

    /// Whether a turn is a deploy check's.
    pub(crate) fn in_deploy_check(&self, run: RunId) -> Done<bool> {
        let Some(task) = self.store.run(run)?.and_then(|r| r.task) else {
            return Ok(false);
        };
        Ok(self.deploy_check_of(task)?.is_some())
    }

    /// The node's tools a turn may use, when HQ narrows them: a deploy check's turn has the
    /// lab's reads alone ([`deploy::LAB_READS`]), and a turn on the work of a project that lists
    /// its tools has that list (the Holodeck plan's phase 4; the node uses the names that are
    /// its own). Explicit assistance and its result verification receive an empty list.
    /// `None` for other turns, which have what their role and brain give them.
    ///
    /// # Errors
    /// A store failure.
    pub fn lab_tools_of(&self, run: RunId) -> Done<Option<Vec<String>>> {
        if self.in_deploy_check(run)? {
            return Ok(Some(
                deploy::LAB_READS.iter().map(|t| (*t).to_owned()).collect(),
            ));
        }
        Ok(self.project_tools_of_run(run)?.map(|(_, tools)| tools))
    }

    /// Whether a turn may be given a shell where the runner has one: not a deploy check's,
    /// which reads the lab through the node and nothing else, nor an explicit assistance or
    /// verification turn, nor one on a project's work whose list does not name the shell.
    ///
    /// # Errors
    /// A store failure.
    pub fn may_use_a_shell(&self, run: RunId) -> Done<bool> {
        if self.in_deploy_check(run)? {
            return Ok(false);
        }
        Ok(self
            .project_tools_of_run(run)?
            .is_none_or(|(_, tools)| tools.iter().any(|t| t == crate::domain::projects::SHELL)))
    }

    /// Why a deploy check's turn is refused one of HQ's tools, if it is: anything but its four,
    /// and a move of its task to anything but done or blocked.
    ///
    /// # Errors
    /// A store failure.
    pub fn deploy_check_refuses(
        &self,
        run: RunId,
        tool: &str,
        args: &Value,
    ) -> Done<Option<String>> {
        if !self.in_deploy_check(run)? {
            return Ok(None);
        }
        if !deploy::HQ_TOOLS.contains(&tool) {
            return Ok(Some(deploy::refused(tool)));
        }
        if tool == "set_status" && !matches!(args["status"].as_str(), Some("done" | "blocked")) {
            return Ok(Some(deploy::refused(
                "set_status to anything but done or blocked",
            )));
        }
        Ok(None)
    }

    /// When a deploy check's turn is over, once: its verdict. Healthy only when the check was
    /// closed done, which the evidence rule allows on a reading of the turn's own; a problem when
    /// it was set blocked, in the member's words; otherwise inconclusive. A check that is not
    /// healthy is closed, so nothing wakes it again; the verdict goes in the change's thread and
    /// on the owner's desk (the digest for healthy, a card that interrupts for anything else).
    pub(super) fn after_a_deploy_check(&self, run: &Run) -> Done<()> {
        let Some(task) = run.task else {
            return Ok(());
        };
        let Some(branch) = self.deploy_check_of(task)? else {
            return Ok(());
        };
        let given = mark(deploy::GIVEN, task);
        if self.store.setting(&given)?.is_some() {
            return Ok(());
        }
        let check = self.task(task)?;
        let branch = self
            .store
            .branches()?
            .into_iter()
            .find(|b| b.id == branch)
            .ok_or(EngineError::Unknown("branch"))?;
        let verdict = match check.status {
            TaskStatus::Done => Verdict::Healthy(self.readings_of(run)?),
            TaskStatus::Blocked => Verdict::Problem(self.blocked_because(task)?),
            _ if run.end_reason.trim().is_empty() => {
                Verdict::Inconclusive("its turn ended".to_owned())
            }
            _ => Verdict::Inconclusive(run.end_reason.clone()),
        };
        self.store.set_setting(&given, verdict.desk_kind())?;
        if !matches!(verdict, Verdict::Healthy(_)) && !check.status.finished() {
            self.store.set_task_status(task, TaskStatus::Dropped)?;
            self.emit_status(&check, TaskStatus::Dropped)?;
            self.closed(
                task,
                Closure::Dropped(DropWhy::BlockedForever),
                None,
                "endora",
            )?;
            self.after_finishing(&check)?;
        }
        let said = verdict.said(&branch.name);
        self.post_held_lock(Author::Endora, task, &said)?;
        self.post_held_lock(Author::Endora, branch.task, &said)?;
        let kind = verdict.desk_kind();
        let item = self.store.add_desk(
            kind,
            Some(branch.task),
            None,
            Some(run.member),
            &said,
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": item.0, "kind": kind, "task": branch.task.0, "branch": branch.name }),
        )?;
        Ok(())
    }

    /// The readings a turn took that the node kept, as cited: `r12 homelab_status`.
    fn readings_of(&self, run: &Run) -> Done<String> {
        let said: Vec<String> = self
            .store
            .call_receipts_of(run.id)?
            .into_iter()
            .filter(|r| r.ok && !r.act && !r.sandbox)
            .map(|r| format!("r{} {}", r.id, r.tool))
            .collect();
        Ok(said.join(", "))
    }

    /// The member's words for setting its check blocked, from the thread.
    fn blocked_because(&self, task: TaskId) -> Done<String> {
        const BLOCKED: &str = "→ blocked: ";
        Ok(self
            .store
            .thread(task)?
            .into_iter()
            .rev()
            .find_map(|p| {
                p.text
                    .split_once(BLOCKED)
                    .map(|(_, why)| why.lines().next().unwrap_or("").trim().to_owned())
            })
            .unwrap_or_else(|| "blocked, with no reason given".to_owned()))
    }
}

/// A setting's key for a task.
fn mark(prefix: &str, task: TaskId) -> String {
    format!("{prefix}{}", task.0)
}
