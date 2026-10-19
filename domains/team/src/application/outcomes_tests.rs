//! The outcomes (ADR 0132) over made-up rows: what the team's work came to, each number from the
//! record alone. Live, over 17 and 18 October, 201 turns and $8.85 delivered no accepted change,
//! and nothing on the screen showed it: these measures are how it will.

use serde_json::{Value, json};

use super::ports::Run;
use super::scorecard::Window;
use super::scorecard::outcomes::{
    Consults, Memory, OutcomeRecord, TalkWork, Waiting, outcomes, talk_work, waiting,
};
use super::scorecard_tests::{MINUTE, NOON, event, run, spending};
use crate::domain::consult::{Consult, Status};
use crate::domain::guards::{self, TurnSeen};
use crate::domain::money::DAY_MS;
use crate::domain::{Branch, Check, Event, Label, MemberId, RunId, RunStatus, TaskId};

const HOUR: i64 = 60 * MINUTE;

fn day() -> Window {
    Window::rolling(NOON, DAY_MS)
}

fn week() -> Window {
    Window::rolling(NOON, 7 * DAY_MS)
}

fn pushed(id: i64, at: i64, task: i64, ok: bool) -> Event {
    event(
        id,
        at,
        "branch.pushed",
        json!({ "task": task, "branch": format!("hq/task-{task}"), "head": "a1b2c3", "ok": ok,
                "receipt": id, "member": 4 }),
    )
}

fn added(id: i64, at: i64, kind: &str) -> Event {
    event(
        id,
        at,
        "desk.added",
        json!({ "item": id, "kind": kind, "task": 7 }),
    )
}

fn sent_back(id: i64, at: i64, reason: Option<&str>) -> Event {
    event(
        id,
        at,
        "task.sent_back",
        json!({ "task": 7, "by": 4, "reason": reason }),
    )
}

fn asked_the_record(id: i64, at: i64) -> Event {
    event(
        id,
        at,
        "record.asked",
        json!({ "run": 3, "member": 2, "task": 7, "found": 2 }),
    )
}

fn branch(id: i64, at: i64) -> Branch {
    Branch {
        id,
        task: TaskId(10 + id),
        run: RunId(1),
        member: MemberId(4),
        name: format!("hq/task-{id}"),
        head: String::new(),
        base: String::new(),
        files: 1,
        plus: 1,
        minus: 0,
        can_push: true,
        status: "submitted".to_owned(),
        at,
    }
}

fn check(id: i64, ok: Option<bool>, done_at: Option<i64>) -> Check {
    Check {
        id,
        task: TaskId(10),
        branch: 1,
        run: RunId(1),
        token: format!("t{id}"),
        status: if done_at.is_some() { "done" } else { "running" }.to_owned(),
        ok,
        summary: String::new(),
        queued_at: NOON - HOUR,
        started_at: None,
        done_at,
    }
}

fn consult(id: i64, status: Status, at: i64) -> Consult {
    Consult {
        id,
        asker: MemberId(2),
        asker_run: RunId(1),
        expert: MemberId(3),
        topic: None,
        thread: TaskId(7),
        question: "why does the indexer restart?".to_owned(),
        status,
        answer_run: None,
        why: String::new(),
        asker_woken: true,
        at,
    }
}

fn record<'a>(
    runs: &'a [Run],
    events: &'a [Event],
    branches: &'a [Branch],
    checks: &'a [Check],
    consults: &'a [Consult],
) -> OutcomeRecord<'a> {
    OutcomeRecord {
        runs,
        events,
        branches,
        checks,
        consults,
    }
}

fn nothing() -> OutcomeRecord<'static> {
    record(&[], &[], &[], &[], &[])
}

#[test]
fn rolling_windows_end_with_the_moment_they_are_asked_at_and_include_it() {
    let w = Window::rolling(NOON, DAY_MS);
    assert_eq!((w.from, w.until), (NOON - DAY_MS + 1, NOON + 1));
}

/// A change has landed when the node pushed to the fork what the owner approved at the push
/// card. The push's own outcome says so: an approval the node has not yet told the outcome of is
/// not a change that landed, and a push that failed never was one. A task counts once.
#[test]
fn a_change_has_landed_when_the_node_pushed_what_the_owner_approved_once_a_task() {
    let events = [
        pushed(1, NOON - 2 * HOUR, 10, true),
        // Pushed again, after a fix: the same change.
        pushed(2, NOON - HOUR, 10, true),
        pushed(3, NOON - HOUR, 11, false),
        pushed(4, NOON - 3 * DAY_MS, 12, true),
        // Before the week.
        pushed(5, NOON - 8 * DAY_MS, 13, true),
        event(
            6,
            NOON - MINUTE,
            "desk.decided",
            json!({ "item": 9, "kind": "push", "decision": "approve" }),
        ),
    ];
    let r = record(&[], &events, &[], &[], &[]);
    assert_eq!(outcomes(&r, &day()).landed, 1);
    assert_eq!(outcomes(&r, &week()).landed, 2);
    assert_eq!(outcomes(&nothing(), &week()).landed, 0, "none is nought");
}

#[test]
fn branches_submitted_and_checks_passed_are_counted_where_they_happened() {
    let branches = [
        branch(1, NOON - HOUR),
        branch(2, NOON - 2 * DAY_MS),
        branch(3, NOON - 9 * DAY_MS),
    ];
    let checks = [
        check(1, Some(true), Some(NOON - HOUR)),
        check(2, Some(false), Some(NOON - HOUR)),
        // Still running.
        check(3, None, None),
        check(4, Some(true), Some(NOON - 3 * DAY_MS)),
    ];
    let r = record(&[], &[], &branches, &checks, &[]);
    let d = outcomes(&r, &day());
    assert_eq!((d.submitted, d.checks_passed), (1, 1));
    let w = outcomes(&r, &week());
    assert_eq!((w.submitted, w.checks_passed), (2, 2));
}

/// What one landed change cost is the window's spend over what landed. With nothing landed it is
/// not $0 and not infinite: there is no cost to give, and the screen says so.
#[test]
fn a_landed_changes_cost_is_the_windows_spend_over_what_landed_and_none_when_nothing_did() {
    let runs = [
        spending(
            run(1, 2, Some(7), Some(NOON - HOUR), Some(Label::Advanced)),
            600_000,
        ),
        spending(
            run(2, 3, Some(8), Some(NOON - 2 * HOUR), Some(Label::Checked)),
            300_000,
        ),
        spending(
            run(
                3,
                2,
                Some(7),
                Some(NOON - 2 * DAY_MS),
                Some(Label::Advanced),
            ),
            100_000,
        ),
        // Still running: spent in no window yet.
        spending(run(4, 2, Some(7), None, None), 55_000),
    ];
    let spent = outcomes(&record(&runs, &[], &[], &[], &[]), &day());
    assert_eq!(spent.spent, 900_000);
    assert_eq!(
        spent.per_landed(),
        None,
        "money spent and nothing landed is not $0 each"
    );

    let events = [
        pushed(1, NOON - HOUR, 10, true),
        pushed(2, NOON - HOUR, 11, true),
        pushed(3, NOON - 2 * DAY_MS, 12, true),
    ];
    let landed = outcomes(&record(&runs, &events, &[], &[], &[]), &week());
    assert_eq!(
        (landed.landed, landed.spent, landed.per_landed()),
        (3, 1_000_000, Some(333_333))
    );
    let free = outcomes(&record(&[], &events, &[], &[], &[]), &week());
    assert_eq!(
        free.per_landed(),
        Some(0),
        "landed on the house's own models, which cost nothing"
    );
}

fn seen(label: Label) -> TurnSeen {
    TurnSeen {
        ended_at: NOON - HOUR,
        label,
        checking: false,
        acted: false,
        branch: false,
        criterion_met: false,
        new_evidence: false,
        asked: false,
        spent: 0,
    }
}

/// Talk against work is counted with the ceremony guard's own labels (ADR 0124), so the card and
/// the guard cannot disagree about a window.
#[test]
fn talk_and_work_are_the_ceremony_guards_own_counts_so_the_two_agree() {
    let acted = TurnSeen {
        acted: true,
        ..seen(Label::Advanced)
    };
    let review = TurnSeen {
        checking: true,
        ..seen(Label::Advanced)
    };
    let mut turns = vec![seen(Label::TalkOnly); 5];
    // A re-check with nothing new is talk; an empty turn is neither.
    turns.push(seen(Label::Checked));
    turns.push(review);
    turns.push(seen(Label::Empty));
    turns.extend(vec![acted; 2]);
    turns.push(TurnSeen {
        ended_at: NOON - 3 * DAY_MS,
        ..seen(Label::TalkOnly)
    });

    let d = talk_work(&turns, &[], &[], &day());
    let guard = guards::ceremony(&turns, NOON, DAY_MS);
    assert_eq!(
        (d.coordination, d.product),
        (guard.coordination, guard.product)
    );
    assert_eq!((d.coordination, d.product), (7, 2));

    let w = talk_work(&turns, &[], &[], &week());
    let guard = guards::ceremony(&turns, NOON, 7 * DAY_MS);
    assert_eq!(
        (w.coordination, w.product),
        (guard.coordination, guard.product)
    );
    assert_eq!(w.coordination, 8);
}

#[test]
fn posts_and_acts_are_counted_by_when_they_happened() {
    let posts = [NOON - MINUTE, NOON - HOUR, NOON - 2 * DAY_MS];
    let acts = [NOON - 2 * HOUR, NOON - 4 * DAY_MS];
    let d = talk_work(&[], &posts, &acts, &day());
    assert_eq!((d.posts, d.acts), (2, 1));
    let w = talk_work(&[], &posts, &acts, &week());
    assert_eq!((w.posts, w.acts), (3, 2));
    assert_eq!(talk_work(&[], &[], &[], &week()), TalkWork::default());
}

/// Of the desk's kinds, those that need the owner's decision interrupt; stuck work, a guard's
/// finding and a notice are for the digest. Live, 30 items in 38.6 hours, only six of them real
/// decisions or approvals.
#[test]
fn the_desks_interruptions_are_what_needs_a_decision_and_the_digest_is_the_rest() {
    let kinds = [
        "decision",
        "approval",
        "question",
        "push",
        "ready_to_merge",
        "project_cap",
        "project_budget",
        "out_for_day",
        "recall",
        "canon",
        "stuck",
        "guard",
        "notice",
    ];
    let mut events: Vec<Event> = kinds
        .iter()
        .zip(1..)
        .map(|(kind, id)| added(id, NOON - HOUR, kind))
        .collect();
    events.push(added(40, NOON - 3 * DAY_MS, "decision"));
    events.push(added(41, NOON - 3 * DAY_MS, "stuck"));
    let r = record(&[], &events, &[], &[], &[]);

    let d = outcomes(&r, &day());
    assert_eq!((d.interrupts, d.digest), (10, 3));
    assert_eq!(d.interrupts_a_day(), 10.0);
    let w = outcomes(&r, &week());
    assert_eq!((w.interrupts, w.digest), (11, 4));
    assert_eq!(w.interrupts_a_day(), 1.6, "eleven in seven days");
    assert_eq!(outcomes(&nothing(), &week()).interrupts_a_day(), 0.0);
}

#[test]
fn turns_that_wait_are_counted_by_what_they_wait_for_and_the_oldest_wait_is_said() {
    let waits = |id: i64, why: &str, queued: i64| {
        let mut r = run(id, 2, Some(7), None, None);
        r.status = RunStatus::Queued;
        r.queued_at = queued;
        r.started_at = None;
        r.end_reason = why.to_owned();
        r
    };
    let runs = [
        waits(
            1,
            "waiting for budget: the budget for it is spent",
            NOON - 30 * MINUTE,
        ),
        waits(2, "waiting for the Mac: owner active", NOON - 10 * MINUTE),
        waits(
            3,
            "waiting for budget: the budget for it is spent",
            NOON - 5 * MINUTE,
        ),
        // Queued and about to start: not a wait.
        waits(4, "", NOON - 3 * HOUR),
        // Under way.
        run(5, 2, Some(7), None, None),
    ];
    assert_eq!(
        waiting(&runs, NOON),
        Waiting {
            budget: 2,
            mac: 1,
            oldest_ms: Some(30 * MINUTE)
        }
    );
    assert_eq!(waiting(&[], NOON), Waiting::default());
}

#[test]
fn the_memory_measures_count_reinvented_work_asks_of_the_record_and_consults() {
    let events = [
        sent_back(1, NOON - HOUR, Some("exists_already")),
        sent_back(2, NOON - 2 * DAY_MS, Some("exists_already")),
        // Sent back for something else, or for no reason named.
        sent_back(3, NOON - HOUR, None),
        asked_the_record(4, NOON - HOUR),
        asked_the_record(5, NOON - 5 * MINUTE),
        asked_the_record(6, NOON - 2 * DAY_MS),
    ];
    let consults = [
        consult(1, Status::Queued, NOON - HOUR),
        consult(2, Status::Answered, NOON - 2 * HOUR),
        consult(3, Status::FromRecord, NOON - 3 * HOUR),
        consult(4, Status::Answered, NOON - 2 * DAY_MS),
    ];
    let r = record(&[], &events, &[], &[], &consults);
    assert_eq!(
        outcomes(&r, &day()).memory,
        Memory {
            reinvented: 1,
            record_asked: 2,
            consults: Consults {
                asked: 3,
                answered: 1,
                from_record: 1
            }
        }
    );
    assert_eq!(
        outcomes(&r, &week()).memory,
        Memory {
            reinvented: 2,
            record_asked: 3,
            consults: Consults {
                asked: 4,
                answered: 2,
                from_record: 1
            }
        }
    );
}

#[test]
fn the_json_says_nothing_landed_plainly_and_what_was_not_measured_as_null() {
    let shown = outcomes(&nothing(), &day()).json(None);
    assert_eq!(shown["landed"], json!(0));
    assert_eq!(shown["submitted"], json!(0));
    assert_eq!(shown["checks_passed"], json!(0));
    assert_eq!(shown["per_landed"], Value::Null, "no cost to give");
    assert_eq!(shown["talk"], Value::Null, "not measured yet");
    assert_eq!(shown["interrupts"], json!(0));
    assert_eq!(shown["digest"], json!(0));
    assert_eq!(shown["since"], Value::Null);
    assert_eq!(shown["memory"]["reinvented"], json!(0));
    assert_eq!(shown["memory"]["consults"]["from_record"], json!(0));

    let talk = TalkWork {
        coordination: 12,
        product: 1,
        posts: 40,
        acts: 6,
    };
    let events = [pushed(1, NOON - HOUR, 10, true)];
    let runs = [spending(
        run(1, 2, Some(7), Some(NOON - HOUR), Some(Label::Advanced)),
        880_000,
    )];
    let shown = outcomes(&record(&runs, &events, &[], &[], &[]), &day()).json(Some(&talk));
    assert_eq!(shown["landed"], json!(1));
    assert_eq!(shown["per_landed"], json!(880_000));
    assert_eq!(
        shown["talk"],
        json!({ "coordination": 12, "product": 1, "posts": 40, "acts": 6 })
    );
    assert_eq!(shown["from"], json!(NOON - DAY_MS + 1));
    assert_eq!(shown["until"], json!(NOON + 1));

    let waits = Waiting {
        budget: 2,
        mac: 0,
        oldest_ms: Some(5 * MINUTE),
    }
    .json();
    assert_eq!(
        waits,
        json!({ "budget": 2, "mac": 0, "oldest_ms": 5 * MINUTE })
    );
    assert_eq!(Waiting::default().json()["oldest_ms"], Value::Null);
}

/// Talk against work is measured by the process-health look and kept as it shows: what was kept
/// reads back whole, and anything else reads back as nothing.
#[test]
fn talk_against_work_reads_back_as_it_was_kept() {
    let talk = TalkWork {
        coordination: 12,
        product: 1,
        posts: 40,
        acts: 6,
    };
    assert_eq!(TalkWork::read(&talk.json()), Some(talk));
    assert_eq!(TalkWork::read(&json!({ "coordination": 3 })), None);
    assert_eq!(TalkWork::read(&Value::Null), None);
    assert_eq!(
        TalkWork::read(&json!({ "coordination": -1, "product": 0, "posts": 0, "acts": 0 })),
        None
    );
}

#[test]
fn a_record_shorter_than_the_window_says_where_it_starts() {
    let events = [pushed(1, NOON - 2 * HOUR, 10, true)];
    let r = record(&[], &events, &[], &[], &[]);
    assert_eq!(outcomes(&r, &day()).since, Some(NOON - 2 * HOUR));
    let early = [pushed(1, NOON - 10 * DAY_MS, 10, true)];
    assert_eq!(
        outcomes(&record(&[], &early, &[], &[], &[]), &week()).since,
        None
    );
    assert_eq!(outcomes(&nothing(), &week()).since, None);
}
