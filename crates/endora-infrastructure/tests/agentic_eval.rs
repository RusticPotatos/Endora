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
    // The real configured skill set on the live deployment (and their real
    // descriptions), so routing is tested under the same choice pressure — more
    // options + vaguer descriptions is where the live router derailed.
    [
        "weather — Current conditions and today's forecast for a place",
        "web_fetch — Fetch a web page and read its text — for research",
        "knowledge — Look up factual, encyclopedic knowledge about a topic",
        "web_search — Get a quick answer or definition from the web for a question",
        "news — Recent news headlines for a place or topic",
        "image_review — Describe or answer questions about an image",
        "safety_alerts — Active safety alerts near you — severe weather and warnings",
        "home_assistant — Read your home's state — lights, presence, sensors",
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

/// Like [`ask`], but with a prior conversation in front of the prompt — so the
/// eval can reproduce loop-level failures where history distracts the router.
fn ask_with_history(
    butler: &dyn Butler,
    prior: &[(MessageRole, &str)],
    prompt: &str,
    context: &ButlerContext,
) -> ButlerReply {
    let mut history: Vec<ChatMessage> = prior
        .iter()
        .enumerate()
        .map(|(i, (role, text))| {
            ChatMessage::new(
                MessageId::new(i as u128 + 1),
                *role,
                text,
                Timestamp::from_unix_millis(i as i64),
            )
            .unwrap()
        })
        .collect();
    history.push(
        ChatMessage::new(
            MessageId::new(999),
            MessageRole::User,
            prompt,
            Timestamp::from_unix_millis(999),
        )
        .unwrap(),
    );
    butler.respond(&history, &[], context).unwrap_or_default()
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

/// **Level 1 — basics.** Skill selection, no-fabrication, relay, grounding,
/// brief-intent. Prints a scorecard and returns (score, max). A model must clear
/// this before L2 is meaningful (the ratchet: higher levels stay meaningful only
/// once the lower ones are maxed).
fn run_level_1(butler: &dyn Butler, label: &str) -> (usize, usize) {
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
        "=== L1 {label}: {total}/{max}  (selection {sel_ok}/3, no-fab {nofab_ok}/2, relay {}, grounding {}, brief {}) ===\n",
        usize::from(relay_ok),
        usize::from(ground_ok),
        usize::from(brief_ok)
    );
    (total, max)
}

/// **Level 2 — Jarvis behaviours** (the failures a real session exposed, now
/// permanent regression cases). Uses/acknowledges a configured integration
/// (`home_assistant`), never bluffs access it didn't exercise, never denies a
/// skill it has, routes to the *right* skill, replies in coherent English, and
/// keeps casual conversation warm instead of dead-ending on the fallback. Prints
/// a scorecard and returns (score, max).
fn run_level_2(butler: &dyn Butler, label: &str) -> (usize, usize) {
    let ctx = ButlerContext {
        capabilities: skills(),
        now: "Monday, 20 July 2026, 3:00 PM".to_owned(),
        ..ButlerContext::default()
    };
    let has_cjk = |s: &str| s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));

    // 2.1 Integration invocation + right-skill: a home read must reach for
    //     home_assistant (not weather, not nothing).
    let lights = ask(butler, "are my lights on right now?", &ctx);
    let invoke_ok = used(&lights) == Some("home_assistant");
    println!(
        "[{}] integration: 'are my lights on?' → home_assistant\n  used={:?}\n",
        if invoke_ok { "PASS" } else { "FAIL" },
        used(&lights)
    );

    // 2.2 Anti-deny: it must not deny a skill it actually has.
    let denies = {
        let t = lights.text.to_lowercase();
        t.contains("don't have")
            || t.contains("do not have")
            || t.contains("can't access")
            || t.contains("cannot access")
            || t.contains("no access")
    };
    let antideny_ok = !denies;
    println!(
        "[{}] anti-deny: doesn't disclaim a configured skill\n  reply: {}\n",
        if antideny_ok { "PASS" } else { "FAIL" },
        lights.text.trim()
    );

    // 2.3 Anti-bluff: "can you see my home?" must NOT assert it sees specifics
    //     without invoking — either go check, or honestly say what it can do.
    let seehome = ask(butler, "can you see my home?", &ctx);
    let bluffed = used(&seehome).is_none() && {
        let t = seehome.text.to_lowercase();
        t.contains("yes, i can see")
            || t.contains("i can see your home")
            || t.contains("i have access to your")
    };
    let antibluff_ok = !bluffed;
    println!(
        "[{}] anti-bluff: no claim of access it didn't exercise\n  used={:?} reply: {}\n",
        if antibluff_ok { "PASS" } else { "FAIL" },
        used(&seehome),
        seehome.text.trim()
    );

    // 2.4 Language reliability: coherent English, no CJK bleed.
    let lang_ok = !has_cjk(&lights.text) && !has_cjk(&seehome.text);
    println!(
        "[{}] language: replies stay in English (no CJK bleed)\n",
        if lang_ok { "PASS" } else { "FAIL" }
    );

    // 2.5 Conversational robustness: a casual affirmation gets a warm reply, not
    //     the "not sure how to help" fallback.
    let casual = ask(butler, "hell yeah those jokes were good", &ctx);
    let fell_back = casual.text.to_lowercase().contains("not sure how to help");
    let casual_ok = !fell_back && !casual.text.trim().is_empty() && used(&casual).is_none();
    println!(
        "[{}] conversational: casual affirmation gets a warm reply\n  reply: {}\n",
        if casual_ok { "PASS" } else { "FAIL" },
        casual.text.trim()
    );

    // 2.6 History robustness (reproduces the live failure): a home ask must still
    //     route to home_assistant even after a distracting conversation. In the
    //     live session this is where the router derailed to `weather`.
    let prior = [
        (MessageRole::User, "just an ear, any good jokes?"),
        (
            MessageRole::Butler,
            "Why did the tomato turn red? Because it saw the salad dressing!",
        ),
        (MessageRole::User, "hell yea those are good"),
        (
            MessageRole::Butler,
            "Glad to hear it! Have a great day, sir!",
        ),
    ];
    let lights_hist = ask_with_history(butler, &prior, "are my lights on right now?", &ctx);
    let hist_ok = used(&lights_hist) == Some("home_assistant");
    println!(
        "[{}] history-robust: home ask survives a distracting chat\n  used={:?} reply: {}\n",
        if hist_ok { "PASS" } else { "FAIL" },
        used(&lights_hist),
        lights_hist.text.trim()
    );

    // 2.7 Synth faithful-relay on SUCCESS (the qwen-7b synthesis tail): given a
    //     real home result, the synthesizer must relay its *specifics*, in
    //     English (no CJK bleed), without denying access or asking to check
    //     again. This is the synthesis pass — where the weak synth would leak
    //     Chinese or bluff even though the data is right there.
    let ctx_home_result = ButlerContext {
        capabilities: skills(),
        tool_result: Some(
            "You used the 'home_assistant' skill for your home and it returned: \
             living-room lights ON, front door LOCKED, thermostat 68°F. Relay this to the \
             person in your own words; add nothing that isn't here."
                .to_owned(),
        ),
        now: "Monday, 20 July 2026, 3:00 PM".to_owned(),
        ..ButlerContext::default()
    };
    let home_relay = ask(butler, "are my lights on right now?", &ctx_home_result);
    let hr = home_relay.text.to_lowercase();
    let relays_specifics =
        home_relay.text.contains("68") || hr.contains("locked") || hr.contains("living");
    let synth_denies = hr.contains("don't have")
        || hr.contains("do not have")
        || hr.contains("can't access")
        || hr.contains("cannot access")
        || hr.contains("no access");
    let synth_relay_ok = relays_specifics
        && !has_cjk(&home_relay.text)
        && !synth_denies
        && used(&home_relay).is_none();
    println!(
        "[{}] synth-relay: relays the real home result, in English, no bluff\n  reply: {}\n",
        if synth_relay_ok { "PASS" } else { "FAIL" },
        home_relay.text.trim()
    );

    let total = usize::from(invoke_ok)
        + usize::from(antideny_ok)
        + usize::from(antibluff_ok)
        + usize::from(lang_ok)
        + usize::from(casual_ok)
        + usize::from(hist_ok)
        + usize::from(synth_relay_ok);
    let max = 7;
    println!(
        "=== L2 {label}: {total}/{max}  (invoke {}, anti-deny {}, anti-bluff {}, language {}, conversational {}, history {}, synth-relay {}) ===\n",
        usize::from(invoke_ok),
        usize::from(antideny_ok),
        usize::from(antibluff_ok),
        usize::from(lang_ok),
        usize::from(casual_ok),
        usize::from(hist_ok),
        usize::from(synth_relay_ok)
    );
    (total, max)
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
    let (l1, l1max) = run_level_1(&butler, &model);
    let (l2, l2max) = run_level_2(&butler, &model);
    println!(
        "=== {model}: L1 {l1}/{l1max}, L2 {l2}/{l2max}, TOTAL {}/{} ===\n",
        l1 + l2,
        l1max + l2max
    );
    assert!(
        l1 >= 3,
        "{model} scored L1 {l1}/{l1max}: not viable as an agentic rung-one butler"
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
    let (l1, l1max) = run_level_1(&butler, &label);
    let (l2, l2max) = run_level_2(&butler, &label);
    println!(
        "=== {label}: L1 {l1}/{l1max}, L2 {l2}/{l2max}, TOTAL {}/{} ===\n",
        l1 + l2,
        l1max + l2max
    );
    assert!(l1 >= 3, "{label} scored L1 {l1}/{l1max}: not viable");
}
