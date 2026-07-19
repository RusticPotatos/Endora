//! A local, OpenAI-compatible model adapter (see
//! `docs/adr/0008-local-model-adapter.md`).
//!
//! This implements the [`Proposer`] port by talking to a local, OpenAI-style
//! `/chat/completions` endpoint (e.g. Ollama or a llama.cpp server). The model
//! only ever *proposes*; its output is returned as text and becomes an ordinary
//! pending proposal that the deterministic policy boundary still governs.
//!
//! The specific model and endpoint are configuration, not code: nothing above
//! this adapter names a provider.

use endora_application::{ProposalError, Proposer};
use serde_json::{Value, json};

/// A [`Proposer`] backed by a local OpenAI-compatible chat endpoint.
pub struct OpenAiCompatibleProposer {
    agent: ureq::Agent,
    /// Base URL, e.g. `http://localhost:11434/v1`.
    base_url: String,
    /// Model name/tag, e.g. `qwen3.5:9b`.
    model: String,
}

impl OpenAiCompatibleProposer {
    /// Creates a proposer for a local endpoint and model.
    #[must_use]
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            agent: ureq::Agent::new_with_defaults(),
            base_url,
            model,
        }
    }
}

impl Proposer for OpenAiCompatibleProposer {
    fn propose_process_change(
        &self,
        reflection_summary: &str,
        evidence_count: usize,
    ) -> Result<String, ProposalError> {
        let body = build_request(&self.model, reflection_summary, evidence_count);
        let url = format!("{}/chat/completions", self.base_url);
        let mut response = self
            .agent
            .post(&url)
            .send_json(&body)
            .map_err(|e| ProposalError::Unavailable(e.to_string()))?;
        if response.status().as_u16() >= 300 {
            return Err(ProposalError::Unavailable(format!(
                "endpoint returned status {}",
                response.status()
            )));
        }
        let json: Value = response
            .body_mut()
            .read_json()
            .map_err(|e| ProposalError::Unavailable(e.to_string()))?;
        parse_response(&json)
    }
}

/// The system instruction that frames the model as a *proposer*, not an actor.
const SYSTEM_PROMPT: &str = "You are a reasoning component for Endora, a \
personal-growth tool. Given a user's reflection, propose ONE small, concrete, \
reversible process change they could try next. Reply with a single imperative \
sentence and nothing else. You are only proposing; a human decides.";

/// Builds the OpenAI-compatible chat request body. Pure, so it is unit-tested.
fn build_request(model: &str, reflection_summary: &str, evidence_count: usize) -> Value {
    let user =
        format!("Reflection (grounded in {evidence_count} observation(s)): {reflection_summary}");
    json!({
        "model": model,
        "stream": false,
        "temperature": 0.4,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": user },
        ],
    })
}

/// Extracts the proposal text from a chat-completions response. Pure, so it is
/// unit-tested independent of any server.
fn parse_response(json: &Value) -> Result<String, ProposalError> {
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| ProposalError::Unavailable("unexpected response shape".to_owned()))?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(ProposalError::Empty);
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{build_request, parse_response};
    use endora_application::ProposalError;
    use serde_json::json;

    #[test]
    fn request_carries_model_and_reflection() {
        let body = build_request("qwen3.5:9b", "mornings work for me", 2);
        assert_eq!(body["model"], "qwen3.5:9b");
        assert_eq!(body["stream"], false);
        let user = body["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("mornings work for me"));
        assert!(user.contains("2 observation"));
    }

    #[test]
    fn parse_extracts_and_trims_the_message() {
        let json = json!({
            "choices": [ { "message": { "content": "  Default runs to mornings.  " } } ]
        });
        assert_eq!(parse_response(&json).unwrap(), "Default runs to mornings.");
    }

    #[test]
    fn parse_rejects_an_empty_message() {
        let json = json!({ "choices": [ { "message": { "content": "   " } } ] });
        assert_eq!(parse_response(&json), Err(ProposalError::Empty));
    }

    #[test]
    fn parse_rejects_an_unexpected_shape() {
        let json = json!({ "error": "boom" });
        assert!(matches!(
            parse_response(&json),
            Err(ProposalError::Unavailable(_))
        ));
    }
}
