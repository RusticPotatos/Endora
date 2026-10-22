//! Persistent conversation intent; implementations use bounded, indexed reads.

use super::ports::Stored;
use crate::domain::conversation::{Conversation, Information, Message, Operation};
use crate::domain::{MemberId, Millis, ProjectId, RunId};

/// Atomic message acceptance result, including a deterministic policy refusal.
pub type Accepted = Result<i64, &'static str>;

/// The store boundary for information-only conversations.
pub trait ConversationStore {
    /// Mark that the turn's briefing was classified under this owner switch revision.
    /// # Errors
    /// A store failure.
    fn mark_conversation_ready(&self, run: RunId, revision: i64) -> Stored<()>;
    /// Whether the turn's classified briefing matches the current switch revision.
    /// # Errors
    /// A store failure.
    fn conversation_ready(&self, run: RunId, revision: i64) -> Stored<bool>;
    /// Create an owner-authorized scope.
    /// # Errors
    /// A store failure.
    fn add_conversation(
        &self,
        project: ProjectId,
        members: [MemberId; 2],
        at: Millis,
    ) -> Stored<i64>;
    /// Look up one non-deleted scope.
    /// # Errors
    /// A store failure.
    fn conversation(&self, id: i64) -> Stored<Option<Conversation>>;
    /// Read at most PAGE+1 summaries after an ID, optionally for one participant.
    /// # Errors
    /// A store failure.
    fn conversation_page(&self, member: Option<MemberId>, after: i64) -> Stored<Vec<Conversation>>;
    /// Atomically deduplicate, check rate limits, insert, audit and update summary.
    /// # Errors
    /// A store failure; user conflicts and quotas are in the accepted result.
    fn accept_information(&self, message: &Information) -> Stored<Accepted>;
    /// Read at most PAGE+1 messages, using an indexed keyset cursor.
    /// # Errors
    /// A store failure.
    fn message_page(&self, conversation: i64, after: i64) -> Stored<Vec<Message>>;
    /// Record an operation without message content or unauthorized locators.
    /// # Errors
    /// A store failure.
    fn conversation_operation(
        &self,
        run: RunId,
        conversation: Option<i64>,
        operation: &str,
        reason: &str,
        at: Millis,
    ) -> Stored<()>;
    /// Read at most PAGE+1 content-free outcomes for the owner, after a stable ID.
    /// # Errors
    /// A store failure.
    fn conversation_operations_page(&self, after: i64) -> Stored<Vec<Operation>>;
    /// Immediately deny reads; later purging is bounded.
    /// # Errors
    /// A store failure.
    fn delete_conversation(&self, id: i64, at: Millis) -> Stored<bool>;
    /// Remove at most PURGE_BATCH messages/operation rows from one deleted scope.
    /// # Errors
    /// A store failure.
    fn purge_conversation_batch(&self) -> Stored<usize>;
}
