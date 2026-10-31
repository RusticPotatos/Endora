//! The steps of `features/what-wakes-an-item-in-the-loops.feature` (ADR 0142, amended
//! 2026-10-31): a turn that never started is tried again later, a blocked item waits for the
//! owner, and an item has one question on the owner's desk at a time.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use serde_json::json;

use endora_team::application::run_ports::RunStore;
use endora_team::application::{Author, NewTask, TeamStore};
use endora_team::domain::{Brain, DeskId, Priority, Receipt, Role, RunStatus, TaskStatus};
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
    fn finish_running(&self) {
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
    for (name, model, role) in [
        ("Mara", "opus", Role::Ceo),
        ("Kit", "sonnet", Role::Lead),
        ("Ada", "haiku", Role::Lead),
        ("Rae", "mac", Role::Developer),
        ("Bo", "local", Role::Developer),
    ] {
        let id = w.hq.hire(name, role, 10, 2, 0).expect("hired");
        w.hq.set_model(id, model).expect("its model");
        w.members.insert(name.to_owned(), id);
    }
}

#[given(expr = "the loop {string}")]
fn the_loop(w: &mut Office, title: String) {
    let goal = match w.goal {
        Some(goal) => goal,
        None => {
            let goal =
                w.hq.set_goal(
                    "Keep the home lab healthy",
                    "the owner asked",
                    Author::Owner,
                )
                .expect("a goal");
            w.goal = Some(goal);
            goal
        }
    };
    let lead = w.member("Mara");
    let project =
        w.hq.add_project(goal, &title, "the fork's work", lead, None)
            .expect("a project");
    w.projects.insert(title, project);
}

#[given(expr = "the loop {string} has the item {string}")]
fn loop_has_item(w: &mut Office, project: String, title: String) {
    let project = w.project(&project).id;
    let goal = w.goal();
    w.add_task(
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
    )
    .expect("the item is on the loop's plan");
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
    let task = w.task(&title);
    assert_eq!(w.hq.loop_failures(task).expect("the store answers"), 0);
    let found =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .expect("the item");
    assert_eq!(found.owner.or(found.assignee), Some(w.member(&name)));
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
    assert_eq!(found.owner.or(found.assignee), Some(w.member(&name)));
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
        let turn = w.turn_of(&name);
        w.hq.note_tokens(turn.run, 50_000, 0, 0, 500)
            .expect("the relay meters the turn");
        let said = w
            .use_tool(
                &name,
                "post",
                &json!({ "task": task.0, "last_word": true, "continues": true,
                         "text": "The turn's prompt reached its budget." }),
            )
            .expect("the last word is posted");
        assert!(!said.contains("not continued"), "{said}");
        w.hq.runner().finish(turn.run, true);
        w.tick();
    }
}
