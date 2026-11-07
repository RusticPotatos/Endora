//! The owner's scenarios for the relay's own count of model requests
//! (`app/node/features/*.feature`). Cucumber runs inside one `#[test]` because the steps use the
//! relay tests' fakes (HQ, the provider) and the real router, which live in this crate; a step
//! does what a member's turn does and reads the count back through the node's store.

use super::tests::{
    anthropic, anthropic_failing, anthropic_serving, call_once, census, hq, hq_refusing_usage, node,
};
use axum::http::StatusCode;
use cucumber::{World, given, then, when};
use endora_infrastructure::RelayRequestRow;
use serde_json::json;

use crate::api::AppState;

/// What the provider does in a scenario.
#[derive(Debug, Default)]
enum Provider {
    #[default]
    Plain,
    Serving(&'static str),
    Overloaded,
}

#[derive(World, Default)]
struct RelayWorld {
    provider: Provider,
    hq_takes_usage: Option<bool>,
    cuts_streams: bool,
    state: Option<AppState>,
    counted: Vec<RelayRequestRow>,
}

impl std::fmt::Debug for RelayWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayWorld")
            .field("provider", &self.provider)
            .field("counted", &self.counted)
            .finish_non_exhaustive()
    }
}

#[given(expr = "the provider answers as {string}")]
fn provider_answers_as(w: &mut RelayWorld, name: String) {
    w.provider = Provider::Serving(Box::leak(name.into_boxed_str()));
}

#[given(expr = "the provider is overloaded")]
fn provider_overloaded(w: &mut RelayWorld) {
    w.provider = Provider::Overloaded;
}

#[given(expr = "the provider cuts its streams short")]
fn provider_cuts(w: &mut RelayWorld) {
    w.cuts_streams = true;
}

#[given(expr = "HQ is not taking usage reports")]
fn hq_refuses(w: &mut RelayWorld) {
    w.hq_takes_usage = Some(false);
}

async fn make_call(w: &mut RelayWorld, stream: bool) {
    let (hq_url, _) = if w.hq_takes_usage == Some(false) {
        hq_refusing_usage(400_000)
    } else {
        hq(400_000)
    };
    let (upstream, _) = match w.provider {
        Provider::Serving(name) => anthropic_serving(name),
        Provider::Overloaded => anthropic_failing(529),
        Provider::Plain => anthropic(),
    };
    let state = node(&hq_url, &upstream);
    let extra = if stream && w.cuts_streams {
        json!({ "stop_sequences": ["cut"] })
    } else {
        json!({})
    };
    let status = call_once(&state, stream, extra).await;
    // Whatever the provider did, the member's call got an answer of some kind.
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR);
    w.counted = census(&state, 1).await;
    w.state = Some(state);
}

#[when(expr = "a member's turn makes one model call")]
async fn one_call(w: &mut RelayWorld) {
    make_call(w, false).await;
}

#[when(expr = "a member's turn makes one streamed model call")]
async fn one_streamed_call(w: &mut RelayWorld) {
    make_call(w, true).await;
}

fn the_one(w: &RelayWorld) -> &RelayRequestRow {
    assert_eq!(w.counted.len(), 1, "{:?}", w.counted);
    &w.counted[0]
}

#[then(
    regex = r"^the node counts (\d+) model request, which (was answered|failed|was interrupted)$"
)]
fn counts_one(w: &mut RelayWorld, n: usize, how: String) {
    assert_eq!(w.counted.len(), n, "{:?}", w.counted);
    let word = match how.as_str() {
        "was answered" => "answered",
        "was interrupted" => "interrupted",
        _ => "failed",
    };
    assert_eq!(the_one(w).outcome, word);
}

#[then(expr = "the node says {string} answered it")]
fn says_model(w: &mut RelayWorld, name: String) {
    assert_eq!(the_one(w).served_model.as_deref(), Some(name.as_str()));
}

#[then(expr = "the node names no model for it")]
fn names_no_model(w: &mut RelayWorld) {
    assert_eq!(the_one(w).served_model, None);
}

#[then(expr = "the node says HQ was told")]
fn hq_was_told(w: &mut RelayWorld) {
    assert_eq!(the_one(w).hq_told, Some(true));
}

#[then(expr = "the node says HQ was not told")]
fn hq_was_not_told(w: &mut RelayWorld) {
    assert_eq!(the_one(w).hq_told, Some(false));
}

#[then(expr = "the node does not count it as answered")]
fn not_answered(w: &mut RelayWorld) {
    let row = the_one(w);
    assert_ne!(row.outcome, "answered");
    assert!(!row.usage_complete);
}

/// Runs every scenario; a failed, skipped or unparsed step fails this test.
#[tokio::test(flavor = "multi_thread")]
async fn the_owners_scenarios_for_the_relays_own_count_pass() {
    use cucumber::cli::Parser as _;
    use cucumber::writer::Stats as _;
    // No command line: libtest's own filter and flags are not cucumber's.
    let none = cucumber::cli::Opts::<
        cucumber::parser::basic::Cli,
        cucumber::runner::basic::Cli,
        cucumber::writer::basic::Cli,
    >::try_parse_from(["cucumber"])
    .expect("cucumber's own options");
    let writer = RelayWorld::cucumber()
        .with_cli(none)
        .fail_on_skipped()
        .run("features/relay")
        .await;
    assert!(
        !writer.execution_has_failed() && writer.parsing_errors() == 0,
        "a scenario failed: see the steps above"
    );
    assert!(writer.passed_steps() > 0, "no scenario ran");
}
