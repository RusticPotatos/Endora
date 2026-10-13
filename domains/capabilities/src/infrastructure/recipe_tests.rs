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

#[test]
fn a_recipe_may_not_reach_the_house_or_the_metadata_service() {
    // The hole this closes: a recipe's URL is arbitrary — the person types it,
    // and one day the butler drafts it — but the interpreter shipped fetching
    // through the SAME helper the constant-host built-ins use, which does not
    // guard. A recipe aimed at a link-local, loopback or private address was
    // simply fetched. `Recipe::new` cannot catch this (a private host is a
    // perfectly well-formed https URL), so the guard has to be at the fetch.
    //
    // No network is reached: the guard refuses before any request is made,
    // which is also why this test is fast and offline.
    for target in [
        "http://169.254.169.254/latest/meta-data/",
        "http://192.168.1.10:8123/api/states",
        "http://127.0.0.1:8787/v1/export",
        "http://localhost/admin",
    ] {
        let recipe = Recipe::new("probe", "probes somewhere private", vec![], target, "{x}")
            .expect("a private host is a well-formed URL — validation cannot catch it");
        let refused = run_recipe(&recipe, &serde_json::json!({}))
            .expect_err("a private address must never be fetched");
        // Asserting on the REASON, not merely that it errored: a refused
        // connection also produces an error, so `is_err()` alone would pass
        // just as happily with the guard removed — a test that lies.
        assert!(
            refused.contains("private/internal address"),
            "{target} failed for the wrong reason ({refused}) — that is a \
             network error, not the guard refusing"
        );
    }
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

// ---- RecipeRunner -------------------------------------------------

#[test]
fn an_unenabled_recipe_is_hidden_and_a_stored_stance_turns_it_on() {
    let runner = RecipeRunner::new(vec![air_quality()], vec![]);
    let spec = runner
        .available()
        .into_iter()
        .find(|s| s.id == "recipe.air_quality")
        .unwrap();
    // Absence defaults to Off — the observe band's own Auto default is not
    // reached; see the struct doc for why.
    assert!(!spec.configured, "an unauthored-into stance must not run");
    assert!(!spec.autonomous);
    assert_eq!(spec.reversibility, Reversibility::Observe);
    assert!(
        spec.third_party,
        "a recipe's response is another service's words"
    );
    assert_eq!(runner.decision("recipe.air_quality"), Some(Decision::Block));
    assert!(runner.run("recipe.air_quality", "{}").is_err());

    let enabled = RecipeRunner::new(
        vec![air_quality()],
        vec![("recipe.air_quality".to_owned(), Stance::Auto)],
    );
    let spec = enabled
        .available()
        .into_iter()
        .find(|s| s.id == "recipe.air_quality")
        .unwrap();
    assert!(spec.configured);
    assert!(spec.autonomous);
    assert_eq!(enabled.decision("recipe.air_quality"), Some(Decision::Act));
}

#[test]
fn a_call_that_looks_like_a_secret_is_refused_before_any_request() {
    let runner = RecipeRunner::new(
        vec![air_quality()],
        vec![("recipe.air_quality".to_owned(), Stance::Auto)],
    );
    let err = runner
        .run(
            "recipe.air_quality",
            r#"{"lat": 1, "lon": "sk-abcdefghijklmnopqrstuvwxyz0123456789ABCD"}"#,
        )
        .unwrap_err();
    assert!(err.contains("looks like it contains"), "{err}");
}

#[test]
fn an_unknown_recipe_id_is_a_named_error_not_a_panic() {
    let runner = RecipeRunner::new(vec![air_quality()], vec![]);
    assert_eq!(runner.decision("recipe.nope"), None);
    assert!(runner.run("recipe.nope", "{}").is_err());
    assert!(
        runner.run("weather", "{}").is_err(),
        "the recipe. prefix is required"
    );
}

#[test]
fn the_input_schema_requires_every_declared_input() {
    let schema = recipe_input_schema(&air_quality());
    let v: Value = serde_json::from_str(&schema).unwrap();
    let required: Vec<&str> = v["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    assert_eq!(required, vec!["lat", "lon"]);
    assert_eq!(v["properties"]["lat"]["type"], "number");
}
