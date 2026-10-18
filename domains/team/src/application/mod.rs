//! The team's application layer: the engine and the ports it needs.

mod chess_tool;
pub mod engine;
pub mod guard_ports;
pub mod history_ports;
pub mod map_ports;
pub mod ports;
pub mod protocol;
pub mod protocol_world;
pub mod replay;
pub mod scorecard;
pub mod world_ports;

pub use engine::{
    Asked, Author, Engine, EngineError, Fired, Proposal, Recorded, Settings, Standing, Submission,
    TickReport, WingSeeded,
};
pub use ports::{Clock, NewProject, NewRoutine, NewTask, RunContext, TeamStore, WorkerAdapter};

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

#[cfg(test)]
mod review_tests;

#[cfg(test)]
mod ceo_tests;

#[cfg(test)]
mod projects_tests;

#[cfg(test)]
mod evidence_tests;

#[cfg(test)]
mod scorecard_tests;

#[cfg(test)]
mod wing_tests;

#[cfg(test)]
mod replay_tests;

#[cfg(test)]
mod staffing_tests;

#[cfg(test)]
mod notebook_tests;

#[cfg(test)]
mod runtime_tests;

#[cfg(test)]
mod office_tests;

#[cfg(test)]
mod games_tests;

#[cfg(test)]
mod raci_tests;

#[cfg(test)]
mod ladder_tests;

#[cfg(test)]
mod world_tests;

#[cfg(test)]
mod guards_tests;

#[cfg(test)]
mod map_tests;

#[cfg(test)]
mod quiz_tests;

#[cfg(test)]
mod history_tests;
