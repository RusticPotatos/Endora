//! The AI screen (plan phase 2): every model Endora uses, where it runs, what it sees, and
//! what the model server says about it right now. Since the chat was retired (ADR 0136) that is
//! the house's model, which the team's local turns use, and vision on its skill; the members'
//! Claude brains are chosen on HQ.

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

/// The deployment's own model endpoint: the local one.
fn local_model_url() -> String {
    std::env::var("ENDORA_MODEL_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_owned())
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
    let out = blocking(move || {
        // Exactly what the team's local relay sends a turn's call to (ADR 0102, 0105).
        let house = super::relay::local_target(&state);
        let vision = settings_map(state.config.as_ref())
            .get("image_review")
            .and_then(|m| m.get("model"))
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());
        let local = local_model_url();
        let loaded = loaded_models(&[house.url.clone(), local.clone()]);
        let runtime = |url: &str, model: &str| {
            ollama_origin(url)
                .and_then(|o| loaded.get(&o))
                .and_then(|ps| runtime_from_ps(ps, model))
        };
        let roles = vec![
            json!({
                "id": "house", "name": "The house's model",
                "does": "Thinks for the team's members whose brain is local, and for downtime in the office.",
                "model": house.model, "endpoint": shown(&house.url),
                "local": endora_infrastructure::is_private_endpoint(&house.url),
                "runtime": runtime(&house.url, &house.model),
                "sees": "One turn's briefing: its task, the team's notes and what its tools return. Always on your own network.",
                "change": "models", "on": true,
            }),
            json!({
                "id": "vision", "name": "Looking at images",
                "does": "Describes or answers questions about an image it is given.",
                "model": vision.clone().unwrap_or_default(), "endpoint": shown(&local), "local": true,
                "runtime": vision.as_deref().and_then(|m| runtime(&local, m)),
                "sees": "Only the image it is given.",
                "change": "vision", "on": vision.is_some(),
            }),
        ];
        Ok::<_, AppError>(json!({ "roles": roles }))
    })
    .await?;
    Ok(Json(out))
}

#[cfg(test)]
mod tests;
