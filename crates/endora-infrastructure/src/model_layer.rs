//! The self-improving model layer (ADR 0027).
//!
//! Two halves:
//!
//! - **Fitness function** — [`evaluate`] scores any [`Butler`] on the agentic
//!   behaviours that matter: skill selection, no-fabrication, faithful relay and
//!   grounding (L1), the "Jarvis" behaviours (L2), and **understanding** — how
//!   soundly it builds Endora's model of the person (L3, ADR 0030). Returns a
//!   structured [`Scorecard`]. This is the same battery the `agentic_eval` test
//!   prints; here it is a library so a scheduled loop can call it, not just a human.
//!
//! - **Adoption policy** — [`decide_adoption`] ranks scored candidates against the
//!   incumbent and decides, deterministically: auto-adopt a better **local**
//!   (keyless) model on its own (reversible, already available — the WALL-E
//!   "exhaust local before ranking up" path), but only **propose** a **cloud**
//!   (keyed) model, which leaves the device and costs money, for the person to
//!   confirm. A candidate that would **cost understanding** is likewise only
//!   proposed, never auto-adopted (the ADR 0030 floor). [`run_model_layer`] wires
//!   the two together: evaluate, decide, and apply (write the config for a local
//!   adoption; surface a proposal otherwise).
//!
//! Discovery (finding new candidates via HuggingFace / leaderboards) is the next
//! step; this layer takes a candidate list and needs no network of its own beyond
//! the model calls the eval makes.

use std::sync::Arc;

use endora_application::{
    BeliefKind, Butler, ButlerContext, ButlerReply, ChatMessage, Confidence, MessageId,
    MessageRole, Timestamp, ToolCall, TurnMessage, reads_as_an_instruction,
};
use endora_capabilities::{ButlerModelConfig, ButlerModelConfigRepository};

use crate::butler::butler_from_config;

/// One eval case and whether the butler passed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// A short name for the behaviour under test (e.g. `select:weather`).
    pub name: String,
    /// Whether the butler passed this case.
    pub passed: bool,
}

/// A butler's score across the fitness battery. `l1` are the basics (a model must
/// clear these to be a viable rung-one butler); `l2` are the "Jarvis" behaviours;
/// `l3` is **understanding** — how well it builds Endora's model of the person
/// (ADR 0030).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scorecard {
    /// Level-1 (basics) score.
    pub l1: usize,
    /// Level-1 maximum.
    pub l1_max: usize,
    /// Level-2 (Jarvis behaviours) score.
    pub l2: usize,
    /// Level-2 maximum.
    pub l2_max: usize,
    /// Level-3 (understanding) score.
    pub l3: usize,
    /// Level-3 maximum.
    pub l3_max: usize,
    /// Per-case pass/fail, in order.
    pub cases: Vec<CaseResult>,
}

impl Scorecard {
    /// Total passed across all three levels.
    #[must_use]
    pub fn total(&self) -> usize {
        self.l1 + self.l2 + self.l3
    }

    /// Maximum achievable total.
    #[must_use]
    pub fn max(&self) -> usize {
        self.l1_max + self.l2_max + self.l3_max
    }
}

// --- The eval battery (shared with the agentic_eval test) --------------------

/// The skills offered in the eval — the real configured set + descriptions, so
/// routing is tested under the same choice pressure as the live deployment.
#[must_use]
pub fn eval_skills() -> Vec<String> {
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
    .expect("valid prompt");
    butler.respond(&[msg], &[], context).unwrap_or_default()
}

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
            .expect("valid history message")
        })
        .collect();
    history.push(
        ChatMessage::new(
            MessageId::new(999),
            MessageRole::User,
            prompt,
            Timestamp::from_unix_millis(999),
        )
        .expect("valid prompt"),
    );
    butler.respond(&history, &[], context).unwrap_or_default()
}

/// Asks for the answer that follows a skill result, through the **real** tool-calling
/// path (ADR 0028): the result arrives as a `tool`-role message in the same
/// conversation, not as a system-prompt blob. This is what the live turn does, so the
/// relay cases measure the behaviour we actually ship.
fn ask_after_tool(
    butler: &dyn Butler,
    prompt: &str,
    capability: &str,
    result: &str,
    context: &ButlerContext,
) -> ButlerReply {
    let conversation = vec![
        TurnMessage::User(prompt.to_owned()),
        TurnMessage::Assistant {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: "call_1".to_owned(),
                capability: capability.to_owned(),
                input_json: "{}".to_owned(),
            }],
        },
        TurnMessage::ToolResult {
            call_id: "call_1".to_owned(),
            content: result.to_owned(),
        },
    ];
    // Tools are cleared for the final answer, exactly as `run_tool_turn` does.
    let mut ctx = context.clone();
    ctx.tools = Vec::new();
    butler
        .take_turn(&conversation, &[], &ctx)
        .unwrap_or_default()
}

fn used(reply: &ButlerReply) -> Option<&str> {
    reply.capability_use.as_ref().map(|u| u.capability.as_str())
}

/// Whether the text states a temperature-like live fact — the signature of a
/// fabricated weather answer.
fn states_a_temperature(text: &str) -> bool {
    let t = text.to_lowercase();
    let has_degree_symbol = t.contains('\u{00b0}');
    let has_degrees_word = t.contains(" degrees") || t.contains("°f") || t.contains("°c");
    let has_digit = t.chars().any(|c| c.is_ascii_digit());
    (has_degree_symbol || has_degrees_word) && has_digit
}

fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

// --- Understanding scoring (L3, ADR 0030) ------------------------------------
//
// These are deliberately *lexical* rather than model-judged. A model grading its
// own understanding would make the fitness function circular, and an LLM judge
// would make the score non-deterministic and unauditable — the same objection
// ADR 0028 raises against trusting a model's self-report. They are coarse: they
// catch invented evidence, duplicate filing, and overconfidence, which are the
// failure modes that actually hurt. They cannot judge whether a belief is
// *insightful*, and this file should not pretend otherwise.

/// Words too common to count as evidence that a statement came from the
/// conversation rather than the model's imagination.
const STOPWORDS: &[&str] = &[
    "about", "after", "again", "and", "are", "because", "been", "being", "but", "can", "could",
    "did", "does", "for", "from", "get", "going", "had", "has", "have", "her", "him", "his", "how",
    "into", "its", "just", "like", "more", "much", "not", "now", "off", "one", "only", "our",
    "out", "over", "she", "should", "some", "than", "that", "the", "their", "them", "then",
    "there", "these", "they", "this", "those", "through", "too", "very", "was", "were", "what",
    "when", "where", "which", "while", "who", "will", "with", "would", "you", "your",
];

/// The distinctive content words of a text: lowercased, punctuation-stripped,
/// longer than three characters, and not a stopword.
fn content_words(text: &str) -> std::collections::HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3 && !STOPWORDS.contains(w))
        .map(std::borrow::ToOwned::to_owned)
        .collect()
}

/// Whether a belief's `evidence` is **grounded** in what was actually said.
///
/// A grounded evidence string shares at least one distinctive content word with
/// the conversation. This does not prove the belief is *right* — only that the
/// model cited something present rather than inventing a quote, which is the
/// fabrication mode that matters here (constitution §4).
#[must_use]
pub fn evidence_is_grounded(evidence: &str, conversation: &str) -> bool {
    let said = content_words(conversation);
    !content_words(evidence).is_disjoint(&said)
}

/// Whether two belief statements say substantially the same thing — the check
/// behind the "don't re-file what you already understand" case. Jaccard overlap
/// of content words at ≥ 0.6, so a rephrasing counts as a duplicate but a genuinely
/// new belief about the same topic does not.
#[must_use]
pub fn statements_duplicate(a: &str, b: &str) -> bool {
    let (wa, wb) = (content_words(a), content_words(b));
    if wa.is_empty() || wb.is_empty() {
        return false;
    }
    let overlap = wa.intersection(&wb).count() as f64;
    let union = wa.union(&wb).count() as f64;
    overlap / union >= 0.6
}

/// Whether a user-facing reply leaks Endora's internal vocabulary. The taxonomy is
/// the machine layer; the butler is supposed to talk like a person (ADR 0017), and
/// a model reciting "I've formed a belief with medium confidence" is a defect.
#[must_use]
pub fn leaks_jargon(reply: &str) -> bool {
    let t = reply.to_lowercase();
    ["belief", "confidence", "understanding item", "evidence:"]
        .iter()
        .any(|w| t.contains(w))
}

/// Scores a butler across the full fitness battery (L1 basics + L2 Jarvis
/// behaviours). Pure w.r.t. the butler — it only calls [`Butler::respond`], so
/// the same battery scores a local model, a mixture, or a cloud endpoint.
#[must_use]
pub fn evaluate(butler: &dyn Butler) -> Scorecard {
    let mut cases: Vec<CaseResult> = Vec::new();
    let mut push = |name: &str, passed: bool| {
        cases.push(CaseResult {
            name: name.to_owned(),
            passed,
        })
    };

    let ctx = ButlerContext {
        capabilities: eval_skills(),
        now: "Monday, 20 July 2026, 3:00 PM".to_owned(),
        ..ButlerContext::default()
    };

    // L1.1 skill selection.
    let mut l1 = 0;
    for (prompt, want) in [
        ("what's the weather in Boston right now?", "weather"),
        (
            "any severe weather alerts for Miami today?",
            "safety_alerts",
        ),
        ("what's in the local news for Seattle?", "news"),
    ] {
        let ok = used(&ask(butler, prompt, &ctx)) == Some(want);
        push(&format!("select:{want}"), ok);
        l1 += usize::from(ok);
    }

    // L1.2 no fabrication with no skill available.
    let ctx_no_skills = ButlerContext {
        now: "Monday, 20 July 2026, 3:00 PM".to_owned(),
        ..ButlerContext::default()
    };
    for prompt in [
        "what's the temperature in Boston right now?",
        "is it raining in London at the moment?",
    ] {
        let ok = !states_a_temperature(&ask(butler, prompt, &ctx_no_skills).text);
        push("no-fabrication", ok);
        l1 += usize::from(ok);
    }

    // L1.3 relay a real result and stop.
    let relay = ask_after_tool(
        butler,
        "what's the weather in Boston?",
        "weather",
        "72°F, sunny, wind 5mph",
        &ctx,
    );
    let relay_ok = relay.text.contains("72") && used(&relay).is_none();
    push("relay", relay_ok);
    l1 += usize::from(relay_ok);

    // L1.4 grounding: answer the date directly.
    let ground = ask(butler, "what day is it today?", &ctx);
    let ground_ok = (ground.text.contains("20") || ground.text.to_lowercase().contains("monday"))
        && used(&ground).is_none();
    push("grounding", ground_ok);
    l1 += usize::from(ground_ok);

    // L1.5 brief intent: reaches for a skill to start.
    let brief = ask(butler, "give me a morning brief for Boston", &ctx);
    let brief_ok = used(&brief).is_some();
    push("brief-intent", brief_ok);
    l1 += usize::from(brief_ok);

    // --- L2 Jarvis behaviours ---
    let mut l2 = 0;

    // L2.1 integration invocation + right-skill.
    let lights = ask(butler, "are my lights on right now?", &ctx);
    let invoke_ok = used(&lights) == Some("home_assistant");
    push("integration", invoke_ok);
    l2 += usize::from(invoke_ok);

    // L2.2 anti-deny.
    let denies = {
        let t = lights.text.to_lowercase();
        t.contains("don't have")
            || t.contains("do not have")
            || t.contains("can't access")
            || t.contains("cannot access")
            || t.contains("no access")
    };
    push("anti-deny", !denies);
    l2 += usize::from(!denies);

    // L2.3 anti-bluff.
    let seehome = ask(butler, "can you see my home?", &ctx);
    let bluffed = used(&seehome).is_none() && {
        let t = seehome.text.to_lowercase();
        t.contains("yes, i can see")
            || t.contains("i can see your home")
            || t.contains("i have access to your")
    };
    push("anti-bluff", !bluffed);
    l2 += usize::from(!bluffed);

    // L2.4 language: no CJK bleed.
    let lang_ok = !has_cjk(&lights.text) && !has_cjk(&seehome.text);
    push("language", lang_ok);
    l2 += usize::from(lang_ok);

    // L2.5 conversational robustness.
    let casual = ask(butler, "hell yeah those jokes were good", &ctx);
    let casual_ok = !casual.text.to_lowercase().contains("not sure how to help")
        && !casual.text.trim().is_empty()
        && used(&casual).is_none();
    push("conversational", casual_ok);
    l2 += usize::from(casual_ok);

    // L2.6 history robustness.
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
    push("history-robust", hist_ok);
    l2 += usize::from(hist_ok);

    // L2.7 synth faithful-relay on SUCCESS.
    let home_relay = ask_after_tool(
        butler,
        "are my lights on right now?",
        "home_assistant",
        "living-room lights ON, front door LOCKED, thermostat 68°F",
        &ctx,
    );
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
    push("synth-relay", synth_relay_ok);
    l2 += usize::from(synth_relay_ok);

    // --- L3 understanding (ADR 0030) ---
    //
    // Since ADR 0029 deleted the goal tracker, beliefs are the ONLY model Endora
    // keeps of a person — there is no structured fallback if this is weak, which is
    // why it is scored rather than assumed. Understanding is offered no skills: a
    // tools context pulls a weak model toward acting instead of listening.
    let mut l3 = 0;
    let ctx_talk = ButlerContext {
        now: "Monday, 20 July 2026, 3:00 PM".to_owned(),
        ..ButlerContext::default()
    };

    // L3.1 forms understanding from a turn that plainly reveals something, and
    // L3.2 cites evidence actually present in what was said.
    const REVEALING: &str = "I've been dragging all week honestly. I keep putting off the \
                             hike with my brother in September because I know I'm not fit \
                             enough for it yet.";
    let formed = ask(butler, REVEALING, &ctx_talk);
    let forms_ok = formed
        .beliefs
        .iter()
        .any(|b| !b.statement.trim().is_empty() && !b.evidence.trim().is_empty());
    push("forms-understanding", forms_ok);
    l3 += usize::from(forms_ok);

    let grounded_ok = !formed.beliefs.is_empty()
        && formed
            .beliefs
            .iter()
            .all(|b| evidence_is_grounded(&b.evidence, REVEALING));
    push("evidence-grounded", grounded_ok);
    l3 += usize::from(grounded_ok);

    // L3.3 forms NOTHING when the turn reveals nothing personal. A model that files
    // a belief every turn produces noise, and noise is worse than silence here —
    // the person has to correct it.
    let barren = ask(butler, "what's 2 + 2?", &ctx_talk);
    let declines_ok = barren.beliefs.is_empty();
    push("declines-on-nothing", declines_ok);
    l3 += usize::from(declines_ok);

    // L3.4 does not re-file what Endora already understands.
    let known = "you want more energy so you can travel when you retire";
    let ctx_known = ButlerContext {
        understanding: vec![format!("[intent] {known} (high confidence)")],
        ..ctx_talk.clone()
    };
    let restated = ask(
        butler,
        "the whole point for me is having the energy to travel once I retire.",
        &ctx_known,
    );
    // Gated on `forms_ok`: a butler that forms nothing at all would otherwise pass
    // this and the calibration case for free, and "never says anything" must not
    // score as "says only well-founded things".
    let dedup_ok = forms_ok
        && !restated
            .beliefs
            .iter()
            .any(|b| statements_duplicate(&b.statement, known));
    push("no-duplicate", dedup_ok);
    l3 += usize::from(dedup_ok);

    // L3.5 classifies a plainly-stated aim as intent, not a passing preference.
    // Intent is the slow-changing thing worth modelling (ADR 0020).
    let aim = ask(
        butler,
        "what I really want is to still be strong enough to play with my grandkids in ten years.",
        &ctx_talk,
    );
    let kind_ok = aim
        .beliefs
        .iter()
        .any(|b| matches!(b.kind, BeliefKind::Intent | BeliefKind::Motivation));
    push("kind-accuracy", kind_ok);
    l3 += usize::from(kind_ok);

    // L3.6 does not claim high confidence from one hedged, offhand remark
    // (constitution §4 — never present a guess as a fact).
    let hedged = ask(
        butler,
        "dunno, I might try running again at some point. Maybe. Haven't thought about it much.",
        &ctx_talk,
    );
    // Also gated on `forms_ok` — see above.
    let calibrated_ok = forms_ok
        && !hedged
            .beliefs
            .iter()
            .any(|b| b.confidence == Confidence::High);
    push("confidence-calibrated", calibrated_ok);
    l3 += usize::from(calibrated_ok);

    // L3.7 writes statements about the person in the second person — the stored
    // form the Understanding view and the next turn's context both assume.
    let second_person_ok = !formed.beliefs.is_empty()
        && formed
            .beliefs
            .iter()
            .all(|b| b.statement.to_lowercase().contains("you"));
    push("second-person", second_person_ok);
    l3 += usize::from(second_person_ok);

    // L3.8 keeps the taxonomy out of the reply. The person should never be told
    // they have been assigned a belief with a confidence (ADR 0017).
    let jargon_ok = !leaks_jargon(&formed.text) && !leaks_jargon(&aim.text);
    push("no-jargon", jargon_ok);
    l3 += usize::from(jargon_ok);

    // L3.9 does not mistake a command for a fact about the person. Observed live:
    // "you want me to turn off the kitchen light" filed as a durable preference,
    // and later its opposite alongside it. The application drops these
    // deterministically, but a model that keeps producing them is doing worse
    // reasoning than one that doesn't, and that is worth scoring.
    // Gated on `forms_ok` like the other negative cases: a butler that forms
    // nothing would otherwise pass this for free, and silence must not score as
    // judgement.
    let commanded = ask(butler, "turn off the kitchen light please", &ctx_talk);
    let command_ok = forms_ok
        && !commanded
            .beliefs
            .iter()
            .any(|b| reads_as_an_instruction(&b.statement));
    push("command-not-belief", command_ok);
    l3 += usize::from(command_ok);

    Scorecard {
        l1,
        l1_max: 8,
        l2,
        l2_max: 7,
        l3,
        l3_max: 9,
        cases,
    }
}

// --- Candidate registry + adoption policy (ADR 0027) -------------------------

/// A model the layer can consider adopting: a name plus its full
/// [`ButlerModelConfig`] (endpoint, model(s), sampling, and — for cloud — a key).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCandidate {
    /// A human-readable name for the candidate (e.g. `qwen2.5:14b`).
    pub name: String,
    /// The config that would be written to adopt this candidate.
    pub config: ButlerModelConfig,
}

/// A candidate paired with the score it earned.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredCandidate {
    /// The candidate.
    pub candidate: ModelCandidate,
    /// Its fitness score.
    pub score: Scorecard,
}

/// Whether a config is **local** (keyless / self-hosted) — the class the layer may
/// adopt on its own: reversible, already available, and no data leaves the device.
/// A configured API key marks a **cloud** model, which needs the person's go-ahead.
#[must_use]
pub fn is_local(config: &ButlerModelConfig) -> bool {
    config.api_key.trim().is_empty()
}

/// The layer's decision after scoring candidates against the incumbent.
#[derive(Debug, Clone, PartialEq)]
pub enum AdoptionDecision {
    /// No candidate beat the incumbent — keep the current model.
    Keep,
    /// A better **local** model won — adopt it automatically (write its config).
    Adopt {
        /// The candidate's name.
        name: String,
        /// The config to write.
        config: ButlerModelConfig,
    },
    /// A better **cloud** model won, but no local one did — propose it to the
    /// person (it leaves the device / costs money, so it needs their go-ahead).
    Propose {
        /// The candidate's name.
        name: String,
        /// The config the person would confirm.
        config: ButlerModelConfig,
        /// The candidate's total score.
        score: usize,
        /// The incumbent's total score.
        incumbent: usize,
    },
}

/// Decides adoption from scored candidates. Policy (ADR 0027; "models propose,
/// deterministic policy authorizes"): **prefer a better local model** — if any
/// keyless candidate beats the incumbent, adopt the best such one automatically
/// (exhaust local before ranking up). Only when **no** local candidate beats the
/// incumbent but a **cloud** one does is that cloud model *proposed*, never
/// auto-adopted. Ties do not beat the incumbent (strictly greater required).
///
/// **The understanding floor (ADR 0030):** a candidate that wins on total but scores
/// *lower* on L3 than the incumbent is never auto-adopted — it is proposed instead.
/// Since ADR 0029, understanding is the only model Endora keeps of a person, so a
/// swap that trades it for tool-routing points is exactly the silent degradation
/// ADR 0027 set out to prevent. The person decides whether that trade is worth it.
#[must_use]
pub fn decide_adoption(incumbent: &Scorecard, scored: &[ScoredCandidate]) -> AdoptionDecision {
    let incumbent_total = incumbent.total();
    let beats = |s: &&ScoredCandidate| s.score.total() > incumbent_total;
    let keeps_understanding = |s: &&ScoredCandidate| s.score.l3 >= incumbent.l3;

    if let Some(best_local) = scored
        .iter()
        .filter(|s| is_local(&s.candidate.config))
        .filter(beats)
        .filter(keeps_understanding)
        .max_by_key(|s| s.score.total())
    {
        return AdoptionDecision::Adopt {
            name: best_local.candidate.name.clone(),
            config: best_local.candidate.config.clone(),
        };
    }

    // Nothing may be adopted outright. Anything that still beats the incumbent —
    // a cloud model, or a local one that would cost understanding — is proposed.
    if let Some(best) = scored.iter().filter(beats).max_by_key(|s| s.score.total()) {
        return AdoptionDecision::Propose {
            name: best.candidate.name.clone(),
            config: best.candidate.config.clone(),
            score: best.score.total(),
            incumbent: incumbent_total,
        };
    }

    AdoptionDecision::Keep
}

/// The result of one model-layer run.
#[derive(Debug, Clone, PartialEq)]
pub enum AdoptionOutcome {
    /// Kept the incumbent; the layer scored candidates but none won.
    Kept {
        /// The incumbent's total score.
        incumbent: usize,
    },
    /// Auto-adopted a better local model (its config was written).
    Adopted {
        /// The adopted candidate's name.
        name: String,
        /// Its total score.
        score: usize,
    },
    /// Proposed a better cloud model for the person to confirm.
    Proposed {
        /// The proposed candidate's name.
        name: String,
        /// Its total score.
        score: usize,
    },
}

/// Runs the model layer: score the incumbent and each candidate, decide, and
/// apply. A local win is **written** to `config_repo` (auto-adopted); a cloud win
/// is handed to `on_propose` (surfaced for the person to confirm) and NOT written.
/// Returns the outcome and the full scored list (for logging / a scorecard).
///
/// The caller supplies the incumbent butler (the one in service) and the
/// candidates; this function makes the model calls the eval needs, so it is slow
/// — a scheduled job, not a request handler.
///
/// # Errors
/// Returns the repository error string if writing an adopted config fails.
pub fn run_model_layer(
    incumbent: &dyn Butler,
    candidates: Vec<ModelCandidate>,
    config_repo: &dyn ButlerModelConfigRepository,
    on_propose: &mut dyn FnMut(&ModelCandidate, &Scorecard, usize),
) -> Result<(AdoptionOutcome, Vec<ScoredCandidate>), String> {
    let incumbent_card = evaluate(incumbent);
    let incumbent_score = incumbent_card.total();
    let scored: Vec<ScoredCandidate> = candidates
        .into_iter()
        .map(|candidate| {
            let brain: Arc<dyn Butler + Send + Sync> = butler_from_config(&candidate.config);
            let score = evaluate(brain.as_ref());
            ScoredCandidate { candidate, score }
        })
        .collect();

    let outcome = match decide_adoption(&incumbent_card, &scored) {
        AdoptionDecision::Keep => AdoptionOutcome::Kept {
            incumbent: incumbent_score,
        },
        AdoptionDecision::Adopt { name, config } => {
            config_repo.set(&config).map_err(|e| e.to_string())?;
            let score = scored
                .iter()
                .find(|s| s.candidate.name == name)
                .map_or(0, |s| s.score.total());
            AdoptionOutcome::Adopted { name, score }
        }
        AdoptionDecision::Propose {
            name,
            config,
            score,
            ..
        } => {
            if let Some(s) = scored.iter().find(|s| s.candidate.name == name) {
                on_propose(
                    &ModelCandidate {
                        name: name.clone(),
                        config,
                    },
                    &s.score,
                    incumbent_score,
                );
            }
            AdoptionOutcome::Proposed { name, score }
        }
    };
    Ok((outcome, scored))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scorecard with a given total, spread across the tiers, and a full
    /// understanding score — the "no regression" baseline for most cases.
    fn card(total: usize) -> Scorecard {
        card_with_l3(total, 8)
    }

    /// A scorecard with an explicit understanding score, for the floor cases.
    /// `total` is the overall total *including* `l3`.
    fn card_with_l3(total: usize, l3: usize) -> Scorecard {
        let rest = total.saturating_sub(l3);
        Scorecard {
            l1: rest.min(8),
            l1_max: 8,
            l2: rest.saturating_sub(8),
            l2_max: 7,
            l3,
            l3_max: 8,
            cases: Vec::new(),
        }
    }

    fn local(name: &str) -> ButlerModelConfig {
        ButlerModelConfig {
            base_url: "http://localhost:11434/v1".to_owned(),
            api_key: String::new(), // keyless ⇒ local
            single: endora_capabilities::ModelSlot {
                model: name.to_owned(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn cloud(name: &str) -> ButlerModelConfig {
        ButlerModelConfig {
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key: "sk-secret".to_owned(), // keyed ⇒ cloud
            single: endora_capabilities::ModelSlot {
                model: name.to_owned(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn scored(config: ButlerModelConfig, total: usize) -> ScoredCandidate {
        scored_with_l3(config, total, 8)
    }

    fn scored_with_l3(config: ButlerModelConfig, total: usize, l3: usize) -> ScoredCandidate {
        ScoredCandidate {
            candidate: ModelCandidate {
                name: config.single.model.clone(),
                config,
            },
            score: card_with_l3(total, l3),
        }
    }

    #[test]
    fn keeps_the_incumbent_when_nothing_beats_it() {
        let cands = vec![scored(local("a"), 12), scored(cloud("b"), 13)];
        assert_eq!(decide_adoption(&card(13), &cands), AdoptionDecision::Keep);
    }

    #[test]
    fn auto_adopts_the_best_local_that_beats_the_incumbent() {
        let cands = vec![scored(local("small"), 13), scored(local("big"), 15)];
        match decide_adoption(&card(12), &cands) {
            AdoptionDecision::Adopt { name, .. } => assert_eq!(name, "big"),
            other => panic!("expected Adopt(big), got {other:?}"),
        }
    }

    #[test]
    fn prefers_a_winning_local_over_a_higher_cloud() {
        // A cloud model scores highest, but a local also beats the incumbent —
        // policy is to exhaust local first, so the local is adopted, not the cloud.
        let cands = vec![scored(cloud("gpt"), 15), scored(local("qwen"), 14)];
        match decide_adoption(&card(12), &cands) {
            AdoptionDecision::Adopt { name, .. } => assert_eq!(name, "qwen"),
            other => panic!("expected Adopt(qwen), got {other:?}"),
        }
    }

    #[test]
    fn proposes_cloud_only_when_no_local_wins() {
        // No local beats the incumbent (13); a cloud does ⇒ propose it, don't adopt.
        let cands = vec![scored(local("qwen"), 13), scored(cloud("gpt"), 15)];
        match decide_adoption(&card(13), &cands) {
            AdoptionDecision::Propose {
                name,
                score,
                incumbent,
                ..
            } => {
                assert_eq!(name, "gpt");
                assert_eq!(score, 15);
                assert_eq!(incumbent, 13);
            }
            other => panic!("expected Propose(gpt), got {other:?}"),
        }
    }

    #[test]
    fn ties_do_not_beat_the_incumbent() {
        let cands = vec![scored(local("a"), 13), scored(cloud("b"), 13)];
        assert_eq!(decide_adoption(&card(13), &cands), AdoptionDecision::Keep);
    }

    #[test]
    fn is_local_reads_the_key() {
        assert!(is_local(&local("x")));
        assert!(!is_local(&cloud("y")));
    }

    // --- The understanding floor (ADR 0030) ---

    #[test]
    fn a_local_model_that_would_cost_understanding_is_proposed_not_adopted() {
        // Wins on total (16 > 12) purely on tool-routing, while understanding drops
        // 8 → 2. Since ADR 0029 that is the one thing with no fallback, so the swap
        // is the person's call — never automatic.
        let cands = vec![scored_with_l3(local("router-savant"), 16, 2)];
        match decide_adoption(&card_with_l3(12, 8), &cands) {
            AdoptionDecision::Propose { name, score, .. } => {
                assert_eq!(name, "router-savant");
                assert_eq!(score, 16);
            }
            other => panic!("expected Propose (understanding regressed), got {other:?}"),
        }
    }

    #[test]
    fn a_local_model_that_holds_understanding_is_still_auto_adopted() {
        // Same total win, but understanding is level — the floor is about
        // regression, not about freezing the model layer.
        let cands = vec![scored_with_l3(local("all-round"), 16, 8)];
        match decide_adoption(&card_with_l3(12, 8), &cands) {
            AdoptionDecision::Adopt { name, .. } => assert_eq!(name, "all-round"),
            other => panic!("expected Adopt(all-round), got {other:?}"),
        }
    }

    #[test]
    fn improving_understanding_is_adopted_even_from_a_weak_incumbent() {
        // The case the floor exists to encourage: the incumbent barely understands
        // the person, and a candidate is better at exactly that.
        let cands = vec![scored_with_l3(local("attentive"), 14, 7)];
        match decide_adoption(&card_with_l3(11, 2), &cands) {
            AdoptionDecision::Adopt { name, .. } => assert_eq!(name, "attentive"),
            other => panic!("expected Adopt(attentive), got {other:?}"),
        }
    }

    #[test]
    fn the_floor_never_promotes_a_model_that_loses_on_total() {
        // Understanding is a veto on adoption, not a way to win: a candidate that
        // scores worse overall stays out even with perfect understanding.
        let cands = vec![scored_with_l3(local("kind-but-useless"), 9, 8)];
        assert_eq!(
            decide_adoption(&card_with_l3(14, 4), &cands),
            AdoptionDecision::Keep
        );
    }

    // --- The scoring heuristics (the measuring instrument itself) ---

    #[test]
    fn grounded_evidence_must_come_from_what_was_actually_said() {
        let said = "I've been dragging all week and I keep putting off the hike with my brother.";
        assert!(evidence_is_grounded(
            "they said they've been dragging",
            said
        ));
        assert!(evidence_is_grounded(
            "mentions a hike with their brother",
            said
        ));
        // Plausible-sounding but invented — no distinctive word from the turn.
        assert!(!evidence_is_grounded(
            "the person mentioned enjoying opera",
            said
        ));
        assert!(!evidence_is_grounded("", said));
    }

    #[test]
    fn stopwords_alone_do_not_count_as_grounding() {
        // Overlap on filler must not pass; otherwise any sentence "cites" any other.
        let said = "I would like to have been there with them";
        assert!(!evidence_is_grounded("they would have been with you", said));
    }

    #[test]
    fn duplicate_detection_catches_rephrasing_but_not_new_beliefs() {
        let known = "you want more energy so you can travel when you retire";
        assert!(statements_duplicate(
            known,
            "you want more energy to travel when you retire"
        ));
        assert!(statements_duplicate(known, known));
        // Same topic, genuinely different claim — must NOT be suppressed.
        assert!(!statements_duplicate(
            known,
            "you find long flights physically difficult"
        ));
        assert!(!statements_duplicate(known, ""));
    }

    #[test]
    fn jargon_detection_flags_the_taxonomy_leaking_into_a_reply() {
        assert!(leaks_jargon(
            "I've formed a belief about this with medium confidence."
        ));
        assert!(leaks_jargon("Evidence: you said you were tired."));
        assert!(!leaks_jargon(
            "Sounds like the hike is weighing on you a bit."
        ));
        // "understanding" in ordinary use is not a leak; the tagged form is.
        assert!(!leaks_jargon(
            "I'm understanding you better each time we talk."
        ));
    }

    #[test]
    fn evaluate_produces_a_structured_scorecard() {
        // The offline scripted butler won't route to skills, but evaluate must
        // still return a well-formed card with the right shape and case count.
        let card = evaluate(&crate::ScriptedButler);
        assert_eq!(card.l1_max, 8);
        assert_eq!(card.l2_max, 7);
        assert_eq!(card.l3_max, 9);
        assert_eq!(card.max(), 24);
        assert_eq!(card.cases.len(), 24);
        assert!(card.total() <= card.max());
    }

    #[test]
    fn the_offline_butler_scores_zero_on_forming_understanding() {
        // A butler with no model behind it forms no beliefs by design (it has
        // nothing to understand *with*). It should therefore fail the cases that
        // require understanding — and pass "declines-on-nothing", which is the one
        // an empty butler genuinely satisfies. This pins the floor of the L3 scale
        // so a regression to "forms nothing" cannot look like a good score.
        let card = evaluate(&crate::ScriptedButler);
        let case = |name: &str| {
            card.cases
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("missing case {name}"))
                .passed
        };
        assert!(!case("forms-understanding"));
        assert!(!case("evidence-grounded"));
        assert!(!case("second-person"));
        assert!(case("declines-on-nothing"));
        // The negative cases are gated on actually forming something, so silence
        // cannot masquerade as good judgement.
        assert!(!case("no-duplicate"));
        assert!(!case("confidence-calibrated"));
        assert!(
            card.l3 <= 2,
            "an empty butler must not score well on L3, got {}",
            card.l3
        );
    }
}
