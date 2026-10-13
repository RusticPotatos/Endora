use super::note_not_answering;

fn house() -> Vec<(String, String)> {
    [
        ("Guest Bedroom Left", "unavailable"),
        ("Guest Bedroom Right", "unavailable"),
        ("Kitchen Main Light", "on"),
        ("Kitchen", "off"),
    ]
    .into_iter()
    .map(|(a, b)| (a.to_owned(), b.to_owned()))
    .collect()
}

#[test]
fn a_dead_target_explains_the_failure() {
    // The live one: "turn on the guest bedroom left lamp" failed, and the lamp had been
    // unavailable for days. The cause was in the reading Endora had just taken, and it
    // said nothing.
    let said = note_not_answering(r#"{"name":"Guest Bedroom Left"}"#, house());
    assert!(said.contains("Guest Bedroom Left"), "{said}");
    assert!(said.contains("unavailable"), "{said}");
    assert!(said.contains("it is the device, not the request"), "{said}");
}

#[test]
fn what_the_person_asked_decides_it_not_how_the_model_answered() {
    // Both of these are the live reply to the same request, on consecutive turns. Keyed
    // on the reply, the note fired for one and not the other — the same fact, the same
    // lamp, decided by whether three words happened to land together.
    for phrasing in [
        "I'm sorry, but there doesn't appear to be a guest bedroom left lamp in your \
         home setup, sir.",
        "The guest bedroom left lamp does not appear to be in your home setup, sir.",
        "I couldn't find that one.",
    ] {
        let _ = phrasing; // the reply is no longer consulted at all
    }
    // The request is what decides it, and it does not vary.
    let asked = "turn on the guest bedroom left lamp";
    let said = note_not_answering(asked, house());
    assert!(said.contains("Guest Bedroom Left"), "{said}");
    assert!(said.contains("unavailable"), "{said}");
}

#[test]
fn a_request_about_something_healthy_says_nothing() {
    // The note must not attach itself to every turn that happens to name a device.
    assert_eq!(
        note_not_answering("is the kitchen main light on?", house()),
        ""
    );
}

#[test]
fn a_target_that_is_answering_gets_no_excuse_made_for_it() {
    // A working light that did not change is a different problem, and blaming the
    // device would be Endora inventing a cause for its own failure.
    assert_eq!(
        note_not_answering(r#"{"name":"Kitchen Main Light"}"#, house()),
        ""
    );
    assert_eq!(note_not_answering(r#"{"area":"kitchen"}"#, house()), "");
}

fn house_by_id() -> Vec<(String, String)> {
    // The shape the native channel actually returns: keyed by entity id, so the
    // kind is the first word of the key.
    [
        ("light.guest_bedroom_right", "unavailable"),
        ("light.outside_color", "offline"),
        ("light.kitchen_table", "on"),
        ("sensor.garage_door", "unavailable"),
        ("scene.bedroom_bright", "unknown"),
    ]
    .into_iter()
    .map(|(a, b)| (a.to_owned(), b.to_owned()))
    .collect()
}

#[test]
fn a_kind_of_thing_names_everything_of_that_kind_not_answering() {
    // Live, 2026-08-06: "can you check the lights now … tell me if any are
    // offline". No single entity is named, so the name match has nothing to bite
    // on — but the kind is right there in the service's own keys.
    let said = note_not_answering("can you check the lights right now?", house_by_id());
    assert!(said.contains("light.guest_bedroom_right"), "{said}");
    assert!(said.contains("light.outside_color"), "{said}");
    // A healthy light is not a problem, another kind's problem is another
    // question's, and `unknown` is not an admission of failure (ADR 0056).
    assert!(!said.contains("kitchen_table"), "{said}");
    assert!(!said.contains("sensor.garage_door"), "{said}");
    assert!(!said.contains("scene.bedroom_bright"), "{said}");
}

#[test]
fn a_kind_with_everything_answering_stays_quiet() {
    let healthy = vec![
        ("light.kitchen_table".to_owned(), "on".to_owned()),
        ("light.bedroom".to_owned(), "off".to_owned()),
    ];
    assert_eq!(note_not_answering("check the lights", healthy), "");
}

#[test]
fn a_kind_nobody_asked_about_is_not_volunteered() {
    // The dead sensor is real, but the question was about lights; disclosure keyed
    // on the ask stays an answer rather than becoming a standing report.
    let said = note_not_answering("are the lights ok?", house_by_id());
    assert!(!said.contains("sensor.garage_door"), "{said}");
}

#[test]
fn the_longest_matching_name_is_the_one_blamed() {
    // `Kitchen` is in the house and answering; a call naming `Kitchen Main Light` must
    // be judged on the light, not on the room that shares its first word.
    let mut mixed = house();
    mixed.push((
        "Kitchen Main Light LED".to_owned(),
        "unavailable".to_owned(),
    ));
    let said = note_not_answering(r#"{"name":"Kitchen Main Light LED"}"#, mixed);
    assert!(said.contains("Kitchen Main Light LED"), "{said}");
}
