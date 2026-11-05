//! The steps of `features/a-turn-that-can-go-on-starts-at-once.feature` (ADR 0102, amended
//! 2026-11-04, the agentic-harness climb's round 17): the runner's report of an end, and the
//! owner's reply that wakes someone, ask for a pass at once.

use cucumber::{then, when};
use endora_team::application::Author;
use endora_team::application::TeamStore;
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
