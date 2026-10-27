//! What a turn read, and so what its words carry (ADR 0138, 0140, 0141): a turn that reads
//! another turn's words, or words of unknown origin, is marked as having read Confidential
//! input. Kept when the conversations went (ADR 0142); the marks are always kept now.

use super::{Done, Engine};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::{ProjectId, RunId};

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Carry a source turn's observed classification to this one. A missing source is
    /// unclassified legacy material and fails closed.
    pub(super) fn inherit_input_from(&self, run: RunId, source: Option<RunId>) -> Done<()> {
        let confidential = match source {
            Some(id) => self.store.run_read_confidential(id)?,
            _ => true,
        };
        if confidential {
            self.store.mark_confidential_input(run)?;
        }
        Ok(())
    }

    /// Text without proven source lineage is read as Confidential.
    pub(crate) fn unclassified_message_input(&self, run: RunId) -> Done<()> {
        self.store.mark_confidential_input(run)?;
        Ok(())
    }

    /// Read only the requested tail and record its source tiers before returning text.
    pub(crate) fn latest_task_posts(
        &self,
        run: RunId,
        task: crate::domain::TaskId,
        most: usize,
    ) -> Done<Vec<crate::application::history_ports::PostInput>> {
        let posts = self.store.latest_authored_posts(task, most)?;
        for post in &posts {
            self.inherit_authored_input(run, post.origin)?;
        }
        Ok(posts)
    }

    pub(super) fn inherit_authored_input(
        &self,
        run: RunId,
        origin: crate::domain::history::InputOrigin,
    ) -> Done<()> {
        match origin {
            // HQ's own words borrow nothing, so like the owner's they carry no tier.
            crate::domain::history::InputOrigin::Owner
            | crate::domain::history::InputOrigin::System => Ok(()),
            crate::domain::history::InputOrigin::Run(source) => {
                self.inherit_input_from(run, Some(source))
            }
            crate::domain::history::InputOrigin::Unknown => self.unclassified_message_input(run),
        }
    }

    /// The project a turn is charged to: its task's.
    pub(super) fn project_of_run(
        &self,
        run: &crate::application::ports::Run,
    ) -> Done<Option<ProjectId>> {
        self.project_of(run.task)
    }
}
