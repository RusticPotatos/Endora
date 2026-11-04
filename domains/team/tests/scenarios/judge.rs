//! The steps of `features/the-judge-is-tested.feature` (the baseline experiment's round 4, item
//! D): known-good and known-bad work from `tests/fixtures/judge.json`, run through HQ for what
//! its rules find and for the delivery of the reviewer's verdict (D19: a right verdict that
//! never landed).

use cucumber::{given, then, when};
use serde_json::{Value, json};

use endora_team::application::{Submission, TeamStore, protocol};
use endora_team::domain::{Criterion, TaskStatus, WakeReason};

use super::Office;

/// The judge's cases, as the fixture holds them.
fn case(name: &str) -> Value {
    let all: Value = serde_json::from_str(include_str!("../fixtures/judge.json"))
        .expect("the judge's fixtures are JSON");
    all["cases"]
        .as_array()
        .expect("the cases")
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no judge's case {name:?}"))
        .clone()
}

/// The item a case is put on its loop as: its name, as the owner would title it.
fn title(name: &str) -> String {
    format!("Judge's case: {name}")
}

/// Whether HQ sends the case back itself, without a reviewer.
fn sent_back_by_hq(case: &Value) -> bool {
    case["hq_finds"] == "a forbidden path"
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

#[given(expr = "the loop {string} has the judge's case {string}")]
fn loop_has_case(w: &mut Office, project: String, name: String) {
    let case = case(&name);
    let criteria: Vec<Criterion> = strings(&case["criteria"])
        .iter()
        .map(|c| Criterion::new(c, false))
        .collect();
    w.loop_item(&project, &title(&name), "", &criteria);
}

/// The worker's `repo_submit` as the node tells HQ (the branch, what it changes, the files HQ's
/// checks read at its head and its workflows), the branch's check passing, then its ask for
/// review, citing the item.
#[when(expr = "{word} submits the judge's case {string} and asks for review")]
fn submits_case(w: &mut Office, name: String, case_name: String) {
    let case = case(&case_name);
    let task = w.task(&title(&case_name));
    w.use_tool(&name, "claim", &json!({ "task": task.0 }))
        .expect("the member takes its item");
    let context = w.turn_of(&name);
    let changed = strings(&case["changed"]);
    let texts: Vec<(String, String)> = case["files"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(path, text)| (path.clone(), text.as_str().unwrap_or("").to_owned()))
        .collect();
    let workflows: Vec<(String, String)> = case["workflows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|pair| {
            (
                pair[0].as_str().unwrap_or("").to_owned(),
                pair[1].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    w.hq.record_branch(
        &context.token,
        &Submission {
            branch: format!("hq/task-{}-judges-case", task.0),
            head: "a1b2c3d4e5f60718293a4b5c".to_owned(),
            base: "0123456789abcdef01234567".to_owned(),
            files: changed.len(),
            plus: 6,
            minus: 0,
            stat: changed
                .iter()
                .map(|f| format!(" {f} | 3 +++"))
                .collect::<Vec<_>>()
                .join("\n"),
            diff: texts
                .iter()
                .map(|(_, t)| format!("+{t}"))
                .collect::<Vec<_>>()
                .join("\n"),
            can_push: true,
            changed,
            texts,
            workflows: Some(workflows),
        },
    )
    .expect("the node's record is taken");
    // The branch's own check passes (lint as develop has it): the judge has the work to judge.
    let check =
        w.hq.next_check()
            .expect("the store answers")
            .expect("a check waits");
    w.hq.check_done(
        check.id,
        &check.token,
        true,
        "lint: the same findings as develop",
        "the commands' last words",
    )
    .expect("reported once");
    let evidence: Vec<_> = strings(&case["criteria"])
        .iter()
        .map(|_| json!([format!("task {}", task.0)]))
        .collect();
    w.asks_for_review(&name, task, "ready for a look", &json!(evidence));
}

#[then(expr = "HQ's checks on the judge's case {string} find what it is known for")]
fn checks_find(w: &mut Office, case_name: String) {
    let case = case(&case_name);
    let task = w.task(&title(&case_name));
    let found =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .expect("the item");
    let report = w.hq.mechanical_checks(&found).expect("the checks run");
    let said = report.said();
    match case["hq_finds"].as_str().unwrap_or("") {
        "every default covered" => {
            assert!(!report.coverage.is_empty(), "{said}");
            assert!(report.coverage.iter().all(|c| c.whole()), "{said}");
            assert!(report.forbidden().is_empty() && report.no_ci_run().is_none());
        }
        "a default not covered" => {
            assert!(report.coverage.iter().any(|c| !c.whole()), "{said}");
        }
        "a forbidden path" => assert!(!report.forbidden().is_empty(), "{said}"),
        "no CI run" => assert!(report.no_ci_run().is_some(), "{said}"),
        other => panic!("a case known for {other:?}"),
    }
}

#[then(
    expr = "the reviewer of the judge's case {string} is offered the review tool up front, unless HQ sent it back itself"
)]
fn reviewer_offered(w: &mut Office, case_name: String) {
    let case = case(&case_name);
    let task = w.task(&title(&case_name));
    let ada = w.member("Ada");
    let asked = w.hq.wakes().expect("the store answers").iter().any(|x| {
        x.member == ada && x.task == Some(task) && x.reason == WakeReason::ReviewRequested
    });
    if sent_back_by_hq(&case) {
        assert!(!asked, "HQ sends it back; no reviewer is asked");
        return;
    }
    assert!(asked, "Ada, at Haiku's rank, is asked to review Kit's work");
    let context = w.turn_of("Ada");
    let (run, member) =
        w.hq.turn_of(&context.token)
            .expect("the store answers")
            .expect("a running turn");
    let offered = protocol::tools_in(&w.hq, &run, &member);
    assert!(
        offered
            .as_array()
            .expect("tools")
            .iter()
            .any(|t| t["name"] == "review" && t["_meta"][protocol::UP_FRONT] == true),
        "the verdict tool is marked up front, so a Claude turn loads it (D19)"
    );
}

#[when(expr = "the reviewer gives the judge's case {string} its right verdict")]
fn gives_verdict(w: &mut Office, case_name: String) {
    let case = case(&case_name);
    if sent_back_by_hq(&case) {
        return;
    }
    let task = w.task(&title(&case_name));
    let why = case["why"].as_str().unwrap_or("").to_owned();
    if case["hq_finds"] == "no CI run" {
        // A pass of a criterion no push can meet is refused, whoever gives it (D6).
        let reading = w.reads("Ada", "repo_checkout", "", "checked out the branch");
        let refused = w.use_tool(
            "Ada",
            "review",
            &json!({ "task": task.0, "verdict": "approve", "notes": "Looks right.",
                     "evidence": [[format!("r{reading}")]] }),
        );
        assert!(refused.is_none(), "HQ refuses a pass it knows cannot hold");
    }
    w.reviews("Ada", task, case["verdict"] == "approve", &why);
}

#[then(expr = "the verdict on the judge's case {string} is recorded and acted on")]
fn verdict_landed(w: &mut Office, case_name: String) {
    let case = case(&case_name);
    let task = w.task(&title(&case_name));
    let found =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .expect("the item");
    let thread = w.hq.store().thread(task).expect("the store answers");
    if case["verdict"] == "approve" {
        assert_eq!(found.status, TaskStatus::Done, "{thread:#?}");
        assert!(
            thread.iter().any(|p| p.author == "Ada"),
            "the pass is in the thread: {thread:#?}"
        );
        assert_eq!(w.failures(&title(&case_name)), 0);
        return;
    }
    assert_eq!(found.status, TaskStatus::Doing, "{thread:#?}");
    assert_eq!(w.holder_of(&title(&case_name)).as_deref(), Some("Kit"));
    assert_eq!(
        w.failures(&title(&case_name)),
        1,
        "a send-back is one failure"
    );
    let kit = w.member("Kit");
    assert!(
        w.hq.wakes()
            .expect("the store answers")
            .iter()
            .any(|x| x.member == kit && x.task == Some(task) && x.reason == WakeReason::SentBack),
        "Kit is woken to redo it"
    );
    if !sent_back_by_hq(&case) {
        let why = case["why"].as_str().unwrap_or("");
        assert!(
            thread
                .iter()
                .any(|p| p.author == "Ada" && p.text.contains(why)),
            "the send-back and its reasons are in the thread: {thread:#?}"
        );
    }
}
