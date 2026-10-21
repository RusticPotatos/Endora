function render() {
  // The node itself is waiting for the vault (ADR 0127): nothing else can load until it
  // opens, sign-in included, so the unlock is the whole screen.
  if (VAULT && VAULT.node_waiting) { app.innerHTML = viewVaultLocked(); return; }
  // Nothing else can load without a token, so nothing else is worth drawing.
  if (NEEDS_TOKEN) { app.innerHTML = viewNeedsToken(); return; }
  const v = NAV.v;
  // A redraw of the same screen (a live update, a decision) keeps where the person was: the
  // page's scroll, and each box marked data-keep-scroll. Restored before the browser paints,
  // so nothing visibly jumps.
  const kept = v === LAST_VIEW
    ? { y: window.scrollY, boxes: [...document.querySelectorAll("[data-keep-scroll]")].map((el) => [el.dataset.keepScroll, el.scrollTop]) }
    : null;
  app.innerHTML =
      v === "audit" ? viewAudit()
    : v === "skills" ? viewSkills()
    : v === "skill" ? viewSkill()
    : v === "connections" ? viewConnections()
    : v === "toolserver" ? viewToolServer()
    : v === "data" ? viewData()
    : v === "spotify" ? viewSpotify()
    : v === "lab" ? viewLab()
    : v === "hq" ? viewHQ()
    : v === "replay" ? viewReplay()
    : v === "containers" ? viewContainers()
    : v === "logs" ? viewLogs()
    : v === "jobs" ? viewJobs()
    : v === "job" ? viewJob()
    : v === "media" ? viewMedia()
    : v === "house" ? viewHouse()
    : v === "music" ? viewMusic()
    : v === "speakers" ? viewSound()
    : v === "settings" ? viewSettings()
    : v === "models" ? viewModels()
    : v === "signin" ? viewSignin()
    : v === "vault" ? viewVault()
    // A screen nobody knows any more opens HQ.
    : viewHQ();
  if (kept) {
    for (const [name, top] of kept.boxes) {
      const box = document.querySelector(`[data-keep-scroll="${name}"]`);
      if (box) box.scrollTop = top;
    }
    window.scrollTo(0, kept.y);
  }
  if (v !== LAST_VIEW) {
    // Every screen is a list with the newest thing FIRST, so it opens at the top. Without
    // this the window simply keeps whatever scroll position the last screen had.
    //
    // Only when the screen actually changes. Doing it on every render would yank
    // someone back to the top mid-read whenever a live update arrived.
    requestAnimationFrame(() => window.scrollTo({ top: 0 }));
  }
  LAST_VIEW = v;
}

// ---- actions (event delegation) -------------------------------------------
async function dispatch(act) {
  const [verb, noun, id, arg] = act.split(":");
  try {
    if (verb === "reload") { location.reload(); return; }
    // The home lab, media, smart home and music.
    if (await labDispatch(verb, noun, id, arg)) return;
    if (await hqDispatch(verb, noun, id, arg)) return;
    if (await replayDispatch(verb, noun, id)) return;
    if (await soundDispatch(verb, noun, id, arg)) return;
    // The vault and its passkeys (ADR 0127).
    if (await vaultDispatch(verb, noun, id)) return;
    // Spotify: one sign-in, held by the node, started or finished from any device.
    if (verb === "spotify") {
      try {
        if (noun === "begin") {
          const field = document.getElementById("spotify-client");
          SPOTIFY = await api("POST", "/v1/connections/spotify/begin", { client_id: field ? field.value : "", via: id });
          // Approving here: go straight to Spotify; it sends you back to Endora.
          if (id === "endora" && SPOTIFY.sign_in_url) { location.href = SPOTIFY.sign_in_url; return; }
        } else if (noun === "finish") {
          const field = document.getElementById("spotify-returned");
          SPOTIFY = await api("POST", "/v1/connections/spotify/finish", { returned: field ? field.value : "" });
          flash(SPOTIFY.detail, "ok");
        } else if (noun === "disconnect") {
          if (!confirm("Disconnect Spotify from Endora? Every device loses it until you connect again.")) return;
          SPOTIFY = await api("POST", "/v1/connections/spotify/disconnect", {});
        } else {
          SPOTIFY = await api("GET", "/v1/connections/spotify");
        }
      } catch (e) { flash("Spotify: " + e.message, "err"); }
      return render();
    }
    if (verb === "go") {
      if (noun === "audit") return go("audit");
      return go(noun, id);
    }
    // The Activity screen opens with the newest thirty; this asks for the next page, by the
    // number already on screen, and says so when there is nothing older.
    if (verb === "activity" && noun === "more") {
      if (ACTIVITY_BUSY) return;
      ACTIVITY_BUSY = true;
      render();
      try {
        const more = await api("GET", `/v1/activity?limit=${ACTIVITY_PAGE}&offset=${ACTIVITY.length}`);
        ACTIVITY = ACTIVITY.concat(more);
        ACTIVITY_MORE = more.length >= ACTIVITY_PAGE;
      } finally {
        ACTIVITY_BUSY = false;
      }
      return render();
    }
    if (verb === "toggle" && noun === "menu") {
      const m = document.getElementById("menu");
      if (m) m.hidden = !m.hidden;
      return;
    }
    // Sign in, and keep whatever the node hands back. Nothing is stored until it works.
    if (verb === "signin" && !noun) {
      const password = val("signin-password");
      const code = val("signin-code");
      if (!password || !code) return;
      const res = await fetch("/v1/session", {
        method: "POST",
        headers: signed({ "content-type": "application/json" }),
        body: JSON.stringify({ password, code }),
      });
      const data = await res.json().catch(() => null);
      if (!res.ok) return flash((data && data.error) || "That didn't match.", "warn");
      try { localStorage.setItem("endora-token", data.token); } catch (_) {}
      NEEDS_TOKEN = false;
      return reload();
    }
    // Choose a password. On a node nobody has claimed this needs no credential, which is the
    // whole point — and the reply carries the authenticator link, because the endpoint that
    // would otherwise serve it needs a credential the claimant does not have yet.
    if (verb === "signin" && noun === "setup") {
      const password = val("setup-password");
      if (!password) return;
      const res = await fetch("/v1/session/setup", {
        method: "POST",
        headers: signed({ "content-type": "application/json" }),
        body: JSON.stringify({ password }),
      });
      const data = await res.json().catch(() => null);
      if (!res.ok) return flash((data && data.error) || "That didn't work.", "warn");
      JUST_ENROLLED = (data && data.otpauth) || "";
      JUST_ENROLLED_QR = (data && data.qr) || "";
      SIGNIN_EXISTS = true;
      NEEDS_TOKEN = true;
      return render();
    }
    if (verb === "enrolled" && noun === "done") {
      JUST_ENROLLED = "";
      JUST_ENROLLED_QR = "";
      return render();
    }
    if (verb === "password" && noun === "change") {
      const current = val("pw-current"), next = val("pw-new");
      if (!current || !next) return;
      try {
        await api("POST", "/v1/session/password", { current, new: next });
        flash("Password changed.", "ok");
      } catch (e) { flash(String(e.message || e), "warn"); }
      return render();
    }
    if (verb === "auth" && noun === "begin") {
      const current = val("rotate-password");
      if (!current) return;
      try {
        const made = await api("POST", "/v1/session/authenticator", { current });
        NEW_AUTHENTICATOR = made.otpauth || "";
        NEW_AUTHENTICATOR_QR = made.qr || "";
      } catch (e) { flash(String(e.message || e), "warn"); }
      return render();
    }
    if (verb === "auth" && noun === "confirm") {
      const code = val("rotate-code");
      if (!code) return;
      try {
        await api("POST", "/v1/session/authenticator/confirm", { code });
        NEW_AUTHENTICATOR = ""; NEW_AUTHENTICATOR_QR = "";
        flash("That's your authenticator now. The old one has stopped working.", "ok");
      } catch (e) { flash(String(e.message || e), "warn"); }
      return render();
    }
    if (verb === "token" && noun === "checks") {
      const made = await api("POST", "/v1/session/checks", {});
      DEV_TOKEN = made.token || "";
      DEV_TOKEN_NOTE = made.note || "";
      return render();
    }
    if (verb === "token" && noun === "recovery") {
      const got = await api("GET", "/v1/session/bootstrap");
      DEV_TOKEN = got.token || "";
      DEV_TOKEN_NOTE = "This is the recovery token. It never expires — keep it somewhere safe, not in a file.";
      return render();
    }
    // Keep the node's token on this device. Nothing is sent anywhere to check it — the next
    // request either works or comes back 401 and puts this screen straight back up.
    if (verb === "token") {
      const t = val("token-input");
      if (!t) return;
      try { localStorage.setItem("endora-token", t); } catch (_) {}
      NEEDS_TOKEN = false;
      return reload();
    }
    // Turn a skill on or off (ADR 0054). `id` is the capability id; `arg` is 1/0.
    // Put back one change Endora made to a service's own settings (ADR 0054).
    if (verb === "unwrite") {
      try {
        const r = await api("POST", `/v1/config-writes/${id}/undo`, {});
        flash(r.undone || "Put back.", "ok");
      } catch (e) { flash("Couldn't put that back: " + e.message, "err"); }
      return reload();
    }
    if (verb === "skill" && noun === "enable") {
      const enabled = arg === "1";
      try {
        await api("POST", `/v1/capabilities/${id}/enable`, { enabled });
        // Worth saying out loud: turning a tool off changes what the butler is even
        // shown, and the way back is not obvious unless we name it (ADR 0054).
        flash(enabled
          ? `Offering ${id} again.`
          : `No longer offering ${id}. You can turn it back on under Skills.`, "ok");
      }
      catch (e) { flash("Couldn't change that skill: " + e.message, "err"); }
      return reload();
    }
    // Open or re-block a skill's irreversible actions (ADR 0051). `arg` is 1/0.
    // Opening only ever moves it from blocked to confirm-each-use — never to
    // autonomous — so we confirm the intent, not fake a bigger promise.
    if (verb === "skill" && noun === "open") {
      const open = arg === "1";
      if (open && !confirm("Allow this skill's irreversible actions?\n\nEndora will still ask you before every single use, and will never do it on its own.")) return;
      try { await api("POST", `/v1/capabilities/${id}/open`, { open }); }
      catch (e) { flash("Couldn't change that skill: " + e.message, "err"); }
      return reload();
    }
    // Set a skill to "ask first" (on with user input) or back to automatic (ADR 0051).
    if (verb === "skill" && noun === "confirm") {
      const wantConfirm = arg === "1";
      try { await api("POST", `/v1/capabilities/${id}/confirm`, { confirm: wantConfirm }); flash(wantConfirm ? "Endora will ask before using this." : "Endora may use this on its own.", "ok"); }
      catch (e) { flash("Couldn't change that skill: " + e.message, "err"); }
      return reload();
    }
    // Ask the hub what exists that would fit. Never fetches — see worthKnowingSection.
    if (verb === "models" && noun === "look") {
      const gb = Number((document.getElementById("vram-gb") || {}).value || 12);
      try {
        const r = await api("GET", `/v1/models/worth-knowing?fits_gb=${gb}`);
        WORTH_KNOWING = { models: r.models || [], fits_gb: r.fits_gb || gb, asked: true };
        flash(`${(r.models || []).length} that would fit.`, "ok");
      } catch (e) { flash("Couldn't reach the hub: " + e.message, "err"); }
      return render();
    }
    // Search the MCP catalog (curated + community registry).
    if (verb === "mcp" && noun === "search") { await mcpSearch(); return; }
    // Add an MCP server (ADR 0054): a local stdio command or an HTTP endpoint. Its
    // tools appear blocked until allowed. A colon in the name would break the action
    // encoding, so reject it.
    if (verb === "mcp" && noun === "add") {
      const name = val("mcp-name");
      const transport = (document.getElementById("mcp-transport") || {}).value || "stdio";
      if (!name) { flash("Enter a name.", "err"); return; }
      // A name is the namespace for every tool the server exposes (`server.tool`), and
      // both of these break resolving it — a dot splits the namespace in the wrong place
      // and hides every tool.
      if (/[.:]/.test(name)) {
        flash("The name can't contain a dot or a colon — it's the prefix for this server's tools.", "err");
        return;
      }
      let body;
      if (transport === "http") {
        const url = val("mcp-url");
        if (!url) { flash("Enter the endpoint URL.", "err"); return; }
        body = { name, transport: "http", url };
        const auth = (document.getElementById("mcp-auth") || {}).value || "";
        if (auth.trim()) body.auth = auth.trim();
      } else {
        const command = val("mcp-command");
        const args = val("mcp-args").split("\n").map((a) => a.trim()).filter(Boolean);
        if (!command) { flash("Enter a command.", "err"); return; }
        // The declared fields first, then anything typed under Advanced — so a raw line
        // can still override a declared one rather than being silently ignored.
        const env = {};
        for (const el of document.querySelectorAll("[data-need]")) {
          const v = (el.value || "").trim();
          if (v) env[el.getAttribute("data-need")] = v;
        }
        for (const line of val("mcp-env").split("\n")) {
          const i = line.indexOf("=");
          if (i <= 0) continue;
          const k = line.slice(0, i).trim();
          if (k) env[k] = line.slice(i + 1).trim();
        }
        // A declared variable left blank is the likeliest reason a server connects to
        // nothing, and it is worth catching here rather than two minutes later.
        const missing = (MCP_NEEDS.fields || [])
          .filter((f) => !env[f.key])
          .map((f) => f.label || f.key);
        if (missing.length && !confirm(`${missing.join(", ")} left blank.\n\nThis server said it needs ${missing.length === 1 ? "it" : "them"} — it will probably connect with no tools. Save anyway?`)) return;
        body = { name, transport: "stdio", command, args, env };
      }
      const trustEl = document.getElementById("mcp-trust");
      body.trust_all = trustEl ? trustEl.checked : true;
      try { await api("POST", "/v1/mcp/servers", body); flash("Server saved.", "ok"); }
      catch (e) { flash("Couldn't add the server: " + e.message, "err"); }
      return reload();
    }
    // Remove an MCP server (its tools disconnect).
    if (verb === "mcp" && noun === "edit") { mcpEditServer(id); return; }
    if (verb === "mcp" && noun === "trust") {
      const on = arg === "1";
      try {
        await api("POST", "/v1/mcp/servers/" + encodeURIComponent(id) + "/trust", { trust_all: on });
        flash(on ? "Allowing all its tools — still asks before each use." : "No longer auto-allowing this server's tools.", "ok");
      } catch (e) { flash("Couldn't update: " + e.message, "err"); }
      return reload();
    }
    if (verb === "mcp" && noun === "reconnect") {
      try {
        const r = await api("POST", "/v1/mcp/servers/" + encodeURIComponent(id) + "/reconnect");
        if (r.connected) flash(`Connected — ${r.tools_live} tool${r.tools_live === 1 ? "" : "s"}.`, "ok");
        else flash("Still not connecting — check it's reachable and the token is right.", "err");
      } catch (e) { flash("Couldn't reconnect: " + e.message, "err"); }
      return reload();
    }
    // Reconnect answers "did the process start"; this answers "does the credential work".
    // A Brave server connected cleanly, listed eight tools and was subscribed to the wrong
    // API — every indicator green, because a handshake never calls the service behind it.
    // A refusal is the useful answer here, so it is shown as prose rather than swallowed.
    if (verb === "mcp" && noun === "test") {
      flash("Testing…", "ok");
      try {
        const r = await api("POST", "/v1/mcp/servers/" + encodeURIComponent(id) + "/test");
        flash(r.ok ? `It answered: ${r.said}` : r.said, r.ok ? "ok" : "err");
      } catch (e) { flash("Couldn't test: " + e.message, "err"); }
      return;
    }
    if (verb === "mcp" && noun === "remove") {
      if (!confirm(`Remove the MCP server "${id}"? Its tools will be disconnected.`)) return;
      try {
        await api("DELETE", "/v1/mcp/servers/" + encodeURIComponent(id));
        flash("Server removed.", "ok");
        // Its own screen has nothing left to show.
        if (NAV.v === "toolserver") NAV = { v: "connections" };
      }
      catch (e) { flash("Couldn't remove: " + e.message, "err"); }
      return reload();
    }
    // Prove a skill works with the settings it has. Read-only skills run themselves; one
    // that can actuate refuses, because "press this to find out" must never be how someone
    // discovers what a skill does. Home Assistant also sends a test notification, which is
    // the only honest way to check a nominated notify service — a misspelled one otherwise
    // fails silently forever and looks exactly like "nothing worth saying happened".
    if (verb === "skilltest") {
      flash("Testing…", "ok");
      try {
        const r = await api("POST", `/v1/capabilities/${noun}/test`);
        flash(r.said || (r.ok ? "Works." : "Didn't work."), r.ok ? "ok" : "err");
      } catch (e) { flash("Couldn't test it: " + e.message, "err"); }
      return;
    }
    // Save a skill's settings (ADR 0054). Only non-empty fields are sent, so a
    // blank secret leaves the stored value unchanged.
    if (verb === "skillcfg") {
      const cap = (CAPS || []).find((c) => c.id === noun);
      if (!cap) return;
      const settings = {};
      for (const s of (cap.settings || [])) {
        const el = document.getElementById(`setting-${noun}-${s.key}`);
        if (el && el.value.trim()) settings[s.key] = el.value.trim();
      }
      try { await api("POST", `/v1/capabilities/${noun}/config`, { settings }); flash("Saved.", "ok"); }
      catch (e) { flash("Couldn't save settings: " + e.message, "err"); }
      return reload();
    }
    // The providers and the ordered model lists (ADR 0105).
    if (verb === "mlist") {
      const val = (el) => ((document.getElementById(el) || {}).value || "").trim();
      const refresh = async () => { try { MODEL_LISTS = await api("GET", "/v1/model-lists"); } catch (_) {} render(); };
      try {
        if (["up", "down", "on", "off", "think", "quiet", "remove"].includes(noun)) {
          if (noun === "remove" && !confirm("Remove this model from the list?")) return;
          await api("POST", `/v1/model-lists/entries/${id}/${noun}`, {});
          return refresh();
        }
        if (noun === "test") {
          flash("Testing…", "ok");
          const r = await api("POST", `/v1/model-lists/entries/${id}/${noun}`, {});
          flash(r.detail || (r.ok ? "Connected." : "Test failed."), r.ok ? "ok" : "err");
          return;
        }
        if (noun === "add") {
          const model = val(`ml-${id}-model`);
          if (!model) { flash("Which model?", "err"); return; }
          await api("POST", "/v1/model-lists/entries", { job: id, provider: Number(val(`ml-${id}-provider`)), model });
          flash(`Added ${model}. Move it with ↑ ↓.`, "ok");
          return refresh();
        }
        if (noun === "discover") {
          const provider = Number(val(`ml-${id}-provider`));
          const r = await api("POST", `/v1/model-lists/providers/${provider}/discover`, {});
          const models = r.models || [];
          // Kept by list, with the provider they came from, so a re-render keeps them and the
          // list stays on that provider.
          FOUND_MODELS[id] = { provider, models };
          render();
          flash(models.length ? `Found ${models.length} models: tap one below the list to use it.` : "No models returned.", models.length ? "ok" : "err");
          return;
        }
        // The form draws the provider's name and endpoint itself, so a re-render keeps them.
        if (noun === "provideredit") { PROVIDER_EDITING = Number(id); return render(); }
        if (noun === "providercancel") { PROVIDER_EDITING = null; return render(); }
        if (noun === "providersave") {
          const body = { name: val("pv-name"), url: val("pv-url") };
          if (val("pv-key")) body.api_key = val("pv-key");
          if (!body.url) { flash("The endpoint, please.", "err"); return; }
          if (PROVIDER_EDITING) await api("POST", `/v1/model-lists/providers/${PROVIDER_EDITING}`, body);
          else await api("POST", "/v1/model-lists/providers", body);
          flash(PROVIDER_EDITING ? "Provider saved." : "Provider added. Now add its models to a list.", "ok");
          PROVIDER_EDITING = null;
          return refresh();
        }
        if (noun === "providerremove") {
          if (!confirm("Remove this provider? Its models leave both lists, and its key is forgotten.")) return;
          await api("POST", `/v1/model-lists/providers/${id}/remove`, {});
          if (PROVIDER_EDITING === Number(id)) PROVIDER_EDITING = null;
          for (const job of Object.keys(FOUND_MODELS)) if (FOUND_MODELS[job].provider === Number(id)) delete FOUND_MODELS[job];
          return refresh();
        }
      } catch (e) { flash("Couldn't do that: " + e.message, "err"); }
      return;
    }
    // Allow or block every tool a server exposes, in one act.
    //
    // Deliberately NOT the auto-allow toggle above. That is a standing policy which opens
    // whatever the server exposes including tools it gains later; this is a decision about
    // the tools that exist now, and grants nothing in future.
    if (verb === "mcp" && noun === "tools") {
      const parts = act.split(":");
      const server = parts[2] || "";
      const open = parts[3] === "1";
      try {
        const said = await api("POST", `/v1/mcp/servers/${encodeURIComponent(server)}/tools/open`, { open });
        flash(`${open ? "Allowed" : "Blocked"} all ${said.changed} tools on ${server}.`, "ok");
      } catch (e) { flash("Couldn't do that: " + e.message, "err"); }
      return reload();
    }
    // Connect something new (ADR 0054). Endora starts the service's own setup flow, shows
    // whatever it asks for, and hands the answers straight back. Nothing typed here is kept.
    if (verb === "connect") {
      const server = (MCP_SERVERS.servers || []).some((x) => x.name === "home-assistant")
        ? "home-assistant" : "home-assistant";
      if (noun === "cancel") { CONNECT = null; return render(); }
      if (noun === "start" || noun === "other") {
        const kind = noun === "other"
          ? ((document.getElementById("connect-other") || {}).value || "").trim()
          : id;
        if (!kind) { flash("Which kind of thing?", "err"); return; }
        flash("Asking your home what it needs…", "ok");
        try {
          const form = await api("POST", "/v1/connect/begin", { server, kind });
          CONNECT = Object.assign({ kind }, form);
          return render();
        } catch (e) { flash("Couldn't start that: " + e.message, "err"); return; }
      }
      if (noun === "submit" && CONNECT) {
        const answers = {};
        for (const f of CONNECT.fields || []) {
          const el = document.getElementById(`connect-${f.name}`);
          const v = el ? el.value.trim() : "";
          if (v) answers[f.name] = v;
        }
        try {
          const next = await api("POST", "/v1/connect/finish",
            { server, form: CONNECT.form, answers });
          if (next.done) {
            CONNECT = null;
            flash("Connected. Endora can use it now.", "ok");
            return reload();
          }
          // Another step: same form, new questions.
          CONNECT = Object.assign({ kind: CONNECT.kind }, next);
          return render();
        } catch (e) { flash(e.message, "err"); return; }
      }
      return;
    }
    if (verb === "export") {
      closeMenu();
      const data = await api("GET", "/v1/export");
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob); a.download = "endora-export.json"; a.click();
      return flash("Exported your data.", "ok");
    }
    if (verb === "purge") {
      closeMenu();
      if (!confirm("Permanently delete ALL of your Endora data? This cannot be undone.")) return;
      await api("POST", "/v1/memory/purge", { confirm: true });
      go("home"); return reload();
    }
  } catch (e) {
    flash(e.message || String(e), "err");
  }
}

document.body.addEventListener("click", (ev) => {
  const t = ev.target.closest("[data-act]");
  if (t) { ev.stopPropagation(); dispatch(t.getAttribute("data-act")); return; }
  // A click anywhere else closes the menu.
  if (!ev.target.closest("#menu")) closeMenu();
});

// A form a password is typed in is a real form, so a password manager knows it (Apple
// Passwords offers to save on its submit): Return or its button submits it, and the console
// does the form's action instead of leaving the page.
document.body.addEventListener("submit", (ev) => {
  const form = ev.target.closest("form[data-submit]");
  if (!form) return;
  ev.preventDefault();
  dispatch(form.getAttribute("data-submit"));
});

// A `<select data-change="verb:noun:id">` applies the chosen value on change,
// dispatching `verb:noun:id:<value>`. A range
// slider does the same once it is let go, not on every step of the drag.
document.body.addEventListener("change", (ev) => {
  const sel = ev.target.closest("select[data-change], input[type=range][data-change]");
  if (sel) dispatch(sel.getAttribute("data-change") + ":" + sel.value);
});

// Build the header: a single menu button, and the menu it opens: HQ first, then the house's own
// screens (ADR 0136).
function setupHeader(health) {
  const service = (health && health.service) || "";
  const menuBtn = document.getElementById("menu-btn");
  if (menuBtn) menuBtn.innerHTML = icon("menu");
  const item = (act, name, label, extra = "", cls = "") =>
    `<button class="${cls}" data-act="${act}">${icon(name)}<span>${label}</span>${extra}</button>`;
  const menu = document.getElementById("menu");
  if (menu) {
    // A short, focused menu: the everyday destinations. Everything set up once lives inside
    // Settings.
    menu.innerHTML =
      item("go:hq", "sparkle", "HQ") +
      item("go:lab", "server", "Home lab") +
      item("go:music", "music", "Music") +
      item("go:speakers", "speakerOn", "Speakers") +
      item("go:house", "house", "Smart home") +
      item("go:media", "film", "Media") +
      item("go:audit", "audit", "Activity") +
      item("go:settings", "gear", "Settings") +
      `<div class="divider"></div>` +
      "";
  }
  // Brand: "Endora" + a version pill. The pill also carries the build id (the
  // deploy's git SHA) so a refresh visibly changes when a new build is live —
  // "v0.11.0 · a1b2c3d". Tap the pill to copy the exact build string.
  const ver = (service.match(/\b\d+\.\d+\.\d+\b/) || [])[0] || (health && health.version);
  const build = health && health.build && health.build !== "dev" ? health.build : "";
  const brand = document.getElementById("brand");
  const label = brand && brand.querySelector("span:last-child");
  if (label && ver) {
    const pill = `v${ver}${build ? ` · ${build}` : ""}`;
    label.innerHTML = `Endora <span class="pill" style="font-weight:500" title="build ${build || "dev"}">${esc(pill)}</span>`;
  }
}

function closeMenu() {
  const m = document.getElementById("menu");
  if (m) m.hidden = true;
}

