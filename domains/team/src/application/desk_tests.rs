//! A desk that interrupts only for decisions (ADR 0132): stuck work, a guard's finding and a notice
//! wait in a digest instead of each becoming a card on the owner's desk; one finding is kept for a
//! task and a guard however often it is seen; and what the owner has judged is not raised again
//! until there is new evidence. Live, over 17 and 18 October, 30 items reached the desk in 38.6
//! hours and six were real decisions; one task was flagged by the stuck guard four times, and a
//! ceremony finding came back three times after the owner marked it resolved.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::Value;

use super::engine::{Author, STRANDED_AFTER_MS};
use super::guard_ports::GuardStore;
use super::guards_tests::{Hands, Hq, NOON, blocked, later, team, turn};
use super::ports::TeamStore;
use crate::domain::guards::{Finding, Verdict};
use crate::domain::{MemberId, TaskId};

fn desk(hq: &Hq<'_>) -> Vec<Value> {
    hq.snapshot().unwrap()["desk"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn digest(hq: &Hq<'_>) -> Vec<Value> {
    hq.snapshot().unwrap()["digest"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn kinds(items: &[Value]) -> Vec<&str> {
    items.iter().filter_map(|d| d["kind"].as_str()).collect()
}

/// Linus's task, blocked with nobody to take it up, left alone until HQ puts it on the owner's
/// desk as stuck: nudged once, then stuck.
fn left_stuck(hq: &Hq<'_>, clock: &Hands, dev: MemberId, task: TaskId) {
    blocked(hq, dev, task, false);
    left_alone(hq, clock, dev);
}

/// HQ looks as time passes with nobody back on the work: a nudge, its turn over, and ten
/// minutes, twice, and once more.
fn left_alone(hq: &Hq<'_>, clock: &Hands, dev: MemberId) {
    for _ in 0..2 {
        later(clock, STRANDED_AFTER_MS + 1);
        hq.tick().unwrap();
        if let Some(t) = hq.runner().started().into_iter().rfind(|c| c.member == dev) {
            hq.runner().finish(t.run, true);
        }
        hq.tick().unwrap();
    }
    later(clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
}

fn stuck_key(task: TaskId) -> String {
    format!("stuck:{}", task.0)
}

fn stuck_finding(hq: &Hq<'_>, task: TaskId) -> Finding {
    hq.store()
        .latest_finding(&stuck_key(task))
        .unwrap()
        .expect("a stuck finding")
}

fn findings_of(hq: &Hq<'_>, key: &str) -> usize {
    hq.store()
        .findings()
        .unwrap()
        .iter()
        .filter(|f| f.key == key)
        .count()
}

fn stuck_cards(hq: &Hq<'_>, task: TaskId, status: &str) -> usize {
    hq.store()
        .desk()
        .unwrap()
        .iter()
        .filter(|d| d.kind == "stuck" && d.task == Some(task) && d.status == status)
        .count()
}

/// The owner writes in the thread and Linus's turn on it ends having done nothing: he looked, and
/// the work is where it was.
fn looked_at_and_left(hq: &Hq<'_>, dev: MemberId, task: TaskId) {
    hq.post(Author::Owner, task, "Try the indexer again.")
        .unwrap();
    let t = turn(hq, dev);
    hq.runner().finish(t.run, true);
    hq.tick().unwrap();
}

// ---- the digest ---------------------------------------------------------------------------------

/// What needs a decision is on the desk; stuck work, a finding and a notice are in the digest,
/// newest first, and acting on one takes it out.
#[test]
fn the_desk_keeps_what_needs_a_decision_and_the_digest_holds_the_rest_newest_first() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, true);
    let add = |kind: &str, text: &str| {
        later(&clock, 1_000);
        hq.store()
            .add_desk(
                kind,
                Some(task),
                None,
                Some(dev),
                text,
                clock.0.load(Ordering::SeqCst),
            )
            .unwrap()
    };
    let question = add("question", "Linus asks: steel or titanium?");
    let stuck = add(
        "stuck",
        "Stuck: Clear the stuck downloads. Nobody came back to it.",
    );
    let notice = add("notice", "Linus restarted sonarr: it was unhealthy.");
    let approval = add("approval", "Linus asks to run sonarr_remove_and_search.");
    let finding = add("guard", "Talk about work (finding 3): Upkeep: 14 turns");

    let on_the_desk = desk(&hq);
    assert_eq!(
        kinds(&on_the_desk),
        ["question", "approval"],
        "{on_the_desk:?}"
    );
    assert_eq!(on_the_desk[0]["id"], question.0);
    assert_eq!(on_the_desk[1]["id"], approval.0);
    let waiting = digest(&hq);
    assert_eq!(
        kinds(&waiting),
        ["guard", "notice", "stuck"],
        "newest first"
    );
    assert_eq!(
        waiting.iter().map(|d| d["id"].clone()).collect::<Vec<_>>(),
        [finding.0, notice.0, stuck.0]
    );
    assert!(
        waiting[0]["text"]
            .as_str()
            .unwrap()
            .starts_with("Talk about work"),
        "each item keeps its words and its choices: {}",
        waiting[0]
    );

    // Acting on an item takes it out of the digest.
    hq.decide(notice, "seen").unwrap();
    assert_eq!(kinds(&digest(&hq)), ["guard", "stuck"]);
    hq.decide(finding, "dismiss").unwrap();
    assert_eq!(kinds(&digest(&hq)), ["stuck"]);
    assert_eq!(
        kinds(&desk(&hq)),
        ["question", "approval"],
        "the desk was never theirs"
    );
}

// ---- one finding for a task and a guard -----------------------------------------------------------

/// The owner judged it, and nothing has moved since: stuck again is not raised again.
#[test]
fn stuck_work_the_owner_judged_is_not_raised_again_while_nothing_has_moved() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    left_stuck(&hq, &clock, dev, task);
    let first = stuck_finding(&hq, task);
    hq.judge_finding(first.id, Verdict::Resolved, None).unwrap();
    assert!(
        digest(&hq).is_empty(),
        "judging it took it out of the digest"
    );
    assert_eq!(stuck_cards(&hq, task, "waiting"), 0);

    // Still stuck, and seen so: no card and no second finding.
    for _ in 0..2 {
        later(&clock, STRANDED_AFTER_MS + 1);
        hq.tick().unwrap();
    }
    assert!(digest(&hq).is_empty());
    assert_eq!(findings_of(&hq, &stuck_key(task)), 1);

    // Linus looks, the work is where it was, and it is stuck again: still nothing new.
    looked_at_and_left(&hq, dev, task);
    left_alone(&hq, &clock, dev);
    assert_eq!(findings_of(&hq, &stuck_key(task)), 1, "no new finding");
    assert_eq!(stuck_cards(&hq, task, "waiting"), 0, "no new card");
    assert!(digest(&hq).is_empty());
    assert_eq!(
        stuck_finding(&hq, task).verdict,
        Some(Verdict::Resolved),
        "and what the owner said stands"
    );
}

// ---- the ceremony ---------------------------------------------------------------------------------
