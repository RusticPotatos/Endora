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
}
