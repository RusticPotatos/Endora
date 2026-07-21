//! Domain errors and shared validation.

use core::fmt;

/// An error produced when a domain rule is violated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// A required text field was empty or contained only whitespace.
    EmptyField {
        /// Name of the offending field, e.g. `"target.statement"`.
        field: &'static str,
    },
    /// A state transition was not allowed from the current state.
    InvalidTransition {
        /// The state transitioned from.
        from: &'static str,
        /// The state that was attempted.
        to: &'static str,
    },
    /// A reflection was created without referencing any observation. Endora
    /// learns from evidence, so a reflection over nothing is not permitted.
    ReflectionWithoutEvidence,
    /// An approval decision was attempted on a proposal that was already decided.
    AlreadyDecided,
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField { field } => write!(f, "`{field}` must not be empty"),
            Self::InvalidTransition { from, to } => {
                write!(f, "cannot transition from `{from}` to `{to}`")
            }
            Self::ReflectionWithoutEvidence => {
                write!(f, "a reflection must cite at least one observation")
            }
            Self::AlreadyDecided => write!(f, "this proposal has already been decided"),
        }
    }
}

impl core::error::Error for DomainError {}

/// Trims `value` and returns it as an owned `String`, or an
/// [`DomainError::EmptyField`] if it is blank. Shared by the entity
/// constructors so "must not be empty" is enforced in exactly one place.
pub fn require_non_empty(field: &'static str, value: &str) -> Result<String, DomainError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(DomainError::EmptyField { field });
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{DomainError, require_non_empty};

    #[test]
    fn require_non_empty_trims_and_accepts() {
        assert_eq!(require_non_empty("f", "  hi  ").unwrap(), "hi");
    }

    #[test]
    fn require_non_empty_rejects_blank() {
        assert_eq!(
            require_non_empty("f", "   "),
            Err(DomainError::EmptyField { field: "f" })
        );
    }
}
