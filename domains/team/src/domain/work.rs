//! The work: members, goals and tasks, and the rules about who may do what with them.

use core::fmt;

/// Milliseconds since the Unix epoch.
pub type Millis = i64;

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub i64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id!(
    /// A member of the team.
    MemberId
);
id!(
    /// A goal.
    GoalId
);
id!(
    /// A task.
    TaskId
);
id!(
    /// One turn of a member.
    RunId
);
id!(
    /// A request that a member take a turn.
    WakeId
);
id!(
    /// An item on the owner's desk.
    DeskId
);
id!(
    /// Work that comes round again (ADR 0107).
    RoutineId
);
id!(
    /// A project: a piece of a goal with its own lead and budget (ADR 0113).
    ProjectId
);

/// What a member does on the team. The rank decides who may hand work to whom: by default a
/// member hands work only to members below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Runs the team for the owner, who is the board (ADR 0111): decides what the lead asks,
    /// reviews the house once a day, and puts to the board only what the owner alone decides.
    Ceo,
    /// Looks over the team's work and hands it out.
    Lead,
    /// Writes and changes code.
    Developer,
    /// Finds things out and writes them up.
    Researcher,
    /// Reviews what others did.
    Reviewer,
}

impl Role {
    /// Stable lowercase name, for storage and the event feed.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ceo => "ceo",
            Self::Lead => "lead",
            Self::Developer => "developer",
            Self::Researcher => "researcher",
            Self::Reviewer => "reviewer",
        }
    }

    /// The role as HQ's screen names it: the reviewer is the tester (ADR 0114).
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Ceo => "CEO",
            Self::Lead => "Lead",
            Self::Developer => "Developer",
            Self::Researcher => "Researcher",
            Self::Reviewer => "Tester",
        }
    }

    /// The role with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "ceo" => Some(Self::Ceo),
            "lead" => Some(Self::Lead),
            "developer" => Some(Self::Developer),
            "researcher" => Some(Self::Researcher),
            "reviewer" => Some(Self::Reviewer),
            _ => None,
        }
    }

    /// Lower is more senior.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Ceo => 0,
            Self::Lead => 1,
            Self::Developer | Self::Researcher | Self::Reviewer => 2,
        }
    }

    /// The role's weight in the team's pool (ADR 0111): what a member in it is hired with as
    /// its share. The day's pool is split among the members by these weights and the price
    /// class of the Claude each judges with (ADR 0131); a member's work costs nothing.
    #[must_use]
    pub const fn share(self) -> u8 {
        match self {
            Self::Ceo => 35,
            Self::Lead => 25,
            Self::Developer => 40,
            Self::Reviewer => 20,
            Self::Researcher => 15,
        }
    }

    /// Whether a member in this role may act on any task of the team: the CEO and the lead,
    /// who lead every goal.
    #[must_use]
    pub const fn leads(self) -> bool {
        matches!(self, Self::Ceo | Self::Lead)
    }
}

/// One worker on the team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Its id.
    pub id: MemberId,
    /// What it is called.
    pub name: String,
    /// What it does.
    pub role: Role,
    /// Its share of the team pool, in percent.
    pub share_percent: u8,
    /// How many tasks it may work at once.
    pub slots: u8,
    /// Which character it looks like in the office.
    pub look: u8,
    /// Whether the owner has paused it.
    pub paused: bool,
    /// What it judges with (ADR 0109, 0131): a Claude its role may use, Haiku until the owner
    /// says otherwise. It is the brain the pool's shares are reckoned on. The brain a turn runs
    /// on is decided per turn (see [`super::turns`]); for the member of a running turn, as the
    /// engine returns it to the team protocol and the node, this is that brain instead, so the
    /// data tier and the relays see what the turn is.
    pub brain: super::brain::Brain,
    /// What it does its work with (ADR 0131): the owner's Mac or the house's own model, never
    /// Claude. Its turns of work run here, and cost nothing; its turns of judgment never do.
    pub work_brain: super::brain::Brain,
    /// The member it reports to (ADR 0113); `None` for the CEO, who reports to the board,
    /// and for a member hired before anyone was there to report to.
    pub reports_to: Option<MemberId>,
    /// When the member was furloughed (ADR 0118), if it ever was: set for the latest furlough.
    /// A furloughed member is never woken, holds no task, and is left out of every briefing's
    /// team and of the share arithmetic; it keeps its record, its name and its notebook.
    pub furloughed_at: Option<Millis>,
    /// When the latest furlough ended, if it has: the member is at work again.
    pub recalled_at: Option<Millis>,
}

impl Member {
    /// The Claude the member judges with (ADR 0131). Its turns of judgment run on it and
    /// nowhere else.
    #[must_use]
    pub const fn judgment_brain(&self) -> super::brain::Brain {
        self.brain
    }

    /// Whether the member is on furlough now (ADR 0118): furloughed and not yet recalled.
    #[must_use]
    pub const fn furloughed(&self) -> bool {
        self.furloughed_at.is_some() && self.recalled_at.is_none()
    }

    /// Whether the member is available for work: neither paused by the owner nor on furlough.
    #[must_use]
    pub const fn at_work(&self) -> bool {
        !self.paused && !self.furloughed()
    }
}

/// Where a goal is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    /// Proposed by Endora, waiting for the owner.
    Proposed,
    /// Being worked towards.
    Active,
    /// Reached.
    Done,
    /// Given up.
    Dropped,
}

impl GoalStatus {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Active => "active",
            Self::Done => "done",
            Self::Dropped => "dropped",
        }
    }

    /// The status with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "proposed" => Some(Self::Proposed),
            "active" => Some(Self::Active),
            "done" => Some(Self::Done),
            "dropped" => Some(Self::Dropped),
            _ => None,
        }
    }
}

/// What a team is for, in a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    /// Its id.
    pub id: GoalId,
    /// The sentence.
    pub title: String,
    /// Why it matters.
    pub reason: String,
    /// Where it is.
    pub status: GoalStatus,
    /// The goal it serves, if it is a smaller one.
    pub parent: Option<GoalId>,
}

/// Where a task is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Not started.
    Todo,
    /// Being worked.
    Doing,
    /// Waiting for the gate and a review.
    InReview,
    /// The gate passed and a different member approved it.
    Done,
    /// Cannot go on without something or someone.
    Blocked,
    /// Given up.
    Dropped,
}

impl TaskStatus {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::InReview => "in_review",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Dropped => "dropped",
        }
    }

    /// The status with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "todo" => Some(Self::Todo),
            "doing" => Some(Self::Doing),
            "in_review" => Some(Self::InReview),
            "done" => Some(Self::Done),
            "blocked" => Some(Self::Blocked),
            "dropped" => Some(Self::Dropped),
            _ => None,
        }
    }

    /// Whether the task is over: nothing more will be done on it.
    #[must_use]
    pub const fn finished(self) -> bool {
        matches!(self, Self::Done | Self::Dropped)
    }
}

/// How soon a task matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Before anything else.
    Urgent,
    /// Soon.
    High,
    /// In turn.
    Normal,
    /// When there is nothing else.
    Low,
}

impl Priority {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Urgent => "urgent",
            Self::High => "high",
            Self::Normal => "normal",
            Self::Low => "low",
        }
    }

    /// The priority with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "urgent" => Some(Self::Urgent),
            "high" => Some(Self::High),
            "normal" => Some(Self::Normal),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

/// One piece of work towards a goal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Its id.
    pub id: TaskId,
    /// The goal it is for.
    pub goal: GoalId,
    /// The project of that goal it is part of, if any (ADR 0113).
    pub project: Option<ProjectId>,
    /// The task it is part of, if it is a sub-task.
    pub parent: Option<TaskId>,
    /// What it is.
    pub title: String,
    /// What is wanted.
    pub description: String,
    /// The one member working it, if any.
    pub owner: Option<MemberId>,
    /// Who it is meant for: the member it was handed to, whether or not they have taken it.
    pub assignee: Option<MemberId>,
    /// Where it is.
    pub status: TaskStatus,
    /// How soon it matters.
    pub priority: Priority,
    /// How many hands it passed through from the goal.
    pub depth: u8,
    /// The tasks it waits on.
    pub waits_on: Vec<TaskId>,
    /// Whether it has had its one recovery nudge.
    pub nudged: bool,
    /// What done means for it (ADR 0114); empty for the owner's work that gave none.
    pub criteria: Vec<super::evidence::Criterion>,
}

/// Why something a member asked for is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Another member has the task.
    TakenBy(MemberId),
    /// The member does not have the task.
    NotYours,
    /// Only the gate decides that work is done.
    OnlyTheGate,
    /// That change is not one a task makes.
    NotAStep {
        /// Where it is.
        from: TaskStatus,
        /// Where it was asked to go.
        to: TaskStatus,
    },
    /// Work is handed only to members below.
    NotBelow,
    /// The task is over.
    Finished,
    /// Asked to review its own work.
    OwnWork,
    /// The gate has not passed.
    GateNotPassed,
    /// Work a member doubts is handed back to its lead, never dropped by it.
    HandBack,
    /// The member's last question or approval about the task still waits on the owner's desk.
    AlreadyAsked,
    /// The task is not one this turn may touch.
    OutOfReach,
    /// The turn has ended.
    TurnOver,
    /// A routine's rhythm is too fast or too slow.
    Rhythm,
    /// A routine's check is closed only by its member, once it is being worked.
    NotACheck,
    /// Longer than it may be, in characters.
    TooLong(usize),
    /// Not a brain a member in this role may think with (ADR 0109).
    NotABrain {
        /// The member's role.
        role: Role,
        /// What was asked for.
        asked: String,
    },
    /// Not a runtime a member in this role, on its brain, may run on (ADR 0117).
    NotARuntime {
        /// The member's role.
        role: Role,
        /// What was asked for.
        asked: String,
    },
    /// Only the lead and the reviewer keep the team's lessons.
    NotALessonKeeper,
    /// A goal's notes have no room for one more lesson.
    NotesFull,
    /// Only the CEO does that (ADR 0111).
    OnlyTheCeo,
    /// The house log has no room for one more line.
    HouseLogFull,
    /// What the team's world refuses (ADR 0122): its pages, stages, stories and handovers.
    World(super::world::WorldRefusal),
    /// What the guards against how agent teams fail refuse (ADR 0124).
    Guard(super::guards::GuardRefusal),
    /// What the team's history refuses (ADR 0129): transcripts, asking the record, consulting.
    History(super::history::HistoryRefusal),
    /// Not in Endora's public library.
    NotADoc,
    /// A decision for the board says what is to be decided, what the CEO recommends, and why.
    NotADecision,
    /// A name another member already has, or none.
    NotAName,
    /// An open task under the same goal is already about these containers: one task a
    /// problem, and what is new about it is added there.
    Covered {
        /// The task that covers it.
        task: TaskId,
        /// The containers both are about.
        names: Vec<String>,
    },
    /// A project's lead is a lead or the CEO (ADR 0113).
    NotAProjectLead,
    /// Only a lead or the CEO proposes a project.
    OnlyLeadsPropose,
    /// The project serves another goal than the task's or the routine's.
    OtherGoal,
    /// The project takes no work: it is proposed, done or dropped.
    ProjectClosed,
    /// The projects' caps together would pass the pool's month; this much is left.
    CapsOverPool {
        /// What is left of the pool's month for this cap.
        room: super::money::Micros,
    },
    /// A raised cap is more than what the project spent this month.
    CapNotAboveSpend {
        /// What it spent this month.
        spent: super::money::Micros,
    },
    /// An allowance for a member out for the day is more than the pool's day has left, or
    /// less than a turn.
    AllowanceOverPool {
        /// What is left of the pool's day.
        left: super::money::Micros,
    },
    /// A top-up of the pool is above nothing, and with the day's earlier ones at most three
    /// times the base pool day.
    TopUpOutOfRange {
        /// What may still be added today.
        room: super::money::Micros,
    },
    /// A proposal names at least one task or event from HQ's record.
    NoEvidence,
    /// Evidence that is not in HQ's record.
    NoSuchEvidence(String),
    /// A member reports to another member, never to itself, and never in a circle; the CEO
    /// reports to the board.
    NotAManager,
    /// Work handed to another member says what done means (ADR 0114).
    NoCriteria,
    /// A criterion with nothing cited for it; `0` for work with no criteria.
    NoEvidenceFor {
        /// Which criterion, from 1.
        criterion: usize,
        /// What it says.
        text: String,
    },
    /// A criterion about the lab now with no reading from this turn.
    NotFresh {
        /// Which criterion, from 1.
        criterion: usize,
        /// What it says.
        text: String,
    },
    /// A time-bound criterion that has not waited its time yet (ADR 0120).
    TooSoon {
        /// Which criterion, from 1.
        criterion: usize,
        /// What it says.
        text: String,
        /// When it may be shown, by the owner's clock: `00:44`.
        until: String,
    },
    /// Evidence from another piece of work than this one.
    NotThisWork(String),
    /// A receipt of a call that was refused or failed: it shows nothing.
    RefusedCall(String),
    /// The tester passes work only on readings of her own.
    NoOwnReading,
    /// The lead accepts work only once the tester has passed it.
    NotTested,
    /// Only the work's lead accepts it, never its own.
    NotTheLead,
    /// That is not a step a project takes from where it is.
    NotAProjectStep {
        /// Where it is.
        from: &'static str,
        /// What was asked.
        asked: String,
    },
    /// The team has one CEO (ADR 0115).
    OnlyOneCeo,
    /// The wing has no seat left for that role (ADR 0115).
    NoSeat {
        /// The role asked for.
        role: Role,
        /// How many the wing seats in it now.
        count: usize,
        /// How many it may.
        most: usize,
    },
    /// The wing's members think with what the owner agreed for their role (ADR 0115).
    WingBrain {
        /// The role asked for.
        role: Role,
        /// What it may think with.
        allowed: &'static str,
    },
    /// Only a lead asks the owner to hire; the CEO puts it to the board (ADR 0115).
    OnlyLeadsHire,
    /// A hire is asked for a role below the asker's.
    NotBelowAsker,
    /// Not a document on a task this turn may read.
    NotADocument,
    /// A kept branch is checked out by the work it is for and the member who submitted it
    /// (ADR 0115): this one is another task's.
    NotYourBranch {
        /// The branch asked for.
        branch: String,
        /// The task it was submitted for.
        task: TaskId,
    },
    /// What waits in review is a draft for the owner, not work: it is forwarded, never passed
    /// (ADR 0114, amended).
    ForwardTheDraft,
    /// A recheck of a branch's checks is asked for by the work's lead or developer (ADR 0115).
    NotYourToRecheck,
    /// A check of the branch is still waiting or running: one at a time (ADR 0115).
    CheckPending(i64),
    /// The CEO is never furloughed: the owner's own line to the team (ADR 0118).
    IsTheCeo,
    /// The member is on furlough already.
    AlreadyFurloughed,
    /// The member is at work: there is nothing to recall it from.
    NotFurloughed,
    /// A member with open work is not furloughed: hand the work on first.
    HoldsWork {
        /// Its name.
        name: String,
        /// How many open tasks it holds.
        count: usize,
    },
    /// Only a lead asks for staff (ADR 0118).
    OnlyLeadsStaff,
    /// Only a project's lead asks for its staff.
    NotYourProject,
    /// A request for staff cites too few of the project's open tasks.
    NoBacklog {
        /// How many open tasks of the project it cited.
        cited: usize,
        /// How many active developers the project has.
        developers: usize,
        /// How many it needed to cite.
        needs: usize,
    },
    /// Evidence for staff is an open task of the project, and this is not one.
    NotOpenInProject(String),
    /// A note is evidence-backed: at least one reference from HQ's record.
    NoteNeedsEvidence,
    /// A note with no words.
    EmptyNote,
    /// No such note in the member's notebook.
    NoSuchNote(i64),
    /// Forgetting a note says why.
    ForgetNeedsReason,
    /// Work is not handed to a member on furlough.
    Furloughed(String),
    /// A notebook holds as many notes as it may.
    NotebookFull,
    /// The task is in a lead's area (named): the CEO does not claim, move, close, accept or
    /// send back its work, but directs the lead (ADR 0120).
    CeoDirects(String),
    /// The CEO hands work to a lead only; the lead hands it on in its area (ADR 0120).
    CeoToLeads,
    /// The task is in no lead's area: there is nobody to direct.
    NoAreaLead,
    /// A strategy change that gives no part.
    NotAStrategy,
    /// An area report that says nothing.
    EmptyReport,
    /// Only a lead reports on its area, in its report's own turn.
    NotAReport,
    /// A huddle's view is three sentences at most.
    ViewTooLong,
    /// That member is not asked for a view on that task's huddle, or gave it already.
    NotInTheHuddle,
    /// Only the lead of the task's area decides stuck work, and only once it is stuck.
    NotStuckForYou,
    /// A decision that is not one of proceed, drop, hand_to, needs_ceo or needs_board.
    NotADecisionForStuck,
    /// The rules of the office refuse a change (ADR 0119).
    Office(super::office::Refused),
    /// A game of chess refuses a move or a challenge (ADR 0121).
    Chess(super::chess::Refused),
    /// There is no such proposal for the office.
    NoSuchProposal(i64),
    /// The proposal is decided already.
    ProposalClosed(i64),
    /// A member has one vote on a proposal, and it is cast; the proposer's is its yes.
    AlreadyVoted,
    /// A downtime turn makes no work: it is for the office and the notebook.
    InDowntime,
    /// Stuck work was to be tried again, but nobody can be woken for it now: nothing was
    /// changed, and the item still waits.
    NotWoken(super::wakes::Refused),
    /// Stuck work was to be tried again, but no member has it to wake.
    NobodyHasIt,
    /// Only the lead of a task's area narrows or rewords its criteria (ADR 0133).
    OnlyTheAreasLead,
    /// Narrowing leaves at least one criterion: done must still mean something (ADR 0133).
    NarrowedToNothing,
    /// A lead narrows or rewords what done means and never widens it: as many criteria as the
    /// task has, or fewer (ADR 0133).
    NotNarrower {
        /// How many criteria the task has.
        had: usize,
        /// How many were given.
        gave: usize,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TakenBy(member) => write!(f, "member {member} has this task"),
            Self::NotYours => f.write_str("this task is not yours; claim it first"),
            Self::OnlyTheGate => f.write_str("only the gate decides that work is done"),
            Self::NotAStep { from, to } => {
                write!(
                    f,
                    "a task does not go from {} to {}",
                    from.name(),
                    to.name()
                )
            }
            Self::NotBelow => f.write_str("work is handed only to members below you"),
            Self::Finished => f.write_str("this task is over"),
            Self::OwnWork => f.write_str("work is reviewed by a different member"),
            Self::GateNotPassed => f.write_str("the gate has not passed"),
            Self::HandBack => {
                f.write_str("a member does not drop work; hand it back to your lead with a reason")
            }
            Self::OutOfReach => f.write_str(
                "that task is not part of this turn: you act on your own task and its sub-tasks, \
                 and write there, on the task above yours, or where you were named; ask in your \
                 own task's thread instead",
            ),
            Self::TurnOver => f.write_str("this turn has ended: stop now"),
            Self::Rhythm => {
                f.write_str("a routine comes round between every 15 minutes and every 30 days")
            }
            Self::NotACheck => f.write_str(
                "only a routine's check is closed by its member, once it is being worked: \
                 claim it first",
            ),
            Self::TooLong(most) => write!(f, "that is longer than {most} characters"),
            Self::NotABrain { role, asked } => {
                let may: Vec<&str> = super::brain::Brain::choices(*role)
                    .into_iter()
                    .map(super::brain::Brain::name)
                    .collect();
                write!(
                    f,
                    "a {} thinks with {} only, not {asked:?}; Opus is the CEO's alone",
                    role.name(),
                    may.join(", ")
                )
            }
            Self::NotARuntime { role, asked } => write!(
                f,
                "a {} runs on the worker, not {asked:?}: Claude Code is a developer's on Claude \
                 alone",
                role.name()
            ),
            Self::NotALessonKeeper => {
                f.write_str("only the lead and the reviewer note the team's lessons")
            }
            Self::NotesFull => f.write_str("the notes are full; ask the owner to tidy them"),
            Self::OnlyTheCeo => f.write_str("only the CEO does that"),
            Self::HouseLogFull => f.write_str(
                "the house log is full: rewrite it shorter with rewrite_house, keeping what still matters",
            ),
            Self::World(refused) => refused.fmt(f),
            Self::Guard(refused) => refused.fmt(f),
            Self::History(refused) => refused.fmt(f),
            Self::NotADoc => f.write_str(
                "read_doc reads Endora's public library only: docs/standards.md, or a path like docs/adr/0111-the-ceo-and-the-chain-of-command.md, docs/plans/endora-hq.md or docs/drafts/ansible-nas-AGENTS.md",
            ),
            Self::NotAName => f.write_str(
                "a member's name is one to 40 characters, and no other member's",
            ),
            Self::NotADecision => f.write_str(
                "a decision for the board gives `decision` (what is to be decided), `recommendation` (what you recommend) and `why`, each in a sentence or two",
            ),
            Self::AlreadyAsked => f.write_str(
                "your last question or approval about this task is still waiting for an answer; end your turn",
            ),
            Self::Covered { task, names } => write!(
                f,
                "#{task} already covers this ({}): add to it with post in #{task}, rather than a \
                 second task",
                names.join(", ")
            ),
            Self::NotAProjectLead => f.write_str("a project's lead is a lead or the CEO"),
            Self::OnlyLeadsPropose => f.write_str(
                "only a lead proposes a project; the CEO decides proposals with decide_project",
            ),
            Self::OtherGoal => f.write_str(
                "that project serves another goal: a task's or a routine's project serves its goal",
            ),
            Self::ProjectClosed => {
                f.write_str("that project takes no work: it is proposed, done or dropped")
            }
            Self::CapsOverPool { room } => write!(
                f,
                "the projects' caps together may not pass the pool's month: at most {} is left \
                 for this one",
                super::projects::dollars(*room)
            ),
            Self::CapNotAboveSpend { spent } => write!(
                f,
                "a raised cap is more than this month's spend, {}",
                super::projects::dollars(*spent)
            ),
            Self::AllowanceOverPool { left } => write!(
                f,
                "an allowance is at least a turn's worth and at most what the pool's day has \
                 left, {}",
                super::projects::dollars(*left)
            ),
            Self::TopUpOutOfRange { room } => write!(
                f,
                "a top-up is more than nothing, and a day's top-ups together are at most three \
                 times the pool's day; {} may still be added today",
                super::projects::dollars(*room)
            ),
            Self::NoEvidence => f.write_str(
                "a proposal names its evidence: at least one task number (`tasks`) or alarm \
                 event (`events`) from HQ's own record",
            ),
            Self::NoSuchEvidence(what) => write!(f, "{what} is not in HQ's record"),
            Self::NoCriteria => f.write_str(
                "work handed to another member says what done means: give `criteria`, a short \
                 list (each { text, live }; live for anything about the lab as it is now)",
            ),
            Self::NoEvidenceFor { criterion: 0, .. } => f.write_str(
                "this needs evidence: cite at least one receipt (r12), task or event from HQ's \
                 record in `evidence`",
            ),
            Self::NoEvidenceFor { criterion, text } => write!(
                f,
                "criterion {criterion} ({text}) has no evidence: cite a receipt (r12), task or \
                 event for it in `evidence`, one list per criterion in order"
            ),
            Self::NotFresh { criterion, text } => write!(
                f,
                "criterion {criterion} ({text}) is about the lab now: cite a reading from this \
                 turn (take one now, e.g. homelab_status, and cite its receipt), not an older \
                 one or someone's word"
            ),
            Self::TooSoon {
                criterion,
                text,
                until,
            } => write!(
                f,
                "criterion {criterion} ({text}) needs time to show: wait until {until} (owner's \
                 time), then cite a fresh reading. Set a check_back for then, rather than \
                 closing it now"
            ),
            Self::NotThisWork(what) => {
                write!(f, "{what} is not from this task's work: cite this work's own")
            }
            Self::RefusedCall(what) => {
                write!(f, "{what} is a call that was refused or failed: it shows nothing")
            }
            Self::NoOwnReading => f.write_str(
                "a tester passes work on her own readings: take them this turn and cite their \
                 receipts",
            ),
            Self::NotTested => f.write_str(
                "the tester has not passed this work yet: wait for her verdict",
            ),
            Self::NotTheLead => f.write_str(
                "only the work's lead accepts it, and never work of its own",
            ),
            Self::NotAProjectStep { from, asked } => {
                write!(f, "a project that is {from} does not {asked}")
            }
            Self::Office(refused) => refused.fmt(f),
            Self::Chess(refused) => refused.fmt(f),
            Self::NoSuchProposal(id) => write!(f, "there is no proposal {id} for the office"),
            Self::ProposalClosed(id) => write!(f, "proposal {id} is closed: it is decided"),
            Self::AlreadyVoted => f.write_str(
                "you already voted on this proposal (a proposer's own vote is its yes)",
            ),
            Self::InDowntime => f.write_str(
                "this is a downtime turn: it makes no work. React to a proposal, propose a small \
                 change to the office, or note an idea in your notebook",
            ),
            Self::NotWoken(why) => write!(
                f,
                "it was not tried again, and still waits here: nobody can be woken for it now \
                 ({})",
                why.plainly()
            ),
            Self::NobodyHasIt => f.write_str(
                "it was not tried again, and still waits here: no member has this task to wake",
            ),
            Self::OnlyTheAreasLead => f.write_str(
                "only the lead of the task's area narrows its criteria: ask that lead, in the \
                 task's thread",
            ),
            Self::NarrowedToNothing => f.write_str(
                "narrowing keeps at least one criterion: done must still mean something",
            ),
            Self::NotNarrower { had, gave } => write!(
                f,
                "narrow_criteria narrows or rewords what done means and never widens it: the \
                 task has {had} and you gave {gave}"
            ),
            Self::NotAManager => f.write_str(
                "a member reports to another member, never itself and never in a circle; the \
                 CEO reports to the board",
            ),
            Self::OnlyOneCeo => f.write_str("the team has one CEO already"),
            Self::NoSeat { role, count, most: 0 } => {
                let _ = count;
                write!(f, "the wing has no seat for a {}", role.name())
            }
            Self::NoSeat { role, count, most } => write!(
                f,
                "the wing seats {count} of its {most} {}{}: no seat is free; the owner may hire beyond that on the HQ screen",
                role.name(),
                if *most == 1 { "" } else { "s" }
            ),
            Self::WingBrain { role, allowed } => write!(
                f,
                "a {} in the wing thinks with {allowed}",
                role.name()
            ),
            Self::OnlyLeadsHire => f.write_str(
                "only a lead asks the owner to hire; the CEO puts hiring to the board with ask_owner",
            ),
            Self::NotBelowAsker => f.write_str("a hire is asked for a role below your own"),
            Self::NotADocument => f.write_str(
                "read_document reads a document of your task, its sub-tasks or the task above: d12, as the thread names them",
            ),
            Self::NotYourBranch { branch, task } => write!(
                f,
                "{branch} is task #{task}'s: check out the branch of your own work (a branch is \
                 handed to the task it was submitted for, the tasks under and above it, and the \
                 member who submitted it)"
            ),
            Self::ForwardTheDraft => f.write_str(
                "what waits in review is a draft for the owner, not work to pass: check it, then \
                 forward it with ask_owner (corrected if it needs it), or send it back with \
                 review send_back",
            ),
            Self::NotYourToRecheck => f.write_str(
                "a recheck is asked for by the work's lead or developer: the tester judges what \
                 the checks found, and the CEO directs",
            ),
            Self::CheckPending(check) => write!(
                f,
                "check c{check} of this branch is still waiting or running: its result comes \
                 back as a receipt, and you may ask again once it has"
            ),
            Self::IsTheCeo => f.write_str("the CEO is never furloughed"),
            Self::AlreadyFurloughed => f.write_str("that member is on furlough already"),
            Self::NotFurloughed => f.write_str("that member is at work: there is nothing to recall"),
            Self::HoldsWork { name, count } => write!(
                f,
                "{name} has {count} open task{}: hand {} on to someone else first",
                if *count == 1 { "" } else { "s" },
                if *count == 1 { "it" } else { "them" }
            ),
            Self::OnlyLeadsStaff => f.write_str(
                "only a lead asks for staff; the CEO puts hiring to the board with ask_owner",
            ),
            Self::NotYourProject => {
                f.write_str("only the project's lead asks for its staff: that project is not yours")
            }
            Self::NoBacklog { cited, developers, needs } => write!(
                f,
                "you cited {cited} open task{} of the project, but it has {developers} active developer{}: a request cites at least {needs}",
                if *cited == 1 { "" } else { "s" },
                if *developers == 1 { "" } else { "s" }
            ),
            Self::NotOpenInProject(what) => write!(
                f,
                "{what} is not an open task of this project: cite its open tasks (task 12)"
            ),
            Self::NoteNeedsEvidence => f.write_str(
                "a note cites its evidence: at least one receipt (r12), task (task 12) or event (event 340) from HQ's record",
            ),
            Self::EmptyNote => f.write_str("a note needs words"),
            Self::NoSuchNote(id) => write!(f, "n{id} is not in your notebook"),
            Self::ForgetNeedsReason => {
                f.write_str("say why you forget a note: it was wrong, or is no longer true")
            }
            Self::Furloughed(name) => write!(
                f,
                "{name} is on furlough and takes no work: hand it to a member at work, or ask for staff with request_staff"
            ),
            Self::NotebookFull => write!(
                f,
                "your notebook holds {} notes, the most it may: forget some that are wrong or no longer true first",
                super::notebook::NOTES_MOST
            ),
            Self::CeoDirects(lead) => write!(
                f,
                "this is {lead}'s area: the CEO does not claim, move, close, accept or send back \
                 its work. Give {lead} a direction with direct (task, direction, evidence), and \
                 {lead} decides"
            ),
            Self::CeoToLeads => f.write_str(
                "the CEO hands work to a lead only, and the lead hands it on in its area",
            ),
            Self::NoAreaLead => f.write_str(
                "that task is in no lead's area: there is no lead at work to direct",
            ),
            Self::NotAStrategy => f.write_str(
                "a strategy change gives at least one of purpose, success, measures, bets or not_now",
            ),
            Self::EmptyReport => f.write_str(
                "a report says at least one of moved, stuck, need or proposals",
            ),
            Self::NotAReport => f.write_str(
                "report_area is a lead's, in the turn of its own \"State of my area\" task",
            ),
            Self::ViewTooLong => f.write_str("a huddle's view is three sentences at most"),
            Self::NotInTheHuddle => f.write_str(
                "you are not asked for a view on that task's huddle, or you gave yours already",
            ),
            Self::NotStuckForYou => f.write_str(
                "only the lead of the task's area decides stuck work, once HQ has found it stuck",
            ),
            Self::NotADecisionForStuck => f.write_str(
                "`decision` is proceed, drop, hand_to (with `to`), needs_ceo or needs_board",
            ),
        }
    }
}

/// Whether a member may move its task from one status to another. `done` is never a member's
/// to set: only the gate and a different member's review make it. Nor is `dropped`: work a
/// member doubts goes back to its lead (`todo`, with a reason), and only the owner drops it.
///
/// # Errors
/// The [`Refusal`] that says why not.
pub fn may_set_status(from: TaskStatus, to: TaskStatus) -> Result<(), Refusal> {
    use TaskStatus::{Blocked, Doing, Done, Dropped, InReview, Todo};
    if to == Done {
        return Err(Refusal::OnlyTheGate);
    }
    if to == Dropped {
        return Err(Refusal::HandBack);
    }
    if from.finished() {
        return Err(Refusal::Finished);
    }
    let allowed = matches!(
        (from, to),
        (Todo | Blocked | InReview, Doing) | (Doing, InReview | Blocked | Todo)
    );
    if !allowed {
        return Err(Refusal::NotAStep { from, to });
    }
    Ok(())
}

/// Whether a member may close a routine's check (`done`) from where it is. A check changes
/// no code, so there is nothing for the gate to test: its member closes it once it has
/// looked, saying what it found. Any fix it made went through a contract and is on the owner's
/// desk as a notice (ADR 0107).
///
/// # Errors
/// [`Refusal::Finished`] for a check that is over; [`Refusal::NotACheck`] for one not yet
/// taken.
pub fn may_close_check(from: TaskStatus) -> Result<(), Refusal> {
    if from.finished() {
        return Err(Refusal::Finished);
    }
    if from == TaskStatus::Todo {
        return Err(Refusal::NotACheck);
    }
    Ok(())
}

/// Whether `from` may hand work to `to`: only to members below it.
///
/// # Errors
/// [`Refusal::NotBelow`] otherwise.
pub fn may_delegate(from: Role, to: Role) -> Result<(), Refusal> {
    if to.rank() > from.rank() {
        return Ok(());
    }
    Err(Refusal::NotBelow)
}

/// Whether work is done: the gate passed, and the member approving it is not the one who did
/// it. A member saying it is done changes nothing.
///
/// # Errors
/// The [`Refusal`] that says why it is not done.
pub fn is_done(
    gate_passed: bool,
    worker: Option<MemberId>,
    reviewer: MemberId,
) -> Result<(), Refusal> {
    if worker == Some(reviewer) {
        return Err(Refusal::OwnWork);
    }
    if !gate_passed {
        return Err(Refusal::GateNotPassed);
    }
    Ok(())
}

/// The members named in a thread post (`@name`, case-insensitive), each once, in order.
#[must_use]
pub fn mentions<'a>(text: &str, members: &'a [Member]) -> Vec<&'a Member> {
    let mut found: Vec<&Member> = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || c == ',' || c == '.') {
        let Some(name) = word.strip_prefix('@') else {
            continue;
        };
        let name = name.trim_end_matches(|c: char| !c.is_alphanumeric());
        if let Some(member) = members.iter().find(|m| m.name.eq_ignore_ascii_case(name)) {
            if !found.iter().any(|f| f.id == member.id) {
                found.push(member);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: i64, name: &str, role: Role) -> Member {
        Member {
            id: MemberId(id),
            name: name.to_owned(),
            role,
            share_percent: 25,
            slots: 2,
            look: 0,
            paused: false,
            brain: crate::domain::brain::Brain::Haiku,
            work_brain: crate::domain::brain::Brain::Mac,
            reports_to: None,
            furloughed_at: None,
            recalled_at: None,
        }
    }

    #[test]
    fn a_member_never_says_its_own_work_is_done() {
        for from in [TaskStatus::Todo, TaskStatus::Doing, TaskStatus::InReview] {
            assert_eq!(
                may_set_status(from, TaskStatus::Done),
                Err(Refusal::OnlyTheGate)
            );
        }
    }

    #[test]
    fn a_task_moves_only_by_real_steps_and_not_after_it_is_over() {
        assert!(may_set_status(TaskStatus::Todo, TaskStatus::Doing).is_ok());
        assert!(may_set_status(TaskStatus::Doing, TaskStatus::InReview).is_ok());
        assert!(may_set_status(TaskStatus::Doing, TaskStatus::Blocked).is_ok());
        assert!(may_set_status(TaskStatus::Blocked, TaskStatus::Doing).is_ok());
        assert!(
            may_set_status(TaskStatus::Doing, TaskStatus::Todo).is_ok(),
            "handed back"
        );
        assert_eq!(
            may_set_status(TaskStatus::Doing, TaskStatus::Dropped),
            Err(Refusal::HandBack),
            "doubted work goes back to the lead"
        );
        assert_eq!(
            may_set_status(TaskStatus::Todo, TaskStatus::InReview),
            Err(Refusal::NotAStep {
                from: TaskStatus::Todo,
                to: TaskStatus::InReview
            })
        );
        assert_eq!(
            may_set_status(TaskStatus::Done, TaskStatus::Doing),
            Err(Refusal::Finished)
        );
    }

    #[test]
    fn a_routines_check_is_closed_by_its_member_once_taken_and_only_once() {
        for from in [TaskStatus::Doing, TaskStatus::Blocked, TaskStatus::InReview] {
            assert!(may_close_check(from).is_ok(), "{from:?}");
        }
        assert_eq!(may_close_check(TaskStatus::Todo), Err(Refusal::NotACheck));
        assert_eq!(may_close_check(TaskStatus::Done), Err(Refusal::Finished));
        assert_eq!(may_close_check(TaskStatus::Dropped), Err(Refusal::Finished));
    }

    #[test]
    fn work_is_handed_down_and_never_up_or_across() {
        assert!(may_delegate(Role::Ceo, Role::Lead).is_ok());
        assert!(may_delegate(Role::Ceo, Role::Developer).is_ok());
        assert_eq!(may_delegate(Role::Lead, Role::Ceo), Err(Refusal::NotBelow));
        assert_eq!(may_delegate(Role::Ceo, Role::Ceo), Err(Refusal::NotBelow));
        assert!(may_delegate(Role::Lead, Role::Developer).is_ok());
        assert!(may_delegate(Role::Lead, Role::Reviewer).is_ok());
        assert_eq!(
            may_delegate(Role::Developer, Role::Lead),
            Err(Refusal::NotBelow)
        );
        assert_eq!(
            may_delegate(Role::Developer, Role::Researcher),
            Err(Refusal::NotBelow)
        );
        assert_eq!(may_delegate(Role::Lead, Role::Lead), Err(Refusal::NotBelow));
    }

    #[test]
    fn the_roles_weights_in_the_pool_are_the_owners() {
        let weights: Vec<u8> = [
            Role::Ceo,
            Role::Lead,
            Role::Developer,
            Role::Reviewer,
            Role::Researcher,
        ]
        .map(Role::share)
        .to_vec();
        assert_eq!(weights, [35, 25, 40, 20, 15]);
        assert!(Role::Ceo.leads() && Role::Lead.leads());
        assert!(!Role::Developer.leads() && !Role::Reviewer.leads());
    }

    #[test]
    fn done_needs_the_gate_and_a_different_reviewer() {
        let dev = Some(MemberId(1));
        assert_eq!(is_done(true, dev, MemberId(1)), Err(Refusal::OwnWork));
        assert_eq!(
            is_done(false, dev, MemberId(2)),
            Err(Refusal::GateNotPassed)
        );
        assert!(is_done(true, dev, MemberId(2)).is_ok());
    }

    #[test]
    fn a_post_names_members_once_each() {
        let team = [
            member(1, "Ada", Role::Lead),
            member(2, "Linus", Role::Developer),
        ];
        let named: Vec<i64> = mentions("@ada can you ask @Linus, then @ADA again? @nobody", &team)
            .iter()
            .map(|m| m.id.0)
            .collect();
        assert_eq!(named, [1, 2]);
    }

    #[test]
    fn names_round_trip() {
        for status in [
            TaskStatus::Todo,
            TaskStatus::Doing,
            TaskStatus::InReview,
            TaskStatus::Done,
            TaskStatus::Blocked,
            TaskStatus::Dropped,
        ] {
            assert_eq!(TaskStatus::from_name(status.name()), Some(status));
        }
        for role in [
            Role::Ceo,
            Role::Lead,
            Role::Developer,
            Role::Researcher,
            Role::Reviewer,
        ] {
            assert_eq!(Role::from_name(role.name()), Some(role));
        }
        for p in [
            Priority::Urgent,
            Priority::High,
            Priority::Normal,
            Priority::Low,
        ] {
            assert_eq!(Priority::from_name(p.name()), Some(p));
        }
        for g in [
            GoalStatus::Proposed,
            GoalStatus::Active,
            GoalStatus::Done,
            GoalStatus::Dropped,
        ] {
            assert_eq!(GoalStatus::from_name(g.name()), Some(g));
        }
    }
}
