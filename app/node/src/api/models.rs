use super::*;

pub(super) fn brief_schedule_json(s: &BriefSchedule) -> serde_json::Value {
    json!({ "enabled": s.enabled, "hour_utc": s.hour_utc })
}

/// The deep model as the chat and its spend need it: the deep list's first entry (ADR 0105),
/// or the single deep model on a node with no lists, and whether Endora may use it on its own.
/// The key is NEVER returned — only whether one is set.
pub(super) async fn get_deep_model(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let cfg = blocking(move || {
        use endora_capabilities::model_lists::{Job, ModelListRepository as _, targets};
        let single = DeepModelRepository::get(config.as_ref())
            .map_err(AppError::Repository)?
            .unwrap_or_default();
        let providers = config.providers().map_err(AppError::Repository)?;
        if providers.is_empty() {
            return Ok::<_, AppError>(single);
        }
        let deep = config.entries(Job::Deep).map_err(AppError::Repository)?;
        Ok(targets(&providers, &deep).into_iter().next().map_or_else(
            || endora_application::DeepModel {
                escalate: single.escalate,
                ..endora_application::DeepModel::default()
            },
            |t| endora_application::DeepModel {
                url: t.url,
                model: t.model,
                api_key: t.api_key,
                escalate: single.escalate,
            },
        ))
    })
    .await?;
    Ok(Json(json!({
        "url": cfg.url,
        "model": cfg.model,
        "configured": !cfg.url.is_empty() && !cfg.model.is_empty(),
        "key_set": !cfg.api_key.is_empty(),
        "escalate": cfg.escalate,
    })))
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
                // The order of the everyday list is the owner's (ADR 0105): a better local
                // model is said, not slotted in.
                AdoptionOutcome::Adopted { name, score } => format!(
                    "found a better local model: {name}, scoring {score}. Add it to Everyday \
                     on the AI screen to use it"
                ),
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
    let knowledge = state.knowledge.clone();
    let config = state.config.clone();
    let events = state.events.clone();
    let audit = state.audit.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let butler = state.butler.clone();
    let capabilities = state.capabilities.clone();
    let mcp = mcp_snapshot(&state);
    let question = req.question;
    let brain = state.brain.clone();
    // Serialize against any other butler turn: this one runs skills too.
    let _turn = state.turn_lock.clone().lock_owned().await;
    let posted = blocking(move || {
        // The deep model is used here because the person pressed the button, whether or not
        // they turned the automatic fallback on; so it is the deep list on request rather than
        // `Butler::deeper`, which honours that switch (ADR 0105). A node with no lists (a
        // test) reads the single deep model.
        let deep = match brain.as_ref() {
            Some(brain) => brain.deep_on_request(),
            None => DeepModelRepository::get(config.as_ref())
                .map_err(AppError::Repository)?
                .filter(|cfg| !cfg.url.is_empty() && !cfg.model.is_empty())
                .map(|cfg| {
                    endora_application::egress::Deeper::new(Arc::new(
                        endora_infrastructure::LlmButler::with_config(
                            cfg.url,
                            cfg.model,
                            cfg.api_key,
                            endora_application::Sampling::default(),
                        )
                        .with_usage_log("deep", store.clone()),
                    ))
                }),
        };
        let Some(deep) = deep else {
            return Ok::<_, AppError>(None);
        };
        // Through the door, or not at all (ADR 0069): refuse a secret up front, before the
        // question is stored or anything is asked of anyone.
        endora_application::egress::TheirOwnQuestion::checked(&question)
            .map_err(|message| AppError::BadRequest { message })?;
        record_event(events.as_ref(), clock.as_ref(), "Asked the deep model");
        let planned =
            endora_application::deep_turn::PlannedByTheDeepModel::new(&deep, butler.as_ref());
        let runner = build_runner(
            config.as_ref(),
            capabilities,
            mcp,
            proven_now(understanding.as_ref()),
        );
        let mut context = usecases::butler_context(
            knowledge.as_ref(),
            knowledge.as_ref(),
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
            knowledge.as_ref(),
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
        // The deep list's DeepSeek provider, the one with a balance to read (ADR 0105); a
        // node with no lists reads the single deep model.
        let host_of = |url: &str| {
            url.split("://")
                .nth(1)
                .and_then(|rest| rest.split('/').next())
                .unwrap_or("")
                .to_ascii_lowercase()
        };
        let is_deepseek = |url: &str| {
            let host = host_of(url);
            host == "api.deepseek.com" || host.ends_with(".deepseek.com")
        };
        let providers =
            endora_capabilities::model_lists::ModelListRepository::providers(config.as_ref())
                .map_err(AppError::Repository)?;
        let cfg = if providers.is_empty() {
            DeepModelRepository::get(config.as_ref()).map_err(AppError::Repository)?
        } else {
            let deep = endora_capabilities::model_lists::ModelListRepository::entries(
                config.as_ref(),
                endora_capabilities::model_lists::Job::Deep,
            )
            .map_err(AppError::Repository)?;
            let found = deep
                .iter()
                .filter_map(|e| providers.iter().find(|p| p.id == e.provider))
                .find(|p| is_deepseek(&p.url))
                .or_else(|| {
                    deep.iter()
                        .find_map(|e| providers.iter().find(|p| p.id == e.provider))
                });
            found.map(|p| endora_application::DeepModel {
                url: p.url.clone(),
                model: String::new(),
                api_key: p.api_key.clone(),
                escalate: false,
            })
        };
        let Some(cfg) = cfg else {
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
