//! Ask deep, with skills (ADR 0086).
//!
//! The manual "Ask deep" used to send the bigger model one bare question, with no skills, so
//! it could only answer from what it already knew. This gives it the skills — without giving
//! up the rule that a stranger's words never leave the device (ADR 0064) or the protection
//! against being commanded by them.
//!
//! The two jobs are split between two models, and the split is the defence:
//!
//! - **The deep model plans.** It sees the person's question and the list of skills, and
//!   chooses which to call. After each call it is told *that* the skill ran and roughly how
//!   much came back ("returned almost nothing", "returned results") — in a sentence Endora
//!   wrote — and never *what* came back. A page that says "ignore previous instructions"
//!   therefore never reaches a model that could act on it, and nothing a stranger wrote
//!   leaves the device.
//! - **The local model reads.** It sees everything the skills returned, because it never
//!   leaves the device, and it is given **no skills** while it does, so an instruction in a
//!   page has nothing to call. At worst it can be talked into saying something wrong, which
//!   the person can see and correct.
//!
//! Everything in between is the ordinary turn loop: the same clearance gate runs each call,
//! and a turn that has read a stranger's words still cannot act on its own. This module
//! only decides which model answers each round.

use std::cell::{Cell, RefCell};

use crate::egress::{Deeper, DoorRefusal, STRANGER_MARK, TheirOwnQuestion};
use crate::ports::{Butler, ButlerContext, ButlerReply, TurnMessage};
use crate::{Preference, ProposalError};
use endora_conversation::ChatMessage;

/// How many times the deep model is asked to plan in one turn. Each is a call to somebody
/// else's service, so it is bounded however many rounds the loop would otherwise allow.
const MOST_PLANS: usize = 3;

/// A third-party result shorter than this has, in practice, found nothing: the sentence a
/// skill writes for "no results" is a few dozen characters, a real result is far more.
const ALMOST_NOTHING: usize = 80;

/// Who answers the rounds that are left.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The deep model is still choosing skills.
    Planning,
    /// Planning is over; the local model reads what came back, with no skills.
    Reading,
    /// The deep model could not be asked, so this is an ordinary local turn.
    Local,
}

/// What the person is told about where their words went (ADR 0055: disclosed every time).
enum Disclosure {
    /// The deep model chose skills; what they returned was read by the everyday model.
    ChoseSkills,
    /// The deep model answered without using any skill.
    AnsweredItself,
    /// It could not be asked, for this reason.
    Declined(&'static str),
}

impl Disclosure {
    /// The line, naming the model that read the results and where (ADR 0105): the everyday
    /// list may have a cloud model first, which reads them disguised.
    fn line(&self, read_by: Option<&crate::AnsweredBy>) -> String {
        match self {
            Self::ChoseSkills => {
                let reader = match read_by {
                    Some(r) if r.on_the_network => format!("{} on your network", r.model),
                    Some(r) => format!("{}, disguised", r.model),
                    None => "your everyday model".to_owned(),
                };
                format!(
                    "[deep] The deep model chose which skills to use. What they returned was \
                     not sent to it: it was read by {reader}."
                )
            }
            Self::AnsweredItself => "[deep] Answered by the deep model without using any skill \
                                     — this question left the box."
                .to_owned(),
            Self::Declined(why) => {
                format!("[deep] Couldn't ask the deep model — {why}. This was answered locally.")
            }
        }
    }
}

/// What one planning round came to.
enum Planned {
    /// Run these skills.
    Calls(Vec<crate::ToolCall>),
    /// It answered in words, using no skill.
    Answer(String),
    /// It has what it needs; the local model reads and answers.
    Enough,
    /// It could not be asked.
    Declined(&'static str),
}

/// A [`Butler`] whose skill choices come from the deep model and whose reading is done
/// locally. Plug it into the ordinary turn in place of the local butler.
pub struct PlannedByTheDeepModel<'a> {
    deeper: &'a Deeper,
    reader: &'a dyn Butler,
    plans: Cell<usize>,
    mode: Cell<Mode>,
    disclosure: RefCell<Option<Disclosure>>,
}

impl<'a> PlannedByTheDeepModel<'a> {
    /// `deeper` is the deep model behind its door; `reader` is the local model.
    #[must_use]
    pub fn new(deeper: &'a Deeper, reader: &'a dyn Butler) -> Self {
        Self {
            deeper,
            reader,
            plans: Cell::new(0),
            mode: Cell::new(Mode::Planning),
            disclosure: RefCell::new(None),
        }
    }

    /// Records where the person's words went. The first thing that happens wins: a turn that
    /// was planned by the deep model is not later described as a local one.
    fn disclose(&self, what: Disclosure) {
        let mut held = self.disclosure.borrow_mut();
        if held.is_none() {
            *held = Some(what);
        }
    }

    /// The reply with the disclosure appended, when it is words for the person (a reply that
    /// is only skill calls says nothing yet).
    fn told(&self, mut reply: ButlerReply) -> ButlerReply {
        if !reply.tool_calls.is_empty() {
            return reply;
        }
        if let Some(what) = self.disclosure.borrow().as_ref() {
            let line = what.line(reply.answered_by.as_ref());
            reply.text = format!("{}\n\n{}", reply.text.trim_end(), line);
        }
        reply
    }

    fn plan(
        &self,
        conversation: &[TurnMessage],
        context: &ButlerContext,
        results: bool,
    ) -> Planned {
        let Some(view) = trusted_view(conversation) else {
            return Planned::Declined("the question looks like it contains a secret");
        };
        // What the deep model is shown: the skills on offer and the date. Nothing about the
        // person — not what Endora understands about them, what it did lately, or where they
        // are. The skills fill the place in themselves when they run, here, on this device.
        let planning_context = ButlerContext {
            tools: context.tools.clone(),
            capabilities: context.capabilities.clone(),
            now: context.now.clone(),
            ..ButlerContext::default()
        };
        match self.deeper.continue_turn(&view, &[], &planning_context) {
            Ok(reply) if !reply.tool_calls.is_empty() => Planned::Calls(reply.tool_calls),
            Ok(reply) if !reply.text.trim().is_empty() => Planned::Answer(reply.text),
            Ok(_) if results => Planned::Enough,
            Ok(_) => Planned::Declined("it gave no answer"),
            Err(DoorRefusal::AStrangerSpoke) => {
                Planned::Declined("this turn had already read something written by someone else")
            }
            Err(DoorRefusal::LooksLikeASecret) => {
                Planned::Declined("the question looks like it contains a secret")
            }
            Err(DoorRefusal::TheModelFailed(_)) => Planned::Declined("it did not answer"),
            Err(DoorRefusal::Restricted) => {
                Planned::Declined("this conversation holds something private that stays home")
            }
        }
    }

    /// The local model reads what came back, with **no skills to call**, and anything it
    /// tries to call anyway is dropped: a page's instruction has nothing to reach.
    fn read(
        &self,
        conversation: &[TurnMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        let reading = ButlerContext {
            tools: Vec::new(),
            deferred: Vec::new(),
            ..context.clone()
        };
        let mut reply = self.reader.take_turn(conversation, preferences, &reading)?;
        reply.tool_calls.clear();
        Ok(self.told(reply))
    }
}

impl Butler for PlannedByTheDeepModel<'_> {
    fn respond(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        self.reader.respond(history, preferences, context)
    }

    fn take_turn(
        &self,
        conversation: &[TurnMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        let results = this_turn(conversation)
            .iter()
            .any(|m| matches!(m, TurnMessage::ToolResult { .. }));
        // The loop's last round takes the skills away so the model must answer in words; with
        // results in hand there is nothing left to plan.
        if results && context.tools.is_empty() && self.mode.get() == Mode::Planning {
            self.mode.set(Mode::Reading);
        }
        if self.mode.get() == Mode::Planning && self.plans.get() < MOST_PLANS {
            self.plans.set(self.plans.get() + 1);
            match self.plan(conversation, context, results) {
                Planned::Calls(calls) => {
                    self.disclose(Disclosure::ChoseSkills);
                    return Ok(ButlerReply {
                        tool_calls: calls,
                        ..ButlerReply::default()
                    });
                }
                Planned::Answer(text) if !results => {
                    self.disclose(Disclosure::AnsweredItself);
                    return Ok(self.told(ButlerReply {
                        text,
                        ..ButlerReply::default()
                    }));
                }
                // Words from a model that has not seen the results are a guess; the local
                // model, which has, answers instead.
                Planned::Answer(_) | Planned::Enough => self.mode.set(Mode::Reading),
                Planned::Declined(why) => {
                    self.disclose(Disclosure::Declined(why));
                    self.mode.set(Mode::Local);
                }
            }
        }
        match self.mode.get() {
            Mode::Local => {
                Ok(self.told(self.reader.take_turn(conversation, preferences, context)?))
            }
            Mode::Planning | Mode::Reading => {
                // Planning ran out of budget, or is over: read what there is.
                self.mode.set(Mode::Reading);
                if results {
                    self.disclose(Disclosure::ChoseSkills);
                }
                self.read(conversation, preferences, context)
            }
        }
    }

    fn summarize(&self, prior_summary: &str, transcript: &str) -> Result<String, ProposalError> {
        self.reader.summarize(prior_summary, transcript)
    }
}

/// The part of the conversation that is this turn: from the person's latest message on.
fn this_turn(conversation: &[TurnMessage]) -> &[TurnMessage] {
    let start = conversation
        .iter()
        .rposition(|m| matches!(m, TurnMessage::User(_)))
        .unwrap_or(0);
    &conversation[start..]
}

/// What the deep model may be shown of this turn, or `None` when the question may not leave.
///
/// The person's latest message, checked and redacted exactly as a bare "Ask deep" question
/// always was; the skills it chose; and, for each, a sentence Endora wrote about *whether and
/// how much* it returned. Earlier chat is not included, and **no result is ever included**:
/// not the text, not the mark, not an error message that might echo a stranger.
fn trusted_view(conversation: &[TurnMessage]) -> Option<Vec<TurnMessage>> {
    let turn = this_turn(conversation);
    let mut called: Vec<(String, String)> = Vec::new();
    let mut view = Vec::with_capacity(turn.len());
    for message in turn {
        view.push(match message {
            TurnMessage::User(text) => {
                TurnMessage::User(TheirOwnQuestion::checked(text).ok()?.as_str().to_owned())
            }
            TurnMessage::Assistant { text, tool_calls } => {
                called.extend(
                    tool_calls
                        .iter()
                        .map(|c| (c.id.clone(), c.capability.clone())),
                );
                TurnMessage::Assistant {
                    text: text.clone(),
                    tool_calls: tool_calls.clone(),
                }
            }
            TurnMessage::ToolResult { call_id, content } => {
                let skill = called
                    .iter()
                    .find(|(id, _)| id == call_id)
                    .map_or("a skill", |(_, skill)| skill.as_str());
                TurnMessage::ToolResult {
                    call_id: call_id.clone(),
                    content: outcome_of(skill, content),
                }
            }
        });
    }
    Some(view)
}

/// A sentence, written here, saying whether a skill ran and how much came back. It carries
/// the skill's name and a count, and nothing the skill returned.
///
/// How the loop words its own refusals is known (`error:`, `'<skill>' …`, `no such skill`),
/// so a call that did not run is recognised by those and not by anything a skill said.
fn outcome_of(skill: &str, content: &str) -> String {
    let did_not_run = content.starts_with("error:")
        || content.starts_with("no such skill")
        || content.starts_with(&format!("'{skill}' "));
    let what = if did_not_run {
        format!("{skill} did not run: it failed or was refused.")
    } else if let Some(outside) = content.strip_prefix(STRANGER_MARK) {
        let characters = outside.trim().chars().count();
        if characters < ALMOST_NOTHING {
            format!("{skill} ran and returned almost nothing ({characters} characters).")
        } else {
            format!("{skill} ran and returned results ({characters} characters).")
        }
    } else {
        format!(
            "{skill} ran and returned a reading ({} characters).",
            content.chars().count()
        )
    };
    format!(
        "{what} The results are read separately. If a different source would help, call it; \
         otherwise reply \"done\"."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolCall;

    fn call(id: &str, skill: &str) -> TurnMessage {
        TurnMessage::Assistant {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: id.to_owned(),
                capability: skill.to_owned(),
                input_json: "{}".to_owned(),
            }],
        }
    }

    fn result(id: &str, content: &str) -> TurnMessage {
        TurnMessage::ToolResult {
            call_id: id.to_owned(),
            content: content.to_owned(),
        }
    }

    #[test]
    fn what_a_skill_returned_is_never_in_what_the_deep_model_is_shown() {
        let conversation = vec![
            TurnMessage::User("what is on this month?".to_owned()),
            call("c1", "web_search"),
            result("c1", &format!("{STRANGER_MARK}SECRET-WORDS from a page")),
            call("c2", "mail"),
            result("c2", "error: the server said SECRET-ERROR"),
        ];

        let shown = format!("{:?}", trusted_view(&conversation).unwrap());

        assert!(!shown.contains("SECRET-WORDS"), "{shown}");
        assert!(!shown.contains("SECRET-ERROR"), "{shown}");
        assert!(!shown.contains(STRANGER_MARK), "{shown}");
        assert!(
            shown.contains("web_search ran and returned almost nothing"),
            "{shown}"
        );
        assert!(shown.contains("mail did not run"), "{shown}");
    }

    /// Reads with no skills, and says which model it is.
    struct Reads(Option<crate::AnsweredBy>);
    impl Butler for Reads {
        fn respond(
            &self,
            _h: &[ChatMessage],
            _p: &[Preference],
            _c: &ButlerContext,
        ) -> Result<ButlerReply, ProposalError> {
            Ok(ButlerReply {
                text: "Two concerts.".to_owned(),
                answered_by: self.0.clone(),
                ..ButlerReply::default()
            })
        }
    }

    #[test]
    fn the_note_says_which_model_read_the_results_and_where() {
        struct Plans;
        impl Butler for Plans {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
        }
        let deeper = Deeper::new(std::sync::Arc::new(Plans));
        let note = |reader: &Reads| {
            let planned = PlannedByTheDeepModel::new(&deeper, reader);
            planned.disclose(Disclosure::ChoseSkills);
            planned
                .read(
                    &[TurnMessage::User("what is on?".to_owned())],
                    &[],
                    &ButlerContext::default(),
                )
                .unwrap()
                .text
        };
        let at_home = note(&Reads(Some(crate::AnsweredBy {
            model: "qwen2.5:7b".to_owned(),
            on_the_network: true,
        })));
        assert!(
            at_home.contains("read by qwen2.5:7b on your network"),
            "{at_home}"
        );
        let outside = note(&Reads(Some(crate::AnsweredBy {
            model: "claude-sonnet-5-5".to_owned(),
            on_the_network: false,
        })));
        assert!(
            outside.contains("read by claude-sonnet-5-5, disguised"),
            "{outside}"
        );
        assert!(!outside.contains("on this device"), "{outside}");
    }

    #[test]
    fn earlier_chat_is_not_shown_only_this_turn() {
        let conversation = vec![
            TurnMessage::User("an earlier question".to_owned()),
            TurnMessage::Assistant {
                text: "an earlier answer taken from a web page".to_owned(),
                tool_calls: Vec::new(),
            },
            TurnMessage::User("the question now".to_owned()),
        ];

        let shown = format!("{:?}", trusted_view(&conversation).unwrap());

        assert!(shown.contains("the question now"), "{shown}");
        assert!(!shown.contains("earlier"), "{shown}");
    }

    #[test]
    fn the_outcome_says_how_much_came_back_without_saying_what() {
        let long = "x".repeat(400);
        assert!(
            outcome_of("web_search", &format!("{STRANGER_MARK}{long}"))
                .starts_with("web_search ran and returned results (400 characters).")
        );
        assert!(
            outcome_of(
                "ticketed_events",
                &format!("{STRANGER_MARK}Nothing ticketed.")
            )
            .starts_with("ticketed_events ran and returned almost nothing")
        );
        for refused in [
            "error: boom",
            "no such skill 'x' — you can't do that.",
            "'web_search' needs their go-ahead — ask; don't claim you did it.",
            "'web_search' isn't allowed yet — you can't run it even if asked.",
            "'web_search' is not set up — you can't use it; tell them plainly.",
        ] {
            assert!(
                outcome_of("web_search", refused).starts_with("web_search did not run"),
                "{refused}"
            );
        }
        assert!(outcome_of("weather", "14 and clear").contains("returned a reading"));
    }

    #[test]
    fn a_question_that_looks_like_a_secret_cannot_be_shown_at_all() {
        let conversation = vec![TurnMessage::User(
            "ok? -----BEGIN PRIVATE KEY----- x".to_owned(),
        )];
        assert!(trusted_view(&conversation).is_none());
    }
}
