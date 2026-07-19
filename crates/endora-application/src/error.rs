//! Application-level errors.

use core::fmt;

use endora_domain::DomainError;

use crate::ports::RepositoryError;

/// An error from running a use case.
///
/// Use cases fail either because the input violated a domain rule, because the
/// storage backend failed, or because a referenced entity was not found. This
/// keeps a single error type at the application boundary that interfaces (the
/// node, the CLI) can map to their own representations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    /// The input violated a domain invariant.
    Domain(DomainError),
    /// The storage backend failed, or stored data was corrupt.
    Repository(RepositoryError),
    /// A referenced entity does not exist.
    NotFound {
        /// What kind of entity was missing, e.g. `"direction"`.
        entity: &'static str,
    },
    /// The request was malformed in a way the domain does not model (e.g. an
    /// unrecognized enum value supplied by an interface).
    BadRequest {
        /// A human-readable explanation.
        message: String,
    },
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(e) => write!(f, "{e}"),
            Self::Repository(e) => write!(f, "{e}"),
            Self::NotFound { entity } => write!(f, "{entity} not found"),
            Self::BadRequest { message } => write!(f, "{message}"),
        }
    }
}

impl core::error::Error for AppError {}

impl From<DomainError> for AppError {
    fn from(error: DomainError) -> Self {
        Self::Domain(error)
    }
}

impl From<RepositoryError> for AppError {
    fn from(error: RepositoryError) -> Self {
        Self::Repository(error)
    }
}
