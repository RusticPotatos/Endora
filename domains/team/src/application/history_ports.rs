//! What the engine needs from the store for the team's history (ADR 0129): the conversations of
//! turns, kept for a while. Only [`HistoryStore::forget_transcripts`] ever removes anything, and
//! only what the retention rule names.

use super::ports::Stored;
use crate::domain::RunId;
use crate::domain::consult::Consult;
use crate::domain::history::{Kept, Transcript};
use crate::domain::record::{Passage, Query};

/// The history's part of HQ's store.
pub trait HistoryStore {
    /// Persist that HQ supplied Confidential history to this turn, before returning it.
    /// # Errors
    /// A store failure.
    fn mark_confidential_input(&self, run: RunId) -> Stored<()>;

    /// Whether this turn read Confidential tool results or inherited Confidential history.
    /// # Errors
    /// A store failure.
    fn run_read_confidential(&self, run: RunId) -> Stored<bool>;

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

    /// The passages of HQ's record that hold any of a query's words, newest first from each
    /// kind of record (posts, handover notes, pages and timelines, stories, notebooks, turns'
    /// conversations). Nothing is chosen, ranked or hidden here: that is the domain's.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn search_record(&self, query: &Query) -> Stored<Vec<Passage>>;

    /// Keeps a consult asked: its id is returned.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn add_consult(&self, consult: &Consult) -> Stored<i64>;

    /// Every consult, oldest first.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn consults(&self) -> Stored<Vec<Consult>>;

    /// Keeps a consult's new standing: its status, the turn that answered, why, whether its
    /// asker has been woken.
    ///
    /// # Errors
    /// [`super::ports::StoreError`] if the store fails.
    fn save_consult(&self, consult: &Consult) -> Stored<()>;
}
