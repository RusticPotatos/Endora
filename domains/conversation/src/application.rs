//! Conversation application layer — the chat repository port.

use endora_kernel::RepositoryError;

use crate::domain::ChatMessage;

/// Persists and retrieves the conversation with the butler.
pub trait ChatRepository {
    /// Appends a message to the conversation.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn append(&self, message: &ChatMessage) -> Result<(), RepositoryError>;

    /// Lists the conversation, oldest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list(&self) -> Result<Vec<ChatMessage>, RepositoryError>;

    /// What a past reply actually **found** — the outputs of the tools it ran, if any.
    ///
    /// Kept because a turn is seeded from prose alone. Every tool result inside a turn is
    /// threaded properly and then thrown away at its end, so asking a second time starts
    /// from nothing: the butler has its own summary of what it said and no trace of what it
    /// read. Ask the same question twice and it looks twice, or gives up and asks you to say
    /// more.
    ///
    /// The findings were already being stored — for the person, so a reply keeps its
    /// expandable trail after a reload. **They were never given back to the butler.** This
    /// is the read that closes that.
    ///
    /// Empty for a reply that ran nothing, which is most of them.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn what_it_found(&self, message_id: &str) -> Result<Vec<String>, RepositoryError> {
        let _ = message_id;
        Ok(Vec::new())
    }

    /// The messages between two moments, oldest first — `from` inclusive, `to` exclusive.
    ///
    /// What a **console** needs, which is not what the butler needs. A turn reads the
    /// recent window and its running summary; a person reads a day. Sending five years of
    /// conversation to a browser so it can show one afternoon is the sort of thing that
    /// works perfectly until it suddenly does not.
    ///
    /// A **moment range** rather than a date, deliberately: the server stores instants and
    /// has no idea what timezone anyone is in. The caller knows where its own midnight
    /// falls, so it asks for a window and nothing here has to be taught about time.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn between(&self, from_ms: i64, to_ms: i64) -> Result<Vec<ChatMessage>, RepositoryError>;

    /// The local days that have any conversation, with how many messages each holds.
    ///
    /// `offset_minutes` is the caller's distance from UTC, so the grouping lands on *its*
    /// midnights. Returned oldest first as `(YYYY-MM-DD, count)`.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn days(&self, offset_minutes: i64) -> Result<Vec<(String, usize)>, RepositoryError>;
}
