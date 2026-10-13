use super::*;
use endora_kernel::Timestamp;
use std::sync::Mutex;

#[derive(Default)]
struct Recorded(Mutex<Vec<Outcome>>);
impl OutcomeRepository for Recorded {
    fn save(&self, o: &Outcome) -> Result<(), endora_kernel::RepositoryError> {
        let mut held = self.0.lock().unwrap();
        if let Some(slot) = held.iter_mut().find(|x| x.id() == o.id()) {
            *slot = o.clone();
        } else {
            held.push(o.clone());
        }
        Ok(())
    }
    fn get(&self, id: crate::OutcomeId) -> Result<Option<Outcome>, endora_kernel::RepositoryError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|x| x.id() == id)
            .cloned())
    }
    fn list(&self) -> Result<Vec<Outcome>, endora_kernel::RepositoryError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

fn acted(id: u128, claim: &str, at_ms: i64) -> Outcome {
    Outcome::record(
        crate::OutcomeId::new(id),
        "home.HassTurnOn",
        "{}",
        claim,
        Some("a reading"),
        Timestamp::from_unix_millis(at_ms),
        None,
        Some(true),
    )
    .expect("valid")
}

fn said(role: MessageRole, text: &str, at_ms: i64) -> ChatMessage {
    ChatMessage::new(
        crate::MessageId::new(at_ms as u128),
        role,
        text,
        Timestamp::from_unix_millis(at_ms),
    )
    .expect("valid")
}

#[test]
fn a_repeated_request_marks_the_action_as_not_having_helped() {
    // 116 outcomes carried zero reactions across two placements of the ask. The signal
    // was in the transcript the whole time: somebody who asks again has told you.
    let outcomes = Recorded::default();
    outcomes.save(&acted(1, "turned it on", 2_000)).unwrap();
    let history = vec![said(
        MessageRole::User,
        "turn on the kitchen table light",
        1_000,
    )];

    let n = note_what_the_repeat_says(
        &outcomes,
        &history,
        "turn on the kitchen table light",
        Timestamp::from_unix_millis(60_000),
    )
    .unwrap();

    assert_eq!(n, 1);
    assert_eq!(
        outcomes
            .get(crate::OutcomeId::new(1))
            .unwrap()
            .unwrap()
            .reaction(),
        Some(Reaction::DidNotHelp)
    );
}

#[test]
fn silence_is_never_taken_as_success() {
    // One direction only. Somebody who says nothing may be satisfied, or may have given
    // up and gone to the switch, and there is no way to tell them apart — so a
    // different next message marks nothing at all.
    let outcomes = Recorded::default();
    outcomes.save(&acted(1, "turned it on", 2_000)).unwrap();
    let history = vec![said(
        MessageRole::User,
        "turn on the kitchen table light",
        1_000,
    )];

    let n = note_what_the_repeat_says(
        &outcomes,
        &history,
        "what's the weather like?",
        Timestamp::from_unix_millis(60_000),
    )
    .unwrap();
    assert_eq!(n, 0);
    assert_eq!(
        outcomes
            .get(crate::OutcomeId::new(1))
            .unwrap()
            .unwrap()
            .reaction(),
        None
    );
}

#[test]
fn the_opposite_request_is_not_a_repeat() {
    // "turn it off" after "turn it on" is a new intention, not a complaint — and
    // marking it as failure would punish an action that worked.
    let outcomes = Recorded::default();
    outcomes.save(&acted(1, "turned it on", 2_000)).unwrap();
    let history = vec![said(
        MessageRole::User,
        "turn on the kitchen table light",
        1_000,
    )];
    assert_eq!(
        note_what_the_repeat_says(
            &outcomes,
            &history,
            "turn off the kitchen table light",
            Timestamp::from_unix_millis(60_000),
        )
        .unwrap(),
        0
    );
}

#[test]
fn the_same_words_tomorrow_are_a_new_request() {
    // A verdict on an action has to be near it in time, or every evening's "turn on the
    // kitchen light" condemns yesterday's.
    let outcomes = Recorded::default();
    outcomes.save(&acted(1, "turned it on", 2_000)).unwrap();
    let history = vec![said(
        MessageRole::User,
        "turn on the kitchen table light",
        1_000,
    )];
    assert_eq!(
        note_what_the_repeat_says(
            &outcomes,
            &history,
            "turn on the kitchen table light",
            Timestamp::from_unix_millis(86_400_000),
        )
        .unwrap(),
        0
    );
}

#[test]
fn an_action_that_already_errored_is_left_alone() {
    // It is visible as a failure without anybody's help, and marking it would count the
    // same miss twice.
    let outcomes = Recorded::default();
    outcomes
        .save(&acted(1, "error: no matching entity", 2_000))
        .unwrap();
    let history = vec![said(
        MessageRole::User,
        "turn on the kitchen table light",
        1_000,
    )];
    assert_eq!(
        note_what_the_repeat_says(
            &outcomes,
            &history,
            "turn on the kitchen table light",
            Timestamp::from_unix_millis(60_000),
        )
        .unwrap(),
        0
    );
}
