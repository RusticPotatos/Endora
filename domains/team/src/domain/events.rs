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
    /// The owner topped the pool up for one day (ADR 0113 rule 6).
    PoolToppedUp,
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
    /// A story was told, or a lesson retold as one (ADR 0122).
    StoryTold,
    /// Work was handed on, with its handover note (ADR 0122).
    HandedOver,
    /// A project was proposed, decided, capped, paused, resumed or put away (ADR 0113).
    ProjectChanged,
    /// A project reached 80% of its cap, or spent it (ADR 0113).
    ProjectBudget,
    /// The owner changed whom a member reports to (ADR 0113).
    ReportsTo,
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
    /// A member was put on furlough, on the owner's word (ADR 0118).
    MemberFurloughed,
    /// A furloughed member came back to work (ADR 0118).
    MemberRecalled,
    /// A note was written in a member's notebook, by the member or the owner (ADR 0118).
    NoteWritten,
    /// A note was forgotten, edited or deleted (ADR 0118).
    NoteForgotten,
    /// A member proposed a change to the office (ADR 0119).
    OfficeProposed,
    /// A member voted on a proposal for the office.
    OfficeVoted,
    /// The office changed: a proposal carried, or the owner undid a change or reset it.
    OfficeChanged,
    /// A proposal for the office was let go, and why.
    OfficeDropped,
    /// The owner vetoed a proposal for the office.
    OfficeVetoed,
    /// A ping-pong or pool match was played, and who won (ADR 0121).
    GamePlayed,
    /// A member challenged another to chess, and the game began.
    ChessStarted,
    /// A move was played in a game of chess.
    ChessMoved,
    /// A game of chess ended: won, drawn or adjourned.
    ChessEnded,
    /// A member acted in a scenario, accepted or refused, and whether that ended it (the
    /// Holodeck plan's phase 5).
    ScenarioActed,
    /// A goal's strategy was written, by the CEO or the owner (ADR 0120).
    StrategySet,
    /// A lead reported on its area (ADR 0120).
    AreaReported,
    /// A guard made a finding (ADR 0124).
    GuardFound,
    /// A guard's finding cleared: its condition no longer holds.
    GuardCleared,
    /// The owner or a lead gave a guard's finding its verdict.
    GuardJudged,
    /// A tester or a lead sent work back from review, with the reason if it was one HQ counts
    /// (ADR 0132).
    SentBack,
    /// A member searched the record with `ask_record`, and how many passages it found (ADR 0132).
    RecordAsked,
    /// A lead narrowed or reworded what done means for a task in its area: the criteria it had
    /// and the criteria it has (ADR 0133).
    CriteriaChanged,
    /// A drain began: nothing new starts until a restart has been (ADR 0134).
    DrainBegan,
    /// A drain ended, and how many turns ended before it and how many the restart cut.
    DrainEnded,
    /// A member's question went to the colleague it asked: who, whom, the task and the
    /// conversation, never the words (ADR 0138).
    HelpAsked,
    /// The colleague answered a question asked of them.
    HelpAnswered,
    /// The asker recorded what it concluded from the answer.
    HelpConcluded,
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
            Self::RuntimeSet => "member.runtime",
            Self::MemberHired => "member.hired",
            Self::MemberRenamed => "member.renamed",
            Self::HouseNoted => "house.noted",
            Self::WorldChanged => "world.changed",
            Self::StoryTold => "story.told",
            Self::HandedOver => "task.handed_over",
            Self::ProjectChanged => "project.changed",
            Self::ProjectBudget => "project.budget",
            Self::ReportsTo => "member.reports_to",
            Self::ReceiptRecorded => "receipt.recorded",
            Self::BranchSubmitted => "branch.submitted",
            Self::CheckDone => "check.done",
            Self::BranchPushed => "branch.pushed",
            Self::BranchDeployed => "branch.deployed",
            Self::MemberFurloughed => "member.furloughed",
            Self::MemberRecalled => "member.recalled",
            Self::NoteWritten => "note.written",
            Self::NoteForgotten => "note.forgotten",
            Self::OfficeProposed => "office.proposed",
            Self::OfficeVoted => "office.voted",
            Self::OfficeChanged => "office.changed",
            Self::OfficeDropped => "office.dropped",
            Self::OfficeVetoed => "office.vetoed",
            Self::GamePlayed => "game.played",
            Self::ChessStarted => "chess.started",
            Self::ChessMoved => "chess.moved",
            Self::ChessEnded => "chess.ended",
            Self::ScenarioActed => "scenario.acted",
            Self::StrategySet => "goal.strategy",
            Self::AreaReported => "area.reported",
            Self::PoolToppedUp => "pool.topped_up",
            Self::GuardFound => "guard.found",
            Self::GuardCleared => "guard.cleared",
            Self::GuardJudged => "guard.judged",
            Self::SentBack => "task.sent_back",
            Self::RecordAsked => "record.asked",
            Self::CriteriaChanged => "task.criteria",
            Self::DrainBegan => "drain.began",
            Self::DrainEnded => "drain.ended",
            Self::HelpAsked => "help.asked",
            Self::HelpAnswered => "help.answered",
            Self::HelpConcluded => "help.concluded",
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
