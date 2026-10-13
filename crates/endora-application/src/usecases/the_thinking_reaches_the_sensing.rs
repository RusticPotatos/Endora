//! Notions ran at zero for two days after shipping. A notion can only be built from
//! citations that resolve against the record, and the record held entities' states
//! *now* — nothing about what had changed. The watch loop and the night pass were two
//! working parts with no join, which is where nine of ten bugs here have lived.

use super::transitions_as_records;
use endora_capabilities::Transition;
use endora_understanding::Source;

fn moved(key: &str, at: i64) -> Transition {
    Transition {
        key: key.to_owned(),
        from: "off".to_owned(),
        to: "on".to_owned(),
        at_ms: at,
    }
}

#[test]
fn a_change_becomes_a_record_a_notion_can_cite() {
    let records = transitions_as_records(
        &[
            moved("house::light.hall", 1),
            moved("house::device_tracker.pixel", 2),
        ],
        &["device_tracker.pixel".to_owned()],
    );
    // The house corroborates and never carries; the person's own device may carry
    // (ADR 0057) — same attribution as a plain reading, decided by the same list.
    assert_eq!(records[0].source, Source::Reading);
    assert_eq!(records[1].source, Source::Personal);
    assert!(records[0].text.contains("light.hall went from off to on"));
}

#[test]
fn three_changes_of_one_thing_are_three_citations_not_one_said_thrice() {
    // Maturity is a count of DISTINCT resolutions; a reference that collapses to the
    // key alone would let one restless sensor mature a notion by itself in a night.
    let records = transitions_as_records(
        &[moved("house::door.back", 1), moved("house::door.back", 2)],
        &[],
    );
    assert_ne!(records[0].reference, records[1].reference);
}
