//! Endora node binary.
//!
//! The node is Endora's authoritative backend runtime — the "brain". It serves
//! the versioned HTTP/JSON protocol that thin clients speak. This build exposes
//! the first vertical slice (Direction & Targets); the policy boundary, the model
//! adapter, and the rest of the learning loop arrive in later slices (see
//! `docs/roadmap.md`).

#![forbid(unsafe_code)]

mod api;
mod auth;
mod mcp_catalog;
mod signin;
mod tls;
mod totp;

use std::sync::Arc;

use api::AppState;
use endora_application::Butler;
use endora_capabilities::ConfigStore;
use endora_infrastructure::{
    ConfigurableButler, LlmButler, MixtureButler, RandomIdSource, SqliteStore, SystemClock,
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
    let model = std::env::var("ENDORA_MODEL").unwrap_or_else(|_| "qwen2.5:7b".to_owned());

    // The butler brain. If a router + synthesizer are configured (ADR 0055), run
    // the mixture — a tool-tuned specialist routes to skills, a generalist writes
    // the reply — which the eval shows out-routes a single model at less VRAM.
    // Otherwise a single model does both. Either way it falls back to the scripted
    // brain, so the conversation works even with no model available.
    let router_model = std::env::var("ENDORA_ROUTER_MODEL").ok();
    let synth_model = std::env::var("ENDORA_SYNTH_MODEL").ok();
    let fallback: Arc<dyn Butler + Send + Sync> = match (router_model, synth_model) {
        (Some(router), Some(synth)) => {
            println!("butler fallback: mixture — router={router}, synth={synth} via {model_url}");
            Arc::new(MixtureButler::new(
                LlmButler::new(model_url.clone(), router),
                LlmButler::new(model_url.clone(), synth),
            ))
        }
        _ => {
            println!("butler fallback: single model {model} via {model_url}");
            Arc::new(LlmButler::new(model_url.clone(), model.clone()))
        }
    };

    // A model configuration saved from the console overrides the environment at
    // runtime (ADR 0055): the butler reads it each turn and rebuilds when it
    // changes, falling back to the environment brain above when nothing is stored.
    let store = Arc::new(SqliteStore::open(&db_path)?);
    let model_config = Arc::new(ConfigStore::new(store.db()));
    // Say which brain is actually IN EFFECT, not merely which one the environment
    // would supply. The stored configuration wins, so announcing the environment's
    // mixture while a stored single model does the work is a lie the logs tell — and
    // it cost an afternoon of diagnosing the wrong model.
    match endora_capabilities::ButlerModelConfigRepository::get(model_config.as_ref()) {
        Ok(Some(cfg)) if cfg.mixture => println!(
            "butler in effect: stored mixture — router={}, synth={}",
            cfg.router.model, cfg.synth.model
        ),
        Ok(Some(cfg)) => println!("butler in effect: stored single model {}", cfg.single.model),
        _ => println!("butler in effect: the fallback above (nothing stored)"),
    }
    // The deep model is attached as a FALLBACK, not as the brain: it is used only when the
    // local one fails a deterministic check, and only if the person turned that on (ADR
    // 0055). `ConfigStore` is both repositories, so this is the same handle.
    let butler: Arc<dyn Butler + Send + Sync> = Arc::new(
        ConfigurableButler::new(model_config.clone(), fallback).also_knowing(model_config),
    );

    let state = AppState::new(
        store,
        Arc::new(RandomIdSource),
        Arc::new(SystemClock),
        butler,
    );

    println!("{}", endora_application::platform_identity());
    println!("drafting model: {model} via {model_url}  (optional; 503 if unavailable)");

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
