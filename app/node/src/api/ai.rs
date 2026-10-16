//! The AI screen (plan phase 2): every model Endora uses, where it runs, what it sees, and
//! what the model server says about it right now.
//!
//! The Models screen showed the conversation model and the deep one. The mail reader's model,
//! vision, speech-to-text and the context size were set in the deployment and visible nowhere,
//! and nothing said whether a model ran on the graphics card or what data reached it.

use super::*;

/// Ollama's own API sits at the OpenAI-compatible address without its path.
fn ollama_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next().filter(|h| !h.is_empty())?;
    Some(format!("{scheme}://{host}"))
}

/// What Ollama's `/api/ps` says about `model` if it is loaded: where it runs, how much memory
/// it holds, its context and its size. `None` when it is not loaded right now.
fn runtime_from_ps(ps: &serde_json::Value, model: &str) -> Option<serde_json::Value> {
    let wanted = if model.contains(':') {
        model.to_owned()
    } else {
        format!("{model}:latest")
    };
    let m = ps["models"]
        .as_array()?
        .iter()
        .find(|m| m["name"].as_str() == Some(wanted.as_str()))?;
    let size = m["size"].as_u64().unwrap_or(0);
    let vram = m["size_vram"].as_u64().unwrap_or(0);
    let processor = if vram == 0 {
        "processor"
    } else if vram >= size {
        "graphics card"
    } else {
        "graphics card and processor"
    };
    #[allow(clippy::cast_precision_loss)]
    let memory_gb = (size as f64 / 1e8).round() / 10.0;
    let details = &m["details"];
    let size_label = [
        details["parameter_size"].as_str(),
        details["quantization_level"].as_str(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ");
    Some(json!({
        "processor": processor,
        "memory_gb": memory_gb,
        "context": m["context_length"],
        "size": size_label,
    }))
}

/// Where the choices this screen adds are kept, beside the other settings.
const SETTINGS_ID: &str = "ai";

fn ai_setting(config: &endora_capabilities::ConfigStore, key: &str) -> Option<String> {
    settings_map(config)
        .get(SETTINGS_ID)
        .and_then(|m| m.get(key))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// The deployment's own model endpoint: the local one. Mail is read here and nowhere else.
fn local_model_url() -> String {
    std::env::var("ENDORA_MODEL_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_owned())
}

/// The deployment's own model, used wherever nothing else was chosen.
fn deployment_model() -> String {
    std::env::var("ENDORA_MODEL").unwrap_or_else(|_| "qwen2.5:7b".to_owned())
}

/// Where the mail reader reads, and with which model: the model chosen for it, or the
/// deployment's, and always the local endpoint. The reader and this screen both ask here, so
/// what the screen says is what the reader does.
fn mail_reader_target(config: &endora_capabilities::ConfigStore) -> (String, String) {
    (
        local_model_url(),
        ai_setting(config, "mail_reader_model").unwrap_or_else(deployment_model),
    )
}

/// The speech-to-text server: the one chosen on this screen, or the deployment's
/// (`ENDORA_STT_URL`), or none (the browser's own recognition).
pub(super) fn speech_url(config: &endora_capabilities::ConfigStore) -> Option<String> {
    ai_setting(config, "speech_url").or_else(|| {
        std::env::var("ENDORA_STT_URL")
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    })
}

/// The quarantined mail reader (ADR 0097), reading with whichever model is chosen for it on
/// the AI screen, **always on the local endpoint**: even when the conversation runs on a cloud
/// provider, mail stays in the house.
pub struct MailReader {
    config: Arc<endora_capabilities::ConfigStore>,
}

impl MailReader {
    /// Reads with the model chosen in `config`, on the deployment's local endpoint.
    #[must_use]
    pub fn new(config: Arc<endora_capabilities::ConfigStore>) -> Self {
        Self { config }
    }
}

impl endora_application::UntrustedReader for MailReader {
    fn read(
        &self,
        instructions: &str,
        data: &str,
    ) -> Result<String, endora_application::ProposalError> {
        let (url, model) = mail_reader_target(self.config.as_ref());
        endora_infrastructure::LlmButler::new(url, model).read(instructions, data)
    }
}

/// The address to show for an endpoint: its host and port, never a path or a key.
fn shown(url: &str) -> String {
    url.split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or("")
        .to_owned()
}

/// Asks each local Ollama what it has loaded. A few seconds at most; an endpoint that is not
/// Ollama, or not answering, simply has nothing to say.
fn loaded_models(urls: &[String]) -> std::collections::HashMap<String, serde_json::Value> {
    let mut out = std::collections::HashMap::new();
    for url in urls {
        let Some(origin) =
            ollama_origin(url).filter(|_| endora_infrastructure::is_private_endpoint(url))
        else {
            continue;
        };
        if out.contains_key(&origin) {
            continue;
        }
        if let Some(body) = endora_infrastructure::ollama_loaded(&origin) {
            out.insert(origin, body);
        }
    }
    out
}

/// Every model Endora uses: what it does, which model, where it runs, what it sees, and what
/// the model server says about it now.
pub(super) async fn ai_overview(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let store = state.store.clone();
    let clock = state.clock.clone();
    let mail_on = state.mail_reader.is_some();
    let out = blocking(move || {
        use endora_application::Clock as _;
        let butler = ButlerModelConfigRepository::get(config.as_ref())
            .map_err(AppError::Repository)?
            .unwrap_or_default();
        let deep = DeepModelRepository::get(config.as_ref())
            .map_err(AppError::Repository)?
            .unwrap_or_default();
        let env_router = std::env::var("ENDORA_ROUTER_MODEL").unwrap_or_default();
        let env_synth = std::env::var("ENDORA_SYNTH_MODEL").unwrap_or_default();
        let (chat_url, chat_models) = if butler.base_url.is_empty() {
            let models = if env_router.is_empty() || env_synth.is_empty() {
                vec![deployment_model()]
            } else {
                vec![env_router, env_synth]
            };
            (local_model_url(), models)
        } else if butler.mixture {
            (butler.base_url.clone(), vec![butler.router.model.clone(), butler.synth.model.clone()])
        } else {
            (butler.base_url.clone(), vec![butler.single.model.clone()])
        };
        let vision = settings_map(config.as_ref())
            .get("image_review")
            .and_then(|m| m.get("model"))
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());
        let speech = speech_url(config.as_ref());
        let (reader_url, reader_model) = mail_reader_target(config.as_ref());
        let local = local_model_url();
        let loaded = loaded_models(&[chat_url.clone(), local.clone()]);
        let runtime = |url: &str, model: &str| {
            ollama_origin(url)
                .and_then(|o| loaded.get(&o))
                .and_then(|ps| runtime_from_ps(ps, model))
        };
        let prices = super::models::DeepPrices::read(config.as_ref());
        let month = store
            .model_usage_totals("deep", super::models::month_start_ms(clock.now().unix_millis()))
            .map_err(AppError::Repository)?;
        let mixture = chat_models.len() == 2;
        let mut roles = Vec::new();
        for (i, model) in chat_models.iter().enumerate() {
            let (id, name, does) = match (mixture, i) {
                (false, _) => ("chat", "Conversation", "Answers you, chooses skills and words what they found."),
                (true, 0) => ("router", "Choosing skills", "Reads your message and chooses which skills to use."),
                _ => ("writer", "Writing replies", "Words the reply from what the skills found."),
            };
            roles.push(json!({
                "id": id, "name": name, "does": does, "model": model,
                "endpoint": shown(&chat_url), "local": endora_infrastructure::is_private_endpoint(&chat_url),
                "runtime": runtime(&chat_url, model),
                "sees": "Your messages, what Endora remembers about you, and what its skills return.",
                "change": "models", "on": true,
            }));
        }
        let deep_on = !deep.url.is_empty() && !deep.model.is_empty();
        roles.push(json!({
            "id": "deep", "name": "Deep model", "does": "A stronger model for hard questions, when you allow it.",
            "model": deep.model, "endpoint": shown(&deep.url), "local": deep_on && endora_infrastructure::is_private_endpoint(&deep.url),
            "runtime": serde_json::Value::Null,
            "sees": if deep.escalate {
                "A question when the everyday model fails a check, and anything you send it yourself. Never your mail, and never a turn that carried what an outside source said."
            } else {
                "Only what you send it yourself. It is not asked on its own."
            },
            "cost_usd_this_month": prices.cost_usd(&month), "calls_this_month": month.calls,
            "change": "deep", "on": deep_on,
        }));
        roles.push(json!({
            "id": "mail_reader", "name": "Reading your mail",
            "does": "Reads one email at a time and says what it is, for the Mail screen and your brief.",
            "model": reader_model, "endpoint": shown(&reader_url), "local": endora_infrastructure::is_private_endpoint(&reader_url),
            "runtime": runtime(&reader_url, &reader_model),
            "sees": "One email at a time, as data: no tools, no memory of you, nothing else. Always on your own network.",
            "change": "mail_reader", "on": mail_on,
        }));
        roles.push(json!({
            "id": "vision", "name": "Looking at images",
            "does": "Describes or answers questions about an image you send.",
            "model": vision.clone().unwrap_or_default(), "endpoint": shown(&local), "local": true,
            "runtime": vision.as_deref().and_then(|m| runtime(&local, m)),
            "sees": "Only the image you send it.",
            "change": "vision", "on": vision.is_some(),
        }));
        roles.push(json!({
            "id": "speech", "name": "Hearing you",
            "does": "Turns what you say into text when you hold the microphone.",
            "model": "", "endpoint": speech.as_deref().map(shown).unwrap_or_default(),
            "local": speech.as_deref().is_none_or(endora_infrastructure::is_private_endpoint),
            "runtime": serde_json::Value::Null,
            "sees": if speech.is_some() {
                "Your voice while you hold the microphone."
            } else {
                "Nothing: your browser's own speech recognition is used, which may send audio to its maker."
            },
            "change": "speech", "on": true,
        }));
        Ok::<_, AppError>(json!({
            "roles": roles,
            "context_tokens": context_window_tokens(),
            "mail_reader_model": ai_setting(config.as_ref(), "mail_reader_model"),
            "speech_url": ai_setting(config.as_ref(), "speech_url"),
        }))
    })
    .await?;
    Ok(Json(out))
}

#[derive(Deserialize)]
pub(super) struct AiRequest {
    /// The mail reader's model; blank goes back to the deployment's.
    mail_reader_model: Option<String>,
    /// The speech-to-text server; blank goes back to the deployment's (or none).
    speech_url: Option<String>,
}

/// Sets the choices only this screen holds. Each field present is saved; blank clears it.
pub(super) async fn set_ai(
    State(state): State<AppState>,
    Json(req): Json<AiRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let invalid = |message: &str| {
        ApiError(AppError::BadRequest {
            message: message.to_owned(),
        })
    };
    if let Some(url) = req
        .speech_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(invalid(
                "the speech-to-text address must start with http:// or https://",
            ));
        }
    }
    if let Some(model) = req.mail_reader_model.as_deref().map(str::trim) {
        if model.chars().any(char::is_control) || model.len() > 200 {
            return Err(invalid("that is not a model name"));
        }
    }
    let config = state.config.clone();
    blocking(move || {
        use endora_capabilities::application::CapabilitySettingsRepository as _;
        for (key, value) in [
            ("mail_reader_model", req.mail_reader_model),
            ("speech_url", req.speech_url),
        ] {
            if let Some(value) = value {
                config
                    .set_setting(SETTINGS_ID, key, value.trim())
                    .map_err(AppError::Repository)?;
            }
        }
        Ok::<_, AppError>(())
    })
    .await?;
    ai_overview(State(state)).await
}

#[cfg(test)]
mod tests;
