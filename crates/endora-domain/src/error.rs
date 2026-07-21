//! Domain errors and shared validation.
//!
//! These now live in the shared [`endora_kernel`]; this module re-exports them so
//! the domain layer's own paths (`crate::error::…`) and the crate's public API
//! are unchanged. See `docs/adr/0026-package-by-bounded-context.md`.

pub use endora_kernel::error::DomainError;
pub(crate) use endora_kernel::error::require_non_empty;
