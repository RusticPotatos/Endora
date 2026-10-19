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
use super::guards_tests::{
    Hands, Hq, NOON, blocked, guards, later, one_turn, project_work, team, turn,
};
use super::ports::TeamStore;
use crate::domain::guards::{Finding, Guard, THRESHOLDS, Verdict};
use crate::domain::{MemberId, TaskId, TaskStatus};

/// A fortnight, longer than any window the guards look over.
const FORTNIGHT: i64 = 14 * 24 * 60 * 60 * 1000;

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

/// The owner writes and Linus is back on the work, moves it on, and his turn ends: it has moved.
fn moved_on(hq: &Hq<'_>, dev: MemberId, task: TaskId) {
    hq.post(Author::Owner, task, "Try the indexer again.")
        .unwrap();
    let t = turn(hq, dev);
    hq.set_status_in(t.run, task, TaskStatus::Doing, "the indexer is back")
        .unwrap();
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

/// Stuck work with no lead to take it is one card in the digest, linked to the one finding, and
/// leaves the desk free for what the owner has to decide.
#[test]
fn stuck_work_waits_in_the_digest_beside_its_finding_and_does_not_interrupt() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    left_stuck(&hq, &clock, dev, task);
    assert_eq!(stuck_cards(&hq, task, "waiting"), 1);
    assert!(desk(&hq).is_empty(), "nothing interrupts: {:?}", desk(&hq));
    let waiting = digest(&hq);
    assert_eq!(kinds(&waiting), ["stuck"], "{waiting:?}");
    assert_eq!(waiting[0]["task"], task.0);
    let finding = stuck_finding(&hq, task);
    assert_eq!(
        waiting[0]["finding"]["id"], finding.id,
        "the card says which finding it is"
    );
    assert_eq!(waiting[0]["finding"]["episodes"], 1);
}

// ---- one finding for a task and a guard -----------------------------------------------------------

/// Seen stuck again while it is stuck, the one finding is refreshed: its time, not a second
/// finding and not a second card.
#[test]
fn work_seen_stuck_again_refreshes_its_one_finding_and_puts_up_no_second_card() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    left_stuck(&hq, &clock, dev, task);
    let first = stuck_finding(&hq, task);
    assert_eq!((first.episodes, first.seen_at), (1, first.opened_at));

    for _ in 0..3 {
        later(&clock, STRANDED_AFTER_MS + 1);
        hq.tick().unwrap();
    }
    let again = stuck_finding(&hq, task);
    assert_eq!(findings_of(&hq, &stuck_key(task)), 1, "one finding");
    assert_eq!(again.id, first.id);
    assert!(
        again.seen_at > first.seen_at,
        "seen again: {} after {}",
        again.seen_at,
        first.seen_at
    );
    assert_eq!(again.episodes, 1, "the same episode");
    assert_eq!(again.opened_at, first.opened_at);
    assert_eq!(stuck_cards(&hq, task, "waiting"), 1, "one card");
    assert_eq!(digest(&hq).len(), 1);
}

/// Work that cleared, with nobody having judged it, and is stuck again opens the same finding
/// again, with a card of its own: one finding for the task, however often it stops and starts.
#[test]
fn work_that_stops_again_after_a_look_opens_its_finding_again_not_a_second_one() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    left_stuck(&hq, &clock, dev, task);
    let first = stuck_finding(&hq, task);
    looked_at_and_left(&hq, dev, task);
    let cleared = stuck_finding(&hq, task);
    assert!(
        cleared.cleared_at.is_some(),
        "it closed itself when Linus looked"
    );
    assert_eq!(stuck_cards(&hq, task, "waiting"), 0);
    assert_eq!(stuck_cards(&hq, task, "cleared"), 1);

    left_alone(&hq, &clock, dev);
    let again = stuck_finding(&hq, task);
    assert_eq!(
        findings_of(&hq, &stuck_key(task)),
        1,
        "one finding for the task"
    );
    assert_eq!(again.id, first.id);
    assert_eq!(again.cleared_at, None, "open again");
    assert_eq!(again.episodes, 2);
    assert_eq!(
        stuck_cards(&hq, task, "waiting"),
        1,
        "and in the digest again"
    );
    assert_eq!(digest(&hq).len(), 1);
}

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

/// It moved, and then it stopped again: a new episode after progress is new evidence, and is
/// raised.
#[test]
fn stuck_work_the_owner_judged_is_raised_again_once_it_has_moved_and_stopped_again() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    left_stuck(&hq, &clock, dev, task);
    let first = stuck_finding(&hq, task);
    hq.judge_finding(first.id, Verdict::Real, None).unwrap();

    moved_on(&hq, dev, task);
    assert!(
        stuck_finding(&hq, task).cleared_at.is_some(),
        "it moved, so the finding closed"
    );
    left_alone(&hq, &clock, dev);
    assert_eq!(
        findings_of(&hq, &stuck_key(task)),
        2,
        "a new episode, a new finding"
    );
    let second = stuck_finding(&hq, task);
    assert_ne!(second.id, first.id);
    assert_eq!(second.verdict, None);
    assert!(
        second.mark > first.mark,
        "{} after {}",
        second.mark,
        first.mark
    );
    assert_eq!(stuck_cards(&hq, task, "waiting"), 1);
    assert_eq!(digest(&hq).len(), 1);
}

// ---- the ceremony ---------------------------------------------------------------------------------

fn talk(hq: &Hq<'_>, ada: MemberId, task: TaskId, turns: usize) {
    for _ in 0..turns {
        one_turn(hq, ada, task, false);
    }
}

fn look(hq: &Hq<'_>, clock: &Hands) {
    later(clock, THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
}

/// The cards the ceremony guard put up, in `status`: the budget of talk has its own (ADR 0133).
fn talk_cards(hq: &Hq<'_>, status: &str) -> usize {
    hq.store()
        .desk()
        .unwrap()
        .iter()
        .filter(|d| d.kind == "guard" && d.status == status)
        .filter(|d| d.text.starts_with("Talk about work"))
        .count()
}

/// Seen again with more talk, the finding is refreshed and its card says the new numbers.
#[test]
fn a_finding_seen_again_says_what_it_fires_on_now_on_its_card() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, task) = project_work(&clock);
    talk(&hq, ada, task, 12);
    look(&hq, &clock);
    let first = hq.store().findings().unwrap().remove(0);
    assert!(
        first
            .evidence
            .contains("12 coordination turns, 0 product changes")
    );
    assert_eq!(first.mark, 12, "the week's coordination turns");

    talk(&hq, ada, task, 3);
    look(&hq, &clock);
    let seen = hq.store().findings().unwrap();
    assert_eq!(seen.len(), 1, "still the one finding");
    assert!(
        seen[0].evidence.contains("15 coordination turns"),
        "{}",
        seen[0].evidence
    );
    assert!(seen[0].seen_at > first.seen_at);
    assert_eq!(
        seen[0].mark, 15,
        "an unjudged finding rests on the most it has seen"
    );
    let waiting = digest(&hq);
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert!(
        waiting[0]["text"]
            .as_str()
            .unwrap()
            .contains("15 coordination turns"),
        "the card says it now: {}",
        waiting[0]["text"]
    );
}

/// The owner's verdict from the Guards card is the owner acting on it, and takes the finding's
/// card out of the digest.
#[test]
fn the_owners_verdict_on_a_finding_takes_its_card_out_of_the_digest() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, task) = project_work(&clock);
    talk(&hq, ada, task, 12);
    look(&hq, &clock);
    assert_eq!(digest(&hq).len(), 1);
    let finding = hq.store().findings().unwrap().remove(0);
    hq.judge_finding(finding.id, Verdict::FalsePositive, None)
        .unwrap();
    assert!(digest(&hq).is_empty(), "{:?}", digest(&hq));
    assert_eq!(talk_cards(&hq, "waiting"), 0);
    assert_eq!(talk_cards(&hq, "false_positive"), 1);
    assert_eq!(
        hq.store().findings().unwrap()[0].verdict,
        Some(Verdict::FalsePositive)
    );
}

/// The owner marked talk about work resolved. It stays quiet while it is the same talk, and a
/// dozen more turns of it is new evidence: live, it came back three times after being resolved.
#[test]
fn a_resolved_ceremony_finding_is_raised_again_only_after_a_dozen_more_turns_of_talk() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, task) = project_work(&clock);
    talk(&hq, ada, task, 12);
    look(&hq, &clock);
    let first = hq.store().findings().unwrap().remove(0);
    assert_eq!(talk_cards(&hq, "waiting"), 1);
    hq.judge_finding(first.id, Verdict::Resolved, None).unwrap();
    assert_eq!(first.mark, 12);

    // Everything ages out, the finding clears, and the same talk comes back: twelve turns again
    // are not twelve more than were judged.
    later(&clock, FORTNIGHT);
    hq.tick().unwrap();
    look(&hq, &clock);
    assert!(
        hq.store().findings().unwrap()[0].cleared_at.is_some(),
        "under the line, it cleared"
    );
    talk(&hq, ada, task, 12);
    look(&hq, &clock);
    assert_eq!(
        hq.store().findings().unwrap().len(),
        1,
        "nothing was raised again"
    );
    assert_eq!(talk_cards(&hq, "waiting"), 0);
    assert!(digest(&hq).is_empty());
    let after = guards(&hq)["ceremony"][0]["over"].clone();
    assert_eq!(
        after, true,
        "it is over the line, and the owner has judged that"
    );

    // A dozen more: new evidence. (Twenty-four turns of talk is over a task's budget too, which
    // has its own finding and card, ADR 0133: this is about the ceremony's.)
    talk(&hq, ada, task, 12);
    look(&hq, &clock);
    let all: Vec<_> = hq
        .store()
        .findings()
        .unwrap()
        .into_iter()
        .filter(|f| f.guard == Guard::Ceremony)
        .collect();
    assert_eq!(all.len(), 2, "a new finding at last");
    assert_eq!(all[1].verdict, None);
    assert_eq!(all[1].mark, 24);
    assert_eq!(talk_cards(&hq, "waiting"), 1);
    let ceremony_cards = digest(&hq)
        .iter()
        .filter(|d| d["finding"]["guard"] == "ceremony")
        .count();
    assert_eq!(ceremony_cards, 1);
}
