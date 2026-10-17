//! ADR 0105: what a Restricted skill returns is marked as it enters the turn, so the one
//! door refuses to let it leave; what the house's ordinary skills return is not.

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
