//! The team's adapters: HQ's SQLite store and the simulated runner.

pub mod simulated;
pub mod sqlite;

pub use simulated::{Script, SimulatedRunner};
pub use sqlite::SqliteTeamStore;
