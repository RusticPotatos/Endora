//! What the engine needs from the store for the team's world (ADR 0122): sections and their
//! revisions, the pages' timelines, stories, handover notes, what each turn was briefed, and
//! the one-time move of the old record into it. Every one of these only adds: a revision, a
//! timeline line and a handover note are never changed or removed (but for the one-time
//! mending of the dates the move got wrong, which keeps what each row said).

use super::ports::Stored;
use crate::domain::situations::Briefed;
use crate::domain::world::{Handover, Section};
use crate::domain::world_migration::TimelineLine;
use crate::domain::{MemberId, Millis, RunId, TaskId};

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

    /// The headlines of a section's revisions as they were written, oldest first, with their
    /// numbers.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn section_revisions(&self, section: i64) -> Stored<Vec<(i64, String)>>;

    /// A page's timeline, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn timeline(&self, page: &str) -> Stored<Vec<TimelineLine>>;

    /// A page's timeline as [`Self::timeline`], each line with the turn that wrote it, if a
    /// turn did (ADR 0138 A2d): what a briefing shows of it is classified by that turn.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn timeline_with_runs(
        &self,
        page: &str,
    ) -> Stored<Vec<(TimelineLine, Option<crate::domain::RunId>)>>;

    /// Adds a line to a page's timeline.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn add_timeline(&self, line: &TimelineLine, run: Option<RunId>) -> Stored<i64>;

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
    /// Latest note for this recipient, otherwise the task's latest note. Reads at
    /// most one row from each indexed branch; retained notes stay unchanged.
    fn latest_handover(&self, task: TaskId, member: MemberId) -> Stored<Option<Handover>>;

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
}
