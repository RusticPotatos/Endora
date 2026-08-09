//! Ports through which time and identity enter the pure layers.
//!
//! The domain never reads the system clock and never generates identifiers, so
//! both enter through these traits. Interfaces wire real implementations (a
//! system clock, a random id source); tests wire deterministic ones.

use crate::ids::Timestamp;

/// Supplies the current time to use cases.
///
/// The domain never reads the clock, so time enters through this port. The node
/// wires a real system clock; tests wire a fixed one.
pub trait Clock {
    /// The current instant, as a [`Timestamp`].
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
