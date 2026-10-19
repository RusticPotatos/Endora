//! The coordination budget (ADR 0133), through the engine: a task that has taken more
//! coordination turns than its budget in a day, with nothing changing, has the talk that would
//! start more turns held, and its lead asked once to narrow it, split it or drop it. Tried in
//! shadow first: it records what it would have held and holds nothing until the owner switches it
//! on. Live, 17 and 18 October 2026, one docs branch took 44 turns, 80 posts and $2.47.

use std::sync::atomic::AtomicI64;

use serde_json::json;

use super::engine::{Author, Engine, EngineError, Settings};
use super::guard_ports::GuardStore;
use super::guards_tests::{
    Hands, Hq, NOON, guards, later, loose_task, one_turn, paid_talk, team, turn, use_tool,
    wakes_for,
};
use super::ports::TeamStore;
use crate::domain::guards::{Finding, Guard, GuardMode, THRESHOLDS};
use crate::domain::{MemberId, Receipt, Refusal, Refused, Role, RunStatus, TaskId, WakeReason};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

/// Ada, the lead, holds the owner's task outside any project; Linus, who reports to her, is the
/// one who keeps talking about it.
fn talkative(clock: &Hands) -> (Hq<'_>, MemberId, MemberId, TaskId) {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let linus = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    let task = loose_task(&hq, goal, "Plan the backups", ada, None);
    // Ada's first turn on it comes and goes: nothing is live on it afterwards.
    settle(&hq);
    (hq, ada, linus, task)
}

/// `n` turns of `who`'s on the task, each only a post.
fn talk(hq: &Hq<'_>, who: MemberId, task: TaskId, n: usize) {
    for _ in 0..n {
        one_turn(hq, who, task, false);
    }
}

/// How many posts of HQ's in the task's thread hold `text`.
fn said(hq: &Hq<'_>, task: TaskId, text: &str) -> usize {
    hq.store()
        .thread(task)
        .unwrap()
        .iter()
        .filter(|p| p.author == "endora" && p.text.contains(text))
        .count()
}

/// The findings the budget of talk has made.
fn budget_findings(hq: &Hq<'_>) -> Vec<Finding> {
    hq.store()
        .findings()
        .unwrap()
        .into_iter()
        .filter(|f| f.guard == Guard::Coordination)
        .collect()
}

/// The budget's cards waiting in the owner's digest.
fn budget_cards(hq: &Hq<'_>) -> usize {
    hq.store()
        .desk()
        .unwrap()
        .iter()
        .filter(|d| d.status == "waiting" && d.text.starts_with("Coordination over budget"))
        .count()
}

/// Starts what is queued, ends every turn that runs, as its worker would, and lets HQ see it.
fn settle(hq: &Hq<'_>) {
    hq.tick().unwrap();
    for run in hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .filter(|r| r.status == RunStatus::Running)
    {
        hq.runner().finish(run.id, true);
    }
    hq.tick().unwrap();
}

/// The wake that was held for `who`, if one is.
fn held_wake(hq: &Hq<'_>, who: MemberId) -> Option<crate::application::ports::Wake> {
    hq.wakes()
        .unwrap()
        .into_iter()
        .find(|w| w.member == who && w.receipt == Receipt::Held)
}

/// The twenty-second ask for talk, with the budget on: held, and Ada, who leads it, asked.
fn held_and_asked(hq: &Hq<'_>, linus: MemberId, task: TaskId) {
    assert_eq!(
        hq.wake(linus, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Held
    );
    assert_eq!(said(hq, task, "over its budget"), 1, "the lead is asked");
}

#[test]
fn in_shadow_a_task_over_its_budget_is_found_and_costed_and_nothing_is_held() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, linus, task) = talkative(&clock);
    assert_eq!(
        hq.guard_mode(Guard::Coordination).unwrap(),
        GuardMode::Shadow,
        "it starts in shadow"
    );
    // Fifteen turns of Linus's talk and six of Ada's at five cents each: twenty-one, one over.
    talk(&hq, linus, task, 15);
    for _ in 0..6 {
        paid_talk(&hq, ada, task, 50_000);
    }
    later(&clock, THRESHOLDS.look_every_ms);
    hq.tick().unwrap();

    let found = budget_findings(&hq);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].key, format!("coordination:{}", task.0));
    assert_eq!(found[0].task, Some(task.0));
    assert_eq!(
        found[0].evidence,
        format!(
            "would have held coordination on #{} (budget 20, 21 turns, $0.30)",
            task.0
        )
    );
    // The owner's digest has it, and so does the Guards card, which says the guard is in shadow.
    assert_eq!(budget_cards(&hq), 1);
    let digest = hq.snapshot().unwrap()["digest"].clone();
    assert!(
        digest
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["finding"]["guard"] == "coordination"),
        "{digest}"
    );
    let card = guards(&hq);
    let row = card["precision"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["guard"] == "coordination")
        .cloned()
        .expect("the guard is on the card");
    assert_eq!(row["mode"], "shadow");
    assert_eq!(card["thresholds"]["coordination_budget"], 20);
    assert!(
        card["findings"]
            .to_string()
            .contains("would have held coordination"),
        "{card}"
    );

    // Nothing is held and nobody is asked: more talk starts a turn as it always did.
    settle(&hq);
    assert_eq!(said(&hq, task, "over its budget"), 0);
    assert_eq!(
        hq.wake(linus, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Queued
    );
    assert!(held_wake(&hq, linus).is_none());
}

#[test]
fn switched_on_it_holds_more_talk_puts_one_decision_to_the_lead_and_lets_go_when_she_narrows() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, linus, task) = talkative(&clock);
    hq.set_guard_mode(Guard::Coordination, GuardMode::On)
        .unwrap();
    talk(&hq, linus, task, 21);

    held_and_asked(&hq, linus, task);
    let held = held_wake(&hq, linus).expect("a held wake");
    assert_eq!(
        wakes_for(&hq, task, WakeReason::Guard),
        1,
        "Ada is woken to decide it"
    );
    assert!(
        hq.events_after(0)
            .unwrap()
            .iter()
            .any(|e| e.kind == "wake.held" && e.data["why"] == "coordination_budget"),
        "the feed says why it was held"
    );
    // The ask is in the thread, with the three ways out.
    let ask = hq
        .store()
        .thread(task)
        .unwrap()
        .into_iter()
        .rfind(|p| p.author == "endora" && p.text.contains("over its budget"))
        .unwrap();
    for way in ["narrow_criteria", "create_task", "decide_stuck"] {
        assert!(ask.text.contains(way), "{way}: {}", ask.text);
    }
    assert!(ask.text.contains("held, not lost"), "{}", ask.text);
    // The finding is the card in the digest, and says it held.
    let found = budget_findings(&hq);
    assert_eq!(found.len(), 1);
    assert!(
        found[0].evidence.starts_with(&format!(
            "held coordination on #{} (budget 20, 21 turns, $0.00)",
            task.0
        )) && found[0].evidence.contains("Ada was asked"),
        "{}",
        found[0].evidence
    );

    // A second ask for talk from the same member is the first's duplicate: no second decision.
    assert_eq!(
        hq.wake(linus, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Refused(Refused::Duplicate)
    );
    assert_eq!(said(&hq, task, "over its budget"), 1);
    assert_eq!(wakes_for(&hq, task, WakeReason::Guard), 1);
    // Nor does HQ's next look at the team: one decision, however long the task stays over.
    settle(&hq);
    later(&clock, THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    assert_eq!(said(&hq, task, "over its budget"), 1);
    assert_eq!(budget_findings(&hq).len(), 1, "and one finding");
    assert_eq!(held_wake(&hq, linus).map(|w| w.id), Some(held.id));

    // Ada narrows it: the hold is lifted and Linus's held wake starts its turn, not lost.
    let a = turn(&hq, ada);
    use_tool(
        &hq,
        &a,
        "narrow_criteria",
        &json!({ "task": task.0, "criteria": [{ "text": "The plan names three backups" }],
                 "why": "the plan was the ask, not the debate" }),
    )
    .unwrap();
    hq.runner().finish(a.run, true);
    hq.tick().unwrap();
    let after = hq
        .wakes()
        .unwrap()
        .into_iter()
        .find(|w| w.id == held.id)
        .unwrap();
    assert_eq!(after.receipt, Receipt::Started, "{after:?}");
    assert!(hq.store().held_wakes(task).unwrap().is_empty());
}

#[test]
fn a_split_or_a_decision_by_the_lead_lifts_the_hold_too() {
    for how in ["split", "proceed"] {
        let clock = Hands(AtomicI64::new(NOON));
        let (hq, ada, linus, task) = talkative(&clock);
        hq.set_guard_mode(Guard::Coordination, GuardMode::On)
            .unwrap();
        talk(&hq, linus, task, 21);
        held_and_asked(&hq, linus, task);
        let held = held_wake(&hq, linus).unwrap();
        let a = turn(&hq, ada);
        let done = match how {
            "split" => use_tool(
                &hq,
                &a,
                "create_task",
                &json!({ "title": "Draft the plan", "description": "Write the backup plan",
                         "assignee": "Linus",
                         "criteria": [{ "text": "The plan is written" }] }),
            ),
            _ => use_tool(
                &hq,
                &a,
                "decide_stuck",
                &json!({ "task": task.0, "decision": "proceed",
                         "detail": "One plan, and no more debate.",
                         "evidence": [format!("task {}", task.0)] }),
            ),
        };
        done.unwrap_or_else(|why| panic!("{how}: {why}"));
        hq.runner().finish(a.run, true);
        hq.tick().unwrap();
        let after = hq
            .wakes()
            .unwrap()
            .into_iter()
            .find(|w| w.id == held.id)
            .unwrap();
        assert_eq!(after.receipt, Receipt::Started, "{how}: {after:?}");
    }
}

#[test]
fn a_member_who_was_not_asked_cannot_decide_it_as_if_it_were_stuck() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, linus, task) = talkative(&clock);
    // The budget is off, so nobody was asked: Ada's decide_stuck is refused, as it always was.
    talk(&hq, linus, task, 21);
    hq.wake(ada, Some(task), WakeReason::Guard).unwrap();
    let a = turn(&hq, ada);
    let refused = use_tool(
        &hq,
        &a,
        "decide_stuck",
        &json!({ "task": task.0, "decision": "proceed", "detail": "Carry on.",
                 "evidence": [format!("task {}", task.0)] }),
    )
    .unwrap_err();
    assert_eq!(refused, Refusal::NotStuckForYou.to_string());
}

#[test]
fn a_change_to_the_product_starts_the_count_again() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, _ada, linus, task) = talkative(&clock);
    hq.set_guard_mode(Guard::Coordination, GuardMode::On)
        .unwrap();
    talk(&hq, linus, task, 15);
    // A turn that restarts something is a change to the product.
    one_turn(&hq, linus, task, true);
    talk(&hq, linus, task, 15);
    assert_eq!(
        hq.wake(linus, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Queued,
        "thirty turns of talk in all, fifteen since anything changed"
    );
    settle(&hq);
    talk(&hq, linus, task, 5);
    assert_eq!(
        hq.wake(linus, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Queued,
        "twenty since"
    );
    settle(&hq);
    one_turn(&hq, linus, task, false);
    assert_eq!(
        hq.wake(linus, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Held,
        "the twenty-first since is over"
    );
}

#[test]
fn talk_from_before_the_window_is_forgotten_and_what_was_held_goes_when_the_day_has_passed() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, _ada, linus, task) = talkative(&clock);
    hq.set_guard_mode(Guard::Coordination, GuardMode::On)
        .unwrap();
    talk(&hq, linus, task, 21);
    held_and_asked(&hq, linus, task);
    let held = held_wake(&hq, linus).unwrap();
    // Ada's turn comes and goes with nothing decided: the wake stays held, not lost.
    settle(&hq);
    assert_eq!(held_wake(&hq, linus).map(|w| w.id), Some(held.id));
    // A day and a millisecond later the talk has aged out of the window, and the wake that was
    // held starts its turn on the next tick.
    later(&clock, THRESHOLDS.coordination_window_ms + 1);
    hq.tick().unwrap();
    let after = hq
        .wakes()
        .unwrap()
        .into_iter()
        .find(|w| w.id == held.id)
        .unwrap();
    assert_eq!(after.receipt, Receipt::Started, "{after:?}");
}

#[test]
fn where_nobody_leads_the_work_nothing_is_held_for_there_is_nobody_to_decide_it() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, linus, task) = team(&clock, false);
    hq.set_guard_mode(Guard::Coordination, GuardMode::On)
        .unwrap();
    talk(&hq, linus, task, 21);
    assert_eq!(
        hq.wake(linus, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Queued
    );
    later(&clock, THRESHOLDS.look_every_ms);
    settle(&hq);
    let found = budget_findings(&hq);
    assert_eq!(found.len(), 1, "it is still found: {found:?}");
    assert!(
        found[0].evidence.contains("nobody leads it"),
        "{}",
        found[0].evidence
    );
    assert_eq!(said(&hq, task, "over its budget"), 0);
}

#[test]
fn the_owner_switches_the_budget_on_and_back_and_a_guard_that_only_records_has_no_mode() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, _ada, linus, task) = talkative(&clock);
    assert_eq!(
        hq.set_guard_mode(Guard::Ceremony, GuardMode::Shadow),
        Err(EngineError::Refused(Refusal::NoShadowMode("ceremony"))),
        "it only records, so it has no shadow"
    );
    assert_eq!(hq.guard_mode(Guard::Ceremony).unwrap(), GuardMode::On);
    hq.set_guard_mode(Guard::Coordination, GuardMode::On)
        .unwrap();
    assert_eq!(
        hq.guard_mode(Guard::Coordination).unwrap(),
        GuardMode::On,
        "it is kept"
    );
    let feed: Vec<_> = hq
        .events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "guard.mode")
        .map(|e| e.data)
        .collect();
    assert_eq!(feed, [json!({ "guard": "coordination", "mode": "on" })]);

    // Back to shadow, what was held goes at the next tick.
    talk(&hq, linus, task, 21);
    held_and_asked(&hq, linus, task);
    let held = held_wake(&hq, linus).unwrap();
    settle(&hq);
    assert_eq!(
        held_wake(&hq, linus).map(|w| w.id),
        Some(held.id),
        "still held"
    );
    hq.set_guard_mode(Guard::Coordination, GuardMode::Shadow)
        .unwrap();
    hq.tick().unwrap();
    let after = hq
        .wakes()
        .unwrap()
        .into_iter()
        .find(|w| w.id == held.id)
        .unwrap();
    assert_eq!(after.receipt, Receipt::Started, "{after:?}");
}

#[test]
fn a_finding_the_owner_judged_is_quiet_until_another_budget_of_talk() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, _ada, linus, task) = talkative(&clock);
    talk(&hq, linus, task, 21);
    later(&clock, THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    settle(&hq);
    assert_eq!(budget_cards(&hq), 1);
    let first = budget_findings(&hq)[0].id;
    hq.judge_finding(first, crate::domain::guards::Verdict::Real, None)
        .unwrap();
    assert_eq!(budget_cards(&hq), 0, "the owner acted on it");

    // Five more turns of talk is not another budget of it.
    talk(&hq, linus, task, 5);
    later(&clock, THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    settle(&hq);
    assert_eq!(budget_cards(&hq), 0);
    assert_eq!(budget_findings(&hq).len(), 1, "the same finding");

    // Another twenty is.
    talk(&hq, linus, task, 15);
    later(&clock, THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    assert_eq!(budget_findings(&hq).len(), 2, "renewed");
    assert_eq!(budget_cards(&hq), 1);
}

#[test]
fn an_owners_word_and_a_decision_reach_a_task_whose_talk_is_held() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, linus, task) = talkative(&clock);
    hq.set_guard_mode(Guard::Coordination, GuardMode::On)
        .unwrap();
    talk(&hq, linus, task, 21);
    held_and_asked(&hq, linus, task);
    settle(&hq);
    // The owner writes in the thread: whoever has the task is woken, held or not.
    hq.post(Author::Owner, task, "How is the plan coming?")
        .unwrap();
    let owner_wake = hq
        .wakes()
        .unwrap()
        .into_iter()
        .rfind(|w| w.reason == WakeReason::OwnerReplied)
        .expect("the owner's word wakes the holder");
    assert_eq!(owner_wake.member, ada);
    assert_ne!(owner_wake.receipt, Receipt::Held, "{owner_wake:?}");
}
