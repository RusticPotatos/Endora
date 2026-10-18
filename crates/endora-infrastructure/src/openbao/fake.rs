//! A pretend OpenBao, in memory, for tests (the node's and this crate's): no server runs in a
//! test. It keeps to the real one's rules where the node depends on them — sealed answers
//! nothing but its status; the root token stops working once revoked; the node's token
//! reaches only what [`POLICY`](super::POLICY) allows; a Transit ciphertext opens only with
//! the context it was made with — and every value in it is an obvious placeholder.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{Initialised, OpenBaoApi, ROLE, SealStatus, TRANSIT_KEY, VaultError, Verb};

#[derive(Default)]
struct State {
    unreachable: bool,
    initialized: bool,
    sealed: bool,
    threshold: u8,
    shares: Vec<String>,
    given: Vec<String>,
    tokens: HashMap<String, bool>,
    root_revoked: bool,
    mounts: HashSet<String>,
    transit_key_derived: Option<bool>,
    policy: Option<String>,
    role: Option<(String, String)>,
    ciphers: HashMap<String, (String, String)>,
    next: u64,
    calls: Vec<String>,
    fail_at: Option<String>,
}

/// A pretend OpenBao. See the module's documentation.
pub struct FakeOpenBao(Mutex<State>);

impl Default for FakeOpenBao {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeOpenBao {
    /// A vault that has never been initialised, and is sealed.
    #[must_use]
    pub fn new() -> Self {
        Self(Mutex::new(State {
            sealed: true,
            ..State::default()
        }))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Seals it again, as a restart of the vault does.
    pub fn seal(&self) {
        let mut s = self.state();
        s.sealed = true;
        s.given.clear();
    }

    /// Stops it answering at all, or starts it again.
    pub fn set_unreachable(&self, unreachable: bool) {
        self.state().unreachable = unreachable;
    }

    /// Makes the call to `path` fail, as a vault that refused a step would.
    pub fn fail_at(&self, path: &str) {
        self.state().fail_at = Some(path.to_owned());
    }

    /// Every path asked for, in order: paths only, never a body.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        self.state().calls.clone()
    }

    /// Whether the root token was revoked.
    #[must_use]
    pub fn root_revoked(&self) -> bool {
        self.state().root_revoked
    }

    /// Whether it is sealed.
    #[must_use]
    pub fn is_sealed(&self) -> bool {
        self.state().sealed
    }

    /// The node's policy, as written.
    #[must_use]
    pub fn policy(&self) -> Option<String> {
        self.state().policy.clone()
    }

    /// Whether the Transit key exists, and is derived.
    #[must_use]
    pub fn transit_key_derived(&self) -> Option<bool> {
        self.state().transit_key_derived
    }

    /// Which mounts exist.
    #[must_use]
    pub fn mounts(&self) -> Vec<String> {
        let mut all: Vec<String> = self.state().mounts.iter().cloned().collect();
        all.sort();
        all
    }

    fn status(s: &State) -> SealStatus {
        SealStatus {
            initialized: s.initialized,
            sealed: s.sealed,
            threshold: s.threshold,
            shares: u8::try_from(s.shares.len()).unwrap_or(u8::MAX),
            progress: u8::try_from(s.given.len()).unwrap_or(u8::MAX),
        }
    }

    fn mint(s: &mut State, prefix: &str) -> String {
        s.next += 1;
        format!("{prefix}-{}", s.next)
    }
}

fn no(status: u16, message: &str) -> VaultError {
    VaultError::Answered {
        status,
        message: message.to_owned(),
    }
}

impl OpenBaoApi for FakeOpenBao {
    fn seal_status(&self) -> Result<SealStatus, VaultError> {
        let s = self.state();
        if s.unreachable {
            return Err(VaultError::Unreachable("connection refused".to_owned()));
        }
        Ok(Self::status(&s))
    }

    fn init(&self, shares: u8, threshold: u8) -> Result<Initialised, VaultError> {
        let mut s = self.state();
        if s.unreachable {
            return Err(VaultError::Unreachable("connection refused".to_owned()));
        }
        s.calls.push("sys/init".to_owned());
        if s.initialized {
            return Err(no(400, "OpenBao is already initialized"));
        }
        s.initialized = true;
        s.threshold = threshold;
        s.shares = (0..shares)
            .map(|i| format!("placeholder-share-{i}-not-real"))
            .collect();
        let root = Self::mint(&mut s, "placeholder-root-token");
        s.tokens.insert(root.clone(), true);
        Ok(Initialised {
            shares: s.shares.iter().map(|k| Zeroizing::new(k.clone())).collect(),
            root_token: Zeroizing::new(root),
        })
    }

    fn unseal(&self, share: &str) -> Result<SealStatus, VaultError> {
        let mut s = self.state();
        if s.unreachable {
            return Err(VaultError::Unreachable("connection refused".to_owned()));
        }
        s.calls.push("sys/unseal".to_owned());
        if !s.shares.iter().any(|k| k == share) {
            return Err(no(400, "invalid key"));
        }
        if !s.given.iter().any(|k| k == share) {
            s.given.push(share.to_owned());
        }
        if s.given.len() >= usize::from(s.threshold) {
            s.sealed = false;
            s.given.clear();
        }
        Ok(Self::status(&s))
    }

    fn active(&self) -> Result<bool, VaultError> {
        let s = self.state();
        if s.unreachable {
            return Err(VaultError::Unreachable("connection refused".to_owned()));
        }
        Ok(s.initialized && !s.sealed)
    }

    fn rekey(
        &self,
        old: &[Zeroizing<String>],
        shares: u8,
        threshold: u8,
    ) -> Result<Vec<Zeroizing<String>>, VaultError> {
        let mut s = self.state();
        if s.unreachable {
            return Err(VaultError::Unreachable("connection refused".to_owned()));
        }
        s.calls.push("sys/rekey/update".to_owned());
        if s.sealed {
            return Err(VaultError::Sealed);
        }
        let mut valid: Vec<&str> = Vec::new();
        for share in old {
            if !s.shares.iter().any(|k| k == share.as_str()) {
                return Err(no(400, "invalid key"));
            }
            if !valid.contains(&share.as_str()) {
                valid.push(share.as_str());
            }
        }
        if valid.len() < usize::from(s.threshold) {
            return Err(VaultError::NotReady(
                "those shares were not enough to make new ones".to_owned(),
            ));
        }
        s.next += 1;
        let round = s.next;
        s.threshold = threshold;
        s.shares = (0..shares)
            .map(|i| format!("placeholder-share-rekeyed-{round}-{i}"))
            .collect();
        Ok(s.shares.iter().map(|k| Zeroizing::new(k.clone())).collect())
    }

    #[allow(clippy::too_many_lines)]
    fn call(
        &self,
        token: &str,
        verb: Verb,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, VaultError> {
        let mut s = self.state();
        if s.unreachable {
            return Err(VaultError::Unreachable("connection refused".to_owned()));
        }
        s.calls.push(path.to_owned());
        if s.sealed {
            return Err(VaultError::Sealed);
        }
        if s.fail_at.as_deref() == Some(path) {
            return Err(no(500, "a pretend failure"));
        }
        let empty = json!({});
        let body = body.unwrap_or(&empty);
        if path == "auth/approle/login" {
            let given = (
                body["role_id"].as_str().unwrap_or(""),
                body["secret_id"].as_str().unwrap_or(""),
            );
            let Some((role, secret)) = s.role.clone() else {
                return Err(no(400, "invalid role or secret ID"));
            };
            if given != (role.as_str(), secret.as_str()) {
                return Err(no(400, "invalid role or secret ID"));
            }
            let token = Self::mint(&mut s, "placeholder-node-token");
            s.tokens.insert(token.clone(), false);
            return Ok(json!({ "auth": { "client_token": token, "lease_duration": 3600 } }));
        }
        let Some(&is_root) = s.tokens.get(token) else {
            return Err(no(403, "permission denied"));
        };
        let transit = |op: &str| format!("transit/{op}/{TRANSIT_KEY}");
        let node_may = path == transit("encrypt")
            || path == transit("decrypt")
            || path.starts_with("secret/data/endora/");
        if !is_root && !node_may {
            return Err(no(403, "permission denied"));
        }
        match path {
            "auth/token/revoke-self" => {
                s.tokens.remove(token);
                if is_root {
                    s.root_revoked = true;
                }
                Ok(json!({}))
            }
            "sys/mounts/secret" | "sys/mounts/transit" | "sys/auth/approle" => {
                let mount = path.rsplit('/').next().unwrap_or_default().to_owned();
                if !s.mounts.insert(mount) {
                    return Err(no(400, "path is already in use"));
                }
                Ok(json!({}))
            }
            p if p == format!("transit/keys/{TRANSIT_KEY}") => {
                s.transit_key_derived = Some(body["derived"].as_bool().unwrap_or(false));
                Ok(json!({}))
            }
            p if p == format!("sys/policies/acl/{ROLE}") => {
                s.policy = body["policy"].as_str().map(str::to_owned);
                Ok(json!({}))
            }
            p if p == format!("auth/approle/role/{ROLE}") => Ok(json!({})),
            p if p == format!("auth/approle/role/{ROLE}/role-id") => {
                let role = Self::mint(&mut s, "placeholder-role-id");
                let secret = s.role.as_ref().map(|r| r.1.clone()).unwrap_or_default();
                s.role = Some((role.clone(), secret));
                Ok(json!({ "data": { "role_id": role } }))
            }
            p if p == format!("auth/approle/role/{ROLE}/secret-id") => {
                let secret = Self::mint(&mut s, "placeholder-secret-id");
                let role = s.role.as_ref().map(|r| r.0.clone()).unwrap_or_default();
                s.role = Some((role, secret.clone()));
                Ok(json!({ "data": { "secret_id": secret } }))
            }
            p if p == transit("encrypt") => {
                if s.transit_key_derived.is_none() {
                    return Err(no(400, "encryption key not found"));
                }
                let context = body["context"].as_str().unwrap_or("").to_owned();
                if context.is_empty() {
                    return Err(no(400, "missing 'context' for key derivation"));
                }
                let plaintext = body["plaintext"].as_str().unwrap_or("").to_owned();
                let ciphertext = Self::mint(&mut s, "vault:v1:placeholder-ciphertext");
                s.ciphers.insert(ciphertext.clone(), (context, plaintext));
                Ok(json!({ "data": { "ciphertext": ciphertext } }))
            }
            p if p == transit("decrypt") => {
                let ciphertext = body["ciphertext"].as_str().unwrap_or("");
                let context = body["context"].as_str().unwrap_or("");
                match s.ciphers.get(ciphertext) {
                    Some((made_with, plaintext)) if made_with == context => {
                        Ok(json!({ "data": { "plaintext": plaintext } }))
                    }
                    _ => Err(no(400, "cipher: message authentication failed")),
                }
            }
            _ if verb == Verb::Get => Err(no(404, "no handler for route")),
            _ => Err(no(405, "unsupported path")),
        }
    }
}
