//! The steps of `features/guards.feature` (ADR 0124): process health over the record, the
//! review carousel, retracted claims and evidence that is already enough.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use endora_team::application::TeamStore;
use endora_team::domain::WakeReason;
use endora_team::domain::guards::THRESHOLDS;
use serde_json::json;

use super::Office;

impl Office {
    /// The guards, as the HQ screen's Guards card reads them.
    fn guards(&self) -> serde_json::Value {
        self.hq.snapshot().expect("the snapshot is built")["guards"].clone()
    }
}

#[given(regex = r#"^the project "(.+)" led by (\w+)$"#)]
fn project_led_by(w: &mut Office, title: String, lead: String) {
    let lead = w.member(&lead);
    let goal = w.goal();
    let id =
        w.hq.add_project(goal, &title, "", lead, None)
            .expect("the owner adds a project");
    w.projects.insert(title, id);
}

/// `turns` turns of the member's on the task, each woken by a mention, each only a post.
#[when(regex = r#"^(\w+) talks about "(.+)" for (\d+) turns and changes nothing$"#)]
fn talks(w: &mut Office, name: String, title: String, turns: usize) {
    let member = w.member(&name);
    let task = w.task(&title);
    for i in 0..turns {
        w.hq.wake(member, Some(task), WakeReason::Mentioned)
            .expect("a wake has a receipt");
        w.use_tool(
            &name,
            "post",
            &json!({ "task": task.0, "text": format!("Still weighing how to do it ({i}).") }),
        )
        .expect("a member may post in its own task");
        super::turn_ends(w, name.clone());
    }
}

#[when("HQ looks at how the team is doing")]
fn looks(w: &mut Office) {
    w.clock
        .0
        .fetch_add(THRESHOLDS.look_every_ms, Ordering::SeqCst);
    w.hq.tick().expect("the tick runs");
}

#[then(expr = "the owner's desk holds a finding {string}")]
fn desk_holds_finding(w: &mut Office, what: String) {
    let found = w.waiting_on_desk("guard");
    assert!(
        found.iter().any(|d| d.text.starts_with(&what)),
        "on the desk: {found:?}"
    );
}

#[then(expr = "the guards card says {string}")]
fn guards_card_says(w: &mut Office, said: String) {
    let card = w.guards().to_string();
    assert!(card.contains(&said), "the guards: {card}");
}

#[when("the owner says the finding was real")]
fn owner_says_real(w: &mut Office) {
    let item = w
        .waiting_on_desk("guard")
        .pop()
        .expect("a finding on the desk");
    let decided = w.hq.decide(item.id, "real");
    w.note(decided);
}

#[then(regex = r#"^the guard "(\w+)" is (\d+)% real$"#)]
fn guard_precision(w: &mut Office, guard: String, percent: u64) {
    let card = w.guards();
    let found = card["precision"]
        .as_array()
        .expect("each guard's precision")
        .iter()
        .find(|p| p["guard"] == guard.as_str())
        .cloned()
        .unwrap_or_else(|| panic!("no guard called {guard}"));
    assert_eq!(found["precision"], json!(percent), "{found}");
}

#[then(regex = r#"^a huddle is called on "(.+)", because it was "(.+)"$"#)]
fn huddle_called_because(w: &mut Office, title: String, why: String) {
    let task = w.task(&title);
    let called: Vec<serde_json::Value> =
        w.hq.store()
            .events_after(0)
            .expect("the store answers")
            .into_iter()
            .filter(|e| e.kind == "huddle.called" && e.data["task"] == task.0)
            .map(|e| e.data)
            .collect();
    assert!(
        called
            .iter()
            .any(|d| d["why"].as_str().is_some_and(|w| w.contains(&why))),
        "huddles on {title}: {called:?}"
    );
}
