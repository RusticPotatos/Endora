use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A local model that always fails the deterministic check, whatever it is asked.
struct AlwaysNothing(AtomicUsize);
impl Butler for AlwaysNothing {
    fn respond(
        &self,
        _h: &[ChatMessage],
        _p: &[Preference],
        _c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ProposalError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ButlerReply::default())
    }
    fn deeper(&self) -> Option<crate::egress::Deeper> {
        Some(crate::egress::Deeper::new(std::sync::Arc::new(Deeper)))
    }
}

/// The stronger model, which answers.
struct Deeper;
impl Butler for Deeper {
    fn respond(
        &self,
        _h: &[ChatMessage],
        _p: &[Preference],
        _c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ProposalError> {
        Ok(ButlerReply {
            text: "The kitchen table light is off.".to_owned(),
            ..ButlerReply::default()
        })
    }
}

/// A local model with nothing better to fall back to — the default.
struct Alone;
impl Butler for Alone {
    fn respond(
        &self,
        _h: &[ChatMessage],
        _p: &[Preference],
        _c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ProposalError> {
        Ok(ButlerReply::default())
    }
}

#[test]
fn a_second_model_answers_after_the_local_one_keeps_failing() {
    // Reliability compounds: two independent attempts fail together only at
    // (1-p1)(1-p2). The trigger is a check Endora applied to the reply, never the
    // model's opinion of how it did.
    let local = AlwaysNothing(AtomicUsize::new(0));
    let reply = take_turn_retrying_empty(
        &local,
        &[TurnMessage::User(
            "is the kitchen table light on?".to_owned(),
        )],
        &[],
        &ButlerContext::default(),
        &mut |_| {},
        &mut Vec::new(),
    )
    .expect("a turn");

    assert_eq!(reply.text, "The kitchen table light is off.");
    assert!(reply.escalated, "an escalated reply must say so");
    // The local model is tried, and retried, BEFORE anything leaves the box.
    assert_eq!(local.0.load(Ordering::SeqCst), 3, "local attempts");
}

/// A deep model that reports exactly what it was sent, so the test can inspect it.
struct Echoes(std::sync::Mutex<String>);
impl Butler for Echoes {
    fn respond(
        &self,
        history: &[ChatMessage],
        _p: &[Preference],
        c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ProposalError> {
        let saw = format!(
            "{} | {}",
            history
                .iter()
                .map(ChatMessage::text)
                .collect::<Vec<_>>()
                .join(" "),
            c.present.join(" ")
        );
        *self.0.lock().unwrap() = saw.clone();
        // Answer using the placeholders it was given, as a model would.
        Ok(ButlerReply {
            text: format!("Noted: {saw}"),
            ..ButlerReply::default()
        })
    }
}

struct SilentWithDeeper(std::sync::Arc<Echoes>);
impl Butler for SilentWithDeeper {
    fn respond(
        &self,
        _h: &[ChatMessage],
        _p: &[Preference],
        _c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ProposalError> {
        Ok(ButlerReply::default())
    }
    fn deeper(&self) -> Option<crate::egress::Deeper> {
        Some(crate::egress::Deeper::new(
            self.0.clone() as std::sync::Arc<dyn Butler + Send + Sync>
        ))
    }
}

#[test]
fn nothing_personal_reaches_the_deep_model_and_the_answer_comes_back_right() {
    // The real brief facts. What leaves must not carry the name or the appointment;
    // what the person reads must carry both (ADR 0051).
    let deep = std::sync::Arc::new(Echoes(std::sync::Mutex::new(String::new())));
    let local = SilentWithDeeper(deep.clone());
    let context = ButlerContext {
        present: vec![
            "john is not home; on the Family calendar: Jane Doe & John Doe at \
             2026-07-31 18:30:00"
                .to_owned(),
        ],
        ..ButlerContext::default()
    };

    let reply = take_turn_retrying_empty(
        &local,
        &[TurnMessage::User("what is on tonight?".to_owned())],
        &[],
        &context,
        &mut |_| {},
        &mut Vec::new(),
    )
    .expect("a turn");

    let sent = deep.0.lock().unwrap().clone();
    for personal in ["john", "Jane Doe", "Doe"] {
        assert!(
            !sent.contains(personal),
            "{personal} left the house: {sent}"
        );
    }
    // And the person still reads their own words.
    assert!(reply.text.contains("Jane Doe & John Doe"), "{}", reply.text);
    assert!(reply.escalated);
}

#[test]
fn with_no_fallback_the_local_answer_stands() {
    let reply = take_turn_retrying_empty(
        &Alone,
        &[TurnMessage::User("hello".to_owned())],
        &[],
        &ButlerContext::default(),
        &mut |_| {},
        &mut Vec::new(),
    )
    .expect("a turn");
    assert!(reply.text.is_empty());
    assert!(!reply.escalated);
}

#[test]
fn saying_the_same_thing_again_counts_as_saying_nothing() {
    // Live: asked what it had done while the person was out, the butler reproduced
    // word for word the answer it had given the previous day, with the day's real
    // record in its context.
    let said = "I checked, but no specific activities were recorded while you were out.";
    let repeat = ButlerReply {
        text: said.to_owned(),
        ..ButlerReply::default()
    };
    let conversation = vec![
        TurnMessage::Assistant {
            text: said.to_owned(),
            tool_calls: Vec::new(),
        },
        TurnMessage::User("did you do anything while I was out?".to_owned()),
    ];
    assert!(repeats_its_last_answer(&repeat, &conversation));

    // A different answer is not a repeat, and a short confirmation is allowed to be
    // identical — "Done." twice is not a failure.
    let different = ButlerReply {
        text: "I posted a brief at 11:01 and two actions failed.".to_owned(),
        ..ButlerReply::default()
    };
    assert!(!repeats_its_last_answer(&different, &conversation));
    let terse = ButlerReply {
        text: "Done.".to_owned(),
        ..ButlerReply::default()
    };
    assert!(!repeats_its_last_answer(
        &terse,
        &[TurnMessage::Assistant {
            text: "Done.".to_owned(),
            tool_calls: Vec::new()
        }]
    ));
}
