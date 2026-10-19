//! The steps of `features/the-map.feature` (ADR 0126): a page for every area, project and role,
//! each with a steward worked out from the data; proposals the steward decides; the map every
//! briefing opens with; gaps and broken roads; and the nightly quiz on the free model.
//!
//! A page is named in a scenario by who or what it is about (`area/Ada`, `project/Upkeep`);
//! the step turns that into the page's address.

use cucumber::{given, then, when};
use endora_team::application::TeamStore;
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

// --- The nightly quiz ----------------------------------------------------------------------

impl Office {
    /// The name the scenario gave a member.
    fn name_of(&self, id: endora_team::domain::MemberId) -> String {
        self.members
            .iter()
            .find(|(_, m)| **m == id)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| panic!("nobody has the id {id:?}"))
    }

    /// The newest quiz, as kept.
    fn last_quiz(&self) -> endora_team::domain::quiz::Quiz {
        self.hq
            .store()
            .quizzes()
            .expect("the store answers")
            .pop()
            .expect("a quiz was kept")
    }

    /// The quizzed member answers the quiz in its downtime turn, from `answer` per question.
    fn answers_the_quiz(
        &mut self,
        answer: impl Fn(&endora_team::domain::quiz::Question) -> String,
    ) {
        let quiz = self.last_quiz();
        let name = self.name_of(quiz.member.expect("asked of a member"));
        let answers: Vec<String> = quiz.questions.iter().map(answer).collect();
        let done = self.use_tool(&name, "answer_quiz", &json!({ "answers": answers }));
        assert!(done.is_some(), "refused: {:?}", self.refused);
    }
}

#[given("the nightly quiz is on")]
fn quiz_on(w: &mut Office) {
    w.hq.set_quiz(true);
}

#[given(regex = r"^it is (\d+):(\d\d) in the morning$")]
#[when(regex = r"^it is (\d+):(\d\d) in the morning$")]
fn it_is_morning(w: &mut Office, hour: i64, minute: i64) {
    let day = 24 * 60 * 60 * 1000;
    let now = w.clock.0.load(std::sync::atomic::Ordering::SeqCst);
    let today = now - now.rem_euclid(day) + hour * 60 * 60 * 1000 + minute * 60 * 1000;
    let next = if today > now { today } else { today + day };
    w.clock.0.store(next, std::sync::atomic::Ordering::SeqCst);
}

#[when("HQ looks at the night")]
fn looks_at_the_night(w: &mut Office) {
    w.tick();
}

#[then("a quiz is asked of a member, in a turn on the house's model that costs nothing")]
fn quiz_asked(w: &mut Office) {
    let quiz = w.last_quiz();
    assert_eq!(quiz.status, endora_team::domain::quiz::Status::Asked);
    let asked = quiz.member.expect("asked of a member");
    // The quiz is a downtime turn, which is work on the house's own model (ADR 0131).
    let turn =
        w.hq.store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .rfind(|r| r.member == asked)
            .expect("the quizzed member has a turn");
    assert_eq!(turn.brain, Some(endora_team::domain::Brain::Local));
    assert_eq!((turn.reserved, turn.spent), (0, 0));
}

#[then("no quiz is asked")]
fn no_quiz_asked(w: &mut Office) {
    assert!(
        w.hq.store()
            .quizzes()
            .expect("the store answers")
            .is_empty()
    );
}

#[when("that member answers the quiz from its map")]
fn answers_from_map(w: &mut Office) {
    w.answers_the_quiz(|q| q.expects[0].clone());
}

#[when("the night's quiz is answered wrongly three nights running")]
fn answered_wrongly_three_nights(w: &mut Office) {
    for _ in 0..3 {
        w.tick();
        w.answers_the_quiz(|_| "I do not know".to_owned());
        let name = w.name_of(w.last_quiz().member.expect("asked of a member"));
        let context = w.turns.get(&name).expect("its turn").clone();
        w.hq.runner().finish(context.run, true);
        w.tick();
        w.clock
            .0
            .fetch_add(24 * 60 * 60 * 1000, std::sync::atomic::Ordering::SeqCst);
    }
}

#[then("the World shows last night's quiz with every question right")]
fn world_shows_all_right(w: &mut Office) {
    let shown = w.hq.quiz_shown().expect("the store answers");
    let last = &shown["nights"][0];
    assert_eq!(last["status"], "answered", "{shown}");
    assert!(last["asked"].as_u64() > Some(0), "{shown}");
    assert_eq!(last["passed"], last["asked"], "{shown}");
}

#[then(expr = "the World shows last night's quiz as skipped because {string}")]
fn world_shows_skipped(w: &mut Office, why: String) {
    let shown = w.hq.quiz_shown().expect("the store answers");
    let last = &shown["nights"][0];
    assert_eq!(last["status"], "skipped", "{shown}");
    assert!(
        last["why"].as_str().unwrap_or_default().contains(&why),
        "{shown}"
    );
}

#[then("the World shows no page as failing the quiz")]
fn world_shows_none_failing(w: &mut Office) {
    let shown = w.hq.quiz_shown().expect("the store answers");
    assert!(
        shown["pages"]
            .as_array()
            .expect("pages")
            .iter()
            .all(|p| p["flagged"] == false),
        "{shown}"
    );
}

#[then(expr = "the World shows {string} as failing the quiz")]
fn world_shows_failing(w: &mut Office, page: String) {
    let page = w.addressed(&page);
    let shown = w.hq.quiz_shown().expect("the store answers");
    let found = shown["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .find(|p| p["slug"] == page.as_str())
        .unwrap_or_else(|| panic!("no score for {page}: {shown}"));
    assert_eq!(found["flagged"], true, "{shown}");
}
