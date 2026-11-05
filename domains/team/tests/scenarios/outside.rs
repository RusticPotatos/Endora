//! The steps of `features/an-outside-branch-is-never-given-the-labs-check.feature` (ADR 0115,
//! amended 2026-11-01): a repository outside the lab named on a loop, and a member asking for
//! its branch to be checked again.

use cucumber::{given, then, when};
use serde_json::json;

use endora_team::application::TeamStore;

use super::Office;

#[given(expr = "the owner names {string} as the repository of the loop {string}")]
fn names_the_repository(w: &mut Office, url: String, title: String) {
    let project = w.project(&title).id;
    w.hq.set_project_repo(project, Some(&url))
        .expect("a public GitHub repository is taken");
}

#[when(expr = "{word} asks for {string} to be checked again")]
fn asks_for_a_check_again(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "recheck",
        &json!({ "task": task.0, "why": "a receipt to cite" }),
    );
}

#[then(expr = "the gate of {string} is open")]
fn gate_open(w: &mut Office, title: String) {
    let task = w.task(&title);
    assert_eq!(
        w.hq.store().gate(task).expect("the store answers"),
        Some(true)
    );
}
