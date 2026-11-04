use super::*;
use crate::domain::{Recipe, RecipeInput, RecipeInputKind};

fn air_quality() -> Recipe {
    Recipe::new(
        "air_quality",
        "Today's air quality where you are.",
        vec![
            RecipeInput { name: "lat".to_owned(), kind: RecipeInputKind::Number },
            RecipeInput { name: "lon".to_owned(), kind: RecipeInputKind::Number },
        ],
        "https://air-quality-api.open-meteo.com/v1/air-quality?latitude={lat}&longitude={lon}&current=us_aqi",
        "The air quality index is {current.us_aqi} right now.",
    )
    .unwrap()
}

#[test]
fn the_fields_offered_are_exactly_the_ones_a_say_template_can_reach() {
    // The picker's contract: every path it offers must resolve, and every
    // path it hides must be one that could not. Offering a field inside a
    // list would hand the person a template that always fails.
    let response = serde_json::json!({
        "current": { "us_aqi": 42, "label": "good" },
        "hourly": [ { "us_aqi": 41 } ],
        "elevation": 12.5,
        "nothing": null,
    });
    let mut fields = Vec::new();
    let mut skipped = false;
    readable_paths(&response, "", &mut fields, &mut skipped);

    let paths: Vec<&str> = fields.iter().map(|(p, _)| p.as_str()).collect();
    assert!(paths.contains(&"current.us_aqi"), "{paths:?}");
    assert!(paths.contains(&"current.label"), "{paths:?}");
    assert!(paths.contains(&"elevation"), "{paths:?}");
    // A list is unreachable, and a null reads as missing to the template.
    assert!(!paths.iter().any(|p| p.starts_with("hourly")), "{paths:?}");
    assert!(!paths.contains(&"nothing"), "{paths:?}");
    assert!(skipped, "the list must be reported, not silently dropped");

    // And the promise holds: every offered path really does resolve.
    for (path, _) in &fields {
        assert!(
            fill_say_template(&format!("{{{path}}}"), &response).is_ok(),
            "offered {path}, which the template cannot read"
        );
    }
}

#[test]
fn a_trial_shows_the_sample_value_it_found() {
    let response = serde_json::json!({ "current": { "us_aqi": 42, "label": "good" } });
    let mut fields = Vec::new();
    let mut skipped = false;
    readable_paths(&response, "", &mut fields, &mut skipped);
    let aqi = fields.iter().find(|(p, _)| p == "current.us_aqi").unwrap();
    assert_eq!(aqi.1, "42");
    let label = fields.iter().find(|(p, _)| p == "current.label").unwrap();
    // Strings are shown bare, not JSON-quoted — this is a sample for a
    // person to read, not a value to round-trip.
    assert_eq!(label.1, "good");
}

// ---- fill_get_template ----------------------------------------------

#[test]
fn the_worked_example_fills_exactly() {
    let url = fill_get_template(&air_quality(), &json!({"lat": 12.5, "lon": -34.25})).unwrap();
    assert_eq!(
        url,
        "https://air-quality-api.open-meteo.com/v1/air-quality?latitude=12.5&longitude=-34.25&current=us_aqi"
    );
}

#[test]
fn a_missing_input_is_refused_by_name() {
    let err = fill_get_template(&air_quality(), &json!({"lat": 40.0})).unwrap_err();
    assert!(err.contains("lon"), "{err}");
}

#[test]
fn the_wrong_shape_is_refused() {
    let err =
        fill_get_template(&air_quality(), &json!({"lat": "north", "lon": -74.0})).unwrap_err();
    assert!(err.contains("lat") && err.contains("number"), "{err}");

    let text_recipe = Recipe::new(
        "x",
        "d",
        vec![RecipeInput {
            name: "q".to_owned(),
            kind: RecipeInputKind::Text,
        }],
        "https://x.test/?q={q}",
        "s",
    )
    .unwrap();
    let err = fill_get_template(&text_recipe, &json!({"q": 5})).unwrap_err();
    assert!(err.contains('q') && err.contains("text"), "{err}");
}

#[test]
fn a_value_cannot_widen_the_request_it_fills() {
    // The ceiling: every character outside the unreserved set is percent-encoded,
    // so a value carrying "&x=y", "../", or a raw CRLF cannot add a query
    // parameter, walk a path, or inject a header — there is no way for a slot's
    // VALUE to change the request's SHAPE, only what one placeholder says.
    let recipe = Recipe::new(
        "x",
        "d",
        vec![RecipeInput {
            name: "q".to_owned(),
            kind: RecipeInputKind::Text,
        }],
        "https://x.test/search?q={q}&format=json",
        "s",
    )
    .unwrap();
    let url = fill_get_template(&recipe, &json!({"q": "a&evil=1"})).unwrap();
    assert_eq!(url, "https://x.test/search?q=a%26evil%3D1&format=json");
    assert!(
        url.ends_with("&format=json"),
        "the real second param must survive: {url}"
    );

    let url = fill_get_template(&recipe, &json!({"q": "../../etc/passwd"})).unwrap();
    assert!(!url.contains("../"), "{url}");

    let url = fill_get_template(&recipe, &json!({"q": "x\r\nX-Evil: 1"})).unwrap();
    assert!(!url.contains('\r') && !url.contains('\n'), "{url}");
}

// ---- fill_say_template ------------------------------------------------

#[test]
fn the_worked_example_reads_the_nested_path() {
    let said = fill_say_template(
        "The air quality index is {current.us_aqi} right now.",
        &json!({"current": {"us_aqi": 42, "time": "2026-08-09T12:00"}}),
    )
    .unwrap();
    assert_eq!(said, "The air quality index is 42 right now.");
}

#[test]
fn a_missing_path_is_refused_by_name() {
    let err = fill_say_template("{a.b.c}", &json!({"a": {"b": {}}})).unwrap_err();
    assert!(err.contains("a.b.c"), "{err}");
}

#[test]
fn a_string_value_is_not_re_quoted() {
    let said = fill_say_template(
        "Condition: {current.condition}",
        &json!({"current": {"condition": "clear"}}),
    )
    .unwrap();
    assert_eq!(said, "Condition: clear");
}

#[test]
fn every_placeholder_in_the_template_is_filled() {
    let said = fill_say_template("{a} and {b}", &json!({"a": 1, "b": "two"})).unwrap();
    assert_eq!(said, "1 and two");
}
