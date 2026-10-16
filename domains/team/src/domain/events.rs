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
