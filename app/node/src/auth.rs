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

/// What a credential is allowed to do.
///
/// One axis, deliberately. A general scope system for a single-person appliance would be more
/// machinery than the thing it protects; this exists because a *specific* credential has to
/// live in a plaintext file on a laptop, and that one should not be able to do the things that
/// matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The console and the person: everything.
    Full,
    /// The smoke suite: **reads only, and never the bulk export.**
    ///
    /// It cannot write, so it cannot point the deep model at somebody else's endpoint, change
    /// what a capability may do, or purge memory. It cannot pull the whole conversation in one
    /// call either.
    ///
    /// It *can* still read beliefs, context and the rest — and that is not an oversight. Three
    /// of the suite's invariants assert on real belief statements, and "asserts about real
    /// data" is the entire reason that tier exists. A credential blind to those would leave
    /// the tests unable to check anything worth checking.
    Checks,
}

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

/// The one read that hands over everything at once.
///
/// Every other read is a slice — this is the whole conversation, every belief and the audit
/// trail in a single call, which is what makes it worth naming separately from "reads".
const THE_ONE_THAT_HANDS_OVER_EVERYTHING: &str = "/v1/export";

/// Whether this scope permits this request.
///
/// Split from [`may_pass`] because they answer different questions — *is this credential real*
/// and *is this credential allowed* — and conflating them is how a scope quietly stops being
/// enforced on a route somebody added later.
#[must_use]
pub fn scope_permits(scope: Scope, path: &str, is_write: bool) -> bool {
    match scope {
        Scope::Full => true,
        Scope::Checks => !is_write && path != THE_ONE_THAT_HANDS_OVER_EVERYTHING,
    }
}

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
    accepted: &[(String, Scope)],
    is_write: bool,
) -> bool {
    if !path.starts_with("/v1") || path == WHERE_YOU_ASK_TO_BE_LET_IN {
        return true;
    }
    if accepted.is_empty() {
        return false;
    }
    let offered = header
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .or_else(|| {
            // Only the live feed, and only because `EventSource` cannot set a header.
            (path == THE_ROUTE_A_BROWSER_CANNOT_SIGN)
                .then_some(query_token)
                .flatten()
        });
    let Some(offered) = offered else {
        return false;
    };
    // Being a real credential and being an allowed one are separate questions, answered
    // separately — conflating them is how a scope quietly stops applying to a route somebody
    // added later.
    matches_any(accepted, offered).is_some_and(|scope| scope_permits(scope, path, is_write))
}

/// Which scope an offered secret carries, if it is one of the accepted ones.
///
/// **Every candidate is checked even after one matches**, for the same reason
/// [`same_secret`] does not stop at the first differing byte: returning early would make how
/// long a rejection took a measurement of how far down the list the near-miss was.
fn matches_any(accepted: &[(String, Scope)], offered: &str) -> Option<Scope> {
    accepted.iter().fold(None, |found, (candidate, scope)| {
        if same_secret(offered, candidate) {
            Some(*scope)
        } else {
            found
        }
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
    use super::{Scope, may_pass, same_secret};

    fn good() -> Vec<(String, Scope)> {
        vec![("a-real-token".to_owned(), Scope::Full)]
    }
    fn nothing() -> Vec<(String, Scope)> {
        Vec::new()
    }

    #[test]
    fn asking_whether_sign_in_exists_needs_no_credential() {
        // The console cannot show the right first screen without knowing whether a password
        // has ever been set, and the endpoint that knew was itself behind the token — so a
        // brand-new node asked people to sign in to an account that did not exist.
        //
        // A single boolean is all that travels. An attacker learns "this Endora has a
        // password", which they would learn from one attempt anyway.
        assert!(may_pass("/v1/session", None, None, &good(), false));
        assert!(may_pass("/v1/session", None, None, &nothing(), false));
    }

    #[test]
    fn setting_the_password_is_still_behind_the_token() {
        // The neighbouring path, and the one that must never open: claiming the account is
        // exactly what a stranger on the network must not be able to do first.
        assert!(!may_pass("/v1/session/setup", None, None, &good(), false));
        assert!(!may_pass(
            "/v1/session/setup",
            None,
            None,
            &nothing(),
            false
        ));
    }

    #[test]
    fn the_console_can_load_before_anyone_has_a_token() {
        // It has to: the screen that asks for the token is served by this node. A health check
        // must answer too, or the container is unhealthy the moment auth is turned on.
        for path in ["/", "/app.js", "/styles.css", "/health"] {
            assert!(
                may_pass(path, None, None, &good(), false),
                "{path} must stay open"
            );
            assert!(
                may_pass(path, None, None, &nothing(), false),
                "{path} must load even with nothing configured"
            );
        }
    }

    #[test]
    fn nothing_configured_refuses_every_api_route() {
        // Fail closed, always. An appliance that reads "unset" as "allow" is one bad migration
        // away from wide open, and fails in the direction nobody notices.
        for path in ["/v1/export", "/v1/chat", "/v1/deep-model"] {
            assert!(!may_pass(
                path,
                Some("Bearer anything"),
                None,
                &nothing(),
                false
            ));
            assert!(!may_pass(path, None, None, &nothing(), false));
        }
    }

    #[test]
    fn the_right_token_gets_in() {
        assert!(may_pass(
            "/v1/export",
            Some("Bearer a-real-token"),
            None,
            &good(),
            false
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
                !may_pass("/v1/export", header, None, &good(), false),
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
            &good(),
            false
        ));
    }

    #[test]
    fn the_live_feed_may_be_signed_in_the_query_because_a_browser_cannot_sign_it() {
        assert!(may_pass(
            "/v1/activity/stream",
            None,
            Some("a-real-token"),
            &good(),
            false
        ));
        assert!(!may_pass(
            "/v1/activity/stream",
            None,
            Some("wrong"),
            &good(),
            false
        ));
    }

    #[test]
    fn no_other_route_accepts_a_token_in_the_query() {
        // A query string reaches access logs and browser history in a way a header does not.
        // One route needs the exception; giving it to everything would be a second way in.
        for path in ["/v1/export", "/v1/chat", "/v1/memory/purge"] {
            assert!(
                !may_pass(path, None, Some("a-real-token"), &good(), false),
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
            &good(),
            false
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

#[cfg(test)]
mod what_a_checking_credential_may_do {
    //! Scopes (`Scope::Checks`). This exists because one credential has to live in a plaintext
    //! file on a laptop so `make smoke` can run, and that one should not be able to do the
    //! things that matter.

    use super::{Scope, scope_permits};

    #[test]
    fn the_console_credential_is_unrestricted() {
        for (path, is_write) in [("/v1/export", false), ("/v1/deep-model", true)] {
            assert!(scope_permits(Scope::Full, path, is_write));
        }
    }

    #[test]
    fn a_checking_credential_cannot_write_anything() {
        // The whole point. A token in a file on a laptop must not be able to point the deep
        // model at somebody else's endpoint, widen what a capability may do, or purge memory —
        // which are the three things a leaked credential would actually be used for.
        for path in [
            "/v1/deep-model",
            "/v1/memory/purge",
            "/v1/capabilities/x/open",
            "/v1/chat",
            "/v1/session/setup",
        ] {
            assert!(
                !scope_permits(Scope::Checks, path, true),
                "{path} should not be writable"
            );
        }
    }

    #[test]
    fn a_checking_credential_cannot_pull_everything_at_once() {
        // Every other read is a slice; this one is the whole conversation, every belief and
        // the audit trail in a single call. The suite never touches it.
        assert!(!scope_permits(Scope::Checks, "/v1/export", false));
    }

    #[test]
    fn a_checking_credential_can_still_read_what_the_suite_asserts_on() {
        // Not an oversight. Three of the nine invariants assert on real belief statements, and
        // "asserts about real data" is why that tier exists at all — a credential blind to
        // them would leave the tests unable to check anything worth checking.
        for path in [
            "/v1/understanding",
            "/v1/context",
            "/v1/notions",
            "/v1/standing-trouble",
            "/v1/reliability",
            "/v1/chat",
            "/v1/audit",
        ] {
            assert!(
                scope_permits(Scope::Checks, path, false),
                "{path} should be readable"
            );
        }
    }
}
