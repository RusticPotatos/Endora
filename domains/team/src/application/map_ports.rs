//! What the engine needs from the store for the team's map (ADR 0126): the pages that exist for
//! areas, projects, goals and roles. Every one of these only adds.

use super::ports::Stored;
use crate::domain::Millis;
use crate::domain::map::{Gap, PageProposal};

/// The map's part of HQ's store.
pub trait MapStore {
    /// The slugs of the pages made for areas, projects, goals and roles, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn world_pages(&self) -> Stored<Vec<String>>;

    /// Makes a page, if there is not one at `slug`. Returns whether it was made now.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn add_world_page(&self, slug: &str, at: Millis) -> Stored<bool>;

    /// Every proposal to a page, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn proposals(&self) -> Stored<Vec<PageProposal>>;

    /// Keeps a proposal: a new one when its id is 0 (its id is returned), else its new standing.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn save_proposal(&self, proposal: &PageProposal) -> Stored<i64>;

    /// Every gap, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn gaps(&self) -> Stored<Vec<Gap>>;

    /// Keeps a gap: a new one when its id is 0 (its id is returned), else its new count and
    /// closing.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn save_gap(&self, gap: &Gap) -> Stored<i64>;
}
