//! The team's map (ADR 0126): a page for every area, project, goal and role, each with a
//! steward worked out from the data; members propose to pages they do not steward; every
//! briefing opens with a map; a member who looks and finds nothing says so, apart from the
//! broken roads code finds; and a nightly quiz on the free model tests that the map carries.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::application::run_ports::RunStore;

use serde_json::{Value, json};

use super::engine::{Author, Engine, Settings};
use super::map_ports::MapStore;
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::domain::{GoalId, MemberId, Priority, ProjectId, Role, TaskId};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

/// 18 October 2026, at noon UTC.
const OCT_18: i64 = 1_792_324_800_000;

struct Team {
    ada: MemberId,
    linus: MemberId,
    dee: MemberId,
    goal: GoalId,
    upkeep: ProjectId,
}

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings::default(),
    )
}

/// Mara the CEO; Ada and Bo, two leads; Linus a developer and Dee the tester, both under Ada;
/// one goal and the project "Upkeep", Ada's.
fn team(hq: &Hq<'_>) -> Team {
    let mara = hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
    hq.set_brain(mara, "claude-opus-5-5").unwrap();
    let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    hq.hire("Bo", Role::Lead, 25, 2, 3).unwrap();
    let linus = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let dee = hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    let upkeep = hq.add_project(goal, "Upkeep", "", ada, None).unwrap();
    Team {
        ada,
        linus,
        dee,
        goal,
        upkeep,
    }
}

fn task_for(hq: &Hq<'_>, goal: GoalId, to: MemberId, title: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
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
    .unwrap()
}

fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn of the member's")
}

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &Value) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

/// A turn of `member`'s on a task of its own, started, ready for tool calls.
fn working(hq: &Hq<'_>, goal: GoalId, member: MemberId, title: &str) -> (RunContext, TaskId) {
    let task = task_for(hq, goal, member, title);
    (turn(hq, member), task)
}

#[test]
fn every_project_and_goal_has_a_page_made_once() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    let made = hq.migrate_map().unwrap();
    assert_eq!((made.projects, made.goals), (1, 1), "one project, one goal");
    let pages = hq.store().world_pages().unwrap();
    for slug in [
        format!("project/{}", t.upkeep.0),
        format!("goal/{}", t.goal.0),
    ] {
        assert!(pages.contains(&slug), "{slug} in {pages:?}");
    }
    assert_eq!(
        hq.migrate_map().unwrap().total(),
        0,
        "a second start changes nothing"
    );
}

#[test]
fn a_store_from_before_the_map_gains_the_pages_and_no_other_row_changes() {
    let path = std::env::temp_dir().join(format!("endora-map-before-{}.db", std::process::id()));
    let file = path.to_str().unwrap().to_owned();
    let _ = std::fs::remove_file(&path);
    let clock = Hands(AtomicI64::new(OCT_18));
    let members_before = {
        let hq = Engine::new(
            SqliteTeamStore::open(&file).unwrap(),
            SimulatedRunner::new(2),
            &clock,
            Settings::default(),
        );
        team(&hq);
        hq.store().members().unwrap().len()
    };
    // The store as it was before the map: none of its tables.
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute_batch("DROP TABLE world_pages;").unwrap();
    drop(raw);
    let hq = Engine::new(
        SqliteTeamStore::open(&file).unwrap(),
        SimulatedRunner::new(2),
        &clock,
        Settings::default(),
    );
    assert!(hq.store().world_pages().unwrap().is_empty());
    assert_eq!(hq.migrate_map().unwrap().projects, 1);
    assert_eq!(hq.migrate_map().unwrap().total(), 0);
    assert_eq!(hq.store().members().unwrap().len(), members_before);
    drop(hq);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_page_that_is_not_there_is_not_an_address() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    task_for(&hq, t.goal, t.linus, "Fix the proxy");
    let linus = turn(&hq, t.linus);
    let said = use_tool(
        &hq,
        &linus,
        "read_world",
        &json!({ "address": "world:project/9999" }),
    )
    .unwrap_err();
    assert!(
        said.contains("no page") || said.contains("nothing is at"),
        "{said}"
    );
    let said = use_tool(
        &hq,
        &linus,
        "read_world",
        &json!({ "address": "world:garage" }),
    )
    .unwrap_err();
    assert!(said.contains("not an address"), "{said}");
    let _ = (t.dee, t.upkeep);
}

fn end(hq: &Hq<'_>, clock: &Hands, context: &RunContext) {
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
    clock.0.fetch_add(3 * 60_000, Ordering::SeqCst);
}

/// The map block of a briefing.
fn map_of(told: &str) -> String {
    crate::domain::map::map_block(told).unwrap_or_else(|| panic!("no map in:\n{told}"))
}

#[test]
fn every_briefing_opens_with_a_map_of_the_pages_that_matter_to_its_member() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: t.goal,
                project: Some(t.upkeep),
                parent: None,
                title: "Fix the proxy".to_owned(),
                description: String::new(),
                assignee: Some(t.linus),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    let linus = turn(&hq, t.linus);
    let map = map_of(&linus.told);
    for wanted in [
        format!("world:project/{}", t.upkeep.0),
        "world:house".to_owned(),
        "world:lab".to_owned(),
        "Kept by the owner.".to_owned(),
        "Kept by HQ.".to_owned(),
        "add_to_map".to_owned(),
    ] {
        assert!(map.contains(&wanted), "{wanted} in\n{map}");
    }
    assert!(
        !map.contains("world:area/") && !map.contains("world:role/"),
        "{map}"
    );
    assert!(map.lines().all(|l| l.chars().count() <= 260), "{map}");
    // It opens the briefing: after who the member is, before its task.
    let at = linus.told.find("Your map of the team's world").unwrap();
    assert!(
        at < linus.told.find("Your task:").unwrap(),
        "{}",
        linus.told
    );
    assert!(at < 120, "right after who it is: {}", linus.told);
    // The run records it was told.
    let briefed = hq.briefed(linus.run).unwrap().unwrap();
    assert_eq!(briefed.briefed.first().map(String::as_str), Some("map"));
    let _ = task;
    end(&hq, &clock, &linus);
}

/// Many projects, for a wide map.
const WIDE: [&str; 14] = [
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet",
    "kilo", "lima", "mike", "november",
];

/// An item that names every one of them: a turn on an item is told only the pages it names
/// (ADR 0142, amended 2026-10-30), so a wide map is one whose item names many.
fn wide_item() -> String {
    format!("Fix the proxy of {}", WIDE.join(", "))
}

#[test]
fn a_turn_on_the_mac_has_a_longer_map_and_a_bigger_briefing_than_one_on_the_nas() {
    use crate::domain::map::{MAP_CHARS_LOCAL, MAP_CHARS_MAC};
    use crate::domain::situations::{LOCAL_BUDGET, MAC_BUDGET};
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    for name in WIDE {
        hq.add_project(t.goal, &format!("Project {name}"), "", t.ada, None)
            .unwrap();
    }
    hq.migrate_map().unwrap();
    // The node says the Mac is free, so Linus's work runs on it: a large context, its own limits.
    hq.tell_mac(true, "ready");
    let (linus, _) = working(&hq, t.goal, t.linus, &wide_item());
    assert_eq!(
        hq.store().run(linus.run).unwrap().unwrap().brain,
        Some(crate::domain::Brain::Mac),
        "his turn runs on the Mac"
    );
    let briefed = hq.briefed(linus.run).unwrap().unwrap();
    assert_eq!(briefed.budget, MAC_BUDGET);
    assert!(briefed.budget > LOCAL_BUDGET);
    let map = map_of(&linus.told);
    let chars = map.chars().count();
    assert!(
        chars <= MAP_CHARS_MAC + 200,
        "the Mac's map keeps to its own size: {chars} chars"
    );
    assert!(
        chars > MAP_CHARS_LOCAL + 200,
        "and is longer than the NAS's model gets: {chars} chars"
    );
    assert!(map.contains("Left off this map to keep it short:"), "{map}");
}

#[test]
fn a_tight_map_leaves_off_the_least_and_names_it() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    // Many projects, so the map is wide; the node has not said the Mac is free, so Linus works on
    // the NAS's model, whose context is small: its map is short and its briefing small.
    for name in WIDE {
        hq.add_project(t.goal, &format!("Project {name}"), "", t.ada, None)
            .unwrap();
    }
    hq.migrate_map().unwrap();
    let (linus, _) = working(&hq, t.goal, t.linus, &wide_item());
    assert_eq!(
        hq.store().run(linus.run).unwrap().unwrap().brain,
        Some(crate::domain::Brain::Local),
        "his turn runs on the NAS's model"
    );
    assert_eq!(
        hq.briefed(linus.run).unwrap().unwrap().budget,
        crate::domain::situations::LOCAL_BUDGET
    );
    let map = map_of(&linus.told);
    assert!(
        map.chars().count() <= crate::domain::map::MAP_CHARS_LOCAL + 200,
        "{} chars",
        map.chars().count()
    );
    assert!(map.contains("Left off this map to keep it short:"), "{map}");
    // What matters most stays: the house.
    assert!(map.contains("- world:house "), "{map}");
    // What was left off is named, one by one.
    let left = map.lines().find(|l| l.starts_with("Left off")).unwrap();
    assert!(left.contains("world:project/"), "{left}");
}
