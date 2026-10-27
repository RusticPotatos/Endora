//! What the engine needs from the store for the owner's desk: what waits on the owner's word, and
//! what it decided.

use super::ports::{DeskItem, Stored};
use crate::domain::{GoalId, MemberId, Millis, ProjectId, TaskId};

/// The owner's desk's part of HQ's store.
pub trait DeskStore {
    /// Says what a desk item is about, when that is a project.
    fn set_desk_project(&self, id: crate::domain::DeskId, project: ProjectId) -> Stored<()>;

    /// Puts something on the owner's desk, about a task or a goal, from a member or not.
    fn add_desk(
        &self,
        kind: &str,
        task: Option<TaskId>,
        goal: Option<GoalId>,
        from: Option<MemberId>,
        text: &str,
        at: Millis,
    ) -> Stored<crate::domain::DeskId>;
    /// Records how the owner decided a desk item.
    fn set_desk_status(&self, id: crate::domain::DeskId, status: &str) -> Stored<()>;
    /// Rewords a desk item that still waits: a finding seen again says what it fired on now
    /// (ADR 0132). An item the owner has decided keeps the words it was decided on.
    fn set_desk_text(&self, id: crate::domain::DeskId, text: &str) -> Stored<()>;
    /// Everything on the desk, oldest first.
    fn desk(&self) -> Stored<Vec<DeskItem>>;
}
