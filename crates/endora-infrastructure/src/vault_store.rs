//! What the node keeps about the vault (ADR 0127), in its own two tables of the node's
//! database:
//!
//! - `vault_passkeys`: the owner's passkeys — public keys, the per-passkey PRF salt, and the
//!   **unseal key wrapped under that passkey's PRF output** (empty until it has been). A
//!   public key and a salt are not secrets; the wrapped key opens only with the passkey.
//! - `vault_state`: one row — the unseal shares sealed under the unseal key, the node's
//!   AppRole login sealed under the key file (a note, ADR 0125's key file), and whether the
//!   node's own data keys have moved into the vault.
//!
//! Nothing in either table is readable without either a passkey or the key file, and this
//! store never sees an unsealed share.

use endora_persistence::{Db, RepositoryError, backend};
use rusqlite::{OptionalExtension as _, params};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS vault_passkeys (
    credential_id TEXT PRIMARY KEY,
    label         TEXT NOT NULL,
    algorithm     INTEGER NOT NULL,
    public_key    BLOB NOT NULL,
    sign_count    INTEGER NOT NULL DEFAULT 0,
    prf_salt      BLOB NOT NULL,
    wrapped       TEXT NOT NULL DEFAULT '',
    created_ms    INTEGER NOT NULL,
    last_used_ms  INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS vault_state (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    shares         TEXT NOT NULL DEFAULT '',
    approle        TEXT NOT NULL DEFAULT '',
    keys_in_vault  INTEGER NOT NULL DEFAULT 0,
    initialised_ms INTEGER NOT NULL DEFAULT 0
) STRICT;
INSERT OR IGNORE INTO vault_state (id) VALUES (1);
";

/// One of the owner's passkeys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPasskey {
    /// The credential id, base64url.
    pub credential_id: String,
    /// What the owner called it ("iPhone").
    pub label: String,
    /// Its COSE algorithm: -7 (ES256) or -8 (`EdDSA`).
    pub algorithm: i64,
    /// Its public key: a P-256 point (65 bytes, uncompressed) or an Ed25519 key (32 bytes).
    pub public_key: Vec<u8>,
    /// The authenticator's signature counter as last seen (0 for synced passkeys).
    pub sign_count: u32,
    /// The salt its PRF is evaluated with (32 random bytes).
    pub prf_salt: Vec<u8>,
    /// The unseal key, wrapped under this passkey's PRF output; empty until it has been.
    pub wrapped: String,
    /// When it was registered.
    pub created_ms: i64,
    /// When it last unlocked, or 0.
    pub last_used_ms: i64,
}

/// The one row of vault state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VaultState {
    /// The unseal shares, sealed under the unseal key; empty until initialised.
    pub shares: String,
    /// The node's AppRole login, a note under the key file; empty until set up.
    pub approle: String,
    /// Whether the node's data keys are wrapped by the vault's Transit key.
    pub keys_in_vault: bool,
    /// When Endora initialised the vault, or 0.
    pub initialised_ms: i64,
}

/// The vault's tables, over the node's shared connection.
#[derive(Clone)]
pub struct VaultStore {
    db: Db,
}

impl VaultStore {
    /// The store, making its tables if they are not there.
    ///
    /// # Errors
    /// [`RepositoryError`] if the tables cannot be made.
    pub fn new(db: Db) -> Result<Self, RepositoryError> {
        db.lock()?.execute_batch(SCHEMA).map_err(backend)?;
        Ok(Self { db })
    }

    /// Every passkey, oldest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database fails.
    pub fn passkeys(&self) -> Result<Vec<StoredPasskey>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT credential_id, label, algorithm, public_key, sign_count, prf_salt, \
                 wrapped, created_ms, last_used_ms FROM vault_passkeys ORDER BY created_ms",
            )
            .map_err(backend)?;
        stmt.query_map([], |r| {
            Ok(StoredPasskey {
                credential_id: r.get(0)?,
                label: r.get(1)?,
                algorithm: r.get(2)?,
                public_key: r.get(3)?,
                sign_count: u32::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
                prf_salt: r.get(5)?,
                wrapped: r.get(6)?,
                created_ms: r.get(7)?,
                last_used_ms: r.get(8)?,
            })
        })
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)
    }

    /// One passkey by its credential id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database fails.
    pub fn passkey(&self, credential_id: &str) -> Result<Option<StoredPasskey>, RepositoryError> {
        Ok(self
            .passkeys()?
            .into_iter()
            .find(|p| p.credential_id == credential_id))
    }

    /// Adds a passkey.
    ///
    /// # Errors
    /// [`RepositoryError`] if it is already there or the database fails.
    pub fn add_passkey(&self, passkey: &StoredPasskey) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT INTO vault_passkeys (credential_id, label, algorithm, public_key, \
                 sign_count, prf_salt, wrapped, created_ms, last_used_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    passkey.credential_id,
                    passkey.label,
                    passkey.algorithm,
                    passkey.public_key,
                    i64::from(passkey.sign_count),
                    passkey.prf_salt,
                    passkey.wrapped,
                    passkey.created_ms,
                    passkey.last_used_ms
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Removes a passkey, and with it its wrapped copy of the unseal key. Returns whether it
    /// was there.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database fails.
    pub fn remove_passkey(&self, credential_id: &str) -> Result<bool, RepositoryError> {
        let removed = self
            .db
            .lock()?
            .execute(
                "DELETE FROM vault_passkeys WHERE credential_id = ?1",
                params![credential_id],
            )
            .map_err(backend)?;
        Ok(removed > 0)
    }

    /// Keeps the unseal key wrapped for one passkey.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database fails.
    pub fn set_wrapped(&self, credential_id: &str, wrapped: &str) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "UPDATE vault_passkeys SET wrapped = ?1 WHERE credential_id = ?2",
                params![wrapped, credential_id],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Notes a passkey's use: its counter and when.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database fails.
    pub fn used(
        &self,
        credential_id: &str,
        sign_count: u32,
        at_ms: i64,
    ) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "UPDATE vault_passkeys SET sign_count = ?1, last_used_ms = ?2 \
                 WHERE credential_id = ?3",
                params![i64::from(sign_count), at_ms, credential_id],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// The vault's state.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database fails.
    pub fn state(&self) -> Result<VaultState, RepositoryError> {
        let conn = self.db.lock()?;
        let found = conn
            .query_row(
                "SELECT shares, approle, keys_in_vault, initialised_ms FROM vault_state \
                 WHERE id = 1",
                [],
                |r| {
                    Ok(VaultState {
                        shares: r.get(0)?,
                        approle: r.get(1)?,
                        keys_in_vault: r.get::<_, i64>(2)? != 0,
                        initialised_ms: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(backend)?;
        Ok(found.unwrap_or_default())
    }

    /// Keeps the sealed shares, and when, at initialisation.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database fails.
    pub fn set_shares(&self, shares: &str, at_ms: i64) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "UPDATE vault_state SET shares = ?1, initialised_ms = ?2 WHERE id = 1",
                params![shares, at_ms],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Keeps the node's AppRole login (already sealed).
    ///
    /// # Errors
    /// [`RepositoryError`] if the database fails.
    pub fn set_approle(&self, sealed: &str) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "UPDATE vault_state SET approle = ?1 WHERE id = 1",
                params![sealed],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Records that the node's data keys are now wrapped by the vault.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database fails.
    pub fn set_keys_in_vault(&self) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute("UPDATE vault_state SET keys_in_vault = 1 WHERE id = 1", [])
            .map_err(backend)?;
        Ok(())
    }

    /// Forgets the vault's state and every passkey's wrapped copy, to set up a vault from
    /// scratch. Refused once the node's data keys are in the vault: forgetting then would
    /// lose them.
    ///
    /// # Errors
    /// [`RepositoryError`] if the keys are in the vault or the database fails.
    pub fn start_over(&self) -> Result<(), RepositoryError> {
        if self.state()?.keys_in_vault {
            return Err(RepositoryError::Backend(
                "the node's own keys are in this vault, so its state cannot be forgotten"
                    .to_owned(),
            ));
        }
        let mut conn = self.db.lock()?;
        let tx = conn.transaction().map_err(backend)?;
        tx.execute(
            "UPDATE vault_state SET shares = '', approle = '', initialised_ms = 0 WHERE id = 1",
            [],
        )
        .map_err(backend)?;
        tx.execute("UPDATE vault_passkeys SET wrapped = ''", [])
            .map_err(backend)?;
        tx.commit().map_err(backend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passkey(id: &str) -> StoredPasskey {
        StoredPasskey {
            credential_id: id.to_owned(),
            label: "Jane Doe's phone".to_owned(),
            algorithm: -7,
            public_key: vec![4; 65],
            sign_count: 0,
            prf_salt: vec![7; 32],
            wrapped: String::new(),
            created_ms: 1,
            last_used_ms: 0,
        }
    }

    #[test]
    fn passkeys_are_kept_used_wrapped_and_removed() {
        let store = VaultStore::new(Db::open_in_memory().unwrap()).unwrap();
        store.add_passkey(&passkey("cred-a")).unwrap();
        assert!(
            store.add_passkey(&passkey("cred-a")).is_err(),
            "one row per credential"
        );
        store.set_wrapped("cred-a", "placeholder-wrapped").unwrap();
        store.used("cred-a", 5, 99).unwrap();
        let got = store.passkey("cred-a").unwrap().unwrap();
        assert_eq!(got.wrapped, "placeholder-wrapped");
        assert_eq!((got.sign_count, got.last_used_ms), (5, 99));
        assert!(store.remove_passkey("cred-a").unwrap());
        assert!(!store.remove_passkey("cred-a").unwrap());
        assert!(store.passkeys().unwrap().is_empty());
    }

    #[test]
    fn the_state_is_one_row_and_can_start_over_until_the_keys_move_in() {
        let db = Db::open_in_memory().unwrap();
        let store = VaultStore::new(db.clone()).unwrap();
        assert_eq!(store.state().unwrap(), VaultState::default());
        store.add_passkey(&passkey("cred-a")).unwrap();
        store.set_wrapped("cred-a", "placeholder-wrapped").unwrap();
        store.set_shares("placeholder-shares", 5).unwrap();
        store.set_approle("placeholder-note").unwrap();
        let again = VaultStore::new(db).unwrap();
        assert_eq!(again.state().unwrap().shares, "placeholder-shares");
        again.start_over().unwrap();
        assert_eq!(again.state().unwrap(), VaultState::default());
        assert!(again.passkey("cred-a").unwrap().unwrap().wrapped.is_empty());
        again.set_keys_in_vault().unwrap();
        assert!(again.start_over().is_err());
    }
}
