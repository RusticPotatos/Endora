//! The target search, run against a **real** reading (ADR 0054).
//!
//! Three corrections to this search shipped and failed in a row, each because the unit
//! tests used a tidy five-line reading I wrote myself. The live one is 5KB, arrives as a
//! single line with 255 escaped newlines, and contains forty-odd entities whose names
//! overlap in ways no hand-written fixture reproduces — a `living room lamp` in one room
//! silently decided whether a light in another room could be switched on.
//!
//! So the fixture is the actual reading, captured from the running system. A synthetic
//! one cannot catch this class of bug, which is the whole reason these tests exist.

use endora_capabilities::{
    candidates, only_real_match, retarget, target_words, target_words_with_kinds,
};

/// The kinds this house actually has, as Home Assistant reports them — domains plus the
/// device classes in use. Read off the service, never a list in Endora's source.
fn kinds() -> Vec<String> {
    ["light", "switch", "scene", "media_player", "sensor"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The live `GetLiveContext` result, exactly as it was stored in an outcome record.
const HOUSE: &str = include_str!("fixtures/live-house-reading.txt");

/// What the search decides for a call, as `(retried against, shortlist offered)`.
fn search(input: &str) -> (Option<String>, Vec<String>) {
    let found = candidates(HOUSE, &target_words(input));
    let best = only_real_match(&found).map(|c| c.value.clone());
    (best, found.iter().map(|c| c.value.clone()).collect())
}

#[test]
fn it_finds_the_light_that_took_fourteen_attempts_to_miss() {
    // The original failure: `name: "table"` in `area: "kitchen"`, fourteen times, with
    // the answer in this very text every time.
    let (best, _) = search(r#"{"name":"table","area":"kitchen","domain":["light"]}"#);
    assert_eq!(best.as_deref(), Some("Kitchen Table"));
}

#[test]
fn a_word_from_another_room_does_not_veto_a_clear_winner() {
    // The correction this file exists for. `Guest Bedroom Left` matches three of the
    // four words; it lost only because a `living room lamp` elsewhere in the house made
    // "lamp" look like a word worth insisting on.
    assert!(
        HOUSE.to_lowercase().contains("lamp"),
        "fixture no longer reproduces the case"
    );
    let (best, _) = search(r#"{"area":"guest bedroom left","name":"lamp","domain":["light"]}"#);
    assert_eq!(best.as_deref(), Some("Guest Bedroom Left"));
}

#[test]
fn the_switch_that_is_not_a_light_is_still_found_by_name() {
    // The kitchen's ceiling light is a `switch` called `Kitchen Main Light`, which is why
    // filtering by domain never found it. The search matches on the name alone.
    let (best, _) = search(r#"{"name":"kitchen main light"}"#);
    assert_eq!(best.as_deref(), Some("Kitchen Main Light"));
}

#[test]
fn a_whole_room_is_too_vague_to_act_on() {
    // "the kitchen" resembles the table, the main light and three scenes. Acting on any
    // of them is a coin flip, so it offers them and stops.
    let (best, shortlist) = search(r#"{"area":"kitchen"}"#);
    assert_eq!(best, None, "acted on a whole room");
    assert!(shortlist.len() > 3, "{shortlist:?}");
    assert!(shortlist.iter().any(|c| c == "Kitchen Table"));
}

#[test]
fn a_room_full_of_scenes_does_not_produce_a_confident_guess() {
    // `Outside` has eleven scenes sharing its name. Nothing here is a clear winner.
    let (best, _) = search(r#"{"area":"outside","domain":["light"]}"#);
    assert_eq!(best, None, "picked one of eleven look-alikes");
}

#[test]
fn something_this_house_does_not_have_is_not_invented() {
    let (best, shortlist) = search(r#"{"name":"greenhouse mister"}"#);
    assert_eq!(best, None);
    assert!(shortlist.is_empty(), "{shortlist:?}");
}

#[test]
fn the_retry_it_would_send_is_aimed_at_exactly_one_thing() {
    // End of the chain: what actually goes back to the server. The redundant `area`
    // fragment is dropped and the kind filter is kept, so the retry is narrower than the
    // call that failed — never wider.
    let input = r#"{"name":"table","area":"kitchen","domain":["light"]}"#;
    let (best, _) = search(input);
    let retry = retarget(input, "name", &best.unwrap());
    let v: serde_json::Value = serde_json::from_str(&retry).unwrap();
    assert_eq!(v["name"], "Kitchen Table");
    assert!(
        v.get("area").is_none(),
        "kept a fragment of the real name: {retry}"
    );
    assert_eq!(
        v["domain"],
        serde_json::json!(["light"]),
        "dropped a filter: {retry}"
    );
}

#[test]
fn the_phrase_that_switched_on_two_lights_now_finds_one() {
    // "turn on the kitchen table" became
    //   {area:"kitchen", device_class:["table"], domain:["light"]}
    // — no name at all. There is no category called `table`, so Home Assistant ignored
    // it and acted on every light in the Kitchen: the main light AND the table.
    let input = r#"{"area":"kitchen","device_class":["table"],"domain":["light"]}"#;

    // What it did before: only "kitchen" to go on, which resembles half the room.
    let vague = candidates(HOUSE, &target_words(input));
    assert!(
        only_real_match(&vague).is_none(),
        "the old reading should not have been actionable: {vague:?}"
    );

    // With the house's own vocabulary, `table` is plainly not a kind — it is the thing.
    let found = candidates(HOUSE, &target_words_with_kinds(input, &kinds()));
    let best = only_real_match(&found).expect("still cannot tell which thing was meant");
    assert_eq!(best.value, "Kitchen Table");
}

#[test]
fn a_real_kind_still_narrows_rather_than_naming() {
    // The rule must not fire on ordinary filters, or every light in the house becomes a
    // candidate whenever someone says "light".
    let input = r#"{"name":"garage main","domain":["light"],"device_class":["switch"]}"#;
    let found = candidates(HOUSE, &target_words_with_kinds(input, &kinds()));
    let best = only_real_match(&found).expect("a plainly named thing was not found");
    assert_eq!(best.value, "Garage Main");
}
