//! The team's application layer: the engine and the ports it needs.

pub mod engine;
pub mod ports;
pub mod protocol;

pub use engine::{Author, Engine, EngineError, Settings, TickReport};
pub use ports::{Clock, NewTask, RunContext, TeamStore, WorkerAdapter};

#[cfg(test)]
mod engine_tests;

#[cfg(test)]
mod protocol_tests;
