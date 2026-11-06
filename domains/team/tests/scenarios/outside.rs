//! The steps of `features/an-outside-branch-is-never-given-the-labs-check.feature` (ADR 0115,
//! amended 2026-11-01): a repository outside the lab named on a loop, and a member asking for
//! its branch to be checked again.

use cucumber::{given, then, when};
use serde_json::json;

use endora_team::application::{Author, NewRoutine, TeamStore};

use super::Office;

#[given(expr = "the owner names {string} as the repository of the loop {string}")]
fn names_the_repository(w: &mut Office, url: String, title: String) {
    let project = w.project(&title).id;
    w.hq.set_project_repo(project, Some(&url))
        .expect("a public GitHub repository is taken");
}

/// Climb 3, round 7: a routine of the loop's own goal and project, handed to one member, fired at
/// once (a routine's task needs no gate: it changes no code of the lab's).
#[given(expr = "the loop {string} has the routine {string} for {word}, which fires now")]
fn loop_routine_fires(w: &mut Office, loop_title: String, title: String, name: String) {
    let project = w.project(&loop_title);
    let assignee = w.member(&name);
    let id =
        w.hq.add_routine(
            Author::Owner,
            NewRoutine {
                title,
                description: "Keep this environment healthy, secure and useful.".to_owned(),
                goal: project.goal,
                project: Some(project.id),
                assignee,
                every_ms: 15 * 60 * 1000,
                on_lab_trouble: false,
            },
        )
        .expect("the owner adds a routine");
    w.hq.fire_routine(id).expect("the routine fires");
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
