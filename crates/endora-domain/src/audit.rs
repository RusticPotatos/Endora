//! Audit & Accountability context.
//!
//! An [`AuditRecord`] is a durable, human-readable note that a consequential
//! decision or action happened — what happened and when. The constitution
//! requires that consequential decisions be auditable; these records exist to
//! protect the user, not to surveil them, and are subject to the same memory
//! rights as any other stored data.

use crate::error::{DomainError, require_non_empty};
use crate::ids::{AuditId, Timestamp};

/// A single entry in the audit trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    id: AuditId,
    at: Timestamp,
    summary: String,
}

impl AuditRecord {
    /// Records that something happened at a caller-supplied time.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `summary` is blank.
    pub fn new(id: AuditId, at: Timestamp, summary: &str) -> Result<Self, DomainError> {
        let summary = require_non_empty("audit.summary", summary)?;
        Ok(Self { id, at, summary })
    }

    /// The record's identifier.
    #[must_use]
    pub const fn id(&self) -> AuditId {
        self.id
    }

    /// When the recorded event happened.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }

    /// The human-readable description of what happened.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

#[cfg(test)]
mod tests {
    use super::AuditRecord;
    use crate::error::DomainError;
    use crate::ids::{AuditId, Timestamp};

    #[test]
    fn record_keeps_its_fields() {
        let at = Timestamp::from_unix_millis(1_700_000_000_000);
        let r =
            AuditRecord::new(AuditId::new(1), at, "policy permitted enacting change 7").unwrap();
        assert_eq!(r.id(), AuditId::new(1));
        assert_eq!(r.at(), at);
        assert_eq!(r.summary(), "policy permitted enacting change 7");
    }

    #[test]
    fn record_rejects_a_blank_summary() {
        assert_eq!(
            AuditRecord::new(AuditId::new(1), Timestamp::from_unix_millis(0), "  "),
            Err(DomainError::EmptyField {
                field: "audit.summary"
            })
        );
    }
}
