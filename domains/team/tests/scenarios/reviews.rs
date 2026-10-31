//! The steps of `features/reviews-are-capped-at-haiku.feature` and
//! `features/a-ci-run-hq-cannot-get.feature` (ADR 0142, amended 2026-10-31): who reviews an
//! item, and what HQ's checks say of a criterion that asks for a CI run.

use cucumber::{given, then, when};
use serde_json::json;

use endora_team::application::engine::Called;
use endora_team::application::{Author, NewTask, Submission, TeamStore};
use endora_team::domain::{Criterion, Priority};

use super::Office;

#[given(expr = "the loop {string} has the item {string}, with the criteria")]
fn loop_has_item_with_criteria(
    w: &mut Office,
    project: String,
    title: String,
    step: &cucumber::gherkin::Step,
) {
    let criteria: Vec<Criterion> = step
        .table
        .as_ref()
        .expect("the criteria are the step's table")
        .rows
        .iter()
        .map(|row| Criterion::new(&row[0], false))
        .collect();
    let project = w.project(&project).id;
    let goal = w.goal();
    w.hq.add_task_with(
        Author::Owner,
        NewTask {
            goal,
            project: Some(project),
            parent: None,
            title,
            description: String::new(),
            assignee: None,
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
        &criteria,
    )
    .expect("the item is on the loop's plan");
}

/// The node's `repo_submit`, as it tells HQ: the branch, with its one workflow at its head.
#[when(
    expr = "{word} submits the branch {string}, whose workflow {string} runs on a push to {string}"
)]
fn submits_with_workflow(
    w: &mut Office,
    name: String,
    branch: String,
    workflow: String,
    branches: String,
) {
    let context = w.turn_of(&name);
    let workflow_yaml =
        format!("name: CI\non:\n  push:\n    branches: [{branches}]\n  workflow_dispatch:\n");
    w.hq.record_branch(
        &context.token,
        &Submission {
            branch: branch.clone(),
            head: "a1b2c3d4e5f60718293a4b5c".to_owned(),
            base: "0123456789abcdef01234567".to_owned(),
            files: 1,
            plus: 3,
            minus: 0,
            stat: format!(" .github/workflows/{workflow} | 3 +++"),
            diff: "+      - run: python3 -m pytest tests/test_lint_rules.py".to_owned(),
            can_push: true,
            changed: vec![format!(".github/workflows/{workflow}")],
            texts: Vec::new(),
            workflows: Some(vec![(
                format!(".github/workflows/{workflow}"),
                workflow_yaml,
            )]),
        },
    )
    .expect("the node's record is taken");
}

/// The member takes its item and asks for review, citing the item for each criterion, and its
/// turn ends.
#[given(expr = "{word} asks for review of {string}")]
#[when(expr = "{word} asks for review of {string}")]
fn asks_for_review(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    w.use_tool(&name, "claim", &json!({ "task": task.0 }))
        .expect("the member takes its item");
    let found =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .expect("the item");
    let evidence: Vec<_> = found
        .criteria
        .iter()
        .map(|_| json!([format!("task {}", task.0)]))
        .collect();
    w.use_tool(
        &name,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "ready for a look",
                 "evidence": evidence }),
    )
    .expect("the work goes to review");
    let context = w.turn_of(&name);
    w.hq.runner().finish(context.run, true);
    w.tick();
}

#[then(expr = "HQ's checks on {string} say {string}")]
fn checks_say(w: &mut Office, title: String, said: String) {
    let task = w.task(&title);
    let found =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .expect("the item");
    let report =
        w.hq.mechanical_checks(&found)
            .expect("the checks run")
            .said();
    assert!(report.contains(&said), "HQ's checks: {report}");
}

/// The reviewer passes the item with the review tool, citing a reading of its own for each
/// criterion.
#[when(expr = "{word} passes {string} on a reading of her own")]
fn passes_on_own_reading(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let context = w.turn_of(&name);
    let reading =
        w.hq.record_receipt(
            &context.token,
            &Called {
                tool: "repo_checkout".to_owned(),
                args: String::new(),
                excerpt: "checked out the branch".to_owned(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .expect("the node's receipt");
    let found =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .expect("the item");
    let evidence: Vec<_> = found
        .criteria
        .iter()
        .map(|_| json!([format!("r{reading}")]))
        .collect();
    w.use_tool(
        &name,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "notes": "Looks right.",
                 "evidence": evidence }),
    );
}
