//! Live: the butler kept answering with the wrong city while the stored preference was
//! right the whole time. Its own past replies were three of the last twelve messages,
//! and to a small model reading the window, that is evidence. ADR 0052 already says
//! Endora's own conduct is not evidence about the person — the conversation was the one
//! place that rule had no reach.

use super::where_they_are;
use endora_kernel::ids::PreferenceId;
use endora_understanding::Preference;

fn pref(id: u128, text: &str) -> Preference {
    Preference::new(
        PreferenceId::new(id),
        text,
        endora_understanding::PreferenceKind::Context,
        super::Timestamp::from_unix_millis(i64::try_from(id).unwrap_or_default()),
    )
    .unwrap()
}

#[test]
fn the_place_comes_from_what_they_said_however_it_was_written() {
    let prefs = vec![
        pref(1, "you usually want news, weather, and traffic"),
        pref(2, "based in Springfield, IL"),
    ];
    assert_eq!(where_they_are(&prefs), "Springfield, IL");
    // The shape the console actually writes — found only by reading the live store,
    // after a parser that assumed the other shape returned nothing at all.
    assert_eq!(
        where_they_are(&[pref(3, "Based in: Springfield, IL")]),
        "Springfield, IL"
    );
    assert_eq!(
        where_they_are(&[pref(4, "BASED IN — Ashfield")]),
        "Ashfield"
    );
}

#[test]
fn the_latest_one_they_set_wins() {
    // Moving house is a preference change, not an argument with history.
    let prefs = vec![
        pref(1, "based in Springfield, IL"),
        pref(2, "based in Ashfield"),
    ];
    assert_eq!(where_they_are(&prefs), "Ashfield");
}

#[test]
fn saying_nothing_is_empty_rather_than_a_guess() {
    assert_eq!(where_they_are(&[]), "");
    assert_eq!(where_they_are(&[pref(1, "likes short answers")]), "");
    // And a trailing full stop is not part of the town.
    assert_eq!(where_they_are(&[pref(1, "based in Ashfield.")]), "Ashfield");
}
