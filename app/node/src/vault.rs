//! The house's vault and the passkey that unlocks it (ADR 0127).
//!
//! OpenBao keeps the secrets; after a reboot it is sealed, and the owner unseals it with one
//! passkey tap in the console. How:
//!
//! - **At initialisation** the node asks OpenBao for three Shamir shares (two open it), makes
//!   a random 32-byte **unseal key**, seals all three shares under it, and keeps that on its
//!   data volume. The unseal key itself is kept only **wrapped**, once per passkey, under a key
//!   derived from that passkey's PRF output:
//!   `HKDF-SHA256(ikm = PRF output, salt = the passkey's PRF salt, info = "endora-vault:v1:unlock:<credential id>")`,
//!   sealed with XChaCha20-Poly1305 as ADR 0125's values are. The owner is shown the shares
//!   once, as a break-glass copy, and the root token is used for setup and revoked.
//! - **To unlock**, the console runs `navigator.credentials.get` with the PRF extension; the
//!   node checks the assertion, derives the key from the PRF output the browser sends over the
//!   TLS session, unwraps the unseal key, opens the shares, gives OpenBao two of them, and
//!   forgets all of it: every one is wiped when dropped.
//! - **Another passkey** gets its own wrapped copy: an existing passkey's tap opens the unseal
//!   key for two minutes (a *ticket*, in memory only), and the new passkey's tap wraps it.
//!
//! The node's own data keys (ADR 0125) can move into the vault's Transit key
//! ([`Vault::keys_into_vault`]); from then on a fresh start while the vault is sealed opens the
//! node *waiting*, and the unlock opens its keys too.

use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload, rand_core::RngCore};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use endora_application::{AppError, RepositoryError};
use endora_infrastructure::SqliteStore;
use endora_infrastructure::openbao::{
    self, AppRole, OpenBaoApi, SealStatus, TransitKeys, VaultError,
};
use endora_infrastructure::vault_store::{StoredPasskey, VaultStore};
use endora_persistence::secrets::KeyFile;
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::signin::{Attempts, Gate, after_failure, after_success, may_try};
use crate::webauthn::{self, PublicKey};

/// The relying party a passkey is bound to: the console's name, not an address.
pub const DEFAULT_RP_ID: &str = "endora.home.arpa";

/// How many passkeys may be registered: a phone, a tablet, a hardware key and a spare.
pub const MOST_PASSKEYS: usize = 5;

/// How long a ceremony's challenge, and an opened ticket, last.
const LASTS_MS: i64 = 2 * 60 * 1_000;

/// The purpose of the note under the key file that holds the AppRole login.
const APPROLE_NOTE: &str = "vault-approle";

/// The user handle every passkey is registered under: one owner. Registering again on the same
/// device replaces that device's passkey rather than adding a second one beside it.
const OWNER: &[u8] = b"endora-owner";

/// What a ceremony was begun for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Purpose {
    Register { label: String, salt: Vec<u8> },
    Unlock,
    Initialise,
    Enrol,
    Adopt,
    Recover,
}

struct Ceremony {
    challenge: Vec<u8>,
    purpose: Purpose,
    issued_ms: i64,
}

/// The unseal key, opened by an existing passkey for long enough to wrap it for another.
struct Ticket {
    unseal_key: Zeroizing<[u8; 32]>,
    until_ms: i64,
}

/// A registration as the browser sends it, every field base64url.
#[derive(Debug, Deserialize)]
pub struct Registration {
    /// The challenge it answers.
    pub challenge: String,
    /// The credential's raw id.
    pub id: String,
    /// `response.clientDataJSON`.
    pub client_data: String,
    /// `response.attestationObject`.
    pub attestation_object: String,
    /// The PRF output, when the browser gave one at creation (some do).
    #[serde(default)]
    pub prf: Option<String>,
}

/// An assertion as the browser sends it, every field base64url.
#[derive(Debug, Deserialize)]
pub struct Assertion {
    /// The challenge it answers.
    pub challenge: String,
    /// The credential's raw id.
    pub id: String,
    /// `response.clientDataJSON`.
    pub client_data: String,
    /// `response.authenticatorData`.
    pub authenticator_data: String,
    /// `response.signature`.
    pub signature: String,
    /// `getClientExtensionResults().prf.results.first`.
    #[serde(default)]
    pub prf: Option<String>,
}

/// What the node and the console are told about the vault. No value, no share, no token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// `not configured`, `unreachable`, `uninitialised`, `locked` or `unlocked`.
    pub vault: &'static str,
    /// Whether Endora initialised it (it holds the wrapped shares).
    pub set_up: bool,
    /// Whether the node's own keys are wrapped by the vault.
    pub keys_in_vault: bool,
    /// Whether the node itself is waiting for the vault to open its keys.
    pub node_waiting: bool,
    /// How many passkeys are registered, and how many of them can unlock.
    pub passkeys: usize,
    /// See `passkeys`.
    pub can_unlock: usize,
    /// Whether another passkey may be given the unseal key now (a ticket is open).
    pub adopting: bool,
}

/// The vault, its passkeys, and the ceremonies in flight.
pub struct Vault {
    api: Option<Arc<dyn OpenBaoApi>>,
    store: VaultStore,
    node: Arc<SqliteStore>,
    key_file: Option<Arc<KeyFile>>,
    rp_id: String,
    ceremonies: Mutex<Vec<Ceremony>>,
    ticket: Mutex<Option<Ticket>>,
    attempts: Mutex<Attempts>,
}

fn refuse(message: impl Into<String>) -> AppError {
    AppError::BadRequest {
        message: message.into(),
    }
}

fn from_vault(error: VaultError) -> AppError {
    match error {
        VaultError::Unreachable(_) | VaultError::Sealed => AppError::Model {
            message: error.to_string(),
        },
        other => refuse(other.to_string()),
    }
}

fn decode(field: &str, text: &str) -> Result<Vec<u8>, AppError> {
    URL_SAFE_NO_PAD
        .decode(text.trim_end_matches('='))
        .map_err(|_| refuse(format!("the {field} is not base64url")))
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn random<const N: usize>() -> Zeroizing<[u8; N]> {
    let mut bytes = Zeroizing::new([0u8; N]);
    OsRng.fill_bytes(&mut *bytes);
    bytes
}

/// The key a passkey's PRF output gives, for that passkey alone.
fn unlock_key(prf: &[u8], salt: &[u8], credential_id: &str) -> Zeroizing<[u8; 32]> {
    struct Len;
    impl ring::hkdf::KeyType for Len {
        fn len(&self) -> usize {
            32
        }
    }
    let info = format!("endora-vault:v1:unlock:{credential_id}");
    let mut out = Zeroizing::new([0u8; 32]);
    let prk = ring::hkdf::Salt::new(ring::hkdf::HKDF_SHA256, salt).extract(prf);
    // Expanding 32 bytes from SHA-256 cannot fail; a zero key would only open nothing.
    if let Ok(okm) = prk.expand(&[info.as_bytes()], Len) {
        let _ = okm.fill(&mut *out);
    }
    out
}

fn seal(key: &[u8; 32], aad: &str, plaintext: &[u8], prefix: &str) -> Result<String, AppError> {
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let sealed = XChaCha20Poly1305::new(key.into())
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| AppError::Repository(RepositoryError::Backend("could not seal".into())))?;
    Ok(format!("{prefix}{}:{}", b64(&nonce), b64(&sealed)))
}

fn open(key: &[u8; 32], aad: &str, text: &str, prefix: &str) -> Option<Zeroizing<Vec<u8>>> {
    let (nonce, sealed) = text.strip_prefix(prefix)?.split_once(':')?;
    let nonce = URL_SAFE_NO_PAD.decode(nonce).ok()?;
    if nonce.len() != 24 {
        return None;
    }
    let sealed = URL_SAFE_NO_PAD.decode(sealed).ok()?;
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &sealed,
                aad: aad.as_bytes(),
            },
        )
        .ok()
        .map(Zeroizing::new)
}

const WRAPPED_KEY: &str = "pk:v1:";
const SEALED_SHARES: &str = "shares:v1:";
const SHARES_AAD: &str = "endora-vault:v1:shares";

fn wrapped_aad(credential_id: &str) -> String {
    format!("endora-vault:v1:unseal-key:{credential_id}")
}

/// The unseal key wrapped for one passkey, under its PRF output.
fn wrap_for(
    passkey: &StoredPasskey,
    prf: &[u8],
    unseal_key: &[u8; 32],
) -> Result<String, AppError> {
    let kek = unlock_key(prf, &passkey.prf_salt, &passkey.credential_id);
    seal(
        &kek,
        &wrapped_aad(&passkey.credential_id),
        unseal_key,
        WRAPPED_KEY,
    )
}

/// The unseal key, from one passkey's wrapped copy and its PRF output; `None` if the output
/// is not the one it was wrapped under.
fn unwrap_for(passkey: &StoredPasskey, prf: &[u8]) -> Option<Zeroizing<[u8; 32]>> {
    let kek = unlock_key(prf, &passkey.prf_salt, &passkey.credential_id);
    let opened = open(
        &kek,
        &wrapped_aad(&passkey.credential_id),
        &passkey.wrapped,
        WRAPPED_KEY,
    )?;
    let array: [u8; 32] = opened.as_slice().try_into().ok()?;
    Some(Zeroizing::new(array))
}

fn seal_shares(unseal_key: &[u8; 32], shares: &[Zeroizing<String>]) -> Result<String, AppError> {
    let list: Vec<&str> = shares.iter().map(|s| s.as_str()).collect();
    let text = Zeroizing::new(serde_json::to_vec(&list).unwrap_or_default());
    seal(unseal_key, SHARES_AAD, &text, SEALED_SHARES)
}

fn open_shares(unseal_key: &[u8; 32], sealed: &str) -> Option<Vec<Zeroizing<String>>> {
    let text = open(unseal_key, SHARES_AAD, sealed, SEALED_SHARES)?;
    let list: Vec<String> = serde_json::from_slice(&text).ok()?;
    Some(list.into_iter().map(Zeroizing::new).collect())
}

impl Vault {
    /// The vault reached through `api` (`None`: `ENDORA_VAULT_URL` is unset), its state in
    /// the node's database, the node's store, and the key file the AppRole login is sealed
    /// under.
    #[must_use]
    pub fn new(
        api: Option<Arc<dyn OpenBaoApi>>,
        node: Arc<SqliteStore>,
        key_file: Option<Arc<KeyFile>>,
        rp_id: &str,
    ) -> Self {
        Self {
            api,
            store: VaultStore::over(node.db()),
            node,
            key_file,
            rp_id: rp_id.to_owned(),
            ceremonies: Mutex::new(Vec::new()),
            ticket: Mutex::new(None),
            attempts: Mutex::new(Attempts::default()),
        }
    }

    /// What the owner called a registered passkey, for the audit trail.
    #[must_use]
    pub fn label_of(&self, credential_id: &str) -> Option<String> {
        self.store
            .passkey(credential_id)
            .ok()
            .flatten()
            .map(|p| p.label)
    }

    /// The relying party id passkeys are bound to.
    #[must_use]
    pub fn rp_id(&self) -> &str {
        &self.rp_id
    }

    fn api(&self) -> Result<&Arc<dyn OpenBaoApi>, AppError> {
        self.api.as_ref().ok_or_else(|| {
            refuse("no vault is configured on this node (ENDORA_VAULT_URL is not set)")
        })
    }

    /// Where the vault stands, asking it.
    ///
    /// # Errors
    /// [`AppError`] if the node's database fails.
    pub fn status(&self, now_ms: i64) -> Result<Status, AppError> {
        let state = self.store.state()?;
        let passkeys = self.store.passkeys()?;
        let vault = match &self.api {
            None => "not configured",
            Some(api) => match api.seal_status() {
                Err(_) => "unreachable",
                Ok(SealStatus {
                    initialized: false, ..
                }) => "uninitialised",
                Ok(SealStatus { sealed: true, .. }) => "locked",
                Ok(_) => "unlocked",
            },
        };
        Ok(Status {
            vault,
            set_up: !state.shares.is_empty(),
            keys_in_vault: state.keys_in_vault,
            node_waiting: !self.node.db().has_key_source(),
            passkeys: passkeys.len(),
            can_unlock: passkeys.iter().filter(|p| !p.wrapped.is_empty()).count(),
            adopting: self.ticket_open(now_ms),
        })
    }

    fn ticket_open(&self, now_ms: i64) -> bool {
        self.ticket
            .lock()
            .is_ok_and(|t| t.as_ref().is_some_and(|t| now_ms < t.until_ms))
    }

    /// The owner's passkeys, as the console lists them: no key material.
    ///
    /// # Errors
    /// [`AppError`] if the node's database fails.
    pub fn passkeys(&self) -> Result<Vec<Value>, AppError> {
        Ok(self
            .store
            .passkeys()?
            .into_iter()
            .map(|p| {
                json!({
                    "id": p.credential_id,
                    "label": p.label,
                    "created_ms": p.created_ms,
                    "last_used_ms": p.last_used_ms,
                    "can_unlock": !p.wrapped.is_empty(),
                })
            })
            .collect())
    }

    fn remember(&self, purpose: Purpose, now_ms: i64) -> Result<Vec<u8>, AppError> {
        let challenge = random::<32>().to_vec();
        let mut all = self
            .ceremonies
            .lock()
            .map_err(|_| refuse("the vault's ceremonies are unavailable"))?;
        all.retain(|c| now_ms - c.issued_ms < LASTS_MS);
        if all.len() >= 16 {
            all.remove(0);
        }
        all.push(Ceremony {
            challenge: challenge.clone(),
            purpose,
            issued_ms: now_ms,
        });
        Ok(challenge)
    }

    /// Takes the ceremony a challenge belongs to, once: a challenge answers one ceremony.
    fn take(&self, challenge: &str, now_ms: i64) -> Result<Purpose, AppError> {
        let challenge = decode("challenge", challenge)?;
        let mut all = self
            .ceremonies
            .lock()
            .map_err(|_| refuse("the vault's ceremonies are unavailable"))?;
        let Some(at) = all.iter().position(|c| c.challenge == challenge) else {
            return Err(refuse("that passkey request has expired: try again"));
        };
        let found = all.remove(at);
        if now_ms - found.issued_ms >= LASTS_MS {
            return Err(refuse("that passkey request has expired: try again"));
        }
        Ok(found.purpose)
    }

    /// Begins registering a passkey: the options for `navigator.credentials.create`, with a
    /// fresh PRF salt for it.
    ///
    /// # Errors
    /// [`AppError::BadRequest`] if there are already [`MOST_PASSKEYS`].
    pub fn begin_registration(&self, label: &str, now_ms: i64) -> Result<Value, AppError> {
        let existing = self.store.passkeys()?;
        if existing.len() >= MOST_PASSKEYS {
            return Err(refuse(format!(
                "there are already {MOST_PASSKEYS} passkeys: remove one first"
            )));
        }
        let label = label.trim();
        let label = if label.is_empty() { "A passkey" } else { label };
        let label: String = label.chars().take(40).collect();
        let salt = random::<32>().to_vec();
        let challenge = self.remember(
            Purpose::Register {
                label,
                salt: salt.clone(),
            },
            now_ms,
        )?;
        Ok(json!({
            "challenge": b64(&challenge),
            "rp": { "id": self.rp_id, "name": "Endora" },
            "user": { "id": b64(OWNER), "name": "owner", "displayName": "Endora's owner" },
            "pubKeyCredParams": [
                { "type": "public-key", "alg": webauthn::ES256 },
                { "type": "public-key", "alg": webauthn::EDDSA },
            ],
            "excludeCredentials": existing
                .iter()
                .map(|p| json!({ "type": "public-key", "id": p.credential_id }))
                .collect::<Vec<_>>(),
            "authenticatorSelection": {
                "residentKey": "preferred",
                "userVerification": "required",
            },
            "attestation": "none",
            "timeout": LASTS_MS,
            "extensions": { "prf": { "eval": { "first": b64(&salt) } } },
        }))
    }

    /// Finishes registering a passkey. If the browser gave the PRF output at creation and a
    /// ticket is open, the new passkey is given the unseal key at once.
    ///
    /// # Errors
    /// [`AppError::BadRequest`] if the registration does not check out.
    pub fn finish_registration(&self, r: &Registration, now_ms: i64) -> Result<Value, AppError> {
        let Purpose::Register { label, salt } = self.take(&r.challenge, now_ms)? else {
            return Err(refuse("that request was not for registering a passkey"));
        };
        let challenge = decode("challenge", &r.challenge)?;
        let registered = webauthn::check_registration(
            &self.rp_id,
            &challenge,
            &decode("client data", &r.client_data)?,
            &decode("attestation object", &r.attestation_object)?,
            &decode("credential id", &r.id)?,
        )
        .map_err(|e| refuse(e.0))?;
        if self.store.passkeys()?.len() >= MOST_PASSKEYS {
            return Err(refuse(format!(
                "there are already {MOST_PASSKEYS} passkeys"
            )));
        }
        let mut passkey = StoredPasskey {
            credential_id: registered.credential_id.clone(),
            label,
            algorithm: registered.public_key.algorithm,
            public_key: registered.public_key.bytes,
            sign_count: registered.sign_count,
            prf_salt: salt,
            wrapped: String::new(),
            created_ms: now_ms,
            last_used_ms: 0,
        };
        if self.store.passkey(&passkey.credential_id)?.is_some() {
            return Err(refuse("that passkey is already registered"));
        }
        if let Some(prf) = r.prf.as_deref().filter(|p| !p.is_empty()) {
            let prf = Zeroizing::new(decode("PRF output", prf)?);
            if let Some(key) = self.ticket_key(now_ms) {
                passkey.wrapped = wrap_for(&passkey, &prf, &key)?;
                self.close_ticket();
            }
        }
        self.store.add_passkey(&passkey)?;
        Ok(json!({ "id": passkey.credential_id, "can_unlock": !passkey.wrapped.is_empty() }))
    }

    /// Removes a passkey. The last one that can unlock is not removed: without it only the
    /// break-glass shares would open the vault.
    ///
    /// # Errors
    /// [`AppError::BadRequest`] if it is the last one that can unlock.
    pub fn remove_passkey(&self, credential_id: &str) -> Result<bool, AppError> {
        let all = self.store.passkeys()?;
        let Some(target) = all.iter().find(|p| p.credential_id == credential_id) else {
            return Ok(false);
        };
        let others = all
            .iter()
            .filter(|p| p.credential_id != credential_id && !p.wrapped.is_empty())
            .count();
        if !target.wrapped.is_empty() && others == 0 {
            return Err(refuse(
                "this is the only passkey that can unlock the vault: add another first",
            ));
        }
        Ok(self.store.remove_passkey(credential_id)?)
    }

    /// Begins a sign-with-a-passkey ceremony: the options for `navigator.credentials.get`, with
    /// each allowed passkey's own PRF salt.
    ///
    /// `purpose` is `unlock`, `initialise`, `enrol` (an existing passkey opens a ticket) or
    /// `adopt` (a passkey is given the unseal key; `credential` names which).
    ///
    /// # Errors
    /// [`AppError::BadRequest`] if there is no passkey for it.
    pub fn begin_assertion(
        &self,
        purpose: &str,
        credential: Option<&str>,
        now_ms: i64,
    ) -> Result<Value, AppError> {
        let all = self.store.passkeys()?;
        let (purpose, allowed): (Purpose, Vec<&StoredPasskey>) = match purpose {
            "unlock" | "enrol" => (
                if purpose == "unlock" {
                    Purpose::Unlock
                } else {
                    Purpose::Enrol
                },
                all.iter().filter(|p| !p.wrapped.is_empty()).collect(),
            ),
            "initialise" => (Purpose::Initialise, all.iter().collect()),
            "adopt" | "recover" => (
                if purpose == "adopt" {
                    Purpose::Adopt
                } else {
                    Purpose::Recover
                },
                all.iter()
                    .filter(|p| credential.is_none_or(|c| c == p.credential_id))
                    .collect(),
            ),
            _ => return Err(refuse("not a passkey ceremony this node knows")),
        };
        if allowed.is_empty() {
            return Err(refuse(
                "no registered passkey can do that: register one under Settings › Vault",
            ));
        }
        let challenge = self.remember(purpose, now_ms)?;
        let mut eval = serde_json::Map::new();
        for p in &allowed {
            eval.insert(
                p.credential_id.clone(),
                json!({ "first": b64(&p.prf_salt) }),
            );
        }
        Ok(json!({
            "challenge": b64(&challenge),
            "rpId": self.rp_id,
            "allowCredentials": allowed
                .iter()
                .map(|p| json!({ "type": "public-key", "id": p.credential_id }))
                .collect::<Vec<_>>(),
            "userVerification": "required",
            "timeout": LASTS_MS,
            "extensions": { "prf": { "evalByCredential": Value::Object(eval) } },
        }))
    }

    /// Checks an assertion for `purpose`, throttled: the passkey, its signature and counter,
    /// and the PRF output's length. Returns the passkey and the PRF output.
    fn verified(
        &self,
        a: &Assertion,
        purpose: &Purpose,
        now_ms: i64,
    ) -> Result<(StoredPasskey, Zeroizing<Vec<u8>>), AppError> {
        let attempts = *self
            .attempts
            .lock()
            .map_err(|_| refuse("the vault is unavailable"))?;
        if let Gate::WaitFor(ms) = may_try(attempts, now_ms) {
            return Err(refuse(format!(
                "too many attempts: try again in about {}s",
                ms / 1_000 + 1
            )));
        }
        let outcome = self.verify_once(a, purpose, now_ms);
        if outcome.is_err() {
            self.failed(now_ms);
        }
        outcome
    }

    fn failed(&self, now_ms: i64) {
        if let Ok(mut attempts) = self.attempts.lock() {
            *attempts = after_failure(*attempts, now_ms);
        }
    }

    fn verify_once(
        &self,
        a: &Assertion,
        purpose: &Purpose,
        now_ms: i64,
    ) -> Result<(StoredPasskey, Zeroizing<Vec<u8>>), AppError> {
        if self.take(&a.challenge, now_ms)? != *purpose {
            return Err(refuse("that passkey request was for something else"));
        }
        let Some(passkey) = self.store.passkey(&a.id)? else {
            return Err(refuse("that passkey is not registered here"));
        };
        let count = webauthn::check_assertion(
            &self.rp_id,
            &decode("challenge", &a.challenge)?,
            &decode("client data", &a.client_data)?,
            &decode("authenticator data", &a.authenticator_data)?,
            &decode("signature", &a.signature)?,
            &PublicKey {
                algorithm: passkey.algorithm,
                bytes: passkey.public_key.clone(),
            },
            passkey.sign_count,
        )
        .map_err(|e| refuse(e.0))?;
        let prf = Zeroizing::new(decode("PRF output", a.prf.as_deref().unwrap_or(""))?);
        if prf.len() != 32 {
            return Err(refuse(
                "this passkey gave no PRF output: it needs a device that supports passkey \
                 PRF (iOS 18 or later, a recent browser, or a security key with hmac-secret)",
            ));
        }
        self.store.used(&passkey.credential_id, count, now_ms)?;
        Ok((passkey, prf))
    }

    fn ticket_key(&self, now_ms: i64) -> Option<Zeroizing<[u8; 32]>> {
        let ticket = self.ticket.lock().ok()?;
        ticket
            .as_ref()
            .filter(|t| now_ms < t.until_ms)
            .map(|t| t.unseal_key.clone())
    }

    fn open_ticket(&self, unseal_key: &[u8; 32], now_ms: i64) {
        if let Ok(mut ticket) = self.ticket.lock() {
            *ticket = Some(Ticket {
                unseal_key: Zeroizing::new(*unseal_key),
                until_ms: now_ms + LASTS_MS,
            });
        }
    }

    fn close_ticket(&self) {
        if let Ok(mut ticket) = self.ticket.lock() {
            *ticket = None;
        }
    }

    /// The unseal key, from a passkey that can unlock; a wrong PRF output counts as a failed
    /// attempt.
    fn unseal_key_of(
        &self,
        passkey: &StoredPasskey,
        prf: &[u8],
        now_ms: i64,
    ) -> Result<Zeroizing<[u8; 32]>, AppError> {
        if passkey.wrapped.is_empty() {
            return Err(refuse("this passkey cannot unlock the vault yet"));
        }
        let Some(key) = unwrap_for(passkey, prf) else {
            self.failed(now_ms);
            return Err(refuse("that passkey's answer did not open the vault's key"));
        };
        Ok(key)
    }

    /// Initialises the vault with a registered passkey's tap: shares, setup, root token
    /// revoked, the unseal key wrapped for that passkey. Returns the shares, **the only time
    /// they are ever shown**, as the owner's break-glass copy. Opens a ticket so the other
    /// registered passkeys can be given the unseal key without another tap of this one.
    ///
    /// # Errors
    /// [`AppError`] if it is already initialised, the passkey does not check out, or a step
    /// fails (the shares are kept before setup, so the vault can still be unlocked).
    pub fn initialise(
        &self,
        a: &Assertion,
        now_ms: i64,
    ) -> Result<Vec<Zeroizing<String>>, AppError> {
        let api = self.api()?.clone();
        let key_file = self
            .key_file
            .clone()
            .ok_or_else(|| refuse("this node has no secrets key file to seal its login under"))?;
        if !self.store.state()?.shares.is_empty() {
            return Err(refuse("Endora has already initialised this vault"));
        }
        let (passkey, prf) = self.verified(a, &Purpose::Initialise, now_ms)?;
        let made = openbao::initialise(api.as_ref()).map_err(from_vault)?;
        let unseal_key = random::<32>();
        let mut passkey = passkey;
        passkey.wrapped = wrap_for(&passkey, &prf, &unseal_key)?;
        self.store
            .set_shares(&seal_shares(&unseal_key, &made.shares)?, now_ms)?;
        self.store
            .set_wrapped(&passkey.credential_id, &passkey.wrapped)?;
        openbao::unseal_with(api.as_ref(), &made.shares).map_err(from_vault)?;
        let role = openbao::set_up_endora(api.as_ref(), &made.root_token).map_err(from_vault)?;
        self.store
            .set_approle(&key_file.seal_note(APPROLE_NOTE, &role.to_text())?)?;
        self.open_ticket(&unseal_key, now_ms);
        *self.attempts.lock().map_err(|_| refuse("unavailable"))? = after_success();
        Ok(made.shares)
    }

    /// Unlocks the vault with a passkey's tap, and then the node's own keys if it was waiting
    /// for them. Returns whether the node's keys were opened by this unlock.
    ///
    /// # Errors
    /// [`AppError`] if the passkey does not check out or the vault refuses the shares.
    pub fn unlock(&self, a: &Assertion, now_ms: i64) -> Result<bool, AppError> {
        let api = self.api()?.clone();
        let (passkey, prf) = self.verified(a, &Purpose::Unlock, now_ms)?;
        let unseal_key = self.unseal_key_of(&passkey, &prf, now_ms)?;
        let state = self.store.state()?;
        let Some(shares) = open_shares(&unseal_key, &state.shares) else {
            return Err(refuse(
                "the vault's kept shares do not open: they are damaged",
            ));
        };
        openbao::unseal_with(api.as_ref(), &shares).map_err(from_vault)?;
        drop(shares);
        drop(unseal_key);
        *self.attempts.lock().map_err(|_| refuse("unavailable"))? = after_success();
        self.open_node_keys()
    }

    /// Opens a ticket with an existing passkey's tap, for giving another passkey the key.
    ///
    /// # Errors
    /// [`AppError`] if the passkey does not check out or cannot unlock.
    pub fn enrol(&self, a: &Assertion, now_ms: i64) -> Result<(), AppError> {
        let (passkey, prf) = self.verified(a, &Purpose::Enrol, now_ms)?;
        let unseal_key = self.unseal_key_of(&passkey, &prf, now_ms)?;
        self.open_ticket(&unseal_key, now_ms);
        Ok(())
    }

    /// Gives a registered passkey its own wrapped copy of the unseal key, with its tap, while
    /// a ticket is open; the ticket closes.
    ///
    /// # Errors
    /// [`AppError`] if no ticket is open or the passkey does not check out.
    pub fn adopt(&self, a: &Assertion, now_ms: i64) -> Result<(), AppError> {
        let Some(unseal_key) = self.ticket_key(now_ms) else {
            return Err(refuse(
                "unlock with a passkey that already can first, then try again within two minutes",
            ));
        };
        let (passkey, prf) = self.verified(a, &Purpose::Adopt, now_ms)?;
        self.store.set_wrapped(
            &passkey.credential_id,
            &wrap_for(&passkey, &prf, &unseal_key)?,
        )?;
        self.close_ticket();
        Ok(())
    }

    /// Sets passkey unlock up again from the owner's break-glass shares, after every passkey
    /// that could unlock is lost (ADR 0127). The vault, unlocked on the NAS with two shares,
    /// makes three new ones from `old` (which stop working); they are kept under a new unseal
    /// key wrapped for the passkey that taps, and **every other passkey's copy is forgotten**.
    /// Returns the new shares: the new break-glass copy, shown once.
    ///
    /// # Errors
    /// [`AppError`] if the vault was not set up by Endora, is locked, refuses the shares, or
    /// the passkey does not check out.
    pub fn recover(
        &self,
        old: &[String],
        a: &Assertion,
        now_ms: i64,
    ) -> Result<Vec<Zeroizing<String>>, AppError> {
        let api = self.api()?.clone();
        if self.store.state()?.shares.is_empty() {
            return Err(refuse("Endora has not initialised this vault"));
        }
        let old: Vec<Zeroizing<String>> = old
            .iter()
            .map(|s| Zeroizing::new(s.trim().to_owned()))
            .filter(|s| !s.is_empty())
            .collect();
        if old.len() < usize::from(openbao::THRESHOLD) {
            return Err(refuse(format!(
                "{} of the break-glass shares are needed",
                openbao::THRESHOLD
            )));
        }
        let (passkey, prf) = self.verified(a, &Purpose::Recover, now_ms)?;
        let fresh = openbao::rekey(api.as_ref(), &old).map_err(|e| {
            self.failed(now_ms);
            from_vault(e)
        })?;
        let unseal_key = random::<32>();
        self.store.reseal(
            &seal_shares(&unseal_key, &fresh)?,
            &passkey.credential_id,
            &wrap_for(&passkey, &prf, &unseal_key)?,
        )?;
        self.close_ticket();
        *self.attempts.lock().map_err(|_| refuse("unavailable"))? = after_success();
        Ok(fresh)
    }

    fn approle(&self) -> Result<AppRole, AppError> {
        let state = self.store.state()?;
        let key_file = self
            .key_file
            .as_ref()
            .ok_or_else(|| refuse("this node has no secrets key file"))?;
        if state.approle.is_empty() {
            return Err(refuse("the vault has not been set up by Endora yet"));
        }
        let text = key_file.open_note(APPROLE_NOTE, &state.approle)?;
        AppRole::from_text(&text).ok_or_else(|| refuse("the vault's login note is damaged"))
    }

    /// The node's data keys, through the vault's Transit key.
    fn transit(&self) -> Result<TransitKeys, AppError> {
        let keys = TransitKeys::new(self.api()?.clone(), self.approle()?);
        keys.check().map_err(from_vault)?;
        Ok(keys)
    }

    /// If the node is waiting for its keys, opens them through Transit. Returns whether it
    /// did.
    ///
    /// # Errors
    /// [`AppError`] if the vault does not open them.
    pub fn open_node_keys(&self) -> Result<bool, AppError> {
        if self.node.db().has_key_source() {
            return Ok(false);
        }
        self.node.attach_keys(Arc::new(self.transit()?))?;
        Ok(true)
    }

    /// Moves the node's data keys into the vault (ADR 0125's `rewrap`, to Transit): from now
    /// on the key file alone no longer opens the database. **One way**: there is no route
    /// back in the console.
    ///
    /// # Errors
    /// [`AppError`] if the vault is not set up and unlocked, or they are already there.
    pub fn keys_into_vault(&self) -> Result<(), AppError> {
        if self.store.state()?.keys_in_vault {
            return Err(refuse("the node's keys are already in the vault"));
        }
        if !self.node.db().has_key_source() {
            return Err(refuse("the node's keys are not open"));
        }
        let keys = self.transit()?;
        self.node.db().rewrap(Arc::new(keys))?;
        self.store.set_keys_in_vault()?;
        Ok(())
    }

    /// Forgets this node's state for the vault (its sealed shares, login, and every passkey's
    /// copy of the unseal key), to initialise a fresh vault. Refused once the node's keys are
    /// in the vault.
    ///
    /// # Errors
    /// [`AppError`] if the node's keys are in the vault.
    pub fn start_over(&self) -> Result<(), AppError> {
        self.close_ticket();
        self.store.start_over().map_err(|e| refuse(e.to_string()))
    }
}

/// Opens the node's store (ADR 0125, ADR 0127): with the key file, or — when the data keys
/// are wrapped by the vault — through Transit, or **waiting** for the vault if it is sealed
/// or unreachable, rather than failing to start.
///
/// # Errors
/// If the database cannot be opened, or its keys are in the key file and do not open.
pub fn open_node_store(
    database: &str,
    key_file: &std::path::Path,
    api: Option<&Arc<dyn OpenBaoApi>>,
) -> Result<SqliteStore, Box<dyn std::error::Error>> {
    let db = endora_persistence::Db::open(database)?;
    if !db
        .key_sources_on_disk()?
        .iter()
        .any(|source| source == "openbao")
    {
        drop(db);
        return Ok(SqliteStore::open_sealed(database, key_file)?);
    }
    let opened = (|| -> Result<TransitKeys, String> {
        let api = api.ok_or("ENDORA_VAULT_URL is not set")?;
        let file = KeyFile::open(key_file, false).map_err(|e| e.to_string())?;
        let state = VaultStore::new(db.clone())
            .and_then(|s| s.state())
            .map_err(|e| e.to_string())?;
        let text = file
            .open_note(APPROLE_NOTE, &state.approle)
            .map_err(|e| e.to_string())?;
        let role = AppRole::from_text(&text).ok_or("the vault's login note is damaged")?;
        let keys = TransitKeys::new(api.clone(), role);
        keys.check().map_err(|e| e.to_string())?;
        Ok(keys)
    })();
    match opened {
        Ok(keys) => {
            db.attach_key_source(Arc::new(keys))?;
            Ok(SqliteStore::from_db(db)?)
        }
        Err(why) => {
            eprintln!(
                "vault: the node's keys are in the vault, which cannot open them now ({why}). \
                 Starting with stored credentials locked: unlock the vault in the console."
            );
            Ok(SqliteStore::open_waiting(db)?)
        }
    }
}

#[cfg(test)]
#[path = "vault_tests.rs"]
mod tests;
