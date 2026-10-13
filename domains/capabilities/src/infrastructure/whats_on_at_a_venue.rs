//! Reading Ticketmaster's Discovery API (ADR 0058 — answers, so MCP-shaped; a built-in
//! skill because the only server offering it is a v0.1.0 remote gateway from an unknown
//! publisher, and routing a credential and every query through a stranger costs more
//! than the code it saves).
//!
//! Parsed rather than trusted, exactly as the civic agenda is. Field names are the whole
//! contract with somebody else's service, and a renamed one must yield **no events**
//! rather than an error or a half-built row.

use super::{describe_ticketed_event, ticketed_events_in};

/// Shaped as the documented response: events under `_embedded.events`, the venue under
/// the event's own `_embedded`, and the time carrying seconds nobody says out loud.
const ANSWERED: &str = r#"{"_embedded":{"events":[
    {"name":"Rovers vs Wanderers",
     "dates":{"start":{"localDate":"2026-08-04","localTime":"20:00:00"}},
     "priceRanges":[{"min":40.0,"max":220.0,"currency":"USD"}],
     "_embedded":{"venues":[{"name":"The Big Ground","city":{"name":"New York"}}]}},
    {"name":"An Evening With Somebody",
     "dates":{"start":{"localDate":"2026-08-07"}},
     "_embedded":{"venues":[{"name":"The Hall"}]}}
]}}"#;

#[test]
fn it_reads_what_is_on_and_where() {
    let on = ticketed_events_in(ANSWERED);
    assert_eq!(on.len(), 2);
    assert_eq!(on[0].what, "Rovers vs Wanderers");
    assert_eq!(on[0].on, "2026-08-04");
    // Seconds are a machine's way of saying eight o'clock.
    assert_eq!(on[0].at, "20:00");
    assert_eq!(on[0].place, "The Big Ground, New York");
    assert_eq!(on[0].from, "40");
}

#[test]
fn a_listing_that_says_less_still_arrives() {
    // No time, no price, no town. All optional — only a name is load-bearing.
    let on = ticketed_events_in(ANSWERED);
    assert_eq!(on[1].at, "");
    assert_eq!(on[1].from, "");
    assert_eq!(on[1].place, "The Hall");
    assert_eq!(
        describe_ticketed_event(&on[1]),
        "An Evening With Somebody on 2026-08-07, The Hall"
    );
}

#[test]
fn it_says_an_event_the_way_somebody_would() {
    let on = ticketed_events_in(ANSWERED);
    assert_eq!(
        describe_ticketed_event(&on[0]),
        "Rovers vs Wanderers on 2026-08-04 at 20:00, The Big Ground, New York, from $40"
    );
}

/// Nothing on is an answer. An error is not.
#[test]
fn an_empty_search_is_empty_rather_than_broken() {
    // What the API returns when a search matches nothing: no `_embedded` at all.
    assert!(ticketed_events_in(r#"{"page":{"totalElements":0}}"#).is_empty());
}

/// A key in a query string is a key in every error about that request.
///
/// What an HTTP crate prints on failure is its choice, not ours, and "401 for
/// https://…?apikey=REAL_KEY" would reach both the person and the record of what was
/// tried. Removing it is a guarantee; hoping about a `Display` impl is not.
#[test]
fn a_failure_never_carries_the_key_back_out() {
    let leaked = "http status 401 for \
                  https://app.ticketmaster.com/discovery/v2/events.json?apikey=s3cr3t&size=20";
    let safe = super::without_the_key(leaked, "s3cr3t");
    assert!(!safe.contains("s3cr3t"), "{safe}");
    // And it still says what went wrong, which is the useful half.
    assert!(safe.contains("401"), "{safe}");
}

#[test]
fn with_no_key_configured_a_message_is_left_alone() {
    // An empty needle would otherwise match everywhere and shred the message.
    assert_eq!(
        super::without_the_key("could not connect", "   "),
        "could not connect"
    );
}

/// The failure this skill exists to avoid is the quiet one.
#[test]
fn a_renamed_field_yields_nothing_rather_than_a_half_built_row() {
    let renamed = r#"{"_embedded":{"events":[
        {"title":"Rovers vs Wanderers","dates":{"start":{"localDate":"2026-08-04"}}}]}}"#;
    assert!(ticketed_events_in(renamed).is_empty());
    assert!(ticketed_events_in("not json at all").is_empty());
}
