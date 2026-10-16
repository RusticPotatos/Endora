//! The team's domain: the work, wakes, runs, money and the event feed, as pure rules.

pub mod events;
pub mod money;
pub mod runs;
pub mod wakes;
pub mod work;

pub use events::{Event, EventKind};
pub use money::Micros;
pub use runs::{Liveness, RunStatus};
pub use wakes::{Receipt, Refused, WakeDecision, WakeReason, WakeSituation};
pub use work::{
    DeskId, Goal, GoalId, GoalStatus, Member, MemberId, Millis, Priority, Refusal, Role, RunId,
    Task, TaskId, TaskStatus, WakeId,
};
