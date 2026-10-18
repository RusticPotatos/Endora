//! The team's domain: the work, wakes, runs, money and the event feed, as pure rules.

pub mod areas;
pub mod brain;
pub mod chain;
pub mod chess;
pub mod closure;
pub mod consult;
pub mod cut;
pub mod docs;
pub mod effort;
pub mod events;
pub mod evidence;
pub mod games;
pub mod guards;
pub mod history;
pub mod house;
pub mod ladder;
pub mod lessons;
pub mod map;
pub mod money;
pub mod notebook;
pub mod office;
pub mod projects;
pub mod quiz;
pub mod record;
pub mod retraction;
pub mod review;
pub mod routines;
pub mod runs;
pub mod runtime;
pub mod situations;
pub mod staffing;
pub mod wakes;
pub mod wing;
pub mod work;
pub mod world;
pub mod world_migration;

pub use brain::Brain;
pub use effort::Effort;
pub use events::{Event, EventKind};
pub use evidence::{CallReceipt, Criterion, Label, Ref};
pub use money::Micros;
pub use notebook::Note;
pub use projects::{Evidence, Pause, Project, ProjectStatus};
pub use routines::{CheckBack, CheckBackStatus, Firing, Routine};
pub use runs::{Liveness, RunStatus};
pub use runtime::Runtime;
pub use wakes::{Receipt, Refused, WakeDecision, WakeReason, WakeSituation};
pub use wing::{Branch, Check, Document, HireRequest};
pub use work::{
    DeskId, Goal, GoalId, GoalStatus, Member, MemberId, Millis, Priority, ProjectId, Refusal, Role,
    RoutineId, RunId, Task, TaskId, TaskStatus, WakeId,
};
