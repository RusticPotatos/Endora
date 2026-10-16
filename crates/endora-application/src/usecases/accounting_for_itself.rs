use super::*;
use endora_kernel::{RepositoryError, Timestamp};

struct Said(Vec<ChatMessage>);
impl ChatRepository for Said {
    fn append(&self, _m: &ChatMessage) -> Result<(), RepositoryError> {
        Ok(())
    }
    fn list(&self) -> Result<Vec<ChatMessage>, RepositoryError> {
        Ok(self.0.clone())
    }
    fn between(&self, from_ms: i64, to_ms: i64) -> Result<Vec<ChatMessage>, RepositoryError> {
        Ok(self
            .0
            .iter()
            .filter(|m| m.at().unix_millis() >= from_ms && m.at().unix_millis() <= to_ms)
            .cloned()
            .collect())
    }
    fn days(&self, _offset_minutes: i64) -> Result<Vec<(String, usize)>, RepositoryError> {
        Ok(Vec::new())
    }
}

struct Nothing;
impl OutcomeRepository for Nothing {
    fn save(&self, _o: &Outcome) -> Result<(), RepositoryError> {
        Ok(())
    }
    fn get(&self, _id: crate::OutcomeId) -> Result<Option<Outcome>, RepositoryError> {
        Ok(None)
    }
    fn list(&self) -> Result<Vec<Outcome>, RepositoryError> {
        Ok(Vec::new())
    }
}
impl endora_capabilities::ConfigWriteLog for Nothing {
    fn record(&self, _w: &endora_capabilities::ConfigWrite) -> Result<(), RepositoryError> {
        Ok(())
    }
    fn writes(
        &self,
        _limit: usize,
    ) -> Result<Vec<endora_capabilities::ConfigWrite>, RepositoryError> {
        Ok(Vec::new())
    }
    fn write(
        &self,
        _id: u128,
    ) -> Result<Option<endora_capabilities::ConfigWrite>, RepositoryError> {
        Ok(None)
    }
    fn mark_undone(&self, _id: u128) -> Result<(), RepositoryError> {
        Ok(())
    }
}

fn message(role: MessageRole, text: &str, at_ms: i64) -> ChatMessage {
    ChatMessage::new(
        crate::MessageId::new(at_ms as u128),
        role,
        text,
        Timestamp::from_unix_millis(at_ms),
    )
    .expect("valid message")
}

/// 11:01 and 04:18 on the day the real failure happened.
const BRIEF_AT: i64 = 39_660_000;
const CHECKIN_AT: i64 = 15_480_000;

#[test]
fn the_brief_it_posted_is_in_the_answer() {
    // The live failure: asked "nothing proactive done today?", it said "No specific
    // activities were recorded today" — four hours after posting a real brief.
    let said = Said(vec![
        message(
            MessageRole::Butler,
            "Good morning, sir. The kitchen and garage lights are on, while others like the bedroom are off.",
            BRIEF_AT,
        ),
        message(MessageRole::User, "Good afternoon", BRIEF_AT + 1_000),
        message(
            MessageRole::Butler,
            "It's good afternoon! How can I assist you?",
            BRIEF_AT + 2_000,
        ),
    ]);
    let report = what_it_has_been_doing(&said, &Nothing, &Nothing, 0, A_DAY_MS).expect("report");

    assert!(report.contains("11:01"), "{report}");
    assert!(report.contains("unprompted"), "{report}");
    assert!(report.contains("Good morning, sir."), "{report}");
    // The reply it gave when ASKED is not something it did unprompted, and must not be
    // reported as though it were.
    assert!(!report.contains("good afternoon! How can"), "{report}");
}

#[test]
fn a_quiet_day_says_so_without_claiming_nothing_was_recorded() {
    // "No specific activities were recorded today" was false, and would have been a
    // different claim from "nothing happened" even if it were true. Endora reports what
    // it did, not what its own storage managed to capture.
    let report =
        what_it_has_been_doing(&Said(Vec::new()), &Nothing, &Nothing, 0, A_DAY_MS).expect("report");
    assert!(report.starts_with("Nothing."), "{report}");
    assert!(!report.to_lowercase().contains("recorded"), "{report}");
}

#[test]
fn two_unprompted_messages_are_both_reported_oldest_first() {
    let said = Said(vec![
        message(
            MessageRole::Butler,
            "None of the functions listed involve a person domain.",
            CHECKIN_AT,
        ),
        message(
            MessageRole::Butler,
            "Good morning, sir. The kitchen lights are on.",
            BRIEF_AT,
        ),
    ]);
    let report = what_it_has_been_doing(&said, &Nothing, &Nothing, 0, A_DAY_MS).expect("report");
    let checkin = report.find("04:18").expect("the check-in");
    let brief = report.find("11:01").expect("the brief");
    assert!(checkin < brief, "oldest first, got: {report}");
}
