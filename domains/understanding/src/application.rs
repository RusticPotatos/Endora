//! Understanding application layer — repository ports for beliefs, preferences and
//! outcomes.

use endora_kernel::RepositoryError;
use endora_kernel::ids::{BeliefId, OutcomeId, PreferenceId};

use crate::domain::{Belief, Outcome, Preference};

/// Persists and retrieves [`Outcome`]s — what happened after Endora acted (ADR 0035).
///
/// The counterpart to [`BeliefRepository`]: one holds what Endora understands, the other
/// what it did and what the world looked like afterwards.
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

/// Persists and retrieves [`Belief`]s — what the butler currently understands
/// about the person.
pub trait BeliefRepository {
    /// Inserts a belief, or replaces the one with the same id (create + update).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, belief: &Belief) -> Result<(), RepositoryError>;

    /// Fetches a belief by id, `None` if absent.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: BeliefId) -> Result<Option<Belief>, RepositoryError>;

    /// Lists all beliefs, most-recently-affirmed first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list(&self) -> Result<Vec<Belief>, RepositoryError>;
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
