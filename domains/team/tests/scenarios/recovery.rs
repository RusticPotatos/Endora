//! The steps of `features/a-turn-that-can-go-on-starts-at-once.feature` (ADR 0102, amended
//! 2026-11-04, the agentic-harness climb's round 17): the runner's report of an end, and the
//! owner's reply that wakes someone, ask for a pass at once.

use cucumber::{given, then, when};
use endora_team::application::Author;
use endora_team::application::TeamStore;
use endora_team::application::receipt_ports::ReceiptStore;
use endora_team::application::run_ports::RunStore;
use endora_team::application::wing_ports::WingStore;
use endora_team::domain::{Exit, RunStatus};

use super::Office;

impl Office {
    /// `name`'s newest turn on `title`, as the store has it.
    fn newest_turn_on(&self, name: &str, title: &str) -> endora_team::application::ports::Run {
        let member = self.member(name);
        let task = self.task(title);
        self.hq
            .store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .filter(|r| r.member == member && r.task == Some(task))
            .max_by_key(|r| r.id.0)
            .unwrap_or_else(|| panic!("{name} has no turn on {title:?}"))
    }
}

#[when(regex = r#"^the runner reports (\w+)'s turn on "(.+)" killed from outside$"#)]
fn runner_reports_killed(w: &mut Office, name: String, title: String) {
    let run = w.newest_turn_on(&name, &title);
    assert_eq!(
        run.status,
        RunStatus::Running,
        "the turn is not running: {run:?}"
    );
    // What HQ's runner door does with an accepted report: the runner holds it, and the engine
    // is told a turn has ended.
    w.hq.runner().finish_badly(run.id, Exit::Code(137));
    w.hq.a_turn_has_ended();
}

#[when(regex = r#"^the owner replies "(.+)" on "(.+)"$"#)]
fn the_owner_replies(w: &mut Office, text: String, title: String) {
    let task = w.task(&title);
    let said = w.hq.post(Author::Owner, task, &text);
    w.note(said);
}

#[then(regex = r#"^(\w+)'s turn on "(.+)" waits to try again$"#)]
fn waits_to_try_again(w: &mut Office, name: String, title: String) {
    let run = w.newest_turn_on(&name, &title);
    assert_eq!(run.status, RunStatus::RetryWaiting, "{run:?}");
    assert!(
        run.retry_at.is_some(),
        "a waiting turn says until when: {run:?}"
    );
}

#[then(regex = r#"^(\w+)'s turn on "(.+)" is running( again)?$"#)]
fn is_running(w: &mut Office, name: String, title: String, again: String) {
    let run = w.newest_turn_on(&name, &title);
    assert_eq!(run.status, RunStatus::Running, "{run:?}");
    if !again.is_empty() {
        assert!(
            run.attempts >= 2,
            "the turn has not been tried again: {run:?}"
        );
    }
}

#[then(regex = r#"^(\w+)'s turn on "(.+)" is not running$"#)]
fn is_not_running(w: &mut Office, name: String, title: String) {
    let run = w.newest_turn_on(&name, &title);
    assert_ne!(run.status, RunStatus::Running, "{run:?}");
}

// ---- `features/a-resumed-item-knows-what-it-submitted.feature` (defect remediation, C) -------

#[then(regex = r#"^"(.+)" has (\d+) branch records?$"#)]
fn branch_records(w: &mut Office, title: String, n: usize) {
    let task = w.task(&title);
    let kept =
        w.hq.store()
            .branches()
            .expect("the store answers")
            .into_iter()
            .filter(|b| b.task == task)
            .count();
    assert_eq!(kept, n);
}

#[then(regex = r#"^"(.+)" has (\d+) checks? queued$"#)]
fn checks_queued(w: &mut Office, title: String, n: usize) {
    let task = w.task(&title);
    let queued =
        w.hq.store()
            .checks()
            .expect("the store answers")
            .into_iter()
            .filter(|c| c.task == task)
            .count();
    assert_eq!(queued, n);
}

#[then(regex = r#"^the thread of "(.+)" says "(.+)" once$"#)]
fn thread_says_once(w: &mut Office, title: String, words: String) {
    let task = w.task(&title);
    let thread = w.hq.store().thread(task).expect("the store answers");
    let times = thread.iter().filter(|p| p.text.contains(&words)).count();
    assert_eq!(times, 1, "{thread:#?}");
}

/// A submission as the node tells HQ of one, on `name`'s running turn, at `head`.
fn told(
    w: &mut Office,
    name: &str,
    branch: &str,
    head: &str,
) -> (String, endora_team::application::Submission) {
    let context = w.turn_of(name);
    (
        context.token,
        endora_team::application::Submission {
            branch: branch.to_owned(),
            head: head.to_owned(),
            base: super::SUBMITTED_BASE.to_owned(),
            files: 1,
            plus: 4,
            minus: 1,
            stat: " parse.py | 5 ++--".to_owned(),
            diff: "-old\n+new".to_owned(),
            can_push: true,
            changed: Vec::new(),
            texts: Vec::new(),
            workflows: None,
        },
    )
}

#[when(regex = r#"^(\w+) keeps the branch "(.+)" as work in progress at the same head$"#)]
fn keeps_in_progress(w: &mut Office, name: String, branch: String) {
    let (token, s) = told(w, &name, &branch, super::SUBMITTED_HEAD);
    w.hq.keep_work_in_progress(&token, &s)
        .expect("the work in progress is kept");
}

#[when(regex = r#"^(\w+) submits the branch "(.+)" at a new head$"#)]
fn submits_new_head(w: &mut Office, name: String, branch: String) {
    let (token, s) = told(w, &name, &branch, "a1b2c3d4e5f60718293a4b5d");
    w.hq.record_branch(&token, &s)
        .expect("the node's record is taken");
}

#[then(regex = r#"^"(.+)" has (\d+) finished submissions?$"#)]
fn finished_submissions(w: &mut Office, title: String, n: usize) {
    let task = w.task(&title);
    let finished =
        w.hq.store()
            .branches()
            .expect("the store answers")
            .into_iter()
            .filter(|b| b.task == task && b.status != endora_team::domain::wing::IN_PROGRESS)
            .count();
    assert_eq!(finished, n);
}

// ---- An item is not closed over work HQ never took as finished (correction-placement r1) ----

/// What the worker reports of its sandbox beside a closing `set_status`: the task's branches
/// with their full heads, read by trusted code from git.
fn sandbox_holding(branch: &str, head: &str) -> serde_json::Value {
    serde_json::json!({ "state": "read", "branches": [{ "branch": branch, "head": head }] })
}

fn sets(w: &mut Office, name: &str, title: &str, status: &str, extra: serde_json::Value) {
    let task = w.task(title);
    let mut args = serde_json::json!({ "task": task.0, "status": status,
        "reason": "Done, as the reading shows." });
    if let Some(reading) = w.newest_reading_of(name) {
        args["evidence"] = serde_json::json!([reading]);
    }
    if let (Some(args), Some(extra)) = (args.as_object_mut(), extra.as_object()) {
        for (k, v) in extra {
            args.insert(k.clone(), v.clone());
        }
    }
    w.use_tool(name, "set_status", &args);
}

#[when(regex = r#"^(\w+) sets "(.+)" ready, the worker reporting "(.+)" at the (\w+) head$"#)]
fn sets_ready_reporting(w: &mut Office, name: String, title: String, branch: String, head: String) {
    let report = serde_json::json!({ "sandbox": sandbox_holding(&branch, &oid(&head)) });
    sets(w, &name, &title, "in_review", report);
}

#[when(regex = r#"^(\w+) sets "(.+)" blocked, the worker reporting "(.+)" at the (\w+) head$"#)]
fn sets_blocked_reporting(
    w: &mut Office,
    name: String,
    title: String,
    branch: String,
    head: String,
) {
    let report = serde_json::json!({ "sandbox": sandbox_holding(&branch, &oid(&head)) });
    sets(w, &name, &title, "blocked", report);
}

#[when(regex = r#"^(\w+) sets "(.+)" ready, the worker reporting no work of the item$"#)]
fn sets_ready_no_work(w: &mut Office, name: String, title: String) {
    let report = serde_json::json!({ "sandbox": { "state": "read", "branches": [] } });
    sets(w, &name, &title, "in_review", report);
}

#[when(regex = r#"^(\w+) sets "(.+)" ready, the worker reporting it could not read the sandbox$"#)]
fn sets_ready_unknown(w: &mut Office, name: String, title: String) {
    let report = serde_json::json!({ "sandbox": { "state": "unknown",
        "why": "git rev-parse failed: not a git repository" } });
    sets(w, &name, &title, "in_review", report);
}

#[when(regex = r#"^(\w+) sets "(.+)" ready with no sandbox report$"#)]
fn sets_ready_unreported(w: &mut Office, name: String, title: String) {
    sets(w, &name, &title, "in_review", serde_json::json!({}));
}

#[when(
    regex = r#"^(\w+) sets "(.+)" ready, leaving "(.+)" at the (\w+) head unsubmitted because "(.+)", the worker reporting "(.+)" at the (\w+) head$"#
)]
#[allow(clippy::too_many_arguments)]
fn sets_ready_leaving(
    w: &mut Office,
    name: String,
    title: String,
    left: String,
    left_head: String,
    why: String,
    branch: String,
    head: String,
) {
    let extra = serde_json::json!({
        "sandbox": sandbox_holding(&branch, &oid(&head)),
        "unsubmitted": [{ "branch": left, "head": oid(&left_head), "why": why }],
    });
    sets(w, &name, &title, "in_review", extra);
}

#[given(regex = r#"^(\w+) took "(.+)" and read (\w+)'s logs: "(.+)"$"#)]
fn took_and_read_logs(w: &mut Office, name: String, title: String, container: String, got: String) {
    let task = w.task(&title);
    w.use_tool(&name, "claim", &serde_json::json!({ "task": task.0 }))
        .expect("the member takes its item");
    w.reads(&name, "container_logs", &container, &got);
}

fn node_receipt(w: &mut Office, name: &str, branch: &str, excerpt: String) {
    let context = w.turn_of(name);
    w.hq.record_receipt(
        &context.token,
        &endora_team::application::engine::Called {
            tool: "repo_checkout".to_owned(),
            args: format!("branch: {branch}"),
            excerpt,
            ok: true,
            act: false,
            confidential: false,
        },
    )
    .expect("the node's receipt of the checkout");
}

/// The node's receipt of a checkout of the default branch, as the node writes it now: its full
/// revision first.
#[given(regex = r#"^the node recorded (\w+)'s checkout of the base at the (\w+) head$"#)]
#[when(regex = r#"^the node recorded (\w+)'s checkout of the base at the (\w+) head$"#)]
fn node_recorded_base(w: &mut Office, name: String, head: String) {
    let excerpt = format!(
        "{}{}: develop (4 KB)",
        endora_team::domain::wing::BASE_CHECKOUT,
        oid(&head)
    );
    node_receipt(w, &name, "develop", excerpt);
}

#[given(
    regex = r#"^the node recorded (\w+)'s checkout of the kept branch "(.+)" at the (\w+) head$"#
)]
fn node_recorded_kept(w: &mut Office, name: String, branch: String, head: String) {
    let excerpt = format!(
        "checked out the team's kept branch {branch} at {} (task #1's, on top of develop; 4 KB)",
        &oid(&head)[..12]
    );
    node_receipt(w, &name, &branch, excerpt);
}

#[given(
    regex = r#"^the node recorded (\w+)'s checkout of "(.+)" at the (\w+) head, abbreviated as older nodes wrote it$"#
)]
fn node_recorded_abbreviated(w: &mut Office, name: String, branch: String, head: String) {
    let excerpt = format!("checked out {branch} at {} (4 KB)", &oid(&head)[..12]);
    node_receipt(w, &name, &branch, excerpt);
}

#[when(regex = r#"^(\w+) sets "(.+)" ready, the worker's report naming a branch with no head$"#)]
fn sets_ready_malformed(w: &mut Office, name: String, title: String) {
    let report = serde_json::json!({ "sandbox": { "state": "read",
        "branches": [{ "branch": "hq/task-1-fix" }] } });
    sets(w, &name, &title, "in_review", report);
}

#[when(regex = r#"^(\w+) submitted the branch "(.+)" at the (\w+) head$"#)]
fn submitted_at(w: &mut Office, name: String, branch: String, head: String) {
    let (token, s) = told(w, &name, &branch, &oid(&head));
    w.hq.record_branch(&token, &s)
        .expect("the node's record is taken");
}

#[when(regex = r#"^(\w+) kept the branch "(.+)" at the (\w+) head as work in progress$"#)]
fn kept_at_in_progress(w: &mut Office, name: String, branch: String, head: String) {
    let (token, s) = told(w, &name, &branch, &oid(&head));
    w.hq.keep_work_in_progress(&token, &s)
        .expect("the work in progress is kept");
}

#[when(
    regex = r#"^(\w+) sets "(.+)" ready citing nothing, leaving "(.+)" at the (\w+) head unsubmitted because "(.+)", the worker reporting "(.+)" at the (\w+) head$"#
)]
#[allow(clippy::too_many_arguments)]
fn sets_ready_citing_nothing_leaving(
    w: &mut Office,
    name: String,
    title: String,
    left: String,
    left_head: String,
    why: String,
    branch: String,
    head: String,
) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "set_status",
        &serde_json::json!({ "task": task.0, "status": "in_review",
            "reason": "Done.",
            "sandbox": sandbox_holding(&branch, &oid(&head)),
            "unsubmitted": [{ "branch": left, "head": oid(&left_head), "why": why }] }),
    );
}

impl Office {
    /// `name`'s newest receipt this turn, when it took one.
    fn newest_reading_of(&mut self, name: &str) -> Option<String> {
        let run = self.turn_of(name).run;
        self.hq
            .store()
            .call_receipts_of(run)
            .expect("the store answers")
            .iter()
            .map(|r| r.id)
            .max()
            .map(|id| format!("r{id}"))
    }
}

/// A full 40-character object id for a head the scenario names (built here: a tracked file never
/// spells one out, as the nothing-personal check would take it for a secret).
fn oid(named: &str) -> String {
    let unit = match named {
        "unsubmitted" => "5faf6aed",
        "finished" => "a1b2c3d4",
        "later" => "a1b2c3d5",
        "handover" => "01234567",
        "other" => "11112222",
        _ => panic!("a head the scenarios do not name: {named}"),
    };
    unit.repeat(5)
}

#[then(regex = r#"^it is refused, naming "(.+)" at the (\w+) head$"#)]
fn refused_naming(w: &mut Office, branch: String, head: String) {
    let said = w.refused.clone().expect("it was refused");
    let wanted = format!("{branch} at {}", oid(&head));
    assert!(said.contains(&wanted), "{said}");
}

#[then(
    regex = r#"^the thread of "(.+)" records "(.+)" at the (\w+) head left unsubmitted because "(.+)"$"#
)]
fn thread_says_left(w: &mut Office, title: String, branch: String, head: String, why: String) {
    let task = w.task(&title);
    let wanted = format!("leaves {branch} at {} unsubmitted: {why}", oid(&head));
    let thread = w.hq.store().thread(task).expect("the store answers");
    let times = thread.iter().filter(|p| p.text.contains(&wanted)).count();
    assert_eq!(times, 1, "{thread:#?}");
}

#[when(regex = r#"^(\w+) sets "(.+)" done, the worker reporting "(.+)" at the (\w+) head$"#)]
fn sets_done_reporting(w: &mut Office, name: String, title: String, branch: String, head: String) {
    let report = serde_json::json!({ "sandbox": sandbox_holding(&branch, &oid(&head)) });
    sets(w, &name, &title, "done", report);
}

#[when(regex = r#"^(\w+) sets "(.+)" done, the worker reporting no work of the item$"#)]
fn sets_done_no_work(w: &mut Office, name: String, title: String) {
    let report = serde_json::json!({ "sandbox": { "state": "read", "branches": [] } });
    sets(w, &name, &title, "done", report);
}

#[then("it is not refused for the sandbox")]
fn not_refused_for_sandbox(w: &mut Office) {
    let said = w.refused.clone().unwrap_or_default();
    assert!(!said.contains("your sandbox"), "{said}");
}
