//! The event feed (spec section 12): HQ describes itself as a snapshot followed by these.

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// A goal was proposed.
    GoalProposed,
    /// A goal changed.
    GoalChanged,
    /// A task was created.
    TaskCreated,
    /// A task changed.
    TaskChanged,
    /// A member took a task.
    TaskClaimed,
    /// A member let a task go.
    TaskReleased,
    /// Someone wrote in a task's thread.
    ThreadPosted,
    /// A wake was queued.
    WakeQueued,
    /// A wake was folded into a running turn.
    WakeFolded,
    /// A wake was held.
    WakeHeld,
    /// A wake was turned away.
    WakeRefused,
    /// A turn started.
    RunStarted,
    /// A turn took a step.
    RunStep,
    /// A turn ended.
    RunEnded,
    /// The gate passed or failed.
    GateResult,
    /// Something landed on the owner's desk.
    DeskAdded,
    /// The owner decided something on the desk.
    DeskDecided,
    /// Spend moved.
    BudgetChanged,
    /// A limit was crossed.
    BudgetCrossed,
    /// A runner changed.
    RunnerChanged,
    /// The tick ran.
    Tick,
    /// The kill switch was pulled.
    Stopped,
    /// The owner resumed the team.
    Resumed,
    /// A routine was made, switched on or off (ADR 0107).
    RoutineChanged,
    /// A routine handed out a task.
    RoutineFired,
    /// A routine's firing was skipped, and why.
    RoutineSkipped,
    /// An alarm was passed into a routine's open task.
    RoutinePosted,
    /// A member asked to come back to its task.
    CheckBackSet,
    /// A check-back woke its member.
    CheckBackFired,
    /// A check-back's task finished first, so it woke nobody.
    CheckBackDropped,
    /// A member noted a lesson in a goal's notes for the team.
    LessonNoted,
    /// The owner set what a member thinks with.
    BrainSet,
    /// A member joined the team.
    MemberHired,
    /// The owner renamed a member.
    MemberRenamed,
    /// The house log was added to or rewritten (ADR 0111).
    HouseNoted,
    /// A project was proposed, decided, capped, paused, resumed or put away (ADR 0113).
    ProjectChanged,
    /// A project reached 80% of its cap, or spent it (ADR 0113).
    ProjectBudget,
    /// The owner changed whom a member reports to (ADR 0113).
    ReportsTo,
}

impl EventKind {
    /// The name in the feed.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::GoalProposed => "goal.proposed",
            Self::GoalChanged => "goal.changed",
            Self::TaskCreated => "task.created",
            Self::TaskChanged => "task.changed",
            Self::TaskClaimed => "task.claimed",
            Self::TaskReleased => "task.released",
            Self::ThreadPosted => "thread.posted",
            Self::WakeQueued => "wake.queued",
            Self::WakeFolded => "wake.folded",
            Self::WakeHeld => "wake.held",
            Self::WakeRefused => "wake.refused",
            Self::RunStarted => "run.started",
            Self::RunStep => "run.step",
            Self::RunEnded => "run.ended",
            Self::GateResult => "gate.result",
            Self::DeskAdded => "desk.added",
            Self::DeskDecided => "desk.decided",
            Self::BudgetChanged => "budget.changed",
            Self::BudgetCrossed => "budget.crossed",
            Self::RunnerChanged => "runner.changed",
            Self::Tick => "tick",
            Self::Stopped => "stopped",
            Self::Resumed => "resumed",
            Self::RoutineChanged => "routine.changed",
            Self::RoutineFired => "routine.fired",
            Self::RoutineSkipped => "routine.skipped",
            Self::RoutinePosted => "routine.posted",
            Self::CheckBackSet => "checkback.set",
            Self::CheckBackFired => "checkback.fired",
            Self::CheckBackDropped => "checkback.dropped",
            Self::LessonNoted => "lesson.noted",
            Self::BrainSet => "member.brain",
            Self::MemberHired => "member.hired",
            Self::MemberRenamed => "member.renamed",
            Self::HouseNoted => "house.noted",
            Self::ProjectChanged => "project.changed",
            Self::ProjectBudget => "project.budget",
            Self::ReportsTo => "member.reports_to",
        }
    }
}

/// One event, as stored and served.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Its place in the feed; the stream resumes after the last one seen.
    pub id: i64,
    /// When.
    pub at: super::work::Millis,
    /// The name of its kind.
    pub kind: String,
    /// What it carries.
    pub data: serde_json::Value,
}
