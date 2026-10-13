use super::*;
use crate::Reversibility;

fn actuator() -> crate::CapabilitySpec {
    crate::CapabilitySpec {
        id: "home-assistant.GetDateTime".to_owned(),
        wants_place: false,
        third_party: false,
        description: String::new(),
        configured: true,
        autonomous: false,
        reversibility: Reversibility::Irreversible,
        input_schema: None,
    }
}

/// The reading that was actually attached to a request for the time.
const THE_WHOLE_HOUSE: &str = "Live Context: An overview of the areas and the devices \
    in this smart home:\n- names: All Lights\n  domain: light\n  state: 'on'\n\
    - names: Apple TV\n  domain: media_player\n  state: 'off'";

#[test]
fn a_call_that_moved_nothing_does_not_drag_the_world_along() {
    // Live: asking Home Assistant for the time came back with five kilobytes of every
    // device in the house, under "answer from the OBSERVATION" — three times in one
    // turn. The briefing that followed was one sentence about a bedroom light.
    let out = note_verification_against(
        "{\"time\": \"05:40:30\"}",
        Some(&actuator()),
        Some(THE_WHOLE_HOUSE),
        Some(THE_WHOLE_HOUSE),
    );
    assert!(!out.contains("Apple TV"), "pasted the house: {out}");
    assert!(out.contains("unchanged from before"), "{out}");
    // The verdict itself is untouched — that is the signal ADR 0053 exists for, and
    // it is still stated, in a sentence rather than in five kilobytes.
    let verdict = note_unchanged(Some(THE_WHOLE_HOUSE), Some(THE_WHOLE_HOUSE));
    assert!(verdict.contains("[unchanged]"));
}

#[test]
fn a_call_that_moved_something_still_carries_the_evidence() {
    // The whole point of reading back. When the world differs, the detail *is* the
    // evidence and it must reach the model.
    let after = THE_WHOLE_HOUSE.replace("state: 'on'", "state: 'off'");
    let out = note_verification_against(
        "turned it off",
        Some(&actuator()),
        Some(THE_WHOLE_HOUSE),
        Some(&after),
    );
    assert!(out.contains("Answer from the OBSERVATION"), "{out}");
    assert!(out.contains("Apple TV"), "{out}");
}

#[test]
fn nothing_to_compare_against_still_carries_the_reading() {
    // A reader that failed before the call and worked after leaves no comparison, and
    // the safe default is to show what was seen rather than to assert it means nothing.
    let out = note_verification_against("done", Some(&actuator()), None, Some(THE_WHOLE_HOUSE));
    assert!(out.contains("Answer from the OBSERVATION"), "{out}");
}
