//! The steps of `features/the-world.feature` (ADR 0122): the house log and the lessons moved
//! into the team's world, canon the owner's alone, trust read from dated evidence, and each
//! turn's situation, budget and handover.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use endora_team::application::TeamStore;
use endora_team::application::engine::Called;
use endora_team::application::world_ports::WorldStore;
use endora_team::domain::world::Stage;
use serde_json::json;

use super::Office;

impl Office {
    /// A reading the node witnessed in `member`'s running turn: its receipt, as cited.
    fn reads_the_lab(&mut self, member: &str) -> String {
        let context = self.turn_of(member);
        let id = self
            .hq
            .record_receipt(
                &context.token,
                &Called {
                    tool: "homelab_status".to_owned(),
                    args: String::new(),
                    excerpt: "backups: last ran 03:00".to_owned(),
                    ok: true,
                    act: false,
                    confidential: false,
                },
            )
            .expect("the node's receipt is kept");
        format!("r{id}")
    }

    /// What `member`'s last turn was told.
    fn told(&self, member: &str) -> String {
        self.turns
            .get(member)
            .unwrap_or_else(|| panic!("{member}'s turn has not started"))
            .told
            .clone()
    }

    fn briefed(&self, member: &str) -> endora_team::domain::situations::Briefed {
        let run = self
            .turns
            .get(member)
            .unwrap_or_else(|| panic!("{member}'s turn has not started"))
            .run;
        self.hq
            .briefed(run)
            .expect("the store answers")
            .expect("the turn's briefing is recorded")
    }
}

// --- Moving the old record in ----------------------------------------------------------------

#[given(regex = r#"^the goal's notes hold the lesson "(.+)" noted by (\w+) on (\d+ \w+)$"#)]
fn lesson_in_notes(w: &mut Office, lesson: String, who: String, on: String) {
    let goal = w.goal();
    let notes = w.hq.goal_note(goal).expect("the store answers");
    let line = format!("- {lesson} ({who}, {on})");
    let notes = if notes.is_empty() {
        line
    } else {
        format!("{notes}\n{line}")
    };
    w.hq.set_goal_note(goal, &notes)
        .expect("the notes take a lesson");
}

#[when("HQ starts with the world")]
fn starts_with_the_world(w: &mut Office) {
    assert!(w.hq.migrate_world().expect("the move is made"), "moved now");
}

#[then(regex = r#"^the house page's timeline says "(.+)"$"#)]
fn timeline_says(w: &mut Office, said: String) {
    let timeline = w.hq.store().timeline("house").expect("the store answers");
    assert!(
        timeline.iter().any(|l| l.line.contains(&said)),
        "the timeline: {timeline:?}"
    );
}

// --- Stories ----------------------------------------------------------------------------------

fn story(w: &Office, handle: &str) -> endora_team::domain::world::Story {
    w.hq.store()
        .stories()
        .expect("the store answers")
        .into_iter()
        .find(|s| s.handle == handle)
        .unwrap_or_else(|| panic!("no story is called {handle}"))
}

#[then(expr = "the story {string} is marked to retell")]
fn to_retell(w: &mut Office, handle: String) {
    assert!(story(w, &handle).to_retell);
}

#[then(expr = "the story {string} is no longer marked to retell")]
fn retold(w: &mut Office, handle: String) {
    assert!(!story(w, &handle).to_retell);
}

#[when(
    expr = "{word} retells {string}: when {string}, tempting {string}, instead {string}, because {string}"
)]
fn retells(
    w: &mut Office,
    name: String,
    handle: String,
    moment: String,
    reflex: String,
    instead: String,
    because: String,
) {
    let task = w.turn_of(&name).task.expect("a turn on a task");
    let told = w.use_tool(
        &name,
        "tell_story",
        &json!({ "handle": handle, "moment": moment, "reflex": reflex, "instead": instead,
                 "because": because, "evidence": [format!("task {}", task.0)] }),
    );
    assert!(told.is_some(), "refused: {:?}", w.refused);
}

#[then(expr = "the story {string} was cited just now")]
fn cited_now(w: &mut Office, handle: String) {
    let now = w.clock.0.load(Ordering::SeqCst);
    assert_eq!(story(w, &handle).last_used_at, now);
}

#[when(expr = "{word} tells the story {string} as an insight on her reading")]
fn tells_insight(w: &mut Office, name: String, handle: String) {
    let reading = w.reads_the_lab(&name);
    let told = w.use_tool(
        &name,
        "tell_story",
        &json!({ "handle": handle, "moment": "An alarm's task looks fixed after a restart.",
                 "reflex": "Close it at once.", "instead": "Wait an hour with no new alarm.",
                 "because": "It came back two hours later.", "evidence": [reading],
                 "stage": "insight", "warrant": "read on the lab" }),
    );
    assert!(told.is_some(), "refused: {:?}", w.refused);
}

#[when(regex = r"^(\w+) tells (\d+) long stories$")]
fn tells_many(w: &mut Office, name: String, count: usize) {
    let task = w.turn_of(&name).task.expect("a turn on a task");
    for n in 0..count {
        let told = w.use_tool(
            &name,
            "tell_story",
            &json!({ "handle": format!("long-story-{n}"),
                     "moment": "m".repeat(280), "reflex": "r".repeat(280),
                     "instead": "i".repeat(280), "because": "b".repeat(280),
                     "evidence": [format!("task {}", task.0)] }),
        );
        assert!(told.is_some(), "refused: {:?}", w.refused);
    }
}

#[then(expr = "the World shows {string} as {string}")]
fn world_shows(w: &mut Office, handle: String, said: String) {
    let shown = w.hq.world_shown().expect("the store answers");
    let found = shown["stories"]
        .as_array()
        .expect("stories")
        .iter()
        .find(|s| s["handle"] == handle.as_str())
        .cloned()
        .unwrap_or_else(|| panic!("no story {handle}: {shown}"));
    assert_eq!(found["said"], said.as_str());
}

#[when(expr = "{word} confirms {string} on a fresh reading")]
fn confirms(w: &mut Office, name: String, address: String) {
    let reading = w.reads_the_lab(&name);
    let done = w.use_tool(
        &name,
        "verify_world",
        &json!({ "address": address, "evidence": [reading] }),
    );
    assert!(done.is_some(), "refused: {:?}", w.refused);
}

// --- The house page and canon -----------------------------------------------------------------

#[when(expr = "{word} writes on the house page {string}: {string} as an insight on her reading")]
fn writes_house(w: &mut Office, name: String, heading: String, headline: String) {
    let reading = w.reads_the_lab(&name);
    w.use_tool(
        &name,
        "write_world",
        &json!({ "heading": heading, "headline": headline, "stage": "insight",
                 "warrant": "the lab says so", "evidence": [reading] }),
    );
}

#[when(expr = "{word} proposes {string} for canon because {string}")]
fn proposes_canon(w: &mut Office, name: String, address: String, warrant: String) {
    let done = w.use_tool(
        &name,
        "propose_canon",
        &json!({ "address": address, "warrant": warrant }),
    );
    assert!(done.is_some(), "refused: {:?}", w.refused);
}

#[then(expr = "the owner's desk holds a proposal for canon of {string} saying {string}")]
fn desk_canon(w: &mut Office, address: String, said: String) {
    let item = w
        .waiting_on_desk("canon")
        .pop()
        .expect("a proposal for canon on the desk");
    assert!(
        item.text.contains(&address) && item.text.contains(&said),
        "{}",
        item.text
    );
}

#[when("the owner approves the proposal for canon")]
fn approves_canon(w: &mut Office) {
    let item = w
        .waiting_on_desk("canon")
        .pop()
        .expect("a proposal for canon on the desk");
    w.hq.decide(item.id, "approve")
        .expect("the owner makes it canon");
}

#[then(regex = r#"^"(.+)" is (?:an? )?(canon|insight|observation)$"#)]
fn is_at_stage(w: &mut Office, address: String, stage: String) {
    let section =
        w.hq.store()
            .sections()
            .expect("the store answers")
            .into_iter()
            .find(|s| s.address() == address)
            .unwrap_or_else(|| panic!("nothing at {address}"));
    assert_eq!(section.stage, Stage::from_name(&stage).expect("a stage"));
}

#[when(expr = "the owner demotes {string} to {word} because {string}")]
fn demotes(w: &mut Office, address: String, to: String, why: String) {
    let to = Stage::from_name(&to).expect("a stage");
    let done = w.hq.demote(&address, to, &why);
    w.note(done);
    assert_eq!(w.refused, None);
}

#[then(expr = "the feed records the demotion of {string} because {string}")]
fn feed_demotion(w: &mut Office, address: String, why: String) {
    let found =
        w.hq.store()
            .events_after(0)
            .expect("the store answers")
            .into_iter()
            .rfind(|e| e.kind == "world.changed" && e.data["what"] == "demoted")
            .expect("a demotion in the feed");
    assert_eq!(found.data["address"], address.as_str());
    assert_eq!(found.data["why"], why.as_str());
}

// --- Situations, budgets and handovers ----------------------------------------------------------

#[then(expr = "{word}'s turn is recorded as {string}")]
fn recorded_as(w: &mut Office, name: String, situation: String) {
    assert_eq!(w.briefed(&name).situation.name(), situation);
}

#[then(expr = "{word}'s briefing ends with {string}")]
fn ends_with(w: &mut Office, name: String, text: String) {
    let told = w.told(&name);
    assert!(told.trim_end().ends_with(&text), "{name} was told:\n{told}");
}

#[then(regex = r"^(\w+)'s briefing keeps to its budget of (\d+) tokens$")]
fn keeps_budget(w: &mut Office, name: String, budget: usize) {
    let briefed = w.briefed(&name);
    assert_eq!(briefed.budget, budget);
    assert!(briefed.tokens <= budget, "{briefed:?}");
    assert!(!briefed.over_budget);
}

#[then(regex = r"^nothing was left out of (\w+)'s briefing unnamed$")]
fn nothing_unnamed(w: &mut Office, name: String) {
    let briefed = w.briefed(&name);
    let told = w.told(&name);
    let last = told.lines().last().unwrap_or_default();
    for dropped in &briefed.dropped {
        assert!(
            last.contains(dropped.as_str()),
            "{dropped} is not named: {last}"
        );
    }
    assert!(briefed.tokens <= briefed.budget, "{briefed:?}");
}

#[when(
    expr = "{word} hands {string} to {word}, saying done {string}, not done {string}, next {string}, watch out {string}"
)]
#[allow(clippy::too_many_arguments)]
fn hands_with(
    w: &mut Office,
    name: String,
    title: String,
    to: String,
    done: String,
    not_done: String,
    next: String,
    watch_out: String,
) {
    let task = w.task(&title);
    let handed = w.use_tool(
        &name,
        "hand_to",
        &json!({ "task": task.0, "to": to, "why": "Zed is busy",
                 "handover": { "done": done, "not_done": not_done, "next": next,
                               "watch_out": watch_out } }),
    );
    assert!(handed.is_some(), "refused: {:?}", w.refused);
}
