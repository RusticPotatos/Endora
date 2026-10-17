//! The hub: the one patchbay, behind a lock, kept in the store after every change.

use core::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::application::ports::{SoundStore, StoreError};
use crate::domain::{Patchbay, Refusal};

/// Why a change did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubError {
    /// The patchbay refused it.
    Refused(Refusal),
    /// It could not be kept, so it was not made.
    Store(StoreError),
}

impl fmt::Display for HubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(r) => r.fmt(f),
            Self::Store(e) => e.fmt(f),
        }
    }
}

impl From<Refusal> for HubError {
    fn from(r: Refusal) -> Self {
        Self::Refused(r)
    }
}

impl From<StoreError> for HubError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

/// The hub's patchbay and where it is kept.
pub struct Hub<S> {
    store: S,
    bay: Mutex<Patchbay>,
}

impl<S: SoundStore> Hub<S> {
    /// Opens the hub on what the store kept, or an empty patchbay the first time.
    ///
    /// # Errors
    /// A store that cannot be read.
    pub fn open(store: S) -> Result<Self, HubError> {
        let bay = store.load()?.unwrap_or_default();
        Ok(Self {
            store,
            bay: Mutex::new(bay),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Patchbay> {
        // Changes are made on a copy, so whatever the lock holds is whole, even after a
        // panic part way through one.
        self.bay.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes a change: on a copy, kept in the store, and only then live. A refusal or a store
    /// failure leaves the patchbay as it was, and a change that changes nothing is not kept.
    ///
    /// # Errors
    /// What the patchbay refuses, or a store failure.
    pub fn change<T>(
        &self,
        f: impl FnOnce(&mut Patchbay) -> Result<T, Refusal>,
    ) -> Result<T, HubError> {
        let mut bay = self.lock();
        let mut next = bay.clone();
        let out = f(&mut next)?;
        if next != *bay {
            self.store.save(&next)?;
            *bay = next;
        }
        Ok(out)
    }

    /// Reads the patchbay as it is now.
    pub fn read<T>(&self, f: impl FnOnce(&Patchbay) -> T) -> T {
        f(&self.lock())
    }
}
