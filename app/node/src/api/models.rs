use super::*;

pub(super) fn brief_schedule_json(s: &BriefSchedule) -> serde_json::Value {
    json!({ "enabled": s.enabled, "hour_utc": s.hour_utc })
}

/// The deep-model config (a bigger AI for hard questions). The key is NEVER
/// returned — only whether one is set.
pub(super) async fn get_deep_model(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let cfg =
        blocking(move || DeepModelRepository::get(config.as_ref()).map_err(AppError::Repository))
            .await?
            .unwrap_or_default();
    Ok(Json(json!({
        "url": cfg.url,
        "model": cfg.model,
        "configured": !cfg.url.is_empty() && !cfg.model.is_empty(),
        "key_set": !cfg.api_key.is_empty(),
        "escalate": cfg.escalate,
    })))
}

#[derive(Deserialize)]
pub(super) struct DeepModelRequest {
    url: String,
    model: String,
    #[serde(default)]
    api_key: Option<String>,
    /// Whether Endora may fall back to it on its own. Absent means "leave as it is", so a
    /// client that only edits the URL cannot silently turn phoning-out on or off.
    #[serde(default)]
    escalate: Option<bool>,
}

/// Configures the deep model. A blank/omitted `api_key` keeps any existing key, so
/// the secret is never round-tripped through the client.
pub(super) async fn set_deep_model(
    State(state): State<AppState>,
    Json(req): Json<DeepModelRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    blocking(move || {
        let existing = DeepModelRepository::get(config.as_ref())
            .map_err(AppError::Repository)?
            .unwrap_or_default();
        let api_key = match req.api_key {
            Some(k) if !k.trim().is_empty() => k.trim().to_owned(),
            _ => existing.api_key, // keep the stored key when none is supplied
        };
        DeepModelRepository::set(
            config.as_ref(),
            &endora_application::DeepModel {
                url: req.url.trim().to_owned(),
                model: req.model.trim().to_owned(),
                api_key,
                escalate: req.escalate.unwrap_or(existing.escalate),
            },
        )
        .map_err(AppError::Repository)?;
        record_event(events.as_ref(), clock.as_ref(), "Configured the deep model");
        Ok::<_, AppError>(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

/// Renders one model slot for the console (sampling params flattened; `null`
/// means "use the endpoint default").
pub(super) fn slot_json(slot: &ModelSlot) -> serde_json::Value {
    json!({
        "model": slot.model,
        "temperature": slot.sampling.temperature,
        "top_p": slot.sampling.top_p,
        "top_k": slot.sampling.top_k,
        "repeat_penalty": slot.sampling.repeat_penalty,
    })
}

/// The butler model configuration (ADR 0055), editable from the console. The API
/// key is NEVER returned — only whether one is set.
pub(super) async fn get_model_config(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let cfg = blocking(move || {
        ButlerModelConfigRepository::get(config.as_ref()).map_err(AppError::Repository)
    })
    .await?
    .unwrap_or_default();
    let configured = !cfg.base_url.is_empty()
        && if cfg.mixture {
            !cfg.router.model.is_empty() && !cfg.synth.model.is_empty()
        } else {
            !cfg.single.model.is_empty()
        };
    // The effective deployment default (the env brain the butler falls back to when
    // nothing is saved). Surfaced so the Everyday card can name the model actually
    // running instead of a bare "using deployment default" — same values main.rs uses.
    let default_base_url = std::env::var("ENDORA_MODEL_URL")
        .unwrap_or_else(|_| "http://localhost:11434/v1".to_owned());
    let default_model = std::env::var("ENDORA_MODEL").unwrap_or_else(|_| "qwen2.5:7b".to_owned());
    let default_router = std::env::var("ENDORA_ROUTER_MODEL").unwrap_or_default();
    let default_synth = std::env::var("ENDORA_SYNTH_MODEL").unwrap_or_default();
    let default_mixture = !default_router.is_empty() && !default_synth.is_empty();
    Ok(Json(json!({
        "base_url": cfg.base_url,
        "preferred_url": cfg.preferred_url,
        "preferred_model": cfg.preferred_model,
        "mixture": cfg.mixture,
        "key_set": !cfg.api_key.is_empty(),
        "configured": configured,
        "single": slot_json(&cfg.single),
        "router": slot_json(&cfg.router),
        "synth": slot_json(&cfg.synth),
        "default_base_url": default_base_url,
        "default_model": default_model,
        "default_mixture": default_mixture,
        "default_router": default_router,
        "default_synth": default_synth,
    })))
}

#[derive(Deserialize, Default)]
pub(super) struct SlotRequest {
    #[serde(default)]
    model: String,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    top_p: Option<f64>,
    #[serde(default)]
    top_k: Option<u32>,
    #[serde(default)]
    repeat_penalty: Option<f64>,
}

impl SlotRequest {
    fn into_slot(self) -> ModelSlot {
        ModelSlot {
            model: self.model.trim().to_owned(),
            sampling: Sampling {
                temperature: self.temperature,
                top_p: self.top_p,
                top_k: self.top_k,
                repeat_penalty: self.repeat_penalty,
            },
        }
    }
}

#[derive(Deserialize)]
pub(super) struct ModelConfigRequest {
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    mixture: bool,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    single: SlotRequest,
    #[serde(default)]
    router: SlotRequest,
    #[serde(default)]
    synth: SlotRequest,
    #[serde(default)]
    preferred_url: String,
    #[serde(default)]
    preferred_model: String,
}

/// Saves the butler model configuration. A blank/omitted `api_key` keeps any
/// existing key, so the secret is never round-tripped through the client. The
/// change takes effect on the next turn — the [`ConfigurableButler`] rereads it.
pub(super) async fn set_model_config(
    State(state): State<AppState>,
    Json(req): Json<ModelConfigRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    blocking(move || {
        let existing = ButlerModelConfigRepository::get(config.as_ref())
            .map_err(AppError::Repository)?
            .unwrap_or_default();
        let api_key = match req.api_key {
            Some(k) if !k.trim().is_empty() => k.trim().to_owned(),
            _ => existing.api_key, // keep the stored key when none is supplied
        };
        // The preferred rung stays on the person's own network, structurally. Base turns
        // carry the raw conversation — the pseudonym door guards only the deep rung
        // (ADR 0069) — so a public address here would move everything off the device on
        // whichever days that endpoint answered. Cloud models belong behind the door.
        let preferred_url = req.preferred_url.trim().to_owned();
        if !preferred_url.is_empty() && !endora_infrastructure::is_private_endpoint(&preferred_url)
        {
            return Err(AppError::BadRequest {
                message: "the preferred endpoint must be on your own network (a private or \
                          local address) — cloud models go under the deep model instead"
                    .to_owned(),
            });
        }
        ButlerModelConfigRepository::set(
            config.as_ref(),
            &ButlerModelConfig {
                base_url: req.base_url.trim().to_owned(),
                api_key,
                mixture: req.mixture,
                single: req.single.into_slot(),
                router: req.router.into_slot(),
                synth: req.synth.into_slot(),
                preferred_url,
                preferred_model: req.preferred_model.trim().to_owned(),
            },
        )
        .map_err(AppError::Repository)?;
        record_event(events.as_ref(), clock.as_ref(), "Updated the butler models");
        Ok::<_, AppError>(())
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(super) struct DiscoverModelsRequest {
    #[serde(default)]
    base_url: String,
    /// The key to use — falls back to the stored key for `role` when blank, so the
    /// person can discover with the already-saved key without re-entering it.
    #[serde(default)]
    api_key: Option<String>,
    /// Which stored key to fall back to: `deep` or `everyday` (default).
    #[serde(default)]
    role: Option<String>,
}

/// Lists the models an OpenAI-compatible endpoint offers, so the console can
/// populate a picker after the person enters the endpoint + key. Uses the key in
/// the request, or the stored key for the role when the field is left blank.
pub(super) async fn discover_models(
    State(state): State<AppState>,
    Json(req): Json<DiscoverModelsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let base = req.base_url.trim().to_owned();
    let models = blocking(move || {
        if base.is_empty() {
            return Err(AppError::Model {
                message: "enter the endpoint first".to_owned(),
            });
        }
        let key = match req.api_key {
            Some(k) if !k.trim().is_empty() => k.trim().to_owned(),
            _ if req.role.as_deref() == Some("deep") => DeepModelRepository::get(config.as_ref())
                .map_err(AppError::Repository)?
                .map(|m| m.api_key)
                .unwrap_or_default(),
            _ => ButlerModelConfigRepository::get(config.as_ref())
                .map_err(AppError::Repository)?
                .map(|c| c.api_key)
                .unwrap_or_default(),
        };
        endora_infrastructure::list_models(&base, &key).map_err(|e| AppError::Model { message: e })
    })
    .await?;
    Ok(Json(json!({ "models": models })))
}

#[derive(Deserialize)]
pub(super) struct TestConnectionRequest {
    #[serde(default)]
    base_url: String,
    /// The key to test — falls back to the stored key for `role` when blank, so the
    /// person can test the already-saved key without re-entering it.
    #[serde(default)]
    api_key: Option<String>,
    /// The model to exercise (a minimal completion). Blank ⇒ reachability check only.
    #[serde(default)]
    model: String,
    /// Which stored key to fall back to: `deep` or `everyday` (default).
    #[serde(default)]
    role: Option<String>,
}

/// Tests that a model endpoint + API key actually work, for the settings "Test
/// connection" button. Sends a minimal completion with the given (or stored) key —
/// a real auth check, not just a `/models` listing — and returns `{ok, detail}`
/// with a human-readable result either way. Never persists anything.
pub(super) async fn test_model_connection(
    State(state): State<AppState>,
    Json(req): Json<TestConnectionRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let base = req.base_url.trim().to_owned();
    let model = req.model.trim().to_owned();
    let result = blocking(move || {
        let key = match req.api_key {
            Some(k) if !k.trim().is_empty() => k.trim().to_owned(),
            _ if req.role.as_deref() == Some("deep") => DeepModelRepository::get(config.as_ref())
                .map_err(AppError::Repository)?
                .map(|m| m.api_key)
                .unwrap_or_default(),
            _ => ButlerModelConfigRepository::get(config.as_ref())
                .map_err(AppError::Repository)?
                .map(|c| c.api_key)
                .unwrap_or_default(),
        };
        Ok::<_, AppError>(endora_infrastructure::test_connection(&base, &key, &model))
    })
    .await?;
    // A failed test is a normal, expected outcome (bad key, wrong model) — report it
    // as data with `ok: false`, not an HTTP error.
    Ok(Json(match result {
        Ok(detail) => json!({ "ok": true, "detail": detail }),
        Err(detail) => json!({ "ok": false, "detail": detail }),
    }))
}

/// Runs the model layer synchronously (discover local models → evaluate each →
/// gated adoption), recording progress + the result to the activity log. Slow
/// (it runs the eval per candidate). Used by the manual `/v1/model-layer/run`; the
/// nightly schedule is gone (owner, 2026-10-16), and the model comparison will run it
/// on purpose.
pub(super) fn run_model_tune(
    butler: &(dyn Butler + Send + Sync),
    config: &endora_capabilities::ConfigStore,
    events: &endora_platform::EventStore,
    clock: &SystemClock,
    model_url: &str,
) {
    use endora_infrastructure::model_layer::{AdoptionOutcome, ModelCandidate, Scorecard};

    record_event(
        events,
        clock,
        "Model layer: evaluating available local models",
    );
    // Discover local models → single-model local candidates (keyless).
    let ids = endora_infrastructure::list_models(model_url, "").unwrap_or_default();
    let candidates: Vec<ModelCandidate> = ids
        .into_iter()
        .map(|id| ModelCandidate {
            name: id.clone(),
            config: ButlerModelConfig {
                base_url: model_url.to_owned(),
                api_key: String::new(),
                mixture: false,
                single: ModelSlot {
                    model: id,
                    sampling: Sampling::default(),
                },
                ..ButlerModelConfig::default()
            },
        })
        .collect();

    let mut on_propose = |c: &ModelCandidate, s: &Scorecard, incumbent: usize| {
        record_event(
            events,
            clock,
            &format!(
                "Model layer: a cloud model ({}) scored {}/{} vs {} — proposing (needs your ok)",
                c.name,
                s.total(),
                s.max(),
                incumbent
            ),
        );
    };

    match endora_infrastructure::run_model_layer(butler, candidates, config, &mut on_propose) {
        Ok((outcome, scored)) => {
            for sc in &scored {
                record_event(
                    events,
                    clock,
                    &format!(
                        "Model eval: {} scored {}/{}",
                        sc.candidate.name,
                        sc.score.total(),
                        sc.score.max()
                    ),
                );
            }
            // Say WHY, not just what. The layer has three floors now, and an outcome
            // without a reason is a judgement nobody can check (ADR 0055). The scores are
            // out of the battery's own total rather than a number frozen in this string.
            let msg = match outcome {
                AdoptionOutcome::Adopted { name, score } => {
                    format!("adopted a better local model: {name}, scoring {score}")
                }
                AdoptionOutcome::Proposed {
                    name,
                    score,
                    held_by,
                } => format!(
                    "{name} scored {score} and is worth considering, but {} — so it is \
                     yours to decide",
                    held_by.as_words()
                ),
                AdoptionOutcome::Kept { incumbent, why } => format!(
                    "kept the current model, scoring {incumbent}: {}",
                    why.as_words()
                ),
            };
            record_event(events, clock, &format!("Model layer: {msg}"));
        }
        Err(e) => record_event(events, clock, &format!("Model layer run failed: {e}")),
    }
}

/// The model endpoint the tune discovers local candidates from.
pub(super) fn tune_model_url() -> String {
    std::env::var("ENDORA_MODEL_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_owned())
}

/// Kicks off the self-improving model layer (ADR 0055) in the background:
/// discover the models on the local endpoint, score each with the fitness
/// function, and gate-adopt the best **local** one (auto-adopt local, propose
/// cloud). Slow (it runs the eval per candidate), so it runs detached and records
/// its progress + result to the activity log; the response returns immediately.
pub(super) async fn run_model_layer_now(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let butler = state.butler.clone();
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    let model_url = tune_model_url();
    tokio::task::spawn_blocking(move || {
        run_model_tune(
            butler.as_ref(),
            config.as_ref(),
            events.as_ref(),
            clock.as_ref(),
            &model_url,
        );
    });
    Ok(Json(json!({ "started": true })))
}

#[derive(Deserialize)]
pub(super) struct DeepAskRequest {
    question: String,
}

/// Escalates one question to the configured deep model and posts its answer to the
/// chat. The person opts in per question (the everyday stays local). The outbound
/// question passes the egress guard — a secret blocks it, PII is redacted (ADR 0051).
///
/// The deep model plans with the person's skills and the local model reads what they
/// return (ADR 0086): the deep model never sees what a skill returned, so nothing a
/// stranger wrote reaches it or can command it, and nothing of the person's own life
/// goes with the question beyond the question itself.
pub(super) async fn deep_ask(
    State(state): State<AppState>,
    Json(req): Json<DeepAskRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let chat = state.chat.clone();
    let understanding = state.understanding.clone();
    let config = state.config.clone();
    let events = state.events.clone();
    let audit = state.audit.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let butler = state.butler.clone();
    let capabilities = state.capabilities.clone();
    let mcp = mcp_snapshot(&state);
    let question = req.question;
    // Serialize against any other butler turn: this one runs skills too.
    let _turn = state.turn_lock.clone().lock_owned().await;
    let posted = blocking(move || {
        let Some(cfg) = DeepModelRepository::get(config.as_ref()).map_err(AppError::Repository)?
        else {
            return Ok::<_, AppError>(None);
        };
        if cfg.url.is_empty() || cfg.model.is_empty() {
            return Ok(None);
        }
        // Through the door, or not at all (ADR 0069): refuse a secret up front, before the
        // question is stored or anything is asked of anyone.
        endora_application::egress::TheirOwnQuestion::checked(&question)
            .map_err(|message| AppError::BadRequest { message })?;
        record_event(events.as_ref(), clock.as_ref(), "Asked the deep model");
        // The deep model is used here because the person pressed the button, whether or not
        // they turned the automatic fallback on; so it is built from the configuration
        // directly rather than through `Butler::deeper`, which honours that switch.
        let deep = endora_application::egress::Deeper::new(Arc::new(
            endora_infrastructure::LlmButler::with_config(
                cfg.url.clone(),
                cfg.model.clone(),
                cfg.api_key.clone(),
                endora_application::Sampling::default(),
            )
            .with_usage_log("deep", store.clone()),
        ));
        let planned =
            endora_application::deep_turn::PlannedByTheDeepModel::new(&deep, butler.as_ref());
        let runner = build_runner(
            config.as_ref(),
            capabilities,
            mcp,
            proven_now(understanding.as_ref()),
        );
        let mut context = usecases::butler_context(
            understanding.as_ref(),
            understanding.as_ref(),
            understanding.as_ref(),
            config.as_ref(),
            chat.as_ref(),
            &runner,
            config.as_ref(),
            clock.as_ref(),
        )?;
        context.context_from_ms = topic_start(config.as_ref());
        let (reply, activity) = usecases::send_to_butler_streaming_for_request(
            chat.as_ref(),
            understanding.as_ref(),
            understanding.as_ref(),
            understanding.as_ref(),
            understanding.as_ref(),
            &runner,
            &planned,
            audit.as_ref(),
            None,
            ids.as_ref(),
            clock.as_ref(),
            &context,
            &question,
            None,
            &mut |_| {},
            &mut |_| {},
            &mut Vec::new(),
        )?;
        for item in &activity {
            record_event(events.as_ref(), clock.as_ref(), item);
        }
        Ok(Some((reply, activity)))
    })
    .await?;
    let _ = state.changes.send(());
    match posted {
        Some((msg, activity)) => Ok(Json(json!({
            "answered": true,
            "message": MessageResponse::from(&msg),
            "activity": activity,
        }))),
        None => Ok(Json(
            json!({ "answered": false, "note": "Configure a deep model in Settings first." }),
        )),
    }
}

/// The daily-brief schedule (when the butler prepares a brief on its own).
pub(super) async fn get_brief_schedule(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let schedules = state.schedules.clone();
    let s = blocking(move || usecases::brief_schedule(schedules.as_ref())).await?;
    Ok(Json(brief_schedule_json(&s)))
}

#[derive(Deserialize)]
pub(super) struct BriefScheduleRequest {
    enabled: bool,
    hour_utc: u8,
}

/// Sets the daily-brief schedule. `hour_utc` is the UTC hour (the console converts
/// the person's local hour). Only ever prepares a brief from reversible skills.
pub(super) async fn set_brief_schedule(
    State(state): State<AppState>,
    Json(req): Json<BriefScheduleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let schedules = state.schedules.clone();
    let s = blocking(move || {
        usecases::set_brief_schedule(schedules.as_ref(), req.enabled, req.hour_utc)
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(brief_schedule_json(&s)))
}

// ---- what the deep model costs (ADR 0091) ----------------------------------

/// Midnight UTC on the first of the month containing `now_ms`.
pub(super) fn month_start_ms(now_ms: i64) -> i64 {
    const DAY_MS: i64 = 86_400_000;
    // Days since 1970-01-01 -> civil date, and back for the first of that month.
    let z = now_ms.div_euclid(DAY_MS) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * DAY_MS
}

/// What the person says a million tokens cost, in US dollars. Endora does not know any
/// provider's prices, so a cost is only shown once these are set.
#[derive(Clone, Copy, Default)]
pub(super) struct DeepPrices {
    pub(super) input: Option<f64>,
    pub(super) output: Option<f64>,
    pub(super) cached: Option<f64>,
}

impl DeepPrices {
    pub(super) fn read(config: &endora_capabilities::ConfigStore) -> Self {
        let settings = settings_map(config);
        let get = |key: &str| {
            settings
                .get("deep_model")
                .and_then(|m| m.get(key))
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0)
        };
        Self {
            input: get("price_input"),
            output: get("price_output"),
            cached: get("price_cached"),
        }
    }

    /// Dollars for these totals, or `None` until input and output prices are known. Cached
    /// input is billed at its own price when one is set, otherwise as ordinary input.
    pub(super) fn cost_usd(&self, t: &endora_application::UsageTotals) -> Option<f64> {
        let (input, output) = (self.input?, self.output?);
        let cached_price = self.cached.unwrap_or(input);
        let fresh = t.prompt_tokens.saturating_sub(t.cached_tokens);
        Some(
            (fresh as f64 * input
                + t.cached_tokens as f64 * cached_price
                + t.completion_tokens as f64 * output)
                / 1_000_000.0,
        )
    }
}

fn totals_json(t: &endora_application::UsageTotals, prices: &DeepPrices) -> serde_json::Value {
    json!({
        "calls": t.calls,
        "prompt_tokens": t.prompt_tokens,
        "completion_tokens": t.completion_tokens,
        "cached_tokens": t.cached_tokens,
        "cost_usd": prices.cost_usd(t),
    })
}

/// What the deep model's calls have used this month, and what that comes to once priced.
pub(super) async fn deep_usage(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let config = state.config.clone();
    let clock = state.clock.clone();
    let out = blocking(move || {
        use endora_application::Clock as _;
        let now = clock.now().unix_millis();
        let prices = DeepPrices::read(config.as_ref());
        let month = store
            .model_usage_totals("deep", month_start_ms(now))
            .map_err(AppError::Repository)?;
        Ok::<_, AppError>(json!({
            "month": totals_json(&month, &prices),
            "prices": { "input": prices.input, "output": prices.output, "cached": prices.cached },
        }))
    })
    .await?;
    Ok(Json(out))
}

#[derive(Deserialize)]
pub(super) struct DeepPricesRequest {
    input: Option<f64>,
    output: Option<f64>,
    cached: Option<f64>,
}

/// Stores what a million tokens cost, so a dollar figure can be shown. Blank clears one.
pub(super) async fn set_deep_prices(
    State(state): State<AppState>,
    Json(req): Json<DeepPricesRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    for v in [req.input, req.output, req.cached].into_iter().flatten() {
        if !v.is_finite() || !(0.0..=10_000.0).contains(&v) {
            return Err(ApiError(AppError::BadRequest {
                message: "a price per million tokens is a number from 0 to 10000".to_owned(),
            }));
        }
    }
    let config = state.config.clone();
    blocking(move || {
        for (key, value) in [
            ("price_input", req.input),
            ("price_output", req.output),
            ("price_cached", req.cached),
        ] {
            config
                .set_setting(
                    "deep_model",
                    key,
                    &value.map_or_else(String::new, |v| v.to_string()),
                )
                .map_err(AppError::Repository)?;
        }
        Ok::<_, AppError>(())
    })
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// How long a fetched balance is reused, so opening two screens in a row asks the provider once.
const BALANCE_REUSED_FOR: std::time::Duration = std::time::Duration::from_secs(30);

static LAST_BALANCE: std::sync::Mutex<Option<(std::time::Instant, String, serde_json::Value)>> =
    std::sync::Mutex::new(None);

/// What the deep model's provider says is left on the account. Only DeepSeek publishes a
/// balance endpoint, so for any other provider this says so and the local usage log is what
/// there is. Asked when the person opens a screen that shows it, never on a timer.
pub(super) async fn deep_balance(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let out = blocking(move || {
        let Some(cfg) = DeepModelRepository::get(config.as_ref()).map_err(AppError::Repository)?
        else {
            return Ok::<_, AppError>(json!({ "supported": false, "reason": "no deep model" }));
        };
        let host = cfg
            .url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("")
            .to_ascii_lowercase();
        if cfg.url.is_empty() || !(host == "api.deepseek.com" || host.ends_with(".deepseek.com")) {
            return Ok(
                json!({ "supported": false, "reason": "this provider has no balance lookup" }),
            );
        }
        if let Ok(last) = LAST_BALANCE.lock() {
            if let Some((at, url, body)) = last.as_ref() {
                if url == &cfg.url && at.elapsed() < BALANCE_REUSED_FOR {
                    return Ok(body.clone());
                }
            }
        }
        let answer = endora_infrastructure::provider_balance(&host, &cfg.api_key);
        let body = match answer {
            Ok(v) => json!({
                "supported": true,
                "available": v["is_available"],
                "balances": v["balance_infos"].as_array().map(|list| list.iter().map(|b| json!({
                    "currency": b["currency"],
                    "total": b["total_balance"],
                    "granted": b["granted_balance"],
                    "topped_up": b["topped_up_balance"],
                })).collect::<Vec<_>>()).unwrap_or_default(),
            }),
            Err(why) => {
                json!({ "supported": true, "error": format!("couldn't read the balance: {why}") })
            }
        };
        if let Ok(mut last) = LAST_BALANCE.lock() {
            *last = Some((std::time::Instant::now(), cfg.url.clone(), body.clone()));
        }
        Ok(body)
    })
    .await?;
    Ok(Json(out))
}
