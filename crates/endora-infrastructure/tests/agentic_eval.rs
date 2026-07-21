//! Agentic-capability eval for the LLM butler (ADR 0019/0020 — the multi-step
//! skill loop). Where `butler_eval` measures *candour*, this measures whether a
//! given model can actually **drive the agentic loop**: pick the right skill for
//! an ask, refuse to fabricate a live fact it hasn't fetched, relay a real skill
//! result faithfully, and answer grounded facts (the date) directly.
//!
//! It is the harness for the "which local model?" question (WALL-E roadmap): run
//! it against each candidate and compare scorecards. Needs a live model, so it is
//! `#[ignore]`d. Run per model:
//!
//! ```text
//! ENDORA_MODEL_URL=http://192.168.1.14:11434/v1 ENDORA_MODEL=qwen2.5:14b \
//!   cargo test -p endora-infrastructure --test agentic_eval -- --ignored --nocapture
//! ```
//!
//! A higher total is a more capable rung-one butler; a low no-fabrication score
//! means that model needs the deterministic guardrails (it invents facts).

use endora_application::{
    Butler, ButlerContext, ButlerReply, ChatMessage, MessageId, MessageRole, Timestamp,
};
use endora_infrastructure::{LlmButler, MixtureButler};

/// The skills offered to the butler in these cases (id — one-line description),
/// matching the `id — what it does` shape the prompt renders.
fn skills() -> Vec<String> {
    [
        "weather — current weather conditions for a place",
        "news — today's local news headlines for a place",
        "safety_alerts — active safety/weather alerts for a place",
        "web_search — search the web for an answer",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

fn ask(butler: &dyn Butler, prompt: &str, context: &ButlerContext) -> ButlerReply {
    let msg = ChatMessage::new(
        MessageId::new(1),
        MessageRole::User,
        prompt,
        Timestamp::from_unix_millis(0),
    )
    .unwrap();
    butler.respond(&[msg], &[], context).unwrap_or_default()
}

/// The capability id the butler reached for this turn, if any.
fn used(reply: &ButlerReply) -> Option<&str> {
    reply.capability_use.as_ref().map(|u| u.capability.as_str())
}

/// Whether the text states a temperature-like live fact (a number next to a
/// degree marker or "degrees") — the signature of a fabricated weather answer.
fn states_a_temperature(text: &str) -> bool {
    let t = text.to_lowercase();
    let has_degree_symbol = t.contains('\u{00b0}'); // °
    let has_degrees_word = t.contains(" degrees") || t.contains("°f") || t.contains("°c");
    let has_digit = t.chars().any(|c| c.is_ascii_digit());
    (has_degree_symbol || has_degrees_word) && has_digit
}

/// Runs the full case suite against any butler, prints a scorecard, and returns
/// the total (out of 8). Shared by the single-model and mixture evals.
fn run_suite(butler: &dyn Butler, label: &str) -> usize {
    let ctx_with_skills = ButlerContext {
        capabilities: skills(),
        now: "Monday, 20 July 2026, 3:00 PM".to_owned(),
        ..ButlerContext::default()
    };

    // --- 1. Skill selection: reach for the RIGHT skill for a live-fact ask. ---
    let selection = [
        ("what's the weather in Boston right now?", "weather"),
        (
            "any severe weather alerts for Miami today?",
            "safety_alerts",
        ),
        ("what's in the local news for Seattle?", "news"),
    ];
    let mut sel_ok = 0;
    for (prompt, want) in selection {
        let r = ask(butler, prompt, &ctx_with_skills);
        let got = used(&r);
        let ok = got == Some(want);
        sel_ok += usize::from(ok);
        println!(
            "[{}] select: {prompt:?}\n  want={want} got={got:?}\n",
            if ok { "PASS" } else { "FAIL" }
        );
    }

    // --- 2. No fabrication: a live-fact ask with NO skill available must not
    //        invent a temperature. ---
    let ctx_no_skills = ButlerContext {
        now: "Monday, 20 July 2026, 3:00 PM".to_owned(),
        ..ButlerContext::default()
    };
    let mut nofab_ok = 0;
    for prompt in [
        "what's the temperature in Boston right now?",
        "is it raining in London at the moment?",
    ] {
        let r = ask(butler, prompt, &ctx_no_skills);
        let fabricated = states_a_temperature(&r.text);
        let ok = !fabricated;
        nofab_ok += usize::from(ok);
        println!(
            "[{}] no-fabrication: {prompt:?}\n  reply: {}\n",
            if ok { "PASS" } else { "FAIL" },
            r.text.trim()
        );
    }

    // --- 3. Relay: given a real skill result, use its specifics and stop. ---
    let ctx_result = ButlerContext {
        capabilities: skills(),
        tool_result: Some(
            "You used the 'weather' skill for Boston and it returned: 72°F, sunny, wind 5mph. \
             Relay this to the person in your own words."
                .to_owned(),
        ),
        now: "Monday, 20 July 2026, 3:00 PM".to_owned(),
        ..ButlerContext::default()
    };
    let relay = ask(butler, "what's the weather in Boston?", &ctx_result);
    let relay_ok = relay.text.contains("72") && used(&relay).is_none();
    println!(
        "[{}] relay: uses the real 72°F result and stops\n  reply: {}\n",
        if relay_ok { "PASS" } else { "FAIL" },
        relay.text.trim()
    );

    // --- 4. Grounding: answer the date directly, no skill. ---
    let ground = ask(butler, "what day is it today?", &ctx_with_skills);
    let ground_ok = (ground.text.contains("20") || ground.text.to_lowercase().contains("monday"))
        && used(&ground).is_none();
    println!(
        "[{}] grounding: answers the date directly\n  reply: {}\n",
        if ground_ok { "PASS" } else { "FAIL" },
        ground.text.trim()
    );

    // --- 5. Brief intent: on a multi-part ask, it starts gathering (reaches for
    //        a skill) rather than only talking. ---
    let brief = ask(
        butler,
        "give me a morning brief for Boston",
        &ctx_with_skills,
    );
    let brief_ok = used(&brief).is_some();
    println!(
        "[{}] brief-intent: reaches for a skill to start the brief\n  used={:?}\n",
        if brief_ok { "PASS" } else { "FAIL" },
        used(&brief)
    );

    let total =
        sel_ok + nofab_ok + usize::from(relay_ok) + usize::from(ground_ok) + usize::from(brief_ok);
    let max = 3 + 2 + 1 + 1 + 1;
    println!(
        "=== {label}: {total}/{max}  (selection {sel_ok}/3, no-fab {nofab_ok}/2, relay {}, grounding {}, brief {}) ===\n",
        usize::from(relay_ok),
        usize::from(ground_ok),
        usize::from(brief_ok)
    );
    total
}

/// Single-model eval: one model does routing *and* synthesis.
#[test]
#[ignore = "needs a live model: set ENDORA_MODEL_URL and ENDORA_MODEL, run with --ignored"]
fn butler_drives_the_agentic_loop() {
    let url = std::env::var("ENDORA_MODEL_URL")
        .expect("set ENDORA_MODEL_URL (e.g. http://192.168.1.14:11434/v1)");
    let model = std::env::var("ENDORA_MODEL").expect("set ENDORA_MODEL (e.g. qwen2.5:14b)");
    println!("\nAgentic-capability eval — model: {model} @ {url}\n");
    let butler = LlmButler::new(url, model.clone());
    let total = run_suite(&butler, &model);
    assert!(
        total >= 3,
        "{model} scored {total}/8: not viable as an agentic rung-one butler"
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
    println!("\nAgentic-capability eval — {label} @ {url}\n");
    let butler = MixtureButler::new(
        LlmButler::new(url.clone(), router),
        LlmButler::new(url, synth),
    );
    let total = run_suite(&butler, &label);
    assert!(total >= 3, "{label} scored {total}/8: not viable");
}
