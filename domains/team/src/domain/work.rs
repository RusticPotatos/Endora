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

/// What a member is hired with as its share of the team's pool when none is given (ADR 0111,
/// 0142): the day's pool is split among the members by their shares and the price class of the
/// Claude each judges with (ADR 0131); a member's work costs nothing.
pub const A_MEMBERS_SHARE: u8 = 40;

/// One worker on the team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Its id.
    pub id: MemberId,
    /// What it is called.
    pub name: String,
    /// Its share of the team pool, in percent.
    pub share_percent: u8,
    /// How many tasks it may work at once.
    pub slots: u8,
    /// Which character it looks like in the office.
    pub look: u8,
    /// Whether the owner has paused it.
    pub paused: bool,
    /// Its model (ADR 0109, 0142): every turn of its runs on it, and the pool's shares are
    /// reckoned on it. For the member of a running turn, as the engine returns it to the team
    /// protocol and the node, this is the brain the turn runs on (a league side's may differ),
    /// so the data tier and the relays see what the turn is.
    pub brain: super::brain::Brain,
    /// When the member was furloughed (ADR 0118), if it ever was: set for the latest furlough.
    /// A furloughed member is never woken, holds no task, and is left out of every briefing's
    /// team and of the share arithmetic; it keeps its record, its name and its notebook.
    pub furloughed_at: Option<Millis>,
    /// When the latest furlough ended, if it has: the member is at work again.
    pub recalled_at: Option<Millis>,
}

impl Member {
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

    /// Whether a task in this status keeps its sandbox between turns (ADR 0142, amended
    /// 2026-11-01): while it is open. Done, dropped or blocked, the runner lets it go; work worth
    /// keeping is kept on the node as a branch.
    #[must_use]
    pub const fn keeps_its_sandbox(self) -> bool {
        !self.finished() && !matches!(self, Self::Blocked)
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

impl Task {
    /// Who holds it: the member working it, else the member it was handed to.
    #[must_use]
    pub const fn holder(&self) -> Option<MemberId> {
        match self.owner {
            Some(owner) => Some(owner),
            None => self.assignee,
        }
    }
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
    /// Work goes to a member through its loop, never from another member (ADR 0142).
    NotBelow,
    /// The task is over.
    Finished,
    /// The work is in review, and work in progress kept now would void its review (ADR 0142,
    /// amended 2026-10-31).
    InReview,
    /// Asked to review its own work.
    OwnWork,
    /// The gate has not passed.
    GateNotPassed,
    /// Work a member doubts is handed back to its lead, never dropped by it.
    HandBack,
    /// The member's last question or approval about the task still waits on the owner's desk.
    AlreadyAsked,
    /// A question about the item already waits on the owner's desk, whoever asked it: an item
    /// has one at a time (ADR 0142, amended 2026-10-31).
    QuestionWaits {
        /// The desk card that waits.
        item: i64,
    },
    /// The owner has answered a question about the item in words: a member's further question
    /// on it is refused with that answer, to act on (ADR 0142, amended 2026-11-01).
    Answered {
        /// The owner's last answer about the item.
        words: String,
    },
    /// The task is not one this turn may touch.
    OutOfReach,
    /// The turn has ended.
    TurnOver,
    /// A handover note was left for a restart, and none is coming (ADR 0134).
    NoRestartComing,
    /// A routine's rhythm is too fast or too slow.
    Rhythm,
    /// A routine's check is closed only by its member, once it is being worked.
    NotACheck,
    /// Longer than it may be, in characters.
    TooLong(usize),
    /// More of something than there may be: criteria, or references cited.
    TooMany {
        /// The most there may be.
        most: usize,
        /// Of what, in the plural.
        of: &'static str,
    },
    /// Not a model Endora has (ADR 0109, 0142).
    NotABrain {
        /// What was asked for.
        asked: String,
    },
    /// Not a runtime a member on its brain may run on (ADR 0117).
    NotARuntime {
        /// What was asked for.
        asked: String,
    },
    /// What the team's world refuses (ADR 0122): its pages, stages, stories and handovers.
    World(super::world::WorldRefusal),
    /// What the guards against how agent teams fail refuse (ADR 0124).
    Guard(super::guards::GuardRefusal),
    /// What the team's history refuses (ADR 0129): transcripts, asking the record, consulting.
    History(super::history::HistoryRefusal),
    /// A deterministic conversation refusal (ADR 0137).
    Conversation(&'static str),
    /// Not in Endora's public library.
    NotADoc,
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
    /// Work handed to another member says what done means (ADR 0114).
    NoCriteria,
    /// Work the owner hands out says what done means too, in at least one line (ADR 0124,
    /// amended): an ask with no definition of done lets a team wander.
    OwnerNeedsCriteria,
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
    /// A criterion asks for a CI run, and none can be had for the item's branch: no workflow at
    /// its head runs on a push to it (ADR 0142, amended 2026-10-31, D6).
    NoCiRun {
        /// The branch.
        branch: String,
        /// The criteria that ask for a CI run.
        criteria: Vec<String>,
        /// Whether the item's loop is worked alone (ADR 0142, amended 2026-11-01): its member,
        /// with nobody to send it back to, blocks it.
        alone: bool,
    },
    /// That is not a step a project takes from where it is.
    NotAProjectStep {
        /// Where it is.
        from: &'static str,
        /// What was asked.
        asked: String,
    },
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
    /// A check of the branch is still waiting or running: one at a time (ADR 0115).
    CheckPending(i64),
    /// No check of the lab's applies to a branch of the repository outside the lab its item
    /// names (ADR 0115, amended 2026-11-01): the item's criteria are its gate.
    NoLabCheck(String),
    /// A criterion past the most one may be, said whole rather than cut (#1049).
    Criterion(String),
    /// Work is not handed to a member on furlough.
    Furloughed(String),
    /// A game of chess refuses a move or a challenge (ADR 0121).
    Chess(super::chess::Refused),
    /// A scenario refuses an action, or its world is not a model's to close (the Holodeck
    /// plan's phase 5): the world's own words.
    Scenario(String),
    /// A downtime turn makes no work: it is for a chess move.
    InDowntime,
    /// Stuck work was to be tried again, but nobody can be woken for it now: nothing was
    /// changed, and the item still waits.
    NotWoken(super::wakes::Refused),
    /// Stuck work was to be tried again, but no member has it to wake.
    NobodyHasIt,
}

impl fmt::Display for Refusal {
    // One message for each refusal: a table, not logic.
    #[allow(clippy::too_many_lines)]
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
            Self::NotBelow => f.write_str(
                "work goes to a member through its loop: you may make work for yourself, not hand \
                 it to another member",
            ),
            Self::Finished => f.write_str("this task is over"),
            Self::OwnWork => f.write_str("work is reviewed by a different member"),
            Self::GateNotPassed => f.write_str("the gate has not passed"),
            Self::InReview => f.write_str(
                "the work is in review: work in progress is not kept, since a new head would \
                 void its review",
            ),
            Self::HandBack => {
                f.write_str("a member does not drop work; hand it back to your lead with a reason")
            }
            Self::OutOfReach => f.write_str(
                "that task is not part of this turn: you act on your own task and its sub-tasks, \
                 and write there, on the task above yours, or where you were named; ask in your \
                 own task's thread instead",
            ),
            Self::TurnOver => f.write_str("this turn has ended: stop now"),
            Self::NoRestartComing => f.write_str(
                "no restart is coming, so there is nothing to wrap up for: carry on, or hand the \
                 task on with hand_to and its handover note",
            ),
            Self::Rhythm => {
                f.write_str("a routine comes round between every 15 minutes and every 30 days")
            }
            Self::NotACheck => f.write_str(
                "only a routine's check is closed by its member, once it is being worked: \
                 claim it first",
            ),
            Self::TooLong(most) => write!(f, "that is longer than {most} characters"),
            Self::TooMany { most, of } => write!(
                f,
                "that is more than the {most} {of} there may be: give fewer, or split the work"
            ),
            Self::NotABrain { asked } => {
                let may: Vec<&str> = super::brain::Brain::all()
                    .into_iter()
                    .map(super::brain::Brain::name)
                    .collect();
                write!(f, "a member thinks with {} only, not {asked:?}", may.join(", "))
            }
            Self::NotARuntime { asked } => write!(
                f,
                "this member runs on the worker, not {asked:?}: Claude Code is for a member on \
                 Claude alone"
            ),
            Self::World(refused) => refused.fmt(f),
            Self::Guard(refused) => refused.fmt(f),
            Self::History(refused) => refused.fmt(f),
            Self::Conversation(reason) => f.write_str(reason),
            Self::NotADoc => f.write_str(
                "read_doc reads Endora's public library only: docs/standards.md, or a path like docs/adr/0111-the-ceo-and-the-chain-of-command.md, docs/plans/endora-hq.md or docs/drafts/ansible-nas-AGENTS.md",
            ),
            Self::NotAName => f.write_str(
                "a member's name is one to 40 characters, and no other member's",
            ),
            Self::AlreadyAsked => f.write_str(
                "your last question or approval about this task is still waiting for an answer; end your turn",
            ),
            Self::QuestionWaits { item } => write!(
                f,
                "a question about this item already waits on the owner's desk (card {item}); \
                 their answer comes to this thread and wakes whoever holds the item: post \
                 what you would add, and end your turn"
            ),
            Self::Answered { words } => write!(
                f,
                "the owner has answered this item already: \"{words}\". Act on that answer; it \
                 is not asked again. If your tools cannot meet it, say what you tried and set \
                 the item blocked with the reason"
            ),
            Self::Covered { task, names } => write!(
                f,
                "#{task} already covers this ({}): add to it with post in #{task}, rather than a \
                 second task",
                names.join(", ")
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
                "a top-up is more than nothing, and a day's top-ups together are at most five \
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
            Self::OwnerNeedsCriteria => f.write_str(
                "the team starts from what done means: say at least one thing that must be \
                 true when this is finished (\"What does done mean?\"; start a line with live: \
                 to have it checked against the lab as it is now)",
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
            Self::NoCiRun {
                branch,
                criteria,
                alone,
            } => write!(
                f,
                "no CI run can be had for {branch}: no workflow at its head runs on a push to it, \
                 so \"{}\" cannot be met as the fork stands (HQ's checks say which). {}, for the \
                 owner to decide",
                criteria.join("\", \""),
                if *alone {
                    "You work this loop alone: block the item with the reason"
                } else {
                    "Send it back saying so"
                }
            ),
            Self::NotAProjectStep { from, asked } => {
                write!(f, "a project that is {from} does not {asked}")
            }
            Self::Chess(refused) => refused.fmt(f),
            Self::Scenario(why) => f.write_str(why),
            Self::InDowntime => f.write_str(
                "this is a downtime turn: it makes no work. Make the chess move it is for, if one \
                 is due, and stop",
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
            Self::CheckPending(check) => write!(
                f,
                "check c{check} of this branch is still waiting or running: its result comes \
                 back as a receipt, and you may ask again once it has"
            ),
            Self::NoLabCheck(repo) => write!(
                f,
                "no check of the lab's applies to {repo}: the item's criteria are its gate, and \
                 its submission opened it. Run the repository's own tests in your sandbox, then \
                 set_status in_review citing your repo_submit's receipt (and the checkout's) for \
                 each criterion"
            ),
            Self::Criterion(why) => f.write_str(why),
            Self::Furloughed(name) => write!(
                f,
                "{name} is on furlough and takes no work: hand it to a member at work, or ask for staff with request_staff"
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

    #[test]
    fn a_downtime_refusal_points_only_at_what_a_downtime_turn_has() {
        // Proposals, votes and notebooks were retired (ADR 0142); a downtime turn is offered
        // my_turn and chess alone.
        let said = Refusal::InDowntime.to_string();
        assert!(said.contains("makes no work"), "{said}");
        assert!(
            !said.contains("notebook") && !said.contains("propos"),
            "{said}"
        );
    }

    fn member(id: i64, name: &str) -> Member {
        Member {
            id: MemberId(id),
            name: name.to_owned(),
            share_percent: 25,
            slots: 2,
            look: 0,
            paused: false,
            brain: crate::domain::brain::Brain::Haiku,
            furloughed_at: None,
            recalled_at: None,
        }
    }

    #[test]
    fn an_open_item_keeps_its_sandbox_and_a_closed_one_lets_it_go() {
        for open in [TaskStatus::Todo, TaskStatus::Doing, TaskStatus::InReview] {
            assert!(open.keeps_its_sandbox(), "{open:?}");
        }
        for closed in [TaskStatus::Done, TaskStatus::Dropped, TaskStatus::Blocked] {
            assert!(!closed.keeps_its_sandbox(), "{closed:?}");
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
        let team = [member(1, "Ada"), member(2, "Linus")];
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
