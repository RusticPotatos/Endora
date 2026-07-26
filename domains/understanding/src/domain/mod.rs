//! Understanding domain model — what the butler has learned about the person.

pub mod beliefs;
pub mod intentions;
pub mod outcomes;
pub mod preferences;
pub mod repairs;

pub use beliefs::{Belief, BeliefKind, BeliefStatus, Confidence};
pub use intentions::{Intention, IntentionState};
pub use outcomes::{Outcome, Reaction};
pub use preferences::{Preference, PreferenceKind};
pub use repairs::{Remedy, RepairProposal, repair_proposals};
