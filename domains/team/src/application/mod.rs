//! The team's application layer: the engine and the ports it needs.

pub mod engine;
pub mod ports;
pub mod protocol;

pub use engine::{Author, Engine, EngineError, Fired, Settings, TickReport};
pub use ports::{Clock, NewRoutine, NewTask, RunContext, TeamStore, WorkerAdapter};

#[cfg(test)]
mod engine_tests;

#[cfg(test)]
mod protocol_tests;

#[cfg(test)]
mod routines_tests;

#[cfg(test)]
mod approvals_tests;

#[cfg(test)]
mod owner_tests;

#[cfg(test)]
mod stranded_tests;
