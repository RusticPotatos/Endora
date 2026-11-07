# 0144 — Endora keeps its own secrets; OpenBao is not part of Endora

## Status

Accepted, 2026-11-07, by the owner (decision recorded by the steward). Prepared with AI
assistance (Claude). Supersedes [0127](0127-openbao-and-the-passkey-unlock.md). Leaves
[0125](0125-secrets-at-rest-in-the-node.md) as the whole of how Endora keeps secrets.

## Context

ADR 0127 put OpenBao (the open-source fork of HashiCorp Vault) inside Endora: a client in the
node, passkey (WebAuthn PRF) unsealing from the console, a one-way switch moving the node's own
data keys into OpenBao's Transit engine, and a "waiting" start for a node whose keys were
locked in a sealed vault. Its 2026-11-07 amendment already moved the OpenBao service out to
ansible-nas's own `openbao` role and left the node as a client.

The owner's decision, 7 Nov 2026: *"that is my own like secrets, not endoras. endora should
keep and own its own. that integration seems def overkill."* OpenBao holds the owner's personal
home secrets. Endora keeping its secrets there coupled two things with different owners and
lifecycles, which is the one-responsibility rule 0127's own amendment cited for moving the
service out.

The evidence that this can be removed now: the vault never held an Endora key
(`keys_in_vault = 0`, [mechanism audit 2026-11-02, safety](../proposals/mechanism-audit-2026-11-02/safety.md));
since the amendment the house's vault was initialised at the terminal, so Endora had no shares,
no AppRole and no Transit key in it; and the console's passkey unlock had never run in a real
browser (0127, "Consequences").

### Pre-removal check: no node secret is stored in or read from the vault

Recorded 2026-11-07 against the code before this change (develop `e595ed5e`), so the removal
takes nothing the node uses.

- **The code path.** The node's store opened through `vault::open_node_store`, which used the
  vault only when a data key on disk was wrapped by the `openbao` source
  (`Db::key_sources_on_disk`); otherwise it opened `SqliteStore::open_sealed` under the key file
  (0125). A key gets that wrapping only from the owner's one-way switch (`keys_into_vault`,
  which sets `vault_state.keys_in_vault`). **KV was never read or written by the node**: `secret/`
  appears only in the AppRole policy text and the `sys/mounts/secret` mount at initialisation;
  no request for `secret/data/…` exists. The node's other requests were `sys/*` (status, init,
  unseal, rekey) and setup, none of which returns a node secret; Transit was the only path to
  one, and only behind the switch.
- **A test of that path**, run on `e595ed5e` and passing
  (`pre_removal_no_node_secret_is_stored_in_or_read_from_the_vault`, kept out of the tree because
  the code it tests is removed): with a vault configured, initialised and answering, and
  `keys_in_vault` false, two starts each open with the key file (not waiting), store and read a
  token, and leave every data key wrapped by `file`; the pretend vault, which records every
  Transit, KV, AppRole and token request, saw none (only the test's own `sys/init`).
- **The live state** the steward reported: `keys_in_vault` false and KV deferred, which is the
  case the test covers. If a node's keys had in fact been moved, this change refuses to start on
  it and changes nothing (Decision 5), so a mistaken deploy cannot lose them. A read-only check
  of a node's database before deploying, which prints source names and counts and never a key:

  ```sh
  sqlite3 -readonly endora.db "SELECT substr(wrapped, 1, instr(wrapped, ':') - 1) AS source, COUNT(*) FROM secret_keys GROUP BY source; SELECT keys_in_vault FROM vault_state;"
  ```

  Expected: `file|<n>` only, and `0`.

## Decision

1. **Endora keeps and owns its secrets in its built-in encrypted store** (0125): every
   credential column sealed with XChaCha20-Poly1305 under data keys wrapped by the key file
   `secrets.key`. That is the only key source the node opens.
2. **Endora has no OpenBao client and no OpenBao dependency, not even optional.** Removed: the
   OpenBao client and its in-memory fake (`endora_infrastructure::openbao`), the vault's store
   (`vault_store`), the node's vault module, its routes (`/v1/vault/*`, including the three that
   answered unsigned), the WebAuthn verifier (it served only the vault's passkeys; sign-in is a
   password and a TOTP code and is unchanged), the console's Vault screen and Settings entry,
   the "waiting for the vault" start, key-file notes (only the AppRole login used them),
   `ENDORA_VAULT_URL`, `ENDORA_VAULT_NETWORK`, `ENDORA_PASSKEY_RP_ID`, the compose file's vault
   network, and the `endora` role's `endora_vault_url`, `endora_vault_network` and
   `endora_passkey_rp_id`.
3. **OpenBao is the owner's personal home service**, declared by ansible-nas's own `openbao`
   role. Endora neither runs, reaches nor documents it beyond this note; its procedures live with
   that role.
4. **The database keeps its vault tables.** `vault_passkeys` and `vault_state` stay in an
   existing node's database, unread and unchanged; a new database does not get them. No
   migration drops them, so rolling back to the release before this one finds them as it left
   them.
5. **A database whose keys an older Endora moved into OpenBao is refused, not reset.** The node
   checks which source wrapped its data keys before opening them; anything but the key file
   stops the start with a message that names OpenBao, and nothing in the database is changed.
   No known install is in that state (point 2 of the Context).

## Consequences

- One way to keep a secret, one key file to back up apart from the database (`docs/recovery.md`).
- A leftover `ENDORA_VAULT_URL` or `ENDORA_VAULT_NETWORK` in `local.mk`, or a leftover role
  variable, is ignored; delete them. The node leaves the `openbao` network on its next deploy.
- The `KeySource` seam of 0125 stays (the key file and in-process keys use it; `rewrap` is
  tested with two key files), so a different source remains possible by a new ADR.
- `ring`, `chacha20poly1305`, `base64` and `zeroize` are no longer declared by the node, nor
  `base64` and `zeroize` by the infrastructure crate; the persistence crate keeps them.
- Tested: the node serves with no vault configuration and every vault route is a 404; the
  compose file, the Makefile and the role name no vault; the console has no vault screen; and
  the owner's scenarios, `app/node/features/secrets/endora-keeps-its-own-secrets.feature`.
