//! Agentic-capability eval for the LLM butler (ADR 0056/0020/0027 — the
//! multi-step skill loop and the self-improving model layer). It measures whether
//! a given model can actually **drive the agentic loop**: pick the right skill,
//! refuse to fabricate a live fact, relay a real result faithfully, answer
//! grounded facts directly, hold the L2 "Jarvis" behaviours (use a configured
//! integration, never bluff/deny, stay in English, keep casual chat warm), and —
//! L3, ADR 0055 — build a sound **understanding** of the person: form beliefs from
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

use endora_infrastructure::model_layer::{RepeatedScore, evaluate_repeated};
use endora_infrastructure::{LlmButler, MixtureButler};

/// How many times to run the battery. Overridable with `ENDORA_EVAL_RUNS`.
///
/// More than one, always: a single run is a smoke test. Sampling is
/// non-deterministic and borderline cases flip — two consecutive runs of
/// `qwen2.5:7b` scored L1 6/8 then 8/8 with nothing in the routing path changed.
fn runs() -> usize {
    std::env::var("ENDORA_EVAL_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3)
}

/// Prints the per-case pass rates, the per-tier scores, and — the point of running
/// more than once — the **spread**, which is the resolution of the instrument.
fn report(label: &str, score: &RepeatedScore) {
    let n = score.runs.len();
    println!("\n=== {label} ({n} runs) ===");
    for rate in &score.rates {
        let mark = if rate.passes == rate.runs {
            "PASS"
        } else if rate.passes == 0 {
            "FAIL"
        } else {
            "FLAKY"
        };
        println!(
            "  [{:>5}] {}/{}  {}",
            mark, rate.passes, rate.runs, rate.name
        );
    }
    if let Some(last) = score.runs.last() {
        println!(
            "  tiers (last run): L1 {}/{}, L2 {}/{}, L3 {}/{}",
            last.l1, last.l1_max, last.l2, last.l2_max, last.l3, last.l3_max
        );
    }
    println!(
        "=== {label}: mean {:.1}/{}, range {}–{}, spread {} ===",
        score.mean_total(),
        score.runs.first().map_or(0, |s| s.max()),
        score.min_total(),
        score.max_total(),
        score.spread()
    );
    let unstable = score.unstable();
    if unstable.is_empty() {
        println!("    every case behaved the same way each run.");
    } else {
        println!(
            "    {} case(s) flipped between runs — the model is marginal on these, and",
            unstable.len()
        );
        println!(
            "    any comparison closer than a spread of {} is noise.",
            score.spread()
        );
    }
    println!();
}

/// Single-model eval: one model does routing *and* synthesis.
#[test]
#[ignore = "needs a live model: set ENDORA_MODEL_URL and ENDORA_MODEL, run with --ignored"]
fn butler_drives_the_agentic_loop() {
    let url = std::env::var("ENDORA_MODEL_URL")
        .expect("set ENDORA_MODEL_URL (e.g. http://192.168.1.14:11434/v1)");
    let model = std::env::var("ENDORA_MODEL").expect("set ENDORA_MODEL (e.g. qwen2.5:14b)");
    let butler = LlmButler::new(url, model.clone());
    let score = evaluate_repeated(&butler, runs());
    report(&model, &score);
    // Asserted on the WORST run, not the mean: a butler that is sometimes unusable
    // is unusable. Understanding is the only model Endora keeps of a person
    // (ADR 0052), so a model that forms none is not viable however well it routes.
    let worst = score
        .runs
        .iter()
        .min_by_key(|s| s.total())
        .expect("at least one run");
    assert!(
        worst.l1 * 4 >= worst.l1_max,
        "{model} scored L1 {}/{} at worst: not viable as an agentic rung-one butler",
        worst.l1,
        worst.l1_max
    );
    assert!(
        worst.l3 * 3 >= worst.l3_max,
        "{model} scored L3 {}/{} at worst: too weak at understanding to be the brain",
        worst.l3,
        worst.l3_max
    );
}

/// Mixture eval (ADR 0055): a routing specialist + a synthesizing generalist.
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
    let score = evaluate_repeated(&butler, runs());
    report(&label, &score);
    let worst = score
        .runs
        .iter()
        .min_by_key(|s| s.total())
        .expect("at least one run");
    assert!(
        worst.l1 * 4 >= worst.l1_max,
        "{label} scored L1 {}/{} at worst: not viable",
        worst.l1,
        worst.l1_max
    );
}
