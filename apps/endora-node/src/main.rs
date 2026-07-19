//! Endora node binary.
//!
//! The node is Endora's authoritative backend runtime — the "brain". It serves
//! the versioned HTTP/JSON protocol that thin clients speak. This build exposes
//! the first vertical slice (Direction & Goals); the policy boundary, the model
//! adapter, and the rest of the learning loop arrive in later slices (see
//! `docs/roadmap.md`).

#![forbid(unsafe_code)]

mod api;

use std::sync::Arc;

use api::AppState;
use endora_infrastructure::{RandomIdSource, SqliteStore, SystemClock};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Local-first defaults; both overridable by environment.
    let db_path = std::env::var("ENDORA_DB").unwrap_or_else(|_| "endora.db".to_owned());
    let addr = std::env::var("ENDORA_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".to_owned());

    let state = AppState {
        store: Arc::new(SqliteStore::open(&db_path)?),
        ids: Arc::new(RandomIdSource),
        clock: Arc::new(SystemClock),
    };

    println!("{}", endora_application::platform_identity());
    println!("node listening on http://{addr}  (db: {db_path})");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, api::app(state)).await?;
    Ok(())
}
