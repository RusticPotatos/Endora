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
    /// The clock the engine reads, for steps that let days pass.
    clock: Hands,
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
        self.hq.tick().expect("the tick runs");
        let id = self.member(member);
        let context = self
            .hq
            .runner()
            .started()
            .into_iter()
            .rfind(|c| c.member == id)
            .unwrap_or_else(|| panic!("no turn of {member}'s has started"));
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
        w.hq.wake(member, Some(task), WakeReason::Assigned)
            .expect("a wake has a receipt");
        let context = w.turn_of(&name);
        assert_eq!(context.task, Some(task), "{name}'s turn is on another task");
        w.hq.report_usage(context.run, micros(amount))
            .expect("the ledger takes it");
        w.hq.runner().finish(context.run, true);
        w.hq.tick().expect("the tick runs");
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
    w.hq.tick().expect("the tick runs");
}

#[given(regex = r"^(\w+) has spent \$(\d+(?:\.\d+)?) of (?:her|his|its) share today$")]
fn spent_of_share(w: &mut Office, name: String, dollars: f64) {
    let title = format!("{name}'s earlier work");
    endora_hands(w, &name, &title);
    let member = w.member(&name);
    let mut left = micros(dollars);
    while left > 0 {
        let amount = left.min(endora_team::domain::money::MOST_A_TURN);
        if left != micros(dollars) {
            let task = w.task(&title);
            w.hq.wake(member, Some(task), WakeReason::Mentioned)
                .expect("a wake has a receipt");
        }
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
    w.hq.tick().expect("the tick runs");
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

fn wing(w: &Office) -> &WingSeeded {
    w.wing.as_ref().expect("the scenario has the wing")
}

#[given(expr = "{word} hands {word} the task {string} in the wing")]
fn wing_hands(w: &mut Office, by: String, to: String, title: String) {
    let by = w.member(&by);
    let to = w.member(&to);
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
        &[Criterion::new("The branch passes its checks", false)],
    )
    .expect("the lead hands work on");
}

/// The node took the member's branch (`repo_submit`) and tells HQ, with the turn's token.
#[when(expr = "{word} submits the branch {string}")]
fn submits_branch(w: &mut Office, name: String, branch: String) {
    let context = w.turn_of(&name);
    w.hq.record_branch(
        &context.token,
        &Submission {
            branch,
            head: "a1b2c3d4e5f60718293a4b5c".to_owned(),
            base: "0123456789abcdef01234567".to_owned(),
            files: 1,
            plus: 4,
            minus: 1,
            stat: " roles/deluge/tasks/main.yml | 5 ++--".to_owned(),
            diff: "-old\n+new".to_owned(),
            can_push: true,
        },
    )
    .expect("the node's record is taken");
}

/// The dev node's runner takes the check and reports with the job's own token.
#[when(expr = "the check of {string} passes")]
fn check_passes(w: &mut Office, branch: String) {
    check_reports(w, &branch, true, "syntax ok, ansible-lint ok, yamllint ok");
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

/// The tester re-checks: she reads the document herself (a receipt of her own) and cites the
/// check's receipt.
#[when(expr = "{word} passes {string} on the check and her own reading")]
fn tester_passes(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let context = w.turn_of(&name);
    let own =
        w.hq.record_receipt(
            &context.token,
            &Called {
                tool: "read_document".to_owned(),
                args: "d1".to_owned(),
                excerpt: "the diff".to_owned(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .expect("her own reading");
    let check =
        w.hq.store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .filter_map(|r| w.hq.store().call_receipts_of(r.id).ok())
            .flatten()
            .find(|r| r.tool == "check" && r.task == Some(task))
            .expect("the check's receipt");
    w.use_tool(
        &name,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "notes": "the diff is what it says",
                 "evidence": [[format!("r{}", check.id), format!("r{own}")]] }),
    )
    .expect("the tester passes it");
    turn_ends(w, name);
}

#[when(expr = "{word} accepts {string}")]
fn lead_accepts(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    w.turn_of(&name);
    w.use_tool(
        &name,
        "accept",
        &json!({ "task": task.0, "verdict": "accept", "notes": "good",
                 "evidence": [[format!("task {}", task.0)]] }),
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
    w.hq.tick().expect("the tick runs");
    w.clock.0.fetch_add(days * DAY_MS, Ordering::SeqCst);
    w.hq.tick().expect("the tick runs");
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

fn main() {
    futures::executor::block_on(
        Office::cucumber()
            .with_cli(options())
            .fail_on_skipped()
            .run_and_exit("features"),
    );
}
