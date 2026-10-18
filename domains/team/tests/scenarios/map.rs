//! The steps of `features/the-map.feature` (ADR 0126): a page for every area, project and role,
//! each with a steward worked out from the data; proposals the steward decides; the map every
//! briefing opens with; gaps and broken roads; and the nightly quiz on the free model.
//!
//! A page is named in a scenario by who or what it is about (`area/Ada`, `project/Upkeep`);
//! the step turns that into the page's address.

use cucumber::{given, then, when};
use endora_team::application::map_ports::MapStore;
use endora_team::application::world_ports::WorldStore;
use endora_team::domain::world::Stage;
use serde_json::json;

use super::Office;

impl Office {
    /// A page or address as a scenario names it, as HQ addresses it: `area/Ada` is `area/2`.
    fn addressed(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (name, id) in &self.members {
            out = out.replace(&format!("area/{name}"), &format!("area/{}", id.0));
        }
        for (title, id) in &self.projects {
            out = out.replace(&format!("project/{title}"), &format!("project/{}", id.0));
        }
        out
    }

    /// The reference to a task by its title, as a member cites it.
    fn cites(&self, title: &str) -> String {
        format!("task {}", self.task(title).0)
    }
}

#[given("HQ has made the map's pages")]
fn pages_made(w: &mut Office) {
    w.hq.migrate_map().expect("the map's pages are made");
}

#[when(regex = r#"^(\w+) writes "(.+)" on "(.+)" citing the task "(.+)"$"#)]
fn writes_on_page(w: &mut Office, name: String, heading: String, page: String, task: String) {
    let page = w.addressed(&page);
    let evidence = w.cites(&task);
    w.use_tool(
        &name,
        "write_world",
        &json!({ "page": page, "heading": heading, "headline": format!("{heading}: how it goes"),
                 "body": "Nightly, at 03:00.", "stage": "observation", "evidence": [evidence] }),
    );
}

#[when(regex = r#"^(\w+) proposes "(.+)" to "(.+)" citing the task "(.+)"$"#)]
fn proposes_to_page(w: &mut Office, name: String, heading: String, page: String, task: String) {
    let page = w.addressed(&page);
    let evidence = w.cites(&task);
    let done = w.use_tool(
        &name,
        "propose_world",
        &json!({ "page": page, "heading": heading, "headline": format!("{heading}: how it goes"),
                 "body": "Nightly, at 03:00.", "evidence": [evidence] }),
    );
    assert!(done.is_some(), "refused: {:?}", w.refused);
}

fn open_proposal(w: &Office) -> i64 {
    w.hq.store()
        .proposals()
        .expect("the store answers")
        .into_iter()
        .rfind(|p| p.status == endora_team::domain::map::ProposalStatus::Open)
        .expect("a proposal is waiting")
        .id
}

#[when(regex = r#"^(\w+) declines the proposal because "(.+)"$"#)]
fn declines(w: &mut Office, name: String, reason: String) {
    let id = open_proposal(w);
    let done = w.use_tool(
        &name,
        "decide_proposal",
        &json!({ "id": id, "accept": false, "reason": reason }),
    );
    assert!(done.is_some(), "refused: {:?}", w.refused);
}

#[when(regex = r"^(\w+) accepts the proposal$")]
fn accepts(w: &mut Office, name: String) {
    let id = open_proposal(w);
    let done = w.use_tool(
        &name,
        "decide_proposal",
        &json!({ "id": id, "accept": true }),
    );
    assert!(done.is_some(), "refused: {:?}", w.refused);
}

#[given(expr = "the owner makes {string} canon")]
#[when(expr = "the owner makes {string} canon")]
fn owner_makes_canon(w: &mut Office, address: String) {
    let address = w.addressed(&address);
    w.hq.promote(&address, "")
        .expect("the owner makes it canon");
}

#[given(expr = "the owner writes {string} on {string} saying {string}")]
#[when(expr = "the owner writes {string} on {string} saying {string}")]
fn owner_writes(w: &mut Office, heading: String, page: String, text: String) {
    let page = w.addressed(&page);
    w.hq.write_as_owner(&page, "", &heading, &text)
        .expect("the owner writes the section");
}

#[then(regex = r#"^"(.+)" stands as (?:an? )?(canon|insight|observation)$"#)]
fn stands_as(w: &mut Office, address: String, stage: String) {
    let address = w.addressed(&address);
    let section =
        w.hq.store()
            .sections()
            .expect("the store answers")
            .into_iter()
            .find(|s| s.address() == address)
            .unwrap_or_else(|| panic!("nothing at {address}"));
    assert_eq!(section.stage, Stage::from_name(&stage).expect("a stage"));
}

#[then(regex = r#"^(\w+) reading "(.+)" sees "(.+)"$"#)]
fn reading_sees(w: &mut Office, name: String, address: String, said: String) {
    let address = w.addressed(&address);
    let read = w.use_tool(&name, "read_world", &json!({ "address": address }));
    let read = read.unwrap_or_else(|| panic!("refused: {:?}", w.refused));
    assert!(read.contains(&said), "{read}");
}

#[then(expr = "no section is on {string}")]
fn no_section(w: &mut Office, page: String) {
    let page = w.addressed(&page);
    let sections = w.hq.store().sections().expect("the store answers");
    assert!(
        sections.iter().all(|s| s.page != page),
        "sections on {page}: {sections:?}"
    );
    let _ = w.hq.store().world_pages();
}

#[then(regex = r#"^(\w+)'s map names "(.+)"$"#)]
fn map_names(w: &mut Office, name: String, address: String) {
    let address = w.addressed(&address);
    let told = &w
        .turns
        .get(&name)
        .unwrap_or_else(|| panic!("{name}'s turn has not started"))
        .told;
    let map = endora_team::domain::map::map_block(told)
        .unwrap_or_else(|| panic!("{name}'s briefing has no map:\n{told}"));
    assert!(map.contains(&format!("- {address} ")), "{map}");
}

#[when(regex = r#"^(\w+) reports a gap "(.+)", looked in "(.+)"$"#)]
fn reports_gap(w: &mut Office, name: String, what: String, looked: String) {
    let done = w.use_tool(
        &name,
        "report_gap",
        &json!({ "what": what, "where_looked": looked }),
    );
    assert!(done.is_some(), "refused: {:?}", w.refused);
}

#[when(regex = r#"^(\w+) retires "(.+)" because "(.+)"$"#)]
fn retires(w: &mut Office, name: String, address: String, why: String) {
    let address = w.addressed(&address);
    let done = w.use_tool(
        &name,
        "retire_world",
        &json!({ "address": address, "why": why }),
    );
    assert!(done.is_some(), "refused: {:?}", w.refused);
}

#[when(regex = r#"^(\w+) writes "(.+)" on the house page, saying "(.+)", citing the task "(.+)"$"#)]
fn writes_on_house(w: &mut Office, name: String, heading: String, headline: String, task: String) {
    let headline = w.addressed(&headline);
    let evidence = w.cites(&task);
    let done = w.use_tool(
        &name,
        "write_world",
        &json!({ "heading": heading, "headline": headline, "stage": "observation",
                 "evidence": [evidence] }),
    );
    assert!(done.is_some(), "refused: {:?}", w.refused);
}

#[then("no road is broken")]
fn no_road_broken(w: &mut Office) {
    let roads = w.hq.broken_roads().expect("the store answers");
    assert!(roads.is_empty(), "{roads:?}");
}

#[then(expr = "the road from {string} to {string} is broken")]
fn road_broken(w: &mut Office, from: String, to: String) {
    let (from, to) = (w.addressed(&from), w.addressed(&to));
    let roads = w.hq.broken_roads().expect("the store answers");
    assert!(
        roads.iter().any(|r| r.from == from && r.to == to),
        "{roads:?}"
    );
}
