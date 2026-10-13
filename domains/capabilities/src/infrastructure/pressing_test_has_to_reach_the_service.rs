//! Live: a Brave server listed eight tools, connected cleanly, reported no error — and
//! was subscribed to the wrong API. A handshake never calls the service, so everything
//! on the card looked healthy while nothing had been proven.

#[test]
fn a_test_call_fills_in_what_the_tool_requires() {
    let schema = r#"{"type":"object",
        "properties":{"query":{"type":"string"},"count":{"type":"integer"}},
        "required":["query"]}"#;
    // The required field is filled; the optional one is not invented.
    assert_eq!(
        super::arguments_for_a_test_call(Some(schema)),
        r#"{"query":"test"}"#
    );
}

#[test]
fn a_test_call_takes_a_declared_choice_over_an_invented_one() {
    let schema = r#"{"type":"object",
        "properties":{"freshness":{"enum":["pd","pw","pm"]}},
        "required":["freshness"]}"#;
    assert_eq!(
        super::arguments_for_a_test_call(Some(schema)),
        r#"{"freshness":"pd"}"#
    );
}

#[test]
fn a_test_call_without_a_schema_sends_nothing() {
    assert_eq!(super::arguments_for_a_test_call(None), "{}");
    assert_eq!(super::arguments_for_a_test_call(Some("not json")), "{}");
}

#[test]
fn a_required_field_of_unstated_type_is_left_to_the_server_to_complain_about() {
    // Guessing produces a rejection that reads like a broken credential. The server's
    // own message is more use than an invented value.
    let schema = r#"{"type":"object","properties":{"thing":{}},"required":["thing"]}"#;
    assert_eq!(super::arguments_for_a_test_call(Some(schema)), "{}");
}
