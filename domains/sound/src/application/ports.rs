//! What the hub needs from the world.

use core::fmt;

use crate::domain::Patchbay;

/// The store failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the store failed: {}", self.0)
    }
}

/// Where the patchbay is kept between runs.
pub trait SoundStore: Send + Sync {
    /// The patchbay as it was last kept, or nothing the first time.
    ///
    /// # Errors
    /// A store that cannot be read, or that holds something that is not a patchbay. Never
    /// "nothing" in that case: starting empty would throw away the owner's setups at the
    /// next save.
    fn load(&self) -> Result<Option<Patchbay>, StoreError>;

    /// Keeps the patchbay, whole.
    ///
    /// # Errors
    /// A store that cannot be written.
    fn save(&self, bay: &Patchbay) -> Result<(), StoreError>;
}
