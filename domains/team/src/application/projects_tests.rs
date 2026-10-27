//! Projects, budgets, proposals, planning and whom members report to (ADR 0113), end to end
//! over HQ's real store and the simulated runner.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::application::run_ports::RunStore;

use serde_json::json;

use super::engine::{Author, Engine, EngineError, Settings};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::{call, tools};
use crate::domain::money::DOLLAR;
use crate::domain::routines::DAY_MS;
use crate::domain::{
    GoalId, MemberId, Pause, Priority, ProjectId, ProjectStatus, Receipt, Refusal, Refused, Role,
    TaskId, TaskStatus, WakeReason,
};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// 17 October 2026, at noon UTC.
const OCT_17: i64 = 1_792_238_400_000;

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    )
}

struct Team {
    lead: MemberId,
    dev: MemberId,
    goal: GoalId,
}

/// The CEO on Opus, a lead on Sonnet, a developer on the house's model, and the home lab's
/// goal.
fn team(hq: &Hq<'_>) -> Team {
    let ceo = hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
    hq.set_brain(ceo, "claude-opus-5-5").unwrap();
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    hq.set_brain(lead, "claude-sonnet-5-5").unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    Team { lead, dev, goal }
}

fn task_in(
    hq: &Hq<'_>,
    goal: GoalId,
    project: Option<ProjectId>,
    to: MemberId,
    title: &str,
) -> Result<TaskId, EngineError> {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
            project,
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
}

/// The newest turn of `member`'s, once the tick has started what is queued. A tester's work is
/// the Mac's (ADR 0131), so the node says it is free.
fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn of the member's")
}

fn use_tool(
    hq: &Hq<'_>,
    context: &RunContext,
    tool: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

fn project(hq: &Hq<'_>, id: ProjectId) -> crate::domain::Project {
    hq.store()
        .projects()
        .unwrap()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
}

fn waiting(hq: &Hq<'_>, kind: &str) -> Vec<crate::application::ports::DeskItem> {
    hq.store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == kind && d.status == "waiting")
        .collect()
}

fn last_wake(hq: &Hq<'_>) -> (MemberId, Option<TaskId>, WakeReason, Receipt) {
    let w = hq.store().wakes().unwrap().into_iter().next_back().unwrap();
    (w.member, w.task, w.reason, w.receipt)
}

/// A turn of the lead's on `task` that spends `amount`, then ends. It is a decision, the turn
/// that is paid for (ADR 0131).
fn spend(hq: &Hq<'_>, lead: MemberId, task: TaskId, amount: i64) {
    hq.wake(lead, Some(task), WakeReason::Decide).unwrap();
    let context = turn(hq, lead);
    assert_eq!(context.task, Some(task));
    hq.report_usage(context.run, amount).unwrap();
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
}

#[test]
fn a_tasks_project_serves_its_goal_and_a_sub_task_stays_in_its_parents() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let other = hq.set_goal("Ship the tracker", "", Author::Owner).unwrap();
    let upkeep = hq
        .add_project(t.goal, "Home lab upkeep", "", t.lead, None)
        .unwrap();
    assert_eq!(
        task_in(&hq, other, Some(upkeep), t.dev, "Write it"),
        Err(EngineError::Refused(Refusal::OtherGoal))
    );
    let fix = task_in(&hq, t.goal, Some(upkeep), t.lead, "Fix sonarr").unwrap();
    let context = turn(&hq, t.lead);
    use_tool(&hq, &context, "claim", &json!({ "task": fix.0 })).unwrap();
    use_tool(
        &hq,
        &context,
        "create_task",
        &json!({ "title": "Restart sonarr", "description": "and watch it", "assignee": "Linus",
                 "criteria": [{ "text": "sonarr is running", "live": true }] }),
    )
    .unwrap();
    let sub = hq.store().tasks().unwrap().pop().unwrap();
    assert_eq!(
        (sub.parent, sub.project),
        (Some(fix), Some(upkeep)),
        "in its parent's project"
    );
    // A project that takes no work takes no task.
    hq.change_project(upkeep, "drop").unwrap();
    assert_eq!(
        task_in(&hq, t.goal, Some(upkeep), t.dev, "One more"),
        Err(EngineError::Refused(Refusal::ProjectClosed))
    );
    assert_eq!(
        hq.store().task(sub.id).unwrap().unwrap().status,
        TaskStatus::Dropped,
        "its open work went with it"
    );
}

#[test]
fn the_projects_caps_together_never_pass_the_pools_month() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let a = hq
        .add_project(t.goal, "Upkeep", "", t.lead, Some(50 * DOLLAR))
        .unwrap();
    assert_eq!(
        hq.add_project(t.goal, "Media", "", t.lead, Some(21 * DOLLAR)),
        Err(EngineError::Refused(Refusal::CapsOverPool {
            room: 20 * DOLLAR
        }))
    );
    let b = hq
        .add_project(t.goal, "Media", "", t.lead, Some(20 * DOLLAR))
        .unwrap();
    assert_eq!(
        hq.set_project_cap(a, Some(51 * DOLLAR)),
        Err(EngineError::Refused(Refusal::CapsOverPool {
            room: 50 * DOLLAR
        })),
        "its own cap is not counted twice"
    );
    // A paused project keeps its cap; a dropped one gives it back.
    hq.change_project(b, "pause").unwrap();
    assert!(hq.set_project_cap(a, Some(51 * DOLLAR)).is_err());
    hq.change_project(b, "drop").unwrap();
    hq.set_project_cap(a, Some(70 * DOLLAR)).unwrap();
    assert_eq!(project(&hq, a).cap, Some(70 * DOLLAR));
    assert_eq!(
        hq.add_project(t.goal, "Led by a developer", "", t.dev, None),
        Err(EngineError::Refused(Refusal::NotAProjectLead))
    );
}

#[test]
fn a_cloud_turn_on_a_capped_project_reserves_no_more_than_is_left_of_it() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let p = hq
        .add_project(t.goal, "Upkeep", "", t.lead, Some(DOLLAR / 4))
        .unwrap();
    let task = task_in(&hq, t.goal, Some(p), t.lead, "Look at sonarr").unwrap();
    // The lead is asked to decide it: the turn of judgment, which is paid for (ADR 0131).
    hq.wake(t.lead, Some(task), WakeReason::Decide).unwrap();
    let context = turn(&hq, t.lead);
    assert_eq!(context.task, Some(task));
    let run = hq.store().run(context.run).unwrap().unwrap();
    assert_eq!(
        run.reserved,
        DOLLAR / 4,
        "the relay's `left` is this, less what it spent"
    );
}

#[test]
fn at_eighty_percent_the_owner_hears_once_a_month_and_nothing_is_blocked() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let p = hq
        .add_project(t.goal, "Upkeep", "", t.lead, Some(DOLLAR / 2))
        .unwrap();
    let task = task_in(&hq, t.goal, Some(p), t.lead, "Look at sonarr").unwrap();
    hq.wake(t.lead, Some(task), WakeReason::Decide).unwrap();
    hq.tick().unwrap();
    let first = hq.runner().started().pop().unwrap();
    hq.report_usage(first.run, 400_000).unwrap();
    let notes = waiting(&hq, "notice");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].project, Some(p));
    assert_eq!(
        notes[0].text,
        "Project #1 Upkeep has spent 80% of its $0.50 cap this month ($0.40)."
    );
    hq.runner().finish(first.run, true);
    hq.tick().unwrap();
    // Still at work, and told no second time this month.
    hq.wake(t.lead, Some(task), WakeReason::Decide).unwrap();
    let again = turn(&hq, t.lead);
    assert_ne!(again.run, first.run);
    hq.report_usage(again.run, 10_000).unwrap();
    assert_eq!(waiting(&hq, "notice").len(), 1, "once a month");
    assert_eq!(project(&hq, p).status, ProjectStatus::Active);
}

#[test]
fn at_its_cap_a_project_pauses_its_cloud_turns_and_its_local_members_go_on() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let p = hq
        .add_project(t.goal, "Upkeep", "", t.lead, Some(DOLLAR / 2))
        .unwrap();
    let task = task_in(&hq, t.goal, Some(p), t.lead, "Look at sonarr").unwrap();
    spend(&hq, t.lead, task, 400_000);
    spend(&hq, t.lead, task, 60_000);
    let paused = project(&hq, p);
    assert_eq!(
        (paused.status, paused.pause),
        (ProjectStatus::Paused, Some(Pause::Budget))
    );
    let items = waiting(&hq, "project_budget");
    assert_eq!(items.len(), 1);
    assert!(
        items[0]
            .text
            .starts_with("Project #1 Upkeep spent its $0.50 cap for the month ($0.46)"),
        "{}",
        items[0].text
    );
    // The lead's decision needs Claude: it is kept, and waits for the month. The developer's
    // work on the house's model goes on.
    hq.wake(t.lead, Some(task), WakeReason::Decide).unwrap();
    assert_eq!(last_wake(&hq).3, Receipt::Queued, "nothing is turned away");
    hq.tick().unwrap();
    let decision = hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .rfind(|r| r.member == t.lead)
        .unwrap();
    assert_eq!(decision.status, crate::domain::RunStatus::Queued);
    assert!(
        decision.end_reason.starts_with("waiting for budget"),
        "no Claude turn on it: {}",
        decision.end_reason
    );
    let local = task_in(&hq, t.goal, Some(p), t.dev, "Read the logs").unwrap();
    let dev = turn(&hq, t.dev);
    assert_eq!(dev.task, Some(local), "a local turn costs nothing");
    let feed: Vec<String> = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "project.budget")
        .map(|e| e.data["level"].as_str().unwrap_or("").to_owned())
        .collect();
    assert_eq!(feed, ["warned", "spent"]);
}

#[test]
fn a_budget_pause_lifts_when_the_owner_raises_the_cap_or_the_month_turns() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let p = hq
        .add_project(t.goal, "Upkeep", "", t.lead, Some(DOLLAR / 2))
        .unwrap();
    let task = task_in(&hq, t.goal, Some(p), t.lead, "Look at sonarr").unwrap();
    spend(&hq, t.lead, task, 400_000);
    spend(&hq, t.lead, task, 60_000);
    let item = waiting(&hq, "project_budget").remove(0);
    // A raise is more than the month's spend, and within the pool.
    assert_eq!(
        hq.decide_saying(item.id, "raise", "0.40"),
        Err(EngineError::Refused(Refusal::CapNotAboveSpend {
            spent: 460_000
        }))
    );
    hq.decide_saying(item.id, "raise", "$1").unwrap();
    let raised = project(&hq, p);
    assert_eq!(
        (raised.status, raised.cap),
        (ProjectStatus::Active, Some(DOLLAR))
    );
    assert!(waiting(&hq, "project_budget").is_empty());
    // Spent again, and the owner waits: the month turns, and it goes on by itself.
    spend(&hq, t.lead, task, 400_000);
    spend(&hq, t.lead, task, 150_000);
    assert_eq!(project(&hq, p).status, ProjectStatus::Paused);
    let item = waiting(&hq, "project_budget").remove(0);
    hq.decide(item.id, "wait").unwrap();
    hq.tick().unwrap();
    assert_eq!(
        project(&hq, p).status,
        ProjectStatus::Paused,
        "still October"
    );
    clock.0.fetch_add(15 * DAY_MS, Ordering::SeqCst);
    hq.tick().unwrap();
    let november = project(&hq, p);
    assert_eq!(
        (november.status, november.pause),
        (ProjectStatus::Active, None)
    );
}

#[test]
fn a_pause_the_owner_made_holds_every_turn_and_never_lifts_itself() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let p = hq.add_project(t.goal, "Upkeep", "", t.lead, None).unwrap();
    hq.change_project(p, "pause").unwrap();
    let task = task_in(&hq, t.goal, Some(p), t.dev, "Read the logs").unwrap();
    assert_eq!(
        last_wake(&hq),
        (
            t.dev,
            Some(task),
            WakeReason::Assigned,
            Receipt::Refused(Refused::ProjectPaused)
        ),
        "a local member waits too"
    );
    clock.0.fetch_add(20 * DAY_MS, Ordering::SeqCst);
    hq.tick().unwrap();
    assert_eq!(project(&hq, p).status, ProjectStatus::Paused, "a month on");
    assert!(hq.runner().started().is_empty());
    assert_eq!(hq.change_project(p, "resume"), Ok(ProjectStatus::Active));
    assert_eq!(
        hq.change_project(p, "resume"),
        Err(EngineError::Refused(Refusal::NotAProjectStep {
            from: "active",
            asked: "resume".to_owned()
        }))
    );
}

/// The names of the tools a turn is offered by HQ.
fn offered_by_hq(hq: &Hq<'_>, context: &RunContext) -> Vec<String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    super::protocol::tools_in(hq, &run, &member)
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect()
}

// Phase 4 of the Holodeck plan: a project may carry the tools its work is given. A read-only
// mission on the real lab reads and never restarts; the turn's own four (my_turn, claim, post,
// set_status) always come with it. HQ offers and runs nothing else, tells the node which of its
// tools the turn may use, and gives a shell only when the list names it.
#[test]
fn a_projects_tools_are_all_its_turns_are_given_besides_their_own_four() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let survey = hq
        .add_project(t.goal, "Read-only survey", "", t.lead, None)
        .unwrap();
    hq.set_project_tools(
        survey,
        Some(vec![
            "homelab_status".to_owned(),
            "container_logs".to_owned(),
        ]),
    )
    .unwrap();
    assert_eq!(
        hq.project_tools(survey).unwrap(),
        Some(vec![
            "homelab_status".to_owned(),
            "container_logs".to_owned()
        ])
    );
    let look = task_in(&hq, t.goal, Some(survey), t.lead, "Survey the lab").unwrap();
    let context = turn(&hq, t.lead);
    let (run, _) = hq.turn_of(&context.token).unwrap().unwrap();
    assert_eq!(run.task, Some(look));

    let mut offered = offered_by_hq(&hq, &context);
    offered.sort();
    assert_eq!(offered, ["claim", "my_turn", "post", "set_status"]);
    assert_eq!(
        hq.lab_tools_of(run.id).unwrap(),
        Some(vec![
            "homelab_status".to_owned(),
            "container_logs".to_owned()
        ]),
        "the node offers and runs these alone: no restart_container"
    );
    assert!(
        !hq.may_use_a_shell(run.id).unwrap(),
        "no shell unless named"
    );

    use_tool(&hq, &context, "claim", &json!({ "task": look.0 })).unwrap();
    let refused = use_tool(
        &hq,
        &context,
        "create_task",
        &json!({ "title": "Restart sonarr", "description": "x" }),
    )
    .unwrap_err();
    assert!(
        refused.contains("create_task") && refused.contains("Read-only survey"),
        "{refused}"
    );
    use_tool(
        &hq,
        &context,
        "post",
        &json!({ "task": look.0, "text": "Looked." }),
    )
    .expect("its own four stay");
}

// A project's list may keep its members' own HQ tools (`team`) and narrow only what the node
// offers: real work keeps its workflow (review, accept, ask_owner) while a slow local model is
// spared every contract it will not use (2026-10-25: about 17k of a 26k-token turn).
#[test]
fn a_list_naming_the_team_narrows_the_node_and_keeps_each_members_own_tools() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let cleanup = hq
        .add_project(t.goal, "Fork clean-up", "", t.lead, None)
        .unwrap();
    let list = vec![
        crate::domain::projects::TEAM.to_owned(),
        "shell".to_owned(),
        "repo_checkout".to_owned(),
        "repo_submit".to_owned(),
    ];
    hq.set_project_tools(cleanup, Some(list.clone())).unwrap();
    let work = task_in(&hq, t.goal, Some(cleanup), t.lead, "Lint the roles").unwrap();
    let context = turn(&hq, t.lead);
    let (run, _) = hq.turn_of(&context.token).unwrap().unwrap();
    assert_eq!(run.task, Some(work));

    let offered = offered_by_hq(&hq, &context);
    for own in ["create_task", "ask_owner", "post", "set_status"] {
        assert!(offered.iter().any(|t| t == own), "{own} kept: {offered:?}");
    }
    assert_eq!(
        hq.lab_tools_of(run.id).unwrap(),
        Some(list),
        "the node offers its listed contracts alone"
    );
    assert!(hq.may_use_a_shell(run.id).unwrap(), "the shell, as listed");
    use_tool(&hq, &context, "claim", &json!({ "task": work.0 })).unwrap();
    assert!(
        hq.project_refuses(run.id, "create_task").unwrap().is_none(),
        "a member's own tool is not refused"
    );
    // The node refuses what the list leaves out (its `only`): no restart_container.
    assert!(
        !hq.lab_tools_of(run.id)
            .unwrap()
            .unwrap()
            .iter()
            .any(|t| t == "restart_container")
    );
}

// A scenario (Phase 5) gets no lab, bridge or repo tool: an empty list leaves the turn its own
// four, nothing from the node, and no shell. With no list, a project's turns are as before.
#[test]
fn an_empty_list_gives_nothing_of_the_nodes_and_no_list_changes_nothing() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let scenario = hq
        .add_project(t.goal, "Silent Station", "", t.lead, None)
        .unwrap();
    hq.set_project_tools(scenario, Some(vec!["shell".to_owned()]))
        .unwrap();
    hq.set_project_tools(scenario, Some(Vec::new())).unwrap();
    task_in(&hq, t.goal, Some(scenario), t.lead, "Play").unwrap();
    let context = turn(&hq, t.lead);
    let (run, _) = hq.turn_of(&context.token).unwrap().unwrap();
    assert_eq!(hq.lab_tools_of(run.id).unwrap(), Some(Vec::new()));
    assert!(!hq.may_use_a_shell(run.id).unwrap());

    let upkeep = hq
        .add_project(t.goal, "Home lab upkeep", "", t.lead, None)
        .unwrap();
    hq.set_project_tools(upkeep, Some(vec!["shell".to_owned()]))
        .unwrap();
    hq.set_project_tools(upkeep, None).unwrap();
    assert_eq!(hq.project_tools(upkeep).unwrap(), None);
    task_in(&hq, t.goal, Some(upkeep), t.dev, "Fix sonarr").unwrap();
    let context = turn(&hq, t.dev);
    let (run, _) = hq.turn_of(&context.token).unwrap().unwrap();
    assert_eq!(hq.lab_tools_of(run.id).unwrap(), None);
    assert!(hq.may_use_a_shell(run.id).unwrap());
    let role_tools = tools();
    let expected: Vec<String> = role_tools
        .as_array()
        .unwrap()
        .iter()
        .filter(|tool| tool["name"] != "consult")
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        offered_by_hq(&hq, &context),
        expected,
        "role tools remain available; disabled peer help is omitted"
    );
    assert_eq!(
        hq.set_project_tools(ProjectId(99), None),
        Err(EngineError::Unknown("project"))
    );
}

/// Live, #676 and #677 asked for the ansible-nas fork's secrets with no project, so their
/// members had none of its tools, and the owner had no way to put them in it.
#[test]
fn the_owner_moves_a_task_into_an_active_project_and_its_member_takes_it_up_there() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let fork = hq
        .add_project(t.goal, "ansible-nas clean-up", "", t.lead, None)
        .unwrap();
    let task = task_in(&hq, t.goal, None, t.dev, "List the secret defaults").unwrap();
    hq.store()
        .set_task_status(task, TaskStatus::Blocked)
        .unwrap();
    hq.move_task_to_project(task, fork).unwrap();
    let moved = hq.store().task(task).unwrap().unwrap();
    assert_eq!(moved.project, Some(fork));
    assert_eq!(moved.status, TaskStatus::Todo, "blocked work starts again");
    assert!(
        hq.store()
            .thread(task)
            .unwrap()
            .iter()
            .any(|p| p.text.contains("ansible-nas clean-up")),
        "the thread says where it went"
    );
    assert_eq!(
        (last_wake(&hq).0, last_wake(&hq).1),
        (t.dev, Some(task)),
        "its member takes it up there"
    );
    // Not into a project that is not active, nor finished work.
    let paused = hq
        .add_project(t.goal, "Maybe later", "", t.lead, None)
        .unwrap();
    hq.change_project(paused, "pause").unwrap();
    assert!(hq.move_task_to_project(task, paused).is_err());
    hq.store().set_task_status(task, TaskStatus::Done).unwrap();
    assert!(hq.move_task_to_project(task, fork).is_err());
}
