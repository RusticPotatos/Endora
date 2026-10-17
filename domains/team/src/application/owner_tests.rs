//! The owner speaks to the team directly: a reply on any open task wakes whoever has it, at
//! once, mention or not; and a goal's notes from the owner reach every briefing under it.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::json;

use super::engine::{Author, Engine, EngineError, Settings};
use super::ports::{Clock, NewTask, TeamStore};
use super::protocol::call;
use crate::domain::{
    GoalId, MemberId, Priority, Receipt, Refusal, Role, TaskId, TaskStatus, WakeReason,
};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const NOON: i64 = 1_792_166_400_000;

/// HQ with a lead, Linus, and a task handed to Linus under a goal; nothing started yet.
fn office(clock: &Hands) -> (Hq<'_>, MemberId, GoalId, TaskId) {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let goal = hq
        .set_goal(
            "Keep the home lab healthy",
            "you asked for it",
            Author::Owner,
        )
        .unwrap();
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: "Look at the music stack".to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    (hq, dev, goal, task)
}

fn last_wake(hq: &Hq<'_>, member: MemberId) -> (WakeReason, Option<TaskId>, Receipt) {
    let w = hq
        .store()
        .wakes()
        .unwrap()
        .into_iter()
        .rfind(|w| w.member == member)
        .unwrap();
    (w.reason, w.task, w.receipt)
}

#[test]
fn the_owners_reply_on_a_task_wakes_whoever_has_it_mention_or_not() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, _, task) = office(&clock);
    // His first turn runs and ends; the task sits with him.
    hq.tick().unwrap();
    let first = hq.runner().started()[0].clone();
    hq.runner().finish(first.run, true);
    hq.tick().unwrap();
    let before = hq.store().wakes().unwrap().len();
    hq.post(Author::Owner, task, "Leave the old player alone.")
        .unwrap();
    assert!(
        hq.store().wakes().unwrap().len() > before,
        "someone is woken"
    );
    let (reason, on, receipt) = last_wake(&hq, dev);
    assert_eq!((reason, on), (WakeReason::OwnerReplied, Some(task)));
    assert!(
        matches!(receipt, Receipt::Queued | Receipt::Folded),
        "{receipt:?}"
    );
    assert!(reason.brings_news(), "never held back by the back-off");
    assert_eq!(
        WakeReason::from_name("owner_replied"),
        Some(WakeReason::OwnerReplied)
    );
    let said = hq.store().thread(task).unwrap().pop().unwrap();
    assert_eq!(
        (said.author.as_str(), said.text.as_str()),
        ("owner", "Leave the old player alone.")
    );
}

#[test]
fn a_reply_on_a_finished_task_is_refused_and_endora_wakes_nobody_by_posting() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, _, task) = office(&clock);
    hq.tick().unwrap();
    let before = hq.store().wakes().unwrap().len();
    hq.post(Author::Endora, task, "sonarr became unhealthy.")
        .unwrap();
    assert_eq!(
        hq.store().wakes().unwrap().len(),
        before,
        "only the owner's words wake whoever has the task"
    );
    hq.store()
        .set_task_status(task, TaskStatus::Dropped)
        .unwrap();
    assert_eq!(
        hq.post(Author::Owner, task, "Too late?"),
        Err(EngineError::Refused(Refusal::Finished))
    );
    let _ = dev;
}

#[test]
fn a_goals_notes_from_the_owner_reach_every_briefing_under_it() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, _, goal, task) = office(&clock);
    let note = "The old music stack is being retired; ignore it.";
    hq.set_goal_note(goal, &format!("  {note}  ")).unwrap();
    assert_eq!(hq.goal_note(goal).unwrap(), note);
    assert_eq!(
        hq.set_goal_note(goal, &"x".repeat(2_001)),
        Err(EngineError::Refused(Refusal::TooLong(2_000)))
    );
    assert!(hq.set_goal_note(GoalId(99), "nothing").is_err());
    hq.tick().unwrap();
    let context = hq.runner().started()[0].clone();
    let (run, me) = hq.turn_of(&context.token).unwrap().unwrap();
    let briefing = call(&hq, &run, &me, "my_turn", &json!({})).unwrap();
    let lines: Vec<&str> = briefing.lines().collect();
    let at = lines
        .iter()
        .position(|l| *l == format!("Notes from the owner for this goal: {note}"))
        .unwrap_or_else(|| panic!("{briefing}"));
    assert!(at <= 4, "near the top: {briefing}");
    let snap = hq.snapshot().unwrap();
    assert_eq!(snap["goals"][0]["note"], note);
    // Cleared, it is gone.
    hq.set_goal_note(goal, "").unwrap();
    assert_eq!(hq.goal_note(goal).unwrap(), "");
    let _ = task;
}

#[test]
fn the_owner_sets_what_a_member_thinks_with_within_its_roles_brains() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, _, _) = office(&clock);
    let lead = MemberId(1);
    let brain = |m: MemberId| {
        hq.store()
            .members()
            .unwrap()
            .into_iter()
            .find(|x| x.id == m)
            .unwrap()
            .brain
    };
    assert_eq!(brain(lead), crate::domain::Brain::Local, "local unless set");
    assert_eq!(
        hq.set_brain(lead, "claude-sonnet-5-5").unwrap(),
        crate::domain::Brain::Sonnet
    );
    assert_eq!(brain(lead), crate::domain::Brain::Sonnet);
    for refused in ["claude-opus-5-5", "gpt-5", ""] {
        assert!(
            matches!(
                hq.set_brain(dev, refused),
                Err(EngineError::Refused(Refusal::NotABrain { .. }))
            ),
            "{refused}"
        );
    }
    assert_eq!(
        brain(dev),
        crate::domain::Brain::Local,
        "a refusal changes nothing"
    );
    let researcher = hq.hire("Marie", Role::Researcher, 15, 1, 2).unwrap();
    assert!(hq.set_brain(researcher, "claude-sonnet-5-5").is_err());
    hq.set_brain(researcher, "claude-haiku-5-5").unwrap();
    assert!(matches!(
        hq.set_brain(MemberId(99), "local"),
        Err(EngineError::Unknown(_))
    ));
    // The snapshot says each member's brain, and what the owner may choose for its role.
    let snap = hq.snapshot().unwrap();
    let ada = &snap["members"][0];
    assert_eq!(ada["brain"], "claude-sonnet-5-5");
    assert_eq!(
        ada["brains"],
        json!([{ "name": "local", "label": "Local" },
               { "name": "claude-sonnet-5-5", "label": "Claude Sonnet" },
               { "name": "claude-haiku-5-5", "label": "Claude Haiku" }])
    );
    let marie = &snap["members"][2];
    assert_eq!(marie["brains"].as_array().unwrap().len(), 2);
    assert_eq!(hq.store().count_events("member.brain").unwrap().0, 2);
}
