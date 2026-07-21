//! # Capabilities context
//!
//! The butler's **skills** and everything that governs running them: the skill
//! catalog and the [`RegistryRunner`] that lists and executes them behind the
//! policy gate (models propose, policy authorizes — ADRs 0019/0020), the egress
//! guard (SSRF protection + outbound-secret tripwire, ADR 0023), the autonomy
//! envelope (ADR 0022), and the ports for a skill's settings, enablement, and the
//! optional deep-model escalation.
//!
//! Layered inward: [`domain`] (the pure [`AutonomyEnvelope`]), [`application`]
//! (ports), [`infrastructure`] (the runner, the concrete skills, the egress
//! guard). Depends only on the shared kernel. See ADR 0026.

#![forbid(unsafe_code)]

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod store;

pub use application::{
    AutonomyEnvelope, AutonomyEnvelopeRepository, ButlerModelConfig, ButlerModelConfigRepository,
    CapabilityConfigRepository, CapabilityRunner, CapabilitySettingsRepository, CapabilitySpec,
    CapabilityUse, DeepModel, DeepModelRepository, ModelSlot, Sampling,
};
pub use infrastructure::{
    Capability, CapabilityError, CapabilityInfo, CapabilitySettings, RegistryRunner, SettingSpec,
    default_capabilities, redact_pii_in_value, scan_outbound_secret,
};
pub use store::{ConfigStore, migrate};
