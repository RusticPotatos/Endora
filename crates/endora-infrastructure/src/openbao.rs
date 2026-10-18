//! OpenBao, the house's secrets manager (ADR 0127): the node's client for it, the one-time
//! setup the node does with the root token, and the Transit key source that wraps the node's
//! own data keys (ADR 0125).
//!
//! Nothing here puts a share, a token, a secret-id or a key in an error or a log line: an
//! error names the step and what OpenBao said, and OpenBao's own errors never carry a value.
//!
//! The vault is reached over its own internal Docker network, in plain HTTP (ADR 0127 says
//! why); the node is the only other member of that network.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use endora_persistence::RepositoryError;
use endora_persistence::secrets::{DataKey, KeySource};
use serde_json::{Value, json};
use zeroize::Zeroizing;

/// How many unseal shares the vault is initialised with, and how many open it (ADR 0127).
///
/// Every share is wrapped under the owner's passkeys together, so against a stolen passkey the
/// threshold adds nothing. It is for the break-glass copy: three shares the owner can keep in
/// different places (the password manager, paper), any two of which open the vault, so one
/// lost is survivable and one found opens nothing.
pub const SHARES: u8 = 3;
/// See [`SHARES`].
pub const THRESHOLD: u8 = 2;

/// The Transit key that wraps the node's data keys. Derived: every request names the data
/// key's id as its context, so a ciphertext for one key id does not open as another's.
pub const TRANSIT_KEY: &str = "endora-node";
/// The node's AppRole, and its policy, by the same name.
pub const ROLE: &str = "endora-node";

/// What the node's AppRole may do, and nothing else: its own corner of KV, and encrypting and
/// decrypting with its own Transit key. Not rotating the key, not reading another key, not
/// any `sys/` path.
pub const POLICY: &str = r#"# Endora's node (ADR 0127). Written once, at setup, with the root token.
path "secret/data/endora/*" {
  capabilities = ["create", "read", "update", "delete", "list"]
}
path "secret/metadata/endora/*" {
  capabilities = ["read", "list", "delete"]
}
path "transit/encrypt/endora-node" {
  capabilities = ["update"]
}
path "transit/decrypt/endora-node" {
  capabilities = ["update"]
}
"#;

/// How long the vault has to answer. It is on the same host, so anything slower is a fault.
const ANSWERS_WITHIN: Duration = Duration::from_secs(10);

/// Why the vault did not do what the node asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultError {
    /// The vault could not be reached.
    Unreachable(String),
    /// The vault is sealed: nothing but its status and unsealing answers.
    Sealed,
    /// The vault answered, and said no.
    Answered {
        /// The HTTP status.
        status: u16,
        /// What it said (OpenBao's errors name paths and reasons, never values).
        message: String,
    },
    /// The vault is not in the state this step needs.
    NotReady(String),
}

impl core::fmt::Display for VaultError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreachable(why) => write!(f, "the vault is not answering ({why})"),
            Self::Sealed => f.write_str("the vault is locked"),
            Self::Answered { status, message } => {
                write!(f, "the vault said no ({status}: {message})")
            }
            Self::NotReady(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<VaultError> for RepositoryError {
    fn from(error: VaultError) -> Self {
        Self::Backend(error.to_string())
    }
}

/// What `sys/seal-status` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SealStatus {
    /// Whether `operator init` has been done.
    pub initialized: bool,
    /// Whether it is sealed.
    pub sealed: bool,
    /// How many shares open it.
    pub threshold: u8,
    /// How many shares there are.
    pub shares: u8,
    /// How many shares have been given towards the next unseal.
    pub progress: u8,
}

/// What `operator init` hands back, once: the unseal shares and the root token. Wiped when
/// dropped; never printed.
pub struct Initialised {
    /// The unseal shares, base64.
    pub shares: Vec<Zeroizing<String>>,
    /// The root token: used for setup, then revoked.
    pub root_token: Zeroizing<String>,
}

impl core::fmt::Debug for Initialised {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Initialised({} shares, ..)", self.shares.len())
    }
}

/// The node's own login: an AppRole's role id and secret id. Wiped when dropped; never
/// printed.
#[derive(Clone)]
pub struct AppRole {
    /// The role id: an identifier, but kept with the secret id all the same.
    pub role_id: Zeroizing<String>,
    /// The secret id: the credential.
    pub secret_id: Zeroizing<String>,
}

impl core::fmt::Debug for AppRole {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("AppRole(..)")
    }
}

impl AppRole {
    /// As one line of text, to be sealed (the role id has no `\n`).
    #[must_use]
    pub fn to_text(&self) -> Zeroizing<String> {
        Zeroizing::new(format!("{}\n{}", &*self.role_id, &*self.secret_id))
    }

    /// From what [`to_text`](Self::to_text) made.
    #[must_use]
    pub fn from_text(text: &str) -> Option<Self> {
        let (role, secret) = text.split_once('\n')?;
        (!role.is_empty() && !secret.is_empty()).then(|| Self {
            role_id: Zeroizing::new(role.to_owned()),
            secret_id: Zeroizing::new(secret.to_owned()),
        })
    }
}

/// The HTTP verbs the node uses on the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Read.
    Get,
    /// Write.
    Post,
    /// Write (the `sys/` endpoints that take PUT).
    Put,
}

/// What the node asks of OpenBao. One implementation speaks HTTP ([`HttpOpenBao`]); tests
/// use [`fake::FakeOpenBao`].
pub trait OpenBaoApi: Send + Sync {
    /// `GET sys/seal-status`.
    ///
    /// # Errors
    /// [`VaultError::Unreachable`] if it does not answer.
    fn seal_status(&self) -> Result<SealStatus, VaultError>;

    /// `PUT sys/init`: Shamir shares and the root token, once in the vault's life.
    ///
    /// # Errors
    /// [`VaultError`] if it is already initialised or does not answer.
    fn init(&self, shares: u8, threshold: u8) -> Result<Initialised, VaultError>;

    /// `PUT sys/unseal` with one share.
    ///
    /// # Errors
    /// [`VaultError`] if the share is refused or it does not answer.
    fn unseal(&self, share: &str) -> Result<SealStatus, VaultError>;

    /// Whether it is unsealed and active (`sys/health` answers 200).
    ///
    /// # Errors
    /// [`VaultError::Unreachable`] if it does not answer.
    fn active(&self) -> Result<bool, VaultError>;

    /// Any other call, with `token` (empty: none, as for a login).
    ///
    /// # Errors
    /// [`VaultError`] if it does not answer or says no.
    fn call(
        &self,
        token: &str,
        verb: Verb,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, VaultError>;
}

/// OpenBao over HTTP, at `url` (`http://openbao:8200`).
pub struct HttpOpenBao {
    url: String,
    agent: ureq::Agent,
}

impl HttpOpenBao {
    /// A client for the vault at `url`.
    #[must_use]
    pub fn new(url: &str) -> Self {
        Self {
            url: url.trim_end_matches('/').to_owned(),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(ANSWERS_WITHIN))
                .http_status_as_error(false)
                .build()
                .into(),
        }
    }

    fn send(
        &self,
        token: &str,
        verb: Verb,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), VaultError> {
        let url = format!("{}/v1/{}", self.url, path.trim_start_matches('/'));
        let response = match verb {
            Verb::Get => {
                let mut request = self.agent.get(&url);
                if !token.is_empty() {
                    request = request.header("x-vault-token", token);
                }
                request.call()
            }
            Verb::Post | Verb::Put => {
                let mut request = if verb == Verb::Post {
                    self.agent.post(&url)
                } else {
                    self.agent.put(&url)
                };
                if !token.is_empty() {
                    request = request.header("x-vault-token", token);
                }
                request.send_json(body.cloned().unwrap_or_else(|| json!({})))
            }
        };
        let mut response = response.map_err(|e| VaultError::Unreachable(e.to_string()))?;
        let status = response.status().as_u16();
        let text = Zeroizing::new(
            response
                .body_mut()
                .read_to_string()
                .map_err(|e| VaultError::Unreachable(e.to_string()))?,
        );
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((status, value))
    }
}

/// What OpenBao said, from its `{"errors": [...]}`.
fn refusal(status: u16, body: &Value) -> VaultError {
    if status == 503 {
        return VaultError::Sealed;
    }
    let message = body["errors"]
        .as_array()
        .map(|errors| {
            errors
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; ")
        })
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| format!("status {status}"));
    VaultError::Answered {
        status,
        message: message.chars().take(300).collect(),
    }
}

fn status_from(body: &Value) -> SealStatus {
    let small = |key: &str| u8::try_from(body[key].as_u64().unwrap_or(0)).unwrap_or(u8::MAX);
    SealStatus {
        initialized: body["initialized"].as_bool().unwrap_or(false),
        sealed: body["sealed"].as_bool().unwrap_or(true),
        threshold: small("t"),
        shares: small("n"),
        progress: small("progress"),
    }
}

impl OpenBaoApi for HttpOpenBao {
    fn seal_status(&self) -> Result<SealStatus, VaultError> {
        let (status, body) = self.send("", Verb::Get, "sys/seal-status", None)?;
        if status != 200 {
            return Err(refusal(status, &body));
        }
        Ok(status_from(&body))
    }

    fn init(&self, shares: u8, threshold: u8) -> Result<Initialised, VaultError> {
        let request = json!({ "secret_shares": shares, "secret_threshold": threshold });
        let (status, mut body) = self.send("", Verb::Put, "sys/init", Some(&request))?;
        if status != 200 {
            return Err(refusal(status, &body));
        }
        let shares: Vec<Zeroizing<String>> = body["keys_base64"]
            .as_array()
            .map(|keys| {
                keys.iter()
                    .filter_map(Value::as_str)
                    .map(|k| Zeroizing::new(k.to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        let root_token = Zeroizing::new(body["root_token"].as_str().unwrap_or("").to_owned());
        // The copy in the parsed body is wiped too, as far as serde_json lets it be.
        if let Some(object) = body.as_object_mut() {
            object.clear();
        }
        if shares.is_empty() || root_token.is_empty() {
            return Err(VaultError::NotReady(
                "the vault's answer to init held no shares or no root token".to_owned(),
            ));
        }
        Ok(Initialised { shares, root_token })
    }

    fn unseal(&self, share: &str) -> Result<SealStatus, VaultError> {
        let request = json!({ "key": share });
        let (status, body) = self.send("", Verb::Put, "sys/unseal", Some(&request))?;
        if status != 200 {
            return Err(refusal(status, &body));
        }
        Ok(status_from(&body))
    }

    fn active(&self) -> Result<bool, VaultError> {
        let (status, _) = self.send("", Verb::Get, "sys/health", None)?;
        Ok(status == 200)
    }

    fn call(
        &self,
        token: &str,
        verb: Verb,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, VaultError> {
        let (status, answer) = self.send(token, verb, path, body)?;
        if (200..300).contains(&status) {
            return Ok(answer);
        }
        Err(refusal(status, &answer))
    }
}

/// Initialises the vault: Shamir shares ([`SHARES`], [`THRESHOLD`]) and a root token, once.
///
/// # Errors
/// [`VaultError::NotReady`] if it is already initialised; otherwise what the vault said.
pub fn initialise(api: &dyn OpenBaoApi) -> Result<Initialised, VaultError> {
    if api.seal_status()?.initialized {
        return Err(VaultError::NotReady(
            "the vault is already initialised; Endora initialises it only once".to_owned(),
        ));
    }
    api.init(SHARES, THRESHOLD)
}

/// Unseals the vault with as many of `shares` as it needs, and waits for it to become
/// active. Nothing is done if it is already unsealed.
///
/// # Errors
/// [`VaultError`] if a share is refused, the shares run out before it opens, or it does not
/// become active.
pub fn unseal_with(
    api: &dyn OpenBaoApi,
    shares: &[Zeroizing<String>],
) -> Result<SealStatus, VaultError> {
    let mut status = api.seal_status()?;
    if !status.initialized {
        return Err(VaultError::NotReady(
            "the vault has not been initialised".to_owned(),
        ));
    }
    for share in shares {
        if !status.sealed {
            break;
        }
        status = api.unseal(share)?;
    }
    if status.sealed {
        return Err(VaultError::NotReady(
            "the vault is still locked after every share was given".to_owned(),
        ));
    }
    for _ in 0..50 {
        if api.active()? {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(VaultError::NotReady(
        "the vault opened but did not become active".to_owned(),
    ))
}

/// A mount or an auth method that is already there is not a failure: a setup that is run
/// again finds what the first run made.
fn tolerate_existing(result: Result<Value, VaultError>) -> Result<(), VaultError> {
    match result {
        Ok(_) => Ok(()),
        Err(VaultError::Answered {
            status: 400,
            message,
        }) if message.contains("already in use") || message.contains("existing mount") => Ok(()),
        Err(e) => Err(e),
    }
}

/// What the node sets up with the root token, then the root token is revoked (ADR 0127):
/// KV v2 at `secret/`, Transit at `transit/` with the derived key [`TRANSIT_KEY`], the
/// [`POLICY`], and an AppRole [`ROLE`] holding only that policy. Returns the AppRole's login.
///
/// **The root token is revoked whatever happens**, on success and on failure alike: a setup
/// that failed half-way is started again from a reset vault (ADR 0127), never by keeping a
/// root token about.
///
/// # Errors
/// [`VaultError`] naming the step that failed.
pub fn set_up_endora(api: &dyn OpenBaoApi, root_token: &str) -> Result<AppRole, VaultError> {
    let made = set_up_with(api, root_token);
    let revoked = api.call(root_token, Verb::Post, "auth/token/revoke-self", None);
    let role = made?;
    revoked.map_err(|e| VaultError::NotReady(format!("could not revoke the root token: {e}")))?;
    Ok(role)
}

fn set_up_with(api: &dyn OpenBaoApi, root: &str) -> Result<AppRole, VaultError> {
    let step =
        |what: &'static str| move |e: VaultError| VaultError::NotReady(format!("{what}: {e}"));
    tolerate_existing(api.call(
        root,
        Verb::Post,
        "sys/mounts/secret",
        Some(&json!({ "type": "kv", "options": { "version": "2" } })),
    ))
    .map_err(step("enabling KV at secret/"))?;
    tolerate_existing(api.call(
        root,
        Verb::Post,
        "sys/mounts/transit",
        Some(&json!({ "type": "transit" })),
    ))
    .map_err(step("enabling Transit at transit/"))?;
    api.call(
        root,
        Verb::Post,
        &format!("transit/keys/{TRANSIT_KEY}"),
        Some(&json!({ "derived": true })),
    )
    .map_err(step("making the Transit key"))?;
    api.call(
        root,
        Verb::Put,
        &format!("sys/policies/acl/{ROLE}"),
        Some(&json!({ "policy": POLICY })),
    )
    .map_err(step("writing the node's policy"))?;
    tolerate_existing(api.call(
        root,
        Verb::Post,
        "sys/auth/approle",
        Some(&json!({ "type": "approle" })),
    ))
    .map_err(step("enabling AppRole"))?;
    api.call(
        root,
        Verb::Post,
        &format!("auth/approle/role/{ROLE}"),
        Some(&json!({
            "token_policies": [ROLE],
            "token_ttl": "1h",
            "token_max_ttl": "4h",
            "secret_id_ttl": "0",
        })),
    )
    .map_err(step("making the node's AppRole"))?;
    let role = api
        .call(
            root,
            Verb::Get,
            &format!("auth/approle/role/{ROLE}/role-id"),
            None,
        )
        .map_err(step("reading the role id"))?;
    let mut secret = api
        .call(
            root,
            Verb::Post,
            &format!("auth/approle/role/{ROLE}/secret-id"),
            None,
        )
        .map_err(step("making the secret id"))?;
    let role_id = Zeroizing::new(role["data"]["role_id"].as_str().unwrap_or("").to_owned());
    let secret_id = Zeroizing::new(
        secret["data"]["secret_id"]
            .as_str()
            .unwrap_or("")
            .to_owned(),
    );
    if let Some(object) = secret.as_object_mut() {
        object.clear();
    }
    if role_id.is_empty() || secret_id.is_empty() {
        return Err(VaultError::NotReady(
            "the vault made no AppRole login".to_owned(),
        ));
    }
    Ok(AppRole { role_id, secret_id })
}

/// A token the node holds, and when to stop trusting it.
struct Held {
    token: Zeroizing<String>,
    until: Instant,
}

/// The node's data keys wrapped by OpenBao's Transit key (ADR 0125's "path to OpenBao
/// Transit", built by ADR 0127). The key-encryption key never leaves the vault: wrapping is
/// `transit/encrypt`, unwrapping `transit/decrypt`, each with the data key's id as the
/// context of the derived key.
///
/// The text kept is `openbao:v1:<OpenBao's ciphertext, vault:v1:…>`.
pub struct TransitKeys {
    api: Arc<dyn OpenBaoApi>,
    role: AppRole,
    held: Mutex<Option<Held>>,
}

/// What a data key wrapped by Transit starts with.
const WRAPPED: &str = "openbao:v1:";

impl TransitKeys {
    /// Transit through `api`, logging in as `role`.
    #[must_use]
    pub fn new(api: Arc<dyn OpenBaoApi>, role: AppRole) -> Self {
        Self {
            api,
            role,
            held: Mutex::new(None),
        }
    }

    /// Logs in now, so a wrong or revoked login is found before it is relied on.
    ///
    /// # Errors
    /// [`VaultError`] if the vault is sealed, unreachable, or refuses the login.
    pub fn check(&self) -> Result<(), VaultError> {
        self.forget_token();
        self.token().map(|_| ())
    }

    fn forget_token(&self) {
        if let Ok(mut held) = self.held.lock() {
            *held = None;
        }
    }

    fn token(&self) -> Result<Zeroizing<String>, VaultError> {
        let mut held = self
            .held
            .lock()
            .map_err(|_| VaultError::NotReady("the vault token lock is poisoned".to_owned()))?;
        if let Some(h) = held.as_ref().filter(|h| Instant::now() < h.until) {
            return Ok(h.token.clone());
        }
        let mut answer = self.api.call(
            "",
            Verb::Post,
            "auth/approle/login",
            Some(&json!({
                "role_id": &*self.role.role_id,
                "secret_id": &*self.role.secret_id,
            })),
        )?;
        let token = Zeroizing::new(
            answer["auth"]["client_token"]
                .as_str()
                .unwrap_or("")
                .to_owned(),
        );
        let lease = answer["auth"]["lease_duration"].as_u64().unwrap_or(0);
        if let Some(object) = answer.as_object_mut() {
            object.clear();
        }
        if token.is_empty() {
            return Err(VaultError::NotReady(
                "the vault's login answer held no token".to_owned(),
            ));
        }
        // Trusted for most of its lease, so it is never used in its last minute.
        let keep = Duration::from_secs(lease.saturating_sub(60).max(30));
        *held = Some(Held {
            token: token.clone(),
            until: Instant::now() + keep,
        });
        Ok(token)
    }

    /// One Transit call, logging in again once if the token was refused.
    fn transit(&self, op: &str, body: &Value) -> Result<Value, VaultError> {
        let path = format!("transit/{op}/{TRANSIT_KEY}");
        let token = self.token()?;
        match self.api.call(&token, Verb::Post, &path, Some(body)) {
            Err(VaultError::Answered { status: 403, .. }) => {
                self.forget_token();
                let token = self.token()?;
                self.api.call(&token, Verb::Post, &path, Some(body))
            }
            other => other,
        }
    }
}

impl KeySource for TransitKeys {
    fn name(&self) -> &'static str {
        "openbao"
    }

    fn wrap(&self, kid: &str, key: &DataKey) -> Result<String, RepositoryError> {
        let plaintext = Zeroizing::new(STANDARD.encode(key.as_bytes()));
        let answer = self
            .transit(
                "encrypt",
                &json!({ "plaintext": &*plaintext, "context": STANDARD.encode(kid) }),
            )
            .map_err(|e| RepositoryError::Backend(format!("could not wrap data key {kid}: {e}")))?;
        let ciphertext = answer["data"]["ciphertext"].as_str().unwrap_or("");
        if !ciphertext.starts_with("vault:") {
            return Err(RepositoryError::Backend(format!(
                "the vault's answer for data key {kid} held no ciphertext"
            )));
        }
        Ok(format!("{WRAPPED}{ciphertext}"))
    }

    fn unwrap(&self, kid: &str, wrapped: &str) -> Result<DataKey, RepositoryError> {
        let Some(ciphertext) = wrapped.strip_prefix(WRAPPED) else {
            let other = wrapped.split(':').next().unwrap_or_default();
            return Err(RepositoryError::Corrupt(format!(
                "data key {kid} was wrapped by the '{other}' key source, not 'openbao'"
            )));
        };
        let mut answer = self
            .transit(
                "decrypt",
                &json!({ "ciphertext": ciphertext, "context": STANDARD.encode(kid) }),
            )
            .map_err(|e| {
                RepositoryError::Backend(format!("could not unwrap data key {kid}: {e}"))
            })?;
        let plaintext = Zeroizing::new(
            answer["data"]["plaintext"]
                .as_str()
                .unwrap_or("")
                .to_owned(),
        );
        if let Some(object) = answer.as_object_mut() {
            object.clear();
        }
        let bytes = Zeroizing::new(STANDARD.decode(plaintext.as_bytes()).map_err(|_| {
            RepositoryError::Corrupt(format!("the vault's answer for data key {kid} is damaged"))
        })?);
        DataKey::from_slice(&bytes)
            .ok_or_else(|| RepositoryError::Corrupt(format!("data key {kid} is not 32 bytes")))
    }
}

pub mod fake;

#[cfg(test)]
mod tests;
