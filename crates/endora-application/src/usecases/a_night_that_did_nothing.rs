use super::*;
use endora_kernel::Timestamp;

fn an_intention() -> Intention {
    Intention::form(
        crate::IntentionId::new(1),
        "you want to run more often",
        crate::BeliefId::new(2),
        Timestamp::from_unix_millis(0),
    )
    .expect("a valid intention")
}

#[test]
fn an_apology_is_not_a_nights_work() {
    // Live: Endora's one long-running intention reached five of its seven steps with
    // its stored progress reading "Sorry — I couldn't reach my language model just
    // now." Non-empty, so it counted as a step, was written down as what happened, and
    // was handed to the next night as the thread to pick up.
    let degraded = ButlerReply {
        text: "Sorry — I couldn't reach my language model just now.".to_owned(),
        degraded: true,
        ..ButlerReply::default()
    };
    assert!(
        not_an_answer(&degraded, &ButlerContext::default()),
        "a degraded night must not count as progress"
    );

    // And the step budget is the thing being protected: seven nights is the whole
    // lifetime of an intention, so a night spent on nothing is a seventh of it.
    let mut spent = an_intention();
    for _ in 0..7 {
        spent.progress("looked into it", Timestamp::from_unix_millis(1));
    }
    assert!(
        spent.is_exhausted(),
        "seven steps spends it — which is why one wasted on an apology matters"
    );
}

#[test]
fn a_real_night_still_counts() {
    let real = ButlerReply {
        text: "Found three routes near you under 5km; the river loop looks best.".to_owned(),
        ..ButlerReply::default()
    };
    assert!(!not_an_answer(&real, &ButlerContext::default()));

    let mut going = an_intention();
    going.progress("Found three routes", Timestamp::from_unix_millis(1));
    assert_eq!(going.steps_taken(), 1);
    assert!(going.is_active());
}
