//! # Scheduling context
//!
//! When the butler acts on its own initiative: the **check-in** cadence (the
//! butler reaching out) and the **daily brief** schedule (a weather/safety/news
//! rundown prepared each day). This context owns the schedule value types and
//! their repositories; the heartbeat-driven orchestration that actually runs a
//! due check-in or brief composes several contexts and lives in the orchestration
//! layer. Depends only on the shared kernel — a leaf context. See ADR 0026.

#![forbid(unsafe_code)]

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::{BriefScheduleRepository, CheckinRepository, NightlyLoopScheduleRepository};
pub use domain::{BriefSchedule, CheckinSchedule, NightlyLoopSchedule};
pub use infrastructure::{ScheduleStore, migrate};
