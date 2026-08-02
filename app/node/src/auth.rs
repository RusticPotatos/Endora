//! Who is allowed to talk to this node.
//!
//! Endora shipped with **no inbound authentication at all**. Every `/v1` route answered
//! anyone who could reach the port — and the node binds `0.0.0.0`, so that is every interface
//! the machine has. A person on the same network could read the whole conversation history
//! from `/v1/export`, drive the house, and — worst of the set — `POST /v1/deep-model` with an
//! endpoint of their own and have every later turn's contents sent there.
//!
//! That was found by asking what would happen to a mailbox credential once it lived here. The
//! credential was never the weak part.
//!
//! The rule below is the whole of the decision, kept pure so every way in and out of it is a
//! test rather than an argument.

/// The one route a browser cannot send a header on.
///
/// `EventSource` has no way to set `Authorization`, so the live activity feed takes its token
/// in the query string instead. **Only this path**, because a query string ends up in access
/// logs and browser history in a way a header does not — a narrow exception with a reason,
/// rather than a second way in.
const THE_ROUTE_A_BROWSER_CANNOT_SIGN: &str = "/v1/activity/stream";

/// The one `/v1` route that must answer an unsigned request: signing in.
///
/// Chicken and egg — a credential is what it exists to hand out. It is not therefore
/// unprotected: it is throttled and locked by [`crate::signin`], which is the whole reason a
/// password and a six-digit code are safe to accept at all.
const WHERE_YOU_ASK_TO_BE_LET_IN: &str = "/v1/session";

/// Whether a request may proceed.
///
/// - Anything outside `/v1` is open: the console has to be able to load in order to ask for a
///   token, and a health check must answer before anyone has one.
/// - **No configured token refuses everything.** Fail closed, always. An appliance that
///   treats "unset" as "allow" is one bad migration away from being wide open, and would fail
///   in exactly the direction nobody notices.
#[must_use]
pub fn may_pass(
    path: &str,
    header: Option<&str>,
    query_token: Option<&str>,
    accepted: &[String],
) -> bool {
    if !path.starts_with("/v1") || path == WHERE_YOU_ASK_TO_BE_LET_IN {
        return true;
    }
    if accepted.is_empty() {
        return false;
    }
    if let Some(offered) = header.and_then(|h| h.strip_prefix("Bearer ")) {
        return any_of(accepted, offered.trim());
    }
    if path == THE_ROUTE_A_BROWSER_CANNOT_SIGN {
        if let Some(offered) = query_token {
            return any_of(accepted, offered);
        }
    }
    false
}

/// Whether an offered secret is any of the accepted ones.
///
/// **Every candidate is checked even after one matches**, for the same reason
/// [`same_secret`] does not stop at the first differing byte: returning early would make how
/// long a rejection took a measurement of how far down the list the near-miss was.
fn any_of(accepted: &[String], offered: &str) -> bool {
    accepted.iter().fold(false, |found, candidate| {
        found | same_secret(offered, candidate)
    })
}

/// Compares two secrets without giving away where they first differ.
///
/// `==` on a string returns as soon as a byte disagrees, so how long it took is a measurement
/// of how much of the token was right. That is only worth a great deal to an attacker who can
/// time many attempts, which is not this threat model — but the correct comparison costs four
/// lines and no dependency, and choosing the wrong one on purpose is how a system ends up
/// explaining itself later.
fn same_secret(offered: &str, expected: &str) -> bool {
    if offered.len() != expected.len() {
        return false;
    }
    offered
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |differences, (a, b)| differences | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::{may_pass, same_secret};

    fn good() -> Vec<String> {
        vec!["a-real-token".to_owned()]
    }
    fn nothing() -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn the_console_can_load_before_anyone_has_a_token() {
        // It has to: the screen that asks for the token is served by this node. A health check
        // must answer too, or the container is unhealthy the moment auth is turned on.
        for path in ["/", "/app.js", "/styles.css", "/health"] {
            assert!(may_pass(path, None, None, &good()), "{path} must stay open");
            assert!(
                may_pass(path, None, None, &nothing()),
                "{path} must load even with nothing configured"
            );
        }
    }

    #[test]
    fn nothing_configured_refuses_every_api_route() {
        // Fail closed, always. An appliance that reads "unset" as "allow" is one bad migration
        // away from wide open, and fails in the direction nobody notices.
        for path in ["/v1/export", "/v1/chat", "/v1/deep-model"] {
            assert!(!may_pass(path, Some("Bearer anything"), None, &nothing()));
            assert!(!may_pass(path, None, None, &nothing()));
        }
    }

    #[test]
    fn the_right_token_gets_in() {
        assert!(may_pass(
            "/v1/export",
            Some("Bearer a-real-token"),
            None,
            &good()
        ));
    }

    #[test]
    fn everything_else_does_not() {
        for header in [
            None,
            Some("Bearer wrong"),
            Some("Bearer "),
            Some("bearer a-real-token"), // the scheme is case-sensitive here on purpose
            Some("a-real-token"),        // the raw token, without the scheme
            Some("Basic a-real-token"),
            Some("Bearer a-real-token-with-more"),
        ] {
            assert!(
                !may_pass("/v1/export", header, None, &good()),
                "{header:?} should not have been let in"
            );
        }
    }

    #[test]
    fn surrounding_whitespace_does_not_change_a_token() {
        assert!(may_pass(
            "/v1/export",
            Some("Bearer  a-real-token  "),
            None,
            &good()
        ));
    }

    #[test]
    fn the_live_feed_may_be_signed_in_the_query_because_a_browser_cannot_sign_it() {
        assert!(may_pass(
            "/v1/activity/stream",
            None,
            Some("a-real-token"),
            &good()
        ));
        assert!(!may_pass(
            "/v1/activity/stream",
            None,
            Some("wrong"),
            &good()
        ));
    }

    #[test]
    fn no_other_route_accepts_a_token_in_the_query() {
        // A query string reaches access logs and browser history in a way a header does not.
        // One route needs the exception; giving it to everything would be a second way in.
        for path in ["/v1/export", "/v1/chat", "/v1/memory/purge"] {
            assert!(
                !may_pass(path, None, Some("a-real-token"), &good()),
                "{path} must not take a token from the query"
            );
        }
    }

    #[test]
    fn the_query_never_rescues_a_wrong_header() {
        // A header that was offered and did not match is a refusal, not a reason to go looking
        // for another credential on the same request.
        assert!(!may_pass(
            "/v1/activity/stream",
            Some("Bearer wrong"),
            Some("a-real-token"),
            &good()
        ));
    }

    #[test]
    fn comparing_secrets_does_not_stop_at_the_first_difference() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abd", "abc"));
        assert!(!same_secret("xbc", "abc"));
        assert!(!same_secret("ab", "abc"));
        assert!(!same_secret("abcd", "abc"));
        assert!(same_secret("", ""));
    }
}
