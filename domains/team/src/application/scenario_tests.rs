//! A scenario (the Holodeck plan's phase 5), end to end over HQ's real store and the
//! simulated runner: started in a project that gives its work the scenario tool alone, played
//! through that tool, closed by the world and replayed from its log.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::json;

use super::engine::{Author, Engine, EngineError, Settings};
use super::guard_ports::GuardStore as _;
use super::ports::{Clock, RunContext, TeamStore};
use super::protocol::{call, tools_in};
use crate::domain::closure::{Closure, DoneWhy, DropWhy};
use crate::domain::scenario::{Action, Cause, Lighthouse, Part};
use crate::domain::{MemberId, ProjectId, Refusal, Role, TaskId, TaskStatus};
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

/// A lead, a developer on the house's model, and a Holodeck project that gives its work the
/// scenario tool alone.
fn holodeck(hq: &Hq<'_>) -> (MemberId, ProjectId) {
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let goal = hq.set_goal("Holodeck", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Lighthouse", "", lead, None).unwrap();
    hq.set_project_tools(project, Some(vec!["scenario".to_owned()]))
        .unwrap();
    (dev, project)
}

/// A seed whose lighthouse has `cause`.
fn seed_for(cause: Cause) -> u64 {
    (0..).find(|s| Lighthouse::new(*s).cause == cause).unwrap()
}

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

fn acted(hq: &Hq<'_>) -> Vec<serde_json::Value> {
    hq.store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "scenario.acted")
        .map(|e| e.data)
        .collect()
}

// G3 of the roadmap: the world changes only through its tool, in code, and a model's claim
// changes nothing. The turn is offered its own four and the scenario tool, nothing of the
// node's and no shell; a refused action is kept and changes nothing; the lamp lit, the world
// closes the task; and the log replays to the same lighthouse.
#[test]
fn a_scenario_is_played_through_its_tool_alone_and_the_world_closes_it() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let (dev, project) = holodeck(&hq);
    let seed = seed_for(Cause::Bulb);
    let task = hq.start_scenario(project, dev, seed).unwrap();
    let found = hq.store().task(task).unwrap().unwrap();
    assert_eq!((found.project, found.assignee), (Some(project), Some(dev)));
    assert!(
        found.description.contains("lamp is dark"),
        "{}",
        found.description
    );

    let context = turn(&hq, dev);
    let (run, member) = hq.turn_of(&context.token).unwrap().unwrap();
    assert_eq!(run.task, Some(task));
    let mut offered: Vec<String> = tools_in(&hq, &run, &member)
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    offered.sort();
    assert_eq!(
        offered,
        ["claim", "my_turn", "post", "scenario", "set_status"]
    );
    assert!(!hq.may_use_a_shell(run.id).unwrap());

    use_tool(&hq, &context, "claim", &json!({ "task": task.0 })).unwrap();
    let claimed = use_tool(
        &hq,
        &context,
        "set_status",
        &json!({ "task": task.0, "status": "done", "reason": "I lit it" }),
    )
    .unwrap_err();
    assert!(claimed.contains("the world"), "{claimed}");
    assert!(
        use_tool(&hq, &context, "scenario", &json!({ "action": "look" }))
            .unwrap()
            .contains("The lamp is dark")
    );
    let refused = use_tool(
        &hq,
        &context,
        "scenario",
        &json!({ "action": "test_circuit" }),
    )
    .unwrap_err();
    assert!(refused.contains("inspect the lamp first"), "{refused}");
    assert!(
        use_tool(&hq, &context, "scenario", &json!({ "action": "inspect" }))
            .unwrap()
            .contains("filament is broken")
    );
    let lit = use_tool(
        &hq,
        &context,
        "scenario",
        &json!({ "action": "replace_part", "part": "bulb" }),
    )
    .unwrap();
    assert!(
        lit.contains("The lamp lights") && lit.contains("Solved"),
        "{lit}"
    );

    let closed = hq.store().task(task).unwrap().unwrap();
    assert_eq!(closed.status, TaskStatus::Done);
    let why = hq.store().closure(task).unwrap().unwrap();
    assert_eq!(
        (why.closure, why.by.as_str()),
        (Closure::Done(DoneWhy::MetCriteria), "endora")
    );
    assert!(
        hq.store()
            .thread(task)
            .unwrap()
            .iter()
            .any(|p| p.author == "endora" && p.text.contains("solved")),
        "the world says so in the thread"
    );
    assert!(
        use_tool(&hq, &context, "scenario", &json!({ "action": "inspect" }))
            .unwrap_err()
            .contains("over")
    );

    // Every action is in the log, the refused ones too; only the accepted change the world.
    let log = hq.store().scenario_actions(task).unwrap();
    let kept: Vec<(&str, bool)> = log.iter().map(|a| (a.action.as_str(), a.ok)).collect();
    assert_eq!(
        kept,
        [
            ("test_circuit", false),
            ("inspect", true),
            ("replace_part bulb", true),
            ("inspect", false),
        ]
    );
    assert!(log.iter().all(|a| a.run == run.id && a.member == dev));
    let events = acted(&hq);
    assert_eq!(events.len(), 4);
    assert_eq!(events[2]["end"], "solved");
    let replayed = hq.replay_scenario(task).unwrap();
    assert_eq!((replayed.lit, replayed.minutes), (true, 20));
    assert_eq!(
        replayed,
        Lighthouse::replay(seed, &[Action::Inspect, Action::Replace(Part::Bulb)]).unwrap()
    );

    // The owner's view: the log step by step, who acted, what was refused, and how it ended;
    // the cause is told once it is over.
    let shown = hq.scenario_shown(task).unwrap();
    assert_eq!(
        (
            &shown["seed"],
            &shown["end"],
            &shown["minutes"],
            &shown["cause"]
        ),
        (&json!(seed), &json!("solved"), &json!(20), &json!("bulb"))
    );
    let steps = shown["log"].as_array().unwrap();
    assert_eq!(steps.len(), 4);
    assert_eq!(
        (&steps[0]["action"], &steps[0]["ok"], &steps[0]["by"]),
        (&json!("test_circuit"), &json!(false), &json!("Linus"))
    );
    assert!(
        steps[0]["said"]
            .as_str()
            .unwrap()
            .contains("inspect the lamp first")
    );
}

#[test]
fn time_running_out_drops_it_and_a_reset_is_a_new_task_with_the_same_seed() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let (dev, project) = holodeck(&hq);
    let seed = seed_for(Cause::Fuse);
    let task = hq.start_scenario(project, dev, seed).unwrap();
    let context = turn(&hq, dev);
    use_tool(&hq, &context, "claim", &json!({ "task": task.0 })).unwrap();
    let mut said = String::new();
    for _ in 0..12 {
        said = use_tool(&hq, &context, "scenario", &json!({ "action": "inspect" })).unwrap();
    }
    assert!(said.contains("failed"), "{said}");
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Dropped
    );
    assert_eq!(
        hq.store().closure(task).unwrap().unwrap().closure,
        Closure::Dropped(DropWhy::BlockedForever)
    );

    let again = hq.start_scenario(project, dev, seed).unwrap();
    assert_ne!(again, task);
    assert_eq!(hq.replay_scenario(again).unwrap(), Lighthouse::new(seed));
    let shown = hq.scenario_shown(again).unwrap();
    assert_eq!(
        (&shown["end"], &shown["cause"], &shown["log"]),
        (&json!(null), &json!(null), &json!([])),
        "a fresh world hides its cause"
    );
    assert_eq!(hq.scenario_of(again).unwrap(), Some(seed));
    assert_eq!(hq.scenario_of(TaskId(999)).unwrap(), None);
}

// A scenario's work is given the scenario tool and nothing else of anyone's: a project with
// no list, or one that gives more, cannot hold one. A turn on other work is never offered it.
#[test]
fn a_scenario_starts_only_in_a_project_that_gives_it_the_tool_alone() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let (dev, project) = holodeck(&hq);
    for list in [
        None,
        Some(vec!["scenario".to_owned(), "homelab_status".to_owned()]),
        Some(Vec::new()),
    ] {
        hq.set_project_tools(project, list.clone()).unwrap();
        let refused = hq.start_scenario(project, dev, 1).unwrap_err();
        assert!(
            matches!(&refused, EngineError::Refused(Refusal::Scenario(why)) if why.contains("scenario")),
            "{list:?}: {refused}"
        );
    }
    assert_eq!(
        hq.start_scenario(ProjectId(99), dev, 1),
        Err(EngineError::Unknown("project"))
    );

    let goal = hq.set_goal("Keep the house", "", Author::Owner).unwrap();
    let other = hq
        .add_task(
            Author::Owner,
            super::ports::NewTask {
                goal,
                project: None,
                parent: None,
                title: "Water the plants".to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: crate::domain::Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    let context = turn(&hq, dev);
    let (run, member) = hq.turn_of(&context.token).unwrap().unwrap();
    assert_eq!(run.task, Some(other));
    assert!(
        !tools_in(&hq, &run, &member)
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "scenario")
    );
    assert!(
        use_tool(&hq, &context, "scenario", &json!({ "action": "look" }))
            .unwrap_err()
            .contains("no scenario")
    );
}
