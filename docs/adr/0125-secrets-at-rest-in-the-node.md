# 0125 — Secrets at rest in the node

## Status

Proposed, 2026-10-18, for the owner to accept at the merge. Prepared with AI assistance
(Claude). Amends
[0098](0098-it-can-be-rebuilt.md) (a node's data is now its database **and** its secrets key,
kept apart) and the sign-in sessions of the console. Built: everything below.

## Context

A names-only inventory of the home lab's credentials (private to the owner) found the node's
SQLite database to be the largest single concentration of them, all **in plaintext columns**:
every model provider's API key, the MCP servers' bearer tokens and environments, the Home
Assistant long-lived token and the other services' keys and passwords in the capability
settings, the authenticator (TOTP) secret, and the bootstrap token. Console sessions were
stored as the tokens themselves, for thirty days. `docs/homelab.md` said so plainly: "not
encrypted". The file was `0600` only where a backup was taken; the database itself, and the
self-signed TLS private key, were written with the default umask.

So one file — or any copy of it: the nightly snapshot, the house's backup of the data folder, a
volume snapshot — handed over all cloud model spend, house control and console sign-in.

The owner plans OpenBao (an open-source Vault fork) later, whose Transit engine encrypts on
request without handing its key out. This design has to let that take over **without a data
migration of the scheme**.

## Decision

### 1. Sessions are kept as digests

`node_sessions.token` holds `sha256:<hex>` of a session token, never the token. The tokens
are 256 random bits, so a plain SHA-256 is enough; there is nothing to guess that a slow hash
would slow down. A request's offered token (the bearer header, or the query on the one route a
browser cannot sign) is hashed and looked up by its digest, then compared in constant time.
The bootstrap token is unchanged in kind: it has to be read back (the log prints it until
sign-in is set up; the console reveals it), so it is sealed (below) rather than hashed.

A session still lasts thirty days from sign-in. A **console** session (`full`) now also ends
after **seven days unused** (`last_seen_ms`, written at most once a minute). The checking
credential (`make live-smoke`, in a file on a laptop) and the pocket dashboard's are left
without an idle limit: they are used rarely by design, and idling them out would only break
the smoke suite and the dashboard.

On the first start, every session stored raw is hashed in place, guarded by the prefix (a
second start changes nothing), and given a last use of the upgrade's moment, so the upgrade
itself signs nobody out.

### 2. Credentials are sealed with an envelope scheme

A **data key** (32 random bytes, with a 16-hex-digit id) seals each secret value with
**XChaCha20-Poly1305**, from RustCrypto's `chacha20poly1305` (already in the lock file at the
same version for the handheld's vault, so no new crate). A sealed value is text:

```text
enc:v1:<kid>:<nonce, base64url>:<ciphertext and tag, base64url>
```

`enc:v1:<kid>` is the associated data, so the version and the key id cannot be swapped without
the tag failing. A fresh 24-byte random nonce each time. An empty string stays empty: it means
"none" all over the node. A value that is not sealed where one belongs is **refused** with an
error naming the column, never the value, and logged the same way: after the migration nothing
should be, and using one would be trusting whatever wrote it around the store.

The data keys live in the database, in `secret_keys`, each **wrapped** by a `KeySource` — the
key-encryption key is never in the database:

```rust
pub trait KeySource: Send + Sync {
    fn name(&self) -> &'static str;                       // recorded with each wrapped key
    fn wrap(&self, kid: &str, key: &DataKey) -> Result<String, RepositoryError>;
    fn unwrap(&self, kid: &str, wrapped: &str) -> Result<DataKey, RepositoryError>;
}
```

The one used today is a **key file**: 32 bytes in `ENDORA_SECRETS_KEY_FILE`, by default
`secrets.key` beside the database (`/data/secrets.key` in the container), made owner-only
(`0600`) on the first start. It wraps each data key with XChaCha20-Poly1305 too, bound to the
key's id. The file is made **only** on a database with no data keys: a missing key file over
sealed secrets stops the node with a message saying to restore it, because a new key would open
none of them. It may hold the bytes raw, or as base64 text (which is what a copy kept in a
password manager comes back as). A key file others can read is made owner-only again.

`shared/persistence` owns the scheme (`secrets.rs`): the handle every context shares (`Db`)
seals and unseals, so the capabilities context's settings store and the node's own sign-in
store use one implementation. In-memory databases (tests) get a key that lives only in the
process.

**The columns sealed** (`SECRET_COLUMNS`, `crates/endora-infrastructure/src/sqlite.rs`):

| Column | What it holds |
|---|---|
| `model_providers.api_key` | each model provider's API key (ADR 0105) |
| `deep_model.api_key` | the deep model's key (kept for the move into the lists) |
| `butler_model_config.api_key` | the old everyday model's key (same) |
| `mcp_servers.env` | a stdio MCP server's environment, where its credentials go |
| `mcp_servers.auth` | an http MCP server's bearer token (e.g. Home Assistant's) |
| `capability_settings.value` | **every** setting: the Home Assistant token, the *arr, dashboard and handheld services' API keys and passwords, Spotify's tokens, beside harmless settings |
| `node_auth.token` | the bootstrap token |
| `node_auth.totp_secret` | the authenticator secret |
| `node_auth.pending_totp` | a replacement authenticator secret waiting for its first code |

`capability_settings.value` is sealed whole rather than by the setting's name: credentials sit
beside harmless settings in that table, and a guess at which is which is how a new one would
slip through. Not sealed: `node_auth.password_hash` (Argon2id is already what a stolen copy
should face) and `node_sessions.token` (a digest, above).

**The migration.** On the first start every plaintext value in those columns is sealed in one
transaction, guarded by the prefix, so a second start changes nothing. When it sealed anything,
the file is then `VACUUM`ed, and from then on the connection runs with `secure_delete`, so the
plaintext does not survive in a free page for the next copy of the file to carry (a test reads
the file's bytes to prove it).

**Rotation.** `endora-node secrets rekey` makes a new data key, seals every sealed value again
under it and forgets the old one, all in one transaction; the key file does not change.
Moving the key-encryption key re-wraps the data keys and touches **no value** (`Db::rewrap`):
that is the reason for the envelope.

**The way out.** If the key file is lost and no copy survives, `endora-node secrets forget
--i-have-lost-the-key` empties every sealed value and the password and drops the data keys, so
the node can start again with a new key; sign-in is set up again from the bootstrap token the
log then prints, and every provider key, token and password is entered again.

### 3. Files the node writes with a key in them are owner-only

The TLS private key (`tls-key.pem`, with `ENDORA_TLS=1`) is created `0600` and one an older
node left readable is made so on start. The database file is made owner-only when opened. The
secrets key file is created `0600`. The nightly snapshots already were (`snapshot`). No other
file the node writes holds a key.

### 4. Never in a log or an answer

Errors and log lines on these paths name a column or a key id, never a value. Tests feed
placeholder secrets in through the API and the stores and look for them in every answer
(audit, activity, export, model lists, capabilities), in every cell of the database (the audit
trail, the event log and the usage log included) and in the error for a planted plaintext
value.

### The path to OpenBao Transit

A Transit `KeySource` wraps a data key by asking Transit to encrypt it (`transit/encrypt/<key>`
with the key id as context) and unwraps by asking it to decrypt; its `name()` is recorded with
each wrapped key, so the node never offers one source's keys to another. Moving to it is one
`rewrap` from the key file to Transit: no value changes, and Transit's own key rotation is a
re-wrap of a handful of rows. The node would then hold the data keys only in memory, unwrapped
at start by a service that keeps the key-encryption key to itself. Nothing for it is written
yet, deliberately: the trait is the seam, and a stub would only be code to keep.

## What this does not fix

- **The Docker socket.** Several containers on the NAS hold it read-write; one compromised
  container is root on the host, and root reads the key file and the database together. This
  protects copies of the database, not a live host.
- **The node's process memory.** A running node holds the data keys and, when it uses them, the
  secrets.
- **The rest of the lab.** Credentials in containers' environments, in the ansible-nas
  inventories and role defaults, SSH keys, and the team service's own database (its per-run
  tokens) are outside this decision. The inventory's suggested migration order applies: no-cost
  hygiene first, then OpenBao, then the file tokens, the environments, database credentials,
  and Transit for these columns last.
- **A key file kept beside the backups.** If the house's backup takes the data folder whole, it
  takes `secrets.key` with the database, and the two together open everything. The backup rule
  below is the other half of this decision.

## The backup rule

The database's backups and the secrets key are kept **apart**: either alone opens nothing, the
two together open everything. Keep a copy of `secrets.key` in the password manager (as base64
text), exclude it from the backup plan that covers the data folder, and restore it with the
database. `docs/homelab.md` ("Secrets at rest") and `docs/recovery.md` say how.

## Consequences

- One copy of the database, or of its backups, no longer hands over the house.
- A restore needs the key file as well as the database: a database restored without it starts
  nothing, and says what is missing.
- A new credential column must be added to `SECRET_COLUMNS` and read and written through
  `Db::unseal` / `Db::seal`; a value written around the store is refused, loudly, the first time
  it is read.
- Every database the node opens carries one more small table, `secret_keys`.
