//! What the engine needs from the store for the team's world (ADR 0122): sections and their
//! revisions, the pages' timelines, stories, handover notes, what each turn was briefed, and
//! the one-time move of the old record into it. Every one of these only adds: a revision, a
//! timeline line and a handover note are never changed or removed.

use super::ports::Stored;
use crate::domain::situations::Briefed;
use crate::domain::world::{Handover, Section, Story};
use crate::domain::world_migration::{Plan, TimelineLine};
use crate::domain::{Millis, RunId, TaskId};

/// The world's part of HQ's store.
pub trait WorldStore {
    /// Every section of every written page, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn sections(&self) -> Stored<Vec<Section>>;

    /// Writes a section: a new one when its id is 0 (its id is returned), else its new
    /// revision. Either way the revision is also kept, with `why`, in its append-only history.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails, or the anchor is taken on its page.
    fn save_section(&self, section: &Section, why: &str) -> Stored<i64>;

    /// Notes that a post cited a section.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn touch_section(&self, id: i64, at: Millis) -> Stored<()>;

    /// A page's timeline, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn timeline(&self, page: &str) -> Stored<Vec<TimelineLine>>;

    /// Adds a line to a page's timeline.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn add_timeline(&self, line: &TimelineLine, run: Option<RunId>) -> Stored<i64>;

    /// Every story, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn stories(&self) -> Stored<Vec<Story>>;

    /// Writes a story: a new one when its id is 0 (its id is returned), else the one with its id.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails, or the handle is taken.
    fn save_story(&self, story: &Story) -> Stored<i64>;

    /// Notes that a post cited a story by its handle.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn touch_story(&self, id: i64, at: Millis) -> Stored<()>;

    /// Keeps a handover note.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn add_handover(&self, handover: &Handover) -> Stored<i64>;

    /// A task's handover notes, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn handovers(&self, task: TaskId) -> Stored<Vec<Handover>>;

    /// Records what a turn was briefed (its latest attempt's).
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn save_briefed(&self, briefed: &Briefed) -> Stored<()>;

    /// What a turn was briefed, if it was recorded.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn briefed(&self, run: RunId) -> Stored<Option<Briefed>>;

    /// Applies the move of the old record into the world, once: in one transaction with the
    /// mark that it is done, so a second start changes nothing. Returns whether it was applied
    /// now.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails; nothing is applied then.
    fn migrate_world(&self, plan: &Plan, at: Millis) -> Stored<bool>;
}
