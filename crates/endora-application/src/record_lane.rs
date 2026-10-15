//! What may run in the lane for goals Endora started for itself (ADR 0094): which model it
//! may use, and when it may run at all. Both are decided here, by code, so that neither
//! depends on a caller remembering.
use crate::ports::{Butler, ButlerContext, ButlerReply, NeedKind, ProposalError, TurnMessage};
use crate::{
    CapabilityRead, CapabilityRunner, CapabilitySpec, ChatMessage, Decision, Preference, Timestamp,
};

/// How long the person must have been silent before a self-started goal takes a pass.
///
/// Ten minutes to start with (ADR 0094). One GPU serves chat and background work, so the
/// worker steps aside for anyone who has just spoken; a number to argue from a week of the
/// activity trail, not a constant to defend.
pub const QUIET_WINDOW_MS: i64 = 10 * 60 * 1000;

/// Whether the person has been quiet long enough for a self-started goal to run a pass.
///
/// `last_spoke` is when the person last wrote in chat, `None` if they never have. A message
/// dated *after* `now` (the clock moved, or the record is wrong) is not quiet: when the
/// answer cannot be trusted the lane waits, because waiting costs nothing and a pass over
/// someone mid-conversation does.
#[must_use]
pub fn quiet_enough(now: Timestamp, last_spoke: Option<Timestamp>) -> bool {
    let Some(last_spoke) = last_spoke else {
        return true;
    };
    let silent_for = now.unix_millis().saturating_sub(last_spoke.unix_millis());
    silent_for >= QUIET_WINDOW_MS
}

/// A butler with no deep model to escalate to.
///
/// Self-started work never reaches the paid model. A setting someone must remember would be
/// the wrong way to keep that: `deeper()` is the one door a turn escalates through, so a
/// butler whose `deeper()` is `None` has no door to walk through, whatever is configured
/// behind the one it wraps. Everything else is the wrapped butler, unchanged.
pub struct LocalOnly<'a> {
    inner: &'a dyn Butler,
}

impl<'a> LocalOnly<'a> {
    /// Wraps `inner`, keeping its local model and dropping its deep one.
    #[must_use]
    pub fn new(inner: &'a dyn Butler) -> Self {
        Self { inner }
    }
}

impl Butler for LocalOnly<'_> {
    fn respond(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        self.inner.respond(history, preferences, context)
    }

    fn respond_streaming(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<ButlerReply, ProposalError> {
        self.inner
            .respond_streaming(history, preferences, context, on_token)
    }

    fn take_turn_streaming(
        &self,
        conversation: &[TurnMessage],
        preferences: &[Preference],
        context: &ButlerContext,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<ButlerReply, ProposalError> {
        self.inner
            .take_turn_streaming(conversation, preferences, context, on_token)
    }

    fn take_turn(
        &self,
        conversation: &[TurnMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        self.inner.take_turn(conversation, preferences, context)
    }

    fn classify_need(&self, question: &str) -> Option<NeedKind> {
        self.inner.classify_need(question)
    }

    fn summarize(&self, prior_summary: &str, transcript: &str) -> Result<String, ProposalError> {
        self.inner.summarize(prior_summary, transcript)
    }

    // The point of the type. Not forwarded.
    fn deeper(&self) -> Option<crate::egress::Deeper> {
        None
    }
}

/// The skills work Endora started for itself may use: those that read the house, and nothing
/// else (ADR 0094). A list of what is allowed rather than of what is not, so a skill added
/// later is out of the lane until someone puts it here on purpose.
///
/// Today that is Home Assistant, which only reads (`Observe`). The reason for the narrowness is
/// what a live run did when it had more: with the web and Wikipedia to hand it was asked about
/// one silent sensor, named a radio protocol the device was never said to use, and wrote
/// generic repair advice from an encyclopedia as though it had looked at the device. It also
/// carried the name of something in the person's home out to a search engine, when nobody had
/// asked for anything. Servers the person has connected are out too, because nothing here
/// knows which of them leave the house.
pub const HOUSE_SKILLS: &[&str] = &["home_assistant"];

/// A runner that offers and serves only [`HOUSE_SKILLS`].
pub struct HouseOnlyRunner<'a>(pub &'a dyn CapabilityRunner);

const NOT_THE_HOUSE: &str = "this kind of work only reads the house";

impl CapabilityRunner for HouseOnlyRunner<'_> {
    fn available(&self) -> Vec<CapabilitySpec> {
        self.0
            .available()
            .into_iter()
            .filter(|spec| HOUSE_SKILLS.contains(&spec.id.as_str()))
            .collect()
    }
    fn run(&self, id: &str, input: &str) -> Result<String, String> {
        self.run_observed(id, input).map(|read| read.text)
    }
    fn run_observed(&self, id: &str, input: &str) -> Result<CapabilityRead, String> {
        if !HOUSE_SKILLS.contains(&id) {
            return Err(NOT_THE_HOUSE.to_owned());
        }
        self.0.run_observed(id, input)
    }
    fn about_the_person(&self) -> Vec<String> {
        self.0.about_the_person()
    }
    fn current_states(&self) -> Vec<(String, String)> {
        self.0.current_states()
    }
    fn decision(&self, id: &str) -> Option<Decision> {
        if !HOUSE_SKILLS.contains(&id) {
            return Some(Decision::Block);
        }
        self.0.decision(id)
    }
    fn verifier(&self, id: &str) -> Option<String> {
        self.0.verifier(id)
    }
    fn read_back_input(&self, id: &str, input: &str) -> String {
        self.0.read_back_input(id, input)
    }
}

/// `context` offering only [`HOUSE_SKILLS`], so the model is not shown what it will be refused.
#[must_use]
pub fn only_the_house(context: &ButlerContext) -> ButlerContext {
    let mut kept = context.clone();
    kept.tools
        .retain(|tool| HOUSE_SKILLS.contains(&tool.id.as_str()));
    kept.deferred
        .retain(|tool| HOUSE_SKILLS.contains(&tool.id.as_str()));
    kept.capabilities
        .retain(|text| HOUSE_SKILLS.contains(&text.split_whitespace().next().unwrap_or("")));
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_unix_millis(ms)
    }

    const MINUTE: i64 = 60 * 1000;

    #[test]
    fn nobody_having_spoken_is_quiet() {
        assert!(quiet_enough(at(1_000_000), None));
    }

    #[test]
    fn someone_who_has_just_spoken_is_not() {
        let now = 100 * MINUTE;
        assert!(!quiet_enough(at(now), Some(at(now))));
        assert!(!quiet_enough(at(now), Some(at(now - MINUTE))));
        assert!(
            !quiet_enough(at(now), Some(at(now - QUIET_WINDOW_MS + 1))),
            "one millisecond short of the window is still inside it"
        );
    }

    #[test]
    fn the_window_ends_exactly_at_its_length() {
        let now = 100 * MINUTE;
        assert!(quiet_enough(at(now), Some(at(now - QUIET_WINDOW_MS))));
        assert!(quiet_enough(at(now), Some(at(now - 3 * QUIET_WINDOW_MS))));
    }

    /// Offers one tool per id and records nothing: enough to see what is shown and refused.
    struct Offers(Vec<&'static str>);
    impl CapabilityRunner for Offers {
        fn available(&self) -> Vec<CapabilitySpec> {
            self.0
                .iter()
                .map(|id| CapabilitySpec {
                    id: (*id).to_owned(),
                    wants_place: false,
                    third_party: false,
                    description: String::new(),
                    configured: true,
                    autonomous: true,
                    input_schema: None,
                    reversibility: endora_kernel::Reversibility::Observe,
                })
                .collect()
        }
        fn run(&self, id: &str, _input: &str) -> Result<String, String> {
            Ok(format!("{id} ran"))
        }
    }

    #[test]
    fn only_the_house_is_shown_and_served_to_work_endora_started() {
        let all = Offers(vec![
            "web_search",
            "web_fetch",
            "knowledge",
            "weather",
            "mail",
            "home_assistant",
        ]);
        let house = HouseOnlyRunner(&all);
        let shown: Vec<String> = house.available().into_iter().map(|s| s.id).collect();
        assert_eq!(shown, ["home_assistant"]);
        assert_eq!(
            house.run("home_assistant", "{}").unwrap(),
            "home_assistant ran"
        );
        for id in ["web_search", "web_fetch", "knowledge", "weather", "mail"] {
            assert!(house.run(id, "{}").is_err(), "{id} was served");
            assert!(house.run_observed(id, "{}").is_err(), "{id} was served");
            assert_eq!(house.decision(id), Some(Decision::Block), "{id}");
        }
    }

    #[test]
    fn a_skill_nobody_has_listed_is_out_of_the_lane() {
        // The point of an allow-list: what is added later is excluded until it is chosen.
        let all = Offers(vec!["a_brand_new_skill", "brave.brave_web_search"]);
        assert!(HouseOnlyRunner(&all).available().is_empty());
        assert!(
            HouseOnlyRunner(&all)
                .run("a_brand_new_skill", "{}")
                .is_err()
        );
        assert!(
            HouseOnlyRunner(&all)
                .run("brave.brave_web_search", "{}")
                .is_err()
        );
    }

    #[test]
    fn the_model_is_not_offered_what_it_will_be_refused() {
        let tool = |id: &str| crate::CapabilityTool {
            id: id.to_owned(),
            description: String::new(),
            input_schema: None,
        };
        let context = ButlerContext {
            tools: vec![tool("web_search"), tool("home_assistant")],
            deferred: vec![tool("knowledge"), tool("time")],
            capabilities: vec![
                "web_search — look things up".to_owned(),
                "home_assistant — the house".to_owned(),
            ],
            ..ButlerContext::default()
        };
        let house = only_the_house(&context);
        let ids = |tools: &[crate::CapabilityTool]| -> Vec<String> {
            tools.iter().map(|t| t.id.clone()).collect()
        };
        assert_eq!(ids(&house.tools), ["home_assistant"]);
        assert!(house.deferred.is_empty());
        assert_eq!(house.capabilities, ["home_assistant — the house"]);
        assert_eq!(context.tools.len(), 2, "the original is untouched");
    }

    #[test]
    fn a_message_from_the_future_is_not_quiet() {
        // Fail closed: a clock that moved, or a wrong record, must not read as silence.
        let now = 100 * MINUTE;
        assert!(!quiet_enough(at(now), Some(at(now + MINUTE))));
        assert!(!quiet_enough(at(0), Some(at(i64::MAX))));
    }
}
