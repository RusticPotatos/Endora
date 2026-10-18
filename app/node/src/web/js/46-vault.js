// ---- the vault and its passkey unlock (ADR 0127) ---------------------------------------
//
// OpenBao keeps the house's secrets, and after a reboot it is locked. One passkey tap here
// unlocks it: the browser asks the passkey for its PRF output, the node turns that into the
// key its unseal shares are kept under, unseals, and forgets. Passkeys bind to a NAME, not an
// address, so all of this works only when the console is opened at https://<rp_id>; opened
// by its IP, the screen says where to go instead of offering a button that cannot work.

let VAULT = null;            // GET /v1/vault: which state it is in (asked without a sign-in)
let VAULT_PASSKEYS = [];     // GET /v1/vault/passkeys: the owner's passkeys, no key material
let VAULT_SHARES = null;     // the break-glass shares, shown ONCE right after initialising
let VAULT_BUSY = "";         // what a passkey ceremony is doing now, for the button

const b64uToBuf = (s) => {
  const text = atob(String(s).replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(String(s).length / 4) * 4, "="));
  const out = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) out[i] = text.charCodeAt(i);
  return out.buffer;
};
const bufToB64u = (buf) => {
  const bytes = new Uint8Array(buf);
  let text = "";
  for (const b of bytes) text += String.fromCharCode(b);
  return btoa(text).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
};

// Opened somewhere a passkey cannot work: by an address, or without the name it is bound to.
function vaultWrongAddress() {
  return !!(VAULT && VAULT.rp_id && location.hostname && location.hostname !== VAULT.rp_id);
}
function vaultRightAddress() {
  const rp = (VAULT && VAULT.rp_id) || "endora.home.arpa";
  return `https://${rp}${location.port ? ":" + location.port : ""}`;
}

async function loadVault() {
  try { VAULT = await api("GET", "/v1/vault"); } catch (_) { VAULT = null; }
}
async function loadVaultPasskeys() {
  try { VAULT_PASSKEYS = (await api("GET", "/v1/vault/passkeys")).passkeys || []; } catch (_) { VAULT_PASSKEYS = []; }
}

// What to tell a browser that cannot do this at all.
function vaultNoPasskeys() {
  return !(window.PublicKeyCredential && navigator.credentials);
}

// navigator.credentials.get with the PRF extension, from the node's options.
async function vaultTap(options) {
  const evalBy = {};
  for (const [id, v] of Object.entries(options.extensions.prf.evalByCredential)) {
    evalBy[id] = { first: b64uToBuf(v.first) };
  }
  const cred = await navigator.credentials.get({ publicKey: {
    challenge: b64uToBuf(options.challenge),
    rpId: options.rpId,
    allowCredentials: options.allowCredentials.map((c) => ({ type: c.type, id: b64uToBuf(c.id) })),
    userVerification: options.userVerification,
    timeout: options.timeout,
    extensions: { prf: { evalByCredential: evalBy } },
  } });
  const prf = (cred.getClientExtensionResults() || {}).prf || {};
  return {
    challenge: options.challenge,
    id: bufToB64u(cred.rawId),
    client_data: bufToB64u(cred.response.clientDataJSON),
    authenticator_data: bufToB64u(cred.response.authenticatorData),
    signature: bufToB64u(cred.response.signature),
    prf: prf.results && prf.results.first ? bufToB64u(prf.results.first) : null,
  };
}

// navigator.credentials.create with the PRF extension, from the node's options.
async function vaultCreate(options) {
  const cred = await navigator.credentials.create({ publicKey: {
    ...options,
    challenge: b64uToBuf(options.challenge),
    user: { ...options.user, id: b64uToBuf(options.user.id) },
    excludeCredentials: options.excludeCredentials.map((c) => ({ type: c.type, id: b64uToBuf(c.id) })),
    extensions: { prf: { eval: { first: b64uToBuf(options.extensions.prf.eval.first) } } },
  } });
  const prf = (cred.getClientExtensionResults() || {}).prf || {};
  if (!prf.enabled) {
    throw new Error("this passkey can't do PRF, which unlocking needs (iOS 18 or later, a recent browser, or a security key with hmac-secret)");
  }
  return {
    challenge: options.challenge,
    id: bufToB64u(cred.rawId),
    client_data: bufToB64u(cred.response.clientDataJSON),
    attestation_object: bufToB64u(cred.response.attestationObject),
    prf: prf.results && prf.results.first ? bufToB64u(prf.results.first) : null,
  };
}

// An owner's tap for `purpose` (initialise, enrol, adopt), answered at `path`.
async function vaultCeremony(purpose, path, credential) {
  const options = await api("POST", "/v1/vault/ceremony", { purpose, credential: credential || null });
  return api("POST", path, await vaultTap(options));
}

// The wrong-address note, or nothing.
function vaultAddressNote() {
  if (!vaultWrongAddress()) return "";
  return `<div class="sub" style="margin-top:8px;">Passkeys work only at <a href="${esc(vaultRightAddress())}">${esc(vaultRightAddress())}</a> — this page was opened by its address. Open it there to use a passkey.</div>`;
}

// The strip across the top of every screen while the vault is locked.
function vaultBannerHtml() {
  if (!VAULT || VAULT.vault !== "locked" || !VAULT.set_up || VAULT.node_waiting) return "";
  const button = vaultWrongAddress()
    ? ""
    : `<button class="primary" data-act="vault:unlock"${VAULT_BUSY ? " disabled" : ""}>${VAULT_BUSY ? esc(VAULT_BUSY) : "Unlock"}</button>`;
  return `
    <div class="card" style="margin-bottom:12px;border-color: color-mix(in srgb, var(--accent) 40%, var(--line));">
      <div class="row"><div class="grow"><div class="title">${icon("lock", 15)} Vault locked</div>
      <div class="sub">The house's secrets are locked since the last restart. One tap of your passkey opens them.</div></div>
      ${button}</div>
      ${vaultAddressNote()}
    </div>`;
}

// The whole screen when the node itself waits for the vault: nothing else can load until it
// opens, sign-in included (its authenticator secret is one of the locked things).
function viewVaultLocked() {
  const unlock = vaultWrongAddress()
    ? ""
    : `<div style="margin-top:12px;"><button class="primary" data-act="vault:unlock"${VAULT_BUSY ? " disabled" : ""}>${VAULT_BUSY ? esc(VAULT_BUSY) : "Unlock with your passkey"}</button></div>`;
  return `
    <h2>${icon("lock")} Vault locked</h2>
    <div class="card">
      <div class="note">Endora restarted and its keys are in the vault, which locks itself on a restart. Unlock it with your passkey — Face ID or Touch ID — and everything comes back.</div>
      ${unlock}
      ${vaultAddressNote()}
      <details style="margin-top:12px;"><summary class="sub">No passkey to hand?</summary>
        <div class="sub" style="margin-top:6px;">Use another registered passkey (a tablet, a security key), or the break-glass shares you saved when the vault was set up: docs/homelab.md, "The vault", says how.</div>
      </details>
    </div>`;
}

function vaultStateWords() {
  if (!VAULT) return "Not known yet.";
  const words = {
    "not configured": "No vault is configured on this node (ENDORA_VAULT_URL is not set).",
    unreachable: "The vault is not answering.",
    uninitialised: "The vault is running and has not been set up yet.",
    locked: "Locked. A passkey tap unlocks it.",
    unlocked: "Unlocked.",
  };
  return words[VAULT.vault] || VAULT.vault;
}

// Settings › Vault.
function viewVault() {
  const v = VAULT || { vault: "not configured", set_up: false, passkeys: 0, can_unlock: 0 };
  const shares = VAULT_SHARES
    ? `<div class="card" style="border-color: color-mix(in srgb, var(--warn, #c60) 55%, var(--line));">
        <div class="title" style="margin-bottom:6px;">Save these now — they are shown once</div>
        <div class="sub" style="margin-bottom:8px;">Your break-glass copy: any <b>two</b> of these three unlock the vault if every passkey is lost. Put them in your password manager (or on paper, apart). Endora will never show them again, and anyone with two of them can open the vault.</div>
        ${VAULT_SHARES.map((s, i) => `<div class="sub" style="word-break:break-all;user-select:all;font-family:ui-monospace,monospace;margin-bottom:6px;">${i + 1}. ${esc(s)}</div>`).join("")}
        <button class="primary" data-act="vault:saved">I've saved them</button>
      </div>`
    : "";
  const rows = VAULT_PASSKEYS.map((p) => `
    <div class="row" style="gap:8px;margin-bottom:10px;">
      <div class="grow"><div class="title" style="font-weight:500;">${esc(p.label)}</div>
        <div class="sub">${p.can_unlock ? "can unlock" : "cannot unlock yet"}${p.last_used_ms ? " · last used " + esc(new Date(p.last_used_ms).toLocaleString()) : ""}</div></div>
      ${!p.can_unlock && v.set_up ? `<button class="ghost" data-act="vault:adopt:${esc(p.id)}">Let it unlock</button>` : ""}
      <button class="ghost danger" data-act="vault:remove:${esc(p.id)}">Remove</button>
    </div>`).join("");
  const initialise = v.vault === "uninitialised" && !v.set_up && v.passkeys > 0 && !vaultWrongAddress()
    ? `<div class="row" style="margin-top:10px;"><button class="primary" data-act="vault:initialise">Initialise the vault</button></div>
       <div class="sub" style="margin-top:6px;">One tap of a passkey. The vault is set up, its root token is revoked, and you are shown its break-glass shares once.</div>`
    : "";
  const add = vaultWrongAddress()
    ? vaultAddressNote()
    : `<div class="form" style="margin-top:10px;">
         <input id="vault-label" type="text" autocomplete="off" placeholder="what to call it (iPhone, iPad, security key)" />
         <button class="ghost" data-act="vault:register">Add a passkey</button>
       </div>
       <div class="sub" style="margin-top:6px;">${v.set_up ? "A passkey that already unlocks is asked first, then the new one twice." : "Add yours first, then initialise."}</div>`;
  const keys = v.set_up && v.vault === "unlocked"
    ? `<details style="margin-top:14px;"><summary class="sub">Endora's own keys</summary>
        <div class="card" style="margin-top:8px;">
          <div class="sub" style="margin-bottom:8px;">${v.keys_in_vault
            ? "Endora's stored credentials are sealed under keys the vault wraps: the key file alone no longer opens them, and after a restart they wait for the unlock."
            : "Endora's stored credentials are sealed under keys its key file wraps. Moving those keys into the vault means a copy of the NAS's disk opens nothing without the vault — and that <b>after every restart, Endora waits for your passkey</b>. This cannot be undone from here."}</div>
          ${v.keys_in_vault ? "" : `<button class="ghost danger" data-act="vault:keys">Move Endora's keys into the vault</button>`}
        </div>
      </details>`
    : "";
  const recover = v.set_up && v.vault === "unlocked" && !vaultWrongAddress()
    ? `<details style="margin-top:8px;"><summary class="sub">Lost every passkey?</summary>
        <div class="card" style="margin-top:8px;">
          <div class="sub" style="margin-bottom:8px;">With two of the break-glass shares you saved, a new passkey can unlock again. The vault makes three new shares (shown once — save them; the old ones stop working) and every other passkey's copy is forgotten. If the vault is locked, unlock it on the NAS with two shares first: docs/homelab.md, "The vault", says how.</div>
          <div class="form" style="flex-direction:column;align-items:stretch;gap:8px;">
            <input id="vault-share-1" type="password" autocomplete="off" placeholder="a break-glass share" />
            <input id="vault-share-2" type="password" autocomplete="off" placeholder="another one" />
            <input id="vault-recover-label" type="text" autocomplete="off" placeholder="what to call the new passkey" />
            <button class="ghost" data-act="vault:recover">Set up a new passkey with them</button>
          </div>
        </div>
      </details>`
    : "";
  const over = v.set_up && !v.keys_in_vault
    ? `<details style="margin-top:8px;"><summary class="sub">Start over</summary>
        <div class="card" style="margin-top:8px;"><div class="sub" style="margin-bottom:8px;">Forgets this node's copy of the vault's shares and login, for a vault whose data you have wiped. The vault itself is not touched.</div>
        <button class="ghost danger" data-act="vault:startover">Forget the vault's state</button></div>
      </details>`
    : "";
  return `
    ${settingsCrumbs("vault")}
    <h2>Vault</h2>
    ${shares}
    ${vaultBannerHtml()}
    <div class="card">
      <div class="title" style="font-weight:500;">${icon("lock", 15)} OpenBao</div>
      <div class="sub">${esc(vaultStateWords())}</div>
      ${initialise}
    </div>
    <h3 style="margin-top:22px;">Passkeys</h3>
    <div class="card">
      ${listOr(rows, "No passkeys yet.")}
      ${add}
    </div>
    ${keys}
    ${recover}
    ${over}`;
}

async function vaultDispatch(verb, noun, id) {
  if (verb === "vault") return vaultAct(noun, id);
  return false;
}

async function vaultAct(noun, id) {
  if (noun === "saved") { VAULT_SHARES = null; render(); return true; }
  if (["unlock", "register", "initialise", "adopt", "recover"].includes(noun) && vaultNoPasskeys()) {
    flash("This browser can't use passkeys here. Open " + vaultRightAddress() + " in Safari or Chrome.", "warn");
    return true;
  }
  let done = false;
  try {
    if (noun === "unlock") {
      VAULT_BUSY = "Unlocking…"; render();
      const options = await api("POST", "/v1/vault/unlock/begin", {});
      await api("POST", "/v1/vault/unlock", await vaultTap(options));
      flash("The vault is unlocked.", "ok");
    } else if (noun === "register") {
      const field = document.getElementById("vault-label");
      const label = field ? field.value.trim() : "";
      if (VAULT && VAULT.set_up && VAULT.can_unlock > 0 && !VAULT.adopting) {
        VAULT_BUSY = "Your existing passkey first…"; render();
        await vaultCeremony("enrol", "/v1/vault/enrol");
      }
      VAULT_BUSY = "The new passkey…"; render();
      const added = await api("POST", "/v1/vault/passkeys/finish",
        await vaultCreate(await api("POST", "/v1/vault/passkeys/begin", { label })));
      if (VAULT && VAULT.set_up && VAULT.can_unlock > 0 && !added.can_unlock) {
        VAULT_BUSY = "Once more, with the new passkey…"; render();
        await vaultCeremony("adopt", "/v1/vault/adopt", added.id);
      }
      flash("Passkey added.", "ok");
    } else if (noun === "adopt") {
      if (!(VAULT && VAULT.adopting)) {
        VAULT_BUSY = "A passkey that already unlocks first…"; render();
        await vaultCeremony("enrol", "/v1/vault/enrol");
      }
      VAULT_BUSY = "Now the passkey to let in…"; render();
      await vaultCeremony("adopt", "/v1/vault/adopt", id);
      flash("That passkey can unlock the vault now.", "ok");
    } else if (noun === "initialise") {
      VAULT_BUSY = "Initialising…"; render();
      const made = await vaultCeremony("initialise", "/v1/vault/initialise");
      VAULT_SHARES = made.shares || [];
    } else if (noun === "recover") {
      const shares = ["vault-share-1", "vault-share-2"].map((f) => {
        const el = document.getElementById(f);
        return el ? el.value.trim() : "";
      }).filter(Boolean);
      if (shares.length < 2) { flash("Two of the break-glass shares are needed.", "warn"); return true; }
      const field = document.getElementById("vault-recover-label");
      VAULT_BUSY = "Your new passkey…"; render();
      const added = await api("POST", "/v1/vault/passkeys/finish",
        await vaultCreate(await api("POST", "/v1/vault/passkeys/begin", { label: field ? field.value.trim() : "" })));
      VAULT_BUSY = "Once more, with the new passkey…"; render();
      const options = await api("POST", "/v1/vault/ceremony", { purpose: "recover", credential: added.id });
      const made = await api("POST", "/v1/vault/recover", { shares, ...(await vaultTap(options)) });
      VAULT_SHARES = made.shares || [];
    } else if (noun === "remove") {
      if (!confirm("Remove this passkey? It will no longer unlock the vault.")) return true;
      await api("POST", `/v1/vault/passkeys/${id}/remove`, {});
    } else if (noun === "keys") {
      if (!confirm("Move Endora's keys into the vault? After every restart Endora will wait for your passkey, and the key file alone will no longer open its data. This cannot be undone from the console.")) return true;
      await api("POST", "/v1/vault/keys-into-vault", { confirm: true });
      flash("Endora's keys are in the vault.", "ok");
    } else if (noun === "startover") {
      if (!confirm("Forget this node's vault state? Only for a vault whose data you have wiped.")) return true;
      await api("POST", "/v1/vault/start-over", { confirm: true });
    }
    done = true;
  } catch (e) {
    flash("Vault: " + (e.message || e), "warn");
  } finally {
    VAULT_BUSY = "";
  }
  await loadVault();
  if (!(VAULT && VAULT.node_waiting)) await loadVaultPasskeys();
  // Unlocked: everything the locked node could not answer can load now.
  if (noun === "unlock" && done) { location.reload(); return true; }
  render();
  return true;
}
