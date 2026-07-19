//! Conversation context — the dialogue between the person and the butler.
//!
//! The butler is a conversational surface over the learning loop
//! (`docs/adr/0014-the-butler-conversation-values-attention.md`): the person
//! talks, the butler replies and *proposes* structured actions. A [`ChatMessage`]
//! is one turn of that dialogue, stored as ordinary, user-owned memory. The
//! butler never acts from a message; it proposes, and deterministic code
//! authorized by the person executes.

use crate::error::{DomainError, require_non_empty};
use crate::ids::{MessageId, Timestamp};

/// Who authored a [`ChatMessage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    /// The person.
    User,
    /// The butler.
    Butler,
}

impl MessageRole {
    /// A stable, lowercase name for storage and the protocol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Butler => "butler",
        }
    }

    /// Parses a role from its [`name`](Self::name), or `None` if unrecognized.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "user" => Some(Self::User),
            "butler" => Some(Self::Butler),
            _ => None,
        }
    }
}

/// One turn in the conversation between the person and the butler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    id: MessageId,
    role: MessageRole,
    text: String,
    at: Timestamp,
}

impl ChatMessage {
    /// Records a message at a caller-supplied time.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `text` is blank.
    pub fn new(
        id: MessageId,
        role: MessageRole,
        text: &str,
        at: Timestamp,
    ) -> Result<Self, DomainError> {
        let text = require_non_empty("message.text", text)?;
        Ok(Self { id, role, text, at })
    }

    /// The message's identifier.
    #[must_use]
    pub const fn id(&self) -> MessageId {
        self.id
    }

    /// Who authored the message.
    #[must_use]
    pub const fn role(&self) -> MessageRole {
        self.role
    }

    /// The message text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// When the message was recorded.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatMessage, MessageRole};
    use crate::error::DomainError;
    use crate::ids::{MessageId, Timestamp};

    #[test]
    fn role_names_round_trip() {
        for r in [MessageRole::User, MessageRole::Butler] {
            assert_eq!(MessageRole::from_name(r.name()), Some(r));
        }
        assert_eq!(MessageRole::from_name("bogus"), None);
    }

    #[test]
    fn message_keeps_its_fields() {
        let at = Timestamp::from_unix_millis(1_000);
        let m = ChatMessage::new(
            MessageId::new(1),
            MessageRole::User,
            "I want to run more",
            at,
        )
        .unwrap();
        assert_eq!(m.role(), MessageRole::User);
        assert_eq!(m.text(), "I want to run more");
        assert_eq!(m.at(), at);
    }

    #[test]
    fn message_rejects_blank_text() {
        assert_eq!(
            ChatMessage::new(
                MessageId::new(1),
                MessageRole::Butler,
                "   ",
                Timestamp::from_unix_millis(0)
            ),
            Err(DomainError::EmptyField {
                field: "message.text"
            })
        );
    }
}
