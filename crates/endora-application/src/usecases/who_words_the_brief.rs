use super::*;
use std::sync::Mutex;

/// A stand-in for somebody else's model, which records exactly what it was sent.
struct Elsewhere(Mutex<String>);
impl Butler for Elsewhere {
    fn respond(
        &self,
        history: &[ChatMessage],
        _p: &[Preference],
        c: &ButlerContext,
    ) -> Result<ButlerReply, crate::ProposalError> {
        *self.0.lock().unwrap() = format!(
            "{} || present={:?} understands={:?} names={:?}",
            history
                .iter()
                .map(ChatMessage::text)
                .collect::<Vec<_>>()
                .join(" "),
            c.present,
            c.understanding,
            c.target_aliases
        );
        Ok(ButlerReply {
            text: "Good morning. <person 1> is out; <event 1> is at 18:30.".to_owned(),
            ..ButlerReply::default()
        })
    }
}

const REAL_FACTS: &str = "- john is not home; on the Family calendar:                               Jane Doe & John Doe at 2026-07-31 18:30:00";

#[test]
fn only_the_facts_leave_and_they_leave_disguised() {
    let deep = std::sync::Arc::new(Elsewhere(Mutex::new(String::new())));
    let door = crate::egress::Deeper::new(deep.clone());
    let written =
        word_the_brief(&door, REAL_FACTS, &FakeIds::default(), &FixedClock(0)).expect("a brief");

    let sent = deep.0.lock().unwrap().clone();
    for personal in ["john", "Jane Doe", "Doe"] {
        assert!(
            !sent.contains(personal),
            "{personal} left the house: {sent}"
        );
    }
    // The context is bare on purpose: writing prose is no reason for beliefs, the
    // names the person uses, or how skills have landed to travel.
    assert!(sent.contains("present=[]"), "{sent}");
    assert!(sent.contains("understands=[]"), "{sent}");
    assert!(sent.contains("names=[]"), "{sent}");

    // And what the person reads is their own words again.
    assert!(written.contains("john"), "{written}");
    assert!(written.contains("Jane Doe & John Doe"), "{written}");
}

#[test]
fn only_what_the_writing_left_out_is_printed_again() {
    // The real second live brief. The facts and the prose say the same things in
    // different words, which is why an exact-substring check failed here.
    let facts = "- john is not home\n                     - on the Family calendar: Yardwork in the morning - Jane Doe and John Doe\n                     - outside it is 77F and partly cloudy";
    let written = "Good morning! Quick brief for you: john is not home right now. On                        the Family calendar, there's Yardwork in the morning - Jane Doe                        and John Doe at 2026-08-01. Outside it's 77F and partly cloudy.";
    assert!(
        not_yet_said(written, facts).is_empty(),
        "{:?}",
        not_yet_said(written, facts)
    );
}

#[test]
fn a_raw_transition_is_recognised_and_a_curated_fact_is_not() {
    use super::is_a_raw_transition;
    // The exact shape what_changed_lately builds.
    assert!(is_a_raw_transition(
        "home-assistant::sensor.bambam_pressure changed from 14.484 to 14.485"
    ));
    // The curated about_the_person lines this must never touch.
    assert!(!is_a_raw_transition("rustic is not home"));
    assert!(!is_a_raw_transition(
        "on the Family calendar: Appointment at Modern Pet Salon Bonnie at 2026-08-11 07:00:00"
    ));
    assert!(!is_a_raw_transition("outside it is 75°F and partly cloudy"));
}

#[test]
fn the_morning_facts_drop_raw_transitions_and_keep_curated_ones() {
    // Live: a brief's "Around the house" section was a page of
    // `sensor.x changed from … to …` lines sitting beside the calendar entry
    // that was the only thing anyone wanted to read. The feed is shared with
    // the chat turn, which DOES want the raw transition — this filters only
    // what the brief reads from it.
    let ctx = ButlerContext {
        present: vec![
            "rustic is not home".to_owned(),
            "home-assistant::sensor.bambam_pressure changed from 14.484 to 14.485".to_owned(),
            "home-assistant::sun.sun changed from below_horizon to above_horizon".to_owned(),
            "outside it is 75°F and partly cloudy".to_owned(),
        ],
        ..ButlerContext::default()
    };
    let said = whats_worth_saying_this_morning(&ctx, &[], 0).expect("something to say");
    assert!(said.contains("rustic is not home"), "{said}");
    assert!(said.contains("partly cloudy"), "{said}");
    assert!(!said.contains("bambam_pressure"), "{said}");
    assert!(!said.contains("sun.sun"), "{said}");
}

#[test]
fn a_problem_statement_says_how_long_it_has_been_wrong() {
    // The first brief to carry these said "has not answered since earlier today" about
    // seven things silent for three days, because the duration was measured against
    // when it started rather than against now. The duration is the entire reason a
    // problem statement is not a status line (ADR 0056).
    const DAY: i64 = 86_400_000;
    let lamp = endora_capabilities::StandingTrouble {
        server: "home-assistant".to_owned(),
        thing: "living room lamp".to_owned(),
        trouble: "unavailable".to_owned(),
        since_ms: 0,
        accepted: false,
    };
    let said = whats_worth_saying_this_morning(&ButlerContext::default(), &[&lamp], 3 * DAY)
        .expect("something to say");
    assert!(said.contains("for 3 days"), "{said}");
    assert!(!said.contains("earlier today"), "{said}");
}

#[test]
fn a_fact_the_writing_dropped_is_still_shown() {
    // The bias is deliberate. A fact wrongly thought missing is printed twice, which is
    // untidy; one wrongly thought covered is lost, and losing them is what kept
    // happening.
    let facts = "- john is not home\n- the porch light has not answered for 4 days";
    let written = "Good morning. john is not home right now.";
    let missed = not_yet_said(written, facts);
    assert_eq!(missed.len(), 1);
    assert!(missed[0].contains("porch light"), "{missed:?}");
}

#[test]
fn a_deep_model_that_says_nothing_does_not_become_the_brief() {
    // A brief must never depend on somebody else's service being up.
    struct Mute;
    impl Butler for Mute {
        fn respond(
            &self,
            _h: &[ChatMessage],
            _p: &[Preference],
            _c: &ButlerContext,
        ) -> Result<ButlerReply, crate::ProposalError> {
            Ok(ButlerReply::default())
        }
    }
    assert_eq!(
        word_the_brief(
            &crate::egress::Deeper::new(std::sync::Arc::new(Mute)),
            REAL_FACTS,
            &FakeIds::default(),
            &FixedClock(0)
        ),
        None
    );
}

#[derive(Default)]
struct FakeIds(std::sync::atomic::AtomicU64);
impl IdSource for FakeIds {
    fn new_id(&self) -> u128 {
        u128::from(self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst)) + 1
    }
}
struct FixedClock(i64);
impl Clock for FixedClock {
    fn now(&self) -> endora_kernel::Timestamp {
        endora_kernel::Timestamp::from_unix_millis(self.0)
    }
}
