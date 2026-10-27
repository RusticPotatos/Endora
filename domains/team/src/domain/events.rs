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
    /// The owner topped the pool up for one day (ADR 0113 rule 6).
    PoolToppedUp,
    /// A member asked to come back to its task.
    CheckBackSet,
    /// A check-back woke its member.
    CheckBackFired,
    /// A check-back's task finished first, so it woke nobody.
    CheckBackDropped,
    /// The owner set what a member thinks with.
    BrainSet,
    /// The owner set what runs a member's turns (ADR 0117).
    RuntimeSet,
    /// A member joined the team.
    MemberHired,
    /// The owner renamed a member.
    MemberRenamed,
    /// The house log was added to or rewritten (ADR 0111); since ADR 0122, a line was added to
    /// the house page's timeline.
    HouseNoted,
    /// The team's world changed (ADR 0122): a section written, confirmed or retired, a piece
    /// proposed for canon, made canon or demoted, or the old record moved in.
    WorldChanged,
    /// Work was handed on, with its handover note (ADR 0122).
    HandedOver,
    /// A project was proposed, decided, capped, paused, resumed or put away (ADR 0113).
    ProjectChanged,
    /// A project reached 80% of its cap, or spent it (ADR 0113).
    ProjectBudget,
    /// The node recorded a call a turn made of Endora's tools (ADR 0114).
    ReceiptRecorded,
    /// The wing submitted a branch, kept on the node (ADR 0115).
    BranchSubmitted,
    /// A check of a submitted branch reported (ADR 0115).
    CheckDone,
    /// The owner had a branch pushed to the fork, or it failed (ADR 0115).
    BranchPushed,
    /// The owner said a pushed branch is deployed; the lab routine's member checks it (ADR
    /// 0115, amended).
    BranchDeployed,
    /// A member challenged another to chess, and the game began.
    ChessStarted,
    /// A move was played in a game of chess.
    ChessMoved,
    /// A game of chess ended: won, drawn or adjourned.
    ChessEnded,
    /// A member acted in a scenario, accepted or refused, and whether that ended it (the
    /// Holodeck plan's phase 5).
    ScenarioActed,
    /// A tester or a lead sent work back from review, with the reason if it was one HQ counts
    /// (ADR 0132).
    SentBack,
    /// A member searched the record with `ask_record`, and how many passages it found (ADR 0132).
    RecordAsked,
    /// A drain began: nothing new starts until a restart has been (ADR 0134).
    DrainBegan,
    /// A drain ended, and how many turns ended before it and how many the restart cut.
    DrainEnded,
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
            Self::RunEnded => "run.ended",
            Self::GateResult => "gate.result",
            Self::DeskAdded => "desk.added",
            Self::DeskDecided => "desk.decided",
            Self::BudgetChanged => "budget.changed",
            Self::BudgetCrossed => "budget.crossed",
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
            Self::BrainSet => "member.brain",
            Self::RuntimeSet => "member.runtime",
            Self::MemberHired => "member.hired",
            Self::MemberRenamed => "member.renamed",
            Self::HouseNoted => "house.noted",
            Self::WorldChanged => "world.changed",
            Self::HandedOver => "task.handed_over",
            Self::ProjectChanged => "project.changed",
            Self::ProjectBudget => "project.budget",
            Self::ReceiptRecorded => "receipt.recorded",
            Self::BranchSubmitted => "branch.submitted",
            Self::CheckDone => "check.done",
            Self::BranchPushed => "branch.pushed",
            Self::BranchDeployed => "branch.deployed",
            Self::ChessStarted => "chess.started",
            Self::ChessMoved => "chess.moved",
            Self::ChessEnded => "chess.ended",
            Self::ScenarioActed => "scenario.acted",
            Self::PoolToppedUp => "pool.topped_up",
            Self::SentBack => "task.sent_back",
            Self::RecordAsked => "record.asked",
            Self::DrainBegan => "drain.began",
            Self::DrainEnded => "drain.ended",
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
