use super::*;

#[test]
fn a_belief_drawn_from_endoras_own_conduct_is_refused() {
    // Live, top of the understanding screen. Endora had been passive because its reach
    // into the house was broken, and concluded the person prefers passivity — then
    // carried that into every later turn as something it knew about them.
    assert!(is_about_endora_itself(
        "I didn't do anything proactive since I rely on your instructions."
    ));
    // A butler that mistakes its own behaviour for evidence reinforces whatever it
    // happens to be doing, including its own faults.
    assert!(is_about_endora_itself("I checked and found nothing"));
}

#[test]
fn evidence_about_the_person_survives_endora_appearing_in_it() {
    // The discriminator is the SUBJECT, not the pronoun. These are all real evidence
    // strings from the same screen, and every one of them is about the person.
    for real in [
        "You asked 'Where did you find this?' after I listed some upcoming events in New York.",
        "you requested temperature in Fahrenheit",
        "your request for an evening briefing suggests this",
        "previous request",
    ] {
        assert!(!is_about_endora_itself(real), "{real}");
    }
}
