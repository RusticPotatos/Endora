//! What the engine needs from the store for the team's history (ADR 0129): the conversations of
//! turns, kept for a while. Only [`HistoryStore::forget_transcripts`] ever removes anything, and
//! only what the retention rule names.

use super::ports::Stored;
use crate::domain::RunId;
use crate::domain::history::{Kept, Transcript};

/// The history's part of HQ's store.
pub trait HistoryStore {
    /// Keeps a turn's conversation. A turn has one.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails, or the turn already has one.
    fn add_transcript(&self, transcript: &Transcript) -> Stored<()>;

    /// A turn's conversation, if HQ kept it and still has it.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn transcript(&self, run: RunId) -> Stored<Option<Transcript>>;

    /// What is kept, without the conversations, oldest first: what the retention rule reads.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn transcripts_kept(&self) -> Stored<Vec<Kept>>;

    /// Lets go of these conversations. Returns how many were kept and are gone.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn forget_transcripts(&self, runs: &[RunId]) -> Stored<usize>;
}
