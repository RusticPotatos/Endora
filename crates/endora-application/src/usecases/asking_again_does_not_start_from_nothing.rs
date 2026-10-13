//! The gap a person found by using it: *"I keep asking the same question and it doesn't
//! get any context from the previous messages."*
//!
//! Every tool result inside a turn was threaded properly and then thrown away at its
//! end. A turn was seeded from prose alone, so the butler began the next one with its
//! own summary of what it said and **no trace of what it read** — and looked again, or
//! asked the person to say more.
//!
//! The findings were already stored. They were stored *for the person*, so a reply keeps
//! its expandable trail after a reload, and were never given back to the butler.

use super::Timestamp;
use super::seeded_from;
use endora_conversation::{ChatMessage, MessageRole};
use endora_kernel::ids::MessageId;

fn said(id: u128, role: MessageRole, text: &str) -> ChatMessage {
    ChatMessage::new(
        MessageId::new(id),
        role,
        text,
        Timestamp::from_unix_millis(id as i64),
    )
    .unwrap()
}

#[test]
fn what_a_past_turn_found_is_in_front_of_the_next_one() {
    let history = [
        said(1, MessageRole::User, "any events this week?"),
        said(
            2,
            MessageRole::Butler,
            "A few council committees are meeting.",
        ),
    ];
    let seeded = seeded_from(&history, &|id| {
        if id == "2" {
            vec!["zoning board Tuesday 5:30pm; housing committee Monday 9am".to_owned()]
        } else {
            Vec::new()
        }
    });
    let assistant = seeded
        .iter()
        .filter_map(|m| match m {
            super::TurnMessage::Assistant { text, .. } => Some(text.clone()),
            _ => None,
        })
        .next()
        .expect("the butler's turn is there");
    assert!(
        assistant.contains("zoning board Tuesday"),
        "asking again starts from nothing: {assistant}"
    );
    // And what it actually said is still there — the finding is added, not substituted.
    assert!(assistant.contains("council committees"), "{assistant}");
}

#[test]
fn a_turn_that_found_nothing_reads_exactly_as_it_did() {
    let history = [said(9, MessageRole::Butler, "Good morning.")];
    let seeded = seeded_from(&history, &|_| Vec::new());
    match &seeded[0] {
        super::TurnMessage::Assistant { text, .. } => assert_eq!(text, "Good morning."),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_long_finding_is_cut_and_a_pile_of_them_is_bounded() {
    // A weak model reads a long context worse than a short one, and a search result page
    // must not crowd out the conversation it belongs to.
    let history = [said(3, MessageRole::Butler, "Here you go.")];
    let seeded = seeded_from(&history, &|_| {
        (0..10)
            .map(|i| format!("{i}:{}", "x".repeat(900)))
            .collect()
    });
    let super::TurnMessage::Assistant { text, .. } = &seeded[0] else {
        panic!("expected the butler's turn")
    };
    assert!(text.len() < 1600, "carried {} characters", text.len());
    assert!(
        text.contains("0:"),
        "the first findings are the ones kept: {text:.80}"
    );
    assert!(!text.contains("5:"), "too many were carried");
}
