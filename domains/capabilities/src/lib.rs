//! # Capabilities context
//!
//! The butler's **skills** and everything that governs running them: the skill
//! catalog and the [`RegistryRunner`] that lists and executes them behind the
//! policy gate (models propose, policy authorizes — ADRs 0019/0020), the egress
//! guard (SSRF protection + outbound-secret tripwire, ADR 0051), the autonomy
//! envelope (ADR 0051), and the ports for a skill's settings, enablement, and the
//! optional deep-model escalation.
//!
//! Layered inward: [`domain`] (the pure [`AutonomyEnvelope`]), [`application`]
//! (ports), [`infrastructure`] (the runner, the concrete skills, the egress
//! guard). Depends only on the shared kernel. See ADR 0050.

#![forbid(unsafe_code)]

pub mod application;
pub mod domain;
pub mod home_assistant;
pub mod infrastructure;
pub mod mcp_http;
pub mod mcp_registry;
pub mod mcp_stdio;
pub mod store;
pub mod target_search;

pub use application::{
    AutonomyEnvelope, AutonomyEnvelopeRepository, ButlerModelConfig, ButlerModelConfigRepository,
    CapabilityConfigRepository, CapabilityRunner, CapabilitySettingsRepository, CapabilitySpec,
    CapabilityUse, ConfigWrite, ConfigWriteLog, DeepModel, DeepModelRepository, McpServer,
    McpServerRegistry, McpTransport, ModelSlot, ModelTuneSchedule, ModelTuneScheduleRepository,
    Sampling, TargetAlias, TargetAliasRepository, WriteKind,
};
pub use home_assistant::{HomeAssistant, paired_server};
pub use infrastructure::{
    AliasRunner, Capability, CapabilityError, CapabilityInfo, CapabilitySettings, CompositeRunner,
    McpClient, McpRunner, McpToolInfo, NativeChannel, OpenerRunner, RegistryRunner,
    ReversibleOnlyRunner, SettingSpec, TargetSearchRunner, WithdrawnRunner, default_capabilities,
    redact_pii_in_value, same_call_as, scan_outbound_secret,
};
pub use mcp_http::HttpMcpClient;
pub use mcp_stdio::StdioMcpClient;
pub use store::{ConfigStore, migrate};
pub use target_search::{
    Candidate, candidates, is_fragment_of, only_real_match, retarget, shortlist, target_fields,
    target_words, target_words_with_kinds,
};
