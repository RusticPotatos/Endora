//! Typed identifiers and time.
//!
//! These now live in the shared [`endora_kernel`] so every bounded context
//! speaks the same id/time vocabulary; this module re-exports them so the domain
//! layer's own paths (`crate::ids::…`) and the crate's public API are unchanged.
//! See `docs/adr/0026-package-by-bounded-context.md`.

pub use endora_kernel::ids::*;
