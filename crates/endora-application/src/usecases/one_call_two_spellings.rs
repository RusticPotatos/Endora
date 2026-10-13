use endora_capabilities::same_call_as;

#[test]
fn punctuation_does_not_make_it_a_different_call() {
    // Live, in one morning briefing: four rounds spent on two pairs of identical
    // calls, none caught, because the guard compared raw text.
    assert_eq!(same_call_as("{}"), same_call_as("{ }"));
    assert_eq!(same_call_as("{}"), same_call_as("\n  {}\n"));
    assert_eq!(
        same_call_as(r#"{"a":1,"b":2}"#),
        same_call_as(r#"{ "b": 2, "a": 1 }"#)
    );
}

#[test]
fn genuinely_different_arguments_stay_different() {
    // The guard must never collapse two calls that ask for different things — that
    // would silently drop work the model actually needed.
    assert_ne!(
        same_call_as(r#"{"domain":["weather"]}"#),
        same_call_as(r#"{"domain":["light"]}"#)
    );
    assert_ne!(same_call_as("{}"), same_call_as(r#"{"domain":[]}"#));
}

#[test]
fn arguments_that_will_not_parse_are_left_alone() {
    // No worse than before: unparseable text compares as text.
    assert_eq!(same_call_as("not json"), "not json");
    assert_ne!(same_call_as("not json"), same_call_as("other junk"));
}
