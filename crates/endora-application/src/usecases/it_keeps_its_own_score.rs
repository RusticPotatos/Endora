//! The reward half of the loop spine: derived from read-back, never from the model's
//! account of itself.

use super::scorecard;
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

#[test]
fn the_score_is_read_back_and_names_the_worst_habit() {
    let mut all = Vec::new();
    for i in 0..4u128 {
        all.push(outcome(i, "home.HassTurnOff", Some(true)));
    }
    for i in 0..5u128 {
        all.push(outcome(10 + i, "home.HassLightSet", Some(false)));
    }
    let said = scorecard(&all).unwrap();
    assert!(said.contains("4 of 9 actions confirmed"), "{said}");
    assert!(
        said.contains("home.HassLightSet has never worked in 5 tries"),
        "{said}"
    );
    assert!(
        said.contains("1 tool has earned acting without asking"),
        "{said}"
    );
}

#[test]
fn nothing_attempted_is_no_scorecard_rather_than_a_vibe() {
    assert!(scorecard(&[]).is_none());
}

#[test]
fn a_tool_barely_tried_is_not_named_the_worst() {
    // One failed try is not a habit; naming it would teach avoidance on no evidence —
    // the same reason a new tool is not punished for having no record (ADR 0060).
    let all = vec![outcome(1, "home.HassBroadcast", Some(false))];
    let said = scorecard(&all).unwrap();
    assert!(!said.contains("never worked"), "{said}");
}
