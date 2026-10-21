//! Invariants asserted against the **deployed** instance, right after a deploy.
//!
//! Five of the last six bugs in this system were found by a person looking at a screen and
//! saying "this is embarrassing" — and every one of them was already visible in the running
//! node's own HTTP surface, minutes after the deploy that introduced it. The information was
//! never missing. Nobody was looking.
//!
//! So this is the tier that unit tests structurally cannot cover:
//!
//! - **CI cannot run it.** GitHub cannot reach the person's house, so it is `#[ignore]`d and
//!   run by `make smoke` after `make deploy`.
//! - **It asserts about real data**, which is the whole point. Local fixtures are written by
//!   whoever writes the test, and are therefore exactly as imaginative as they were. A live
//!   house has scenes, diagnostic entities, aliases and forty devices.
//! - **It uses the production rules**, imported from the crates that implement them, so an
//!   invariant cannot drift away from the behaviour it is checking.
//!
//! What it deliberately does **not** do is judge whether a screen reads well. That still
//! needs eyes. The aim is to make a screenshot the last line of defence rather than the
//! first.
//!
//! ```text
//! make smoke                        # ENDORA_URL and ENDORA_TOKEN from local.mk
//! ENDORA_URL=https://host:8787 ENDORA_TOKEN=… make smoke
//! ```
//!
//! The node refuses `/v1` without a credential, so this suite needs one too — and it should be
//! the **narrowest**, because it lives in a plaintext file on a laptop. `POST
//! /v1/session/checks` mints a read-only token that is also refused `/v1/export`, so a leak
//! cannot change a model, widen a capability, delete the record, or pull it all at once.

use serde_json::Value;

/// Where the node is, unless told otherwise.
const DEFAULT_URL: &str = "https://127.0.0.1:8787";

/// Everything the interface loads on start-up. A 500 from any of these is a broken screen,
/// and it is how the `config_writes` schema divergence showed up in production — twice,
/// with a green test suite both times.
const WHAT_THE_INTERFACE_LOADS: &[&str] = &[
    "/v1/capabilities",
    "/v1/activity",
    "/v1/config-writes",
    "/v1/audit",
    "/v1/mcp/servers",
    "/v1/connect/connected",
    "/v1/model-lists",
    "/v1/hq",
];

fn agent() -> ureq::Agent {
    // The node serves self-signed TLS on the person's own network, which is the whole
    // deployment model. Verification is off for that reason and no other: this test talks
    // to one host, named by the operator, on their LAN.
    ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .disable_verification(true)
                .build(),
        )
        // A 4xx or 5xx must reach the assertion below rather than being raised as a
        // transport error: "answered 500" is a diagnosis and "could not reach the node" is
        // not, and the first run of this suite reported a 405 as unreachable.
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .into()
}

fn base() -> String {
    std::env::var("ENDORA_URL")
        .unwrap_or_else(|_| DEFAULT_URL.to_owned())
        .trim_end_matches('/')
        .to_owned()
}

/// The node's token, from the environment.
///
/// `make smoke` passes it through from `local.mk`. Empty gives a 401 and the assertion below
/// names it, which is a better failure than a suite that silently skips because a credential
/// was missing.
fn token() -> String {
    std::env::var("ENDORA_TOKEN").unwrap_or_default()
}

/// How long the suite waits for a node that is still starting before it checks anything:
/// `make smoke` straight after `make deploy` met a node that had been up for a second.
const READY_WITHIN: std::time::Duration = std::time::Duration::from_secs(30);

/// How long it waits between two looks at the node's health.
const READY_PAUSE: std::time::Duration = std::time::Duration::from_secs(1);

/// Asks `up` until it says yes or `within` has passed, waiting `pause` between two asks.
/// Whether it said yes. Asks at least once.
fn wait_until(
    mut up: impl FnMut() -> bool,
    within: std::time::Duration,
    pause: std::time::Duration,
) -> bool {
    let start = std::time::Instant::now();
    loop {
        if up() {
            return true;
        }
        if start.elapsed() + pause > within {
            return false;
        }
        std::thread::sleep(pause);
    }
}

/// Waits, once per run of the suite, for the node's health route to answer (it needs no
/// credential), so the first check is not of a node still starting.
fn ready() {
    static READY: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    READY.get_or_init(|| {
        let url = format!("{}/health", base());
        let probe: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .disable_verification(true)
                    .build(),
            )
            .timeout_global(Some(std::time::Duration::from_secs(3)))
            .build()
            .into();
        let up = wait_until(|| probe.get(&url).call().is_ok(), READY_WITHIN, READY_PAUSE);
        assert!(
            up,
            "{url}: the node did not answer its health route within {}s",
            READY_WITHIN.as_secs()
        );
    });
}

#[test]
fn the_wait_for_the_node_asks_until_it_is_up_and_gives_up_in_time() {
    use std::time::Duration;
    let mut asked = 0;
    let up = wait_until(
        || {
            asked += 1;
            asked == 3
        },
        Duration::from_secs(5),
        Duration::from_millis(1),
    );
    assert!(up);
    assert_eq!(asked, 3, "it stops asking once the node is up");
    let mut asked = 0;
    let started = std::time::Instant::now();
    let up = wait_until(
        || {
            asked += 1;
            false
        },
        Duration::from_millis(30),
        Duration::from_millis(5),
    );
    assert!(!up);
    assert!(asked >= 2, "it asked again: {asked}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "it gave up in time"
    );
}

/// Fetches one path, failing with the status and the path when it is not a success.
fn get(path: &str) -> Value {
    ready();
    let url = format!("{}{path}", base());
    let mut response = agent()
        .get(&url)
        .header("Authorization", &format!("Bearer {}", token()))
        .call()
        .unwrap_or_else(|e| panic!("{path}: could not reach the node at {url}: {e}"));
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().unwrap_or_default();
    assert!(
        status != 401,
        "{path}: answered 401 — set ENDORA_TOKEN to the node's token (it is printed to the \
         container log on first run)"
    );
    assert!(
        (200..300).contains(&status),
        "{path}: answered {status} — the interface shows this as a broken screen. Body: {}",
        body.chars().take(300).collect::<String>()
    );
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("{path}: not JSON ({e}): {body}"))
}

#[test]
#[ignore = "talks to a deployed node: run with `make smoke` after `make deploy`"]
fn every_screen_the_interface_opens_still_answers() {
    for path in WHAT_THE_INTERFACE_LOADS {
        let _ = get(path);
    }
}

#[test]
#[ignore = "talks to a deployed node: run with `make smoke` after `make deploy`"]
fn the_chat_is_gone_from_the_deployed_node() {
    // Endora is HQ (ADR 0136): the chat's routes answer nothing, so no client left behind (an
    // old handheld, a bookmark) can reach what was removed, and health says no turn is running.
    ready();
    for path in ["/v1/chat", "/v1/brain", "/v1/agenda", "/v1/mail"] {
        let url = format!("{}{path}", base());
        let status = agent()
            .get(&url)
            .header("Authorization", &format!("Bearer {}", token()))
            .call()
            .unwrap_or_else(|e| panic!("{path}: could not reach the node at {url}: {e}"))
            .status()
            .as_u16();
        assert_eq!(status, 404, "{path} still answers on the deployed node");
    }
    let health: Value = agent()
        .get(&format!("{}/health", base()))
        .call()
        .and_then(|mut r| r.body_mut().read_json())
        .unwrap_or_else(|e| panic!("/health: {e}"));
    assert!(
        health.get("busy").is_none(),
        "health still reports a chat turn: {health}"
    );
}
