//! Understanding domain model — what the butler has learned about the person.

pub mod beliefs;
pub mod intentions;
pub mod outcomes;
pub mod preferences;
pub mod repairs;
pub mod research;
pub mod specimens;

pub use beliefs::{Belief, BeliefKind, BeliefStatus, Confidence};
pub use intentions::{Intention, IntentionOrigin, IntentionState};
pub use outcomes::{Outcome, Reaction, Reliability};
pub use preferences::{Preference, PreferenceKind};
pub use repairs::{Remedy, RepairProposal, repair_proposals};
pub use research::{
    RESEARCH_MODEL_LIMIT, RESEARCH_READ_LIMIT, ResearchAssignment, ResearchReport, ResearchStatus,
};
pub use specimens::{MOST_SPECIMENS_OPEN, REPLAYS_BEFORE_GIVING_UP, Specimen, SpecimenState};
