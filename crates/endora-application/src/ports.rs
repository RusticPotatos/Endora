//! Ports the application defines and the infrastructure layer implements.
//!
//! These are the abstractions the application depends on for persistence. The
//! application never names a concrete storage engine; infrastructure provides
//! adapters (e.g. SQLite — see `docs/adr/0004-sqlite-first.md`). This keeps the
//! dependency direction pointing inward: `Infrastructure -> Application/Domain`.

use core::fmt;

use endora_domain::{Direction, DirectionId, Goal, GoalId, Timestamp};

/// A failure from a storage backend behind a repository port.
///
/// Deliberately free of any engine-specific type: adapters translate their own
/// errors (driver, I/O, corrupt rows) into these variants so the application
/// stays independent of the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryError {
    /// The backend itself failed (connection, I/O, driver error, lock).
    Backend(String),
    /// Stored data could not be reconstituted into a valid domain value.
    Corrupt(String),
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(msg) => write!(f, "storage backend error: {msg}"),
            Self::Corrupt(msg) => write!(f, "corrupt stored data: {msg}"),
        }
    }
}

impl core::error::Error for RepositoryError {}

/// Persists and retrieves [`Direction`]s.
pub trait DirectionRepository {
    /// Inserts a direction, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, direction: &Direction) -> Result<(), RepositoryError>;

    /// Fetches a direction by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: DirectionId) -> Result<Option<Direction>, RepositoryError>;
}

/// Persists and retrieves [`Goal`]s.
pub trait GoalRepository {
    /// Inserts a goal, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, goal: &Goal) -> Result<(), RepositoryError>;

    /// Fetches a goal by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: GoalId) -> Result<Option<Goal>, RepositoryError>;

    /// Lists the goals belonging to a direction, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_direction(&self, direction: DirectionId) -> Result<Vec<Goal>, RepositoryError>;
}

/// Supplies the current time to use cases.
///
/// The domain never reads the clock, so time enters through this port. The node
/// wires a real system clock; tests wire a fixed one.
pub trait Clock {
    /// The current instant, as a domain [`Timestamp`].
    fn now(&self) -> Timestamp;
}

/// Supplies fresh, unique identifier values to use cases.
///
/// The domain never generates identifiers, so they enter through this port. Use
/// cases wrap the raw value in the appropriate typed id. The node wires a random
/// source; tests wire a deterministic one.
pub trait IdSource {
    /// Returns a fresh identifier value, unique within this store's lifetime.
    fn new_id(&self) -> u128;
}
