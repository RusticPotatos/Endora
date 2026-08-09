//! Understanding application layer — repository ports for beliefs, preferences,
//! outcomes and intentions.

use endora_kernel::RepositoryError;
use endora_kernel::ids::{BeliefId, IntentionId, NotionId, OutcomeId, PreferenceId};

use crate::domain::{Belief, Intention, Notion, Outcome, Preference};

/// Persists and retrieves [`Notion`]s — what Endora is still thinking about (ADR 0057).
///
/// The counterpart to [`BeliefRepository`] one stage earlier: that one holds what Endora has
/// earned the right to believe, this one what it suspects and is still gathering evidence for.
///
/// [`open`](Self::open) is the load-bearing query, because the cap is enforced against it. A
/// notion that matured or died is kept but is no longer thought about, so the working set stays
/// bounded however long the history grows.
pub trait NotionRepository {
    /// Inserts a notion, or replaces the one with the same id (form + support).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, notion: &Notion) -> Result<(), RepositoryError>;

    /// Fetches a notion by id, `None` if absent.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: NotionId) -> Result<Option<Notion>, RepositoryError>;

    /// The ones still being thought about, best-supported first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn open(&self) -> Result<Vec<Notion>, RepositoryError>;

    /// Every notion, most recently supported first — including the ones that matured or
    /// died, so what Endora dropped on its own stays visible rather than vanishing.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list(&self) -> Result<Vec<Notion>, RepositoryError>;
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
}

/// Persists and retrieves [`Outcome`]s — what happened after Endora acted (ADR 0053).
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

/// Keeps the asks the butler failed, so it can notice itself improving (ADR 0075).
///
/// Intent-named, no storage vocabulary: a failure is *filed*, a night's attempt is
/// *recorded*, and what remains to try is *open*. Retirement is derived inside the
/// record calls — passing a replay retires, and so does failing enough of them —
/// never decided by a caller.
pub trait SpecimenRepository {
    /// Files a failed ask. Returns `false` — and files nothing — when the same ask
    /// is already open (asking twice is one problem) or the shelf is full
    /// ([`MOST_SPECIMENS_OPEN`](crate::domain::MOST_SPECIMENS_OPEN)).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn file_specimen(
        &self,
        id: &str,
        asked: &str,
        verdict: &str,
        now_ms: i64,
    ) -> Result<bool, RepositoryError>;

    /// The specimens still worth replaying, oldest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn open_specimens(&self) -> Result<Vec<crate::domain::Specimen>, RepositoryError>;

    /// Records one nightly replay. A pass retires the specimen; so does the
    /// [`REPLAYS_BEFORE_GIVING_UP`](crate::domain::REPLAYS_BEFORE_GIVING_UP)th
    /// failure, because re-asking past that stops being information.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn record_replay(&self, id: &str, passed: bool, now_ms: i64) -> Result<(), RepositoryError>;
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
