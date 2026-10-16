//! The team's application layer: the engine and the ports it needs.

pub mod engine;
pub mod ports;

pub use engine::{Author, Engine, EngineError, Settings, TickReport};
pub use ports::{Clock, NewTask, RunContext, TeamStore, WorkerAdapter};

#[cfg(test)]
mod engine_tests;
