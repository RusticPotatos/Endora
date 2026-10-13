//! `trust_all` on connect (ADR 0054 — confirmed beats declared).
//!
//! Live: voice broadcast was blocked at the person's request and came back on, because
//! connect re-opened every tool on the trusted server at every start-up. Four deploys in
//! one afternoon restored a capability they had said no to, and nothing announced it.

use super::tools_to_open_on_connect;

fn ids(all: &[&str]) -> Vec<String> {
    all.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn a_tool_the_person_blocked_is_never_reopened() {
    // The bug, exactly. `HassBroadcast` has a stored decision, so connect leaves it alone
    // however many times the node restarts.
    let open_these = tools_to_open_on_connect(
        &ids(&["house.HassTurnOn", "house.HassBroadcast"]),
        &ids(&["house."]),
        &ids(&["house.HassBroadcast"]),
    );
    assert_eq!(open_these, vec!["house.HassTurnOn"]);
}

#[test]
fn a_tool_the_person_allowed_is_left_alone_too() {
    // Not just blocks. Anything already ruled on is theirs, and rewriting it to the same
    // value would still be the system overwriting a decision it did not make.
    assert!(
        tools_to_open_on_connect(
            &ids(&["house.HassTurnOn"]),
            &ids(&["house."]),
            &ids(&["house.HassTurnOn"]),
        )
        .is_empty()
    );
}

#[test]
fn a_new_tool_on_a_trusted_server_is_still_opened() {
    // The point of the flag, and what must survive the fix: a server that grows a tool
    // should not need a click for it.
    let open_these = tools_to_open_on_connect(
        &ids(&["house.HassTurnOn", "house.HassBrandNew"]),
        &ids(&["house."]),
        &ids(&["house.HassTurnOn"]),
    );
    assert_eq!(open_these, vec!["house.HassBrandNew"]);
}

#[test]
fn an_untrusted_server_is_not_touched_at_all() {
    assert!(
        tools_to_open_on_connect(&ids(&["elsewhere.DoAThing"]), &ids(&["house."]), &[],).is_empty()
    );
}

/// The other half, and the one that was missing.
///
/// Live: a freshly added search server read "Allow all its tools: On" with all eight of
/// its tools blocked, because the only thing the on direction did was store the flag and
/// leave the opening to connect — which skips anything already ruled on.
///
/// Connect is a default and must not overwrite a decision. The toggle **is** a decision.
#[test]
fn the_toggle_governs_every_tool_including_the_ruled_on_ones() {
    let governed = super::tools_the_toggle_governs(
        &ids(&["search.web", "search.news", "search.images"]),
        "search",
    );
    assert_eq!(governed, vec!["search.web", "search.news", "search.images"]);
}

#[test]
fn the_toggle_stops_at_its_own_server() {
    // Same rule as the off direction, which already closed only its own server's tools.
    // A prefix that is a prefix of another name must not reach into it.
    let governed = super::tools_the_toggle_governs(
        &ids(&["search.web", "search-extra.web", "house.HassTurnOn"]),
        "search",
    );
    assert_eq!(governed, vec!["search.web"]);
}
