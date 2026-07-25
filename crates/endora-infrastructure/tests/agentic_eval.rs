//! Agentic-capability eval for the LLM butler (ADR 0019/0020/0027 — the
//! multi-step skill loop and the self-improving model layer). It measures whether
//! a given model can actually **drive the agentic loop**: pick the right skill,
//! refuse to fabricate a live fact, relay a real result faithfully, answer
//! grounded facts directly, hold the L2 "Jarvis" behaviours (use a configured
//! integration, never bluff/deny, stay in English, keep casual chat warm), and —
//! L3, ADR 0030 — build a sound **understanding** of the person: form beliefs from
//! real evidence, stay quiet when a turn reveals nothing, avoid re-filing what it
//! already knows, and not overclaim confidence.
//!
//! The scoring battery itself now lives in [`endora_infrastructure::model_layer`]
//! as the reusable **fitness function** ([`evaluate`]) the model layer calls to
//! rank candidates — this test is a thin, human-facing harness over it. Needs a
//! live model, so it is `#[ignore]`d. Run per model:
//!
//! ```text
//! ENDORA_MODEL_URL=http://192.168.1.14:11434/v1 ENDORA_MODEL=qwen2.5:14b \
//!   cargo test -p endora-infrastructure --test agentic_eval -- --ignored --nocapture
//! ```

use endora_infrastructure::model_layer::{Scorecard, evaluate};
use endora_infrastructure::{LlmButler, MixtureButler};

/// Prints a per-case scorecard and the level/total lines, then returns it.
///
/// **A single run is noisy.** Sampling is non-deterministic, so borderline cases flip
/// between runs — two consecutive runs of `qwen2.5:7b` scored L1 6/8 and then 8/8
/// with nothing in the routing path changed. Treat one run as a smoke test, not a
/// measurement: compare models over several runs, and do not read a few points of
/// movement as a regression or an improvement. Widening the battery is the fix, and
/// is what has to happen before scores can gate anything finer-grained than the
/// existing adoption floor (ADR 0030).
fn report(label: &str, card: &Scorecard) {
    println!("\n=== {label} ===");
    for case in &card.cases {
        println!(
            "  [{}] {}",
            if case.passed { "PASS" } else { "FAIL" },
            case.name
        );
    }
    println!(
        "=== {label}: L1 {}/{}, L2 {}/{}, L3 {}/{}, TOTAL {}/{} ===\n",
        card.l1,
        card.l1_max,
        card.l2,
        card.l2_max,
        card.l3,
        card.l3_max,
        card.total(),
        card.max()
    );
}

/// Single-model eval: one model does routing *and* synthesis.
#[test]
#[ignore = "needs a live model: set ENDORA_MODEL_URL and ENDORA_MODEL, run with --ignored"]
fn butler_drives_the_agentic_loop() {
    let url = std::env::var("ENDORA_MODEL_URL")
        .expect("set ENDORA_MODEL_URL (e.g. http://192.168.1.14:11434/v1)");
    let model = std::env::var("ENDORA_MODEL").expect("set ENDORA_MODEL (e.g. qwen2.5:14b)");
    let butler = LlmButler::new(url, model.clone());
    let card = evaluate(&butler);
    report(&model, &card);
    assert!(
        card.l1 >= 3,
        "{model} scored L1 {}/{}: not viable as an agentic rung-one butler",
        card.l1,
        card.l1_max
    );
    // Understanding is the only model Endora keeps of a person (ADR 0029), so a
    // model that forms none is not a viable butler however well it routes tools.
    assert!(
        card.l3 >= 3,
        "{model} scored L3 {}/{}: too weak at understanding to be the butler's brain",
        card.l3,
        card.l3_max
    );
}

/// Mixture eval (ADR 0027): a routing specialist + a synthesizing generalist.
/// Set ENDORA_ROUTER_MODEL and ENDORA_SYNTH_MODEL (both served at
/// ENDORA_MODEL_URL). Compares the router+synthesizer split against a single
/// model — the question being whether it matches a big generalist at less VRAM.
#[test]
#[ignore = "needs a live model: set ENDORA_MODEL_URL, ENDORA_ROUTER_MODEL, ENDORA_SYNTH_MODEL"]
fn mixture_drives_the_agentic_loop() {
    let url = std::env::var("ENDORA_MODEL_URL")
        .expect("set ENDORA_MODEL_URL (e.g. http://192.168.1.14:11434/v1)");
    let router =
        std::env::var("ENDORA_ROUTER_MODEL").expect("set ENDORA_ROUTER_MODEL (e.g. hermes3:8b)");
    let synth =
        std::env::var("ENDORA_SYNTH_MODEL").expect("set ENDORA_SYNTH_MODEL (e.g. qwen2.5:7b)");
    let label = format!("mixture(router={router}, synth={synth})");
    let butler = MixtureButler::new(
        LlmButler::new(url.clone(), router),
        LlmButler::new(url, synth),
    );
    let card = evaluate(&butler);
    report(&label, &card);
    assert!(
        card.l1 >= 3,
        "{label} scored L1 {}/{}: not viable",
        card.l1,
        card.l1_max
    );
}
