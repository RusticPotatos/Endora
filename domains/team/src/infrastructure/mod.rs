//! The team's adapters: HQ's SQLite store and the simulated runner.

pub mod demo;
pub mod simulated;
pub mod sqlite;

pub use simulated::{Script, SimulatedRunner};
pub use sqlite::SqliteTeamStore;
