//! The self-improving model layer (ADR 0027).
//!
//! Two halves:
//!
//! - **Fitness function** — [`evaluate`] scores any [`Butler`] on the agentic
//!   behaviours that matter (skill selection, no-fabrication, faithful relay,
//!   grounding, and the L2 "Jarvis" behaviours), returning a structured
//!   [`Scorecard`]. This is the same battery the `agentic_eval` test prints; here
//!   it is a library so a scheduled loop can call it, not just a human.
//!
//! - **Adoption policy** — [`decide_adoption`] ranks scored candidates against the
//!   incumbent and decides, deterministically: auto-adopt a better **local**
//!   (keyless) model on its own (reversible, already available — the WALL-E
//!   "exhaust local before ranking up" path), but only **propose** a **cloud**
//!   (keyed) model, which leaves the device and costs money, for the person to
//!   confirm. [`run_model_layer`] wires the two together: evaluate, decide, and
//!   apply (write the config for a local adoption; surface a proposal for cloud).
//!
//! Discovery (finding new candidates via HuggingFace / leaderboards) is the next
//! step; this layer takes a candidate list and needs no network of its own beyond
//! the model calls the eval makes.

use std::sync::Arc;

use endora_application::{
    Butler, ButlerContext, ButlerReply, ChatMessage, MessageId, MessageRole, Timestamp,
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
/// clear these to be a viable rung-one butler); `l2` are the "Jarvis" behaviours.
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
    /// Per-case pass/fail, in order.
    pub cases: Vec<CaseResult>,
}

impl Scorecard {
    /// Total passed across both levels.
    #[must_use]
    pub fn total(&self) -> usize {
        self.l1 + self.l2
    }

    /// Maximum achievable total.
    #[must_use]
    pub fn max(&self) -> usize {
        self.l1_max + self.l2_max
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
    let ctx_result = ButlerContext {
        capabilities: eval_skills(),
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
    let ctx_home_result = ButlerContext {
        capabilities: eval_skills(),
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
    push("synth-relay", synth_relay_ok);
    l2 += usize::from(synth_relay_ok);

    Scorecard {
        l1,
        l1_max: 8,
        l2,
        l2_max: 7,
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
#[must_use]
pub fn decide_adoption(incumbent: usize, scored: &[ScoredCandidate]) -> AdoptionDecision {
    let beats = |s: &&ScoredCandidate| s.score.total() > incumbent;

    if let Some(best_local) = scored
        .iter()
        .filter(|s| is_local(&s.candidate.config))
        .filter(beats)
        .max_by_key(|s| s.score.total())
    {
        return AdoptionDecision::Adopt {
            name: best_local.candidate.name.clone(),
            config: best_local.candidate.config.clone(),
        };
    }

    if let Some(best_cloud) = scored
        .iter()
        .filter(|s| !is_local(&s.candidate.config))
        .filter(beats)
        .max_by_key(|s| s.score.total())
    {
        return AdoptionDecision::Propose {
            name: best_cloud.candidate.name.clone(),
            config: best_cloud.candidate.config.clone(),
            score: best_cloud.score.total(),
            incumbent,
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
    let incumbent_score = evaluate(incumbent).total();
    let scored: Vec<ScoredCandidate> = candidates
        .into_iter()
        .map(|candidate| {
            let brain: Arc<dyn Butler + Send + Sync> = butler_from_config(&candidate.config);
            let score = evaluate(brain.as_ref());
            ScoredCandidate { candidate, score }
        })
        .collect();

    let outcome = match decide_adoption(incumbent_score, &scored) {
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

    fn card(total: usize) -> Scorecard {
        Scorecard {
            l1: total.min(8),
            l1_max: 8,
            l2: total.saturating_sub(8),
            l2_max: 7,
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
        ScoredCandidate {
            candidate: ModelCandidate {
                name: config.single.model.clone(),
                config,
            },
            score: card(total),
        }
    }

    #[test]
    fn keeps_the_incumbent_when_nothing_beats_it() {
        let cands = vec![scored(local("a"), 12), scored(cloud("b"), 13)];
        assert_eq!(decide_adoption(13, &cands), AdoptionDecision::Keep);
    }

    #[test]
    fn auto_adopts_the_best_local_that_beats_the_incumbent() {
        let cands = vec![scored(local("small"), 13), scored(local("big"), 15)];
        match decide_adoption(12, &cands) {
            AdoptionDecision::Adopt { name, .. } => assert_eq!(name, "big"),
            other => panic!("expected Adopt(big), got {other:?}"),
        }
    }

    #[test]
    fn prefers_a_winning_local_over_a_higher_cloud() {
        // A cloud model scores highest, but a local also beats the incumbent —
        // policy is to exhaust local first, so the local is adopted, not the cloud.
        let cands = vec![scored(cloud("gpt"), 15), scored(local("qwen"), 14)];
        match decide_adoption(12, &cands) {
            AdoptionDecision::Adopt { name, .. } => assert_eq!(name, "qwen"),
            other => panic!("expected Adopt(qwen), got {other:?}"),
        }
    }

    #[test]
    fn proposes_cloud_only_when_no_local_wins() {
        // No local beats the incumbent (13); a cloud does ⇒ propose it, don't adopt.
        let cands = vec![scored(local("qwen"), 13), scored(cloud("gpt"), 15)];
        match decide_adoption(13, &cands) {
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
        assert_eq!(decide_adoption(13, &cands), AdoptionDecision::Keep);
    }

    #[test]
    fn is_local_reads_the_key() {
        assert!(is_local(&local("x")));
        assert!(!is_local(&cloud("y")));
    }

    #[test]
    fn evaluate_produces_a_structured_scorecard() {
        // The offline scripted butler won't route to skills, but evaluate must
        // still return a well-formed card with the right shape and case count.
        let card = evaluate(&crate::ScriptedButler);
        assert_eq!(card.l1_max, 8);
        assert_eq!(card.l2_max, 7);
        assert_eq!(card.max(), 15);
        assert_eq!(card.cases.len(), 15);
        assert!(card.total() <= card.max());
    }
}
