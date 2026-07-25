//! Understanding domain model — what the butler has learned about the person.

pub mod beliefs;
pub mod outcomes;
pub mod preferences;

pub use beliefs::{Belief, BeliefKind, BeliefStatus, Confidence};
pub use outcomes::{Outcome, Reaction};
pub use preferences::{Preference, PreferenceKind};
