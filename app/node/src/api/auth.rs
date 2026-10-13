use super::*;

/// How long a session lasts before it has to be earned again.
pub(super) const SESSIONS_LAST_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

/// Prints the bootstrap token to the log, **for as long as it is still the only way in**.
///
/// It used to be printed once, on the run that generated it. That locked somebody out of their
/// own node within a day: `docker logs` shows the *current* container, every deploy recreates
/// it, and the token is stored rather than regenerated — so the one message carrying it was
/// gone and the console was asking for something nobody could obtain.
///
/// So it is printed every start until a password exists. That is exactly the window where it
/// is needed, and the log is host-only — the same trust boundary as the database the token
/// already sits in, so this reveals nothing new to anybody. **Once sign-in is set up it stops**,
/// because after that it is a recovery credential rather than a setup step, and a recovery
/// credential should not be lying around in a log that gets shipped to a pastebin the first
/// time somebody debugs a container.
pub(super) fn already_signed_in(store: &SqliteStore) -> bool {
    // A store that cannot answer is treated as not set up: better to print a token nobody
    // needed than to withhold the only one somebody did.
    store
        .sign_in_setup()
        .map(|(hash, _)| !hash.is_empty())
        .unwrap_or(false)
}

pub(super) fn say_the_token_if_it_is_still_needed(store: &SqliteStore, token: &str) {
    if token.is_empty() {
        return;
    }
    // A store that cannot answer is treated as not set up: better to print a token nobody
    // needed than to withhold the only one somebody did.
    if already_signed_in(store) {
        return;
    }
    eprintln!(
        "\nendora: sign-in is not set up yet, so this node's token is\n\n    {token}\n\n\
         Paste it into the console to claim this node, then choose a password and add Endora\n\
         to your authenticator. This stops appearing once that is done.\n"
    );
}

/// Makes the authenticator secret if there is not one yet, and leaves it alone if there is.
///
/// **Never regenerated.** Replacing a secret that a phone already holds would silently break
/// every code the person has, so this only ever fills a blank.
///
/// # Errors
/// [`RepositoryError`] if the backend fails.
pub(super) fn ensure_enrolment_secret(
    store: &SqliteStore,
    ids: &RandomIdSource,
) -> Result<(), endora_application::RepositoryError> {
    let (hash, secret) = store.sign_in_setup()?;
    if !secret.is_empty() {
        return Ok(());
    }
    use endora_application::IdSource as _;
    // 160 bits, which is what RFC 4226 recommends and what authenticator apps expect, taken
    // from the same source the node's identifiers already use.
    let bytes: Vec<u8> = ids.new_id().to_be_bytes()[..crate::totp::SECRET_BYTES.min(16)]
        .iter()
        .chain(ids.new_id().to_be_bytes()[..4].iter())
        .copied()
        .collect();
    store.set_sign_in_setup(&hash, &crate::totp::to_hex(&bytes))
}

/// Hashes a password with Argon2id, salt and parameters carried in the string.
pub(super) fn hash_password(password: &str) -> Result<String, String> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    argon2::Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

/// Whether a password matches a stored Argon2 hash. Any malformed hash is a refusal.
pub(super) fn password_matches(password: &str, stored: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    PasswordHash::new(stored).is_ok_and(|parsed| {
        argon2::Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

#[derive(Deserialize)]
pub(super) struct ChangePassword {
    current: String,
    new: String,
}

/// Changes the password. Requires the current one.
///
/// Being signed in is not enough. A session lives in a browser and can be taken; the password
/// is the thing only the person knows, so it is what proves this is them and not somebody
/// sitting at their unlocked laptop.
pub(super) async fn change_password(
    State(state): State<AppState>,
    Json(req): Json<ChangePassword>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if req.new.chars().count() < 12 {
        return Err(ApiError(endora_application::AppError::BadRequest {
            message: "a password shorter than twelve characters is not worth hashing".to_owned(),
        }));
    }
    let hash = hash_password(&req.new).map_err(|e| {
        ApiError(endora_application::AppError::Repository(
            endora_application::RepositoryError::Backend(e),
        ))
    })?;
    let store = state.store.clone();
    let changed = blocking(move || {
        let (current_hash, secret) = store.sign_in_setup()?;
        if !password_matches(&req.current, &current_hash) {
            return Ok::<_, endora_application::AppError>(false);
        }
        store.set_sign_in_setup(&hash, &secret)?;
        Ok(true)
    })
    .await?;
    if !changed {
        return Err(ApiError(endora_application::AppError::BadRequest {
            message: "that is not your current password".to_owned(),
        }));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(super) struct ProveItIsYou {
    current: String,
}

/// Begins replacing the authenticator: makes a new secret and hands back its QR.
///
/// **Changes nothing yet.** The live secret keeps working, and the old phone keeps working,
/// until a code from the new one proves it was really scanned. A swap that took effect on
/// being *offered* would lock somebody out of their own house the first time a QR did not
/// scan cleanly.
pub(super) async fn begin_new_authenticator(
    State(state): State<AppState>,
    Json(req): Json<ProveItIsYou>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let ids = state.ids.clone();
    let made = blocking(move || {
        let (current_hash, _) = store.sign_in_setup()?;
        if !password_matches(&req.current, &current_hash) {
            return Ok::<_, endora_application::AppError>(None);
        }
        use endora_application::IdSource as _;
        let bytes: Vec<u8> = ids.new_id().to_be_bytes()[..16]
            .iter()
            .chain(ids.new_id().to_be_bytes()[..4].iter())
            .copied()
            .collect();
        let hex = crate::totp::to_hex(&bytes);
        store.set_pending_authenticator(&hex)?;
        Ok(Some(bytes))
    })
    .await?;
    let Some(bytes) = made else {
        return Err(ApiError(endora_application::AppError::BadRequest {
            message: "that is not your current password".to_owned(),
        }));
    };
    Ok(Json(json!({
        "qr": crate::totp::enrolment_qr(&bytes, "you"),
        "otpauth": crate::totp::enrolment_uri(&bytes, "you"),
    })))
}

#[derive(Deserialize)]
pub(super) struct ConfirmAuthenticator {
    code: String,
}

/// Locks in the replacement authenticator, once a code from **it** proves it was scanned.
///
/// Verified against the pending secret, never the live one — checking the old one would
/// happily "confirm" a new phone that was never set up, which is precisely the lock-out this
/// two-step exists to prevent.
pub(super) async fn confirm_new_authenticator(
    State(state): State<AppState>,
    Json(req): Json<ConfirmAuthenticator>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let now = u64::try_from(state.clock.now().unix_millis() / 1_000).unwrap_or_default();
    let locked_in = blocking(move || {
        let pending = store.pending_authenticator()?;
        if pending.is_empty() {
            return Ok::<_, endora_application::AppError>(false);
        }
        if !crate::totp::verify(&crate::totp::from_hex(&pending), &req.code, now) {
            return Ok(false);
        }
        let (hash, _) = store.sign_in_setup()?;
        store.set_sign_in_setup(&hash, &pending)?;
        store.set_pending_authenticator("")?;
        Ok(true)
    })
    .await?;
    if !locked_in {
        return Err(ApiError(endora_application::AppError::BadRequest {
            message: "that code did not come from the new authenticator".to_owned(),
        }));
    }
    Ok(Json(json!({ "ok": true })))
}

/// The node's bootstrap token, for somebody already signed in.
///
/// They can already do everything this token allows, so showing it adds no power — it saves
/// an SSH to read a log. Named plainly as the recovery credential, because that is what it is
/// and what it should be kept like.
pub(super) async fn reveal_the_bootstrap_token(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(json!({ "token": (*state.token).clone() })))
}

/// Mints a read-only credential for the smoke suite.
///
/// Behind a full credential, because handing one out is an act only the owner should take.
///
/// It exists because `make smoke` needs a token in a plaintext file on a laptop, and that one
/// should not be able to do the things that matter — point the deep model somewhere else,
/// widen a capability, purge memory, or pull the whole conversation in a single call.
///
/// It can still read beliefs and context, and that is deliberate: three of the suite's
/// invariants assert on real belief statements, and asserting about real data is the entire
/// reason that tier exists.
pub(super) async fn mint_a_checking_token(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let ids = state.ids.clone();
    let now_ms = state.clock.now().unix_millis();
    let token = blocking(move || {
        use endora_application::IdSource as _;
        let token = format!("{:032x}{:032x}", ids.new_id(), ids.new_id());
        store.add_session(&token, "checks", now_ms, SESSIONS_LAST_MS)?;
        Ok::<_, endora_application::AppError>(token)
    })
    .await?;
    Ok(Json(json!({
        "token": token,
        "scope": "checks",
        "note": "Reads only, and not /v1/export. Put it in local.mk as ENDORA_TOKEN.",
    })))
}

/// Whether sign-in has ever been set up. **Open, and a single boolean.**
///
/// The console cannot choose its first screen without this, and the endpoint that knew was
/// itself behind the token — so a brand-new node asked people to sign in to an account that
/// did not exist yet.
///
/// Nothing else travels. Somebody learns that this Endora has a password, which one attempt
/// would have told them anyway.
pub(super) async fn whether_sign_in_exists(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let (hash, _) = blocking(move || {
        store
            .sign_in_setup()
            .map_err(endora_application::AppError::from)
    })
    .await?;
    Ok(Json(json!({ "set_up": !hash.is_empty() })))
}

/// What the console needs to set sign-in up, or to know it already is.
///
/// Behind the bearer token, deliberately: enrolling is claiming the account, and the token
/// printed to the node's log is the proof that whoever is asking has a shell on the machine
/// this runs on. Leaving this open would let whoever reached it first take the house.
pub(super) async fn sign_in_setup(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let (hash, secret) = blocking(move || {
        store
            .sign_in_setup()
            .map_err(endora_application::AppError::from)
    })
    .await?;
    // Stored as hex and shown as base32 inside an `otpauth://` URI, because those are two
    // different things: the bytes are the secret, base32 is only how they travel to a phone.
    let bytes = crate::totp::from_hex(&secret);
    let uri = crate::totp::enrolment_uri(&bytes, "you");
    Ok(Json(json!({
        "password_set": !hash.is_empty(),
        "enrolled": !secret.is_empty(),
        "qr": if hash.is_empty() { crate::totp::enrolment_qr(&bytes, "you") } else { String::new() },
        // Only ever handed out before a password exists. After that the secret is never sent
        // again — an authenticator not set up by then needs a fresh enrolment.
        "otpauth": if hash.is_empty() { uri } else { String::new() },
    })))
}

#[derive(Deserialize)]
pub(super) struct SetUpSignIn {
    password: String,
}

/// Sets the password, completing enrolment.
///
/// Also behind the bearer token, for the same reason.
pub(super) async fn set_sign_in(
    State(state): State<AppState>,
    Json(req): Json<SetUpSignIn>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if req.password.chars().count() < 12 {
        return Err(ApiError(endora_application::AppError::BadRequest {
            message: "a password shorter than twelve characters is not worth hashing".to_owned(),
        }));
    }
    let hash = hash_password(&req.password).map_err(|e| {
        ApiError(endora_application::AppError::Repository(
            endora_application::RepositoryError::Backend(e),
        ))
    })?;
    let store = state.store.clone();
    let secret = blocking(move || {
        let (_, secret) = store.sign_in_setup()?;
        store.set_sign_in_setup(&hash, &secret)?;
        Ok::<_, endora_application::AppError>(secret)
    })
    .await?;
    // Handed back **here and only here**. Claiming a node closes the window that let the
    // request through, so a second call cannot ask again — and the GET that would otherwise
    // serve it needs a credential the claimant does not have yet.
    let bytes = crate::totp::from_hex(&secret);
    Ok(Json(json!({
        "ok": true,
        // The QR is how this is actually done — nobody types a hundred-character URI into a
        // phone. The text goes alongside for a password manager, or a machine that cannot scan.
        "qr": crate::totp::enrolment_qr(&bytes, "you"),
        "otpauth": crate::totp::enrolment_uri(&bytes, "you"),
    })))
}

#[derive(Deserialize)]
pub(super) struct SignIn {
    password: String,
    code: String,
}

/// Signs in with a password and an authenticator code, returning a session token.
///
/// **The one `/v1` route that answers unsigned**, because a credential is what it exists to
/// hand out. It is not thereby unprotected: a password you can remember and six digits are
/// both guessable in a way the bootstrap token is not, so every attempt goes through
/// [`crate::signin`]'s throttle and the failures are counted across restarts.
///
/// A wrong password and a wrong code are one answer, never two. Telling somebody which half
/// they got right turns one search into two much smaller ones.
pub(super) async fn sign_in(
    State(state): State<AppState>,
    Json(req): Json<SignIn>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let now_ms = state.clock.now().unix_millis();
    let ids = state.ids.clone();
    let outcome = blocking(move || {
        let (failures, last_failure_ms) = store.sign_in_failures()?;
        let attempts = crate::signin::Attempts {
            failures,
            last_failure_ms,
        };
        if let crate::signin::Gate::WaitFor(ms) = crate::signin::may_try(attempts, now_ms) {
            return Ok::<_, endora_application::AppError>(Err(ms / 1_000));
        }
        let (hash, secret) = store.sign_in_setup()?;
        let good = !hash.is_empty()
            && password_matches(&req.password, &hash)
            && crate::totp::verify(
                // Decoded, not the stored text: the authenticator hashes the *bytes*.
                &crate::totp::from_hex(&secret),
                &req.code,
                u64::try_from(now_ms / 1_000).unwrap_or_default(),
            );
        if !good {
            let next = crate::signin::after_failure(attempts, now_ms);
            store.set_sign_in_failures(next.failures, next.last_failure_ms)?;
            return Ok(Err(0));
        }
        let cleared = crate::signin::after_success();
        store.set_sign_in_failures(cleared.failures, cleared.last_failure_ms)?;
        use endora_application::IdSource as _;
        let session = format!("{:032x}{:032x}", ids.new_id(), ids.new_id());
        store.add_session(&session, "full", now_ms, SESSIONS_LAST_MS)?;
        Ok(Ok(session))
    })
    .await?;

    match outcome {
        Ok(session) => Ok(Json(json!({ "token": session }))),
        Err(0) => Err(ApiError(endora_application::AppError::BadRequest {
            message: "that did not match".to_owned(),
        })),
        Err(seconds) => Err(ApiError(endora_application::AppError::BadRequest {
            message: format!("too many attempts — try again in about {seconds}s"),
        })),
    }
}

/// Middleware: refuses any `/v1` request that does not carry the node's token.
///
/// The console, its assets and `/health` stay open — the screen that *asks* for a token is
/// served by this node, and a health check has to answer before anybody has one. Everything
/// else is closed, including when no token is configured at all (see [`crate::auth::may_pass`]).
///
/// `401` rather than `403`: the request may be retried with a credential, which is exactly
/// what the console does after the person pastes one.
pub(super) async fn require_the_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();
    // Anything that is not a plain read. A checking credential may do none of it.
    let is_write = request.method() != Method::GET;
    let offered = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    // Only the live feed is allowed to carry it here, and only because `EventSource` cannot
    // set a header. `may_pass` enforces that, not this parsing.
    let in_query = request.uri().query().and_then(|q| {
        q.split('&')
            .find_map(|pair| pair.strip_prefix("token=").map(str::to_owned))
    });
    // The bootstrap token from the log, plus any live session. Read per request: it is one
    // indexed lookup over a handful of rows, and it means signing out takes effect at once
    // rather than whenever a cache felt like it.
    let mut accepted: Vec<(String, crate::auth::Scope)> = Vec::new();
    if !state.token.is_empty() {
        // The bootstrap token is the recovery path and is unrestricted; scoping it would mean
        // the credential you fall back to could not fix whatever went wrong.
        accepted.push(((*state.token).clone(), crate::auth::Scope::Full));
    }
    accepted.extend(
        state
            .store
            .sessions(state.clock.now().unix_millis(), SESSIONS_LAST_MS)
            .unwrap_or_default()
            .into_iter()
            .map(|(token, scope)| {
                // Anything unrecognised is treated as the narrow scope, not the wide one: a
                // scope nobody understands must not be read as permission.
                let scope = match scope.as_str() {
                    "full" => crate::auth::Scope::Full,
                    _ => crate::auth::Scope::Checks,
                };
                (token, scope)
            }),
    );
    // A node nobody has claimed yet may be claimed without the token, briefly after it comes
    // up. This is the one place the closed-by-default rule bends, and it bends only while
    // there is nothing to protect: no password, no owner, and a fifteen-minute window from
    // start-up (`crate::signin::may_claim`).
    // POST only. A GET here hands out the authenticator secret, and letting *that* through the
    // window would let somebody enrol their own phone against a node they never claimed and
    // then simply wait — which a test caught within a minute of the window existing.
    let claimable = path == "/v1/session/setup"
        && request.method() == Method::POST
        && crate::signin::may_claim(
            already_signed_in(&state.store),
            state.clock.now().unix_millis() - state.started_ms,
        );
    if !claimable
        && !crate::auth::may_pass(
            &path,
            offered.as_deref(),
            in_query.as_deref(),
            &accepted,
            is_write,
        )
    {
        return (
            StatusCode::UNAUTHORIZED,
            "this node needs its token — see the container log on first run",
        )
            .into_response();
    }
    next.run(request).await
}

/// Middleware: after a successful write (any `POST`), send a "changed" signal so
/// activity-stream subscribers refresh. Reads never notify, so the stream itself
/// (a `GET`) does not trigger it.
pub(super) async fn notify_on_change(
    State(changes): State<broadcast::Sender<()>>,
    request: Request,
    next: Next,
) -> Response {
    let is_write = request.method() == Method::POST;
    let response = next.run(request).await;
    if is_write && response.status().is_success() {
        // Ignored on purpose: no subscribers is a normal, benign state.
        let _ = changes.send(());
    }
    response
}
