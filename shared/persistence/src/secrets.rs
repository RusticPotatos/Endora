//! Secrets at rest (ADR 0125): an envelope scheme for the columns that hold credentials.
//!
//! A **data key** (32 random bytes, one active at a time, each with a short id) seals every
//! secret value with XChaCha20-Poly1305. The stored text is
//!
//! ```text
//! enc:v1:<kid>:<nonce, base64url>:<ciphertext and tag, base64url>
//! ```
//!
//! with `enc:v1:<kid>` as the associated data, so neither the version nor the key id can be
//! swapped without the tag failing.
//!
//! The data keys themselves are kept in the database's `secret_keys` table, each **wrapped**
//! by a [`KeySource`]: the key-encryption key lives outside the database, so a copy of the
//! database alone (a backup, a volume snapshot) opens none of it. Today's source is a key
//! file ([`KeyFile`]); a later one can be a service that never hands its key out at all (an
//! OpenBao Transit key). Changing source re-wraps the data keys and touches no value
//! ([`Db::rewrap`](crate::Db::rewrap)), which is the reason for the envelope.
//!
//! Nothing here ever puts a value in an error message or a log line. A message names the
//! column, never what was in it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload, rand_core::RngCore};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::{RepositoryError, backend, corrupt};

/// What every sealed value starts with, and the version of the scheme that wrote it.
pub const SEALED: &str = "enc:v1:";

/// One data key: 32 bytes, wiped from memory when dropped, never printed.
pub struct DataKey(Zeroizing<[u8; 32]>);

impl DataKey {
    /// A fresh key from the operating system's random source.
    #[must_use]
    pub fn random() -> Self {
        let mut bytes = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(&mut *bytes);
        Self(bytes)
    }

    /// A key from exactly 32 bytes, or `None`.
    #[must_use]
    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        let array: [u8; 32] = bytes.try_into().ok()?;
        Some(Self(Zeroizing::new(array)))
    }

    /// The key's bytes, for a cipher.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl core::fmt::Debug for DataKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("DataKey(..)")
    }
}

/// Where the key-encryption key lives: whatever wraps and unwraps the data keys.
///
/// The database never holds this key. An implementation may hold it in memory (a key file)
/// or never see it at all (a Transit engine that encrypts on request); the scheme for the
/// values is the same either way.
pub trait KeySource: Send + Sync {
    /// A short, stable name recorded at the front of every key it wraps (`file`), so a data
    /// key wrapped by one source is never handed to another.
    fn name(&self) -> &'static str;

    /// Wraps `key`, bound to its id `kid`. The text returned is what the database keeps.
    ///
    /// # Errors
    /// [`RepositoryError`] if the source cannot wrap.
    fn wrap(&self, kid: &str, key: &DataKey) -> Result<String, RepositoryError>;

    /// Unwraps what [`wrap`](Self::wrap) returned for `kid`.
    ///
    /// # Errors
    /// [`RepositoryError`] if the text is not this source's, or fails to authenticate.
    fn unwrap(&self, kid: &str, wrapped: &str) -> Result<DataKey, RepositoryError>;
}

/// A key-encryption key held in memory, sealing with XChaCha20-Poly1305: the cipher behind
/// both [`KeyFile`] and [`MemoryKey`].
struct LocalKek(DataKey);

impl LocalKek {
    fn wrap(&self, name: &str, kid: &str, key: &DataKey) -> Result<String, RepositoryError> {
        let aad = format!("endora-data-key:v1:{kid}");
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let sealed = XChaCha20Poly1305::new(self.0.as_bytes().into())
            .encrypt(
                &nonce,
                Payload {
                    msg: key.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| backend("could not wrap a data key"))?;
        Ok(format!(
            "{name}:v1:{}:{}",
            URL_SAFE_NO_PAD.encode(nonce),
            URL_SAFE_NO_PAD.encode(sealed)
        ))
    }

    fn unwrap(&self, name: &str, kid: &str, wrapped: &str) -> Result<DataKey, RepositoryError> {
        let prefix = format!("{name}:v1:");
        let Some(rest) = wrapped.strip_prefix(&prefix) else {
            let other = wrapped.split(':').next().unwrap_or_default();
            return Err(corrupt(format!(
                "data key {kid} was wrapped by the '{other}' key source, not '{name}'"
            )));
        };
        let (nonce, sealed) = rest
            .split_once(':')
            .ok_or_else(|| corrupt(format!("data key {kid} is not in the wrapped form")))?;
        let nonce = decode_nonce(nonce)
            .ok_or_else(|| corrupt(format!("data key {kid} has a damaged nonce")))?;
        let sealed = URL_SAFE_NO_PAD
            .decode(sealed)
            .map_err(|_| corrupt(format!("data key {kid} is damaged")))?;
        let aad = format!("endora-data-key:v1:{kid}");
        let opened = Zeroizing::new(
            XChaCha20Poly1305::new(self.0.as_bytes().into())
                .decrypt(
                    &nonce,
                    Payload {
                        msg: &sealed,
                        aad: aad.as_bytes(),
                    },
                )
                .map_err(|_| {
                    corrupt(format!(
                        "data key {kid} does not open with this key (a different key file?)"
                    ))
                })?,
        );
        DataKey::from_slice(&opened)
            .ok_or_else(|| corrupt(format!("data key {kid} is not 32 bytes")))
    }
}

/// The key-encryption key in a file of its own: `ENDORA_SECRETS_KEY_FILE`, beside the
/// database unless placed elsewhere (ADR 0125).
///
/// 32 bytes, made on first start and written owner-only (`0600`). Its own backup rule: keep a
/// copy **apart** from the database's backups, because the two together open everything and
/// either one alone opens nothing.
pub struct KeyFile(LocalKek);

impl KeyFile {
    /// Reads the key at `path`, or makes it there when `may_create` and it does not exist.
    ///
    /// The file holds the 32 bytes raw, or as standard base64 text (which is what a copy kept
    /// in a password manager comes back as). A file that is group- or world-readable is made
    /// owner-only again.
    ///
    /// `may_create` is false when the database already holds data keys: those were wrapped
    /// by a key that existed, and making a new one would quietly lose every secret.
    ///
    /// # Errors
    /// [`RepositoryError`] if the file is missing and may not be made, is not a 32-byte key,
    /// or cannot be read or written.
    pub fn open(path: &Path, may_create: bool) -> Result<Self, RepositoryError> {
        match std::fs::read(path) {
            Ok(bytes) => Self::from_file(path, &Zeroizing::new(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && may_create => Self::create(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(backend(format!(
                "the secrets key file {} is missing, and this database holds secrets sealed \
                 under it. Restore it from the copy you kept apart from the backups \
                 (docs/homelab.md, \"Secrets at rest\"); the node will not make a new one over \
                 sealed secrets.",
                path.display()
            ))),
            Err(e) => Err(backend(format!(
                "the secrets key file {} cannot be read: {e}",
                path.display()
            ))),
        }
    }

    fn from_file(path: &Path, bytes: &[u8]) -> Result<Self, RepositoryError> {
        let key = DataKey::from_slice(bytes).or_else(|| {
            let text = std::str::from_utf8(bytes).ok()?.trim();
            let decoded = Zeroizing::new(STANDARD.decode(text).ok()?);
            DataKey::from_slice(&decoded)
        });
        let Some(key) = key else {
            return Err(corrupt(format!(
                "the secrets key file {} is not a 32-byte key",
                path.display()
            )));
        };
        owner_only(path);
        Ok(Self(LocalKek(key)))
    }

    fn create(path: &Path) -> Result<Self, RepositoryError> {
        use std::io::Write as _;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(backend)?;
        }
        let key = DataKey::random();
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(path) {
            Ok(file) => file,
            // Another process made it first (a setup command beside a starting node): use
            // theirs rather than replace it.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Self::open(path, false);
            }
            Err(e) => return Err(backend(e)),
        };
        file.write_all(key.as_bytes()).map_err(backend)?;
        file.sync_all().map_err(backend)?;
        Ok(Self(LocalKek(key)))
    }
}

impl KeySource for KeyFile {
    fn name(&self) -> &'static str {
        "file"
    }
    fn wrap(&self, kid: &str, key: &DataKey) -> Result<String, RepositoryError> {
        self.0.wrap(self.name(), kid, key)
    }
    fn unwrap(&self, kid: &str, wrapped: &str) -> Result<DataKey, RepositoryError> {
        self.0.unwrap(self.name(), kid, wrapped)
    }
}

/// A key-encryption key that lives only in this process: for an in-memory database, whose
/// contents die with it anyway, and for tests. Never what a node on disk uses.
pub struct MemoryKey(LocalKek);

impl MemoryKey {
    /// A fresh random key.
    #[must_use]
    pub fn random() -> Self {
        Self(LocalKek(DataKey::random()))
    }
}

impl KeySource for MemoryKey {
    fn name(&self) -> &'static str {
        "memory"
    }
    fn wrap(&self, kid: &str, key: &DataKey) -> Result<String, RepositoryError> {
        self.0.wrap(self.name(), kid, key)
    }
    fn unwrap(&self, kid: &str, wrapped: &str) -> Result<DataKey, RepositoryError> {
        self.0.unwrap(self.name(), kid, wrapped)
    }
}

/// A column whose values are secrets, named for the migration and the rekey that walk it.
#[derive(Debug, Clone, Copy)]
pub struct SecretColumn {
    /// The table.
    pub table: &'static str,
    /// The column, `TEXT NOT NULL`; an empty string means "none" and is never sealed.
    pub column: &'static str,
}

impl SecretColumn {
    /// `table.column`, the only way a column is ever named in a message.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{}.{}", self.table, self.column)
    }
}

/// The data keys this database has, unwrapped, and which one seals new values.
pub(crate) struct Keyring {
    pub(crate) source: Arc<dyn KeySource>,
    pub(crate) active: String,
    pub(crate) keys: HashMap<String, DataKey>,
}

impl Keyring {
    pub(crate) fn seal(&self, plaintext: &str) -> Result<String, RepositoryError> {
        if plaintext.is_empty() {
            return Ok(String::new());
        }
        let key = self
            .keys
            .get(&self.active)
            .ok_or_else(|| backend("the active data key is not loaded"))?;
        let header = format!("{SEALED}{}", self.active);
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let sealed = XChaCha20Poly1305::new(key.as_bytes().into())
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: header.as_bytes(),
                },
            )
            .map_err(|_| backend("could not seal a value"))?;
        Ok(format!(
            "{header}:{}:{}",
            URL_SAFE_NO_PAD.encode(nonce),
            URL_SAFE_NO_PAD.encode(sealed)
        ))
    }

    pub(crate) fn unseal(&self, label: &str, stored: &str) -> Result<String, RepositoryError> {
        if stored.is_empty() {
            return Ok(String::new());
        }
        let Some(rest) = stored.strip_prefix(SEALED) else {
            let message = format!(
                "{label} holds a plaintext value where a sealed one belongs; it was written \
                 around the store, and is refused rather than used"
            );
            eprintln!("secrets: {message}");
            return Err(corrupt(message));
        };
        let mut parts = rest.splitn(3, ':');
        let (Some(kid), Some(nonce), Some(sealed)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(corrupt(format!("{label} holds a damaged sealed value")));
        };
        let key = self.keys.get(kid).ok_or_else(|| {
            corrupt(format!(
                "{label} is sealed under data key {kid}, which this database does not have"
            ))
        })?;
        let nonce =
            decode_nonce(nonce).ok_or_else(|| corrupt(format!("{label} has a damaged nonce")))?;
        let sealed = URL_SAFE_NO_PAD
            .decode(sealed)
            .map_err(|_| corrupt(format!("{label} holds a damaged sealed value")))?;
        let header = format!("{SEALED}{kid}");
        let opened = XChaCha20Poly1305::new(key.as_bytes().into())
            .decrypt(
                &nonce,
                Payload {
                    msg: &sealed,
                    aad: header.as_bytes(),
                },
            )
            .map_err(|_| {
                let message = format!("{label} did not authenticate: altered, or not ours");
                eprintln!("secrets: {message}");
                corrupt(message)
            })?;
        String::from_utf8(opened).map_err(|_| corrupt(format!("{label} is not text")))
    }
}

/// A key id: 8 random bytes as hex. Never contains `:`, which the sealed form splits on.
pub(crate) fn new_kid() -> String {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_nonce(text: &str) -> Option<XNonce> {
    let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
    (bytes.len() == 24).then(|| *XNonce::from_slice(&bytes))
}

/// Makes `path` owner-only if it is readable by anyone else. Best effort: a file the node
/// cannot change the mode of is still a file it can read, and says so.
pub(crate) fn owner_only(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = std::fs::metadata(path) else {
            return;
        };
        if !meta.is_file() || meta.permissions().mode() & 0o077 == 0 {
            return;
        }
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            eprintln!(
                "endora: {} is readable by others and could not be made owner-only ({e})",
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

// The tests sit beside this file, not in a `secrets/` folder: `.gitignore` keeps any folder of
// that name out of git, on purpose.
#[cfg(test)]
#[path = "secrets_tests.rs"]
mod tests;
