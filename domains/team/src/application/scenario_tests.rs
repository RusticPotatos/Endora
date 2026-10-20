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
use crate::domain::station::{Action as StationAction, Anomaly, Station};
use crate::domain::trials::Arm;
use crate::domain::{MemberId, ProjectId, Refusal, Role, TaskId, TaskStatus, WakeReason};
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
        lit.contains("The lamp lights")
            && lit.contains("Solved")
            && lit.ends_with("this turn has ended: stop now."),
        "the world's last word ends the turn: {lit}"
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

/// Three members on the house's models, and a Holodeck project for scenarios.
fn crew(hq: &Hq<'_>) -> ([MemberId; 3], ProjectId) {
    let (dev, project) = holodeck(hq);
    let zed = hq.hire("Zed", Role::Developer, 40, 2, 4).unwrap();
    let rae = hq.hire("Rae", Role::Developer, 40, 2, 4).unwrap();
    ([dev, zed, rae], project)
}

/// The station whose anomaly is `anomaly`, by seed.
fn station_seed(anomaly: Anomaly) -> u64 {
    (0..).find(|s| Station::new(*s).anomaly == anomaly).unwrap()
}

/// Each member's running turn, once the tick has started them.
fn turns(hq: &Hq<'_>, members: &[MemberId]) -> Vec<RunContext> {
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    let started = hq.runner().started();
    members
        .iter()
        .map(|m| {
            started
                .iter()
                .rev()
                .find(|c| c.member == *m)
                .unwrap_or_else(|| {
                    panic!(
                        "a turn of {m:?}'s: started {:?}; wakes {:?}",
                        started
                            .iter()
                            .map(|c| (c.member, c.task))
                            .collect::<Vec<_>>(),
                        hq.store()
                            .wakes()
                            .unwrap()
                            .iter()
                            .map(|w| (w.member, w.task, w.reason, w.receipt))
                            .collect::<Vec<_>>()
                    )
                })
                .clone()
        })
        .collect()
}

fn act(hq: &Hq<'_>, context: &RunContext, action: &str) -> Result<String, String> {
    use_tool(hq, context, "scenario", &json!({ "action": action }))
}

// G4 of the roadmap: Silent Station plays end to end, with enforced consequences and not a
// scripted chat. Three members wear the mission's roles: the captain holds the mission, science
// and the engineer a part of it each, all in one world. Each acts only as its role; an accepted
// action wakes the rest of the crew; the world closes the mission and every part of it, scored.
#[test]
fn silent_station_is_played_by_a_crew_in_one_world_and_closes_itself() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([captain, science, engineer], project) = crew(&hq);
    let seed = station_seed(Anomaly::MagneticField);
    let mission = hq
        .start_station(project, [captain, science, engineer], seed)
        .unwrap();
    let parts: Vec<_> = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .filter(|t| t.parent == Some(mission))
        .collect();
    assert_eq!(
        parts.iter().map(|t| t.assignee).collect::<Vec<_>>(),
        [Some(science), Some(engineer)]
    );
    assert!(parts[1].description.contains("You are the engineer"));
    let [c, s, e] = <[RunContext; 3]>::try_from(turns(&hq, &[captain, science, engineer]))
        .ok()
        .unwrap();
    let (run, member) = hq.turn_of(&e.token).unwrap().unwrap();
    let scenario_tool = tools_in(&hq, &run, &member)
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "scenario")
        .cloned()
        .expect("the crew is offered the scenario tool");
    assert!(
        scenario_tool.to_string().contains("route_power"),
        "the station's actions: {scenario_tool}"
    );

    // Each role acts alone, and nothing happens before the scan.
    assert!(
        act(&hq, &c, "attempt_contact")
            .unwrap_err()
            .contains("scan")
    );
    assert!(
        act(&hq, &e, "scan_station")
            .unwrap_err()
            .contains("the science's to do, not the engineer's")
    );
    let woken_before = hq.store().wakes().unwrap().len();
    assert!(
        act(&hq, &s, "scan_station")
            .unwrap()
            .contains("looks intact")
    );
    let woken: Vec<MemberId> = hq
        .store()
        .wakes()
        .unwrap()
        .into_iter()
        .skip(woken_before)
        .map(|w| w.member)
        .collect();
    assert!(
        woken.contains(&captain) && woken.contains(&engineer) && !woken.contains(&science),
        "the rest of the crew hears of it: {woken:?}"
    );
    assert!(
        use_tool(
            &hq,
            &e,
            "set_status",
            &json!({ "task": parts[1].id.0, "status": "done", "reason": "fixed it" }),
        )
        .unwrap_err()
        .contains("the world")
    );

    // The probe as a relay: analyse, place it, call.
    assert!(
        act(&hq, &s, "analyze_anomaly")
            .unwrap()
            .contains("magnetic field")
    );
    assert!(act(&hq, &c, "send_probe").unwrap().contains("relay"));
    let answered = act(&hq, &c, "attempt_contact").unwrap();
    assert!(
        answered.contains("answers") && answered.contains("success"),
        "{answered}"
    );

    for task in std::iter::once(mission).chain(parts.iter().map(|t| t.id)) {
        assert_eq!(
            hq.store().task(task).unwrap().unwrap().status,
            TaskStatus::Done,
            "the world closes the mission and its parts"
        );
    }
    let shown = hq.scenario_shown(mission).unwrap();
    assert_eq!(
        (
            &shown["kind"],
            &shown["end"],
            &shown["score"],
            &shown["anomaly"]
        ),
        (
            &json!("silent_station"),
            &json!("success"),
            &json!(75),
            &json!("magnetic field")
        )
    );
    let log: Vec<(String, bool, String)> = shown["log"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["action"].as_str().unwrap().to_owned(),
                a["ok"].as_bool().unwrap(),
                a["role"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        log,
        [
            ("attempt_contact".to_owned(), false, "captain".to_owned()),
            ("scan_station".to_owned(), false, "engineer".to_owned()),
            ("scan_station".to_owned(), true, "science".to_owned()),
            ("analyze_anomaly".to_owned(), true, "science".to_owned()),
            ("send_probe".to_owned(), true, "captain".to_owned()),
            ("attempt_contact".to_owned(), true, "captain".to_owned()),
        ]
    );
    // The same log replays to the same station.
    assert_eq!(
        hq.replay_station(mission).unwrap(),
        Station::replay(
            seed,
            &[
                StationAction::ScanStation,
                StationAction::AnalyzeAnomaly,
                StationAction::SendProbe,
                StationAction::AttemptContact
            ]
        )
        .unwrap()
    );
}

#[test]
fn rushing_the_storm_fails_the_mission_and_drops_every_part_of_it() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([captain, science, engineer], project) = crew(&hq);
    let mission = hq
        .start_station(
            project,
            [captain, science, engineer],
            station_seed(Anomaly::IonStorm),
        )
        .unwrap();
    let [_, s, e] = <[RunContext; 3]>::try_from(turns(&hq, &[captain, science, engineer]))
        .ok()
        .unwrap();
    act(&hq, &s, "scan_station").unwrap();
    act(&hq, &e, "inspect_comms").unwrap();
    let hurt = act(&hq, &e, "route_power").unwrap();
    assert!(hurt.contains("hurt") && hurt.contains("failure"), "{hurt}");
    let all: Vec<TaskStatus> = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .filter(|t| t.id == mission || t.parent == Some(mission))
        .map(|t| t.status)
        .collect();
    assert_eq!(all, [TaskStatus::Dropped; 3]);
    assert_eq!(hq.scenario_shown(mission).unwrap()["score"], 0);
    assert_eq!(
        hq.start_station(project, [captain, captain, engineer], 1),
        Err(EngineError::Refused(Refusal::Scenario(
            "a crew is three different members".to_owned()
        )))
    );
}

fn arm(label: &str, members: &[MemberId]) -> Arm {
    Arm {
        label: label.to_owned(),
        members: members.to_vec(),
        brief: None,
    }
}

// The Holodeck plan's phase 7: one thing changes between conditions, here who plays, and the
// world, its version and its seeds do not. Each condition plays every seed; what each run was
// configured with is kept when it starts; the report puts the conditions side by side from the
// record, every run with its own, and says what a few runs cannot.
#[test]
fn a_trial_plays_each_condition_on_the_same_seeds_and_reports_them_side_by_side() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([linus, zed, _], project) = crew(&hq);
    let seed = seed_for(Cause::Bulb);
    let started = hq
        .start_trial(
            "who-lights-it",
            project,
            "lighthouse",
            &[arm("linus", &[linus]), arm("zed", &[zed])],
            &[seed],
        )
        .unwrap();
    assert_eq!(started.len(), 2);
    assert!(
        started
            .iter()
            .all(|t| hq.scenario_of(*t).unwrap() == Some(seed))
    );
    let [l, z] = <[RunContext; 2]>::try_from(turns(&hq, &[linus, zed]))
        .ok()
        .unwrap();
    // Linus goes straight to it; Zed tries the circuit first and the wrong part.
    for (context, actions) in [
        (&l, &[("inspect", None), ("replace_part", Some("bulb"))][..]),
        (
            &z,
            &[
                ("test_circuit", None),
                ("inspect", None),
                ("replace_part", Some("fuse")),
                ("replace_part", Some("bulb")),
            ][..],
        ),
    ] {
        for (action, part) in actions {
            let _ = use_tool(
                &hq,
                context,
                "scenario",
                &json!({ "action": action, "part": part }),
            );
        }
    }

    let report = hq.trial_report("who-lights-it").unwrap();
    assert_eq!(
        (&report["kind"], &report["version"], &report["seeds"]),
        (&json!("lighthouse"), &json!(1), &json!([seed]))
    );
    let conditions = report["conditions"].as_array().unwrap();
    let by = |i: usize, key: &str| conditions[i][key].clone();
    assert_eq!(
        (
            by(0, "label"),
            by(0, "done"),
            by(0, "mean_score"),
            by(0, "mean_elapsed"),
            by(0, "refused")
        ),
        (json!("linus"), json!(1), json!(100), json!(20), json!(0))
    );
    assert_eq!(
        (
            by(1, "label"),
            by(1, "done"),
            by(1, "mean_elapsed"),
            by(1, "refused"),
            by(1, "turns")
        ),
        (json!("zed"), json!(1), json!(35), json!(1), json!(1))
    );
    let first = &report["runs"][0];
    assert_eq!(
        (&first["condition"], &first["config"]["members"][0]["name"]),
        (&json!("linus"), &json!("Linus"))
    );
    assert!(first["config"]["members"][0]["work_brain"].is_string());
    let markdown = report["markdown"].as_str().unwrap();
    assert!(markdown.starts_with("# Trial who-lights-it"), "{markdown}");
    assert!(
        markdown.contains("| zed | 1 | 1 | 0 | 0 | 100 | 35 | 1 |"),
        "{markdown}"
    );
}

#[test]
fn a_trial_compares_two_conditions_or_more_once_and_within_bounds() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([a, b, c], project) = crew(&hq);
    let refused = |name: &str, kind: &str, arms: &[Arm], seeds: &[u64]| match hq
        .start_trial(name, project, kind, arms, seeds)
    {
        Err(EngineError::Refused(Refusal::Scenario(why))) => why,
        other => panic!("{name}: {other:?}"),
    };
    let two = [arm("a", &[a]), arm("b", &[b])];
    assert!(refused("Bad Name", "lighthouse", &two, &[1]).contains("name"));
    assert!(refused("one", "lighthouse", &two[..1], &[1]).contains("two conditions"));
    assert!(
        refused(
            "same",
            "lighthouse",
            &[arm("a", &[a]), arm("a", &[b])],
            &[1]
        )
        .contains("label")
    );
    assert!(refused("no-seeds", "lighthouse", &two, &[]).contains("seed"));
    assert!(refused("big", "lighthouse", &two, &(0..13).collect::<Vec<_>>()).contains("24"));
    assert!(refused("crew", "silent_station", &two, &[1]).contains("three"));
    assert!(
        refused("what", "holodeck", &two, &[1]).contains("lighthouse, silent_station or chess")
    );
    let mut odd = arm("odd", &[a]);
    odd.brief = Some("plan_first".to_owned());
    assert!(refused("brief", "lighthouse", &[odd, arm("b", &[b])], &[1]).contains("no briefing"));
    assert_eq!(
        hq.start_trial("ok", project, "lighthouse", &two, &[1])
            .unwrap()
            .len(),
        2
    );
    assert!(refused("ok", "lighthouse", &two, &[2]).contains("already"));
    assert_eq!(
        hq.trial_report("nothing"),
        Err(EngineError::Unknown("trial"))
    );
    let crews = [arm("x", &[a, b, c]), arm("y", &[c, b, a])];
    assert_eq!(
        hq.start_trial("crews", project, "silent_station", &crews, &[1])
            .unwrap()
            .len(),
        2
    );

    // What a run is configured with is the brain its turns will really get, as the engine
    // places them: live, a reviewer whose own work brain was the house's model played on the
    // Mac, since a tester's work is the Mac's (ADR 0131), and the trial said "local".
    let ivy = hq.hire("Ivy", Role::Reviewer, 20, 2, 1).unwrap();
    hq.start_trial(
        "roles",
        project,
        "lighthouse",
        &[arm("dev", &[a]), arm("tester", &[ivy])],
        &[1],
    )
    .unwrap();
    let report = hq.trial_report("roles").unwrap();
    let turn_brain = |i: usize| report["runs"][i]["config"]["members"][0]["turn_brain"].clone();
    assert_eq!(turn_brain(1), json!("mac"), "{}", report["runs"][1]);
    assert!(turn_brain(0).is_string());
}

fn play(hq: &Hq<'_>, context: &RunContext, san: &str) -> Result<String, String> {
    use_tool(
        hq,
        context,
        "scenario",
        &json!({ "action": "move", "san": san }),
    )
}

// Chess as a scenario: the owner wanted to see a game move by move, which model played what,
// and the illegal moves, and none of it was kept (and no game was ever played: moves came only
// in downtime turns). A match is two members' world: a move a turn, every move tried kept, the
// side not to move told its turn is over, the other side woken by a legal move, and the rules
// close it.
#[test]
fn a_chess_match_is_played_a_move_a_turn_with_its_illegal_moves_kept() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let game = hq.start_chess(project, [white, black], 1).unwrap();
    let part = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.parent == Some(game))
        .expect("black's part");
    assert_eq!(part.assignee, Some(black));
    let [w, b] = <[RunContext; 2]>::try_from(turns(&hq, &[white, black]))
        .ok()
        .unwrap();
    let (run, member) = hq.turn_of(&b.token).unwrap().unwrap();
    let tool = tools_in(&hq, &run, &member)
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "scenario")
        .cloned()
        .unwrap();
    assert!(tool.to_string().contains("san"), "{tool}");

    let waiting = act(&hq, &b, "look").unwrap();
    assert!(
        waiting.contains("not your move: this turn has ended"),
        "{waiting}"
    );
    assert!(
        play(&hq, &w, "e5").unwrap_err().contains("e4"),
        "illegal, with the legal moves"
    );
    let woken_before = hq.store().wakes().unwrap().len();
    let moved = play(&hq, &w, "f3").unwrap();
    assert!(
        moved.contains("white played f3") && moved.contains("this turn has ended"),
        "{moved}"
    );
    let woken: Vec<(MemberId, Option<TaskId>)> = hq
        .store()
        .wakes()
        .unwrap()
        .into_iter()
        .skip(woken_before)
        .map(|w| (w.member, w.task))
        .collect();
    assert_eq!(woken, [(black, Some(part.id))], "the other side is woken");
    assert!(play(&hq, &w, "e4").unwrap_err().contains("black's move"));
    play(&hq, &b, "e5").unwrap();
    play(&hq, &w, "g4").unwrap();
    let mate = play(&hq, &b, "Qh4").unwrap();
    assert!(
        mate.contains("over: black_won") && mate.contains("this turn has ended"),
        "{mate}"
    );
    for task in [game, part.id] {
        assert_eq!(
            hq.store().task(task).unwrap().unwrap().status,
            TaskStatus::Done
        );
    }

    let shown = hq.scenario_shown(game).unwrap();
    assert_eq!(
        (
            &shown["kind"],
            &shown["result"],
            &shown["score"],
            &shown["moves"]
        ),
        (
            &json!("chess"),
            &json!("black_won"),
            &json!(0),
            &json!("1. f3 e5 2. g4 Qh4#")
        )
    );
    assert_eq!(shown["sides"]["white"], json!("Linus"));
    let by_role: Vec<(String, u64, u64)> = shown["by_role"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["role"].as_str().unwrap().to_owned(),
                r["accepted"].as_u64().unwrap(),
                r["refused"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        by_role,
        [("black".to_owned(), 2, 0), ("white".to_owned(), 2, 2)],
        "white's illegal move and its move out of turn are kept"
    );
    assert_eq!(
        hq.replay_chess(game).unwrap().move_text(),
        "1. f3 e5 2. g4 Qh4#"
    );

    // Model against model: the same two, colours swapped between conditions.
    let started = hq
        .start_trial(
            "colours",
            project,
            "chess",
            &[
                arm("linus-white", &[white, black]),
                arm("zed-white", &[black, white]),
            ],
            &[1],
        )
        .unwrap();
    assert_eq!(started.len(), 2);
    let report = hq.trial_report("colours").unwrap();
    assert_eq!(report["kind"], "chess");
    assert!(report["runs"][0]["by_role"].is_array());
}

// The owner's screen lists the Holodeck: the scenarios, newest first, with their world, title
// and end, and the trials, with their world and how many runs they started.
#[test]
fn the_screen_lists_the_scenarios_and_the_trials() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([linus, zed, rae], project) = crew(&hq);
    let lamp = hq.start_scenario(project, linus, 2).unwrap();
    let game = hq.start_chess(project, [zed, rae], 1).unwrap();
    hq.start_trial(
        "pair",
        project,
        "lighthouse",
        &[arm("a", &[linus]), arm("b", &[zed])],
        &[3],
    )
    .unwrap();
    let shown = hq.holodeck_shown().unwrap();
    let scenarios = shown["scenarios"].as_array().unwrap();
    assert_eq!(
        scenarios.len(),
        4,
        "the two started and the trial's two: {shown}"
    );
    assert_eq!(scenarios[2]["task"], game.0, "newest first");
    assert_eq!(scenarios[2]["kind"], "chess");
    assert_eq!(scenarios[3]["task"], lamp.0);
    assert!(
        scenarios[3]["title"]
            .as_str()
            .unwrap()
            .contains("lighthouse")
    );
    assert_eq!(scenarios[3]["end"], json!(null));
    assert_eq!(
        shown["trials"],
        json!([{ "name": "pair", "kind": "lighthouse", "runs": 2 }])
    );
}

// Live, the project's lead was woken on the crew's parts by the stuck-work ladder and acted in
// their roles: a role is its task's member's, and nobody else's, whoever is woken on it.
#[test]
fn only_a_tasks_own_member_acts_in_its_role() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([captain, science, engineer], project) = crew(&hq);
    let ada = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.name == "Ada")
        .unwrap()
        .id;
    let mission = hq
        .start_station(
            project,
            [captain, science, engineer],
            station_seed(Anomaly::IonStorm),
        )
        .unwrap();
    let part = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.parent == Some(mission))
        .unwrap();
    // The crew's first turns end, so a wake on a part is not held behind its own member's.
    for c in turns(&hq, &[captain, science, engineer]) {
        hq.runner().finish(c.run, true);
    }
    hq.tick().unwrap();
    hq.wake(ada, Some(part.id), WakeReason::Mentioned).unwrap();
    let [lead] = <[RunContext; 1]>::try_from(turns(&hq, &[ada]))
        .ok()
        .unwrap();
    let (run, member) = hq.turn_of(&lead.token).unwrap().unwrap();
    assert_eq!(run.task, Some(part.id));
    assert!(
        !tools_in(&hq, &run, &member)
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "scenario"),
        "not offered to her"
    );
    let refused = act(&hq, &lead, "scan_station").unwrap_err();
    assert!(refused.contains("Zed's"), "{refused}");
    assert!(
        hq.store().scenario_actions(mission).unwrap().is_empty(),
        "nothing kept as an act"
    );
}

// A scenario's parts wait for their world to wake them, by design: the stuck-work guards and
// the ladder leave them alone (live, the parts were blocked and the lead woken to decide).
#[test]
fn the_stuck_work_guards_leave_a_scenario_alone() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([captain, science, engineer], project) = crew(&hq);
    let mission = hq
        .start_station(
            project,
            [captain, science, engineer],
            station_seed(Anomaly::IonStorm),
        )
        .unwrap();
    let contexts = turns(&hq, &[captain, science, engineer]);
    for (context, task) in contexts.iter().zip(
        hq.store()
            .tasks()
            .unwrap()
            .into_iter()
            .filter(|t| t.id == mission || t.parent == Some(mission)),
    ) {
        use_tool(&hq, context, "claim", &json!({ "task": task.id.0 })).unwrap();
    }
    for c in &contexts {
        hq.runner().finish(c.run, true);
    }
    let decided_before = hq
        .store()
        .wakes()
        .unwrap()
        .iter()
        .filter(|w| w.reason == WakeReason::Decide)
        .count();
    for _ in 0..40 {
        clock.0.fetch_add(15 * 60 * 1000, Ordering::SeqCst);
        hq.tick().unwrap();
        for started in hq.runner().started() {
            hq.runner().finish(started.run, true);
        }
    }
    let statuses: Vec<TaskStatus> = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .filter(|t| t.id == mission || t.parent == Some(mission))
        .map(|t| t.status)
        .collect();
    assert!(!statuses.contains(&TaskStatus::Blocked), "{statuses:?}");
    let decided = hq
        .store()
        .wakes()
        .unwrap()
        .iter()
        .filter(|w| w.reason == WakeReason::Decide)
        .count();
    assert_eq!(decided, decided_before, "no lead woken to decide it");
}

// A world that has ended with its tasks still open (as live, when its end came from a rule
// added after its last action) is closed by the tick.
#[test]
fn the_tick_closes_a_world_that_has_ended() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([captain, science, engineer], project) = crew(&hq);
    let mission = hq
        .start_station(
            project,
            [captain, science, engineer],
            station_seed(Anomaly::IonStorm),
        )
        .unwrap();
    let [c] = <[RunContext; 1]>::try_from(turns(&hq, &[captain]))
        .ok()
        .unwrap();
    for action in [
        "scan_station",
        "analyze_anomaly",
        "inspect_comms",
        "route_power",
        "send_probe",
    ] {
        hq.store()
            .add_scenario_action(&super::ports::ScenarioAction {
                id: 0,
                task: mission,
                run: c.run,
                member: captain,
                action: action.to_owned(),
                ok: true,
                said: String::new(),
                at: OCT_17,
            })
            .unwrap();
    }
    hq.tick().unwrap();
    for task in hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .filter(|t| t.id == mission || t.parent == Some(mission))
    {
        assert_eq!(
            task.status,
            TaskStatus::Dropped,
            "partial is not the work done"
        );
    }
    assert_eq!(hq.scenario_shown(mission).unwrap()["end"], "partial");
}

// Live, black's turn on the Mac ran past its time and was stopped, the wake that would have
// brought it back had been folded into that turn, and the match waited for ever. A world whose
// player has no turn coming is woken again when it has sat idle, three times at most; then the
// world abandons it, and says so.
#[test]
fn an_idle_player_is_woken_again_three_times_and_then_the_world_abandons_it() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let game = hq.start_chess(project, [white, black], 1).unwrap();
    let part = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.parent == Some(game))
        .unwrap();
    let [w, _b] = <[RunContext; 2]>::try_from(turns(&hq, &[white, black]))
        .ok()
        .unwrap();
    play(&hq, &w, "e4").unwrap();
    let woken = |hq: &Hq<'_>| {
        hq.store()
            .wakes()
            .unwrap()
            .iter()
            .filter(|x| x.member == black && x.task == Some(part.id))
            .count()
    };
    // Black's turns all end without a move; nothing else would bring it back.
    let mut counts = Vec::new();
    for _ in 0..6 {
        for started in hq.runner().started() {
            hq.runner().finish(started.run, true);
        }
        clock.0.fetch_add(11 * 60 * 1000, Ordering::SeqCst);
        hq.tick().unwrap();
        counts.push(woken(&hq));
    }
    let first = counts[0];
    assert!(
        counts.iter().max().unwrap() - first <= 3,
        "woken again three times at most: {counts:?}"
    );
    assert!(
        counts.iter().max().unwrap() > &first,
        "woken again: {counts:?}"
    );
    for task in [game, part.id] {
        assert_eq!(
            hq.store().task(task).unwrap().unwrap().status,
            TaskStatus::Dropped,
            "abandoned: {counts:?}"
        );
    }
    let shown = hq.scenario_shown(game).unwrap();
    assert_eq!(shown["status"], "dropped");
    assert!(
        hq.store()
            .thread(game)
            .unwrap()
            .iter()
            .any(|p| p.text.contains("abandoned")),
        "the world says why"
    );
}

// A move resets it: a player who acts is never counted idle for its earlier silences.
#[test]
fn acting_again_resets_the_count_of_wakes() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let game = hq.start_chess(project, [white, black], 1).unwrap();
    let [w, b] = <[RunContext; 2]>::try_from(turns(&hq, &[white, black]))
        .ok()
        .unwrap();
    play(&hq, &w, "e4").unwrap();
    for _ in 0..2 {
        for started in hq.runner().started() {
            if started.run != b.run {
                hq.runner().finish(started.run, true);
            }
        }
        clock.0.fetch_add(11 * 60 * 1000, Ordering::SeqCst);
        hq.tick().unwrap();
    }
    play(&hq, &b, "e5").unwrap();
    assert!(!hq.store().task(game).unwrap().unwrap().status.finished());
}

// A trial whose conditions differ in how the crew is briefed, not in who plays: the same crew,
// the same seeds; in one the captain posts the crew's plan before anyone acts, and the world
// holds every action until it has.
#[test]
fn a_trial_compares_briefings_with_the_crew_and_seeds_fixed() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([captain, science, engineer], project) = crew(&hq);
    let crew_of = [captain, science, engineer];
    let mut planned = arm("plan-first", &crew_of);
    planned.brief = Some("plan_first".to_owned());
    let seed = station_seed(Anomaly::IonStorm);
    let started = hq
        .start_trial(
            "briefings",
            project,
            "silent_station",
            &[arm("current", &crew_of), planned],
            &[seed],
        )
        .unwrap();
    assert_eq!(started.len(), 2);
    let report = hq.trial_report("briefings").unwrap();
    assert_eq!(report["runs"][0]["config"]["brief"], json!(null));
    assert_eq!(report["runs"][1]["config"]["brief"], "plan_first");

    // Played alone, so each member's turn is on its part of this mission.
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([captain, science, engineer], project) = crew(&hq);
    let mission = hq
        .start_station_with(project, [captain, science, engineer], seed, true)
        .unwrap();
    let parts: Vec<_> = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .filter(|t| t.parent == Some(mission))
        .collect();
    assert!(parts[0].description.contains("Wait for the captain's plan"));
    let [c, s, _e] = <[RunContext; 3]>::try_from(turns(&hq, &[captain, science, engineer]))
        .ok()
        .unwrap();
    assert!(
        act(&hq, &s, "scan_station")
            .unwrap_err()
            .contains("has not posted the crew's plan")
    );
    let woken_before = hq.store().wakes().unwrap().len();
    let posted = use_tool(
        &hq,
        &c,
        "scenario",
        &json!({ "action": "plan", "text": "Science scans and analyses; the engineer inspects and routes power; I call." }),
    )
    .unwrap();
    assert!(posted.starts_with("The captain's plan:"), "{posted}");
    assert!(
        hq.store().wakes().unwrap().len() > woken_before,
        "the crew is woken to the plan"
    );
    assert!(
        act(&hq, &s, "scan_station")
            .unwrap()
            .contains("Four life signs")
    );
    let shown = hq.scenario_shown(mission).unwrap();
    assert_eq!(shown["brief"], "plan_first");
    assert!(shown["plan"].as_str().unwrap().contains("I call."));
    assert_eq!(
        shown["log"][0]["action"], "scan_station",
        "the refused scan is kept first"
    );
}
