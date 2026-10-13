//! Classifying a reading (ADR 0057). The house is shared and the person's own devices are
//! not, and only the second may carry a notion into a belief about them.

use super::{Source, TheRecord};

#[test]
fn the_persons_own_devices_are_marked_apart_from_the_house() {
    let record = TheRecord::of(super::readings_as_records(
        vec![
            ("person.john".to_owned(), "home".to_owned()),
            ("device_tracker.pixel".to_owned(), "home".to_owned()),
            ("light.hall".to_owned(), "on".to_owned()),
            (
                "calendar.family".to_owned(),
                "Yardwork - Jane Doe and John Doe".to_owned(),
            ),
        ],
        &["person.john".to_owned(), "device_tracker.pixel".to_owned()],
    ));
    let kind = |reference: &str| {
        record
            .entries()
            .iter()
            .find(|e| e.reference == reference)
            .map(|e| e.source)
    };
    assert_eq!(kind("person.john"), Some(Source::Personal));
    assert_eq!(kind("device_tracker.pixel"), Some(Source::Personal));
    assert_eq!(kind("light.hall"), Some(Source::Reading));
    assert_eq!(
        kind("calendar.family"),
        Some(Source::Reading),
        "a shared calendar naming two other people is not evidence about this one"
    );
}

#[test]
fn with_nothing_known_to_be_theirs_everything_is_the_house() {
    // The safe direction, and what a household with several `person` entities produces:
    // nothing is attributed rather than the house being mistaken for the person.
    let record = TheRecord::of(super::readings_as_records(
        vec![("person.john".to_owned(), "home".to_owned())],
        &[],
    ));
    assert_eq!(
        record.entries().first().map(|e| e.source),
        Some(Source::Reading)
    );
}
