//! The steps of `features/a-drain-before-a-restart.feature` (ADR 0134): a restart is announced,
//! nothing new starts, the turns under way wrap up, and what the restart still cuts is counted.

use std::sync::atomic::Ordering;

use cucumber::{then, when};
use endora_team::application::TeamStore;
use serde_json::{Value, json};

use super::Office;

impl Office {
    /// What the drain's door tells a deploy, as it asks.
    fn drain(&self) -> Value {
        self.hq.drain_shown().expect("the drain is told")
    }

    /// The newest time a drain ended, as the feed told it.
    fn drain_ended(&self) -> Value {
        self.hq
            .events_after(0)
            .expect("the feed answers")
            .into_iter()
            .rfind(|e| e.kind == "drain.ended")
            .map(|e| e.data)
            .expect("a drain ended")
    }
}

#[when("the owner announces a restart")]
fn announces(w: &mut Office) {
    w.hq.begin_drain(None).expect("the restart is announced");
}

#[when(expr = "the owner announces a restart that waits {int} seconds")]
fn announces_with_a_cap(w: &mut Office, seconds: i64) {
    w.hq.begin_drain(Some(seconds))
        .expect("the restart is announced");
}

#[when("the owner calls the restart off")]
fn calls_it_off(w: &mut Office) {
    w.hq.end_drain().expect("the restart is called off");
}

#[when(expr = "{int} seconds pass")]
fn seconds_pass(w: &mut Office, seconds: i64) {
    w.clock.0.fetch_add(seconds * 1_000, Ordering::SeqCst);
}

#[then(expr = "{word}'s turn on {string} is waiting for a restart")]
fn waiting_for_a_restart(w: &mut Office, name: String, title: String) {
    let (member, task) = (w.member(&name), w.task(&title));
    let waiting =
        w.hq.store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .find(|r| r.member == member && r.task == Some(task))
            .unwrap_or_else(|| panic!("{name} has had no turn on {title:?}"));
    assert_eq!(
        waiting.status,
        endora_team::domain::RunStatus::Queued,
        "{waiting:?}"
    );
    assert_eq!(
        waiting.end_reason,
        "waiting for a restart: nothing new starts until it has been"
    );
    // The owner's screen says the same.
    let shown = w.hq.snapshot().expect("the snapshot is built");
    let run = shown["runs"]
        .as_array()
        .expect("runs")
        .iter()
        .find(|r| r["id"] == waiting.id.0)
        .expect("the waiting turn is on screen");
    assert_eq!(
        run["why"],
        "waiting for a restart: nothing new starts until it has been"
    );
}

#[when(expr = "{word} looks at what his turn is for")]
fn looks_at_his_turn(w: &mut Office, name: String) {
    w.answer = w.use_tool(&name, "my_turn", &json!({}));
    assert!(w.answer.is_some(), "refused: {:?}", w.refused);
}

#[then(regex = r#"^(\w+) is told "(.+)"$"#)]
fn is_told(w: &mut Office, name: String, text: String) {
    let answer = w.answer.as_deref().unwrap_or_default();
    assert!(answer.contains(&text), "{name} was told:\n{answer}");
}

#[when(regex = r#"^(\w+) leaves the handover note "(.+)" on "(.+)"$"#)]
fn leaves_a_handover(w: &mut Office, name: String, next: String, title: String) {
    let task = w.task(&title);
    let said = w.use_tool(
        &name,
        "post",
        &json!({ "task": task.0, "text": "Wrapping up for the restart.",
                 "handover": { "done": "The limit is found.", "not_done": "It is not set.",
                               "next": next, "watch_out": "Sonarr restarts when it is set." } }),
    );
    assert!(said.is_some(), "refused: {:?}", w.refused);
}

#[then(regex = r#"^(\w+) still has "(.+)"$"#)]
fn still_has(w: &mut Office, name: String, title: String) {
    let (member, task) = (w.member(&name), w.task(&title));
    let held = w.hq.store().task(task).expect("the store answers").unwrap();
    assert!(
        held.owner == Some(member) || held.assignee == Some(member),
        "{title:?} is no longer {name}'s: {held:?}"
    );
    assert!(
        !held.status.finished(),
        "{title:?} is {:?}, not open",
        held.status
    );
    // It was not handed on: nothing was handed to anyone else.
    let notes = endora_team::application::world_ports::WorldStore::handovers(w.hq.store(), task)
        .expect("the store answers");
    assert!(
        notes.iter().all(|n| n.to == Some(member)),
        "handed to another: {notes:?}"
    );
}

#[then("no turn is running, and the restart can go ahead")]
fn nothing_is_running(w: &mut Office) {
    assert_eq!(w.drain()["running"], 0, "{}", w.drain());
}

#[then(regex = r"^the restart must wait for (\d+) running turns?$")]
fn must_wait(w: &mut Office, running: u64) {
    assert_eq!(w.drain()["running"], running, "{}", w.drain());
}

#[then(regex = r"^the restart's cap has passed with (\d+) turns? still running$")]
fn the_cap_has_passed(w: &mut Office, running: u64) {
    let shown = w.drain();
    let since = shown["since"].as_i64().expect("a restart is announced");
    let cap = shown["cap_seconds"].as_i64().expect("with a cap");
    assert!(
        w.clock.0.load(Ordering::SeqCst) >= since + cap * 1_000,
        "the cap has not passed: {shown}"
    );
    assert_eq!(shown["running"], running, "{shown}");
}

#[then("a restart is still announced")]
fn still_announced(w: &mut Office) {
    assert_eq!(w.drain()["draining"], true, "{}", w.drain());
}

#[then("no restart is announced")]
fn none_announced(w: &mut Office) {
    assert_eq!(w.drain()["draining"], false, "{}", w.drain());
}

#[then("the Activity says the drain ended because nobody ended it")]
fn the_activity_says_it_was_forgotten(w: &mut Office) {
    let ended = w.drain_ended();
    assert_eq!(ended["by"], "max age", "{ended}");
}

#[then(regex = r"^the Activity says the restart came after (\d+) turns? ended and (\d+) were cut$")]
fn the_activity_says(w: &mut Office, clean: u64, cut: u64) {
    let ended = w.drain_ended();
    assert_eq!(ended["by"], "restart", "{ended}");
    assert_eq!(ended["clean"], clean, "{ended}");
    assert_eq!(ended["cut"], cut, "{ended}");
}
