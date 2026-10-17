//! The team's domain: the work, wakes, runs, money and the event feed, as pure rules.

pub mod brain;
pub mod chain;
pub mod docs;
pub mod effort;
pub mod events;
pub mod evidence;
pub mod house;
pub mod lessons;
pub mod money;
pub mod projects;
pub mod review;
pub mod routines;
pub mod runs;
pub mod wakes;
pub mod work;

pub use brain::Brain;
pub use effort::Effort;
pub use events::{Event, EventKind};
pub use evidence::{CallReceipt, Criterion, Label, Ref};
pub use money::Micros;
pub use projects::{Evidence, Pause, Project, ProjectStatus};
pub use routines::{CheckBack, CheckBackStatus, Firing, Routine};
pub use runs::{Liveness, RunStatus};
pub use wakes::{Receipt, Refused, WakeDecision, WakeReason, WakeSituation};
pub use work::{
    DeskId, Goal, GoalId, GoalStatus, Member, MemberId, Millis, Priority, ProjectId, Refusal, Role,
    RoutineId, RunId, Task, TaskId, TaskStatus, WakeId,
};
