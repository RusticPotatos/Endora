//! The vault's routes (ADR 0127).
//!
//! Three answer without a sign-in, and only these: whether the vault is locked
//! (`GET /v1/vault`), and beginning and finishing an unlock (`/v1/vault/unlock/begin`,
//! `/v1/vault/unlock`). An unlock is authorised by the passkey itself, which is a stronger
//! proof than a sign-in, and has to be: after a reboot with the node's keys in the vault, the
//! sign-in's own authenticator secret is one of the things that is locked. Every unlock is
//! throttled and audited, with who and when and never a value.
//!
//! Everything else is the signed-in owner's: registering and removing passkeys, initialising,
//! giving another passkey the key, and moving the node's keys into the vault.

use serde_json::Value;

use super::*;
use crate::vault::{Assertion, Registration, Vault};

fn now(state: &AppState) -> i64 {
    state.clock.now().unix_millis()
}

/// Writes one vault event to the audit trail and Activity: who and when, never a value.
fn record(state: &AppState, what: &str) {
    let at = state.clock.now();
    let summary = format!("Vault: {what}");
    if let Ok(entry) = endora_application::AuditRecord::new(
        endora_application::AuditId::new(state.ids.new_id()),
        at,
        &summary,
    ) {
        let _ = endora_application::AuditLog::append(state.audit.as_ref(), &entry);
    }
    let _ = endora_application::EventLog::record(state.events.as_ref(), at, &summary);
    eprintln!("vault: {what}");
}

fn vault(state: &AppState) -> Arc<Vault> {
    state.vault.clone()
}

/// `GET /v1/vault`: where the vault stands. Open: the console shows "Vault locked" before
/// anyone has signed in.
pub(super) async fn status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let vault = vault(&state);
    let at = now(&state);
    let status = blocking(move || vault.status(at)).await?;
    Ok(Json(json!({
        "vault": status.vault,
        "set_up": status.set_up,
        "keys_in_vault": status.keys_in_vault,
        "node_waiting": status.node_waiting,
        "passkeys": status.passkeys,
        "can_unlock": status.can_unlock,
        "adopting": status.adopting,
        "rp_id": state.vault.rp_id(),
    })))
}

/// `POST /v1/vault/unlock/begin`: the options for the unlocking tap. Open, as above.
pub(super) async fn begin_unlock(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let vault = vault(&state);
    let at = now(&state);
    Ok(Json(
        blocking(move || vault.begin_assertion("unlock", None, at)).await?,
    ))
}

/// `POST /v1/vault/unlock`: the tap's answer. Open, as above; the passkey is the credential.
pub(super) async fn unlock(
    State(state): State<AppState>,
    Json(answer): Json<Assertion>,
) -> Result<Json<Value>, ApiError> {
    let vault = vault(&state);
    let at = now(&state);
    let label = state
        .vault
        .label_of(&answer.id)
        .unwrap_or_else(|| "an unknown passkey".to_owned());
    let outcome = blocking(move || vault.unlock(&answer, at)).await;
    let opened = match outcome {
        Ok(opened) => opened,
        Err(ApiError(error)) => {
            record(
                &state,
                &format!("an unlock with {label} was refused ({error})"),
            );
            return Err(ApiError(error));
        }
    };
    record(&state, &format!("unlocked with {label}"));
    if opened {
        // The node's own keys opened with the vault: the tool servers that need a stored
        // credential could not connect at start, so they connect now.
        record(&state, "the node's stored credentials are open again");
        let config = state.config.clone();
        let runner = tokio::task::spawn_blocking(move || connect_mcp(config.as_ref())).await;
        if let (Ok(runner), Ok(mut slot)) = (runner, state.mcp.write()) {
            *slot = Arc::new(runner);
        }
    }
    Ok(Json(
        json!({ "unlocked": true, "node_keys_opened": opened }),
    ))
}

/// `GET /v1/vault/passkeys`: the owner's passkeys, without key material.
pub(super) async fn passkeys(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let vault = vault(&state);
    Ok(Json(json!({
        "passkeys": blocking(move || vault.passkeys()).await?,
    })))
}

/// What a new passkey is to be called.
#[derive(Deserialize)]
pub(super) struct NewPasskey {
    #[serde(default)]
    label: String,
}

/// `POST /v1/vault/passkeys/begin`: the options for registering a passkey.
pub(super) async fn begin_registration(
    State(state): State<AppState>,
    Json(new): Json<NewPasskey>,
) -> Result<Json<Value>, ApiError> {
    let vault = vault(&state);
    let at = now(&state);
    Ok(Json(
        blocking(move || vault.begin_registration(&new.label, at)).await?,
    ))
}

/// `POST /v1/vault/passkeys/finish`: the registration's answer.
pub(super) async fn finish_registration(
    State(state): State<AppState>,
    Json(registration): Json<Registration>,
) -> Result<Json<Value>, ApiError> {
    let vault = vault(&state);
    let at = now(&state);
    let added = blocking(move || vault.finish_registration(&registration, at)).await?;
    let label = state
        .vault
        .label_of(added["id"].as_str().unwrap_or_default())
        .unwrap_or_default();
    record(&state, &format!("registered the passkey {label}"));
    Ok(Json(added))
}

/// `POST /v1/vault/passkeys/{id}/remove`.
pub(super) async fn remove_passkey(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let vault = vault(&state);
    let label = state.vault.label_of(&id).unwrap_or_default();
    let removed = blocking(move || vault.remove_passkey(&id)).await?;
    if removed {
        record(&state, &format!("removed the passkey {label}"));
    }
    Ok(Json(json!({ "removed": removed })))
}

/// Which ceremony, and for which passkey.
#[derive(Deserialize)]
pub(super) struct Ceremony {
    purpose: String,
    #[serde(default)]
    credential: Option<String>,
}

/// `POST /v1/vault/ceremony`: the options for an owner's tap: `initialise`, `enrol` or
/// `adopt`.
pub(super) async fn begin_ceremony(
    State(state): State<AppState>,
    Json(ceremony): Json<Ceremony>,
) -> Result<Json<Value>, ApiError> {
    if ceremony.purpose == "unlock" {
        return Err(ApiError(AppError::BadRequest {
            message: "an unlock begins at /v1/vault/unlock/begin".to_owned(),
        }));
    }
    let vault = vault(&state);
    let at = now(&state);
    Ok(Json(
        blocking(move || {
            vault.begin_assertion(&ceremony.purpose, ceremony.credential.as_deref(), at)
        })
        .await?,
    ))
}

/// `POST /v1/vault/initialise`: the tap that initialises the vault. The answer carries the
/// unseal shares, **the only time they are ever shown**.
pub(super) async fn initialise(
    State(state): State<AppState>,
    Json(answer): Json<Assertion>,
) -> Result<Json<Value>, ApiError> {
    let vault = vault(&state);
    let at = now(&state);
    let label = state.vault.label_of(&answer.id).unwrap_or_default();
    let shares = blocking(move || vault.initialise(&answer, at)).await?;
    record(
        &state,
        &format!("initialised with {label}; the root token was revoked"),
    );
    let shares: Vec<&str> = shares.iter().map(|s| s.as_str()).collect();
    Ok(Json(json!({
        "shares": shares,
        "threshold": endora_infrastructure::openbao::THRESHOLD,
    })))
}

/// `POST /v1/vault/enrol`: an existing passkey's tap, opening two minutes in which another
/// passkey may be given the key.
pub(super) async fn enrol(
    State(state): State<AppState>,
    Json(answer): Json<Assertion>,
) -> Result<Json<Value>, ApiError> {
    let vault = vault(&state);
    let at = now(&state);
    blocking(move || vault.enrol(&answer, at)).await?;
    Ok(Json(json!({ "adopting": true })))
}

/// `POST /v1/vault/adopt`: the new passkey's tap, giving it its own copy of the key.
pub(super) async fn adopt(
    State(state): State<AppState>,
    Json(answer): Json<Assertion>,
) -> Result<Json<Value>, ApiError> {
    let vault = vault(&state);
    let at = now(&state);
    let label = state.vault.label_of(&answer.id).unwrap_or_default();
    blocking(move || vault.adopt(&answer, at)).await?;
    record(&state, &format!("the passkey {label} can now unlock"));
    Ok(Json(json!({ "can_unlock": true })))
}

/// A one-way step, said so.
#[derive(Deserialize)]
pub(super) struct Confirmed {
    #[serde(default)]
    confirm: bool,
}

/// `POST /v1/vault/keys-into-vault`: moves the node's data keys into the vault. One way.
pub(super) async fn keys_into_vault(
    State(state): State<AppState>,
    Json(confirmed): Json<Confirmed>,
) -> Result<Json<Value>, ApiError> {
    if !confirmed.confirm {
        return Err(ApiError(AppError::BadRequest {
            message: r#"send {"confirm": true}: this cannot be undone from the console"#.to_owned(),
        }));
    }
    let vault = vault(&state);
    blocking(move || vault.keys_into_vault()).await?;
    record(&state, "the node's own keys are now wrapped by the vault");
    Ok(Json(json!({ "keys_in_vault": true })))
}

/// `POST /v1/vault/start-over`: forgets this node's state for the vault, to initialise a
/// fresh one. Refused once the node's keys are in it.
pub(super) async fn start_over(
    State(state): State<AppState>,
    Json(confirmed): Json<Confirmed>,
) -> Result<Json<Value>, ApiError> {
    if !confirmed.confirm {
        return Err(ApiError(AppError::BadRequest {
            message: r#"send {"confirm": true} to forget this node's vault state"#.to_owned(),
        }));
    }
    let vault = vault(&state);
    blocking(move || vault.start_over()).await?;
    record(&state, "forgot its state for the vault, to start over");
    Ok(Json(json!({ "set_up": false })))
}
