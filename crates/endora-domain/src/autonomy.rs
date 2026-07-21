//! Autonomy levels.
//!
//! [`AutonomyLevel`] now lives in the shared [`endora_kernel`] because two
//! bounded contexts speak it (direction's policy and capabilities' envelope);
//! this module re-exports it so the domain layer's own path (`crate::autonomy::…`)
//! and the crate's public API are unchanged. See
//! `docs/adr/0026-package-by-bounded-context.md`.

pub use endora_kernel::autonomy::AutonomyLevel;
