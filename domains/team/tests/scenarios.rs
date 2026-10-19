//! HQ's scenarios (docs/testing.md): the owner's words in `features/*.feature`, run against the
//! real engine through its public use cases and the team protocol's tools, over HQ's SQLite
//! store in memory, the simulated runner and a clock the steps move. Nothing here reaches into
//! the engine: a step does what the owner, the node or a member's worker would do, and reads
//! what the HQ screen would show.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use cucumber::cli::Parser as _;
use cucumber::{World, given, then, when};
use endora_team::application::engine::Called;
use endora_team::application::protocol::call;
use endora_team::application::{
    Author, Clock, Engine, NewRoutine, NewTask, RunContext, Settings, Submission, TeamStore,
    WingSeeded,
};
use endora_team::domain::money::{DAY_MS, DOLLAR};
use endora_team::domain::routines::HOUR_MS;
use endora_team::domain::{
    Criterion, DeskId, GoalId, MemberId, Micros, Pause, Priority, ProjectId, ProjectStatus,
    Receipt, Ref, Refused, Role, TaskId, WakeReason,
};
use endora_team::infrastructure::{SimulatedRunner, SqliteTeamStore};
use serde_json::json;

/// The team's world (ADR 0122), in its own file.
#[path = "scenarios/world.rs"]
mod world;

/// The guards (ADR 0124), in their own file.
#[path = "scenarios/guards.rs"]
mod guards;

/// The team's map (ADR 0126), in its own file.
#[path = "scenarios/map.rs"]
mod map;

/// The team's history (ADR 0129), in its own file.
#[path = "scenarios/history.rs"]
mod history;

/// The Mac's brain (ADR 0130), in its own file.
#[path = "scenarios/mac.rs"]
mod mac;

/// Two brains per member and the kinds of turn (ADR 0131), in their own file.
#[path = "scenarios/brains.rs"]
mod brains;

/// What the team's work came to (ADR 0132), in its own file.
#[path = "scenarios/outcomes.rs"]
mod outcomes;

/// A desk that interrupts only for decisions (ADR 0132), in its own file.
#[path = "scenarios/desk.rs"]
mod desk;

/// 17 October 2026, at noon UTC: the day of the live incident the first scenarios come from.
const OCT_17: i64 = 1_792_238_400_000;

/// A clock the steps move by hand.
#[derive(Clone)]
struct Hands(Arc<AtomicI64>);

impl Clock for Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq = Engine<SqliteTeamStore, SimulatedRunner, Hands>;

/// One scenario's HQ: the engine and what the steps have named so far.
#[derive(World)]
struct Office {
    hq: Hq,
    /// Members by name, as the owner hired them.
    members: HashMap<String, MemberId>,
    /// The goal the scenario set.
    goal: Option<GoalId>,
    /// Projects by title.
    projects: HashMap<String, ProjectId>,
    /// The last turn each member's step started, by name.
    turns: HashMap<String, RunContext>,
    /// What the engine last refused, as the member or the owner reads it; `None` when the
    /// last thing asked was done.
    refused: Option<String>,
    /// Whether HQ seeded its house log the last time it started.
    seeded: Option<bool>,
    /// The engineering wing, once seeded (ADR 0115).
    wing: Option<WingSeeded>,
    /// The push the owner last decided on.
    push: Option<DeskId>,
    /// The branch the last checkout was of, the head HQ handed out for it, and its receipt.
    checkout: Option<(String, String, i64)>,
    /// The clock the engine reads, for steps that let days pass.
    clock: Hands,
    /// The receipt of the last reading of the lab a step took.
    reading: Option<i64>,
    /// What the owner's last retraction said (ADR 0124): its id, and its sweep.
    retraction: Option<serde_json::Value>,
    /// What the record last answered a member (ADR 0129), and how many wakes there were when
    /// it was asked.
    answer: Option<String>,
    wakes_at_ask: usize,
    /// The file HQ keeps its team in, when a scenario is about HQ restarting (ADR 0130).
    file: Option<String>,
    /// The pool for a month, when a scenario is about money (ADR 0131).
    pool_per_month: Micros,
    /// Whether the scenario has said what the node says about the owner's Mac. Until it does,
    /// the Mac is on and free, as it is most of the day (ADR 0131: code work waits for it).
    mac_said: bool,
    /// How many wakes there were when a turn began to wait, for what HQ does about work whose
    /// turn waits (ADR 0131): only what came after is held against it.
    wakes_when_waiting: usize,
}

impl Office {
    /// The settings this scenario's HQ starts with.
    fn settings(&self) -> Settings {
        Settings {
            pool_per_month: self.pool_per_month,
            ..Settings::default()
        }
    }

    /// One tick of HQ. The node says the Mac is free before every tick, as it does while it is,
    /// unless the scenario has said otherwise.
    fn tick(&mut self) {
        if !self.mac_said {
            self.hq.tell_mac(true, "ready");
        }
        self.hq.tick().expect("the tick runs");
    }
}

impl Default for Office {
    fn default() -> Self {
        let clock = Hands(Arc::new(AtomicI64::new(OCT_17)));
        Self {
            hq: Engine::new(
                SqliteTeamStore::in_memory().expect("an in-memory store"),
                SimulatedRunner::new(4),
                clock.clone(),
                Settings::default(),
            ),
            clock,
            members: HashMap::new(),
            goal: None,
            projects: HashMap::new(),
            turns: HashMap::new(),
            refused: None,
            seeded: None,
            wing: None,
            push: None,
            checkout: None,
            reading: None,
            retraction: None,
            answer: None,
            wakes_at_ask: 0,
            file: None,
            pool_per_month: endora_team::domain::money::POOL_PER_MONTH,
            mac_said: false,
            wakes_when_waiting: 0,
        }
    }
}

impl fmt::Debug for Office {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Office")
            .field("members", &self.members)
            .field("goal", &self.goal)
            .field("projects", &self.projects)
            .field("refused", &self.refused)
            .finish_non_exhaustive()
    }
}

impl Office {
    fn member(&self, name: &str) -> MemberId {
        *self
            .members
            .get(name)
            .unwrap_or_else(|| panic!("nobody on the team is called {name}"))
    }

    fn goal(&self) -> GoalId {
        self.goal.expect("the scenario set a goal")
    }

    /// The task with this title, as the HQ screen would find it.
    fn task(&self, title: &str) -> TaskId {
        self.hq
            .store()
            .tasks()
            .expect("the store answers")
            .into_iter()
            .find(|t| t.title == title)
            .unwrap_or_else(|| panic!("no task is titled {title:?}"))
            .id
    }

    fn project(&self, title: &str) -> endora_team::domain::Project {
        let id = *self
            .projects
            .get(title)
            .unwrap_or_else(|| panic!("no project is titled {title:?}"));
        self.hq
            .store()
            .projects()
            .expect("the store answers")
            .into_iter()
            .find(|p| p.id == id)
            .expect("the project is in the store")
    }

    /// Remembers whether something was refused, and why.
    fn note<T, E: fmt::Display>(&mut self, result: Result<T, E>) -> Option<T> {
        match result {
            Ok(value) => {
                self.refused = None;
                Some(value)
            }
            Err(why) => {
                self.refused = Some(why.to_string());
                None
            }
        }
    }

    /// `member`'s turn, running now: the tick starts what is queued, and the newest turn the
    /// runner started for the member must still be running.
    fn turn_of(&mut self, member: &str) -> RunContext {
        self.tick();
        let id = self.member(member);
        let context = self
            .hq
            .runner()
            .started()
            .into_iter()
            .rfind(|c| c.member == id)
            .unwrap_or_else(|| {
                // What is under way, so a turn that waits says for whom.
                let live: Vec<_> = self
                    .hq
                    .store()
                    .runs()
                    .expect("the store answers")
                    .into_iter()
                    .filter(|r| r.status.live())
                    .map(|r| {
                        format!(
                            "member {} on {:?}: {:?} {:?}",
                            r.member.0, r.task, r.status, r.end_reason
                        )
                    })
                    .collect();
                panic!("no turn of {member}'s has started; turns under way: {live:#?}")
            });
        assert!(
            self.hq
                .turn_of(&context.token)
                .expect("the store answers")
                .is_some(),
            "{member}'s turn is over"
        );
        self.turns.insert(member.to_owned(), context.clone());
        context
    }

    /// `member` uses one of the team protocol's tools in its turn, as its worker would.
    fn use_tool(&mut self, member: &str, tool: &str, args: &serde_json::Value) -> Option<String> {
        let context = self.turn_of(member);
        let (run, who) = self
            .hq
            .turn_of(&context.token)
            .expect("the store answers")
            .expect("a running turn");
        let said = call(&self.hq, &run, &who, tool, args);
        self.note(said)
    }

    fn last_wake(&self) -> endora_team::application::ports::Wake {
        self.hq
            .wakes()
            .expect("the store answers")
            .pop()
            .expect("somebody was woken")
    }

    fn waiting_on_desk(&self, kind: &str) -> Vec<endora_team::application::ports::DeskItem> {
        self.hq
            .store()
            .desk()
            .expect("the store answers")
            .into_iter()
            .filter(|d| d.status == "waiting" && (kind.is_empty() || d.kind == kind))
            .collect()
    }

    fn add_task(&mut self, by: Author, task: NewTask) -> Option<TaskId> {
        let made = self.hq.add_task(by, task);
        self.note(made)
    }
}

/// Dollars as the owner writes them, in the ledger's micros.
fn micros(dollars: f64) -> Micros {
    (dollars * DOLLAR as f64).round() as Micros
}

// --- The team ----------------------------------------------------------------------------

/// The owner hires a member by role; the CEO, HQ seats as it does on start (ADR 0111).
#[given(expr = "{word} is the {word}")]
fn hire(w: &mut Office, name: String, role: String) {
    if role == "CEO" {
        let id =
            w.hq.seat_ceo(&name)
                .expect("HQ seats a CEO")
                .expect("no CEO yet");
        w.members.insert(name, id);
        return;
    }
    let (role, share) = match role.as_str() {
        "lead" => (Role::Lead, 25),
        "developer" => (Role::Developer, 40),
        // The reviewer's role is what the owner reads as the tester (ADR 0114).
        "reviewer" | "tester" => (Role::Reviewer, 20),
        "researcher" => (Role::Researcher, 15),
        other => panic!("the team has no role called {other}"),
    };
    let id = w.hq.hire(&name, role, share, 2, 0).expect("HQ hires");
    w.members.insert(name, id);
}

#[given(expr = "{word} thinks with Claude")]
fn thinks_with_claude(w: &mut Office, name: String) {
    let id = w.member(&name);
    let role =
        w.hq.store()
            .members()
            .expect("the store answers")
            .into_iter()
            .find(|m| m.id == id)
            .expect("hired")
            .role;
    let brain = if role == Role::Researcher {
        "claude-haiku-5-5"
    } else {
        "claude-sonnet-5-5"
    };
    w.hq.set_brain(id, brain)
        .expect("a brain the role may think with");
}

#[when(regex = r#"^the owner sets (\w+)'s brain to "(.+)"$"#)]
fn set_brain(w: &mut Office, name: String, brain: String) {
    let id = w.member(&name);
    let set = w.hq.set_brain(id, &brain);
    w.note(set);
}

#[then(expr = "{word} thinks with {string}")]
fn thinks_with(w: &mut Office, name: String, brain: String) {
    let id = w.member(&name);
    let found =
        w.hq.store()
            .members()
            .expect("the store answers")
            .into_iter()
            .find(|m| m.id == id)
            .expect("hired");
    assert_eq!(found.brain.name(), brain);
}

#[given(expr = "the goal {string}")]
fn set_goal(w: &mut Office, title: String) {
    let goal =
        w.hq.set_goal(&title, "", Author::Owner)
            .expect("the owner sets a goal");
    w.goal = Some(goal);
}

#[given(expr = "the home lab has the containers {string}")]
fn lab_names(w: &mut Office, names: String) {
    w.hq.set_lab_names(
        names
            .split(',')
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty())
            .collect(),
    );
}

// --- Tasks and routines --------------------------------------------------------------------

#[given(expr = "{word} has the task {string}")]
#[when(expr = "{word} is handed the task {string}")]
fn owner_hands(w: &mut Office, name: String, title: String) {
    let to = w.member(&name);
    let goal = w.goal();
    w.add_task(
        Author::Owner,
        NewTask {
            goal,
            project: None,
            parent: None,
            title,
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .expect("the owner may hand out what they like");
}

#[given(expr = "{word} has the task {string} in the project {string}")]
#[when(expr = "{word} is handed the task {string} in the project {string}")]
fn owner_hands_in_project(w: &mut Office, name: String, title: String, project: String) {
    let to = w.member(&name);
    let goal = w.goal();
    let project = w.project(&project).id;
    w.add_task(
        Author::Owner,
        NewTask {
            goal,
            project: Some(project),
            parent: None,
            title,
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .expect("the owner may hand out what they like");
}

#[given(expr = "{word} hands {word} the task {string} under {string}")]
#[when(expr = "{word} hands {word} the task {string} under {string}")]
fn member_hands_on(w: &mut Office, by: String, to: String, title: String, parent: String) {
    let by = w.member(&by);
    let to = w.member(&to);
    let goal = w.goal();
    let parent = w.task(&parent);
    w.add_task(
        Author::Member(by),
        NewTask {
            goal,
            project: None,
            parent: Some(parent),
            title,
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    );
}

#[given(expr = "the routine {string} for {word}, which takes the home lab's alarms")]
fn routine_taking_alarms(w: &mut Office, title: String, name: String) {
    let assignee = w.member(&name);
    let goal = w.goal();
    w.hq.add_routine(
        Author::Owner,
        NewRoutine {
            title,
            description: "Look at the home lab with homelab_status.".to_owned(),
            goal,
            project: None,
            assignee,
            every_ms: 6 * HOUR_MS,
            on_lab_trouble: true,
        },
    )
    .expect("the owner adds a routine");
}

#[given(expr = "Endora raised the alarm {string}")]
#[when(expr = "Endora raises the alarm {string}")]
fn alarm(w: &mut Office, said: String) {
    w.hq.alarm(&said).expect("the node's alarm is taken");
}

#[when(expr = "Endora says {string}")]
fn recovered(w: &mut Office, said: String) {
    w.hq.recovered(&said).expect("the node's news is taken");
}

#[when(expr = "{word} is woken about {string}")]
fn woken_about(w: &mut Office, name: String, title: String) {
    let member = w.member(&name);
    let task = w.task(&title);
    w.hq.wake(member, Some(task), WakeReason::Assigned)
        .expect("a wake has a receipt");
}

#[then(regex = r"^the team (?:still )?has (\d+) tasks?$")]
fn task_count(w: &mut Office, count: usize) {
    assert_eq!(
        w.hq.store().tasks().expect("the store answers").len(),
        count
    );
}

#[then("the team has no task")]
fn no_task(w: &mut Office) {
    task_count(w, 0);
}

#[then(expr = "{string} is the last post in the task {string}")]
fn last_post(w: &mut Office, post: String, title: String) {
    let task = w.task(&title);
    let last =
        w.hq.store()
            .thread(task)
            .expect("the store answers")
            .pop()
            .expect("something was posted");
    assert_eq!(format!("{}: {}", last.author, last.text), post);
}

// --- Turns, tools and the chain of command ----------------------------------------------------

#[when(regex = r"^(\w+)'s turn starts$")]
fn turn_starts(w: &mut Office, name: String) {
    w.turn_of(&name);
}

#[then(regex = r#"^(\w+)'s turn starts on "(.+)"$"#)]
fn turn_starts_on(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let context = w.turn_of(&name);
    assert_eq!(context.task, Some(task), "{name}'s turn is on another task");
}

#[then(regex = r#"^(\w+)'s briefing says "(.+)"$"#)]
fn briefing_says(w: &mut Office, name: String, text: String) {
    let told = &w
        .turns
        .get(&name)
        .unwrap_or_else(|| panic!("{name}'s turn has not started"))
        .told;
    assert!(told.contains(&text), "{name} was told:\n{told}");
}

#[when(expr = "{word} asks {string}")]
fn asks(w: &mut Office, name: String, text: String) {
    // A question up the chain rests on the asker's own task (ADR 0114).
    let task = w.turn_of(&name).task.expect("a turn on a task");
    w.use_tool(
        &name,
        "ask_owner",
        &json!({ "text": text, "evidence": [format!("task {}", task.0)] }),
    )
    .expect("the question goes somewhere");
}

#[when(
    expr = "{word} puts the decision {string} to the board, recommending {string}, because {string}"
)]
fn decides(w: &mut Office, name: String, decision: String, recommendation: String, why: String) {
    // What goes to the board rests on the CEO's own task (ADR 0114).
    let task = w.turn_of(&name).task.expect("a turn on a task");
    w.use_tool(
        &name,
        "ask_owner",
        &json!({
            "decision": decision, "recommendation": recommendation, "why": why,
            "evidence": [format!("task {}", task.0)]
        }),
    )
    .expect("the CEO's decision goes to the board");
}

#[when(expr = "{word} posts {string}")]
fn posts(w: &mut Office, name: String, text: String) {
    let context = w.turn_of(&name);
    let task = context.task.expect("a turn on a task");
    w.use_tool(&name, "post", &json!({ "task": task.0, "text": text }))
        .expect("a member may post in its own task");
}

#[then(expr = "the question goes to {word}")]
fn question_goes_to(w: &mut Office, name: String) {
    let member = w.member(&name);
    let wake = w.last_wake();
    assert_eq!(
        (wake.member, wake.reason),
        (member, WakeReason::QuestionAsked),
        "the last wake: {wake:?}"
    );
    assert!(
        !matches!(wake.receipt, Receipt::Refused(_)),
        "the question was refused: {wake:?}"
    );
    let task = wake.task.expect("a question is about a task");
    let asked = format!("Question for {name}:");
    assert!(
        w.hq.store()
            .thread(task)
            .expect("the store answers")
            .iter()
            .any(|p| p.text.starts_with(&asked)),
        "nothing in the thread says {asked:?}"
    );
}

#[then(expr = "{word} is woken about it")]
fn woken(w: &mut Office, name: String) {
    let member = w.member(&name);
    let wake = w.last_wake();
    assert_eq!(wake.member, member, "the last wake: {wake:?}");
    assert!(
        !matches!(wake.receipt, Receipt::Refused(_)),
        "the wake was refused: {wake:?}"
    );
}

#[then(expr = "{word} is not woken: {word}")]
fn not_woken(w: &mut Office, name: String, why: String) {
    let member = w.member(&name);
    let wake = w.last_wake();
    assert_eq!(wake.member, member, "the last wake: {wake:?}");
    let Receipt::Refused(refused) = wake.receipt else {
        panic!("{name} was woken: {wake:?}");
    };
    assert_eq!(refused.name(), why);
}

#[then(regex = r"^(\w+)'s turn is refused: (\w+)$")]
fn turn_refused(w: &mut Office, name: String, why: String) {
    not_woken(w, name, why);
}

#[then("the owner's desk is empty")]
fn desk_empty(w: &mut Office) {
    let desk = w.waiting_on_desk("");
    assert!(desk.is_empty(), "on the desk: {desk:?}");
}

#[then(regex = r"^the owner's desk holds (\d+) decisions?$")]
fn desk_decisions(w: &mut Office, count: usize) {
    assert_eq!(w.waiting_on_desk("decision").len(), count);
}

// --- Refusals -------------------------------------------------------------------------------

#[then(expr = "it is refused: {string}")]
fn refused_saying(w: &mut Office, why: String) {
    assert_eq!(w.refused.as_deref(), Some(why.as_str()));
}

#[then("it is refused")]
fn refused(w: &mut Office) {
    assert!(w.refused.is_some(), "it was done");
}

#[then("it is not refused")]
fn not_refused(w: &mut Office) {
    assert_eq!(w.refused, None);
}

// --- Projects and money ---------------------------------------------------------------------

#[given(regex = r#"^the project "(.+)" led by (\w+) with a cap of \$(\d+(?:\.\d+)?) a month$"#)]
fn add_project(w: &mut Office, title: String, lead: String, cap: f64) {
    let lead = w.member(&lead);
    let goal = w.goal();
    let id =
        w.hq.add_project(goal, &title, "", lead, Some(micros(cap)))
            .expect("the owner adds a project");
    w.projects.insert(title, id);
}

#[when(regex = r#"^(\w+) proposes the project "(.+)" with a cap of \$(\d+(?:\.\d+)?) a month$"#)]
fn propose_project(w: &mut Office, name: String, title: String, cap: f64) {
    let context = w.turn_of(&name);
    let task = context.task.expect("a turn on a task");
    w.use_tool(
        &name,
        "propose_project",
        &json!({ "title": title, "reason": "it keeps failing", "tasks": [task.0], "cap": cap }),
    )
    .expect("a lead proposes");
    let id =
        w.hq.store()
            .projects()
            .expect("the store answers")
            .into_iter()
            .find(|p| p.title == title)
            .expect("the proposal is a project")
            .id;
    w.projects.insert(title, id);
}

#[when(expr = "{word} accepts the project {string} because {string}")]
fn accept_project(w: &mut Office, name: String, title: String, why: String) {
    // The CEO's decision rests on what the proposal cited (ADR 0114).
    let project = w.project(&title);
    let evidence: Vec<String> = project
        .evidence
        .iter()
        .map(|e| match e {
            endora_team::domain::Evidence::Task(t) => format!("task {}", t.0),
            endora_team::domain::Evidence::Event(e) => format!("event {e}"),
        })
        .collect();
    w.use_tool(
        &name,
        "decide_project",
        &json!({ "project": project.id.0, "verdict": "accept", "why": why, "evidence": evidence }),
    )
    .expect("the CEO decides");
}

#[then(expr = "the project {string} is {word}")]
fn project_status(w: &mut Office, title: String, status: String) {
    let project = w.project(&title);
    let expected = match status.as_str() {
        "proposed" => ProjectStatus::Proposed,
        "active" => ProjectStatus::Active,
        "paused" => ProjectStatus::Paused,
        "done" => ProjectStatus::Done,
        "dropped" => ProjectStatus::Dropped,
        other => panic!("a project is never {other}"),
    };
    assert_eq!(project.status, expected, "{project:?}");
}

#[then(expr = "the project {string} is paused for its budget")]
fn project_paused_for_budget(w: &mut Office, title: String) {
    let project = w.project(&title);
    assert_eq!(
        (project.status, project.pause),
        (ProjectStatus::Paused, Some(Pause::Budget)),
        "{project:?}"
    );
}

#[then(expr = "the project {string} has no cap")]
fn project_has_no_cap(w: &mut Office, title: String) {
    assert_eq!(w.project(&title).cap, None);
}

#[then(regex = r#"^the project "(.+)" has a cap of \$(\d+(?:\.\d+)?)$"#)]
fn project_has_cap(w: &mut Office, title: String, cap: f64) {
    assert_eq!(w.project(&title).cap, Some(micros(cap)));
}

#[then(regex = r#"^the owner's desk holds a cap request for "(.+)" of \$(\d+\.\d\d) a month$"#)]
fn desk_cap_request(w: &mut Office, title: String, dollars: String) {
    let project = w.project(&title);
    let items = w.waiting_on_desk("project_cap");
    assert_eq!(items.len(), 1, "cap requests waiting: {items:?}");
    let expected = format!(
        "A cap for project #{} {}: ${dollars} a month.",
        project.id.0, title
    );
    assert!(items[0].text.starts_with(&expected), "{}", items[0].text);
}

#[when(regex = r"^the owner grants the cap at \$(\d+(?:\.\d+)?)$")]
fn grant_cap(w: &mut Office, dollars: String) {
    let item = w
        .waiting_on_desk("project_cap")
        .pop()
        .expect("a cap request waits on the desk");
    w.hq.decide_saying(item.id, "approve", &dollars)
        .expect("the owner decides");
}

#[when(regex = r#"^(\w+)'s turns on "(.+)" spend \$(\d+(?:\.\d+)?) and then \$(\d+(?:\.\d+)?)$"#)]
fn turns_spend(w: &mut Office, name: String, title: String, first: f64, second: f64) {
    let member = w.member(&name);
    let task = w.task(&title);
    for amount in [first, second] {
        // Only a turn of judgment is paid for (ADR 0131): the member is asked to decide.
        w.hq.wake(member, Some(task), WakeReason::Decide)
            .expect("a wake has a receipt");
        let context = w.turn_of(&name);
        assert_eq!(context.task, Some(task), "{name}'s turn is on another task");
        w.hq.report_usage(context.run, micros(amount))
            .expect("the ledger takes it");
        w.hq.runner().finish(context.run, true);
        w.tick();
    }
}

impl Office {
    /// The owner's day, as the ledger counts it: these scenarios keep to 17 October.
    fn today(&self) -> i64 {
        endora_team::domain::money::day_of(OCT_17, 0)
    }
}

/// A task Endora hands a member, not the owner: its turns are the member's own.
fn endora_hands(w: &mut Office, name: &str, title: &str) {
    let to = w.member(name);
    let goal = w.goal();
    w.add_task(
        Author::Endora,
        NewTask {
            goal,
            project: None,
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .expect("Endora may hand out work");
}

/// One turn of `name`'s on `title`, as it starts next, spending `amount` and ending.
fn turn_spends(w: &mut Office, name: &str, title: &str, amount: Micros) {
    let task = w.task(title);
    let context = w.turn_of(name);
    assert_eq!(context.task, Some(task), "{name}'s turn is on another task");
    w.hq.report_usage(context.run, amount)
        .expect("the ledger takes it");
    w.hq.runner().finish(context.run, true);
    w.tick();
}

#[given(regex = r"^(\w+) has spent \$(\d+(?:\.\d+)?) of (?:her|his|its) share today$")]
fn spent_of_share(w: &mut Office, name: String, dollars: f64) {
    let title = format!("{name}'s earlier work");
    endora_hands(w, &name, &title);
    let member = w.member(&name);
    let mut left = micros(dollars);
    while left > 0 {
        let amount = left.min(endora_team::domain::money::MOST_A_TURN);
        // Only a turn of judgment is paid for (ADR 0131): the member is asked to decide, and
        // the first of these joins the turn Endora's handing queued.
        let task = w.task(&title);
        w.hq.wake(member, Some(task), WakeReason::Decide)
            .expect("a wake has a receipt");
        turn_spends(w, &name, &title, amount);
        left -= amount;
    }
}

#[when(expr = "Endora hands {word} the task {string}")]
fn endora_hands_step(w: &mut Office, name: String, title: String) {
    endora_hands(w, &name, &title);
}

#[when(regex = r#"^(\w+)'s turn on "(.+)" spends \$(\d+(?:\.\d+)?)$"#)]
fn one_turn_spends(w: &mut Office, name: String, title: String, dollars: f64) {
    turn_spends(w, &name, &title, micros(dollars));
}

#[then(regex = r"^(\w+)'s share for today is spent$")]
fn share_spent(w: &mut Office, name: String) {
    let member = w.member(&name);
    let snap = w.hq.snapshot().expect("the snapshot is built");
    let share = snap["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["id"] == member.0)
        .expect("on the team")["per_day"]
        .as_i64()
        .expect("a share");
    let used =
        w.hq.store()
            .share_committed(member, w.today())
            .expect("the store answers");
    assert!(used >= share, "{name} used {used} of {share}");
}

#[then(regex = r"^the pool shows \$(\d+\.\d\d) spent today$")]
fn pool_spent(w: &mut Office, dollars: f64) {
    let snap = w.hq.snapshot().expect("the snapshot is built");
    assert_eq!(snap["pool"]["spent"].as_i64(), Some(micros(dollars)));
}

#[then(regex = r"^(\w+)'s share shows \$(\d+\.\d\d) spent today$")]
fn share_shows(w: &mut Office, name: String, dollars: f64) {
    let member = w.member(&name);
    let used =
        w.hq.store()
            .share_committed(member, w.today())
            .expect("the store answers");
    assert_eq!(used, micros(dollars));
}

// --- How the team is doing ------------------------------------------------------------------

impl Office {
    /// Today's scorecard, as the HQ screen's "How the team is doing" card reads it.
    fn scorecard_today(&self) -> serde_json::Value {
        self.hq.snapshot().expect("the snapshot is built")["scorecard"]["today"].clone()
    }
}

#[when(regex = r"^(\w+)'s turn ends$")]
fn turn_ends(w: &mut Office, name: String) {
    let context = w
        .turns
        .get(&name)
        .unwrap_or_else(|| panic!("{name}'s turn has not started"))
        .clone();
    w.hq.runner().finish(context.run, true);
    w.tick();
}

/// The member takes its task and hands it in, citing the task as what counts (the owner's
/// work gave no criteria), and its turn ends.
#[when(expr = "{word} submits {string} for review")]
fn submits(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    w.use_tool(&name, "claim", &json!({ "task": task.0 }))
        .expect("the member takes its task");
    w.use_tool(
        &name,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "ready for a look",
                 "evidence": [format!("task {}", task.0)] }),
    )
    .expect("the work goes to review");
    // Its turn is over; the tester's wake was held while it ran on the task.
    turn_ends(w, name);
}

#[when(expr = "{word} sends back {string} because {string}")]
fn sends_back(w: &mut Office, name: String, title: String, notes: String) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "review",
        &json!({ "task": task.0, "verdict": "send_back", "notes": notes }),
    )
    .expect("the tester sends it back");
}

#[when(expr = "the owner drops the task {string}")]
fn owner_drops(w: &mut Office, title: String) {
    let task = w.task(&title);
    w.hq.drop_task(task).expect("the owner drops it");
}

#[then(regex = r"^the scorecard counts (\d+) turns? today, (\d+) of them showing nothing$")]
fn scorecard_turns(w: &mut Office, turns: usize, nothing: usize) {
    let card = w.scorecard_today();
    assert_eq!(card["turns"]["total"], json!(turns), "{card}");
    assert_eq!(card["turns"]["nothing"], json!(nothing), "{card}");
    assert_eq!(card["turns"]["unlabelled"], json!(0), "{card}");
}

#[then(regex = r"^the scorecard counts (\d+) submissions? today, (\d+) of them sent back$")]
fn scorecard_review(w: &mut Office, submissions: usize, sent_back: usize) {
    let card = w.scorecard_today();
    assert_eq!(card["review"]["submissions"], json!(submissions), "{card}");
    assert_eq!(card["review"]["sent_back"], json!(sent_back), "{card}");
}

#[then(regex = r"^the scorecard puts \$(\d+(?:\.\d+)?) of today's spend on dropped tasks$")]
fn scorecard_waste(w: &mut Office, dollars: f64) {
    let card = w.scorecard_today();
    assert_eq!(card["finished"]["dropped"], json!(1), "{card}");
    assert_eq!(
        card["finished"]["dropped_spent"],
        json!(micros(dollars)),
        "{card}"
    );
}

// --- The house log --------------------------------------------------------------------------

#[when("HQ starts with an empty house log")]
#[when("the owner clears the house log and HQ starts again")]
fn hq_starts(w: &mut Office) {
    if w.seeded.is_some() {
        w.hq.set_house_log("").expect("the owner clears the log");
    }
    w.seeded = Some(w.hq.seed_house_log().expect("HQ starts"));
}

#[then(expr = "the house log says {string}")]
fn house_log_says(w: &mut Office, text: String) {
    assert_eq!(w.seeded, Some(true), "HQ did not seed the log");
    let log = w.hq.house_log().expect("the store answers");
    assert!(log.contains(&text), "the house log:\n{log}");
}

#[then("the house log stays empty")]
fn house_log_empty(w: &mut Office) {
    assert_eq!(w.seeded, Some(false), "HQ seeded the log again");
    assert_eq!(w.hq.house_log().expect("the store answers"), "");
}

// --- The engineering wing (ADR 0115) --------------------------------------------------------

/// HQ seeds the wing as it does on start: Kit the lead, Rae and Zed the developers, the goal
/// "Improve the lab" and the project "ansible-nas clean-up", with Kit's first task.
#[given("the engineering wing")]
fn the_wing(w: &mut Office) {
    let seeded =
        w.hq.seed_wing()
            .expect("HQ seeds the wing")
            .expect("not seeded yet");
    for m in w.hq.store().members().expect("the store answers") {
        w.members.entry(m.name.clone()).or_insert(m.id);
    }
    w.projects
        .insert("ansible-nas clean-up".to_owned(), seeded.project);
    w.wing = Some(seeded);
}

/// A member's first turn, the one HQ queues when it seeds the wing (Kit's, on the plan), is over.
/// The Mac takes one turn at a time (ADR 0131), and the scenario is about the others'.
#[given(expr = "{word}'s planning turn is over")]
fn planning_turn_is_over(w: &mut Office, name: String) {
    let context = w.turn_of(&name);
    w.hq.runner().finish(context.run, true);
    w.tick();
}

fn wing(w: &Office) -> &WingSeeded {
    w.wing.as_ref().expect("the scenario has the wing")
}

#[given(expr = "{word} hands {word} the task {string} in the wing")]
fn wing_hands(w: &mut Office, by: String, to: String, title: String) {
    hand_in_the_wing(
        w,
        &by,
        &to,
        title,
        vec![Criterion::new("The branch passes its checks", false)],
    );
}

/// As above, with what done means as the owner wrote it: a line of the step's table each.
#[given(expr = "{word} hands {word} the task {string} in the wing, with the criteria")]
fn wing_hands_with_criteria(
    w: &mut Office,
    by: String,
    to: String,
    title: String,
    step: &cucumber::gherkin::Step,
) {
    let criteria: Vec<Criterion> = step
        .table
        .as_ref()
        .expect("the criteria are the step's table")
        .rows
        .iter()
        .map(|row| Criterion::new(&row[0], false))
        .collect();
    hand_in_the_wing(w, &by, &to, title, criteria);
}

fn hand_in_the_wing(w: &mut Office, by: &str, to: &str, title: String, criteria: Vec<Criterion>) {
    let by = w.member(by);
    let to = w.member(to);
    let (goal, project, plan) = (wing(w).goal, wing(w).project, wing(w).task);
    w.hq.add_task_with(
        Author::Member(by),
        NewTask {
            goal,
            project: Some(project),
            parent: Some(plan),
            title,
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
        &criteria,
    )
    .expect("the lead hands work on");
}

/// The head the node's record of a submission has in these scenarios, and the base under it.
const SUBMITTED_HEAD: &str = "a1b2c3d4e5f60718293a4b5c";
const SUBMITTED_BASE: &str = "0123456789abcdef01234567";

/// The node took the member's branch (`repo_submit`) and tells HQ, with the turn's token; and
/// the receipt of that call, as the node writes one (an act, which the member may cite).
#[when(expr = "{word} submits the branch {string}")]
fn submits_branch(w: &mut Office, name: String, branch: String) {
    let context = w.turn_of(&name);
    w.hq.record_branch(
        &context.token,
        &Submission {
            branch: branch.clone(),
            head: SUBMITTED_HEAD.to_owned(),
            base: SUBMITTED_BASE.to_owned(),
            files: 1,
            plus: 4,
            minus: 1,
            stat: " roles/deluge/tasks/main.yml | 5 ++--".to_owned(),
            diff: "-old\n+new".to_owned(),
            can_push: true,
        },
    )
    .expect("the node's record is taken");
    w.hq.record_receipt(
        &context.token,
        &Called {
            tool: "repo_submit".to_owned(),
            args: format!("branch: {branch}"),
            excerpt: format!("submitted branch {branch} at {}", &SUBMITTED_HEAD[..12]),
            ok: true,
            act: true,
            confidential: false,
        },
    )
    .expect("the node's receipt of the submission");
}

/// The dev node's runner takes the check and reports with the job's own token.
#[when(expr = "the check of {string} passes")]
fn check_passes(w: &mut Office, branch: String) {
    check_reports(w, &branch, true, "syntax ok, ansible-lint ok, yamllint ok");
}

/// As above, with the line the check job gives: both counts, and the verdict against develop.
#[when(expr = "the check passes: {string}")]
fn check_passes_saying(w: &mut Office, summary: String) {
    let check =
        w.hq.next_check()
            .expect("the store answers")
            .expect("a check waits");
    w.hq.check_done(
        check.id,
        &check.token,
        true,
        &summary,
        "the commands' last words",
    )
    .expect("reported once");
}

#[then(expr = "the gate of {string} has passed")]
fn the_gate_has_passed(w: &mut Office, title: String) {
    let task = w.task(&title);
    assert_eq!(
        w.hq.store().gate(task).expect("the store answers"),
        Some(true)
    );
}

#[when(expr = "the check of {string} fails: {string}")]
fn check_fails(w: &mut Office, branch: String, summary: String) {
    check_reports(w, &branch, false, &summary);
}

fn check_reports(w: &mut Office, branch: &str, ok: bool, summary: &str) {
    let check =
        w.hq.next_check()
            .expect("the store answers")
            .expect("a check waits");
    let (_, named) =
        w.hq.check_by_token(&check.token)
            .expect("the store answers")
            .expect("the job's token names it");
    assert_eq!(named.name, branch);
    w.hq.check_done(
        check.id,
        &check.token,
        ok,
        summary,
        "the commands' last words",
    )
    .expect("reported once");
}

#[then("the owner's desk holds no push")]
fn no_push(w: &mut Office) {
    assert!(w.waiting_on_desk("push").is_empty());
}

/// The push card offers the head that was submitted and reviewed, and says how its checks went.
#[then(expr = "the owner's desk holds a push of {string} at the submitted head, saying {string}")]
fn push_offered_at_the_head(w: &mut Office, branch: String, saying: String) {
    push_offered(w, branch);
    let item = w.push.expect("a push is offered");
    let text = w
        .waiting_on_desk("push")
        .into_iter()
        .find(|d| d.id == item)
        .expect("the push waits")
        .text;
    assert!(
        text.contains(&format!(
            "It is {} on top of {}",
            &SUBMITTED_HEAD[..12],
            &SUBMITTED_BASE[..12]
        )),
        "{text}"
    );
    assert!(text.contains(&saying), "{text}");
}

#[then(expr = "the owner's desk holds a push of {string}")]
fn push_offered(w: &mut Office, branch: String) {
    let items = w.waiting_on_desk("push");
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(
        items[0]
            .text
            .starts_with(&format!("Push {branch} to your fork (")),
        "{}",
        items[0].text
    );
    w.push = Some(items[0].id);
}

/// A member checks a kept branch out, as the node does for the turn (ADR 0115, amended): HQ's
/// rule says whether the branch is the turn's to have (its work, or its member's), and the
/// node writes the receipt of the checkout, naming the branch and its head. Returns the
/// receipt, which is the turn's own reading of the branch; `None` when HQ refuses, with the
/// refusal noted as the node would tell the turn.
fn checks_out(w: &mut Office, name: &str, branch: &str) -> Option<i64> {
    let context = w.turn_of(name);
    let asked = w.hq.checkout_branch(&context.token, branch);
    let kept = w.note(asked)?;
    let receipt =
        w.hq.record_receipt(
            &context.token,
            &Called {
                tool: "repo_checkout".to_owned(),
                args: format!("branch: {branch}"),
                excerpt: format!(
                    "checked out the team's kept branch {branch} at {} (task #{}'s, on top of develop)",
                    &kept.head[..12],
                    kept.task.0
                ),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .expect("the checkout's receipt");
    w.checkout = Some((branch.to_owned(), kept.head, receipt));
    Some(receipt)
}

/// The newest branch the team submitted for a task.
fn branch_of(w: &Office, title: &str) -> endora_team::domain::Branch {
    let task = w.task(title);
    w.hq.store()
        .branches()
        .expect("the store answers")
        .into_iter()
        .filter(|b| b.task == task)
        .max_by_key(|b| b.id)
        .unwrap_or_else(|| panic!("no branch was submitted for {title:?}"))
}

/// The tester re-checks: she checks the branch out (a reading of her own, in her turn) and
/// cites that receipt with the check's.
#[when(expr = "{word} passes {string} on the check and her own reading")]
fn tester_passes(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let branch = branch_of(w, &title).name;
    let own = checks_out(w, &name, &branch).expect("the branch is hers to read");
    let check = receipt_of(w, task, "check");
    w.use_tool(
        &name,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "notes": "the diff is what it says",
                 "evidence": [[format!("r{check}"), format!("r{own}")]] }),
    )
    .expect("the tester passes it");
    turn_ends(w, name);
}

/// A member checks the branch of a task out (HQ's rule, then the node's receipt of it).
#[when(expr = "{word} checks out {string}")]
fn checks_out_step(w: &mut Office, name: String, branch: String) {
    checks_out(w, &name, &branch);
}

#[then(expr = "the checkout is the submitted head of {string}")]
fn the_checkout_is_the_head(w: &mut Office, title: String) {
    let (branch, head, _) = w.checkout.clone().expect("a checkout was made");
    let submitted = branch_of(w, &title);
    assert_eq!((branch, head), (submitted.name, submitted.head));
}

/// The newest receipt of `tool` on a task, by whichever turn it was made in.
fn receipt_of(w: &Office, task: TaskId, tool: &str) -> i64 {
    w.hq.store()
        .runs()
        .expect("the store answers")
        .into_iter()
        .filter_map(|r| w.hq.store().call_receipts_of(r.id).ok())
        .flatten()
        .filter(|r| r.tool == tool && r.task == Some(task))
        .map(|r| r.id)
        .max()
        .unwrap_or_else(|| panic!("no {tool} receipt is on the task"))
}

/// How many criteria a task has (one list of evidence is cited for each; at least one).
fn criteria_of(w: &Office, task: TaskId) -> usize {
    w.hq.store()
        .task(task)
        .expect("the store answers")
        .expect("a task")
        .criteria
        .len()
        .max(1)
}

/// The member takes the task and hands it in for review, citing the receipts of the branch's
/// submission and of its check: the first two criteria rest on what was submitted and the last
/// on the check. Its turn ends, so the tester's can start.
#[when(expr = "{word} submits {string} for review citing the submit and the check receipts")]
fn submits_citing(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let (submit, check) = (
        receipt_of(w, task, "repo_submit"),
        receipt_of(w, task, "check"),
    );
    let criteria = criteria_of(w, task);
    let evidence: Vec<Vec<String>> = (0..criteria)
        .map(|i| {
            let r = if i + 1 == criteria { check } else { submit };
            vec![format!("r{r}")]
        })
        .collect();
    w.use_tool(&name, "claim", &json!({ "task": task.0 }))
        .expect("the member takes its task");
    w.use_tool(
        &name,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "the check passed",
                 "evidence": evidence }),
    )
    .expect("the work goes to review");
    turn_ends(w, name);
}

/// The tester passes the task, citing the check's receipt for every criterion and nothing of her
/// own: refused, since a pass rests on a reading she took herself.
#[when(expr = "{word} passes {string} citing the check receipt alone")]
fn passes_on_the_check_alone(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let check = receipt_of(w, task, "check");
    let evidence = vec![vec![format!("r{check}")]; criteria_of(w, task)];
    w.use_tool(
        &name,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "notes": "the check passed",
                 "evidence": evidence }),
    );
}

/// The tester passes the task on her checkout of the branch, which is a reading of her own,
/// and the check's receipt, for every criterion. Whether it is taken is for the next step.
#[when(expr = "{word} passes {string} citing her checkout and the check receipt")]
fn passes_on_her_checkout(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let (_, _, own) = w.checkout.clone().expect("she checked the branch out");
    let check = receipt_of(w, task, "check");
    let evidence = vec![vec![format!("r{check}"), format!("r{own}")]; criteria_of(w, task)];
    w.use_tool(
        &name,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "notes": "the diff is what it says",
                 "evidence": evidence }),
    );
}

#[when(expr = "{word} accepts {string}")]
fn lead_accepts(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    w.turn_of(&name);
    // The task itself is the evidence of each of its criteria: one list for every one.
    let evidence = vec![vec![format!("task {}", task.0)]; criteria_of(w, task)];
    w.use_tool(
        &name,
        "accept",
        &json!({ "task": task.0, "verdict": "accept", "notes": "good", "evidence": evidence }),
    )
    .expect("the lead accepts");
}

#[then(expr = "the task {string} is {word}")]
fn task_is(w: &mut Office, title: String, status: String) {
    let task = w.task(&title);
    let found =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .expect("a task");
    assert_eq!(found.status.name(), status);
}

#[when("the owner approves the push")]
fn approve_push(w: &mut Office) {
    let item = w.push.expect("a push is offered");
    w.hq.decide(item, "approve").expect("the owner decides");
}

#[then(expr = "Endora is asked to push {string}")]
fn node_asked_to_push(w: &mut Office, branch: String) {
    let item = w.push.expect("a push is offered");
    let asked =
        w.hq.push_asked(item)
            .expect("the store answers")
            .expect("the node is asked");
    assert_eq!(asked.name, branch);
}

#[when(expr = "the node says it pushed: {string}")]
fn node_pushed(w: &mut Office, text: String) {
    let item = w.push.expect("a push is offered");
    w.hq.push_outcome(item, true, &text).expect("told once");
}

#[when(expr = "{word} asks to hire a {word} on {string}")]
fn asks_to_hire(w: &mut Office, name: String, role: String, brain: String) {
    let plan = wing(w).task;
    w.turn_of(&name);
    let said = w.use_tool(
        &name,
        "request_hire",
        &json!({ "role": role, "brain": brain, "why": "two branches wait with nobody free",
                 "evidence": [format!("task {}", plan.0)] }),
    );
    let _ = said;
}

#[then(regex = r"^the owner's desk holds (\d+) hire requests?$")]
fn desk_hires(w: &mut Office, count: usize) {
    assert_eq!(w.waiting_on_desk("hire").len(), count);
}

#[when("the owner approves the hire")]
fn approve_hire(w: &mut Office) {
    let item = w.waiting_on_desk("hire").pop().expect("a request waits");
    w.hq.decide(item.id, "approve").expect("the owner decides");
    for m in w.hq.store().members().expect("the store answers") {
        w.members.entry(m.name.clone()).or_insert(m.id);
    }
}

#[then(regex = r"^the wing has (\d+) members$")]
fn wing_size(w: &mut Office, count: usize) {
    assert_eq!(w.hq.wing_members().expect("the store answers").len(), count);
}

#[given(expr = "the owner hires {word} as a {word} under {word}")]
#[when(expr = "the owner hires {word} as a {word} under {word}")]
fn owner_hires(w: &mut Office, name: String, role: String, under: String) {
    let role = Role::from_name(&role).unwrap_or_else(|| panic!("no role {role}"));
    let manager = w.member(&under);
    let hired = w.hq.hire_by_owner(&name, role, "local", Some(manager));
    if let Some(id) = w.note(hired) {
        w.members.insert(name, id);
    }
}

#[then(expr = "{word} reports to {word}")]
fn reports_to(w: &mut Office, name: String, manager: String) {
    let id = w.member(&name);
    let manager = w.member(&manager);
    let found =
        w.hq.store()
            .members()
            .expect("the store answers")
            .into_iter()
            .find(|m| m.id == id)
            .expect("hired");
    assert_eq!(found.reports_to, Some(manager));
}

#[then(expr = "it is refused for {string}")]
fn refused_for(w: &mut Office, why: String) {
    let said = w.refused.as_deref().expect("it was done");
    assert!(said.contains(&why), "refused for: {said}");
}

// --- Elastic staffing and notebooks (ADR 0118) ----------------------------------------------

/// Days go by, with a tick before (the team is looked over once a day, and a member is first
/// met then) and one after.
#[given(regex = r"^(\d+) days pass$")]
#[when(regex = r"^(\d+) days pass$")]
fn days_pass(w: &mut Office, days: i64) {
    w.tick();
    w.clock.0.fetch_add(days * DAY_MS, Ordering::SeqCst);
    w.tick();
}

#[then(expr = "the owner's desk offers to furlough {string}")]
fn offers_furlough(w: &mut Office, name: String) {
    let id = w.member(&name);
    assert!(
        w.waiting_on_desk("furlough")
            .iter()
            .any(|d| d.member == Some(id)),
        "no offer for {name}: {:?}",
        w.waiting_on_desk("furlough")
    );
}

#[when(expr = "the owner approves furloughing {string}")]
fn approve_furlough(w: &mut Office, name: String) {
    let id = w.member(&name);
    let item = w
        .waiting_on_desk("furlough")
        .into_iter()
        .find(|d| d.member == Some(id))
        .expect("the owner was offered it");
    w.hq.decide(item.id, "approve").expect("the owner decides");
}

fn found(w: &Office, name: &str) -> endora_team::domain::Member {
    let id = w.member(name);
    w.hq.store()
        .members()
        .expect("the store answers")
        .into_iter()
        .find(|m| m.id == id)
        .unwrap_or_else(|| panic!("{name} is no longer on the team"))
}

#[given(expr = "{word} is on furlough")]
fn put_on_furlough(w: &mut Office, name: String) {
    let id = w.member(&name);
    w.hq.furlough(id).expect("the owner furloughs");
}

#[when(expr = "the owner furloughs {word}")]
fn owner_furloughs(w: &mut Office, name: String) {
    let id = w.member(&name);
    let done = w.hq.furlough(id);
    w.note(done);
}

#[when(expr = "the owner recalls {word}")]
fn owner_recalls(w: &mut Office, name: String) {
    let id = w.member(&name);
    let done = w.hq.recall(id);
    w.note(done);
}

#[then(expr = "{word} is on furlough")]
fn is_on_furlough(w: &mut Office, name: String) {
    assert!(found(w, &name).furloughed(), "{name} is at work");
}

#[then(expr = "{word} is at work")]
fn is_at_work(w: &mut Office, name: String) {
    assert!(!found(w, &name).furloughed(), "{name} is on furlough");
}

/// Nobody is ever fired: the member is still there, by name, with its record.
#[then(expr = "{word} is still on the team")]
fn still_on_the_team(w: &mut Office, name: String) {
    assert_eq!(found(w, &name).name, name);
}

#[then(expr = "waking {word} is refused as furloughed")]
fn waking_refused(w: &mut Office, name: String) {
    let id = w.member(&name);
    let receipt =
        w.hq.wake(id, None, WakeReason::Mentioned)
            .expect("the wake is answered");
    assert_eq!(receipt, Receipt::Refused(Refused::Furloughed));
}

#[given(regex = r"^the wing's project has (\d+) open tasks$")]
fn wing_backlog(w: &mut Office, count: usize) {
    let (goal, project) = (wing(w).goal, wing(w).project);
    for n in 1..=count {
        w.hq.add_task(
            Author::Owner,
            NewTask {
                goal,
                project: Some(project),
                parent: None,
                title: format!("Branch {n}"),
                description: String::new(),
                assignee: None,
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .expect("the owner hands the wing work");
    }
}

#[when(regex = r"^(\w+) asks for staff for the wing's project, citing its (\d+) open tasks$")]
fn asks_for_staff(w: &mut Office, name: String, count: usize) {
    let project = wing(w).project;
    let cited: Vec<String> =
        w.hq.store()
            .tasks()
            .expect("the store answers")
            .into_iter()
            .filter(|t| t.project == Some(project) && t.title.starts_with("Branch "))
            .take(count)
            .map(|t| format!("task {}", t.id))
            .collect();
    w.use_tool(
        &name,
        "request_staff",
        &json!({ "project": project.0, "why": "the branches are piling up", "evidence": cited }),
    );
}

#[then(expr = "the owner's desk holds a request to recall {string}")]
fn desk_recall(w: &mut Office, name: String) {
    assert!(
        w.waiting_on_desk("recall")
            .iter()
            .any(|d| d.text.contains(&format!("{name} is on furlough"))),
        "{:?}",
        w.waiting_on_desk("recall")
    );
}

#[when("the owner approves the recall")]
fn approve_recall(w: &mut Office) {
    let item = w.waiting_on_desk("recall").pop().expect("a request waits");
    w.hq.decide(item.id, "approve").expect("the owner decides");
}

#[when(regex = r#"^(\w+) notes "(.+)" citing nothing$"#)]
fn notes_citing_nothing(w: &mut Office, name: String, text: String) {
    w.use_tool(&name, "note", &json!({ "text": text, "evidence": [] }));
}

#[when(regex = r#"^(\w+) notes "(.+)" citing the task "(.+)"$"#)]
fn notes_citing_task(w: &mut Office, name: String, text: String, title: String) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "note",
        &json!({ "text": text, "evidence": [format!("task {task}")] }),
    );
}

#[when(regex = r#"^(\w+) notes "(.+)" citing "(.+)"$"#)]
fn notes_citing(w: &mut Office, name: String, text: String, cited: String) {
    w.use_tool(&name, "note", &json!({ "text": text, "evidence": [cited] }));
}

#[given(regex = r#"^(\w+) noted "(.+)" resting on the wing's first task$"#)]
fn noted_before(w: &mut Office, name: String, text: String) {
    let (id, plan) = (w.member(&name), wing(w).task);
    let now = w.clock.now();
    w.hq.store()
        .add_note(id, &text, &[Ref::Task(plan)], false, None, now)
        .expect("the store answers");
}

#[then(regex = r"^(\w+)'s notebook holds (\d+) notes?$")]
fn notebook_holds(w: &mut Office, name: String, count: usize) {
    let id = w.member(&name);
    assert_eq!(w.hq.notebook(id).expect("the member exists").len(), count);
}

/// cucumber's command line, less what `cargo test` passes every test binary (`--test-threads`,
/// `--nocapture` and the like), which it would otherwise refuse. A bare word is a scenario
/// name to run, as it is for libtest: `cargo test -p endora-team --test scenarios -- "One task"`.
fn options() -> cucumber::cli::Opts<
    cucumber::parser::basic::Cli,
    cucumber::runner::basic::Cli,
    cucumber::writer::basic::Cli,
> {
    const WITH_A_VALUE: &[&str] = &["--test-threads", "--skip", "--format", "--color", "-Z"];
    let mut args = std::env::args();
    let mut kept = vec![args.next().unwrap_or_default()];
    while let Some(arg) = args.next() {
        if WITH_A_VALUE.contains(&arg.as_str()) {
            args.next();
            continue;
        }
        if arg.starts_with("--test-threads=") || (arg.starts_with('-') && !arg.starts_with("--")) {
            continue;
        }
        if arg.starts_with("--") {
            if arg.starts_with("--name") || arg.starts_with("--tags") {
                kept.push(arg);
            }
            continue;
        }
        kept.push("--name".to_owned());
        kept.push(arg);
    }
    cucumber::cli::Opts::try_parse_from(kept).expect("cucumber's own options")
}

// --- Roles as an enforced RACI (ADR 0120) ------------------------------------------------------

#[then(expr = "{word} has never been woken")]
fn never_woken(w: &mut Office, name: String) {
    let member = w.member(&name);
    let wakes: Vec<_> =
        w.hq.wakes()
            .expect("the store answers")
            .into_iter()
            .filter(|wake| wake.member == member)
            .collect();
    assert!(wakes.is_empty(), "{name} was woken: {wakes:?}");
}

#[when(expr = "{word} closes {string} as done")]
fn closes_bare(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "set_status",
        &json!({ "task": task.0, "status": "done", "reason": "it held" }),
    );
}

/// The CEO's direction, on the task as its evidence; her turn ends, so the lead's turn may
/// start on the task.
#[when(expr = "{word} directs {word} on {string}: {string}")]
fn directs(w: &mut Office, name: String, _lead: String, title: String, direction: String) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "direct",
        &json!({ "task": task.0, "direction": direction,
                 "evidence": [format!("task {}", task.0)] }),
    )
    .expect("the CEO directs the area's lead");
    turn_ends(w, name);
}

#[then(regex = r#"^(\w+)'s turn is told "(.+)"$"#)]
fn turn_is_told(w: &mut Office, name: String, text: String) {
    let context = w.turn_of(&name);
    assert!(
        context.told.contains(&text),
        "{name} was told:\n{}",
        context.told
    );
}

/// A reading of the lab in the member's turn, as the node records it.
fn reads_the_lab(w: &mut Office, name: &str, tool: &str, act: bool) -> i64 {
    let context = w.turn_of(name);
    w.hq.record_receipt(
        &context.token,
        &Called {
            tool: tool.to_owned(),
            args: if act {
                "container: deluge".to_owned()
            } else {
                String::new()
            },
            excerpt: "deluge: running".to_owned(),
            ok: true,
            act,
            confidential: false,
        },
    )
    .expect("the node records the call")
}

#[when(expr = "{word} restarts deluge and reads the lab")]
fn restarts_and_reads(w: &mut Office, name: String) {
    let task = w.turn_of(&name).task.expect("a turn on a task");
    w.use_tool(&name, "claim", &json!({ "task": task.0 }))
        .expect("the member takes its task");
    reads_the_lab(w, &name, "restart_container", true);
    w.clock.0.fetch_add(2 * 60_000, Ordering::SeqCst);
    w.reading = Some(reads_the_lab(w, &name, "homelab_status", false));
}

#[when(expr = "{word} closes {string} as done on her reading")]
fn closes_on_reading(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let r = format!("r{}", w.reading.expect("a reading was taken"));
    w.use_tool(
        &name,
        "set_status",
        &json!({ "task": task.0, "status": "done", "reason": "deluge is up",
                 "evidence": [r.clone(), r] }),
    );
}

#[when(expr = "{word} reads the lab again and closes {string} as done on her reading")]
fn reads_again_and_closes(w: &mut Office, name: String, title: String) {
    w.reading = Some(reads_the_lab(w, &name, "homelab_status", false));
    closes_on_reading(w, name, title);
}

#[then(expr = "it is refused, saying {string}")]
fn refused_containing(w: &mut Office, part: String) {
    let said = w.refused.clone().expect("it was refused");
    assert!(said.contains(&part), "{said}");
}

#[given(expr = "the home lab's work is {word}'s project")]
fn lab_is_a_project(w: &mut Office, _lead: String) {
    let id =
        w.hq.migrate_projects()
            .expect("the store answers")
            .expect("the home lab's project is made");
    w.projects.insert("Home lab upkeep".to_owned(), id);
}

/// The lead's weekly patterns routine, made once and run now; what it ran before is over.
#[when(expr = "{word}'s weekly patterns turn comes")]
fn patterns_turn(w: &mut Office, name: String) {
    let lead = w.member(&name);
    w.hq.seed_area_routines()
        .expect("the leads' routines are made");
    let patterns =
        w.hq.store()
            .routines()
            .expect("the store answers")
            .into_iter()
            .find(|r| r.assignee == lead && r.title == "Patterns")
            .expect("the lead has a patterns routine");
    w.tick();
    for started in w.hq.runner().started() {
        w.hq.runner().finish(started.run, true);
    }
    w.tick();
    w.hq.fire_routine(patterns.id).expect("the routine runs");
}

#[when(expr = "{word} proposes a project on the alarms that came back")]
fn proposes_on_alarms(w: &mut Office, name: String) {
    let alarms: Vec<i64> = w
        .hq
        .store()
        .events_after(0)
        .expect("the store answers")
        .into_iter()
        .filter(|e| {
            (e.kind == "routine.fired" && e.data["firing"] == "alarm") || e.kind == "routine.posted"
        })
        .map(|e| e.id)
        .collect();
    w.use_tool(
        &name,
        "propose_project",
        &json!({ "title": "Fix what keeps exiting",
                 "reason": "the same containers exited twice in two hours", "events": alarms }),
    )
    .expect("a lead proposes");
    let id =
        w.hq.store()
            .projects()
            .expect("the store answers")
            .into_iter()
            .find(|p| p.title == "Fix what keeps exiting")
            .expect("the proposal is a project")
            .id;
    w.projects.insert("Fix what keeps exiting".to_owned(), id);
}

/// The CEO's daily review, as HQ makes it, run now.
#[when(expr = "{word}'s daily review comes")]
fn daily_review(w: &mut Office, name: String) {
    let ceo = w.member(&name);
    let routine =
        w.hq.add_routine(
            Author::Endora,
            NewRoutine {
                title: "Daily review".to_owned(),
                description: "Review the house.".to_owned(),
                goal: w.goal(),
                project: None,
                assignee: ceo,
                every_ms: DAY_MS,
                on_lab_trouble: false,
            },
        )
        .expect("HQ makes the review");
    w.hq.fire_routine(routine).expect("the review runs");
}

/// Time passes on a task nobody comes back to, as the tick sees it: what is queued starts and
/// ends, ten minutes go by, and again, until the ladder climbs a rung (at most four rounds).
#[when(expr = "{string} sits with nobody coming back to it")]
fn sits(w: &mut Office, title: String) {
    let task = w.task(&title);
    let climbed = |w: &Office| {
        w.hq.store()
            .events_after(0)
            .expect("the store answers")
            .iter()
            .filter(|e| {
                matches!(e.kind.as_str(), "stuck.climbed" | "huddle.called")
                    && e.data["task"] == task.0
            })
            .count()
    };
    let before = climbed(w);
    for _ in 0..4 {
        w.tick();
        w.tick();
        for started in w.hq.runner().started() {
            w.hq.runner().finish(started.run, true);
        }
        w.tick();
        w.clock.0.fetch_add(
            endora_team::application::engine::STRANDED_AFTER_MS + 1,
            Ordering::SeqCst,
        );
        w.tick();
        if climbed(w) > before {
            return;
        }
    }
    panic!("{title} never climbed the ladder");
}

#[then(expr = "{word} is asked to decide {string}")]
fn asked_to_decide(w: &mut Office, name: String, title: String) {
    let (member, task) = (w.member(&name), w.task(&title));
    let wake = w.last_wake();
    assert_eq!(
        (wake.member, wake.task, wake.reason),
        (member, Some(task), WakeReason::Decide),
        "{wake:?}"
    );
}

#[then(expr = "a huddle is called on {string}")]
fn huddle_called(w: &mut Office, title: String) {
    let task = w.task(&title);
    assert!(
        w.hq.store()
            .events_after(0)
            .expect("the store answers")
            .iter()
            .any(|e| e.kind == "huddle.called" && e.data["task"] == task.0),
        "no huddle on {title}"
    );
}

/// A member's one short turn in a huddle: its view, on the task as evidence; then it ends.
#[when(expr = "{word} gives the view {string} on {string}")]
fn gives_view(w: &mut Office, name: String, view: String, title: String) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "huddle_view",
        &json!({ "task": task.0, "view": view, "evidence": [format!("task {}", task.0)] }),
    )
    .expect("the member is heard");
    turn_ends(w, name);
}

#[when(regex = r#"^(\w+) decides "(.+)": (\w+), "(.+)"$"#)]
fn decides_stuck(w: &mut Office, name: String, title: String, decision: String, detail: String) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "decide_stuck",
        &json!({ "task": task.0, "decision": decision, "detail": detail,
                 "evidence": [format!("task {}", task.0)] }),
    )
    .expect("the lead decides");
}

#[when(expr = "{word} checks back in an hour")]
fn checks_back(w: &mut Office, name: String) {
    w.use_tool(
        &name,
        "check_back",
        &json!({ "minutes": 60, "why": "no new alarm since the restart" }),
    )
    .expect("the member says when it comes back");
    turn_ends(w, name);
}

/// An hour and a minute on the clock, ticked: what was due comes.
#[when("an hour passes")]
fn an_hour_passes(w: &mut Office) {
    w.clock.0.fetch_add(HOUR_MS + 60_000, Ordering::SeqCst);
    w.tick();
}

fn main() {
    futures::executor::block_on(
        Office::cucumber()
            .with_cli(options())
            .fail_on_skipped()
            .run_and_exit("features"),
    );
}

// --- A member out for the day -----------------------------------------------------------------

#[then(regex = r"^the owner's desk holds one request about (\w+) being out for the day$")]
fn desk_out_for_the_day(w: &mut Office, name: String) {
    let member = w.member(&name);
    let items = w.waiting_on_desk("out_for_day");
    assert_eq!(items.len(), 1, "out for the day: {items:?}");
    assert_eq!(items[0].member, Some(member));
    assert!(
        items[0]
            .text
            .starts_with(&format!("{name} is out for today")),
        "{}",
        items[0].text
    );
}

#[when(regex = r"^the owner gives (\w+) \$(\d+(?:\.\d+)?) more today$")]
fn give_allowance(w: &mut Office, _name: String, dollars: String) {
    let item = w
        .waiting_on_desk("out_for_day")
        .pop()
        .expect("a request waits on the desk");
    w.hq.decide_saying(item.id, "approve", &dollars)
        .expect("the owner decides");
}

// --- The open office (ADR 0119) ------------------------------------------------------------------

impl Office {
    /// Makes sure `member` has a turn about no task running, as a mention would.
    fn office_turn(&mut self, member: &str) {
        let id = self.member(member);
        let live = self
            .hq
            .store()
            .runs()
            .expect("the store answers")
            .iter()
            .any(|r| r.member == id && r.status.live());
        if !live {
            self.hq
                .wake(id, None, WakeReason::Mentioned)
                .expect("the member is woken");
        }
    }

    fn office_items(&self) -> Vec<endora_team::domain::office::Item> {
        self.hq.store().office_items().expect("the store answers")
    }

    /// The newest proposal for the office.
    fn newest_proposal(&self) -> endora_team::domain::office::Proposal {
        self.hq
            .store()
            .office_proposals()
            .expect("the store answers")
            .pop()
            .expect("somebody proposed something")
    }

    /// `member`'s downtime turns, as the record has them.
    fn downtime_turns(&self, member: &str) -> Vec<endora_team::application::ports::Run> {
        let id = self.member(member);
        let wakes = self.hq.wakes().expect("the store answers");
        self.hq
            .store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .filter(|r| r.member == id)
            .filter(|r| {
                wakes
                    .iter()
                    .any(|w| w.id == r.wake && w.reason == WakeReason::Downtime)
            })
            .collect()
    }

    fn proposes(&mut self, name: &str, kind: &str, x: i64, y: i64, why: &str) {
        self.office_turn(name);
        self.use_tool(
            name,
            "office",
            &json!({ "action": "propose", "why": why,
                     "changes": [{ "op": "place", "kind": kind, "x": x, "y": y }] }),
        );
    }

    fn votes(&mut self, name: &str, vote: &str) {
        let proposal = self.newest_proposal().id;
        self.office_turn(name);
        self.use_tool(
            name,
            "office",
            &json!({ "action": "react", "proposal": proposal, "vote": vote }),
        );
    }
}

#[given("the office has its default layout")]
fn office_seeded(w: &mut Office) {
    assert!(w.hq.seed_office().expect("HQ seeds the office"));
}

#[when(regex = r#"^(\w+) proposes to add a "(.+)" at (\d+), (\d+) because "(.*)"$"#)]
fn proposes_because(w: &mut Office, name: String, kind: String, x: i64, y: i64, why: String) {
    w.proposes(&name, &kind, x, y, &why);
}

#[when(regex = r#"^(\w+) proposes to add a "(.+)" at (\d+), (\d+)$"#)]
fn proposes_plainly(w: &mut Office, name: String, kind: String, x: i64, y: i64) {
    w.proposes(&name, &kind, x, y, "");
}

#[when(regex = r"^(\w+) votes (yes|no) on the proposal$")]
fn votes_on_it(w: &mut Office, name: String, vote: String) {
    w.votes(&name, &vote);
}

#[when("the owner undoes the last change")]
fn owner_undoes(w: &mut Office) {
    let undone = w.hq.undo_office();
    w.note(undone);
}

#[when("the owner vetoes the proposal")]
fn owner_vetoes(w: &mut Office) {
    let id = w.newest_proposal().id;
    let vetoed = w.hq.veto_office(id);
    w.note(vetoed);
}

#[when("the owner resets the office")]
fn owner_resets(w: &mut Office) {
    let reset = w.hq.reset_office();
    w.note(reset);
}

#[given(regex = r"^(\d+) hours pass$")]
#[when(regex = r"^(\d+) hours pass$")]
fn hours_pass(w: &mut Office, hours: i64) {
    w.clock.0.fetch_add(hours * HOUR_MS, Ordering::SeqCst);
    w.tick();
}

#[then(expr = "the activity feed says {string}")]
fn feed_says(w: &mut Office, said: String) {
    let feed: Vec<String> =
        w.hq.store()
            .events_after(0)
            .expect("the store answers")
            .into_iter()
            .filter_map(|e| e.data["text"].as_str().map(str::to_owned))
            .collect();
    assert!(
        feed.iter().any(|t| t.contains(&said)),
        "the feed: {feed:#?}"
    );
}

#[then(expr = "the activity feed does not say {string}")]
fn feed_does_not_say(w: &mut Office, said: String) {
    let feed: Vec<String> =
        w.hq.store()
            .events_after(0)
            .expect("the store answers")
            .into_iter()
            .filter_map(|e| e.data["text"].as_str().map(str::to_owned))
            .collect();
    assert!(
        !feed.iter().any(|t| t.contains(&said)),
        "the feed: {feed:#?}"
    );
}

#[then(regex = r#"^the office has no "(.+)"$"#)]
fn office_has_no(w: &mut Office, kind: String) {
    let kind = endora_team::domain::office::Kind::from_name(&kind).expect("a kind in the catalog");
    assert!(w.office_items().iter().all(|i| i.kind != kind));
}

#[then(regex = r#"^the office has a "(.+)" at (\d+), (\d+)$"#)]
fn office_has_at(w: &mut Office, kind: String, x: i64, y: i64) {
    let kind = endora_team::domain::office::Kind::from_name(&kind).expect("a kind in the catalog");
    assert!(
        w.office_items()
            .iter()
            .any(|i| i.kind == kind && (i.x, i.y) == (x, y)),
        "the office: {:?}",
        w.office_items()
    );
}

#[then("the office has the default layout")]
fn office_is_default(w: &mut Office) {
    let items = w.office_items();
    assert_eq!(
        items.len(),
        endora_team::domain::office::default_layout().len()
    );
    assert!(items.iter().all(|i| i.placed_by == "endora"));
}

#[then("the proposal is not a task on the board or a card on the owner's desk")]
fn proposal_is_no_work(w: &mut Office) {
    assert!(w.hq.store().tasks().expect("the store answers").is_empty());
    assert!(w.hq.store().desk().expect("the store answers").is_empty());
    let snap = w.hq.snapshot().expect("the snapshot is built");
    assert_eq!(snap["tasks"].as_array().expect("tasks").len(), 0);
    assert_eq!(snap["desk"].as_array().expect("desk").len(), 0);
}

#[given("downtime is on")]
fn downtime_on(w: &mut Office) {
    w.hq.set_downtime(true);
}

#[when("a tick passes")]
fn a_tick_passes(w: &mut Office) {
    w.tick();
}

#[then(regex = r"^(\w+) has a downtime turn that costs nothing$")]
fn has_a_free_downtime_turn(w: &mut Office, name: String) {
    let turns = w.downtime_turns(&name);
    assert_eq!(turns.len(), 1, "{name}'s downtime turns: {turns:?}");
    assert_eq!(turns[0].reserved, 0);
    assert!(!turns[0].asked_by_owner);
}

#[then(regex = r"^(\w+) has no downtime turn$")]
fn has_no_downtime_turn(w: &mut Office, name: String) {
    assert!(w.downtime_turns(&name).is_empty());
}

#[when(regex = r"^(\w+) tries to create a task in her downtime turn$")]
fn tries_to_make_work(w: &mut Office, name: String) {
    w.use_tool(
        &name,
        "create_task",
        &json!({ "title": "A job", "description": "Anything", "assignee": "Bo" }),
    );
}

// --- Games in the office (ADR 0121) ---------------------------------------------------------------

#[given("the games are on")]
fn games_on(w: &mut Office) {
    w.hq.set_games(true);
}

#[given(regex = r"^(\d+) minutes pass$")]
#[when(regex = r"^(\d+) minutes pass$")]
fn minutes_pass(w: &mut Office, minutes: i64) {
    w.clock.0.fetch_add(minutes * 60 * 1000, Ordering::SeqCst);
    w.tick();
}

impl Office {
    fn matches_of(
        &self,
        kind: endora_team::domain::games::Kind,
    ) -> Vec<endora_team::domain::games::Record> {
        self.hq
            .store()
            .games()
            .expect("the store answers")
            .into_iter()
            .filter(|g| g.kind == kind)
            .collect()
    }
}

#[then("ping-pong has been played")]
fn ping_pong_played(w: &mut Office) {
    let played = w.matches_of(endora_team::domain::games::Kind::PingPong);
    assert!(played.len() >= 2, "ping-pong matches: {played:?}");
}

#[then("every ping-pong match was first to 11, two clear")]
fn ping_pong_by_the_rules(w: &mut Office) {
    for m in w.matches_of(endora_team::domain::games::Kind::PingPong) {
        let (won, lost) = m.scores();
        assert!(won >= 11 && won - lost >= 2, "{m:?}");
    }
}

#[then("the ping-pong leaderboard names the players")]
fn ping_pong_board(w: &mut Office) {
    let snap = w.hq.snapshot().expect("the snapshot is built");
    let board = snap["games"]["ping_pong"]["leaderboard"]
        .as_array()
        .expect("a leaderboard")
        .clone();
    assert!(!board.is_empty());
    for row in &board {
        let name = row["name"].as_str().expect("a name");
        assert!(["Ada", "Bo", "Cy"].contains(&name), "{row}");
    }
    let wins: i64 = board.iter().filter_map(|r| r["wins"].as_i64()).sum();
    assert_eq!(
        wins as usize,
        w.matches_of(endora_team::domain::games::Kind::PingPong)
            .len()
    );
}

#[then("nobody was woken and no turn was run")]
fn nobody_woken(w: &mut Office) {
    assert!(w.hq.wakes().expect("the store answers").is_empty());
    assert!(w.hq.store().runs().expect("the store answers").is_empty());
}

#[when(regex = r"^(\w+) challenges (\w+) to chess$")]
fn challenges_to_chess(w: &mut Office, name: String, other: String) {
    w.use_tool(
        &name,
        "chess",
        &json!({ "action": "challenge", "member": other }),
    );
}

#[when(regex = r#"^(\w+) plays "(.+)" at chess$"#)]
fn plays_at_chess(w: &mut Office, name: String, san: String) {
    w.use_tool(&name, "chess", &json!({ "action": "move", "san": san }));
}

#[given(regex = r#"^(\w+) and (\w+) are in a game of chess after "(.+)"$"#)]
fn in_a_game_of_chess(w: &mut Office, white: String, black: String, moves: String) {
    let (white, black) = (w.member(&white), w.member(&black));
    let id =
        w.hq.store()
            .add_chess_game(white, black, w.clock.now())
            .expect("the store answers");
    let mut game =
        w.hq.store()
            .chess_games()
            .expect("the store answers")
            .into_iter()
            .find(|g| g.id == id)
            .expect("the game");
    for (i, san) in moves.split_whitespace().enumerate() {
        let mover = if i % 2 == 0 { white } else { black };
        game.play(mover, san, w.clock.now()).expect("a legal move");
    }
    w.hq.store()
        .save_chess_game(&game)
        .expect("the store answers");
}

#[then(regex = r#"^the chess moves are "(.*)"$"#)]
fn chess_moves_are(w: &mut Office, moves: String) {
    let game =
        w.hq.store()
            .chess_games()
            .expect("the store answers")
            .pop()
            .expect("a game of chess");
    assert_eq!(game.moves.join(" "), moves);
}

#[then("the chess game is over")]
fn chess_game_over(w: &mut Office) {
    let game =
        w.hq.store()
            .chess_games()
            .expect("the store answers")
            .pop()
            .expect("a game of chess");
    assert!(!game.live(), "{game:?}");
}
