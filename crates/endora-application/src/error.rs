//! Application-level errors.
//!
//! [`AppError`] lives in the shared [`endora_kernel`] so every context's use cases return the
//! same type without a cross-context dependency (ADR 0050). It is re-exported here so
//! `endora_application::AppError` paths are unchanged.

pub use endora_kernel::AppError;
