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
//!   invariant cannot drift away from the behaviour it is checking. A smoke test that
//!   re-implements `reads_as_an_instruction` is testing its own copy.
//!
//! What it deliberately does **not** do is judge whether a screen reads well. That still
//! needs eyes. The aim is to make a screenshot the last line of defence rather than the
//! first.
//!
//! ```text
//! make smoke                        # ENDORA_URL from local.mk, or the default below
//! ENDORA_URL=https://host:8787 make smoke
//! ```

use endora_application::{reads_as_an_instruction, says_the_same_thing, statements_disagree};
use serde_json::Value;

/// Where the node is, unless told otherwise.
const DEFAULT_URL: &str = "https://127.0.0.1:8787";

/// Everything the interface loads on start-up. A 500 from any of these is a broken screen,
/// and it is how the `config_writes` schema divergence showed up in production — twice,
/// with a green test suite both times.
const WHAT_THE_INTERFACE_LOADS: &[&str] = &[
    "/v1/understanding",
    "/v1/capabilities",
    "/v1/activity",
    "/v1/intentions",
    "/v1/outcomes",
    "/v1/repairs",
    "/v1/standing-trouble",
    "/v1/reliability",
    "/v1/config-writes",
    "/v1/autonomy",
    "/v1/audit",
    "/v1/chat",
    "/v1/checkin",
    "/v1/deep-model",
];

/// More things than this being raised at once is a failure whatever the cause.
///
/// Not a diagnosis — a tripwire. The absence-word list once flagged 28 healthy scenes, and
/// the specific reason was unguessable in advance; the *shape* of the failure was not. A
/// butler asking about more than a handful of things at once has stopped being a butler,
/// so this fires and someone goes and looks.
const MOST_IT_MAY_EVER_RAISE: usize = 10;

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

/// Fetches one path, failing with the status and the path when it is not a success.
fn get(path: &str) -> Value {
    let url = format!("{}{path}", base());
    let mut response = agent()
        .get(&url)
        .call()
        .unwrap_or_else(|e| panic!("{path}: could not reach the node at {url}: {e}"));
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().unwrap_or_default();
    assert!(
        (200..300).contains(&status),
        "{path}: answered {status} — the interface shows this as a broken screen. Body: {}",
        body.chars().take(300).collect::<String>()
    );
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("{path}: not JSON ({e}): {body}"))
}

fn statements(beliefs: &Value) -> Vec<String> {
    beliefs
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(|b| b["statement"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
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
fn nothing_it_believes_is_a_spent_instruction() {
    // Live: "you want me to turn off the kitchen light — because turn off the kitchen light
    // entity" sat on the understanding screen long after the rule rejecting that shape
    // shipped, because the rule only ran at formation. Asserting it here catches both the
    // rule regressing and the tidy pass failing to reach backwards.
    let offenders: Vec<String> = statements(&get("/v1/understanding"))
        .into_iter()
        .filter(|s| reads_as_an_instruction(s))
        .collect();
    assert!(
        offenders.is_empty(),
        "these are commands you gave, not facts about you: {offenders:#?}"
    );
}

#[test]
#[ignore = "talks to a deployed node: run with `make smoke` after `make deploy`"]
fn it_does_not_believe_the_same_thing_twice() {
    // Live: three cards about Fahrenheit on one screen, each asking to be confirmed
    // separately. Uses the production rule rather than a copy of it.
    let held = statements(&get("/v1/understanding"));
    let mut duplicates: Vec<(String, String)> = Vec::new();
    for (i, a) in held.iter().enumerate() {
        for b in held.iter().skip(i + 1) {
            if says_the_same_thing(a, b) {
                duplicates.push((a.clone(), b.clone()));
            }
        }
    }
    assert!(
        duplicates.is_empty(),
        "the same thought is held more than once: {duplicates:#?}"
    );
}

#[test]
#[ignore = "talks to a deployed node: run with `make smoke` after `make deploy`"]
fn no_single_belief_contradicts_everything_else() {
    // Live: one belief about thermometers was reported as contradicting all nine others,
    // because disagreement was decided from negation alone. A genuine contradiction is
    // between two beliefs; one statement at odds with the entire model is a broken rule.
    let held = statements(&get("/v1/understanding"));
    for a in &held {
        let against: Vec<&String> = held
            .iter()
            .filter(|b| *b != a && statements_disagree(a, b))
            .collect();
        assert!(
            against.len() <= 1,
            "{a:?} is reported as contradicting {} other beliefs, which is a broken rule \
             rather than {} real disagreements: {against:#?}",
            against.len(),
            against.len()
        );
    }
}

#[test]
#[ignore = "talks to a deployed node: run with `make smoke` after `make deploy`"]
fn a_settled_belief_is_never_also_a_question() {
    // Internal consistency of what the screen is told: a card cannot be settled and still
    // be low-confidence, and a card in a contradiction can never be settled.
    for belief in get("/v1/understanding").as_array().unwrap_or(&Vec::new()) {
        if belief["settled"].as_bool() != Some(true) {
            continue;
        }
        assert_eq!(
            belief["confidence"].as_str(),
            Some("high"),
            "settled but not confident: {belief}"
        );
        assert!(
            belief["contradicts"].is_null(),
            "settled while contradicting something: {belief}"
        );
    }
}

#[test]
#[ignore = "talks to a deployed node: run with `make smoke` after `make deploy`"]
fn it_is_not_asking_about_half_the_house() {
    // The tripwire. The absence-word list once flagged 28 healthy scenes against 7 real
    // devices; three days later the person would have been asked about all of them. The
    // specific cause was unguessable, the shape was not.
    let raised = get("/v1/standing-trouble");
    let count = raised.as_array().map_or(0, Vec::len);
    assert!(
        count <= MOST_IT_MAY_EVER_RAISE,
        "raising {count} problems at once is a pile of chores, not a butler: {raised:#}"
    );
}

#[test]
#[ignore = "talks to a deployed node: run with `make smoke` after `make deploy`"]
fn most_of_what_it_tries_is_not_failing_outright() {
    // The tripwire for the system rather than the model. An outright error is the most
    // visible kind of failure and the easiest to fix, so a majority of them means something
    // is broken right now — a server down, a renamed entity, a withdrawn tool still being
    // called — and not that the butler is having an off day.
    //
    // Deliberately NOT a check on `unchanged`: a tool Endora cannot know is read-only lands
    // there permanently, so a threshold on it would fail forever for a reason that is not a
    // fault (ADR 0053).
    let landing = get("/v1/reliability");
    let considered = landing["considered"].as_u64().unwrap_or_default();
    if considered < 10 {
        return; // too little to be a trend
    }
    let failed = landing["failed"].as_u64().unwrap_or_default();
    assert!(
        failed * 2 <= considered,
        "{failed} of the last {considered} actions failed outright, which is something \
         broken rather than a bad day: {landing:#}"
    );
}
