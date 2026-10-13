//! The guarantee ADR 0057 rests on: a language model proposes the wording of a notion,
//! and nothing here trusts it. These are the tests that decide whether this feature is
//! thinking or a fabrication store with citations on it.

use super::{
    KnownRecord, ProposedNotion, Source, TheRecord, evidence_that_holds_up, form_a_notion,
    new_support_for, speaks_to,
};
use endora_kernel::ids::{NotionId, Timestamp};
use endora_understanding::Citation;

fn at(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(ms)
}

fn record() -> TheRecord {
    TheRecord::of(vec![
        KnownRecord {
            source: Source::Reading,
            reference: "calendar.rustic".to_owned(),
            text: "Gym".to_owned(),
        },
        KnownRecord {
            source: Source::Message,
            reference: "msg-2".to_owned(),
            text: "skipping the gym again this morning, too tired".to_owned(),
        },
        KnownRecord {
            source: Source::Outcome,
            reference: "outcome-7".to_owned(),
            text: "HassTurnOff light.bedroom — changed 1".to_owned(),
        },
    ])
}

fn proposal(statement: &str, citations: &[(Source, &str)]) -> ProposedNotion {
    ProposedNotion {
        statement: statement.to_owned(),
        citations: citations
            .iter()
            .map(|(s, r)| (*s, (*r).to_owned()))
            .collect(),
        settles_when: "whether next Monday's block survives".to_owned(),
    }
}

#[test]
fn a_citation_that_names_nothing_is_discarded() {
    // The plainest fabrication: an id the model made up. Not scored down — gone.
    let kept = evidence_that_holds_up(
        &proposal(
            "you skip the gym when tired",
            &[(Source::Message, "msg-999")],
        ),
        &record(),
    );
    assert!(kept.is_empty());
}

#[test]
fn a_real_record_that_says_nothing_about_the_claim_is_discarded() {
    // The subtler and more likely failure. Handed a list of ids, a model can cite them
    // perfectly while claiming something they say nothing about — every citation resolves
    // and the notion is fully sourced and entirely invented. Existence is not enough.
    let kept = evidence_that_holds_up(
        &proposal(
            "you skip the gym when tired",
            &[(Source::Outcome, "outcome-7")], // a bedroom light
        ),
        &record(),
    );
    assert!(
        kept.is_empty(),
        "a light being switched off is not evidence about the gym"
    );
}

#[test]
fn evidence_that_does_speak_to_the_claim_is_kept() {
    let kept = evidence_that_holds_up(
        &proposal(
            "you skip the gym when tired",
            &[
                (Source::Message, "msg-2"),
                (Source::Outcome, "outcome-7"),
                (Source::Message, "msg-999"),
            ],
        ),
        &record(),
    );
    assert_eq!(kept, vec![Citation::new(Source::Message, "msg-2").unwrap()]);
}

#[test]
fn a_short_record_counts_when_all_of_it_matches() {
    // A calendar entry reading "Gym" has one content word. A flat two-word rule would
    // refuse it for having too little to say, which would throw away exactly the kind of
    // terse record a calendar is made of.
    assert!(speaks_to("the gym block gets cancelled", "Gym"));
    assert!(!speaks_to("the gym block gets cancelled", "Dentist"));
}

#[test]
fn one_word_in_common_is_not_enough_for_a_wordy_record() {
    // "tired" alone should not make a long message about something else into evidence.
    assert!(!speaks_to(
        "you skip the gym when tired",
        "the delivery arrived late and the driver seemed tired of the rain"
    ));
}

#[test]
fn an_empty_record_speaks_to_nothing() {
    assert!(!speaks_to("you skip the gym when tired", "   "));
}

#[test]
fn a_proposal_the_record_does_not_bear_out_forms_nothing() {
    // The expected outcome most nights, and the design working rather than failing.
    assert!(
        form_a_notion(
            NotionId::new(1),
            &proposal("you are unhappy at work", &[(Source::Message, "msg-2")]),
            &record(),
            at(0),
        )
        .is_none()
    );
}

#[test]
fn a_proposal_the_record_bears_out_becomes_a_notion() {
    let formed = form_a_notion(
        NotionId::new(1),
        &proposal(
            "you skip the gym when tired",
            &[
                (Source::Message, "msg-2"),
                (Source::Reading, "calendar.rustic"),
            ],
        ),
        &record(),
        at(50),
    )
    .expect("the record bears this out");
    assert_eq!(formed.support_count(), 2);
    assert_eq!(formed.last_supported_at(), at(50));
}

#[test]
fn advancing_a_notion_needs_no_model_at_all() {
    // The statement is already fixed, so whether a later record speaks to it is the same
    // arithmetic that formed it. This is what makes maturity mean something: a notion
    // cannot be talked into being believed.
    let notion = form_a_notion(
        NotionId::new(1),
        &proposal("you skip the gym when tired", &[(Source::Message, "msg-2")]),
        &record(),
        at(0),
    )
    .expect("formed");

    let found = new_support_for(&notion, &record());
    assert_eq!(
        found,
        vec![Citation::new(Source::Reading, "calendar.rustic").unwrap()],
        "the calendar entry is new; the message it already cites is not, and the bedroom \
         light speaks to nothing"
    );
}

#[test]
fn re_reading_the_same_records_finds_nothing_new() {
    // Death by silence depends on this. A nightly pass that re-reads the same week must
    // not renew every open notion, or nothing ever expires.
    let mut notion = form_a_notion(
        NotionId::new(1),
        &proposal(
            "you skip the gym when tired",
            &[
                (Source::Message, "msg-2"),
                (Source::Reading, "calendar.rustic"),
            ],
        ),
        &record(),
        at(0),
    )
    .expect("formed");

    for citation in new_support_for(&notion, &record()) {
        notion.support(citation, at(1_000));
    }
    assert!(new_support_for(&notion, &record()).is_empty());
    assert_eq!(notion.last_supported_at(), at(0), "the clock never moved");
}
