//! A loop's lessons (ADR 0142, amended 2026-10-31, after snarktank/ralph's "Codebase Patterns"
//! at the top of `progress.txt`): a few lines every work turn on the loop reads right after its
//! item, added from a worker's last word or a reviewer's notes when an item is accepted or sent
//! back, and the owner's to edit. All data is fictional.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::json;

use super::engine::{Author, Called, Engine, Settings};
use super::ports::{Clock, NewTask, RunContext};
use crate::domain::lessons::{LOOP_LESSON_MOST, LOOP_LESSONS_MOST};
use crate::domain::money::DOLLAR;
use crate::domain::{GoalId, MemberId, Priority, ProjectId, Role, TaskId, WakeReason};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const NOON: i64 = 1_792_166_400_000;
const MINUTE: i64 = 60_000;

struct Loop<'a> {
    hq: Hq<'a>,
    clock: &'a Hands,
    goal: GoalId,
    project: ProjectId,
    bo: MemberId,
    rae: MemberId,
}

fn a_loop(clock: &Hands) -> Loop<'_> {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings {
            pool_per_month: 30 * DOLLAR,
            ..Settings::default()
        },
    );
    let goal = hq
        .set_goal("Keep the lab healthy", "you asked", Author::Owner)
        .unwrap();
    let seat = |name: &str, model: &str| {
        let id = hq.hire(name, Role::Developer, 10, 2, 0).unwrap();
        hq.set_model(id, model).unwrap();
        id
    };
    let mara = seat("Mara", "opus");
    let rae = seat("Rae", "mac");
    let bo = seat("Bo", "local");
    let project = hq
        .add_project(goal, "ansible-nas", "the fork's work", mara, None)
        .unwrap();
    hq.set_loop_items(project, 1).unwrap();
    hq.tell_mac(true, "ready");
    Loop {
        hq,
        clock,
        goal,
        project,
        bo,
        rae,
    }
}

impl Loop<'_> {
    fn item(&self, title: &str) -> TaskId {
        self.hq
            .add_task(
                Author::Owner,
                NewTask {
                    goal: self.goal,
                    project: Some(self.project),
                    parent: None,
                    title: title.to_owned(),
                    description: format!("{title}, in roles/gluetun."),
                    assignee: None,
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
            )
            .unwrap()
    }

    fn turn_of(&self, member: MemberId) -> RunContext {
        self.clock.0.fetch_add(MINUTE, Ordering::SeqCst);
        self.hq.tick().unwrap();
        self.hq
            .runner()
            .started()
            .into_iter()
            .rfind(|c| c.member == member)
            .expect("a turn of the member's")
    }

    fn call(&self, context: &RunContext, tool: &str, args: &serde_json::Value) -> String {
        let (run, member) = self
            .hq
            .turn_of(&context.token)
            .unwrap()
            .expect("a running turn");
        super::protocol::call(&self.hq, &run, &member, tool, args).unwrap()
    }

    /// A reading the turn takes: its receipt.
    fn reading(&self, context: &RunContext) -> i64 {
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
            .unwrap()
    }

    /// Bo works the item and asks for review, saying `reason`.
    fn bo_submits(&self, item: TaskId, reason: &str) {
        let bo = self.turn_of(self.bo);
        self.call(&bo, "claim", &json!({ "task": item.0 }));
        let receipt = self.reading(&bo);
        self.call(
            &bo,
            "set_status",
            &json!({ "task": item.0, "status": "in_review", "reason": reason,
                     "evidence": [[format!("r{receipt}")]] }),
        );
        self.hq.runner().finish(bo.run, true);
    }

    /// Rae reviews the item: approves it or sends it back, with `notes`.
    fn rae_reviews(&self, item: TaskId, approve: bool, notes: &str) {
        assert!(
            self.hq
                .wakes()
                .unwrap()
                .iter()
                .any(|w| w.member == self.rae && w.reason == WakeReason::ReviewRequested),
            "Rae is asked to review"
        );
        let rae = self.turn_of(self.rae);
        let mut args = json!({ "task": item.0, "notes": notes,
            "verdict": if approve { "approve" } else { "send_back" } });
        if approve {
            args["evidence"] = json!([[format!("r{}", self.reading(&rae))]]);
        }
        self.call(&rae, "review", &args);
        self.hq.runner().finish(rae.run, true);
    }

    fn lessons(&self) -> Vec<String> {
        self.hq.loop_lessons(self.project).unwrap()
    }
}

/// Given Bo's item, when Bo submits it saying a lesson and Rae sends it back with another,
/// then the loop keeps both, the worker's first; and Bo's next turn on it reads them right
/// after the item.
#[test]
fn a_send_back_keeps_the_lessons_said_and_the_next_turn_reads_them() {
    let clock = Hands(AtomicI64::new(NOON));
    let l = a_loop(&clock);
    let item = l.item("Find what stops gluetun");
    l.bo_submits(
        item,
        "Fixed the provider setting.\nLesson: gluetun's provider is set in roles/gluetun/defaults/main.yml.",
    );
    assert!(l.lessons().is_empty(), "nothing is kept before a verdict");
    l.rae_reviews(
        item,
        false,
        "The README misses two defaults.\n- Lesson: every default goes in the role's README with its value.",
    );
    assert_eq!(
        l.lessons(),
        [
            "gluetun's provider is set in roles/gluetun/defaults/main.yml.",
            "every default goes in the role's README with its value.",
        ]
    );
    let bo = l.turn_of(l.bo);
    let told = &bo.told;
    let lessons = told
        .find("Lessons this loop learned")
        .unwrap_or_else(|| panic!("the lessons are told: {told}"));
    let item_at = told.find("Your task: Find what stops gluetun").unwrap();
    let why_at = told.find("Why it exists:").unwrap();
    assert!(
        item_at < lessons && lessons < why_at,
        "right after the item: {told}"
    );
    assert!(told.contains("- every default goes in the role's README with its value.\n"));
}

/// Given an accepted item, the worker's last word and the reviewer's approval notes each carry
/// a lesson; a repeat, one too long and one holding a token are not kept.
#[test]
fn an_accepted_item_keeps_short_new_lessons_with_nothing_secret_in_them() {
    let clock = Hands(AtomicI64::new(NOON));
    let l = a_loop(&clock);
    let item = l.item("Pin the gluetun image");
    l.bo_submits(
        item,
        "Pinned.\nLesson: Pin images in each role's defaults/main.yml.",
    );
    // A last word of the worker's, as the worker posts it, after the turn that submitted.
    let rae = l.turn_of(l.rae);
    l.call(
        &rae,
        "post",
        &json!({ "task": item.0, "last_word": true,
                 "text": format!("Lesson: pin IMAGES in each role's defaults/main.yml\nLesson: {}\nLesson: the key is ghp_1234567890abcdefghijklmnopqrstuv",
                                 "x".repeat(LOOP_LESSON_MOST + 1)) }),
    );
    let reading = l.reading(&rae);
    l.call(
        &rae,
        "review",
        &json!({ "task": item.0, "verdict": "approve", "evidence": [[format!("r{reading}")]],
                 "notes": "Checked the tag.\nLesson: ansible-lint runs from the fork's root." }),
    );
    l.hq.runner().finish(rae.run, true);
    assert_eq!(
        l.lessons(),
        [
            "Pin images in each role's defaults/main.yml.",
            "ansible-lint runs from the fork's root.",
        ]
    );
}

/// The owner edits the list: each line checked like a lesson, and a full list refused.
#[test]
fn the_owner_edits_a_loops_lessons() {
    let clock = Hands(AtomicI64::new(NOON));
    let l = a_loop(&clock);
    let set =
        l.hq.set_loop_lessons(
            l.project,
            &["Lesson: one".to_owned(), String::new(), "two".to_owned()],
        )
        .unwrap();
    assert_eq!(set, ["one", "two"]);
    assert_eq!(l.lessons(), ["one", "two"]);
    let secret =
        l.hq.set_loop_lessons(l.project, &["sk-abcdefabcdefabcdefabcdef".to_owned()])
            .unwrap_err();
    assert!(secret.to_string().contains("secret"), "{secret}");
    let many: Vec<String> = (0..=LOOP_LESSONS_MOST).map(|i| format!("l {i}")).collect();
    assert!(l.hq.set_loop_lessons(l.project, &many).is_err());
    assert_eq!(
        l.lessons(),
        ["one", "two"],
        "a refused edit changes nothing"
    );
    assert!(l.hq.set_loop_lessons(l.project, &[]).unwrap().is_empty());
    assert!(l.lessons().is_empty());
}
