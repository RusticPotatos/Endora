//! ADR 0062. The permission model was the one place "evidence beats claims" did not
//! reach: thirteen read-back confirmed outcomes proved a light switch is a light
//! switch, and it was still filed next to a wire transfer.

use super::{PROVEN_AFTER, proven_by_the_record, withdrawn_by_them};
use endora_kernel::ids::OutcomeId;
use endora_understanding::Outcome;

fn outcome(id: u128, capability: &str, changed: Option<bool>) -> Outcome {
    Outcome::from_parts(
        OutcomeId::new(id),
        capability.to_owned(),
        "{}".to_owned(),
        "done".to_owned(),
        changed.map(|_| "seen".to_owned()),
        super::Timestamp::from_unix_millis(0),
        None,
        None,
        changed,
    )
}

/// Same as [`outcome`], plus what the person said about it afterwards.
fn judged(id: u128, capability: &str, reaction: super::Reaction) -> Outcome {
    Outcome::from_parts(
        OutcomeId::new(id),
        capability.to_owned(),
        "{}".to_owned(),
        "done".to_owned(),
        Some("seen".to_owned()),
        super::Timestamp::from_unix_millis(0),
        None,
        Some(reaction),
        Some(true),
    )
}

#[test]
fn a_tool_they_keep_calling_unhelpful_does_not_act_unasked() {
    // Read-back proves a light went off. It cannot prove anybody wanted it off, and
    // that was the whole of what graduation asked before this.
    let mut all: Vec<_> = (0..u128::from(PROVEN_AFTER))
        .map(|i| outcome(i + 1, "home.HassTurnOff", Some(true)))
        .collect();
    assert!(
        proven_by_the_record(&all).contains("home.HassTurnOff"),
        "the record alone should still grant it"
    );

    for i in 0..u128::from(PROVEN_AFTER) {
        all.push(judged(
            50 + i,
            "home.HassTurnOff",
            super::Reaction::DidNotHelp,
        ));
    }
    assert!(
        !proven_by_the_record(&all).contains("home.HassTurnOff"),
        "it kept acting unasked through three verdicts that it was not wanted"
    );
}

#[test]
fn one_stray_verdict_does_not_withdraw_a_working_tool() {
    // Repeating an ask marks *every* successful action in its window as unhelpful, so
    // a single mark is far too coarse a thing to revoke autonomy on.
    assert!(!withdrawn_by_them((0, 1)));
    assert!(!withdrawn_by_them((0, PROVEN_AFTER - 1)));
    assert!(withdrawn_by_them((0, PROVEN_AFTER)));
}

#[test]
fn saying_it_helped_holds_the_grant_open() {
    // A tool the person is mostly happy with keeps acting, even with some marks
    // against it. The verdict has to be net negative, not merely present.
    assert!(!withdrawn_by_them((PROVEN_AFTER, PROVEN_AFTER)));
    assert!(!withdrawn_by_them((5, 4)));
    assert!(withdrawn_by_them((2, 5)));
}

#[test]
fn made_no_difference_is_a_third_answer_not_a_middle_one() {
    // "I saw it and it changed nothing for me" says something about the moment, not
    // about whether the tool works. Counting it either way tilts a scale it is not on.
    let mut all: Vec<_> = (0..u128::from(PROVEN_AFTER))
        .map(|i| outcome(i + 1, "home.HassTurnOn", Some(true)))
        .collect();
    for i in 0..10u128 {
        all.push(judged(
            50 + i,
            "home.HassTurnOn",
            super::Reaction::NoReaction,
        ));
    }
    assert!(proven_by_the_record(&all).contains("home.HassTurnOn"));
}

#[test]
fn withdrawal_recedes_when_the_verdicts_do() {
    // Derived, never stored — the same property the grant already had. Delete the
    // marks and the tool is trusted again, with nothing to administer.
    let granted: Vec<_> = (0..u128::from(PROVEN_AFTER))
        .map(|i| outcome(i + 1, "home.HassTurnOff", Some(true)))
        .collect();
    let mut withdrawn = granted.clone();
    for i in 0..u128::from(PROVEN_AFTER) {
        withdrawn.push(judged(
            50 + i,
            "home.HassTurnOff",
            super::Reaction::DidNotHelp,
        ));
    }
    assert!(!proven_by_the_record(&withdrawn).contains("home.HassTurnOff"));
    assert!(proven_by_the_record(&granted).contains("home.HassTurnOff"));
}

#[test]
fn enough_confirmed_changes_prove_a_tool_and_claims_prove_nothing() {
    let mut all = Vec::new();
    // Three read-back confirmed changes: proven.
    for i in 0..u128::from(PROVEN_AFTER) {
        all.push(outcome(i + 1, "home.HassTurnOff", Some(true)));
    }
    // Thirty raw claims with no read-back: the claim is the thing that can be untrue.
    for i in 0..30u128 {
        all.push(outcome(100 + i, "home.HassBroadcast", None));
    }
    // Confirmed *failures* are tries, not proof.
    for i in 0..5u128 {
        all.push(outcome(200 + i, "home.HassLightSet", Some(false)));
    }
    let proven = proven_by_the_record(&all);
    assert!(proven.contains("home.HassTurnOff"));
    assert!(
        !proven.contains("home.HassBroadcast"),
        "thirty unverified claims must prove nothing"
    );
    assert!(!proven.contains("home.HassLightSet"));
}

#[test]
fn one_short_of_the_bar_is_not_proven() {
    let all: Vec<Outcome> = (0..u128::from(PROVEN_AFTER) - 1)
        .map(|i| outcome(i + 1, "home.HassTurnOff", Some(true)))
        .collect();
    assert!(proven_by_the_record(&all).is_empty());
}
