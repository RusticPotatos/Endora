//! Endora node binary.
//!
//! The node is Endora's authoritative backend runtime — the "brain". It serves
//! the versioned HTTP/JSON protocol that thin clients speak. This build exposes
//! the first vertical slice (Direction & Targets); the policy boundary, the model
//! adapter, and the rest of the learning loop arrive in later slices (see
//! `docs/roadmap.md`).

#![forbid(unsafe_code)]

mod api;
mod tls;

use std::sync::Arc;

use api::AppState;
use endora_infrastructure::{
    LlmButler, OpenAiCompatibleProposer, RandomIdSource, SqliteStore, SystemClock,
};

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
        // The butler tries the model and falls back to a scripted brain, so the
        // conversation works even with no model available.
        Arc::new(LlmButler::new(model_url.clone(), model.clone())),
    );

    println!("{}", endora_application::platform_identity());
    println!("model: {model} via {model_url}  (drafting is optional; 503 if unavailable)");

    // The butler's heartbeat: proactive check-ins on the person's cadence (off
    // until they enable it). Runs for the life of the process.
    api::spawn_heartbeat(state.clone());

    let app = api::app(state);

    // Optional self-signed HTTPS so the console is a secure context (browser voice
    // needs it). No domain/proxy required; one-time cert warning per browser.
    if std::env::var("ENDORA_TLS").ok().as_deref() == Some("1") {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let dir = std::path::Path::new(&db_path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        let mut sans = vec!["localhost".to_owned(), "127.0.0.1".to_owned()];
        if let Ok(extra) = std::env::var("ENDORA_TLS_SAN") {
            sans.extend(
                extra
                    .split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty()),
            );
        }
        let (cert_pem, key_pem) = tls::load_or_generate(&dir, &sans)?;
        let config = axum_server::tls_rustls::RustlsConfig::from_pem(cert_pem, key_pem).await?;
        let sockaddr: std::net::SocketAddr = addr.parse()?;
        println!("node listening on https://{addr}  (self-signed TLS; db: {db_path})");
        axum_server::bind_rustls(sockaddr, config)
            .serve(app.into_make_service())
            .await?;
    } else {
        println!("node listening on http://{addr}  (db: {db_path})");
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;
    }
    Ok(())
}
