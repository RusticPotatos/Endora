//! Reading Legistar (ADR 0058 — answers, so MCP-shaped, but it is a built-in skill because
//! there is no server to run).
//!
//! Parsed rather than trusted. Field names are the only contract this has with somebody
//! else's service, a renamed one yields empty meetings rather than an error, and **a screen
//! quietly saying "nothing on" is the worst possible failure** for a thing whose entire job
//! is to say what is on.

use super::{describe_meeting, meetings_in, today_utc};

/// Shaped exactly as the live API answered on 2026-08-02, including the zeroed time in
/// `EventDate` that a naive read would show somebody as midnight.
const REAL: &str = r#"[
  {"EventId":1,"EventDate":"2026-08-03T00:00:00","EventTime":"9:00 AM",
   "EventBodyName":"Housing Council Committee",
   "EventLocation":"City Government Center, Room 267"},
  {"EventId":2,"EventDate":"2026-08-03T00:00:00","EventTime":"5:00 PM",
   "EventBodyName":"Transportation, Planning, and Development Council Committee ",
   "EventLocation":""}
]"#;

#[test]
fn it_reads_the_shape_the_service_actually_returns() {
    let found = meetings_in(REAL);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].who, "Housing Council Committee");
    assert_eq!(found[0].at, "9:00 AM");
    assert_eq!(
        found[0].on, "2026-08-03",
        "the zeroed time must not reach a person as midnight"
    );
}

#[test]
fn a_trailing_space_in_a_committee_name_is_not_part_of_the_name() {
    // The live answer has one. Left in, it reads as a typo somebody will assume is ours.
    assert_eq!(
        meetings_in(REAL)[1].who,
        "Transportation, Planning, and Development Council Committee"
    );
}

#[test]
fn a_meeting_nobody_can_name_is_not_reported() {
    let body = r#"[{"EventId":1,"EventDate":"2026-08-03T00:00:00","EventBodyName":""}]"#;
    assert!(meetings_in(body).is_empty());
}

#[test]
fn everything_but_the_name_is_allowed_to_be_missing() {
    // A meeting with no time or place is still a meeting, and dropping it would be
    // silently answering "nothing on" when something is.
    let body = r#"[{"EventId":1,"EventBodyName":"City Council Business Meeting"}]"#;
    let found = meetings_in(body);
    assert_eq!(found.len(), 1);
    assert_eq!(describe_meeting(&found[0]), "City Council Business Meeting");
}

#[test]
fn an_answer_this_cannot_read_is_no_meetings_rather_than_a_crash() {
    for body in ["", "not json", "{}", r#"{"error":"nope"}"#, "[]"] {
        assert!(meetings_in(body).is_empty(), "{body:?}");
    }
}

#[test]
fn a_meeting_is_said_the_way_somebody_would_mention_it() {
    let found = meetings_in(REAL);
    assert_eq!(
        describe_meeting(&found[0]),
        "Housing Council Committee on 2026-08-03 at 9:00 AM, \
         City Government Center, Room 267"
    );
    assert_eq!(
        describe_meeting(&found[1]),
        "Transportation, Planning, and Development Council Committee on 2026-08-03 at 5:00 PM"
    );
}

#[test]
fn today_is_the_shape_the_query_needs() {
    // The filter is a string comparison in somebody else's query language; a wrong shape
    // returns everything since 2015 rather than failing.
    let today = today_utc();
    assert_eq!(today.len(), 10, "{today}");
    assert_eq!(today.matches('-').count(), 2, "{today}");
    let (y, rest) = today.split_at(4);
    assert!(
        y.parse::<u32>().is_ok_and(|y| (2020..2200).contains(&y)),
        "{today}"
    );
    assert!(rest.starts_with('-'));
}
