//! The steps of `features/what-wakes-an-item-in-the-loops.feature` (ADR 0142, amended
//! 2026-10-31): a turn that never started is tried again later, a blocked item waits for the
//! owner, and an item has one question on the owner's desk at a time.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use serde_json::json;

use endora_team::application::run_ports::RunStore;
use endora_team::application::{Author, TeamStore};
use endora_team::domain::{Brain, DeskId, Receipt, RunStatus, TaskStatus, WakeReason};
use endora_team::infrastructure::Script;

use super::Office;

impl Office {
    /// The one question waiting on the owner's desk about the task titled `title`.
    fn question_about(&self, title: &str) -> DeskId {
        let task = self.task(title);
        let waiting: Vec<_> = self
            .waiting_on_desk("question")
            .into_iter()
            .filter(|d| d.task == Some(task))
            .collect();
        assert_eq!(waiting.len(), 1, "questions about {title:?}: {waiting:?}");
        waiting[0].id
    }

    /// Ends every turn still running, well, as their workers would.
    pub(super) fn finish_running(&self) {
        for context in self.hq.runner().started() {
            let running = self
                .hq
                .store()
                .run(context.run)
                .expect("the store answers")
                .is_some_and(|r| r.status == RunStatus::Running);
            if running {
                self.hq.runner().finish(context.run, true);
            }
        }
    }
}

#[given("the five ranked members Mara, Kit, Ada, Rae and Bo")]
fn five_ranked_members(w: &mut Office) {
    for (name, model) in [
        ("Mara", "opus"),
        ("Kit", "sonnet"),
        ("Ada", "haiku"),
        ("Rae", "mac"),
        ("Bo", "local"),
    ] {
        w.hires_model(name, model);
    }
}

#[given(expr = "the loop {string}")]
fn the_loop(w: &mut Office, title: String) {
    if w.goal.is_none() {
        let goal =
            w.hq.set_goal(
                "Keep the home lab healthy",
                "the owner asked",
                Author::Owner,
            )
            .expect("a goal");
        w.goal = Some(goal);
    }
    let lead = w.member("Mara");
    w.makes_loop(&title, lead);
}

#[given(expr = "the loop {string} has the item {string}")]
fn loop_has_item(w: &mut Office, project: String, title: String) {
    w.loop_item(&project, &title, "", &[]);
}

/// The loop gets another item while the scenario runs, as the owner's routine would add it.
#[when(expr = "the loop {string} gets the item {string}")]
fn loop_gets_item(w: &mut Office, project: String, title: String) {
    w.loop_item(&project, &title, "", &[]);
}

/// The item is done: its running turns end as their workers would, and it is recorded done, as
/// HQ records an item it closes.
#[when(expr = "{string} is done")]
fn item_is_done(w: &mut Office, title: String) {
    w.finish_running();
    let task = w.task(&title);
    w.hq.store()
        .set_task_status(task, TaskStatus::Done)
        .expect("the store records it done");
}

#[given(expr = "the loop {string} starts at {word}'s rank")]
fn loop_starts_at(w: &mut Office, project: String, name: String) {
    let project = w.project(&project).id;
    let member = w.member(&name);
    let brain =
        w.hq.store()
            .members()
            .expect("the store answers")
            .into_iter()
            .find(|m| m.id == member)
            .map_or(Brain::Local, |m| m.brain);
    w.hq.set_loop_rank(project, brain.rank())
        .expect("the loop's rank is set");
}

#[given(expr = "{word}'s turns cannot start, because the runner cannot start his runtime")]
fn turns_cannot_start(w: &mut Office, name: String) {
    let member = w.member(&name);
    w.hq.runner().script(member, Script::NeverStarts);
}

#[when(expr = "the runner can start {word}'s turns again")]
fn turns_can_start(w: &mut Office, name: String) {
    let member = w.member(&name);
    w.hq.runner().script(member, Script::StaysRunning);
}

#[given("the loop hands out its items")]
#[when("the loop hands out its items")]
fn loop_hands_out(w: &mut Office) {
    w.tick();
}

/// Ticks a minute apart until the member's newest turn on the item has failed for good.
#[when(expr = "every try of {word}'s turn on {string} fails to start")]
fn every_try_fails(w: &mut Office, name: String, title: String) {
    for _ in 0..10 {
        w.tick();
        if w.newest_turn_of(&name, &title).status == RunStatus::Failed {
            return;
        }
        w.clock.0.fetch_add(61_000, Ordering::SeqCst);
    }
    panic!("{name}'s turn on {title:?} did not fail for good");
}

#[then(expr = "{string} has no failures and is still {word}'s")]
fn no_failures_still_held(w: &mut Office, title: String, name: String) {
    assert_eq!(w.failures(&title), 0);
    assert_eq!(w.holder_of(&title), Some(name));
}

#[then(regex = r#"^(\w+) has been woken on "(.+)" (\d+) times?$"#)]
fn woken_times(w: &mut Office, name: String, title: String, times: usize) {
    let (member, task) = (w.member(&name), w.task(&title));
    let wakes: Vec<_> =
        w.hq.wakes()
            .expect("the store answers")
            .into_iter()
            .filter(|wake| wake.member == member && wake.task == Some(task))
            .filter(|wake| !matches!(wake.receipt, Receipt::Refused(_)))
            .collect();
    assert_eq!(wakes.len(), times, "{name}'s wakes: {wakes:#?}");
}

#[when(expr = "{word} blocks {string}, saying {string}")]
fn blocks(w: &mut Office, name: String, title: String, reason: String) {
    let task = w.task(&title);
    w.use_tool(&name, "claim", &json!({ "task": task.0 }))
        .expect("the member takes its item");
    w.use_tool(
        &name,
        "set_status",
        &json!({ "task": task.0, "status": "blocked", "reason": reason }),
    )
    .expect("the member blocks its item");
}

#[then(expr = "the owner's desk holds one question about {string}")]
fn one_question_about(w: &mut Office, title: String) {
    w.question_about(&title);
}

#[then(expr = "the owner's desk holds no question about {string}")]
fn no_question_about(w: &mut Office, title: String) {
    let task = w.task(&title);
    let waiting: Vec<_> = w
        .waiting_on_desk("question")
        .into_iter()
        .filter(|d| d.task == Some(task))
        .collect();
    assert!(
        waiting.is_empty(),
        "questions about {title:?}: {waiting:#?}"
    );
}

#[when(regex = r"^(\d+) hours pass, with HQ ticking every few minutes$")]
fn hours_pass(w: &mut Office, hours: i64) {
    for _ in 0..hours * 12 {
        w.clock.0.fetch_add(5 * 60_000, Ordering::SeqCst);
        w.tick();
        w.finish_running();
    }
}

#[when(expr = "the owner marks the question about {string} seen, with no words")]
fn marks_seen(w: &mut Office, title: String) {
    let item = w.question_about(&title);
    w.hq.decide(item, "seen").expect("the owner decides");
}

#[when(expr = "the owner answers the question about {string}: {string}")]
fn answers(w: &mut Office, title: String, words: String) {
    let item = w.question_about(&title);
    w.hq.answer(item, &words).expect("the owner answers");
}

#[when(expr = "{word} asks the owner {string}")]
fn asks_the_owner(w: &mut Office, name: String, text: String) {
    let task = w.turn_of(&name).task.expect("a turn on an item");
    w.use_tool(
        &name,
        "ask_owner",
        &json!({ "text": text, "evidence": [format!("task {}", task.0)] }),
    );
}

#[given(expr = "the owner pauses {word}")]
#[when(expr = "the owner pauses {word}")]
fn owner_pauses(w: &mut Office, name: String) {
    let member = w.member(&name);
    w.hq.pause(member, true).expect("paused");
    w.tick();
}

#[then(expr = "{string} waits at {word}'s rank, not blocked, with nothing on the owner's desk")]
fn waits_at_rank(w: &mut Office, title: String, name: String) {
    let task = w.task(&title);
    let found =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .expect("the item");
    assert_ne!(found.status, TaskStatus::Blocked, "{found:?}");
    assert_eq!(w.holder_of(&title), Some(name));
    let desk: Vec<_> = w
        .waiting_on_desk("")
        .into_iter()
        .filter(|d| d.task == Some(task))
        .collect();
    assert!(desk.is_empty(), "{desk:#?}");
    let thread = w.hq.store().thread(task).expect("the store answers");
    assert!(
        thread.iter().any(|p| p.text.contains("waits for Rae")),
        "it failed and was held: {thread:#?}"
    );
}

/// The member's turn on the item reads near its brain's budget, as the relay meters it, and
/// ends as the worker ends one whose prompt reached its budget: its last word, marked as
/// continuing.
#[when(regex = r#"^(\w+)'s turn on "(.+)" ends at its prompt's budget(?: (\d+) times in a row)?$"#)]
fn ends_at_budget(w: &mut Office, name: String, title: String, times: String) {
    let task = w.task(&title);
    for _ in 0..times.parse::<usize>().unwrap_or(1) {
        assert_eq!(
            w.turn_of(&name).task,
            Some(task),
            "{name}'s turn is on {title:?}"
        );
        let said = w.runs_out_of_room(&name, 50_000, "The turn's prompt reached its budget.");
        assert!(!said.contains("not continued"), "{said}");
    }
}

#[then(expr = "{string} still waits for its review")]
fn still_in_review(w: &mut Office, title: String) {
    let task = w.task(&title);
    let found =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .expect("the item");
    assert_eq!(found.status, TaskStatus::InReview, "{found:?}");
}

#[then(expr = "HQ refused no wake of {word}'s on {string}")]
fn no_wake_refused(w: &mut Office, name: String, title: String) {
    let (member, task) = (w.member(&name), w.task(&title));
    let refused: Vec<_> =
        w.hq.wakes()
            .expect("the store answers")
            .into_iter()
            .filter(|wake| wake.member == member && wake.task == Some(task))
            .filter(|wake| matches!(wake.receipt, Receipt::Refused(_)))
            .collect();
    assert!(refused.is_empty(), "{name}'s refused wakes: {refused:#?}");
}

#[then(regex = r"^a turn on (\w+) counts its budget by (its conversation|every call)$")]
fn turn_counts_budget(_w: &mut Office, model: String, counting: String) {
    let brain = Brain::from_name(&model).expect("a model");
    assert_eq!(
        endora_team::domain::handover::counts_conversation(brain),
        counting == "its conversation",
        "{model}"
    );
}

// ---- One member works a loop alone (ADR 0142, amended 2026-11-01) --------------------------

#[given(expr = "the owner sets the loop {string} to be worked by {word} alone")]
fn loop_worked_alone(w: &mut Office, project: String, name: String) {
    let project = w.project(&project).id;
    let member = w.member(&name);
    w.hq.set_loop_solo(project, Some(member))
        .expect("the owner names the member");
}

/// The member sets its item ready (in review) citing its newest reading, in its turn; a
/// refusal is noted, not a failure of the step.
#[when(regex = r#"^(\w+) sets "(.+)" ready, citing (?:his|her) reading$"#)]
fn sets_ready(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let reading = w.newest_reading(&name);
    w.use_tool(
        &name,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "Done, as the reading shows.",
                 "evidence": [reading] }),
    );
}

/// The receipt and the document HQ's thread names for the newest check of `task`, as its line
/// says them: `(receipt r12, document d34)`.
fn check_line_ids(w: &Office, task: endora_team::domain::TaskId) -> (String, String) {
    let line =
        w.hq.store()
            .thread(task)
            .expect("the store answers")
            .into_iter()
            .rev()
            .find(|p| p.text.starts_with("Check c"))
            .expect("the check's line is in the thread")
            .text;
    let between = |from: &str| {
        let start = line.find(from).expect("the line names it") + from.len();
        line[start..]
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    (
        format!("r{}", between("receipt r")),
        format!("d{}", between("document d")),
    )
}

/// The member sets its item ready citing the document the check wrote, as set 4's turns did;
/// a refusal is noted, not a failure of the step.
#[when(regex = r#"^(\w+) sets "(.+)" ready, citing the check's document$"#)]
fn sets_ready_citing_document(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let (_, document) = check_line_ids(w, task);
    w.use_tool(
        &name,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "Done, as the check shows.",
                 "evidence": [document] }),
    );
}

/// The member sets its item ready citing the check's receipt with a line beside it saying what
/// it shows, in one criterion's list, as 48 refused calls in HQ's record did.
#[when(
    regex = r#"^(\w+) sets "(.+)" ready, citing the check's receipt with a line saying what it shows$"#
)]
fn sets_ready_citing_receipt_with_gloss(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let (receipt, _) = check_line_ids(w, task);
    w.use_tool(
        &name,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "Done, as the check shows.",
                 "evidence": [[receipt, "the check passed: syntax, ansible-lint and yamllint"]] }),
    );
}

#[then(regex = r#"^the refusal names the receipt of the check of "(.+)" to cite$"#)]
fn refusal_names_check_receipt(w: &mut Office, title: String) {
    let task = w.task(&title);
    let (receipt, document) = check_line_ids(w, task);
    let said = w.refused.as_deref().expect("it was refused");
    assert!(
        said.contains(&format!(
            "{document} is a document, not a receipt: cite {receipt}"
        )),
        "{said}"
    );
}

/// The member claims its item and sets it ready citing the item for each criterion, in its
/// turn; a refusal is noted, not a failure of the step.
#[when(regex = r#"^(\w+) sets "(.+)" ready, citing the item for each criterion$"#)]
fn sets_ready_citing_the_item(w: &mut Office, name: String, title: String) {
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
        &json!({ "task": task.0, "status": "in_review", "reason": "ready",
                 "evidence": evidence }),
    );
}

#[then(expr = "no member is asked to review {string}")]
fn nobody_reviews(w: &mut Office, title: String) {
    let task = w.task(&title);
    let asked: Vec<_> =
        w.hq.wakes()
            .expect("the store answers")
            .into_iter()
            .filter(|k| k.task == Some(task) && k.reason == WakeReason::ReviewRequested)
            .collect();
    assert!(asked.is_empty(), "asked to review: {asked:#?}");
}
