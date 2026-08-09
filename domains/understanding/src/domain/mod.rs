//! Understanding domain model — what the butler has learned about the person.

pub mod beliefs;
pub mod intentions;
pub mod notions;
pub mod outcomes;
pub mod preferences;
pub mod repairs;
pub mod specimens;

pub use beliefs::{Belief, BeliefKind, BeliefStatus, Confidence};
pub use intentions::{Intention, IntentionState};
pub use notions::{Citation, Notion, NotionStatus, Source, make_way_for_a_new_one};
pub use outcomes::{Outcome, Reaction, Reliability};
pub use preferences::{Preference, PreferenceKind};
pub use repairs::{Remedy, RepairProposal, repair_proposals};
pub use specimens::{MOST_SPECIMENS_OPEN, REPLAYS_BEFORE_GIVING_UP, Specimen};
