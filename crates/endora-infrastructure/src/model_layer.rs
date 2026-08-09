//! The self-improving model layer (ADR 0055).
//!
//! Two halves:
//!
//! - **Fitness function** — [`evaluate`] scores any [`Butler`] on the agentic
//!   behaviours that matter: skill selection, no-fabrication, faithful relay and
//!   grounding (L1), the "Jarvis" behaviours (L2), and **understanding** — how
//!   soundly it builds Endora's model of the person (L3, ADR 0055). Returns a
//!   structured [`Scorecard`]. This is the same battery the `agentic_eval` test
//!   prints; here it is a library so a scheduled loop can call it, not just a human.
//!
//! - **Adoption policy** — [`decide_adoption`] ranks scored candidates against the
//!   incumbent and decides, deterministically: auto-adopt a better **local**
//!   (keyless) model on its own (reversible, already available — the WALL-E
//!   "exhaust local before ranking up" path), but only **propose** a **cloud**
//!   (keyed) model, which leaves the device and costs money, for the person to
//!   confirm. A candidate that would **cost understanding** is likewise only
//!   proposed, never auto-adopted (the ADR 0055 floor). [`run_model_layer`] wires
//!   the two together: evaluate, decide, and apply (write the config for a local
//!   adoption; surface a proposal otherwise).
//!
//! Discovery (finding new candidates via HuggingFace / leaderboards) is the next
//! step; this layer takes a candidate list and needs no network of its own beyond
//! the model calls the eval makes.

use std::sync::Arc;

use endora_application::Butler;
use endora_capabilities::{ButlerModelConfig, ButlerModelConfigRepository};

use crate::butler::butler_from_config;

// The fitness battery lives in `crate::eval` — the cases, the probes, and the
// scoring. Re-exported here so `model_layer::{evaluate, Scorecard, …}` paths hold
// and this module can stay about the *adoption policy*.
pub use crate::eval::{
    CaseRate, CaseResult, RepeatedScore, Scorecard, eval_skills, evaluate, evaluate_repeated,
    evidence_is_grounded, leaks_jargon, statements_duplicate,
};

// --- Candidate registry + adoption policy (ADR 0055) -------------------------

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
    /// No candidate cleared the bar — keep the current model, and say why.
    Keep {
        /// What stopped a swap, so a quiet run reads differently from a near miss.
        why: Held,
    },
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
        /// Which floor stopped it being adopted outright.
        held_by: Held,
    },
}

/// Decides adoption from scored candidates. Policy (ADR 0055; "models propose,
/// deterministic policy authorizes"): **prefer a better local model** — if any
/// keyless candidate beats the incumbent, adopt the best such one automatically
/// (exhaust local before ranking up). Only when **no** local candidate beats the
/// incumbent but a **cloud** one does is that cloud model *proposed*, never
/// auto-adopted. Ties do not beat the incumbent (strictly greater required).
///
/// **A win smaller than the instrument's noise is not a win.** The layer scores each
/// candidate once, and a single run of this battery is not repeatable to a point: run
/// three times, the same model lands in a range of one to two. Adopting on "strictly
/// greater" therefore adopts on noise — and did, live: it switched the butler to
/// `qwen2.5:14b` on 35 against 34, a model measured at **1.8x slower** for a difference
/// that vanishes on a second run. A margin is the cheapest honest fix; the alternative,
/// repeating every candidate's battery on the heartbeat, costs far more than it settles.
///
/// **The speed floor:** a candidate that wins on total but is **materially slower** is
/// never auto-adopted either. Score says nothing about how long a turn takes, and latency
/// is the one property of a model the person feels on every single turn — the swap that
/// prompted this was 1.8x slower for a point. Same shape as the understanding floor, and
/// for the same reason: it is a *trade*, and a trade is the person's to make.
///
/// **The understanding floor (ADR 0055):** a candidate that wins on total but scores
/// *lower* on L3 than the incumbent is never auto-adopted — it is proposed instead.
/// Since ADR 0052, understanding is the only model Endora keeps of a person, so a
/// swap that trades it for tool-routing points is exactly the silent degradation
/// ADR 0055 set out to prevent. The person decides whether that trade is worth it.
#[must_use]
pub fn decide_adoption(incumbent: &Scorecard, scored: &[ScoredCandidate]) -> AdoptionDecision {
    let incumbent_total = incumbent.total();
    let beats = |s: &&ScoredCandidate| s.score.total() >= incumbent_total + CLEARLY_BETTER;
    let keeps_understanding = |s: &&ScoredCandidate| s.score.l3 >= incumbent.l3;
    let stays_quick = |s: &&ScoredCandidate| !much_slower_than(&s.score, incumbent);

    if let Some(best_local) = scored
        .iter()
        .filter(|s| is_local(&s.candidate.config))
        .filter(beats)
        .filter(keeps_understanding)
        .filter(stays_quick)
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
        // Which floor stopped it, checked in the order the filters above apply.
        let held_by = if !is_local(&best.candidate.config) {
            Held::NeedsAKey
        } else if !keeps_understanding(&best) {
            Held::WouldCostUnderstanding
        } else {
            Held::WouldBeSlower
        };
        return AdoptionDecision::Propose {
            name: best.candidate.name.clone(),
            config: best.candidate.config.clone(),
            score: best.score.total(),
            incumbent: incumbent_total,
            held_by,
        };
    }

    // Nothing cleared the margin. Say whether anything even came close, so a quiet run is
    // distinguishable from a run that found a near-miss.
    let anything_higher = scored.iter().any(|s| s.score.total() > incumbent_total);
    AdoptionDecision::Keep {
        why: if anything_higher {
            Held::InsideTheNoise
        } else {
            Held::NothingBetter
        },
    }
}

/// Whether a candidate takes long enough more than the incumbent to be felt.
///
/// The battery is identical work for every candidate, so its elapsed time compares
/// directly. A quarter again as long is the threshold: below that it is within the noise
/// of a shared machine, and above it every turn of every day is noticeably slower.
///
/// A missing timing — an older scorecard, or a stubbed one in a test — is treated as *not*
/// slower, so this can only ever hold a swap back on evidence, never on an absence of it.
fn much_slower_than(candidate: &Scorecard, incumbent: &Scorecard) -> bool {
    if candidate.took_ms == 0 || incumbent.took_ms == 0 {
        return false;
    }
    candidate.took_ms * 4 > incumbent.took_ms * 5
}

/// How much better a candidate must score before it is worth swapping to.
///
/// Three, because the battery's own spread across repeated runs is one to two: anything
/// inside that is the instrument moving, not the model. Deliberately a plain constant —
/// it is a statement about the measurement, and it should be re-read whenever the battery
/// changes shape.
const CLEARLY_BETTER: usize = 3;

/// Why the layer did not simply swap to the best-scoring candidate.
///
/// The layer has three floors now — margin, understanding, speed — and until this existed
/// it reported only its *outcome*. "Kept" told the person nothing about whether anything
/// had come close, which floor stopped it, or whether the run had found anything at all.
/// A judgement nobody can inspect is indistinguishable from one nobody made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Held {
    /// Nothing scored higher than what is already running.
    NothingBetter,
    /// Something scored higher, but by less than the battery moves between runs.
    InsideTheNoise,
    /// It wins overall and understands the person less well (ADR 0052's floor).
    WouldCostUnderstanding,
    /// It wins overall and is materially slower — felt on every turn.
    WouldBeSlower,
    /// Only a model needing a key won, and those are never adopted outright.
    NeedsAKey,
}

impl Held {
    /// The reason in the person's words, for the activity trail.
    #[must_use]
    pub const fn as_words(self) -> &'static str {
        match self {
            Self::NothingBetter => "nothing scored better",
            Self::InsideTheNoise => "the difference is inside the battery's own spread",
            Self::WouldCostUnderstanding => "it understands you less well",
            Self::WouldBeSlower => "it is noticeably slower",
            Self::NeedsAKey => "it needs a key, so it is yours to approve",
        }
    }
}

/// The result of one model-layer run.
#[derive(Debug, Clone, PartialEq)]
pub enum AdoptionOutcome {
    /// Kept the incumbent; the layer scored candidates and none cleared the bar.
    Kept {
        /// The incumbent's total score.
        incumbent: usize,
        /// Why nothing was swapped in.
        why: Held,
    },
    /// Auto-adopted a better local model (its config was written).
    Adopted {
        /// The adopted candidate's name.
        name: String,
        /// Its total score.
        score: usize,
    },
    /// Proposed a better model for the person to confirm — it won, and a floor stopped
    /// it being taken up automatically.
    Proposed {
        /// The proposed candidate's name.
        name: String,
        /// Its total score.
        score: usize,
        /// Which floor stopped it being adopted outright.
        held_by: Held,
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
        AdoptionDecision::Keep { why } => AdoptionOutcome::Kept {
            why,
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
            held_by,
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
            AdoptionOutcome::Proposed {
                name,
                score,
                held_by,
            }
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
            took_ms: 0,
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
        assert_eq!(
            decide_adoption(&card(13), &cands),
            AdoptionDecision::Keep {
                why: Held::NothingBetter
            }
        );
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
        // Both clear the margin; the point of the case is the ORDER, not the threshold.
        let cands = vec![scored(cloud("gpt"), 20), scored(local("qwen"), 18)];
        match decide_adoption(&card(12), &cands) {
            AdoptionDecision::Adopt { name, .. } => assert_eq!(name, "qwen"),
            other => panic!("expected Adopt(qwen), got {other:?}"),
        }
    }

    #[test]
    fn proposes_cloud_only_when_no_local_wins() {
        // No local clears the margin over the incumbent (13); a cloud does ⇒ propose it.
        let cands = vec![scored(local("qwen"), 13), scored(cloud("gpt"), 18)];
        match decide_adoption(&card(13), &cands) {
            AdoptionDecision::Propose {
                name,
                score,
                incumbent,
                ..
            } => {
                assert_eq!(name, "gpt");
                assert_eq!(score, 18);
                assert_eq!(incumbent, 13);
            }
            other => panic!("expected Propose(gpt), got {other:?}"),
        }
    }

    #[test]
    fn a_win_inside_the_instruments_noise_is_not_a_win() {
        // Live, and it swapped the person's butler unasked: 35 against 34 on a single run
        // of a battery whose own spread across repeated runs is one to two. The adopted
        // model was measured 1.8x slower for a difference that vanishes on a re-run.
        let cands = vec![scored(local("qwen2.5:14b"), 35)];
        assert_eq!(
            decide_adoption(&card(34), &cands),
            AdoptionDecision::Keep {
                why: Held::InsideTheNoise
            },
            "adopted on noise"
        );
        // Two points is still inside the spread.
        assert_eq!(
            decide_adoption(&card(33), &cands),
            AdoptionDecision::Keep {
                why: Held::InsideTheNoise
            }
        );
        // Three is the point at which the difference outlives a re-run.
        match decide_adoption(&card(32), &cands) {
            AdoptionDecision::Adopt { name, .. } => assert_eq!(name, "qwen2.5:14b"),
            other => panic!("a clear win should be adopted, got {other:?}"),
        }
    }

    #[test]
    fn ties_do_not_beat_the_incumbent() {
        let cands = vec![scored(local("a"), 13), scored(cloud("b"), 13)];
        assert_eq!(
            decide_adoption(&card(13), &cands),
            AdoptionDecision::Keep {
                why: Held::NothingBetter
            }
        );
    }

    #[test]
    fn is_local_reads_the_key() {
        assert!(is_local(&local("x")));
        assert!(!is_local(&cloud("y")));
    }

    // --- The understanding floor (ADR 0055) ---

    #[test]
    fn a_local_model_that_would_cost_understanding_is_proposed_not_adopted() {
        // Wins on total (16 > 12) purely on tool-routing, while understanding drops
        // 8 → 2. Since ADR 0052 that is the one thing with no fallback, so the swap
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
            AdoptionDecision::Keep {
                why: Held::NothingBetter
            }
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

    /// A scorecard that also says how long the battery took.
    fn card_taking(total: usize, took_ms: u64) -> Scorecard {
        Scorecard {
            took_ms,
            ..card(total)
        }
    }

    #[test]
    fn a_clearly_better_but_much_slower_model_is_proposed_not_adopted() {
        // The measured case: qwen2.5:14b is genuinely a shade better on some cases and
        // 1.8x slower on every turn of every day. Score alone cannot see that, and the
        // trade is the person's to make.
        let mut slow = scored(local("qwen2.5:14b"), 40);
        slow.score.took_ms = 480_000;
        match decide_adoption(&card_taking(32, 260_000), &[slow]) {
            AdoptionDecision::Propose { name, .. } => assert_eq!(name, "qwen2.5:14b"),
            other => panic!("expected it to be proposed, got {other:?}"),
        }
    }

    #[test]
    fn a_clearly_better_model_that_keeps_up_is_adopted() {
        let mut quick = scored(local("qwen3:8b"), 40);
        quick.score.took_ms = 270_000;
        match decide_adoption(&card_taking(32, 260_000), &[quick]) {
            AdoptionDecision::Adopt { name, .. } => assert_eq!(name, "qwen3:8b"),
            other => panic!("expected it to be adopted, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_timing_never_holds_a_swap_back() {
        // An older scorecard carries no timing. Absence of evidence must not read as
        // evidence of slowness, or the floor would block every swap it cannot measure.
        let unknown = scored(local("qwen3:8b"), 40);
        match decide_adoption(&card_taking(32, 260_000), &[unknown]) {
            AdoptionDecision::Adopt { name, .. } => assert_eq!(name, "qwen3:8b"),
            other => panic!("expected it to be adopted, got {other:?}"),
        }
    }

    #[test]
    fn a_kept_run_says_whether_anything_came_close() {
        // "Kept" on its own is indistinguishable from a run that found nothing and one
        // that found a near miss. The person deserves to know which.
        let near = vec![scored(local("qwen3:8b"), 35)];
        assert_eq!(
            decide_adoption(&card(34), &near),
            AdoptionDecision::Keep {
                why: Held::InsideTheNoise
            },
            "a near miss should say so"
        );
        let nothing = vec![scored(local("qwen3:8b"), 30)];
        assert_eq!(
            decide_adoption(&card(34), &nothing),
            AdoptionDecision::Keep {
                why: Held::NothingBetter
            }
        );
    }

    #[test]
    fn a_proposal_names_the_floor_that_stopped_it() {
        // Three floors, three different things to tell the person — and each is a trade
        // only they can weigh.
        let mut slow = scored(local("slowcoach"), 40);
        slow.score.took_ms = 480_000;
        match decide_adoption(&card_taking(32, 260_000), &[slow]) {
            AdoptionDecision::Propose { held_by, .. } => {
                assert_eq!(held_by, Held::WouldBeSlower);
            }
            other => panic!("expected a proposal, got {other:?}"),
        }

        let shallow = vec![scored_with_l3(local("forgetful"), 40, 2)];
        match decide_adoption(&card_with_l3(30, 8), &shallow) {
            AdoptionDecision::Propose { held_by, .. } => {
                assert_eq!(held_by, Held::WouldCostUnderstanding);
            }
            other => panic!("expected a proposal, got {other:?}"),
        }

        let paid = vec![scored(cloud("gpt"), 40)];
        match decide_adoption(&card(30), &paid) {
            AdoptionDecision::Propose { held_by, .. } => assert_eq!(held_by, Held::NeedsAKey),
            other => panic!("expected a proposal, got {other:?}"),
        }
    }
}
