//! Which of the node's tools a turn is given or refused, when HQ narrows them: a project's own
//! list, and the lab's acts on a task whose words leave the lab as it is. And which items'
//! sandboxes the runner goes on keeping between their turns.

use super::{Done, Engine};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::lab_tools;
use crate::domain::{RunId, TaskId};

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The node's tools a turn may use, when HQ narrows them: a turn on the work of a project
    /// that lists its tools has that list (the Holodeck plan's phase 4; the node uses the names
    /// that are its own). `None` for other turns, which have what their brain gives them.
    pub fn lab_tools_of(&self, run: RunId) -> Done<Option<Vec<String>>> {
        // A downtime turn is a chess move: none of the node's tools (30 Oct).
        if self.in_downtime(run)? {
            return Ok(Some(Vec::new()));
        }
        Ok(self.project_tools_of_run(run)?.map(|(_, tools)| tools))
    }

    /// The node's tools a turn is refused whatever else it may use: the lab's acts, when its
    /// task's words say the lab is left as it is (two read-only tasks restarted containers in
    /// the baseline, 28-29 Oct). Empty otherwise.
    pub fn lab_tools_refused(&self, run: RunId) -> Done<Vec<String>> {
        let Some(task) = self.store.run(run)?.and_then(|r| r.task) else {
            return Ok(Vec::new());
        };
        let task = self.task(task)?;
        let mut words = format!("{}\n{}", task.title, task.description);
        for criterion in &task.criteria {
            words.push('\n');
            words.push_str(&criterion.text);
        }
        if !lab_tools::leaves_the_lab_alone(&words) {
            return Ok(Vec::new());
        }
        Ok(lab_tools::LAB_ACTS
            .iter()
            .map(|t| (*t).to_owned())
            .collect())
    }

    /// Whether a turn may be given a shell where the runner has one: not one on a project's
    /// work whose list does not name the shell.
    pub fn may_use_a_shell(&self, run: RunId) -> Done<bool> {
        Ok(self
            .project_tools_of_run(run)?
            .is_none_or(|(_, tools)| tools.iter().any(|t| t == crate::domain::projects::SHELL)))
    }

    /// Of the items whose sandboxes the runner keeps between their turns (ADR 0142, amended
    /// 2026-11-01), those to go on keeping: the ones that exist and are open. A done, dropped
    /// or blocked item's sandbox is let go, and so is one HQ has no task for.
    pub fn sandboxes_to_keep(&self, items: &[TaskId]) -> Done<Vec<TaskId>> {
        let mut keep = Vec::new();
        for item in items {
            if let Some(task) = self.store.task(*item)? {
                if task.status.keeps_its_sandbox() {
                    keep.push(*item);
                }
            }
        }
        Ok(keep)
    }
}
