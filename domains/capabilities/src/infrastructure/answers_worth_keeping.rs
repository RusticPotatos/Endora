//! ADR 0061.
//!
//! The sources this runs on are free tiers, and a free tier is the budget. Asking the
//! same question twice paid twice, so the butler could not afford to look things up
//! often — and so it did not.

use super::{fingerprint, keep_for_ms};

/// The whole of an answer, live:
///
/// > error: unavailable: io: invalid peer certificate: certificate expired:
/// > verification time 1785717318 (UNIX), but certificate is not valid after
/// > 1543938436 (241778882 seconds ago)
///
/// Every word true, none of it an answer.
#[test]
fn a_transport_failure_is_said_the_way_a_person_would_say_it() {
    let raw = "io: invalid peer certificate: certificate expired: verification time \
               1785717318 (UNIX), but certificate is not valid after 1543938436";
    let said = super::plainly(raw);
    assert!(said.contains("certificate has expired"), "{said}");
    assert!(
        !said.contains("1785717318"),
        "a unix timestamp reached the person: {said}"
    );
    assert!(!said.contains("io:"), "{said}");
}

#[test]
fn an_unrecognised_failure_is_passed_through_rather_than_guessed_at() {
    // A wrong guess reads worse than a technical sentence, and unrecognised is exactly
    // where the next unknown failure lives.
    let odd = "the remote end did something nobody has seen before";
    assert_eq!(super::plainly(odd), odd);
}

#[test]
fn the_source_decides_how_long_its_answer_is_good_for() {
    assert_eq!(keep_for_ms(Some("public, max-age=3600")), 3_600_000);
    // `s-maxage` wins where both are given: it is the one addressed to a shared cache,
    // which is what this is.
    assert_eq!(keep_for_ms(Some("max-age=60, s-maxage=1800")), 1_800_000);
}

#[test]
fn a_source_is_not_owed_unlimited_trust_about_its_own_freshness() {
    // Claiming zero would make the whole thing pointless; the floor decides instead.
    assert_eq!(keep_for_ms(Some("max-age=0")), super::KEEP_AT_LEAST_MS);
    // Claiming a year would make the butler confidently out of date for a year.
    assert_eq!(
        keep_for_ms(Some("max-age=31536000")),
        super::KEEP_AT_MOST_MS
    );
    // Saying nothing at all is the common case, and gets the floor.
    assert_eq!(keep_for_ms(None), super::KEEP_AT_LEAST_MS);
    assert_eq!(keep_for_ms(Some("private")), super::KEEP_AT_LEAST_MS);
}

#[test]
fn no_store_is_obeyed_exactly() {
    // The one instruction worth taking literally: the source is saying do not keep this.
    assert_eq!(keep_for_ms(Some("no-store")), 0);
    assert_eq!(keep_for_ms(Some("private, no-store, max-age=600")), 0);
}

/// The whole privacy posture of this record, as an assertion.
///
/// A URL carries the API key of whatever is being asked and often the person's own town.
/// What is kept must be enough to recognise the same question and not enough to rebuild
/// it — so there is nothing here to redact in a log, because there is nothing to print.
#[test]
fn what_is_kept_cannot_be_turned_back_into_the_question() {
    let asked = "https://example.com/events?apikey=s3cr3t&city=Springfield";
    let (a, b) = fingerprint(asked);
    let held = format!("{a}{b}");
    assert!(!held.contains("s3cr3t"), "the credential survived");
    assert!(!held.contains("Springfield"), "the place survived");
    assert!(!held.contains("example.com"), "the source survived");
}

#[test]
fn the_same_question_is_recognised_and_a_different_one_is_not() {
    let one = fingerprint("https://example.com/events?city=A");
    assert_eq!(one, fingerprint("https://example.com/events?city=A"));
    assert_ne!(one, fingerprint("https://example.com/events?city=B"));
    // Two independent hashes, so a collision is another question's answer rather than a
    // slow one — the halves must not be the same function twice.
    assert_ne!(one.0, one.1);
}
