//! Live: a new skill that needs a venue was offered to the model with an empty parameter
//! object — *this takes nothing* — so it called it with `{}` twice and was told "say what
//! to look for" both times. It was doing exactly what it had been told it could.
//!
//! MCP tools have carried a real schema all along. The skills written here had no way to
//! say, and the two that really take arguments got by on a hand-written example in the
//! system prompt: a per-skill patch that only ever covered whoever was remembered.

use crate::application::CapabilityRunner;
use std::sync::Arc;

#[test]
fn a_skill_that_needs_arguments_hands_the_model_their_names() {
    let runner = super::RegistryRunner::new(Arc::new(super::default_capabilities()));
    let spec = runner
        .available()
        .into_iter()
        .find(|c| c.id == "ticketed_events")
        .expect("the skill is registered");
    let schema = spec
        .input_schema
        .expect("a skill that needs arguments must say so");
    let parsed: serde_json::Value =
        serde_json::from_str(&schema).expect("the schema must be JSON the model can read");
    let properties = parsed
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .expect("a schema with no properties tells the model it takes nothing");
    assert!(properties.contains_key("what"), "{properties:?}");
    assert!(properties.contains_key("city"), "{properties:?}");
}

#[test]
fn a_skill_that_takes_nothing_still_says_nothing() {
    // The default has to stay honest: an empty schema is right for a skill with no
    // arguments, and wrong only for one that has them.
    let runner = super::RegistryRunner::new(Arc::new(super::default_capabilities()));
    let spec = runner
        .available()
        .into_iter()
        .find(|c| c.id == "own_activity" || c.id == "city_meetings")
        .expect("some skill takes nothing");
    assert!(spec.input_schema.is_none());
}
