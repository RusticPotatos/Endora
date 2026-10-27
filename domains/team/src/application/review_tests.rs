//! What reaches the owner's desk lists the names the home lab does not have.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::json;

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, DeskItem, NewRoutine, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::application::desk_ports::DeskStore;
use crate::domain::routines::HOUR_MS;
use crate::domain::{Priority, Role, TaskId, TaskStatus};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

struct Team {
    task: TaskId,
}

/// A lead, a developer and (when `with_reviewer`) a reviewer; a goal, with the home lab's
/// routine when `lab`; and a task of the developer's under it, whose turn has started.
fn team(clock: &Hands, with_reviewer: bool, lab: bool) -> (Hq<'_>, Team, RunContext) {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    if with_reviewer {
        hq.hire("Grace", Role::Reviewer, 20, 2, 1).unwrap();
    }
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    if lab {
        hq.add_routine(
            Author::Owner,
            NewRoutine {
                title: "Check the home lab".to_owned(),
                description: "Look at the home lab with homelab_status.".to_owned(),
                goal,
                project: None,
                assignee: lead,
                every_ms: 6 * HOUR_MS,
                on_lab_trouble: true,
            },
        )
        .unwrap();
    }
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: "Look into the media apps".to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    hq.tick().unwrap();
    let context = hq
        .runner()
        .started()
        .into_iter()
        .find(|c| c.member == dev)
        .expect("the developer's turn");
    (hq, Team { task }, context)
}

/// Calls a tool as the turn `context` is.
fn use_tool(
    hq: &Hq<'_>,
    context: &RunContext,
    tool: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

fn questions(hq: &Hq<'_>) -> Vec<DeskItem> {
    hq.store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "question")
        .collect()
}

#[test]
fn with_no_reviewer_a_question_goes_straight_to_the_desk_still_checked() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, false, true);
    hq.set_lab_names(vec!["sonarr".to_owned()]);
    let said = use_tool(
        &hq,
        &context,
        "ask_owner",
        &json!({ "text": "Crappy_Cray failed a ping test. Restart it?" }),
    )
    .unwrap();
    assert!(said.contains("owner's desk"), "{said}");
    let asked = questions(&hq);
    assert_eq!(
        asked[0].text,
        "Linus asks: Crappy_Cray failed a ping test. Restart it?\nNot found in Endora's home \
         lab: Crappy_Cray"
    );
    assert_ne!(
        hq.store().task(t.task).unwrap().unwrap().status,
        TaskStatus::InReview
    );
}
