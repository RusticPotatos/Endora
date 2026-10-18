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

fn propose(page: &str, heading: &str, task: TaskId) -> Value {
    json!({ "page": page, "heading": heading, "headline": format!("{heading}: how it goes"),
            "body": "Nightly, at 03:00.", "evidence": [format!("task {}", task.0)] })
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
