use super::*;

fn house() -> Vec<(String, String)> {
    [
        ("Home", "0"),
        ("Kitchen Table", "off"),
        ("Kitchen Main Light", "on"),
        ("Bedroom", "off"),
    ]
    .into_iter()
    .map(|(a, b)| (a.to_owned(), b.to_owned()))
    .collect()
}

#[test]
fn a_greeting_gets_no_facts_attached_to_it() {
    // Live, under "It's good afternoon! How can I assist you today?": `[state] Home is
    // 0`. The house contains something called `Home`, the sentence contained the word
    // "home", and the disclosure was both an exact match and meaningless.
    let greeting = "It's good afternoon! How can I assist you today? Do you need help \
                    with any specific tasks or information related to your home \
                    automation setup?";
    assert_eq!(facts_behind("", greeting, house()), "");
}

#[test]
fn a_reply_that_asserts_a_state_still_carries_the_facts() {
    // The reason the disclosure exists: the model said "already on" about something the
    // reading says is off, and the person can see both.
    let claim = "The kitchen table light is already on.";
    let shown = facts_behind("", claim, house());
    assert!(shown.contains("Kitchen Table is off"), "{shown}");
    // Longest first, and a name inside a matched longer one is not repeated.
    assert!(!shown.contains("Home is 0"), "{shown}");
}

#[test]
fn a_name_is_matched_as_words_not_as_letters() {
    // `Home` must not match "homework", and the state word must be a word too —
    // "information" contains "on" and asserts nothing.
    let states = vec![("Home".to_owned(), "0".to_owned())];
    assert_eq!(
        facts_behind("", "I finished my homework, 0 problems", states.clone()),
        ""
    );
    assert!(facts_behind("", "home is 0 right now", states).contains("Home is 0"));
}

#[test]
fn the_vocabulary_comes_from_the_reading_not_from_english() {
    // A service reporting open/closed is handled by the same code as one reporting
    // on/off, with nothing anywhere naming either pair.
    let garage = vec![("Garage Door".to_owned(), "closed".to_owned())];
    let shown = facts_behind("", "The garage door is open.", garage.clone());
    assert_eq!(shown, "", "'open' is not a value this service reported");
    assert!(
        facts_behind("", "The garage door is closed, I think.", garage)
            .contains("Garage Door is closed")
    );
}
