//! The team's map (ADR 0126): a page for every area, project, goal and role, each with a
//! steward worked out from the data; members propose to pages they do not steward; every
//! briefing opens with a map; a member who looks and finds nothing says so, apart from the
//! broken roads code finds; and a nightly quiz on the free model tests that the map carries.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Engine, Settings};
use super::map_ports::MapStore;
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::call;
use super::world_ports::WorldStore;
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
    ceo: MemberId,
    ada: MemberId,
    bo: MemberId,
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
    let ceo = hq.seat_ceo("Mara").unwrap().unwrap();
    let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let bo = hq.hire("Bo", Role::Lead, 25, 2, 3).unwrap();
    let linus = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let dee = hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
    for member in [linus, dee] {
        hq.set_reports_to(member, ada).unwrap();
    }
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    let upkeep = hq.add_project(goal, "Upkeep", "", ada, None).unwrap();
    Team {
        ceo,
        ada,
        bo,
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

fn area(lead: MemberId) -> String {
    format!("area/{}", lead.0)
}

fn write(page: &str, heading: &str, task: TaskId) -> Value {
    json!({ "page": page, "heading": heading, "headline": format!("{heading} headline"),
            "body": "How it goes.", "stage": "observation", "evidence": [format!("task {}", task.0)] })
}

#[test]
fn every_area_project_goal_and_role_has_a_page_made_once() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    let made = hq.migrate_map().unwrap();
    assert_eq!(
        (made.areas, made.projects, made.goals, made.roles),
        (2, 1, 1, 5),
        "two leads, one project, one goal, five roles"
    );
    let pages = hq.store().world_pages().unwrap();
    for slug in [
        area(t.ada),
        area(t.bo),
        format!("project/{}", t.upkeep.0),
        format!("goal/{}", t.goal.0),
        "role/ceo".to_owned(),
        "role/lead".to_owned(),
        "role/developer".to_owned(),
        "role/researcher".to_owned(),
        "role/tester".to_owned(),
    ] {
        assert!(pages.contains(&slug), "{slug} in {pages:?}");
    }
    assert!(
        !pages.contains(&area(t.linus)),
        "a developer has no area of its own"
    );
    assert_eq!(
        hq.migrate_map().unwrap().total(),
        0,
        "a second start changes nothing"
    );
    // A lead hired later gets its page, and only that one.
    let cy = hq.hire("Cy", Role::Lead, 25, 2, 5).unwrap();
    let later = hq.migrate_map().unwrap();
    assert_eq!((later.areas, later.total()), (1, 1));
    assert!(hq.store().world_pages().unwrap().contains(&area(cy)));
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
    assert_eq!(hq.migrate_map().unwrap().areas, 2);
    assert_eq!(hq.migrate_map().unwrap().total(), 0);
    assert_eq!(hq.store().members().unwrap().len(), members_before);
    drop(hq);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_pages_steward_is_who_leads_it_and_moves_when_the_lead_does() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    let roadmap = hq.add_project(t.goal, "Roadmap", "", t.ceo, None).unwrap();
    hq.migrate_map().unwrap();
    task_for(&hq, t.goal, t.linus, "Look at backups");
    let linus = turn(&hq, t.linus);
    let read = |address: &str| use_tool(&hq, &linus, "read_world", &json!({ "address": address }));
    let said = read(&format!("world:area/{}", t.ada.0)).unwrap();
    assert!(
        said.contains("Ada's area (world:area/") && said.contains("Kept by Ada."),
        "{said}"
    );
    assert!(
        read(&format!("world:project/{}", t.upkeep.0))
            .unwrap()
            .contains("Kept by Ada.")
    );
    assert!(
        read(&format!("world:project/{}", roadmap.0))
            .unwrap()
            .contains("Kept by Mara, the CEO."),
        "a project the CEO leads is the CEO's"
    );
    assert!(
        read(&format!("world:goal/{}", t.goal.0))
            .unwrap()
            .contains("Kept by Mara, the CEO.")
    );
    assert!(
        read("world:role/developer")
            .unwrap()
            .contains("Kept by the owner.")
    );
    assert!(
        read("world:house")
            .unwrap()
            .contains("Kept by Mara, the CEO.")
    );
    // Nothing says Ada's name where the steward is worked out: Bo leads the project instead.
    let mut upkeep = hq
        .store()
        .projects()
        .unwrap()
        .into_iter()
        .find(|p| p.id == t.upkeep)
        .unwrap();
    upkeep.lead = t.bo;
    hq.store().save_project(&upkeep).unwrap();
    assert!(
        read(&format!("world:project/{}", t.upkeep.0))
            .unwrap()
            .contains("Kept by Bo."),
        "the stewardship moved with the lead"
    );
}

#[test]
fn only_the_steward_or_the_owner_writes_a_stewarded_page() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    let ada_area = area(t.ada);
    let (ada, ada_task) = working(&hq, t.goal, t.ada, "Look at backups");
    let (bo, bo_task) = working(&hq, t.goal, t.bo, "Look at the router");
    let (linus, linus_task) = working(&hq, t.goal, t.linus, "Fix the proxy");
    // The area's lead writes its area page.
    let written = use_tool(
        &hq,
        &ada,
        "write_world",
        &write(&ada_area, "Backups", ada_task),
    )
    .unwrap();
    assert!(
        written.contains(&format!("world:{ada_area}#backups")),
        "{written}"
    );
    assert_eq!(
        hq.store().timeline(&ada_area).unwrap().len(),
        1,
        "a write is a line on the page's timeline"
    );
    // Another lead, the CEO and a developer are told to propose it instead.
    let refused = use_tool(
        &hq,
        &bo,
        "write_world",
        &write(&ada_area, "Router", bo_task),
    )
    .unwrap_err();
    assert!(
        refused.contains("kept by Ada") && refused.contains("propose_world"),
        "{refused}"
    );
    task_for(&hq, t.goal, t.ceo, "Review");
    let mara = turn(&hq, t.ceo);
    let refused = use_tool(
        &hq,
        &mara,
        "write_world",
        &write(&ada_area, "Router", ada_task),
    )
    .unwrap_err();
    assert!(refused.contains("kept by Ada"), "{refused}");
    let refused = use_tool(
        &hq,
        &linus,
        "write_world",
        &write(&ada_area, "Proxy", linus_task),
    )
    .unwrap_err();
    assert!(refused.contains("propose_world"), "{refused}");
    // A lead does not write the house page either; the CEO does.
    let refused =
        use_tool(&hq, &ada, "write_world", &write("house", "Disk", ada_task)).unwrap_err();
    assert!(refused.contains("kept by Mara, the CEO"), "{refused}");
    use_tool(&hq, &mara, "write_world", &write("house", "Disk", ada_task)).unwrap();
    // A role's page is the owner's.
    let refused = use_tool(
        &hq,
        &mara,
        "write_world",
        &write("role/developer", "Flow", ada_task),
    )
    .unwrap_err();
    assert!(refused.contains("kept by the owner"), "{refused}");
    assert_eq!(
        hq.write_as_owner(
            "role/developer",
            "flow",
            "How we work",
            "Branch, test, push."
        )
        .unwrap(),
        "world:role/developer#flow"
    );
    // The owner writes any written page; a derived one is nobody's.
    hq.write_as_owner(&ada_area, "owner-note", "Owner's note", "Backups matter.")
        .unwrap();
    assert!(hq.write_as_owner("lab", "x", "X", "y").is_err());
    // Only the steward retires or confirms a section.
    let refused = use_tool(
        &hq,
        &bo,
        "retire_world",
        &json!({ "address": format!("world:{ada_area}#backups"), "why": "stale" }),
    )
    .unwrap_err();
    assert!(refused.contains("Ada's to write"), "{refused}");
    use_tool(
        &hq,
        &ada,
        "retire_world",
        &json!({ "address": format!("world:{ada_area}#backups"), "why": "stale" }),
    )
    .unwrap();
    let after = use_tool(
        &hq,
        &bo,
        "read_world",
        &json!({ "address": format!("world:{ada_area}#backups") }),
    )
    .unwrap();
    assert!(
        after.contains("retired"),
        "it still resolves and says why\n{after}"
    );
}

#[test]
fn a_sections_address_survives_its_pages_promotion_and_edits() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    let page = area(t.ada);
    let (ada, task) = working(&hq, t.goal, t.ada, "Look at backups");
    use_tool(&hq, &ada, "write_world", &write(&page, "Backups", task)).unwrap();
    let address = format!("world:{page}#backups");
    let before = use_tool(&hq, &ada, "read_world", &json!({ "address": address })).unwrap();
    assert!(before.contains("observation"));
    hq.promote(&address, "the owner checked").unwrap();
    // A later edit of the heading would not move it: the anchor was set once.
    let after = use_tool(&hq, &ada, "read_world", &json!({ "address": address })).unwrap();
    assert!(
        after.contains("canon") && after.contains(&address),
        "{after}"
    );
    let refused = use_tool(&hq, &ada, "write_world", &write(&page, "Backups", task)).unwrap_err();
    assert!(refused.contains("owner"), "canon is the owner's: {refused}");
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
        &json!({ "address": "world:area/9999" }),
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
        "world:role/developer".to_owned(),
        format!("world:area/{}", t.ada.0),
        format!("world:project/{}", t.upkeep.0),
        "world:house".to_owned(),
        "world:lab".to_owned(),
        "Kept by Ada.".to_owned(),
        "Kept by the owner.".to_owned(),
        "Kept by Mara, the CEO.".to_owned(),
        "report_gap".to_owned(),
    ] {
        assert!(map.contains(&wanted), "{wanted} in\n{map}");
    }
    // Wide and shallow: another lead's area is on it too, a line each.
    assert!(map.contains(&format!("world:area/{}", t.bo.0)), "{map}");
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
    // A lead's map holds the project it leads; the CEO's holds every area.
    task_for(&hq, t.goal, t.ada, "Look at backups");
    let ada = turn(&hq, t.ada);
    assert!(map_of(&ada.told).contains(&format!("world:project/{}", t.upkeep.0)));
    assert!(map_of(&ada.told).contains("world:role/lead"));
    task_for(&hq, t.goal, t.ceo, "Review");
    let mara = turn(&hq, t.ceo);
    let wide = map_of(&mara.told);
    assert!(
        wide.contains(&format!("world:area/{}", t.ada.0))
            && wide.contains(&format!("world:area/{}", t.bo.0)),
        "{wide}"
    );
    assert!(wide.contains("world:role/ceo"), "{wide}");
}

#[test]
fn a_turn_on_the_mac_has_a_longer_map_and_a_bigger_briefing_than_one_on_the_nas() {
    use crate::domain::map::{MAP_CHARS_LOCAL, MAP_CHARS_MAC};
    use crate::domain::situations::{LOCAL_BUDGET, MAC_BUDGET};
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    for n in 0..14 {
        hq.hire(&format!("Lead{n}"), Role::Lead, 25, 2, 5).unwrap();
    }
    hq.migrate_map().unwrap();
    // The node says the Mac is free, so Linus's work runs on it: a large context, its own limits.
    hq.tell_mac(true, "ready");
    let (linus, _) = working(&hq, t.goal, t.linus, "Fix the proxy");
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
    // Many areas, so the map is wide; the node has not said the Mac is free, so Linus works on
    // the NAS's model, whose context is small: its map is short and its briefing small.
    for n in 0..14 {
        hq.hire(&format!("Lead{n}"), Role::Lead, 25, 2, 5).unwrap();
    }
    hq.migrate_map().unwrap();
    let (linus, _) = working(&hq, t.goal, t.linus, "Fix the proxy");
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
    // What matters most stays: his role, his area, the house.
    for kept in [
        "world:role/developer".to_owned(),
        format!("world:area/{}", t.ada.0),
        "world:house".to_owned(),
    ] {
        assert!(map.contains(&format!("- {kept} ")), "{kept} in\n{map}");
    }
    // What was left off is named, one by one.
    let left = map.lines().find(|l| l.starts_with("Left off")).unwrap();
    assert!(left.contains("world:area/"), "{left}");
}

#[test]
fn a_leads_map_points_at_its_areas_sections_and_the_house_canon() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    hq.write_as_owner("house", "disk", "The disk", "The NAS has 16 GB.")
        .unwrap();
    hq.promote("world:house#disk", "").unwrap();
    let page = area(t.ada);
    let (ada, task) = working(&hq, t.goal, t.ada, "Look at backups");
    use_tool(&hq, &ada, "write_world", &write(&page, "Backups", task)).unwrap();
    end(&hq, &clock, &ada);
    let (again, _) = working(&hq, t.goal, t.ada, "Look at the router");
    let map = map_of(&again.told);
    assert!(
        map.contains(&format!("world:{page}#backups")),
        "a section of her own area is on the map\n{map}"
    );
    assert!(
        again.told.contains("The NAS has 16 GB."),
        "the house canon is told"
    );
}

#[test]
fn a_role_page_starts_from_what_the_role_is_for() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    let (linus, _) = working(&hq, t.goal, t.linus, "Fix the proxy");
    let said = use_tool(
        &hq,
        &linus,
        "read_world",
        &json!({ "address": "world:role/developer" }),
    )
    .unwrap();
    assert!(
        said.contains("Kept by the owner.") && said.contains("How a developer works here"),
        "{said}"
    );
    // The default is HQ's to show, never stored: it cannot go stale or pass for a confirmation.
    assert!(
        said.contains("Endora's default") && said.contains("read_doc docs/standards.md"),
        "{said}"
    );
    assert!(hq.store().sections().unwrap().is_empty());
    // Once the owner writes the page, the default steps aside.
    hq.write_as_owner(
        "role/developer",
        "flow",
        "How we work",
        "Branch, test, push.",
    )
    .unwrap();
    let said = use_tool(
        &hq,
        &linus,
        "read_world",
        &json!({ "address": "world:role/developer" }),
    )
    .unwrap();
    assert!(
        said.contains("Branch, test, push.") && !said.contains("Endora's default"),
        "{said}"
    );
}

#[test]
fn a_gap_is_one_item_for_the_members_lead_and_never_the_owners_desk() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    let cy = hq.hire("Cy", Role::Developer, 40, 2, 6).unwrap();
    hq.set_reports_to(cy, t.ada).unwrap();
    hq.migrate_map().unwrap();
    let (linus, _) = working(&hq, t.goal, t.linus, "Fix the proxy");
    // A report says what was looked for and where.
    let bare = use_tool(
        &hq,
        &linus,
        "report_gap",
        &json!({ "what": "the desk notice" }),
    )
    .unwrap_err();
    assert!(bare.contains("where you looked"), "{bare}");
    let said = use_tool(
        &hq,
        &linus,
        "report_gap",
        &json!({ "what": "How the desk notice works", "where_looked": "world:lab, world:house" }),
    )
    .unwrap();
    assert!(said.contains("Ada"), "it goes to the lead: {said}");
    end(&hq, &clock, &linus);
    let (cy_turn, _) = working(&hq, t.goal, cy, "Fix the VPN");
    use_tool(
        &hq,
        &cy_turn,
        "report_gap",
        &json!({ "what": "how the desk  notice works?", "where_looked": "world:team" }),
    )
    .unwrap();
    let gaps = hq.store().gaps().unwrap();
    assert_eq!(gaps.len(), 1, "the same gap is one item");
    assert_eq!(gaps[0].count, 2);
    assert_eq!(gaps[0].steward, Some(t.ada));
    assert!(
        hq.store().desk().unwrap().is_empty(),
        "never the owner's desk"
    );
    end(&hq, &clock, &cy_turn);
    // The lead's next briefing carries it; another lead's does not.
    let (bo, _) = working(&hq, t.goal, t.bo, "Look at the router");
    assert!(!bo.told.contains("desk notice"), "{}", bo.told);
    end(&hq, &clock, &bo);
    let (ada, _) = working(&hq, t.goal, t.ada, "Look at backups");
    assert!(
        ada.told.contains("Gaps reported to you") && ada.told.contains("How the desk notice works"),
        "{}",
        ada.told
    );
    // Closing it takes it out of the next briefing.
    let id = gaps[0].id;
    let refused = use_tool(&hq, &ada, "close_gap", &json!({ "id": id })).unwrap_err();
    assert!(refused.contains("how"), "{refused}");
    use_tool(
        &hq,
        &ada,
        "close_gap",
        &json!({ "id": id, "how": "written at world:area/x#desk" }),
    )
    .unwrap();
    end(&hq, &clock, &ada);
    let (again, _) = working(&hq, t.goal, t.ada, "Look at the NAS");
    assert!(
        !again.told.contains("Gaps reported to you"),
        "{}",
        again.told
    );
    // A lead's own gap goes to the CEO.
    use_tool(
        &hq,
        &again,
        "report_gap",
        &json!({ "what": "the budget rules", "where_looked": "world:house" }),
    )
    .unwrap();
    assert_eq!(hq.store().gaps().unwrap().last().unwrap().steward, None);
    task_for(&hq, t.goal, t.ceo, "Review");
    let mara = turn(&hq, t.ceo);
    assert!(mara.told.contains("the budget rules"), "{}", mara.told);
}

#[test]
fn a_cited_address_that_does_not_resolve_is_a_broken_road_found_by_code() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    let page = area(t.ada);
    let (ada, ada_task) = working(&hq, t.goal, t.ada, "Look at backups");
    use_tool(
        &hq,
        &ada,
        "write_world",
        &write(&page, "Restores", ada_task),
    )
    .unwrap();
    use_tool(&hq, &ada, "write_world", &write(&page, "Spare", ada_task)).unwrap();
    use_tool(
        &hq,
        &ada,
        "write_world",
        &write(&page, "Restoring", ada_task),
    )
    .unwrap();
    task_for(&hq, t.goal, t.ceo, "Review");
    let mara = turn(&hq, t.ceo);
    let cites = json!({ "heading": "Disks", "headline": format!("Disks: see world:{page}#restores"),
                        "body": "Also world:area/9999 and story:no-such-story.", "stage": "observation",
                        "evidence": [format!("task {}", ada_task.0)] });
    use_tool(&hq, &mara, "write_world", &cites).unwrap();
    let roads = hq.broken_roads().unwrap();
    let wrong: Vec<&str> = roads.iter().map(|r| r.to.as_str()).collect();
    assert_eq!(
        wrong,
        ["world:area/9999", "story:no-such-story"],
        "{roads:?}"
    );
    assert!(roads.iter().all(|r| r.from == "world:house#disks"));
    // Retiring what is cited breaks the road; the steward of the citing page is told.
    use_tool(
        &hq,
        &ada,
        "retire_world",
        &json!({ "address": format!("world:{page}#restores"), "why": "restores moved" }),
    )
    .unwrap();
    let roads = hq.broken_roads().unwrap();
    let retired = roads
        .iter()
        .find(|r| r.to == format!("world:{page}#restores"))
        .unwrap();
    assert!(retired.why.contains("retired"), "{retired:?}");
    // A superseded section is not broken: it says where to go.
    use_tool(
        &hq,
        &ada,
        "retire_world",
        &json!({ "address": format!("world:{page}#spare"), "why": "merged", "superseded_by": format!("world:{page}#restoring") }),
    )
    .unwrap();
    assert!(
        hq.broken_roads()
            .unwrap()
            .iter()
            .all(|r| r.from != format!("world:{page}#spare")),
        "a superseded section points at what replaces it"
    );
    end(&hq, &clock, &mara);
    task_for(&hq, t.goal, t.ceo, "Review again");
    let mara = turn(&hq, t.ceo);
    assert!(
        mara.told.contains("Broken roads") && mara.told.contains(&format!("world:{page}#restores")),
        "{}",
        mara.told
    );
    // Gaps and roads are kept apart.
    assert!(hq.store().gaps().unwrap().is_empty());
    // Mending the cite clears the road.
    let mended = json!({ "anchor": "disks", "heading": "Disks", "headline": "Disks: nothing cited", "body": "Plain.",
                         "stage": "observation", "evidence": [format!("task {}", ada_task.0)] });
    use_tool(&hq, &mara, "write_world", &mended).unwrap();
    assert!(
        hq.broken_roads()
            .unwrap()
            .iter()
            .all(|r| r.from != "world:house#disks")
    );
    // And a section shows its routes out when read.
    let read = use_tool(
        &hq,
        &ada,
        "read_world",
        &json!({ "address": format!("world:{page}#spare") }),
    )
    .unwrap();
    assert!(read.contains("Superseded by"), "{read}");
}

fn propose(page: &str, heading: &str, task: TaskId) -> Value {
    json!({ "page": page, "heading": heading, "headline": format!("{heading}: how it goes"),
            "body": "Nightly, at 03:00.", "evidence": [format!("task {}", task.0)] })
}

/// A claim the owner retracted is not proposed to a page again (ADR 0124), in the proposer's own
/// words around it; the refusal says when it was retracted, and nothing is proposed.
#[test]
fn a_retracted_claim_is_not_proposed_to_a_page_again() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    let page = area(t.ada);
    let (linus, task) = working(&hq, t.goal, t.linus, "Fix the proxy");
    use_tool(
        &hq,
        &linus,
        "note",
        &json!({ "text": "Backups Backups: how it goes Nightly, at 03:00.",
                 "evidence": [format!("task {}", task.0)] }),
    )
    .unwrap();
    let wrong = hq.notebook(t.linus).unwrap()[0].id;
    hq.retract(&format!("note:{wrong}")).unwrap();
    let refused = use_tool(
        &hq,
        &linus,
        "propose_world",
        &propose(&page, "Backups", task),
    )
    .unwrap_err();
    assert!(
        refused.contains("this says again what was retracted on"),
        "{refused}"
    );
    assert!(hq.store().proposals().unwrap().is_empty());
}

#[test]
fn a_member_proposes_to_a_page_and_its_steward_accepts_and_the_proposer_is_told() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    let page = area(t.ada);
    let (linus, task) = working(&hq, t.goal, t.linus, "Fix the proxy");
    // The developer cannot write the page; it proposes.
    let proposed = use_tool(
        &hq,
        &linus,
        "propose_world",
        &propose(&page, "Backups", task),
    )
    .unwrap();
    assert!(proposed.contains("Ada decides"), "{proposed}");
    assert!(
        hq.store().sections().unwrap().is_empty(),
        "nothing is written by a proposal"
    );
    end(&hq, &clock, &linus);
    // The steward's next briefing carries it; nobody else's does.
    task_for(&hq, t.goal, t.bo, "Look at the router");
    let bo = turn(&hq, t.bo);
    assert!(!bo.told.contains("Backups: how it goes"), "{}", bo.told);
    end(&hq, &clock, &bo);
    task_for(&hq, t.goal, t.ada, "Look at backups");
    let ada = turn(&hq, t.ada);
    assert!(
        ada.told.contains("decide_proposal")
            && ada.told.contains("Linus proposes")
            && ada.told.contains("Backups: how it goes"),
        "{}",
        ada.told
    );
    // Nobody but the steward decides it.
    let id = hq.store().proposals().unwrap()[0].id;
    let decide = json!({ "id": id, "accept": true });
    let refused = use_tool(
        &hq,
        &bo_turn(&hq, &clock, t.bo, t.goal),
        "decide_proposal",
        &decide,
    )
    .unwrap_err();
    assert!(refused.contains("kept by Ada"), "{refused}");
    let accepted = use_tool(&hq, &ada, "decide_proposal", &decide).unwrap();
    assert!(
        accepted.contains(&format!("world:{page}#backups")),
        "{accepted}"
    );
    let section = hq.store().sections().unwrap().remove(0);
    assert_eq!((section.written_by.as_str(), section.revision), ("Ada", 1));
    assert!(
        hq.store().timeline(&page).unwrap()[0]
            .line
            .contains("Linus's proposal")
    );
    assert!(
        use_tool(&hq, &ada, "decide_proposal", &decide)
            .unwrap_err()
            .contains("decided already")
    );
    end(&hq, &clock, &ada);
    // The proposer hears how it went once, in its next briefing.
    task_for(&hq, t.goal, t.linus, "Fix the DNS");
    let again = turn(&hq, t.linus);
    assert!(again.told.contains("accepted by Ada"), "{}", again.told);
    end(&hq, &clock, &again);
    task_for(&hq, t.goal, t.linus, "Fix the VPN");
    let third = turn(&hq, t.linus);
    assert!(
        !third.told.contains("accepted by Ada"),
        "told once: {}",
        third.told
    );
}

/// A turn of `member`'s, on a fresh task of its own.
fn bo_turn(hq: &Hq<'_>, clock: &Hands, member: MemberId, goal: GoalId) -> RunContext {
    clock.0.fetch_add(60_000, Ordering::SeqCst);
    working(hq, goal, member, "Another look").0
}

#[test]
fn a_decline_needs_a_reason_and_the_proposer_is_told_which() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    let page = area(t.ada);
    let (linus, task) = working(&hq, t.goal, t.linus, "Fix the proxy");
    use_tool(
        &hq,
        &linus,
        "propose_world",
        &propose(&page, "Backups", task),
    )
    .unwrap();
    end(&hq, &clock, &linus);
    let (ada, _) = working(&hq, t.goal, t.ada, "Look at backups");
    let id = hq.store().proposals().unwrap()[0].id;
    let silent = use_tool(
        &hq,
        &ada,
        "decide_proposal",
        &json!({ "id": id, "accept": false }),
    )
    .unwrap_err();
    assert!(silent.contains("say why"), "{silent}");
    use_tool(
        &hq,
        &ada,
        "decide_proposal",
        &json!({ "id": id, "accept": false, "reason": "the backups move next week" }),
    )
    .unwrap();
    assert!(hq.store().sections().unwrap().is_empty());
    end(&hq, &clock, &ada);
    let (again, _) = working(&hq, t.goal, t.linus, "Fix the DNS");
    assert!(
        again
            .told
            .contains("declined by Ada: the backups move next week"),
        "{}",
        again.told
    );
}

#[test]
fn a_proposal_is_for_a_page_someone_else_keeps_and_rests_on_the_record() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    let (ada, ada_task) = working(&hq, t.goal, t.ada, "Look at backups");
    let own = use_tool(
        &hq,
        &ada,
        "propose_world",
        &propose(&area(t.ada), "Backups", ada_task),
    )
    .unwrap_err();
    assert!(
        own.contains("is yours") && own.contains("write_world"),
        "{own}"
    );
    let (linus, task) = working(&hq, t.goal, t.linus, "Fix the proxy");
    let derived =
        use_tool(&hq, &linus, "propose_world", &propose("lab", "Disk", task)).unwrap_err();
    assert!(derived.contains("derived"), "{derived}");
    let nowhere = use_tool(
        &hq,
        &linus,
        "propose_world",
        &propose("area/9999", "Disk", task),
    )
    .unwrap_err();
    assert!(nowhere.contains("no page"), "{nowhere}");
    let mut bare = propose(&area(t.ada), "Disk", task);
    bare["evidence"] = json!([]);
    assert!(
        use_tool(&hq, &linus, "propose_world", &bare)
            .unwrap_err()
            .contains("evidence")
    );
    // The same proposal twice is one; six different ones are too many.
    let first = use_tool(
        &hq,
        &linus,
        "propose_world",
        &propose(&area(t.ada), "Disk", task),
    )
    .unwrap();
    let again = use_tool(
        &hq,
        &linus,
        "propose_world",
        &propose(&area(t.ada), "Disk", task),
    )
    .unwrap();
    assert_eq!(first, again);
    assert_eq!(hq.store().proposals().unwrap().len(), 1);
    for n in 2..=5 {
        use_tool(
            &hq,
            &linus,
            "propose_world",
            &propose(&area(t.ada), &format!("Topic {n}"), task),
        )
        .unwrap();
    }
    let flood = use_tool(
        &hq,
        &linus,
        "propose_world",
        &propose(&area(t.ada), "Topic 6", task),
    )
    .unwrap_err();
    assert!(flood.contains("5 proposals open"), "{flood}");
}

#[test]
fn a_roles_page_is_decided_by_the_owner_and_canon_is_not_a_stewards_to_replace() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    let (linus, task) = working(&hq, t.goal, t.linus, "Fix the proxy");
    use_tool(
        &hq,
        &linus,
        "propose_world",
        &propose("role/developer", "Branches", task),
    )
    .unwrap();
    let id = hq.store().proposals().unwrap()[0].id;
    let (ada, _) = working(&hq, t.goal, t.ada, "Look at backups");
    let refused = use_tool(
        &hq,
        &ada,
        "decide_proposal",
        &json!({ "id": id, "accept": true }),
    )
    .unwrap_err();
    assert!(refused.contains("kept by the owner"), "{refused}");
    assert!(
        !ada.told.contains("Branches"),
        "only the owner decides it, so no lead's briefing carries it"
    );
    let address = hq.decide_proposal_as_owner(id, true, "").unwrap();
    assert_eq!(address.as_deref(), Some("world:role/developer#branches"));
    let section = hq.store().sections().unwrap().remove(0);
    assert_eq!(section.written_by, "the owner");
    // A section the owner made canon is not replaced by a steward's acceptance.
    let page = area(t.ada);
    hq.write_as_owner(&page, "backups", "Backups", "Nightly.")
        .unwrap();
    hq.promote(&format!("world:{page}#backups"), "").unwrap();
    let mut revise = propose(&page, "Backups", task);
    revise["anchor"] = json!("backups");
    use_tool(&hq, &linus, "propose_world", &revise).unwrap();
    let id = hq.store().proposals().unwrap().last().unwrap().id;
    let refused = use_tool(
        &hq,
        &ada,
        "decide_proposal",
        &json!({ "id": id, "accept": true }),
    )
    .unwrap_err();
    assert!(refused.contains("owner"), "{refused}");
    hq.decide_proposal_as_owner(id, false, "not now").unwrap();
    assert_eq!(
        hq.store().proposals().unwrap().last().unwrap().why,
        "not now"
    );
}

#[test]
fn the_world_card_shows_every_page_its_steward_and_what_waits_for_someone() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.migrate_map().unwrap();
    let page = area(t.ada);
    let (ada, ada_task) = working(&hq, t.goal, t.ada, "Look at backups");
    let cites = json!({ "page": page, "heading": "Restores", "headline": "Restores: see world:area/9999",
                        "stage": "observation", "evidence": [format!("task {}", ada_task.0)] });
    use_tool(&hq, &ada, "write_world", &cites).unwrap();
    let (linus, task) = working(&hq, t.goal, t.linus, "Fix the proxy");
    use_tool(
        &hq,
        &linus,
        "propose_world",
        &propose(&page, "Backups", task),
    )
    .unwrap();
    use_tool(
        &hq,
        &linus,
        "report_gap",
        &json!({ "what": "the desk notice", "where_looked": "world:lab" }),
    )
    .unwrap();
    let shown = hq.world_shown().unwrap();
    let map = &shown["map"];
    let find = |slug: &str| {
        map["pages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["slug"] == slug)
            .unwrap_or_else(|| panic!("no page {slug} in {map}"))
            .clone()
    };
    let ada_page = find(&page);
    assert_eq!(
        (
            ada_page["title"].as_str(),
            ada_page["steward"].as_str(),
            ada_page["steward_kind"].as_str(),
            ada_page["sections"].as_u64(),
            ada_page["proposals"].as_u64()
        ),
        (
            Some("Ada's area"),
            Some("Ada"),
            Some("lead"),
            Some(1),
            Some(1)
        )
    );
    assert_eq!(find("role/developer")["steward_kind"], "owner");
    assert_eq!(find("lab")["steward_kind"], "hq");
    assert_eq!(find("house")["steward"], "Mara, the CEO");
    assert!(map["pages"].as_array().unwrap().len() >= 10, "{map}");
    assert_eq!(map["proposals"][0]["by"], "Linus");
    assert_eq!(map["proposals"][0]["status"], "open");
    assert_eq!(map["gaps"][0]["to"], "Ada");
    assert_eq!(map["gaps"][0]["open"], true);
    assert_eq!(map["roads"][0]["to"], "world:area/9999");
    assert!(map["quiz"]["pages"].is_array());
    // The Markdown export carries each page under its own heading, with its steward.
    let md = hq.world_markdown().unwrap();
    let heading = format!("## Ada's area (world:{page})\n\nKept by Ada.");
    assert!(md.contains(&heading), "{md}");
    assert!(md.contains("Restores"), "{md}");
    assert_eq!(
        md.matches("### Timeline (newest first)").count(),
        2,
        "the house's and Ada's, not every page's: {md}"
    );
}
