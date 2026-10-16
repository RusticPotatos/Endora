//! Endora HQ's team context (ADR 0102): a team of workers taking a goal forward while the
//! owner watches and decides. Goals and tasks, claiming, wakes and their receipts, runs, the
//! tick, the budget ledger and the event feed, with a simulated runner for tests and the demo.
//!
//! Self-contained on purpose: HQ is its own service with its own database, and talks to the
//! Endora node only over its API.

#![forbid(unsafe_code)]

pub mod application;
pub mod domain;
pub mod infrastructure;
