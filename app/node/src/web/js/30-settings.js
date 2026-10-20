// Catalog results, kept so a "Use" click can prefill the form from the entry.
let MCP_CATALOG = [];

// Search the MCP catalog (curated + community registry) and render the results.
async function mcpSearch() {
    const q = ((document.getElementById("mcp-search") || {}).value || "").trim();
    const box = document.getElementById("mcp-catalog-results");
    if (box) box.innerHTML = `<div class="sub" style="margin-top:8px;">Searching…</div>`;
    try {
      const r = await api("GET", "/v1/mcp/catalog?q=" + encodeURIComponent(q));
      MCP_CATALOG = r.servers || [];
      if (!box) return;
      if (!MCP_CATALOG.length) { box.innerHTML = `<div class="sub" style="margin-top:8px;">Nothing matched.</div>`; return; }
      // Checked entries first, then the registry by recency. The registry publishes no
      // download or star count of any kind, so recency is the only ordering signal there is
      // — say that rather than imply a popularity sort nobody can actually provide.
      //
      // And say who publishes it. Searching "brave" put a stranger's fork first and Brave's
      // own fourth, purely because the fork was edited more recently, on a search whose next
      // step is typing an API key into whatever came back. Recency reads like ranking unless
      // the screen says otherwise.
      const note = r.registry_ok
        ? `<div class="sub" style="margin-top:6px;">Checked entries first, then the community registry, newest first — it publishes no download counts, so recency is all the ordering there is. Anyone can publish there, so check a name is who you think it is before giving it a key.</div>`
        : `<div class="sub" style="margin-top:6px;">Showing built-in suggestions — the community registry wasn't reachable.</div>`;
      box.innerHTML = note + MCP_CATALOG.map((e, i) => `
        <div class="row" style="align-items:flex-start;gap:10px;margin-top:8px;border-top:1px solid var(--line);padding-top:8px;">
          <div class="grow">
            <div class="title" style="font-weight:500;">${esc(e.name)} <span class="pill">${esc(e.source)}</span>${e.transport === "http" ? ` <span class="pill">http</span>` : ""}${e.updated ? ` <span class="pill">updated ${esc(e.updated)}</span>` : ""}</div>
            <div class="sub">${esc(e.description || "")}</div>
            ${e.docs ? `<div class="sub"><a class="link" href="${esc(e.docs)}" target="_blank" rel="noopener noreferrer">docs</a></div>` : ""}
          </div>
          <button class="ghost" onclick="mcpUseCatalog(${i})">Use</button>
        </div>`).join("");
    } catch (e) {
      if (box) box.innerHTML = `<div class="sub" style="margin-top:8px;">Couldn't search: ${esc(e.message)}</div>`;
    }
}

// The variables the chosen entry declares, as real form fields.
//
// Nothing is invented: this renders only what the registry says the server needs. When it
// declares nothing — which is common — the section stays empty and the Advanced box below
// is the way in, with the server's own docs alongside it.
function renderMcpNeeds() {
  const host = document.getElementById("mcp-needs");
  if (!host) return;
  const fields = MCP_NEEDS.fields || [];
  if (!fields.length) {
    host.innerHTML = MCP_NEEDS.docs
      ? `<div class="sub">This server didn't say what it needs. Check <a href="${esc(MCP_NEEDS.docs)}" target="_blank" rel="noopener">its docs</a>, then add any variables under Advanced.</div>`
      : "";
    return;
  }
  host.innerHTML = `
    <div class="sub" style="margin-bottom:6px;">What this server needs${MCP_NEEDS.docs ? ` · <a href="${esc(MCP_NEEDS.docs)}" target="_blank" rel="noopener">its docs</a>` : ""}</div>
    ${fields.map((f) => `
      <div class="field">
        <label>${esc(f.label || f.key)}${f.secret ? ` <span class="sub" style="font-weight:400;">· kept secret, never shown again</span>` : ""}</label>
        <input id="mcp-need-${esc(f.key)}" data-need="${esc(f.key)}"
               type="${f.secret ? "password" : "text"}" autocomplete="off"
               placeholder="${esc(f.placeholder || "")}" />
      </div>`).join("")}`;
}

// A short, safe name for a server, from whatever the registry calls it.
//
// Registry ids are reverse-DNS with a path — `io.github.XavierFabregat/spotify-mcp` —
// and a name is NOT just a label: tools are namespaced `server.tool` and resolved on the
// FIRST dot, so a name containing dots resolves to a server called "io" and every one of
// its tools silently disappears. The last path segment is both safe and what a person
// would have typed anyway.
function shortServerName(id) {
  const last = String(id).split("/").pop() || "";
  return last.replace(/[^A-Za-z0-9_-]/g, "-").replace(/-+/g, "-").replace(/^-|-$/g, "");
}

// Bring a form into view, opening whatever it is folded inside first.
//
// The add form lives in a `<details>`, because almost nobody arrives at that screen to type
// a launch command out by hand. Filling it in while that is shut fills in nothing anybody
// can see: `scrollIntoView` on a collapsed element moves the page nowhere, so choosing a
// server flashed "review and add it" over a screen that did not change, and the field asking
// for the API key stayed behind a fold there was no reason to suspect.
//
// Ancestors only. A nested `<details>` — Advanced, and the other environment variables —
// stays shut, which is the whole point of it being there.
function revealForm(id) {
  const form = document.getElementById(id);
  if (!form) return;
  let fold = form.closest ? form.closest("details") : null;
  while (fold) {
    fold.open = true;
    fold = fold.parentElement && fold.parentElement.closest
      ? fold.parentElement.closest("details")
      : null;
  }
  form.scrollIntoView({ block: "center" });
}

// Prefill the add form from a catalog entry. Everything stays editable — the entry
// is a starting point, not a fixed recipe.
function mcpUseCatalog(i) {
  const e = MCP_CATALOG[i];
  if (!e) return;
  const set = (id, v) => { const el = document.getElementById(id); if (el) el.value = v; };
  set("mcp-name", shortServerName(e.id || e.name || ""));
  const t = document.getElementById("mcp-transport");
  if (t) { t.value = e.transport === "http" ? "http" : "stdio"; mcpTransportChange(t.value); }
  set("mcp-command", e.command || "");
  set("mcp-args", (e.args || []).join("\n"));
  // What this entry says it needs, rendered as real inputs below — one per variable,
  // masked where it is a credential. A textarea of KEY=value is the wrong instrument for
  // a secret: the value sits in plain view and a mistyped key fails silently.
  MCP_NEEDS = { fields: (e.fields || []).filter((f) => f.target === "env"), docs: e.docs || "" };
  renderMcpNeeds();
  set("mcp-env", "");
  set("mcp-url", e.url || "");
  set("mcp-auth", "");
  const needs = (e.fields || []).filter((f) => f.target !== "env");
  flash(needs.length
    ? `Filled in ${e.name}. Still needs: ${needs.map((f) => f.label).join(", ")}.`
    : `Filled in ${e.name} — review and add it.`, "ok");
  revealForm("mcp-name");
}

// Load an already-registered server into the Add/Save form so its URL or settings can
// be changed in place. Secrets are never returned by the API, so the token/env values
// come back blank — left blank on save they're kept as-is (the server merges them).
function mcpEditServer(name) {
  const s = ((MCP_SERVERS && MCP_SERVERS.servers) || []).find((x) => x.name === name);
  if (!s) return;
  const set = (id, v) => { const el = document.getElementById(id); if (el) el.value = v; };
  set("mcp-name", s.name);
  const t = document.getElementById("mcp-transport");
  if (t) { t.value = s.transport === "http" ? "http" : "stdio"; mcpTransportChange(t.value); }
  set("mcp-command", s.command || "");
  set("mcp-args", (s.args || []).join("\n"));
  // Env keys come back as names only; show them as KEY= lines to re-fill if wanted.
  set("mcp-env", (s.env_keys || []).map((k) => `${k}=`).join("\n"));
  set("mcp-url", s.url || "");
  set("mcp-auth", "");
  const trustEl = document.getElementById("mcp-trust");
  if (trustEl) trustEl.checked = s.trust_all !== false;
  const secretNote = s.auth_set || (s.env_keys || []).length
    ? " Leave the token/secret blank to keep what's saved."
    : "";
  flash(`Editing "${s.name}". Change what you need, then Save.${secretNote}`, "ok");
  revealForm("mcp-name");
}

// Toggle the MCP add-form fields between the stdio (command/args) and http (url) sets.
function mcpTransportChange(v) {
  const stdio = document.getElementById("mcp-stdio-fields");
  const http = document.getElementById("mcp-http-fields");
  if (stdio) stdio.style.display = v === "http" ? "none" : "block";
  if (http) http.style.display = v === "http" ? "block" : "none";
}

// Set the model field(s) from a tapped discovery result. For the everyday card that
// is the synthesizer when a mixture is on, else the single model — mirroring what
// The preset key whose endpoint matches a saved base URL (so a configured card
// shows e.g. "DeepSeek" instead of "Choose a provider…"). "" if none matches.
function presetFor(url) {
  if (!url) return "";
  const u = url.trim().replace(/\/+$/, "");
  const hit = Object.entries(MODEL_PRESETS).find(([, v]) => (v.base_url || "").replace(/\/+$/, "") === u);
  return hit ? hit[0] : "";
}

// Provider <option>s with `selected` set on the given key.
function presetOptions(selected) {
  return Object.entries(MODEL_PRESETS)
    .map(([k, v]) => `<option value="${k}"${k === selected ? " selected" : ""}>${esc(v.label)}</option>`).join("");
}

// Models in ordered lists (ADR 0105): providers entered once, then for each job a list the
// first working, cleared entry of answers. Sorted with ↑ ↓; an entry can be switched off
// without losing it. A key goes in once and never comes back: the screen only knows it is set.
// "Thinks" lets a model on the house's network think before it answers: slower, off by default;
// a cloud entry is never told either way.
function modelListRows(job) {
  const rows = ((MODEL_LISTS && MODEL_LISTS[job]) || []).map((e, i, all) => `
    <li class="mlist-row${e.enabled ? "" : " off"}">
      <span class="mlist-n">${i + 1}</span>
      <div class="grow">
        <div class="title" style="font-weight:500;">${esc(e.model)}</div>
        <div class="sub">${esc(e.provider_name)} · ${e.local ? "on your network" : "cloud, sees your messages disguised"}${e.enabled ? "" : " · switched off"}</div>
      </div>
      <div class="mlist-acts">
        <button class="ghost small" data-act="mlist:up:${e.id}" aria-label="Move up" ${i === 0 ? "disabled" : ""}>↑</button>
        <button class="ghost small" data-act="mlist:down:${e.id}" aria-label="Move down" ${i === all.length - 1 ? "disabled" : ""}>↓</button>
        <button class="ghost small" data-act="mlist:${e.enabled ? "off" : "on"}:${e.id}">${e.enabled ? "Off" : "On"}</button>
        <button class="ghost small" data-act="mlist:${e.thinks ? "quiet" : "think"}:${e.id}" aria-pressed="${e.thinks ? "true" : "false"}" title="${e.local ? "Let it think before answering: better on hard questions, about ten times slower" : "A cloud model is never told whether to think"}">Thinks: ${e.thinks ? "on" : "off"}</button>
        <button class="ghost small" data-act="mlist:test:${e.id}">Test</button>
        <button class="ghost small danger" data-act="mlist:remove:${e.id}" aria-label="Remove">×</button>
      </div>
    </li>`).join("");
  return rows ? `<ol class="mlist">${rows}</ol>` : `<div class="sub" style="margin:6px 0;">Nothing in this list yet.</div>`;
}

// What Discover found for a list, as buttons to tap: a datalist alone is all but invisible on a
// phone. A model name holds ':', and data-act is split on ':', so the name rides in a data
// attribute and a tap goes to pickListModel.
function foundModelsHtml(job) {
  const found = FOUND_MODELS[job];
  if (!found) return "";
  if (!found.models.length) return `<div class="sub">No models returned.</div>`;
  return `<div class="sub" style="margin:6px 0 4px;">Tap a model to use it:</div>
    <div class="model-picker">${found.models.map((m) =>
      `<button type="button" class="pill pick" data-model="${esc(m)}" onclick="pickListModel('${job}', this.dataset.model)">${esc(m)}</button>`).join("")}</div>`;
}

// Fill a list's model box with a model Discover found.
function pickListModel(job, name) {
  if (!name) return;
  const box = document.getElementById(`ml-${job}-model`);
  if (box) box.value = name;
}

function modelListAdd(job) {
  const providers = (MODEL_LISTS && MODEL_LISTS.providers) || [];
  if (!providers.length) return `<div class="sub">Add a provider below first.</div>`;
  const found = FOUND_MODELS[job];
  const chosen = found ? found.provider : null;
  const models = found ? found.models : [];
  return `
    <div class="mlist-add">
      <select id="ml-${job}-provider" aria-label="Provider">${providers.map((p) => `<option value="${p.id}"${p.id === chosen ? " selected" : ""}>${esc(p.name)}</option>`).join("")}</select>
      <input id="ml-${job}-model" list="ml-${job}-models" placeholder="model, e.g. ${job === "deep" ? "claude-sonnet-5-5" : "qwen2.5:7b"}" aria-label="Model" />
      <datalist id="ml-${job}-models">${models.map((m) => `<option value="${esc(m)}"></option>`).join("")}</datalist>
      <button class="ghost small" data-act="mlist:discover:${job}">${icon("sparkle", 14)} Discover models</button>
      <button class="primary small" data-act="mlist:add:${job}">Add</button>
    </div>
    ${foundModelsHtml(job)}`;
}

function providersCard() {
  const providers = (MODEL_LISTS && MODEL_LISTS.providers) || [];
  // The provider being changed, drawn with what it holds; one removed meanwhile is not changed.
  const editing = providers.find((p) => p.id === PROVIDER_EDITING) || null;
  if (!editing) PROVIDER_EDITING = null;
  const rows = providers.map((p) => `
    <li class="mlist-row">
      <div class="grow">
        <div class="title" style="font-weight:500;">${esc(p.name)}</div>
        <div class="sub">${esc(p.url)} · ${p.local ? "on your network" : "cloud"} · ${p.key_set ? "key set" : "no key"}</div>
      </div>
      <div class="mlist-acts">
        <button class="ghost small" data-act="mlist:provideredit:${p.id}">Edit</button>
        <button class="ghost small danger" data-act="mlist:providerremove:${p.id}" aria-label="Remove">×</button>
      </div>
    </li>`).join("");
  return `
    <div class="card model-card">
      <div class="model-role">Providers</div>
      <div class="sub" style="margin:-4px 0 6px;">Where models run, each with its key, entered once and used by both lists.</div>
      ${rows ? `<ol class="mlist plain">${rows}</ol>` : `<div class="sub">None yet.</div>`}
      <details class="adv" id="provider-form" ${editing ? "open" : ""}>
        <summary>${editing ? `Change ${esc(editing.name)}` : "Add a provider"}</summary>
        <div class="field"><label>Preset</label>
          <select id="pv-preset" onchange="applyProviderPreset(this.value)"><option value="">Choose a provider…</option>${presetOptions("")}</select></div>
        <div class="field"><label>Name</label><input id="pv-name" placeholder="e.g. Anthropic" value="${esc(editing ? editing.name : "")}" /></div>
        <div class="field"><label>Endpoint</label><input id="pv-url" placeholder="https://api.provider.com/v1" value="${esc(editing ? editing.url : "")}" /></div>
        <div class="field"><label>API key <span style="opacity:.7;">· cloud only; stored securely, never shown</span></label>
          <input id="pv-key" type="password" autocomplete="off" placeholder="${editing ? "•••••• (unchanged, unless the endpoint changes)" : ""}" /></div>
        <div class="row" style="justify-content:flex-end;gap:8px;">
          ${editing ? `<button class="ghost" data-act="mlist:providercancel">Cancel</button>` : ""}
          <button class="primary" data-act="mlist:providersave">${editing ? "Save" : "Add provider"}</button>
        </div>
      </details>
    </div>`;
}

// Fill the provider form from a preset (the key is left as typed).
function applyProviderPreset(name) {
  const p = MODEL_PRESETS[name];
  if (!p) return;
  const set = (id, v) => { const el = document.getElementById(id); if (el && v != null) el.value = v; };
  set("pv-name", p.label.replace(/ \(.*\)$/, ""));
  set("pv-url", p.base_url);
}

function modelsSection() {
  if (!MODEL_LISTS) return `<h3>Models</h3><div class="empty">Loading…</div>`;
  const serving = MODEL_LISTS.serving;
  return `
    <h3>Models</h3>
    <div class="note">Endora asks the first model in a list that works and is cleared for what it is asked. A cloud model sees your messages only with names, places and appointments disguised, and never anything that came from outside (a web page, an email); those stay on your network.</div>

    <div class="card model-card">
      <div class="model-role">Everyday${serving && serving.model ? ` · <span class="sub" style="font-weight:400;">answering now: <b>${esc(serving.model)}</b></span>` : ""}</div>
      <div class="sub" style="margin:-4px 0 6px;">The conversation. If none answers, the model Endora was installed with does.</div>
      ${modelListRows("everyday")}
      ${modelListAdd("everyday")}
    </div>

    <div class="card model-card">
      <div class="model-role">Deep</div>
      <div class="sub" style="margin:-4px 0 6px;">A stronger model for hard questions: when you ask for it, and on its own if you let it.</div>
      ${modelListRows("deep")}
      ${modelListAdd("deep")}
      ${onOffRow(!!MODEL_LISTS.escalate, "mlist:escalate", "Let Endora use these on its own", "To step in when the everyday model can't answer, and to word your daily brief. Your name, place names and appointment titles are swapped for placeholders before anything leaves, but the shape of your day still goes out.")}
      <div class="field" style="margin-top:10px;"><label>Balance and use</label>
        <div id="deep-spend" class="ctx-spend">${deepSpendHtml() || '<span class="sub">Nothing used yet.</span>'}</div>
        <div class="row" style="justify-content:flex-end;"><button class="ghost" data-act="deepspend:refresh" style="font-size:13px;">Refresh</button></div></div>
      <details class="adv"><summary>Prices, to show dollars</summary>
        <div class="sub" style="margin:4px 0 8px;">What a million tokens costs, in US dollars, from your provider's price page. Endora does not know any provider's prices, so a dollar figure only appears once you set these. Leave one blank to clear it.</div>
        <div class="sampling-row">
          <label>Input<input id="d-price-in" inputmode="decimal" placeholder="e.g. 0.30" value="${esc(DEEP_USAGE && DEEP_USAGE.prices && DEEP_USAGE.prices.input != null ? DEEP_USAGE.prices.input : "")}" /></label>
          <label>Output<input id="d-price-out" inputmode="decimal" placeholder="e.g. 1.20" value="${esc(DEEP_USAGE && DEEP_USAGE.prices && DEEP_USAGE.prices.output != null ? DEEP_USAGE.prices.output : "")}" /></label>
          <label>Cached input<input id="d-price-cached" inputmode="decimal" placeholder="optional" value="${esc(DEEP_USAGE && DEEP_USAGE.prices && DEEP_USAGE.prices.cached != null ? DEEP_USAGE.prices.cached : "")}" /></label>
        </div>
        <div class="row" style="justify-content:flex-end;margin-top:6px;"><button class="ghost" data-act="deepprices:save" style="font-size:13px;">Save prices</button></div>
      </details>
    </div>

    ${providersCard()}`;
}

// When Endora reaches out on its own: the daily brief. Local hours shown, saved as UTC. (The
// overnight review and the check-ins were removed on 2026-10-16.)
function proactivitySection() {
  const tzOff = new Date().getTimezoneOffset() / 60;
  const hourSelect = (id, change, hourUtc, on) => {
    const localHour = on ? ((hourUtc - tzOff) % 24 + 24) % 24 : -1;
    const opts = Array.from({ length: 24 }, (_, h) => {
      const ampm = h < 12 ? "AM" : "PM"; const h12 = (h % 12) || 12;
      return `<option value="${h}" ${h === localHour ? "selected" : ""}>${h12}:00 ${ampm}</option>`;
    }).join("");
    return `<select id="${id}" data-change="${change}" style="width:auto;"><option value="off" ${!on ? "selected" : ""}>Off</option>${opts}</select>`;
  };
  const row = (label, note, control) => `
    <div class="row" style="align-items:center; gap:10px; margin-bottom:12px;">
      <div class="grow"><div class="title" style="font-weight:500;">${label}</div><div class="sub">${note}</div></div>
      ${control}
    </div>`;
  return `
    <h3>When Endora reaches out</h3>
    <div class="card">
      ${row("Daily brief", "weather · safety · news, once a day", hourSelect("brief-time", "briefsched", BRIEF_SCHED.hour_utc, BRIEF_SCHED.enabled))}
      <div class="row" style="justify-content:flex-end;"><button class="ghost" data-act="brief">${icon("sparkle", 15)} Brief me now</button></div>
    </div>`;
}

// One home for app settings — so they aren't scattered across pages.
function viewSettings() {
  const row = (on, act, label, note) => `
    <div class="row" style="align-items:flex-start;gap:10px;">
      <div class="grow"><div class="title" style="font-weight:500;">${label}</div>${note ? `<div class="sub">${note}</div>` : ""}</div>
      <button class="${on ? "primary" : "ghost"}" data-act="${act}">${on ? "On" : "Off"}</button>
    </div>`;
  const nav = (act, ic, label, note) => `
    <button class="ghost" data-act="${act}">${icon(ic)}
      <span class="grow" style="text-align:left;">${label}${note ? `<span class="sub" style="display:block;font-weight:400;">${note}</span>` : ""}</span>
      ${icon("chevron", 15)}</button>`;
  // One list of rows, each leading somewhere — nothing configured inline. Settings was
  // a pile of toggles with a "Manage" list buried under it; this is the Manage list all
  // the way down, so every category is findable in one glance.
  return `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: "Settings" }])}
    <h2>Settings</h2>
    <div class="card nav-list">
      ${UNDER_SETTINGS.map((p) => nav(`go:${p.view}`, p.icon, p.label, p.note)).join("")}
    </div>`;
}

// Taking a copy of what Endora knows, and deleting it: the person's rights over their memory,
// on their own screen rather than a red button at the foot of Settings.
function viewData() {
  return `
    ${settingsCrumbs("data")}
    <h2>Your data</h2>
    <div class="card nav-list">
      <button class="ghost" data-act="export">${icon("export")}
        <span class="grow" style="text-align:left;">Export my data<span class="sub" style="display:block;font-weight:400;">Endora's memory of you as a file: messages, beliefs, preferences, research and its record of what it did. It is not a backup: settings and keys are not in it, and nothing can import it.</span></span>
      </button>
    </div>
    <div class="card nav-list" style="margin-top:14px;">
      <button class="ghost danger" data-act="purge">${icon("purge")}
        <span class="grow" style="text-align:left;">Delete everything<span class="sub" style="display:block;font-weight:400;">every message, belief and record — this cannot be undone. Your settings, keys and models stay, and so does the team</span></span>
      </button>
    </div>`;
}

// What Endora made of the mail (ADR 0097): what is worth a look, then every bucket folded.
// Only what it kept — who, a checked sentence, why it matters. Never a message body, and no
// link to follow: nothing on this screen opens anything in the mail.
function viewMail() {
  const head = `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: "Mail" }])}
    <h2>Mail</h2>`;
  if (!MAIL) return `${head}<div class="note">Loading…</div>`;
  if (!MAIL.mounted) {
    return `${head}<div class="note">No mail copy is connected. Set up the ansible-nas
      <code>mail_mirror</code> role, then set <code>ENDORA_MAIL_DIR</code> in local.mk and deploy.</div>`;
  }
  const when = (ms) => new Date(ms).toLocaleString([], { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
  const row = (m, showWhy) => `
    <div class="card">
      <div class="sub">${esc(when(m.at_ms))} · ${esc(m.from)}${m.amount ? ` · <strong>${esc(m.amount)}</strong>` : ""}</div>
      <div class="title" style="font-weight:400;">${esc(m.sentence || "(no summary)")}</div>
      ${showWhy && (m.why || []).length ? `<div class="sub">${esc(m.why.join(" · "))}${m.more_like_it ? ` · and ${m.more_like_it} more like it` : ""}</div>` : ""}
      ${(m.dates || []).length ? `<div class="sub">${m.dates.map((d) => `${esc(d.date)}: ${esc(d.what)}`).join(" · ")}</div>` : ""}
      ${m.appointment ? `<div class="sub">Follows "${esc(m.appointment.title)}" on ${esc(m.appointment.day)}</div>` : ""}
      ${(m.cautions || []).length ? `<div class="sub" style="color:var(--warn, #b45309);">${esc(m.cautions.join(" · "))}</div>` : ""}
    </div>`;
  const status = MAIL.reading
    ? `Reading in quiet time. ${MAIL.unread_recent ? `${MAIL.unread_recent} of the last 90 days still to read.` : "Caught up."}`
    : "Not reading right now: turn on ENDORA_OWN_WORK to let Endora read the mail.";
  const important = (MAIL.important || []).map((m) => row(m, true));
  const buckets = (MAIL.buckets || []).filter((b) => b.count).map((b) => `
    <details class="card">
      <summary><span class="title">${esc(b.title)}</span> <span class="pill">${b.count}</span></summary>
      ${(b.recent || []).map((m) => row(m, true)).join("")}
    </details>`);
  return `${head}
    <div class="note" style="margin-bottom:10px;">${esc(status)} ${MAIL.indexed} messages indexed. The last two weeks are shown.</div>
    <h3>Worth a look</h3>
    ${listOr(important, "Nothing that needs you in the last two weeks.")}
    <h3>Coming up</h3>
    ${listOr((MAIL.coming_up || []).map((u) => `<div class="card"><div class="title" style="font-weight:400;">${esc(u.date)} · ${esc(u.what)}</div><div class="sub">${esc(u.from)} · ${esc(u.because)}</div></div>`), "No confirmed dates from your mail.")}
    ${MAIL.held_back ? `<div class="note">${MAIL.held_back} date${MAIL.held_back === 1 ? "" : "s"} held back: mentioned once, by someone you don't write to.</div>` : ""}
    <h3>By bucket</h3>
    ${listOr(buckets, "Nothing read yet.")}`;
}

// The plain on/off preferences. Their own screen, so Settings can stay a list.
function viewDisplay() {
  const row = (on, act, label, note) => `
    <div class="row" style="align-items:flex-start;gap:10px;">
      <div class="grow"><div class="title" style="font-weight:500;">${label}</div>${note ? `<div class="sub">${note}</div>` : ""}</div>
      <button class="${on ? "primary" : "ghost"}" data-act="${act}">${on ? "On" : "Off"}</button>
    </div>`;
  return `
    ${settingsCrumbs("display")}
    <h2>Preferences</h2>
    <div class="card" style="display:flex;flex-direction:column;gap:14px;">
      ${row(SPEAK, "toggle:speak", "Read replies aloud", TTS ? "" : "not supported in this browser")}
      ${row(SHOW_ACTIVITY, "toggle:activity", "Show Endora's actions", "a note of what it did each turn")}
      ${navigator.vibrate ? row(HAPTIC, "toggle:haptic", "Vibrate", "a short buzz when a reply lands and when the mic starts listening") : ""}
      <div class="row" style="align-items:flex-start;gap:10px;">
        <div class="grow"><div class="title" style="font-weight:500;">Start on</div><div class="sub">the screen this device opens on; each device keeps its own</div></div>
        ${[["chat", "Chat"], ["hq", "HQ"]].map(([v, l]) => `<button class="${startOn() === v ? "primary" : "ghost"}" data-act="starton:${v}">${l}</button>`).join("")}
      </div>
    </div>
    ${proactivitySection()}`;
}

function viewModels() {
  return `
    ${settingsCrumbs("models")}
    <h2>AI</h2>
    ${aiOverviewSection()}
    ${modelsSection()}
    <h3>The Mac's brain</h3>
    ${hqMacSetup()}
    ${worthKnowingSection()}`;
}

// Every model Endora uses, one card each: what it does, which model, where it runs, what the
// model server says about it now, and what it sees. The mail reader's model and the speech
// server are chosen here; the conversation and deep models below; vision on its skill.
function aiOverviewSection() {
  if (!AI) return `<h3>What Endora runs</h3><div class="note">Asking the model server…</div>`;
  const ctx = AI.context_tokens || 0;
  const card = (r) => {
    const where = r.on === false
      ? `<span class="pill">off</span>`
      : r.local ? `<span class="pill concluded">on your network</span>` : `<span class="pill">cloud</span>`;
    const rt = r.runtime;
    const runtime = rt ? `<div class="sub">Running now on the ${esc(rt.processor)} · ${esc(String(rt.memory_gb))} GB${rt.context ? ` · ${Number(rt.context).toLocaleString()}-token context` : ""}${rt.size ? ` · ${esc(rt.size)}` : ""}</div>` : "";
    // Endora trims what it sends to the window it believes the model has; a server giving less
    // cuts the start of long turns off without saying so.
    const short = rt && rt.context && ctx && rt.context < ctx
      ? `<div class="sub" style="color:var(--warn, #b45309);">The model server gives ${Number(rt.context).toLocaleString()} tokens, but Endora assumes ${Number(ctx).toLocaleString()}: long turns lose their beginning. Raise the server's context length, or set ENDORA_CONTEXT_TOKENS to match.</div>`
      : "";
    const cost = r.id === "deep" && r.on
      ? `<div class="sub">${r.calls_this_month || 0} call${r.calls_this_month === 1 ? "" : "s"} this month${r.cost_usd_this_month != null ? ` · $${Number(r.cost_usd_this_month).toFixed(2)}` : ""}</div>`
      : "";
    // Folded: most visits are to see what runs, not to change it.
    const field = (id, value, placeholder, act) => `
      <details class="adv" style="margin-top:6px;"><summary>Change</summary>
      <div class="form" style="margin-top:8px;">
        <input id="${id}" value="${esc(value || "")}" placeholder="${esc(placeholder)}" autocomplete="off" />
        <button class="ghost" data-act="${act}">Save</button>
      </div></details>`;
    // A role with nothing to change (dictation is the browser's) offers nothing.
    const change = !r.change
      ? ""
      : r.change === "mail_reader"
        ? field("ai-mail-reader", AI.mail_reader_model, `blank: the deployment's (${r.model})`, "ai:mailreader")
        : r.change === "vision"
          ? `<div class="sub" style="margin-top:6px;"><a data-act="go:skill:image_review">Choose it on the Image review skill</a></div>`
          : `<div class="sub" style="margin-top:6px;">Change it below.</div>`;
    return `
      <div class="card">
        <div class="title">${esc(r.name)} ${where}</div>
        <div class="sub">${esc(r.does)}</div>
        <div class="sub" style="margin-top:4px;"><b>${esc(r.model || (r.id === "speech" ? "your browser" : "not set"))}</b>${r.endpoint ? ` at ${esc(r.endpoint)}` : ""}</div>
        ${runtime}${short}${cost}
        <div class="sub" style="margin-top:4px;">Sees: ${esc(r.sees)}</div>
        ${change}
      </div>`;
  };
  return `
    <h3>What Endora runs</h3>
    ${listOr((AI.roles || []).map(card), "Nothing to show.")}`;
}

// Models the hub has that would fit this machine (ADR 0055).
//
// Reports; it does not fetch. Endora doesn't manage the model runtime — it says what
// exists and hands over the command to run. And it asks rather than guessing how much
// card there is, because it runs in a container and cannot see it.
function worthKnowingSection() {
  const rows = (WORTH_KNOWING.models || []).map((m) => `
    <div class="card"><div class="row" style="align-items:flex-start;gap:10px;">
      <div class="grow">
        <div class="title" style="font-weight:500;">${esc(m.id)}</div>
        <div class="sub">about ${m.about_gb} GB at 4-bit · updated ${esc(m.updated)} · ${Number(m.downloads).toLocaleString()} downloads</div>
        <div class="sub" style="margin-top:4px;"><code>${esc(m.how_to_get_it)}</code></div>
      </div>
    </div></div>`).join("");
  return `
    <h3 style="margin-top:22px;">Worth knowing about</h3>
    <div class="note" style="margin-bottom:10px;">Recent models that would fit your card, most-used first. Endora doesn't download anything — run the command yourself, then it'll be scored with the rest next time the model layer runs. Sizes are estimated from the name.</div>
    <div class="card">
      <div class="row" style="gap:8px;align-items:center;">
        <label class="sub" for="vram-gb">Card size</label>
        <input id="vram-gb" type="number" min="2" max="200" value="${WORTH_KNOWING.fits_gb || 12}" style="width:5.5em;" />
        <span class="sub">GB</span>
        <button class="ghost" data-act="models:look">${icon("sparkle", 14)} Look</button>
      </div>
    </div>
    ${WORTH_KNOWING.asked ? listOr(rows, "Nothing recent that fits — which is a fine answer.") : ""}`;
}

// Kept only so an older link or a back button lands on the screen that now holds it.
// Removing a route is how a bookmark becomes a blank page.
function viewProactive() {
  return viewDisplay();
}

// What the person has told Endora to remember — visible, correctable, deletable. A section of
// About you, beside what Endora has learned on its own.
function rememberedSection() {
  // Friendly labels — the internal kind names ("taste") mean nothing to a person.
  const KIND_LABEL = { taste: "preference", authority: "permission", context: "context" };
  const rows = (DB.preferences || []).map((p) => `
    <div class="card"><div class="row">
      <div class="grow"><div class="title">${esc(p.text)}</div>
      <div class="sub">${esc(KIND_LABEL[p.kind] || p.kind)}</div></div>
      <button class="ghost danger" data-act="delete:pref:${p.id}" title="forget this">${icon("purge",15)}</button>
    </div></div>`);
  return `
    <h3 style="margin-top:16px;">What you've told it</h3>
    ${listOr(rows, "Nothing yet. Endora will offer to remember things as you talk — or add one below.")}
    <div class="form">
      <input id="new-pref" placeholder="e.g. I prefer mornings…" />
      <button class="primary" data-act="create:pref">Remember</button>
    </div>`;
}

