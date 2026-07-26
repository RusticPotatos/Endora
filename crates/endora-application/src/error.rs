//! Application-level errors.
//!
//! [`AppError`] now lives in the shared [`endora_kernel`] so every context's use
//! cases return the same type without a cross-context dependency (ADR 0050). It
//! is re-exported here so `endora_application::AppError` paths are unchanged.
//! The conversion from [`ProposalError`] — a port error owned by this crate —
//! stays here, where the source type is local.

pub use endora_kernel::AppError;

use crate::ports::ProposalError;

impl From<ProposalError> for AppError {
    fn from(error: ProposalError) -> Self {
        Self::Model {
            message: error.to_string(),
        }
    }
}
