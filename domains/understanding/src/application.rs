//! Understanding application layer — repository ports for beliefs, preferences,
//! outcomes and intentions.

use endora_kernel::RepositoryError;
use endora_kernel::ids::{IntentionId, OutcomeId, PreferenceId};

use crate::domain::{Intention, Outcome, Preference, ResearchAssignment};

use crate::domain::PagePath;

/// A page as the brain lists it (ADR 0101).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageInfo {
    /// Where it lives, such as `hobbies/watches`.
    pub path: PagePath,
    /// Its `# ` title.
    pub title: String,
    /// The first line of its summary.
    pub line: String,
}

/// One change in a page's history, newest first when listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageChange {
    /// When, as the history records it (ISO 8601).
    pub at: String,
    /// Why, in a few words: "noted from chat", "edited by you".
    pub why: String,
}

/// What Endora knows about the person, kept as markdown pages in a folder with its own
/// history (ADR 0101). Every change is one commit, and anything edited outside Endora is
/// committed first, so nothing the person wrote is ever overwritten unseen.
pub trait Brain {
    /// A page's text, or `None` if there is no such page.
    ///
    /// # Errors
    /// [`RepositoryError`] if the folder cannot be read.
    fn read(&self, path: &PagePath) -> Result<Option<String>, RepositoryError>;

    /// Every page, memory included, in name order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the folder cannot be read.
    fn pages(&self) -> Result<Vec<PageInfo>, RepositoryError>;

    /// The index: every page but memory, with its one line.
    ///
    /// # Errors
    /// [`RepositoryError`] if the folder cannot be read.
    fn index(&self) -> Result<String, RepositoryError>;

    /// Changes one page under the brain's lock, so nothing can write between the read
    /// and the write. `edit` is given the page's current text (`None` if there is none)
    /// and returns the new text, or `None` to leave it alone. Returns whether it changed.
    ///
    /// # Errors
    /// [`RepositoryError`] if the folder or its history cannot be written.
    fn change(
        &self,
        path: &PagePath,
        when: &str,
        why: &str,
        edit: &mut dyn FnMut(Option<&str>) -> Option<String>,
    ) -> Result<bool, RepositoryError>;

    /// Removes a page; its history keeps it. Memory cannot be removed. Returns whether
    /// there was a page to remove.
    ///
    /// # Errors
    /// [`RepositoryError`] if the folder or its history cannot be written.
    fn delete(&self, path: &PagePath, when: &str, why: &str) -> Result<bool, RepositoryError>;

    /// What happened to a page, newest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the history cannot be read.
    fn history(&self, path: &PagePath) -> Result<Vec<PageChange>, RepositoryError>;

    /// Deletes every page **and the history**, and starts an empty brain. Forgetting
    /// that left the old pages one `git checkout` away would not be forgetting.
    ///
    /// # Errors
    /// [`RepositoryError`] if the folder cannot be emptied or started again.
    fn forget_everything(&self) -> Result<(), RepositoryError>;
}

/// Persists research owned by a user-requested intention (ADR 0077).
pub trait ResearchRepository {
    /// Updates mutable lifecycle state for existing research.
    fn save_research(&self, work: &ResearchAssignment) -> Result<(), RepositoryError>;
    /// Fetches research by its owning intention.
    fn research(&self, id: IntentionId) -> Result<Option<ResearchAssignment>, RepositoryError>;
    /// Returns the one nonterminal user assignment, if any.
    fn active_research(&self) -> Result<Option<ResearchAssignment>, RepositoryError>;
    /// Returns all research, most recently updated first.
    fn list_research(&self) -> Result<Vec<ResearchAssignment>, RepositoryError>;
}

/// Persists and retrieves [`Intention`]s — what Endora is pursuing (ADR 0052).
///
/// [`active`](Self::active) is the load-bearing query: **at most one** intention is
/// active at a time, so this is a cursor rather than a queue, and there is no backlog
/// for it to become.
pub trait IntentionRepository {
    /// Inserts an intention, or replaces the one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, intention: &Intention) -> Result<(), RepositoryError>;

    /// Fetches an intention by id, `None` if absent.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: IntentionId) -> Result<Option<Intention>, RepositoryError>;

    /// The one Endora is currently pursuing, if any.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn active(&self) -> Result<Option<Intention>, RepositoryError>;

    /// Every intention, most recently moved first — including finished ones, so the
    /// person can see what Endora has pursued and dropped.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list(&self) -> Result<Vec<Intention>, RepositoryError>;

    /// Deletes every intention Endora formed from a belief, and says how many there were.
    /// Research the person asked for is a different origin and is not touched.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn forget_its_own(&self) -> Result<usize, RepositoryError>;
}

/// Persists and retrieves [`Outcome`]s — what happened after Endora acted (ADR 0053).
///
/// The counterpart to the [`Brain`]: one holds what Endora knows about the person, the
/// other what it did and what the world looked like afterwards.
pub trait OutcomeRepository {
    /// Inserts an outcome, or replaces the one with the same id (record + react).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, outcome: &Outcome) -> Result<(), RepositoryError>;

    /// Fetches an outcome by id, `None` if absent.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: OutcomeId) -> Result<Option<Outcome>, RepositoryError>;

    /// Lists outcomes, most recent first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list(&self) -> Result<Vec<Outcome>, RepositoryError>;
}

/// Persists and retrieves [`Preference`]s — durable things the person wants the
/// butler to keep in mind.
pub trait PreferenceRepository {
    /// Inserts a preference, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, preference: &Preference) -> Result<(), RepositoryError>;

    /// Lists all preferences, oldest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_all(&self) -> Result<Vec<Preference>, RepositoryError>;

    /// Permanently removes a preference.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn delete(&self, id: PreferenceId) -> Result<(), RepositoryError>;
}
