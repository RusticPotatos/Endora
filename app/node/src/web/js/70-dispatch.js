function render() {
  updateMenuState();
  // Nothing else can load without a token, so nothing else is worth drawing.
  if (NEEDS_TOKEN) { app.innerHTML = viewNeedsToken(); return; }
  const v = NAV.v;
  // The chat fills the window with its own scrolling thread and a docked composer; every
  // other screen is an ordinary scrolling page.
  document.body.classList.toggle("on-chat", v === "chat");
  app.innerHTML =
      v === "audit" ? viewAudit()
    : v === "chat" ? viewChat()
    : v === "skills" ? viewSkills()
    : v === "skill" ? viewSkill()
    : v === "connections" ? viewConnections()
    : v === "permissions" ? viewPermissions()
    : v === "toolserver" ? viewToolServer()
    : v === "data" ? viewData()
    : v === "spotify" ? viewSpotify()
    : v === "lab" ? viewLab()
    : v === "containers" ? viewContainers()
    : v === "logs" ? viewLogs()
    : v === "jobs" ? viewJobs()
    : v === "job" ? viewJob()
    : v === "media" ? viewMedia()
    : v === "house" ? viewHouse()
    : v === "music" ? viewMusic()
    // Older links: what it remembers is now part of About you.
    : v === "prefs" ? viewUnderstanding()
    : v === "settings" ? viewSettings()
    : v === "mail" ? viewMail()
    : v === "display" ? viewDisplay()
    : v === "models" ? viewModels()
    : v === "proactive" ? viewProactive()
    : v === "understanding" ? viewUnderstanding()
    : v === "signin" ? viewSignin()
    : v === "recipes" ? viewRecipes()
    : viewUnderstanding();
  // On the chat, jump to the newest message (kept clear of the sticky composer).
  if (v === "chat") {
    const thread = document.getElementById("chat-thread");
    const last = thread && thread.lastElementChild;
    if (last) requestAnimationFrame(() => scrollBubbleIntoView(last));
  } else if (v !== LAST_VIEW) {
    // Every other screen is a list with the newest thing FIRST, so it opens at the
    // top. Without this the window simply keeps whatever scroll position the last
    // screen had — arrive from the bottom of a long conversation and the inbox opens
    // at its oldest message.
    //
    // Only when the screen actually changes. Doing it on every render would yank
    // someone back to the top mid-read whenever a live update arrived.
    requestAnimationFrame(() => window.scrollTo({ top: 0 }));
  }
  LAST_VIEW = v;
}

// Reflect the activity toggle's state in the menu.
function updateMenuState() {
  const act = document.getElementById("menu-activity-state");
  if (act) act.textContent = SHOW_ACTIVITY ? "on" : "off";
}

// ---- actions (event delegation) -------------------------------------------
async function dispatch(act) {
  const [verb, noun, id, arg] = act.split(":");
  // Choosing anything from the "+" menu closes it; opening it is the one exception.
  if (!(verb === "toggle" && noun === "tools")) {
    const tools = document.getElementById("tools-menu");
    if (tools) tools.hidden = true;
  }
  try {
    if (verb === "reload") { location.reload(); return; }
    // The home lab, media, smart home and music.
    if (await labDispatch(verb, noun, id, arg)) return;
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
    // The AI screen's own choices: the mail reader's model and the speech server.
    if (verb === "ai" && (noun === "mailreader" || noun === "speech")) {
      const input = document.getElementById(noun === "mailreader" ? "ai-mail-reader" : "ai-speech");
      const body = noun === "mailreader" ? { mail_reader_model: input ? input.value : "" } : { speech_url: input ? input.value : "" };
      try {
        AI = await api("POST", "/v1/ai", body);
        flash(noun === "mailreader" ? "Mail will be read with that model, on your network." : "Saved the speech server.", "ok");
      } catch (e) { flash("Couldn't save: " + e.message, "err"); }
      return render();
    }
    if (verb === "go") {
      if (noun === "home") return go("home");
      if (noun === "audit") return go("audit");
      return go(noun, id);
    }
    if (verb === "chat" && noun === "send") { await sendChat(); return; }
    if (verb === "chat" && noun === "mic") { listen(); return; }
    if (verb === "chat" && noun === "stop") { stopChat(); return; }
    if (verb === "research" && noun === "start") { await startResearch(); return; }
    if (verb === "research" && noun === "cancel") {
      await api("POST", `/v1/research/${encodeURIComponent(id)}/cancel`);
      flash("Research cancelled.", "ok");
      return reload();
    }
    if (verb === "research" && noun === "download") { await downloadResearch(id); return; }
    // Ask the same thing again after a turn that never answered. It re-sends the person's
    // own last words rather than making them retype them, which is the whole difference
    // between a dead end and a hiccup.
    if (verb === "chat" && noun === "retry") {
      const list = (DB.messages || []).slice().reverse();
      const mine = list.find((m) => m.role === "user");
      if (!mine || !mine.text) return;
      CHAT_QUEUE.push(LAST_CHAT_REQUEST && LAST_CHAT_REQUEST.message === mine.text
        ? LAST_CHAT_REQUEST : mine.text);
      updateComposerButton();
      return drainChat();
    }
    if (verb === "brief") {
      try {
        const r = await api("POST", "/v1/brief");
        if (r && r.briefed === false) flash(r.note || "Nothing to brief yet.", "err");
      } catch (e) { flash("Couldn't prepare a brief: " + e.message, "err"); }
      return reload();
    }
    if (verb === "toggle" && noun === "speak") {
      SPEAK = !SPEAK;
      localStorage.setItem("endora.speak", SPEAK ? "1" : "0");
      if (SPEAK) unlockSpeech(); else if (TTS) TTS.cancel();
      // Update the button in place — a full render() rebuilds the chat thread from
      // persisted state and would wipe a reply that's still streaming in.
      updateSpeakButton();
      return;
    }
    // Deep mode: while on, Send routes to the bigger model (an option, not a
    // one-off button). Update in place so it never disturbs a streaming reply.
    if (verb === "toggle" && noun === "deep") {
      DEEP_MODE = !DEEP_MODE;
      localStorage.setItem("endora.deepmode", DEEP_MODE ? "1" : "0");
      updateDeepButton();
      flash(DEEP_MODE ? "Deep mode on — your messages go to the bigger model." : "Deep mode off.", "ok");
      return;
    }
    if (verb === "toggle" && noun === "research") {
      RESEARCH_MODE = !RESEARCH_MODE;
      localStorage.setItem("endora.researchmode", RESEARCH_MODE ? "1" : "0");
      return render();
    }
    if (verb === "ctx" && noun === "toggle") {
      CTX_OPEN = !CTX_OPEN;
      const panel = document.getElementById("ctx-panel");
      if (panel) panel.hidden = !CTX_OPEN;
      if (CTX_OPEN) {
        try { CTX = await api("GET", "/v1/chat/context"); } catch (_) {}
        if (DEEP_MODEL.configured) await loadDeepSpend();
        refreshContextRing();
      }
      return;
    }
    if (verb === "deepspend" && noun === "refresh") {
      await loadDeepSpend();
      const box = document.getElementById("deep-spend");
      if (box) box.innerHTML = deepSpendHtml() || '<span class="sub">Nothing used yet.</span>';
      return;
    }
    if (verb === "deepprices" && noun === "save") {
      const num = (id) => {
        const v = ((document.getElementById(id) || {}).value || "").trim();
        return v === "" ? null : Number(v);
      };
      const body = { input: num("d-price-in"), output: num("d-price-out"), cached: num("d-price-cached") };
      if (Object.values(body).some((v) => v !== null && !Number.isFinite(v))) {
        flash("A price is a number, like 0.30.", "err");
        return;
      }
      await api("POST", "/v1/deep-model/prices", body);
      flash("Prices saved.", "ok");
      return reload();
    }
    if (verb === "ctx" && noun === "reset") {
      await api("POST", "/v1/chat/context/reset");
      CTX_OPEN = false;
      flash("New topic. Earlier messages stay here but are no longer sent to the model.", "ok");
      return reload();
    }
    if (verb === "toggle" && noun === "tools") {
      const t = document.getElementById("tools-menu");
      if (t) t.hidden = !t.hidden;
      return;
    }
    if (verb === "toggle" && noun === "menu") {
      const m = document.getElementById("menu");
      if (m) m.hidden = !m.hidden;
      return;
    }
    if (verb === "toggle" && noun === "haptic") {
      HAPTIC = !HAPTIC;
      localStorage.setItem("endora.haptic", HAPTIC ? "1" : "0");
      buzz(25); // confirm the change in the medium it controls
      return render();
    }
    if (verb === "toggle" && noun === "activity") {
      SHOW_ACTIVITY = !SHOW_ACTIVITY;
      localStorage.setItem("endora.showActivity", SHOW_ACTIVITY ? "1" : "0");
      closeMenu();
      return render();
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
    // Save the person's home location as a preference the butler reads for context.
    if (verb === "setlocation") {
      const where = val("setup-location");
      if (!where) return;
      await api("POST", "/v1/preferences", { text: `Based in: ${where}`, kind: "context" });
      flash("Got it — Endora knows where you're based.", "ok");
      return reload();
    }
    // Save how the person likes to be addressed, so the butler uses it and matches
    // the formality it implies.
    if (verb === "setaddress") {
      const how = val("setup-address");
      if (!how) return;
      await api("POST", "/v1/preferences", { text: `Address me as: ${how}`, kind: "context" });
      flash("Got it.", "ok");
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
    // A recipe's own enable route (ADR 0071) — NOT /v1/capabilities/{id}/enable.
    // That generic route turns "on" into clearing the stored stance, which for a
    // recipe means falling back to its OWN default (off), not the observe band's —
    // so it would look like it worked and quietly not enable anything. This one
    // stores "on" explicitly.
    if (verb === "recipe" && noun === "enable") {
      const enabled = arg === "1";
      try {
        await api("POST", `/v1/recipes/${id}/enable`, { enabled });
        flash(enabled ? `${id} is on.` : `${id} is off. You can turn it back on any time.`, "ok");
      }
      catch (e) { flash("Couldn't change that recipe: " + e.message, "err"); }
      return reload();
    }
    if (verb === "recipe" && noun === "delete") {
      if (!confirm(`Delete the "${id}" recipe? This can't be undone.`)) return;
      try { await api("DELETE", `/v1/recipes/${id}`); flash("Recipe deleted.", "ok"); }
      catch (e) { flash("Couldn't delete that recipe: " + e.message, "err"); }
      return reload();
    }
    // Fill the form from a ready-made one. Like `recipe:try`, it writes into the
    // fields rather than re-rendering — the form is open and the person is
    // standing in it.
    if (verb === "recipe" && noun === "start") {
      const starter = RECIPE_STARTERS[Number(id)];
      if (!starter) return;
      const put = (field, value) => {
        const box = document.getElementById(`recipe-${field}`);
        if (box) box.value = value;
      };
      put("id", starter.id);
      put("description", starter.description);
      put("inputs", "");
      put("get", starter.get);
      put("say", starter.say);
      // The address changed, so any earlier trial is about a different recipe.
      recipeSamplesChanged("");
      const trial = document.getElementById("recipe-trial");
      if (trial) trial.innerHTML = "";
      flash(`Filled in "${starter.description}" — ${starter.note}, then try it.`, "ok");
      return;
    }
    // Fetch the draft once and show what came back. Deliberately does NOT
    // reload(): a re-render would wipe the half-written form this exists to
    // help you finish, so it writes into its own result node and nothing else.
    if (verb === "recipe" && noun === "try") {
      const get = val("recipe-get");
      if (!get) { flash("Put in a web address first.", "err"); return; }
      const inputs = recipeInputsFrom(val("recipe-inputs"));
      // Values are typed per their declaration, because the address filler is
      // strict about it — a number arriving as text is refused there, and being
      // refused by your own Try button is a confusing way to learn that.
      const values = {};
      for (const box of document.querySelectorAll("[data-sample]")) {
        const name = box.getAttribute("data-sample");
        const raw = (box.value || "").trim();
        if (!raw) continue;
        values[name] = box.getAttribute("data-sample-type") === "number" ? Number(raw) : raw;
      }
      const where = document.getElementById("recipe-trial");
      if (where) where.innerHTML = `<div class="sub" style="margin-top:8px;">Fetching…</div>`;
      let result;
      try { result = await api("POST", "/v1/recipes/try", { inputs, get, say: val("recipe-say"), values }); }
      catch (e) { result = { ok: false, error: e.message }; }
      if (where) where.innerHTML = recipeTrialHtml(result);
      return;
    }
    // Put a field from the trial into the sentence. Appends rather than
    // replaces: a sentence is usually several fields and some words.
    if (verb === "recipe" && noun === "pick") {
      const path = act.slice("recipe:pick:".length);
      const say = document.getElementById("recipe-say");
      if (say && path) {
        say.value = `${say.value}{${path}}`;
        say.focus();
      }
      return;
    }
    if (verb === "recipe" && noun === "add") {
      const id2 = val("recipe-id");
      const description = val("recipe-description");
      const get = val("recipe-get");
      const say = val("recipe-say");
      if (!id2 || !description || !get || !say) { flash("Fill in a name, what it does, the address, and what to say.", "err"); return; }
      // One "name:type" per line — the same shape the domain layer stores, typed
      // out by hand rather than built from repeatable form rows, because five
      // fields do not need a dynamic form to stay honest about their limits.
      const inputs = val("recipe-inputs").split("\n").map((line) => line.trim()).filter(Boolean)
        .map((line) => {
          const [name, type] = line.split(":").map((s) => (s || "").trim());
          return { name, type };
        });
      const badInput = inputs.find((i) => !i.name || (i.type !== "string" && i.type !== "number"));
      if (badInput) { flash(`"${badInput.name || "?"}" needs a type of "string" or "number".`, "err"); return; }
      try {
        await api("POST", "/v1/recipes", { id: id2, description, inputs, get, say });
        flash(`Taught it "${id2}" — turn it on above when you're ready.`, "ok");
      }
      catch (e) { flash("Couldn't save that recipe: " + e.message, "err"); return; }
      return reload();
    }
    // Step to another day's conversation. Nothing is loaded or archived — the messages
    // are all here and a day is just which of them to show.
    if (verb === "chat" && noun === "day") {
      CHAT_DAY = id === dayOf(Date.now()) ? null : id;
      await loadChatDay(CHAT_DAY);
      return render();
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
    // Discover the models an endpoint offers, into its picker.
    if (verb === "discover") { await discoverModels(noun); return; }
    if (verb === "testconn") { await testConnection(noun); return; }
    // Save the butler models. A blank key keeps the stored one; blank sampling
    // fields mean "use the endpoint default". Applies on the next message.
    if (verb === "modelsave") {
      const val = (id) => (document.getElementById(id) || {}).value;
      const numOrNull = (id) => { const v = val(id); return (v === undefined || String(v).trim() === "") ? null : Number(v); };
      const intOrNull = (id) => { const v = numOrNull(id); return v === null ? null : Math.round(v); };
      const slot = (p) => ({
        model: (val(`m-${p}-model`) || "").trim(),
        temperature: numOrNull(`m-${p}-temperature`),
        top_p: numOrNull(`m-${p}-top_p`),
        top_k: intOrNull(`m-${p}-top_k`),
        repeat_penalty: numOrNull(`m-${p}-repeat_penalty`),
      });
      const body = {
        base_url: (val("m-base") || "").trim(),
        mixture: !!(document.getElementById("m-mix") || {}).checked,
        single: slot("single"), router: slot("router"), synth: slot("synth"),
        preferred_url: (val("m-preferred-url") || "").trim(),
        preferred_model: (val("m-preferred-model") || "").trim(),
      };
      const key = (val("m-key") || "").trim();
      if (key) body.api_key = key;
      try { await api("POST", "/v1/model-config", body); flash("Models saved — active on your next message.", "ok"); }
      catch (e) { flash("Couldn't save models: " + e.message, "err"); }
      return reload();
    }
    // Save the deep-model config. A blank key keeps the stored one.
    if (verb === "deepsave") {
      const url = (document.getElementById("deep-url") || {}).value || "";
      const model = (document.getElementById("deep-model") || {}).value || "";
      const key = (document.getElementById("deep-key") || {}).value || "";
      // Sent explicitly, not defaulted: the server leaves it alone when absent, so
      // saving an endpoint must never silently flip whether Endora phones out.
      const escalate = !!(document.getElementById("deep-escalate") || {}).checked;
      const body = { url: url.trim(), model: model.trim(), escalate };
      if (key.trim()) body.api_key = key.trim();
      try { await api("POST", "/v1/deep-model", body); flash(escalate ? "Deep model saved — it may now step in on its own." : "Deep model saved.", "ok"); }
      catch (e) { flash("Couldn't save: " + e.message, "err"); }
      return reload();
    }
    // Widen/narrow the autonomy envelope (ADR 0051). `noun` is the lever; `id` is 1/0.
    if (verb === "autonomy") {
      const env = Object.assign({ auto_external: true, auto_consequential: false }, AUTONOMY || {});
      if (noun === "external") env.auto_external = id === "1";
      if (noun === "consequential") env.auto_consequential = id === "1";
      try { await api("POST", "/v1/autonomy", env); }
      catch (e) { flash("Couldn't change that: " + e.message, "err"); }
      return reload();
    }
    // Review Endora's understanding: affirm (raises confidence) or correct (drops
    // it). Both give feedback — affirming an already-sure belief changes nothing
    // visible, so without a note it feels like nothing happened.
    if (verb === "affirm" && noun === "belief") {
      try { await api("POST", `/v1/understanding/${id}/affirm`); flash("Thanks — I've got that right.", "ok"); }
      catch (e) { flash("Couldn't note that: " + e.message, "err"); }
      return reload();
    }
    if (verb === "correct" && noun === "belief") {
      try { await api("POST", `/v1/understanding/${id}/correct`); flash("Got it — I'll hold that more loosely.", "ok"); }
      catch (e) { flash("Couldn't note that: " + e.message, "err"); }
      return reload();
    }
    if (verb === "play" && noun === "msg") {
      const m = messages().find((x) => String(x.id) === String(id));
      if (m) readAloud(m.text);
      return;
    }
    // Answer what Endora asked about a target it can't hit (ADR 0054). This is the
    // confirmed source — Endora never fills it in from a server's own text.
    // Stop asking about one thing. Nothing is turned off — it silences this question only.
    if (verb === "repair" && noun === "leave") {
      const server = String(id || "").split(".")[0];
      try {
        await api("POST", `/v1/repairs/${encodeURIComponent(server)}/${encodeURIComponent(arg)}/leave`, {});
        flash("Alright — I'll stop asking about that one.", "ok");
      } catch (e) { flash("Couldn't do that: " + e.message, "err"); }
      return reload();
    }
    if (verb === "alias") {
      const server = String(noun || "").split(".")[0];
      const el = document.getElementById(`alias-${noun}-${id}`);
      const means = el ? el.value.trim() : "";
      if (!means) { flash("Tell it what the thing is actually called.", "err"); return; }
      try {
        await api("POST", "/v1/aliases", { server, said: id, means });
        flash(`Noted — “${id}” means “${means}”.`, "ok");
      } catch (e) { flash("Couldn't note that: " + e.message, "err"); }
      return reload();
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
    // Answer a problem statement about something in your world (ADR 0056). Both answers
    // end it: one changes the service, the other says it is meant to be like that. There
    // is deliberately no "remind me later" — that is how a queue starts.
    // Stop working on something. The person's ONLY verb over an intention — there is
    // deliberately no way to create or edit one (ADR 0052).
    // Start fresh: forget everything Endora learned, keeping what they told it.
    if (verb === "forgetlearned") {
      if (!confirm("Forget everything Endora has learned about you? What you told it yourself stays. This can't be undone.")) return;
      try {
        const done = await api("POST", "/v1/understanding/forget", { confirm: true });
        const meant = done.intentions ? `, and ${done.intentions} thing${done.intentions === 1 ? "" : "s"} it meant to do` : "";
        flash(`Forgot ${done.forgot} thing${done.forgot === 1 ? "" : "s"} it had learned${meant}.`, "ok");
      } catch (e) { flash("Couldn't forget: " + e.message, "err"); }
      return reload();
    }
    // How an action landed. Offered where the action appears; never asked for, and
    // the latest word wins (ADR 0053).
    if (verb === "react") {
      try {
        await api("POST", `/v1/outcomes/${id}/reaction`, { reaction: noun });
        flash(noun === "helped" ? "Good — I'll do more of that." : noun === "did_not_help" ? "Noted — I'll rethink that one." : "Noted.", "ok");
      } catch (e) { flash("Couldn't note that: " + e.message, "err"); }
      return reload();
    }
    // Daily-brief schedule: `noun` is "off" or a LOCAL hour; convert to UTC.
    if (verb === "briefsched") {
      const enabled = noun !== "off";
      const tzOff = new Date().getTimezoneOffset() / 60;
      const hour_utc = enabled ? ((Number(noun) + tzOff) % 24 + 24) % 24 : (BRIEF_SCHED.hour_utc || 12);
      await api("POST", "/v1/brief/schedule", { enabled, hour_utc });
      flash(enabled ? "The butler will bring you a daily brief." : "Daily brief off.", "ok");
      return reload();
    }
    if (verb === "create" && noun === "pref") {
      const text = val("new-pref");
      if (!text) return;
      await api("POST", "/v1/preferences", { text }); return reload();
    }
    if (verb === "delete" && noun === "pref") {
      await api("DELETE", `/v1/preferences/${id}`); return reload();
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

// Enter sends the chat message (Shift+Enter is free for a newline if the input
// ever becomes multi-line).
document.body.addEventListener("keydown", (ev) => {
  if (ev.target.id === "chat-input" && ev.key === "Enter" && !ev.shiftKey && !ev.isComposing) {
    ev.preventDefault(); sendChat();
  }
});

// iOS/software keyboards often don't fire a keydown for Return on a textarea, so
// the desktop handler above never sees it. Catch the line-break input on touch
// devices and send instead (a hardware Shift+Enter on desktop is a `pointer:fine`
// device, so it still inserts a newline).
document.body.addEventListener("beforeinput", (ev) => {
  if (
    ev.target.id === "chat-input" &&
    ev.inputType === "insertLineBreak" &&
    window.matchMedia("(pointer: coarse)").matches
  ) {
    ev.preventDefault(); sendChat();
  }
});

// Auto-grow the chat textarea as it wraps, so long messages expand instead of
// scrolling within one line.
function growInput(el) {
  if (!el) return;
  el.style.height = "auto";
  el.style.height = Math.min(el.scrollHeight, window.innerHeight * 0.4) + "px";
}
document.body.addEventListener("input", (ev) => {
  if (ev.target.id === "chat-input") growInput(ev.target);
});

// A `<select data-change="verb:noun:id">` applies the chosen value on change,
// dispatching `verb:noun:id:<value>` (used for the lifecycle Status dropdown).
document.body.addEventListener("change", (ev) => {
  const sel = ev.target.closest("select[data-change]");
  if (sel) dispatch(sel.getAttribute("data-change") + ":" + sel.value);
});

// Build the header: a single menu button, and the menu it opens. Everything
// beyond the butler chat lives in here, so the main surface stays clean.
function setupHeader(health) {
  const service = (health && health.service) || "";
  STT_AVAILABLE = !!(health && health.stt); // a Whisper server is configured
  const menuBtn = document.getElementById("menu-btn");
  if (menuBtn) menuBtn.innerHTML = icon("menu");
  const item = (act, name, label, extra = "", cls = "") =>
    `<button class="${cls}" data-act="${act}">${icon(name)}<span>${label}</span>${extra}</button>`;
  const menu = document.getElementById("menu");
  if (menu) {
    // A short, focused menu: the everyday destinations. Everything else
    // (Skills, preferences, export) lives inside Settings.
    menu.innerHTML =
      item("go:chat", "chat", "Home") +
      item("go:mail", "inbox", "Mail") +
      item("go:music", "music", "Music") +
      item("go:house", "house", "Smart home") +
      item("go:media", "film", "Media") +
      item("go:lab", "server", "Home lab") +
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
  const t = document.getElementById("tools-menu");
  if (t) t.hidden = true;
}

