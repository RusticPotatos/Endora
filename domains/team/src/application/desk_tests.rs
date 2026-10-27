//! A desk that interrupts only for decisions (ADR 0132): stuck work, a guard's finding and a notice
//! wait in a digest instead of each becoming a card on the owner's desk; one finding is kept for a
//! task and a guard however often it is seen; and what the owner has judged is not raised again
//! until there is new evidence. Live, over 17 and 18 October, 30 items reached the desk in 38.6
//! hours and six were real decisions; one task was flagged by the stuck guard four times, and a
//! ceremony finding came back three times after the owner marked it resolved.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::Value;

use super::guards_tests::{Hands, Hq, NOON, later, team};
use super::ports::TeamStore;

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

    let on_the_desk = desk(&hq);
    assert_eq!(
        kinds(&on_the_desk),
        ["question", "approval"],
        "{on_the_desk:?}"
    );
    assert_eq!(on_the_desk[0]["id"], question.0);
    assert_eq!(on_the_desk[1]["id"], approval.0);
    let waiting = digest(&hq);
    assert_eq!(kinds(&waiting), ["notice", "stuck"], "newest first");
    assert_eq!(
        waiting.iter().map(|d| d["id"].clone()).collect::<Vec<_>>(),
        [notice.0, stuck.0]
    );
    assert!(
        waiting[0]["text"]
            .as_str()
            .unwrap()
            .starts_with("Linus restarted sonarr"),
        "each item keeps its words and its choices: {}",
        waiting[0]
    );

    // Acting on an item takes it out of the digest.
    hq.decide(notice, "seen").unwrap();
    assert_eq!(kinds(&digest(&hq)), ["stuck"]);
    assert_eq!(
        kinds(&desk(&hq)),
        ["question", "approval"],
        "the desk was never theirs"
    );
}

// ---- one finding for a task and a guard -----------------------------------------------------------

// ---- the ceremony ---------------------------------------------------------------------------------
