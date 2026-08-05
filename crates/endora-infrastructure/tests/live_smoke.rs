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
//! make smoke                        # ENDORA_URL and ENDORA_TOKEN from local.mk
//! ENDORA_URL=https://host:8787 ENDORA_TOKEN=… make smoke
//! ```
//!
//! The node refuses `/v1` without a credential, so this suite needs one too — and it should be
//! the **narrowest**, because it lives in a plaintext file on a laptop. `POST
//! /v1/session/checks` mints a read-only token that is also refused `/v1/export`, so a leak
//! cannot redirect the deep model, widen a capability, purge memory, or pull the whole
//! conversation at once.
//!
//! It can still read beliefs and context. That is deliberate: three of the invariants below
//! assert on real belief statements, and asserting about real data is why this tier exists.

use endora_application::{reads_as_an_instruction, says_the_same_thing, statements_disagree};
use endora_understanding::domain::notions::MOST_NOTIONS_AT_ONCE;
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
    "/v1/notions",
    "/v1/outcomes",
    "/v1/repairs",
    "/v1/standing-trouble",
    "/v1/reliability",
    "/v1/context",
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

/// The node's token, from the environment.
///
/// `make smoke` passes it through from `local.mk`. Empty gives a 401 and the assertion below
/// names it, which is a better failure than a suite that silently skips because a credential
/// was missing.
fn token() -> String {
    std::env::var("ENDORA_TOKEN").unwrap_or_default()
}

/// Fetches one path, failing with the status and the path when it is not a success.
fn get(path: &str) -> Value {
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

/// Posts a JSON body to one path, returning the status and the body — the caller judges,
/// because on this suite's credential a refusal can be the correct answer (see
/// [`the_verdict_channel_is_wired_and_scoped`]).
fn post(path: &str, body: &Value) -> (u16, String) {
    let url = format!("{}{path}", base());
    let mut response = agent()
        .post(&url)
        .header("Authorization", &format!("Bearer {}", token()))
        .send_json(body)
        .unwrap_or_else(|e| panic!("{path}: could not reach the node at {url}: {e}"));
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    (status, text)
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

#[test]
#[ignore = "talks to a deployed node: run with `make smoke` after `make deploy`"]
fn the_butler_is_told_where_the_person_is() {
    // Five separate times a fact was believed to be reaching a turn and was not — presence,
    // the facts behind an answer, the activity account, the calendar. Each was diagnosed by
    // inference, because there was no way to look.
    //
    // A service Endora has direct reach into knows whether anyone is in the house. If that
    // is missing here, the model never had it, and the reply that follows is uninformed
    // rather than unhelpful (ADR 0056).
    let told = get("/v1/context");
    let right_now = told["right_now"].as_array().cloned().unwrap_or_default();
    assert!(
        !right_now.is_empty(),
        "the turn carries nothing about the person's world: {told:#}"
    );

    // And where they live, which is the fact this test used not to check. Four briefs
    // opened with the wrong city while this test was green, because it asserted that the
    // context *contained* something and never that the answer *used* it. The turn now
    // fills a missing place in from here rather than asking the model to remember it, so
    // an empty value is no longer a prompt that reads oddly — it is a skill call going
    // out with no place at all.
    let place = told["where_they_are"].as_str().unwrap_or_default();
    assert!(
        !place.trim().is_empty(),
        "the node does not know where the person lives, so every skill that needs a \
         place will be asked without one: {told:#}"
    );
}

#[test]
#[ignore = "talks to a deployed node: run with `make smoke` after `make deploy`"]
fn nothing_it_wonders_about_is_unfounded() {
    // The one guarantee ADR 0057 rests on, checked against the real thing rather than a
    // fixture: a notion exists only if records that exist bore it out. A statement about the
    // person with no evidence behind it is the failure this whole feature was designed around,
    // and it is the kind that would be invisible — the card would read perfectly well.
    //
    // Uses the production constant rather than a copy: a smoke test with its own idea of the
    // cap is testing its own idea.
    let wondering = get("/v1/notions");
    let open = wondering.as_array().cloned().unwrap_or_default();

    for notion in &open {
        let because = notion["because"].as_array().cloned().unwrap_or_default();
        assert!(
            !because.is_empty(),
            "this is a statement about the person with nothing behind it: {notion:#}"
        );
        assert!(
            !notion["statement"].as_str().unwrap_or_default().is_empty(),
            "a notion with no statement: {notion:#}"
        );
    }

    assert!(
        open.len() <= MOST_NOTIONS_AT_ONCE,
        "{} open notions is past the cap of {MOST_NOTIONS_AT_ONCE} — the bound that keeps this \
         a cursor rather than the queue ADR 0029 deleted: {wondering:#}",
        open.len()
    );
}

#[test]
#[ignore = "talks to a deployed node: run with `make smoke` after `make deploy`"]
fn the_verdict_channel_is_wired_and_scoped() {
    // ADR 0066 withdraws a tool's autonomy on the person's verdict — and when that record
    // shipped, zero reactions had ever been stored on this install. The endpoint is
    // unit-tested in-process; what had never been proven is the deployed wire. Writing
    // this test proved something else first: the suite's own credential is **checks
    // scope** — reads pass, writes are refused — which is why every smoke before this one
    // is a GET. That is the monitoring credential working as designed, and this test pins
    // it rather than working around it.
    //
    // So it asserts whichever truth its credential can reach:
    // - a full-scope token proves the round trip: accepted, echoed, persisted;
    // - the checks-scope token proves the refusal: the same token that just read the
    //   record may not write a verdict into it. Either outcome is the node behaving;
    //   anything else — a 500, a 404, a write that vanishes — fails.
    //
    // The true end-to-end (a thumb on the console button) is the person's to press, and
    // doing so also seeds ADR 0066 with its first real signal.
    let outcomes = get("/v1/outcomes");
    let all = outcomes.as_array().cloned().unwrap_or_default();
    if all.is_empty() {
        return; // a fresh install has nothing to react to; that is not a broken channel
    }
    // The oldest unjudged outcome: far outside the repeat-ask marker's window, and
    // `no_reaction` counts for neither side in every consumer, so a full-scope run
    // leaves the graduation arithmetic exactly as it found it.
    let Some(unjudged) = all
        .iter()
        .filter(|o| o["reaction"].is_null())
        .min_by_key(|o| o["at_ms"].as_i64().unwrap_or(i64::MAX))
    else {
        return; // every outcome already carries a verdict — the channel demonstrably works
    };
    let id = unjudged["id"].as_str().expect("an outcome has an id");

    let (status, body) = post(
        &format!("/v1/outcomes/{id}/reaction"),
        &serde_json::json!({ "reaction": "no_reaction" }),
    );
    if status == 401 {
        // The read above succeeded with this same token, so this is not a bad
        // credential — it is the write scope holding. Pin that and stop.
        assert!(
            body.contains("token"),
            "refused, but not by the token check: {body}"
        );
        return;
    }
    assert!(
        (200..300).contains(&status),
        "/v1/outcomes/{id}/reaction: answered {status}: {}",
        body.chars().take(300).collect::<String>()
    );
    let answered: Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("reaction reply is not JSON ({e}): {body}"));
    assert_eq!(
        answered["reaction"].as_str(),
        Some("no_reaction"),
        "the endpoint accepted the reaction but did not echo it: {answered:#}"
    );
    // Read back through the same door the console uses — a stored verdict is the thing
    // ADR 0066 consumes.
    let read_back = get("/v1/outcomes");
    let stored = read_back
        .as_array()
        .and_then(|xs| xs.iter().find(|o| o["id"].as_str() == Some(id)))
        .unwrap_or_else(|| panic!("the outcome vanished on read-back: {id}"));
    assert_eq!(
        stored["reaction"].as_str(),
        Some("no_reaction"),
        "the reaction was accepted but not persisted: {stored:#}"
    );
}
