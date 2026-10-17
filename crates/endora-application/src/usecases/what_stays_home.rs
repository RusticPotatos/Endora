//! ADR 0105: what a Restricted skill returns is marked as it enters the turn, so the one
//! door refuses to let it leave; what the house's ordinary skills return is not. The marks are
//! kept with what is stored, so nothing leaves on the next turn either.

use super::run_tool_turn;
use crate::egress::RESTRICTED_MARK;
use crate::ports::{ButlerContext, ButlerReply};
use crate::{ToolCall, TurnMessage};
use endora_capabilities::{CapabilityRunner, CapabilitySpec};
use endora_kernel::Reversibility;

/// The owner's movement history, and the containers.
struct TheHouse;
impl CapabilityRunner for TheHouse {
    fn available(&self) -> Vec<CapabilitySpec> {
        ["location", "homelab-docker"]
            .into_iter()
            .map(|id| CapabilitySpec {
                id: id.to_owned(),
                wants_place: false,
                third_party: false,
                description: "read the house".to_owned(),
                configured: true,
                autonomous: true,
                input_schema: None,
                reversibility: Reversibility::Observe,
            })
            .collect()
    }
    fn run(&self, id: &str, _input: &str) -> Result<String, String> {
        Ok(if id == "location" {
            "home, office, home"
        } else {
            "12 running"
        }
        .to_owned())
    }
}

/// Calls one skill, then notes what came back.
struct CallsThenReads {
    skill: &'static str,
    turns: std::cell::Cell<usize>,
    saw: std::cell::RefCell<String>,
}
impl crate::ports::Butler for CallsThenReads {
    fn respond(
        &self,
        _h: &[endora_conversation::ChatMessage],
        _p: &[endora_understanding::Preference],
        _c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ports::ProposalError> {
        Ok(ButlerReply::default())
    }
    fn take_turn(
        &self,
        conversation: &[TurnMessage],
        _p: &[endora_understanding::Preference],
        _c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ports::ProposalError> {
        let n = self.turns.get();
        self.turns.set(n + 1);
        if n == 0 {
            return Ok(ButlerReply {
                tool_calls: vec![ToolCall {
                    id: "c0".to_owned(),
                    capability: self.skill.to_owned(),
                    input_json: "{}".to_owned(),
                }],
                ..ButlerReply::default()
            });
        }
        if let Some(TurnMessage::ToolResult { content, .. }) = conversation.last() {
            content.clone_into(&mut self.saw.borrow_mut());
        }
        Ok(ButlerReply {
            text: "Done.".to_owned(),
            ..ButlerReply::default()
        })
    }
}

fn what_came_back(skill: &'static str) -> String {
    let butler = CallsThenReads {
        skill,
        turns: std::cell::Cell::new(0),
        saw: std::cell::RefCell::new(String::new()),
    };
    run_tool_turn(
        &butler,
        &TheHouse,
        &super::tests::FakeAudit::default(),
        &super::OutcomeSink::unmotivated(&super::tests::FakeOutcomes::default()),
        &super::tests::SeqIds::default(),
        &super::tests::FixedClock(0),
        &super::tests::one_user_turn("where have I been, and is the NAS fine?"),
        &|_id| Vec::new(),
        &[],
        &ButlerContext::default(),
        6,
        &mut |_| {},
        &mut |_t: &str| {},
        &mut Vec::new(),
        &mut Vec::new(),
    )
    .expect("the turn answers");
    butler.saw.into_inner()
}

#[test]
fn a_restricted_skills_result_is_marked_and_an_ordinary_one_is_not() {
    let trail = what_came_back("location");
    assert!(trail.starts_with(RESTRICTED_MARK), "{trail}");
    assert!(
        trail.contains("home, office, home"),
        "the local model still reads it"
    );
    let status = what_came_back("homelab-docker");
    assert_eq!(status, "12 running");
}

/// A chat that keeps what each reply found, as the node's store does.
#[derive(Default)]
struct Remembers {
    messages: std::cell::RefCell<Vec<endora_conversation::ChatMessage>>,
    found: std::cell::RefCell<std::collections::HashMap<String, Vec<String>>>,
}
impl endora_conversation::ChatRepository for Remembers {
    fn append(
        &self,
        message: &endora_conversation::ChatMessage,
    ) -> Result<(), endora_kernel::RepositoryError> {
        self.messages.borrow_mut().push(message.clone());
        Ok(())
    }
    fn list(
        &self,
    ) -> Result<Vec<endora_conversation::ChatMessage>, endora_kernel::RepositoryError> {
        Ok(self.messages.borrow().clone())
    }
    fn between(
        &self,
        _from_ms: i64,
        _to_ms: i64,
    ) -> Result<Vec<endora_conversation::ChatMessage>, endora_kernel::RepositoryError> {
        self.list()
    }
    fn days(&self, _offset: i64) -> Result<Vec<(String, usize)>, endora_kernel::RepositoryError> {
        Ok(Vec::new())
    }
    fn what_it_found(
        &self,
        message_id: &str,
    ) -> Result<Vec<String>, endora_kernel::RepositoryError> {
        Ok(self
            .found
            .borrow()
            .get(message_id)
            .cloned()
            .unwrap_or_default())
    }
}

/// A mailbox: a stranger's words.
struct TheMail;
impl CapabilityRunner for TheMail {
    fn available(&self) -> Vec<CapabilitySpec> {
        vec![CapabilitySpec {
            id: "mail".to_owned(),
            wants_place: false,
            third_party: true,
            description: "read the mail".to_owned(),
            configured: true,
            autonomous: true,
            input_schema: None,
            reversibility: Reversibility::Observe,
        }]
    }
    fn run(&self, _id: &str, _input: &str) -> Result<String, String> {
        Ok("From a shop: your parcel is delayed".to_owned())
    }
}

/// Fails the test by being spoken to at all.
struct MustNotBeReached;
impl crate::ports::Butler for MustNotBeReached {
    fn respond(
        &self,
        _h: &[endora_conversation::ChatMessage],
        _p: &[endora_understanding::Preference],
        _c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ports::ProposalError> {
        panic!("something private left the house");
    }
    fn take_turn(
        &self,
        _c: &[TurnMessage],
        _p: &[endora_understanding::Preference],
        _x: &ButlerContext,
    ) -> Result<ButlerReply, crate::ports::ProposalError> {
        panic!("something private left the house");
    }
}

/// One turn through the whole chat path, with what each reply found kept as the node keeps
/// it; then what the next turn would send out through the door.
fn the_next_turn_after(
    skill: &'static str,
    capabilities: &dyn CapabilityRunner,
) -> (String, Result<ButlerReply, crate::egress::DoorRefusal>) {
    let chat = Remembers::default();
    let butler = CallsThenReads {
        skill,
        turns: std::cell::Cell::new(0),
        saw: std::cell::RefCell::new(String::new()),
    };
    let mut found = Vec::new();
    let (reply, _) = super::send_to_butler_streaming(
        &chat,
        &super::tests::FakeStore::default(),
        &super::tests::FakeOutcomes::default(),
        capabilities,
        &butler,
        &super::tests::FakeAudit::default(),
        None,
        &super::tests::SeqIds::default(),
        &super::tests::FixedClock(0),
        &ButlerContext::default(),
        "what is new?",
        &mut |_| {},
        &mut |step| found.extend(step.output),
        &mut Vec::new(),
    )
    .expect("the turn answers");
    chat.found
        .borrow_mut()
        .insert(reply.id().value().to_string(), found);
    let history = endora_conversation::ChatRepository::list(&chat).unwrap();
    let mut next = super::seeded_from(&history, &|id| {
        endora_conversation::ChatRepository::what_it_found(&chat, id).unwrap()
    });
    next.push(TurnMessage::User("thanks, anything else?".to_owned()));
    let door = crate::egress::Deeper::new(std::sync::Arc::new(MustNotBeReached));
    (
        reply.text().to_owned(),
        door.continue_turn(&next, &[], &ButlerContext::default()),
    )
}

#[test]
fn a_restricted_result_stays_home_on_the_next_turn_too() {
    let (stored, next) = the_next_turn_after("location", &TheHouse);
    assert!(
        stored.starts_with(RESTRICTED_MARK),
        "the reply that read it is kept private: {stored}"
    );
    assert!(
        matches!(next, Err(crate::egress::DoorRefusal::Restricted)),
        "{next:?}"
    );
}

#[test]
fn a_strangers_words_stay_home_on_the_next_turn_too() {
    let (stored, next) = the_next_turn_after("mail", &TheMail);
    assert!(!stored.starts_with(RESTRICTED_MARK), "{stored}");
    assert!(
        matches!(next, Err(crate::egress::DoorRefusal::AStrangerSpoke)),
        "{next:?}"
    );
}

/// The brief, and a light that is on.
struct BriefAndLights;
impl CapabilityRunner for BriefAndLights {
    fn available(&self) -> Vec<CapabilitySpec> {
        vec![CapabilitySpec {
            id: "brief".to_owned(),
            wants_place: false,
            third_party: false,
            description: "assemble the brief".to_owned(),
            configured: true,
            autonomous: true,
            input_schema: None,
            reversibility: Reversibility::Observe,
        }]
    }
    fn current_states(&self) -> Vec<(String, String)> {
        vec![("light.kitchen_table".to_owned(), "on".to_owned())]
    }
    fn run(&self, _id: &str, _input: &str) -> Result<String, String> {
        Ok("- John Doe is not home\n- outside it is 77F".to_owned())
    }
}

/// A local model with a cloud deep model behind it that must never be asked.
struct LocalWithACloudBehind;
impl crate::ports::Butler for LocalWithACloudBehind {
    fn respond(
        &self,
        _h: &[endora_conversation::ChatMessage],
        _p: &[endora_understanding::Preference],
        _c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ports::ProposalError> {
        Ok(ButlerReply::default())
    }
    fn deeper(&self) -> Option<crate::egress::Deeper> {
        Some(crate::egress::Deeper::new(std::sync::Arc::new(
            MustNotBeReached,
        )))
    }
}

#[test]
fn a_private_standing_answer_is_never_worded_outside_and_is_kept_private() {
    for (asked, says) in [
        (format!("{RESTRICTED_MARK}brief me"), "77f"),
        (format!("{RESTRICTED_MARK}are any lights on?"), "kitchen"),
    ] {
        let chat = Remembers::default();
        let (reply, _) = super::send_to_butler(
            &chat,
            &super::tests::FakeStore::default(),
            &super::tests::FakeOutcomes::default(),
            &BriefAndLights,
            &LocalWithACloudBehind,
            &super::tests::FakeAudit::default(),
            &super::tests::SeqIds::default(),
            &super::tests::FixedClock(0),
            &ButlerContext::default(),
            &asked,
        )
        .expect("answered at home");
        assert!(
            reply.text().starts_with(RESTRICTED_MARK),
            "{}",
            reply.text()
        );
        assert!(
            reply.text().to_lowercase().contains(says),
            "{}",
            reply.text()
        );
    }
}

/// Echoes what it was told, and records what learning was shown.
#[derive(Default)]
struct Echoes {
    learned_from: std::cell::RefCell<Vec<String>>,
}
impl crate::ports::Butler for Echoes {
    fn respond(
        &self,
        history: &[endora_conversation::ChatMessage],
        _p: &[endora_understanding::Preference],
        _c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ports::ProposalError> {
        self.learned_from
            .borrow_mut()
            .extend(history.iter().map(|m| m.text().to_owned()));
        Ok(ButlerReply::default())
    }
    fn take_turn(
        &self,
        conversation: &[TurnMessage],
        _p: &[endora_understanding::Preference],
        _c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ports::ProposalError> {
        let last = match conversation.last() {
            Some(TurnMessage::User(t)) => t.clone(),
            _ => String::new(),
        };
        Ok(ButlerReply {
            text: format!("You said {last}."),
            ..ButlerReply::default()
        })
    }
}

#[test]
fn what_endora_writes_with_an_account_number_in_it_is_kept_private() {
    let chat = Remembers::default();
    let butler = Echoes::default();
    let (reply, _) = super::send_to_butler(
        &chat,
        &super::tests::FakeStore::default(),
        &super::tests::FakeOutcomes::default(),
        &TheHouse,
        &butler,
        &super::tests::FakeAudit::default(),
        &super::tests::SeqIds::default(),
        &super::tests::FixedClock(0),
        &ButlerContext::default(),
        "my card is 4111 1111 1111 1111",
    )
    .expect("answered at home");
    assert!(
        reply.text().starts_with(RESTRICTED_MARK),
        "{}",
        reply.text()
    );
    assert!(
        butler.learned_from.borrow().is_empty(),
        "learning read an account number: {:?}",
        butler.learned_from.borrow()
    );
    let posted = super::post_butler_message(
        &chat,
        &super::tests::SeqIds::default(),
        &super::tests::FixedClock(0),
        "Your account 12345678901 is overdrawn.",
    )
    .unwrap();
    assert!(
        posted.text().starts_with(RESTRICTED_MARK),
        "{}",
        posted.text()
    );
    let plain = super::post_butler_message(
        &chat,
        &super::tests::SeqIds::default(),
        &super::tests::FixedClock(0),
        "Good morning.",
    )
    .unwrap();
    assert_eq!(plain.text(), "Good morning.");
}

#[test]
fn a_running_summary_with_an_account_number_in_it_is_kept_private() {
    struct Holds(std::cell::RefCell<Option<crate::ports::ConversationSummary>>);
    impl crate::ports::ConversationSummaryStore for Holds {
        fn get(&self) -> Option<crate::ports::ConversationSummary> {
            self.0.borrow().clone()
        }
        fn set(&self, summary: crate::ports::ConversationSummary) {
            *self.0.borrow_mut() = Some(summary);
        }
    }
    struct Summarises;
    impl crate::ports::Butler for Summarises {
        fn respond(
            &self,
            _h: &[endora_conversation::ChatMessage],
            _p: &[endora_understanding::Preference],
            _c: &ButlerContext,
        ) -> Result<ButlerReply, crate::ports::ProposalError> {
            Ok(ButlerReply::default())
        }
        fn summarize(&self, _prior: &str, _t: &str) -> Result<String, crate::ports::ProposalError> {
            Ok("They gave account 12345678901.".to_owned())
        }
    }
    let full: Vec<endora_conversation::ChatMessage> = (0..30)
        .map(|i| {
            endora_conversation::ChatMessage::new(
                endora_kernel::ids::MessageId::new(i + 1),
                endora_conversation::MessageRole::User,
                &format!("message {i}"),
                endora_kernel::ids::Timestamp::from_unix_millis(i64::try_from(i).unwrap()),
            )
            .unwrap()
        })
        .collect();
    let store = Holds(std::cell::RefCell::new(None));
    let (_, summary) = super::compact_history(&Summarises, &store, &full, 12);
    let summary = summary.expect("a summary");
    assert!(summary.starts_with(RESTRICTED_MARK), "{summary}");
}

/// The brief, as the node offers it: other people's words among the house's own.
struct TheBrief;
impl CapabilityRunner for TheBrief {
    fn available(&self) -> Vec<CapabilitySpec> {
        vec![CapabilitySpec {
            id: "brief".to_owned(),
            wants_place: false,
            third_party: true,
            description: "assemble the brief".to_owned(),
            configured: true,
            autonomous: true,
            input_schema: None,
            reversibility: Reversibility::Observe,
        }]
    }
    fn run(&self, _id: &str, _input: &str) -> Result<String, String> {
        Ok("- From a shop: your parcel is delayed".to_owned())
    }
}

#[test]
fn a_standing_answer_is_marked_for_what_is_in_it() {
    // The brief is Confidential, so its reply is not private; what it carries from outside
    // keeps the stranger's mark, so the next turn stays home.
    let chat = Remembers::default();
    let mut found = Vec::new();
    let (reply, _) = super::send_to_butler_streaming(
        &chat,
        &super::tests::FakeStore::default(),
        &super::tests::FakeOutcomes::default(),
        &TheBrief,
        &Echoes::default(),
        &super::tests::FakeAudit::default(),
        None,
        &super::tests::SeqIds::default(),
        &super::tests::FixedClock(0),
        &ButlerContext::default(),
        "brief me",
        &mut |_| {},
        &mut |step| found.extend(step.output),
        &mut Vec::new(),
    )
    .expect("the brief answers");
    assert!(
        !reply.text().starts_with(RESTRICTED_MARK),
        "{}",
        reply.text()
    );
    assert!(
        found
            .iter()
            .any(|f| f.starts_with(crate::egress::STRANGER_MARK)),
        "{found:?}"
    );
    assert_eq!(
        super::marked_result(
            "location",
            false,
            super::StepStatus::Done,
            "home".to_owned()
        ),
        format!("{RESTRICTED_MARK}home")
    );
    assert_eq!(
        super::marked_result("brief", false, super::StepStatus::Done, "a".to_owned()),
        "a"
    );
}
