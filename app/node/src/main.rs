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
mod backup;
mod homelab_setup;
mod mcp_catalog;
mod signin;
mod terminal_setup;
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
    if std::env::args().skip(1).collect::<Vec<_>>() == ["homelab-setup"] {
        return homelab_setup::run();
    }
    if std::env::args().skip(1).collect::<Vec<_>>() == ["terminal-setup"] {
        return terminal_setup::run();
    }
    // Local-first defaults; all overridable by environment.
    let db_path = std::env::var("ENDORA_DB").unwrap_or_else(|_| "endora.db".to_owned());
    // `endora-node backup <file>`: a consistent copy of the database, then exit. Handled
    // before anything starts, so it never opens a port or runs a migration against the
    // live database.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(done) = backup::dispatch(&args, &db_path) {
        return done;
    }
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
    // Set-but-empty means unset: compose materialises a blanked variable as "", and a
    // mixture built from two empty model names is not a butler, it is two 404s.
    let router_model = std::env::var("ENDORA_ROUTER_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty());
    let synth_model = std::env::var("ENDORA_SYNTH_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty());
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
    let brain = Arc::new(
        ConfigurableButler::new(model_config.clone(), fallback)
            .also_knowing(model_config)
            .with_usage_log(store.clone()),
    );
    let butler: Arc<dyn Butler + Send + Sync> = brain.clone();

    // A zone name that is not one stops the node rather than quietly becoming UTC.
    let time_zone = match endora_infrastructure::TimeZone::named_or_fixed(
        &std::env::var("ENDORA_TIME_ZONE").unwrap_or_default(),
        std::env::var("ENDORA_UTC_OFFSET_MINUTES")
            .ok()
            .and_then(|minutes| minutes.trim().parse::<i64>().ok())
            .filter(|minutes| minutes.abs() <= 14 * 60)
            .unwrap_or(0),
    ) {
        Ok(zone) => zone,
        Err(error) => {
            eprintln!("ENDORA_TIME_ZONE: {error}");
            std::process::exit(2);
        }
    };
    // The clock lines the model reads are the person's local time, not UTC.
    SystemClock::use_time_zone(time_zone);
    let state = AppState::new(
        store,
        Arc::new(RandomIdSource),
        Arc::new(SystemClock),
        butler,
    )
    .with_brain(brain)
    .with_time_zone(time_zone);

    println!("{}", endora_application::platform_identity());
    println!("drafting model: {model} via {model_url}  (optional; 503 if unavailable)");

    // The butler's heartbeat: proactive check-ins on the person's cadence (off
    // until they enable it). Runs for the life of the process.
    api::spawn_heartbeat(state.clone());
    // A consistent copy of the database every night, for the house's backup to find.
    api::spawn_snapshots(state.clone(), std::path::PathBuf::from(&db_path));
    api::spawn_research_worker(state.clone());
    api::spawn_homelab(state.clone());
    // The mail (ADR 0097): a read-only copy mounted at /mail, read by the local model only.
    // Nothing happens without the copy.
    let state = match mail_copy("/mail") {
        Some(copy) => {
            println!("mail: reading the copy at /mail with the local model, in quiet time");
            // The model chosen on the AI screen, always on the local endpoint.
            let reader: Arc<dyn endora_application::UntrustedReader + Send + Sync> =
                Arc::new(api::MailReader::new(state.config.clone()));
            state.with_mail(copy, reader)
        }
        _ => state,
    };
    if state.mail_copy.is_some() {
        api::spawn_mail_reader(state.clone());
    }

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

/// The mail copy at `root`, if one is mounted there with at least one account in it.
fn mail_copy(root: &str) -> Option<endora_mail::MaildirCopy> {
    let accounts = std::fs::read_dir(root).ok()?;
    let any = accounts
        .flatten()
        .any(|entry| entry.path().join("new").is_dir() || entry.path().join("cur").is_dir());
    any.then(|| endora_mail::MaildirCopy::new(root))
}
