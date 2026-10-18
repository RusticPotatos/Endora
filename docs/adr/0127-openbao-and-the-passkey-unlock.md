# 0127 — OpenBao and the passkey unlock

## Status

Proposed, 2026-10-18, for the owner to accept at the merge. Prepared with AI assistance
(Claude). Builds the "path to OpenBao Transit" [0125](0125-secrets-at-rest-in-the-node.md)
left as a seam. Phase 1 of the house's secrets manager: the vault running, unlockable with a
passkey, and Endora's own keys able to move into it. Built: everything below except what
"Deferred" lists.

## Context

The home lab's credentials sit in many places, most of them plaintext (a names-only inventory,
private to the owner). The owner chose **OpenBao** — the open-source fork of HashiCorp Vault,
MPL-2.0 — as the one place they should live: agents see a secret's **name**, never its value,
and the node injects values where they are used.

A vault seals itself on every restart. Something has to unseal it, and the choices were poor:

- **By hand**, with key shares on the NAS's command line, after every reboot. Nobody does
  that at eleven at night.
- **From the Mac** (a launchd job with the shares in its Keychain). The Mac is a laptop and is
  often away; a vault that waits for a laptop is a vault that is locked.
- **Auto-unseal from another service.** Moves the problem; the other service's key has to live
  somewhere too.

The owner's choice: unseal with **the owner's passkey**. WebAuthn's **PRF extension** lets a
passkey (iCloud Keychain on iOS 18 and later, a recent browser, a security key with
`hmac-secret`) produce a stable 32-byte secret for a given salt, only after the owner's Face ID,
Touch ID or PIN. The unseal material is kept on the NAS **encrypted under a key derived from
that output**; the console shows "Vault locked → Unlock"; one tap unseals; the node decrypts,
unseals and forgets.

The console is served over HTTPS from a private CA the owner's devices trust, at
`https://endora.home.arpa:8787` and by the NAS's address. WebAuthn needs that secure context,
and **a passkey is bound to a name, not an address**: the relying party is `endora.home.arpa`.

## Decision

### 1. The service is declared by the collection, not by Endora's compose file

OpenBao runs from the `endora.homelab` collection's **`openbao` role**
(`deploy/ansible/collection/roles/openbao`), which the owner's ansible-nas playbook runs, like
every other piece of the lab's infrastructure. Endora's `docker-compose.yml` has no OpenBao
service; it only declares the network the two share and passes the node its address.

- The official image, **pinned by digest** (`openbao/openbao:2.7.1@sha256:6d2b…2acf`, the
  multi-architecture index), labelled out of watchtower: a new vault image is a decision.
- **Integrated (raft) storage**, single node, in `openbao_data_dir` (`<docker home>/openbao`,
  so the house's backup reaches it): no second service to run, and `bao operator raft snapshot
  save` makes a consistent copy. The **audit device** is declared in the configuration (a file,
  `audit.log`, in the same folder), so it exists from the first request, values HMAC'd.
- Read-only root, every capability dropped, `no-new-privileges`, **256 MB** with `GOMEMLIMIT`
  at 200 MiB (it idles near 20 MB; the NAS has 16 GB and has had memory pressure), mlock off
  (it would need `IPC_LOCK`).
- On **`endora_vault`**, a network Endora's compose file makes: internal, with an isolated
  gateway like the team's, joined by the node and the vault only. **No host port** unless
  `openbao_host_port` asks for one. The node reaches it as `http://openbao:8200`
  (`ENDORA_VAULT_URL`; the `endora` role's `endora_vault_url`).
- **Plain HTTP on that network.** TLS there would need a certificate the node trusts, minted
  and rotated: the PKI work below. What it would protect against is something on the host
  reading a private bridge between two containers, and anything that can do that is root on the
  NAS and can read both processes' memory anyway. The unseal shares cross it once per unlock.
- A health check that passes while it answers, sealed or not (`bao status` exits 2 when sealed,
  1 only when it cannot answer).
- Its unauthenticated **rekey** endpoints are on (OpenBao turns them off by default): the
  break-glass recovery (5) needs them, and only the node reaches the listener. Someone who
  could reach them could start or cancel a rekey, never finish one; that takes the shares.

### 2. Initialisation, once, by the node, with a passkey's tap

The owner registers a passkey, then taps **Initialise** (Settings › Vault). The node:

1. checks the passkey's assertion (2) and takes its PRF output;
2. `operator init` with **3 Shamir shares, threshold 2**;
3. makes a random 32-byte **unseal key**, seals all three shares under it
   (XChaCha20-Poly1305, as 0125's values are), and wraps the unseal key for that passkey;
   both are kept in the node's database **before** setup, so a failed setup still leaves a vault
   the passkey unlocks;
4. unseals, and with the root token: KV v2 at `secret/`, Transit at `transit/` with the key
   **`endora-node`** (derived: every request names its context), a policy `endora-node`
   (its own `secret/data/endora/*`, encrypt and decrypt with its own key, nothing in `sys/`,
   not rotating), and an AppRole `endora-node` holding only that policy;
5. **revokes the root token** — on success and on failure alike;
6. keeps the AppRole login (role id and secret id) as a note sealed by the key file (6);
7. answers with the three shares, **the only time they are shown**: the owner's break-glass
   copy, for the password manager or paper, with the warning that any two open the vault.

**Why 3 and 2.** Every share is wrapped under the passkeys together, so against a stolen
passkey the threshold adds nothing. It is for the break-glass copy: three shares the owner can
keep in different places, any two of which open the vault, so one lost is survivable and one
found opens nothing.

### 3. Passkeys, in the node

Owner-only routes register a passkey (`navigator.credentials.create` with `prf` enabled), list
them, and remove one — never the last one that can unlock. At most five. Each has its own
32-byte **PRF salt** and its **own wrapped copy** of the unseal key:

```text
kek     = HKDF-SHA256(ikm = PRF output, salt = the passkey's PRF salt,
                      info = "endora-vault:v1:unlock:<credential id>")
wrapped = "pk:v1:" nonce ":" XChaCha20-Poly1305(kek, unseal key,
                      aad = "endora-vault:v1:unseal-key:<credential id>")
shares  = "shares:v1:" nonce ":" XChaCha20-Poly1305(unseal key, JSON of the shares,
                      aad = "endora-vault:v1:shares")
```

**Not `webauthn-rs`.** It was the first choice: well maintained, and the obvious crate. It
verifies through OpenSSL, which this workspace keeps out (the node cross-compiles for the NAS
with rustls and ring), carries attestation and policy machinery a single owner does not use,
and does not hand over the PRF extension's output, which is the point. What is left is small
and fixed by the specification, and is checked in `app/node/src/webauthn.rs`: the client data
(type, a single-use two-minute challenge, an origin of `https://endora.home.arpa` on any port),
the authenticator data (relying-party hash, user **present and verified**, a counter that moves
forward unless it is a synced passkey's 0), and the signature (ES256 or Ed25519) with **`ring`**,
already in the build through rustls. A registration's COSE key is read with a definite-length
CBOR reader of about sixty lines. **Attestation is not checked**: `none` is asked for, and a
registration is accepted only from the signed-in owner, which is what decides whose passkey it
is. New dependencies: none; `ring`, `chacha20poly1305`, `base64` and `zeroize` were in the lock
file and are now declared by the node.

**The PRF output is not signed.** It comes back beside the assertion, not inside what the
authenticator signs. The signature proves the passkey was present for this challenge; the PRF
output proves itself, because a wrong one does not open the wrapped key.

**Another passkey** (an iPad, a security key) gets its own copy: an existing passkey's tap opens
the unseal key into a two-minute, in-memory, single-use **ticket**; the new passkey's tap wraps
it. Initialisation leaves such a ticket open for the passkeys already registered.

**Opened by its address**, a ceremony cannot work (the origin is not the relying party's), so
the console says to open `https://endora.home.arpa:<port>` instead of offering the button.

### 4. The unlock

On start the node asks the vault for its seal status; sealed, the console shows **Vault locked
→ Unlock**. The tap's options name every passkey that can unlock, each with its salt
(`evalByCredential`); the browser sends the assertion and the PRF output over the TLS session;
the node checks the assertion, derives the key, unwraps the unseal key, opens the shares, gives
the vault two, and **wipes all of it** (every key and share is held in `Zeroizing` buffers).

`GET /v1/vault`, `POST /v1/vault/unlock/begin` and `POST /v1/vault/unlock` are the **only**
vault routes that answer without a sign-in (`crate::auth::may_pass`). After a reboot with the
node's keys in the vault (5), the sign-in's own authenticator secret is one of the locked
things, so a sign-in cannot come first; the passkey — a signature over a single-use challenge,
and an output that must open the wrapped key — is a stronger credential than the sign-in it
stands in for. Attempts are **throttled** with the sign-in's own ladder (`crate::signin`), and
**every unlock, refused or not, is audited** (Activity and the audit trail): which passkey,
when, why refused — never a value.

### 5. Endora's own keys into the vault

`TransitKeys` (`endora_infrastructure::openbao`) is a 0125 `KeySource` named `openbao`: a data
key is wrapped by `transit/encrypt/endora-node` with its key id as the derived key's context,
unwrapped by `transit/decrypt`, logging in with the AppRole and once more if a token is refused.

**Moving the keys is the owner's switch**, Settings › Vault › Endora's own keys, with a one-way
warning: one `Db::rewrap` from the key file to Transit; **no value changes**. There is no route
back in the console. Until the owner flips it, the key file remains the source, as before.

From then on, a **fresh start while the vault is sealed opens the node waiting**
(`SqliteStore::open_waiting`): it starts, serves the console, and everything that needs a
stored credential fails saying "the vault is locked … unlock it in the console". The console
shows only the unlock. The unlock then opens the node's keys through Transit, seals anything an
older node left in plaintext, and reconnects the tool servers that could not start. A node that
already holds its data keys keeps working while the vault is sealed (a restart of the vault
alone); a restart of the node while the vault is open opens straight away.

**Recovery, every passkey lost.** The vault is unsealed on the NAS with two break-glass shares
(`docker exec -it endora-openbao bao operator unseal`, twice), the node restarted if it was
waiting, the owner signs in, registers a new passkey, and gives two shares under **Lost every
passkey?**. The vault **rekeys** from them: three new shares (shown once; the old ones stop
working), sealed under a new unseal key wrapped for the passkey that tapped, and every other
passkey's copy forgotten.

### 6. What stays outside the vault, and why

- **The unseal material.** It cannot be inside what it unlocks. It is on the node's data volume,
  under the owner's passkeys; the break-glass copy is the owner's.
- **The AppRole login.** The node needs it to reach Transit, which wraps the keys that open
  everything else the node keeps, so it cannot be sealed by those keys. It is a note sealed by
  the **key file** (0125), bound to its purpose. After the move, the key file opens that note and
  nothing else — and the login is worthless while the vault is sealed.
- **The TLS key** of the console. It is what the passkey ceremony runs over; a vault that has to
  be unlocked before the page that unlocks it can be served would never open.

## Threat model

| Who | What they get |
|---|---|
| **The NAS, stolen powered off** (disks, every volume, every backup on it) | The vault's storage, encrypted by its barrier, whose root key needs two shares; the shares, sealed under an unseal key wrapped only by passkeys; the AppRole note and the key file. **Before** the node's keys move: the key file and the database together still open Endora's own credentials (0125's backup rule is what protects those). **After**: nothing — the data keys need Transit, Transit needs an unsealed vault, unsealing needs a passkey's PRF output or two shares. |
| **The NAS, taken while running and unsealed** (root on the host, or any container holding the Docker socket) | Everything: the node's process memory holds its data keys, and OpenBao's holds its own. This decision protects copies and a powered-off box, not a live host (as 0125). |
| **The owner's phone** | Locked: nothing (the passkey needs Face ID or the passcode). Unlocked, with the passcode: an unlock of the vault from the house's network — which reveals no secret and no share to the browser — but no sign-in (password, and a code). Recovery: remove that passkey from another device; if it was the only one, the break-glass recovery, which also replaces the shares. An Apple account taken over is the same as the phone. |
| **The Mac, lost** | Nothing for the vault: nothing on it takes part in unsealing, by design. |
| **A backup of the data folder** | As the NAS powered off. Keep the break-glass shares apart from it, as 0125 keeps the key file apart from the database. |
| **Someone on the house's network** | The vault's status (one of five words). An unlock attempt needs a registered passkey; attempts are throttled and audited. The vault itself is reachable only from the node's network. |
| **A model, or an agent** | Nothing: no route hands a value to a model, and the vault's own routes are the owner's. Names-only access for agents is the next phase. |

## Deferred

- **Agents using secrets by name**: KV paths under `secret/endora/`, the node injecting values
  at use, never into model context, receipts or logs.
- **The NAS's environment secrets and ansible-nas's** into KV (agent-rendered `*_FILE`s,
  `community.hashi_vault` lookups), replacing literal defaults; the **database secrets engine**
  for the lab's databases; an **SSH CA**; and **PKI** replacing the private CA (and with it TLS
  on the vault's listener).
- Binding the AppRole's secret id and tokens to the vault network's addresses
  (`secret_id_bound_cidrs`), once that network has a fixed subnet.
- Rotating the audit log (it grows; it is small).

## The ansible-nas wiring

The fork needs, in `requirements.yml`, the collection pinned to the merge commit (0.14.0 or
later); in the playbook, after the Endora node's role:

```yaml
    - role: endora.homelab.endora
      tags: [endora]
    - role: endora.homelab.openbao
      tags: [openbao]
```

and in its variables (no secret among them):

```yaml
openbao_enabled: true
openbao_data_dir: "{{ docker_home }}/openbao"
endora_vault_url: http://openbao:8200
```

The node's role must run first on a fresh host: Endora's compose file makes the `endora_vault`
network the vault joins, and the `openbao` role refuses to start without it. For `make deploy`
from the Mac, `ENDORA_VAULT_URL = http://openbao:8200` in `local.mk` keeps the node's settings
in step with the role's.

## Consequences

- After a NAS reboot the owner taps once; the Mac is not involved.
- Passkey unlock works only at `https://endora.home.arpa:8787`, and the screen says so.
- Once the node's keys are in the vault, a restart of the node while the vault is sealed starts
  it waiting, and the console shows only the unlock until it opens. A restore needs the vault's
  data and the break-glass shares as well as the database and the key file.
- The node's database carries two more tables (`vault_passkeys`, `vault_state`).
- Tested: the passkey checks against a pretend authenticator signing with ring's own P-256 and
  Ed25519 keys and answering PRF as the specification says; the whole flow against an in-memory
  OpenBao; and, in ignored tests run against the pinned image on Docker, initialisation, setup,
  root revocation, Transit, sealing on restart, the node waiting and the passkey unlock opening
  it, and the rekey. A real browser ceremony (Face ID, the secure enclave, iCloud Keychain's
  PRF) cannot run in a test and has not been run.
