//! Endora node binary.
//!
//! The node is Endora's authoritative backend runtime — the "brain". It serves
//! the versioned HTTP/JSON protocol that thin clients speak. This build exposes
//! the first vertical slice (Direction & Targets); the policy boundary, the model
//! adapter, and the rest of the learning loop arrive in later slices (see
//! `docs/roadmap.md`).

#![forbid(unsafe_code)]

mod api;

use std::sync::Arc;

use api::AppState;
use endora_infrastructure::{OpenAiCompatibleProposer, RandomIdSource, SqliteStore, SystemClock};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Local-first defaults; all overridable by environment.
    let db_path = std::env::var("ENDORA_DB").unwrap_or_else(|_| "endora.db".to_owned());
    let addr = std::env::var("ENDORA_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".to_owned());
    // A local, OpenAI-compatible model endpoint (e.g. Ollama). Optional: the
    // node runs without it and only the drafting endpoint returns 503.
    let model_url = std::env::var("ENDORA_MODEL_URL")
        .unwrap_or_else(|_| "http://localhost:11434/v1".to_owned());
    let model = std::env::var("ENDORA_MODEL").unwrap_or_else(|_| "qwen3.5:9b".to_owned());

    let state = AppState::new(
        Arc::new(SqliteStore::open(&db_path)?),
        Arc::new(RandomIdSource),
        Arc::new(SystemClock),
        Arc::new(OpenAiCompatibleProposer::new(
            model_url.clone(),
            model.clone(),
        )),
    );

    println!("{}", endora_application::platform_identity());
    println!("node listening on http://{addr}  (db: {db_path})");
    println!("model: {model} via {model_url}  (drafting is optional; 503 if unavailable)");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, api::app(state)).await?;
    Ok(())
}
