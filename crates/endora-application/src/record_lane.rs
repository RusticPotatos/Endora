//! What may run in the lane for goals Endora started for itself (ADR 0094): which model it
//! may use, and when it may run at all. Both are decided here, by code, so that neither
//! depends on a caller remembering.
use crate::ports::{Butler, ButlerContext, ButlerReply, NeedKind, ProposalError, TurnMessage};
use crate::{ChatMessage, Preference, Timestamp};

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

    #[test]
    fn a_message_from_the_future_is_not_quiet() {
        // Fail closed: a clock that moved, or a wrong record, must not read as silence.
        let now = 100 * MINUTE;
        assert!(!quiet_enough(at(now), Some(at(now + MINUTE))));
        assert!(!quiet_enough(at(0), Some(at(i64::MAX))));
    }
}
