//! The Mac's brain (ADR 0130): when a turn of a member who thinks with the owner's Mac may
//! start. The node knows whether the Mac is free (plugged in, not switched to busy, its model
//! up, the owner idle) and tells HQ; HQ never guesses. A turn that may not start waits where it
//! is, queued, and says why: it is never given another brain.

use super::work::Millis;

/// How long what the node said about the Mac is trusted: two minutes and a half. The node says
/// it every thirty seconds and whenever it changes, so a node that has gone quiet, or a Mac that
/// stopped reporting, counts as the Mac being away rather than as it being free.
pub const TRUSTED_FOR_MS: Millis = 150_000;

/// What a waiting turn says when the node has not said anything since HQ started.
pub const NOT_SAID_YET: &str = "the node has not said yet";

/// What a waiting turn says when the node's last word is older than [`TRUSTED_FOR_MS`].
pub const NOT_SAID_LATELY: &str = "the node has not said lately";

/// What a waiting turn says when another turn is using the Mac: the Mac takes one at a time.
pub const ANOTHER_TURN: &str = "another turn is using the Mac";

/// What the node last said about the Mac, and when HQ heard it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacFacts {
    /// Whether the Mac is free for a turn now.
    pub ready: bool,
    /// Why, in the node's few plain words: `ready`, `owner active`, `on battery`, `busy`,
    /// `model loading` or `not heard from`.
    pub why: String,
    /// When HQ heard it.
    pub told_at: Millis,
}

/// Why a turn of a member on the Mac must wait now, in plain words for the owner; `None` when
/// it may start. `facts` is what the node last said, if it has said anything since HQ started;
/// `another_turn_runs` is whether a turn already runs on the Mac.
#[must_use]
pub fn wait_for(facts: Option<&MacFacts>, now: Millis, another_turn_runs: bool) -> Option<String> {
    let Some(facts) = facts else {
        return Some(NOT_SAID_YET.to_owned());
    };
    if now - facts.told_at > TRUSTED_FOR_MS {
        return Some(NOT_SAID_LATELY.to_owned());
    }
    if !facts.ready {
        return Some(facts.why.clone());
    }
    if another_turn_runs {
        return Some(ANOTHER_TURN.to_owned());
    }
    None
}

/// A turn's `why` as it is shown while the turn waits for the Mac.
#[must_use]
pub fn waiting_words(why: &str) -> String {
    format!("waiting for the Mac: {why}")
}

/// How long work may wait for the Mac before the owner's digest says so (the Phase 0 audit's
/// P5): longer than a working day's "owner active", shorter than a night lost to a Mac that is
/// asleep or out of reach. The work still waits; only the owner is told.
pub const LONG_WAIT_MS: Millis = 3 * 60 * 60 * 1000;

/// The setting that marks a long wait as said, so it is said once while it lasts.
pub const LONG_WAIT_SAID: &str = "mac_long_wait_said";

/// The digest's note for work that has waited long for the Mac: who, on what, how long, why,
/// and what the owner can do.
#[must_use]
pub fn long_wait_said(waiting: &[String], hours: i64, why: &str) -> String {
    format!(
        "Work has waited for the Mac for over {hours} h ({}): {why}. It still waits: free the \
         Mac, or change who works on it from the roster.",
        waiting.join(", ")
    )
}

/// Whether `said` is a turn's waiting words (see [`waiting_words`]): the turn's reason, which
/// the engine clears when the turn starts.
#[must_use]
pub fn is_waiting_words(said: &str) -> bool {
    said.starts_with("waiting for the Mac: ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn told(ready: bool, why: &str, at: Millis) -> MacFacts {
        MacFacts {
            ready,
            why: why.to_owned(),
            told_at: at,
        }
    }

    #[test]
    fn a_turn_waits_until_the_node_has_said_the_mac_is_free() {
        assert_eq!(
            wait_for(None, 1_000, false).as_deref(),
            Some(NOT_SAID_YET),
            "a new HQ does not guess"
        );
        let busy = told(false, "owner active", 1_000);
        assert_eq!(
            wait_for(Some(&busy), 2_000, false).as_deref(),
            Some("owner active")
        );
        let free = told(true, "ready", 1_000);
        assert_eq!(wait_for(Some(&free), 2_000, false), None);
    }

    #[test]
    fn the_mac_takes_one_turn_at_a_time() {
        let free = told(true, "ready", 1_000);
        assert_eq!(
            wait_for(Some(&free), 2_000, true).as_deref(),
            Some(ANOTHER_TURN)
        );
    }

    #[test]
    fn a_node_gone_quiet_is_the_mac_being_away() {
        let free = told(true, "ready", 1_000);
        assert_eq!(wait_for(Some(&free), 1_000 + TRUSTED_FOR_MS, false), None);
        assert_eq!(
            wait_for(Some(&free), 1_001 + TRUSTED_FOR_MS, false).as_deref(),
            Some(NOT_SAID_LATELY)
        );
    }

    #[test]
    fn waiting_words_are_recognised_so_they_can_be_cleared() {
        let said = waiting_words("on battery");
        assert_eq!(said, "waiting for the Mac: on battery");
        assert!(is_waiting_words(&said));
        assert!(!is_waiting_words(
            "backing off: its last turns moved nothing"
        ));
    }
}
