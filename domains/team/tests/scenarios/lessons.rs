//! The steps of `features/what-every-pass-reads-first.feature` (ADR 0142, amended 2026-10-31,
//! after Ralph): every work turn opens with the same words, and a loop keeps a few lessons its
//! work turns read right after the item. All data is fictional.

use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use serde_json::json;

use endora_team::application::TeamStore;
use endora_team::application::engine::Called;
use endora_team::domain::WakeReason;

use super::Office;

impl Office {
    /// The lessons of the loop titled `title`, oldest first.
    fn lessons_of(&self, title: &str) -> Vec<String> {
        let project = self.project(title).id;
        self.hq.loop_lessons(project).expect("the store answers")
    }

    /// A reading `name`'s running turn takes: the node's receipt of it.
    fn reading(&mut self, name: &str) -> i64 {
        let context = self.turn_of(name);
        self.hq
            .record_receipt(
                &context.token,
                &Called {
                    tool: "container_logs".to_owned(),
                    args: "gluetun".to_owned(),
                    excerpt: "healthy".to_owned(),
                    ok: true,
                    act: false,
                    confidential: false,
                },
            )
            .expect("the node's receipt")
    }

    /// The reviewer's turn on `title`: it was asked to review it, and reviews it with `notes`.
    fn reviews(&mut self, name: &str, title: &str, approve: bool, notes: &str) {
        let task = self.task(title);
        let member = self.member(name);
        assert!(
            self.hq
                .wakes()
                .expect("the store answers")
                .iter()
                .any(|w| w.member == member && w.reason == WakeReason::ReviewRequested),
            "{name} is asked to review {title:?}"
        );
        let mut args = json!({ "task": task.0, "notes": notes,
            "verdict": if approve { "approve" } else { "send_back" } });
        if approve {
            args["evidence"] = json!([[format!("r{}", self.reading(name))]]);
        }
        self.use_tool(name, "review", &args)
            .unwrap_or_else(|| panic!("the review is taken: {:?}", self.refused));
        self.ends_turn(name);
    }

    /// The briefing `name`'s running turn on `title` was told.
    fn told_on(&mut self, name: &str, title: &str) -> String {
        let task = self.task(title);
        let context = self.turn_of(name);
        assert_eq!(context.task, Some(task), "{name}'s turn is on {title:?}");
        context.told
    }
}

/// The lines of a step's table, one a row.
fn lines_of(step: &Step) -> Vec<String> {
    step.table
        .as_ref()
        .expect("the lessons are the step's table")
        .rows
        .iter()
        .map(|row| row[0].trim().to_owned())
        .collect()
}

#[then(
    regex = r#"^(\w+) on "(.+)" and (\w+) on "(.+)" are told the same opening before who each is$"#
)]
fn same_opening(w: &mut Office, one: String, first: String, other: String, second: String) {
    let a = w.told_on(&one, &first);
    let b = w.told_on(&other, &second);
    let who = |told: &str| told.find("You are ").expect("it says who the member is");
    let (at_a, at_b) = (who(&a), who(&b));
    assert!(
        at_a > 0,
        "the briefing opens with what every turn is told: {a}"
    );
    assert_eq!(a[..at_a], b[..at_b], "the same opening");
    assert_ne!(a[at_a..], b[at_b..], "what follows is each turn's own");
}

#[when(regex = r#"^(\w+) asks for review of "(.+)", saying "(.+)"$"#)]
fn asks_for_review(w: &mut Office, name: String, title: String, reason: String) {
    let task = w.task(&title);
    w.use_tool(&name, "claim", &json!({ "task": task.0 }));
    let reading = w.reading(&name);
    w.use_tool(
        &name,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": reason,
                 "evidence": [[format!("r{reading}")]] }),
    )
    .unwrap_or_else(|| panic!("the item goes to review: {:?}", w.refused));
    w.ends_turn(&name);
}

#[when(regex = r#"^(\w+) sends "(.+)" back, saying "(.+)"$"#)]
fn sends_back(w: &mut Office, name: String, title: String, notes: String) {
    w.reviews(&name, &title, false, &notes);
}

#[when(
    regex = r#"^(\w+) approves "(.+)", noting lessons a line too long, one holding a token, and "(.+)"$"#
)]
fn approves_noting(w: &mut Office, name: String, title: String, lesson: String) {
    let notes = format!(
        "Checked the tag.\nLesson: {}\nLesson: the key is ghp_{}\nLesson: {lesson}",
        "x".repeat(161),
        "a1".repeat(18)
    );
    w.reviews(&name, &title, true, &notes);
}

#[given(expr = "the loop {string} has the lessons:")]
fn has_the_lessons_given(w: &mut Office, title: String, step: &Step) {
    let project = w.project(&title).id;
    w.hq.set_loop_lessons(project, &lines_of(step))
        .expect("the owner sets them");
}

#[then(expr = "the loop {string} has the lessons:")]
fn has_the_lessons(w: &mut Office, title: String, step: &Step) {
    assert_eq!(w.lessons_of(&title), lines_of(step));
}

#[then(expr = "the loop {string} has no lessons")]
fn has_no_lessons(w: &mut Office, title: String) {
    assert!(
        w.lessons_of(&title).is_empty(),
        "{:?}",
        w.lessons_of(&title)
    );
}

#[given(regex = r#"^the loop "(.+)" has (\d+) lessons, the oldest "(.+)"$"#)]
fn has_many(w: &mut Office, title: String, count: usize, oldest: String) {
    let project = w.project(&title).id;
    let lines: Vec<String> = (1..=count).map(|i| format!("Lesson number {i}.")).collect();
    assert_eq!(lines[0], oldest);
    w.hq.set_loop_lessons(project, &lines)
        .expect("the owner sets them");
}

#[then(regex = r#"^the loop "(.+)" has (\d+) lessons, the newest "(.+)"$"#)]
fn has_many_newest(w: &mut Office, title: String, count: usize, newest: String) {
    let lessons = w.lessons_of(&title);
    assert_eq!(lessons.len(), count, "{lessons:?}");
    assert_eq!(lessons.last(), Some(&newest));
}

#[then(expr = "the loop {string} no longer has the lesson {string}")]
fn no_longer_has(w: &mut Office, title: String, lesson: String) {
    assert!(!w.lessons_of(&title).contains(&lesson));
}

#[then(regex = r#"^(\w+)'s next turn on "(.+)" reads the lessons right after the item$"#)]
fn next_turn_reads(w: &mut Office, name: String, title: String) {
    let told = w.told_on(&name, &title);
    let item = told
        .find(&format!("Your task: {title}"))
        .expect("the item is told");
    let lessons = told
        .find("Lessons this loop learned")
        .unwrap_or_else(|| panic!("the lessons are told: {told}"));
    let next = told[item..]
        .find("\nWhy it exists:")
        .map_or(told.len(), |at| item + at);
    assert!(
        item < lessons && lessons < next,
        "right after the item: {told}"
    );
    let task = w.task(&title);
    let project =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .and_then(|t| t.project)
            .expect("the item is on a loop's plan");
    for lesson in w.hq.loop_lessons(project).expect("the store answers") {
        assert!(told.contains(&format!("- {lesson}\n")), "{lesson}: {told}");
    }
}

#[when(regex = r#"^the owner sets the lessons of "(.+)" to "(.+)" and "(.+)"$"#)]
fn owner_sets(w: &mut Office, title: String, one: String, two: String) {
    let project = w.project(&title).id;
    let set = w.hq.set_loop_lessons(project, &[one, two]);
    w.note(set);
}
