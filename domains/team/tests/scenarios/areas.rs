//! The steps of the scenario in `features/the-engineering-wing.feature` that is about areas
//! (ADR 0120, amended): the CEO directs, and does not move an area's work to another's lead.

use cucumber::{then, when};
use endora_team::application::TeamStore;
use serde_json::json;

use super::Office;

/// A member tries to hand a task to another, saying why and what is done, as a tool call, and
/// whatever came of it (the task moved, or a refusal) is for the next step to read.
#[when(expr = "{word} tries to hand {string} to {word}")]
fn tries_to_hand(w: &mut Office, name: String, title: String, to: String) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "hand_to",
        &json!({ "task": task.0, "to": to, "why": "Ops has the time",
                 "handover": { "done": "nothing yet", "not_done": "the role", "next": "tidy it",
                               "watch_out": "the check" } }),
    );
}

#[then(expr = "the task {string} is still held by {word}")]
fn still_held_by(w: &mut Office, title: String, name: String) {
    let member = w.member(&name);
    let task =
        w.hq.store()
            .task(w.task(&title))
            .expect("the store answers")
            .expect("a task");
    assert_eq!(
        task.owner.or(task.assignee),
        Some(member),
        "{title:?} moved: {task:?}"
    );
}
