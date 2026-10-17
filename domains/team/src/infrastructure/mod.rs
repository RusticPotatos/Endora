//! The team's adapters: HQ's SQLite store, and the runners: the simulated one and the remote
//! one.

pub mod demo;
pub mod remote;
pub mod runner;
pub mod simulated;
pub mod sqlite;

pub use remote::RemoteRunner;
pub use runner::Runner;
pub use simulated::{Script, SimulatedRunner};
pub use sqlite::SqliteTeamStore;
