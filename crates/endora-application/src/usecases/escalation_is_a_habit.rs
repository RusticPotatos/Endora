//! The third trigger: one call failed, the read-back named what really exists, and the
//! local model answered prose instead of retrying — measured 0/3 before this was
//! written. A deeper model gets one chance at exactly that moment.

use super::gave_up_after_a_failure;
use crate::ports::ButlerReply;
use crate::{ToolCall, TurnMessage};

fn after(result: &str) -> Vec<TurnMessage> {
    vec![
        TurnMessage::User("turn the kitchen switch off".to_owned()),
        TurnMessage::ToolResult {
            call_id: "c1".to_owned(),
            content: result.to_owned(),
        },
    ]
}

#[test]
fn prose_after_a_failed_action_escalates() {
    let waffle = ButlerReply {
        text: "Let's try again. Here is the request:".to_owned(),
        ..ButlerReply::default()
    };
    assert!(gave_up_after_a_failure(
        &waffle,
        &after("error: no_match_reason=NAME")
    ));
    assert!(gave_up_after_a_failure(
        &waffle,
        &after("not run — reached this turn's action limit; answer with what you have.")
    ));
}

#[test]
fn still_trying_is_the_loop_working_not_a_dead_end() {
    let retrying = ButlerReply {
        tool_calls: vec![ToolCall {
            id: "c2".to_owned(),
            capability: "home.HassTurnOff".to_owned(),
            input_json: "{}".to_owned(),
        }],
        ..ButlerReply::default()
    };
    assert!(!gave_up_after_a_failure(
        &retrying,
        &after("error: no_match_reason=NAME")
    ));
}

#[test]
fn prose_after_a_success_is_just_an_answer() {
    let answer = ButlerReply {
        text: "It's off now.".to_owned(),
        ..ButlerReply::default()
    };
    assert!(!gave_up_after_a_failure(&answer, &after("action_done")));
    // And a turn with no tool results at all has nothing to have given up on.
    assert!(!gave_up_after_a_failure(
        &answer,
        &[TurnMessage::User("hi".to_owned())]
    ));
}
