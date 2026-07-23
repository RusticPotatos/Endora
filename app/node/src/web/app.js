"use strict";

let DB = null;                 // latest /v1/export snapshot
let ACTIVITY = [];             // latest /v1/activity feed (newest first)
let ATTENTION = [];            // latest /v1/attention items (most pressing first)
let CHAT_PROPOSALS = [];       // proposals from the butler's last reply
let SUGGESTIONS = [];          // pending suggestions (the butler's durable proposal inbox)
let CHECKIN = { enabled: false, interval_ms: 0 }; // proactive check-in cadence
let CAPS = [];                 // the butler's capabilities/skills (modules)
let AUTONOMY = { auto_external: true, auto_consequential: false }; // the autonomy envelope (ADR 0022)
let BRIEF_SCHED = { enabled: false, hour_utc: 12 }; // daily-brief schedule
let NIGHT_SCHED = { enabled: false, hour_utc: 3 }; // nightly self-improvement loop (ADR 0024)
let DEEP_MODEL = { configured: false, key_set: false, url: "", model: "" }; // optional bigger AI
let MODEL_CONFIG = { configured: false, key_set: false, base_url: "", mixture: false,
  single: {}, router: {}, synth: {} }; // the butler's own models, editable at runtime (ADR 0027)
let TUNE_SCHED = { enabled: false, hour_utc: 4 }; // nightly self-improving model tune (ADR 0027)
let UNDERSTANDING = [];        // Endora's beliefs about the person (the home surface)
let LAST_ACTIVITY = [];        // what Endora did behind the scenes on the last turn
let LAST_ACTIVITY_MSG = null;  // the butler message id that activity belongs to
let STEP_LIST = [];            // the live action trail for the turn currently streaming
let SHOW_ACTIVITY = localStorage.getItem("endora.showActivity") !== "0"; // default on
let CHAT_STREAMING = false;    // true while a reply is streaming in (guards live-render)
let CHAT_QUEUE = [];           // messages awaiting their turn — turns are SERIALIZED
let CHAT_ABORT = null;         // AbortController for the in-flight turn (the Stop button)
let NAV = { v: "chat" };       // current view (the butler is the landing page)

const app = document.getElementById("app");
const msgEl = document.getElementById("msg");

// ---- icons ----------------------------------------------------------------
// A small monochrome line-icon set (inherits currentColor) so the console reads
// consistently instead of relying on the browser's emoji rendering.
const ICONS = {
  chat: '<path d="M21 11.5a8.4 8.4 0 0 1-12.9 7.1L3 20.5l1.9-4.8A8.4 8.4 0 1 1 21 11.5z"/><path d="M8.5 10.5h7M8.5 13.5h4.5"/>',
  prefs: '<line x1="4" y1="8" x2="20" y2="8"/><line x1="4" y1="16" x2="20" y2="16"/><circle cx="15" cy="8" r="2.4"/><circle cx="9" cy="16" r="2.4"/>',
  audit: '<line x1="9" y1="6" x2="20" y2="6"/><line x1="9" y1="12" x2="20" y2="12"/><line x1="9" y1="18" x2="20" y2="18"/><circle cx="4.5" cy="6" r="1.1"/><circle cx="4.5" cy="12" r="1.1"/><circle cx="4.5" cy="18" r="1.1"/>',
  export: '<path d="M12 3v12M8 11l4 4 4-4"/><path d="M5 20h14"/>',
  purge: '<path d="M4 7h16M9 7V4h6v3M18 7l-1 13H7L6 7"/><path d="M10 11v6M14 11v6"/>',
  clock: '<circle cx="12" cy="12" r="8.2"/><path d="M12 8v4.3l2.8 1.7"/>',
  tag: '<path d="M3.5 12.5V5a1.5 1.5 0 0 1 1.5-1.5h7.5L21 12l-8.5 8.5z"/><circle cx="7.8" cy="7.8" r="1.3"/>',
  target: '<circle cx="12" cy="12" r="8.2"/><circle cx="12" cy="12" r="3.3"/>',
  speakerOn: '<path d="M4 9.5v5h3.5L12 18V6L7.5 9.5H4z"/><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"/>',
  speakerOff: '<path d="M4 9.5v5h3.5L12 18V6L7.5 9.5H4z"/><line x1="16" y1="9.5" x2="21" y2="14.5"/><line x1="21" y1="9.5" x2="16" y2="14.5"/>',
  mic: '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0"/><line x1="12" y1="17.5" x2="12" y2="21"/><line x1="9" y1="21" x2="15" y2="21"/>',
  send: '<path d="M4.5 12h13M11 5.5l6.5 6.5-6.5 6.5"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2.5"/>',
  chevron: '<path d="M9 6l6 6-6 6"/>',
  note: '<path d="M6 3h9l4 4v14H6z"/><path d="M15 3v4h4M9 12h6M9 16h4"/>',
  scale: '<path d="M12 4v16M7 20h10"/><path d="M4 8h16M4 8l-2.2 5a2.6 2.6 0 0 0 4.4 0zM20 8l-2.2 5a2.6 2.6 0 0 0 4.4 0z"/><path d="M8 5h8"/>',
  sparkle: '<path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z"/>',
  inbox: '<path d="M4 13l2-8h12l2 8v6H4z"/><path d="M4 13h4l1.5 2.5h5L16 13h4"/>',
  menu: '<line x1="4" y1="7" x2="20" y2="7"/><line x1="4" y1="12" x2="20" y2="12"/><line x1="4" y1="17" x2="20" y2="17"/>',
  skills: '<path d="M12 3l2.5 5 5.5.8-4 3.9 1 5.5-5-2.6-5 2.6 1-5.5-4-3.9 5.5-.8z"/>',
  gear: '<circle cx="12" cy="12" r="3.2"/><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1"/>',
  check: '<path d="M5 13l4 4L19 7"/>',
};
function icon(name, size = 17) {
  const p = ICONS[name];
  if (!p) return "";
  return `<svg class="ic" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${p}</svg>`;
}

// ---- tiny helpers ---------------------------------------------------------
const esc = (s) => String(s).replace(/[&<>"']/g, (c) =>
  ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const byId = (list, id) => (list || []).find((x) => x.id === id);
const shortId = (id) => id.slice(0, 6) + "…";

function flash(text, kind) {
  msgEl.textContent = text;
  msgEl.className = "msg show " + (kind || "ok");
  // A fixed toast, so always auto-dismiss (errors linger a little longer to read).
  clearTimeout(flash._t);
  flash._t = setTimeout(() => (msgEl.className = "msg"), kind === "err" ? 6000 : 2500);
}
function clearMsg() { msgEl.className = "msg"; }

async function api(method, path, body) {
  const res = await fetch(path, {
    method,
    headers: body !== undefined ? { "content-type": "application/json" } : {},
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  let data = null;
  try { data = await res.json(); } catch (_) {}
  if (!res.ok) throw new Error((data && data.error) || ("HTTP " + res.status));
  return data;
}

async function reload() {
  // The export snapshot, the derived activity feed, what needs attention, and the
  // pending suggestions inbox (proposals the butler made, waiting to be applied).
  const [db, activity, attention, suggestions, checkin, caps, autonomy] = await Promise.all([
    api("GET", "/v1/export"),
    api("GET", "/v1/activity?limit=30"),
    api("GET", "/v1/attention"),
    api("GET", "/v1/suggestions?status=pending"),
    api("GET", "/v1/checkin"),
    api("GET", "/v1/capabilities"),
    api("GET", "/v1/autonomy"),
  ]);
  try { BRIEF_SCHED = await api("GET", "/v1/brief/schedule"); } catch (_) {}
  try { NIGHT_SCHED = await api("GET", "/v1/nightly-loop/schedule"); } catch (_) {}
  try { DEEP_MODEL = await api("GET", "/v1/deep-model"); } catch (_) {}
  try { MODEL_CONFIG = await api("GET", "/v1/model-config"); } catch (_) {}
  try { TUNE_SCHED = await api("GET", "/v1/model-tune/schedule"); } catch (_) {}
  DB = db;
  // Attach each butler reply's persisted action trail (steps + sources) from the
  // chat endpoint, so past answers keep their expandable actions after a reload.
  try {
    const hist = await api("GET", "/v1/chat");
    const byId = {};
    for (const m of hist) if (m.actions) byId[m.id] = m.actions;
    for (const m of (DB.messages || [])) if (byId[m.id]) m.actions = byId[m.id];
  } catch (_) {}
  ACTIVITY = activity;
  ATTENTION = attention;
  SUGGESTIONS = suggestions;
  CHECKIN = checkin;
  CAPS = caps;
  AUTONOMY = autonomy;
  try { UNDERSTANDING = await api("GET", "/v1/understanding"); } catch (_) { UNDERSTANDING = []; }
  render();
}

// Subscribe to the node's change stream; every "changed" event refreshes the
// snapshot and feed live. Reconnection is handled by the browser's EventSource.
function subscribeToActivity() {
  try {
    const es = new EventSource("/v1/activity/stream");
    // While a reply is streaming in, don't let a change-event reload wipe the
    // live bubble mid-stream; sendChat reloads to the true state when it ends.
    es.addEventListener("changed", () => { if (!CHAT_STREAMING) reload().catch(() => {}); });
  } catch (_) { /* SSE unavailable: the UI still works, just not live. */ }
}

function go(v, id) { NAV = { v, id }; clearMsg(); closeMenu(); render(); }

// Observations reachable under a target (target → assumptions → experiments → obs).
function observationsForTarget(targetId) {
  const assumptionIds = DB.assumptions.filter((a) => a.target_id === targetId).map((a) => a.id);
  const experimentIds = DB.experiments.filter((e) => assumptionIds.includes(e.assumption_id)).map((e) => e.id);
  return DB.observations.filter((o) => experimentIds.includes(o.experiment_id));
}

const val = (id) => document.getElementById(id).value.trim();

// ---- lifecycle (North Stars & Targets) ------------------------------------
let SHOW_ARCHIVED = false;
const isArchived = (x) => x.status === "archived";
const visible = (items) => (SHOW_ARCHIVED ? items : items.filter((x) => !isArchived(x)));

// A status pill, shown only when not the default "active".
function statusPill(item) {
  return item.status && item.status !== "active"
    ? ` <span class="pill ${item.status}">${esc(item.status)}</span>` : "";
}

// The lifecycle control for a North Star ("direction") or "target": one Status
// dropdown (pick a state to transition to — clearer than a row of look-alike
// verb buttons), with Delete set apart as the one destructive action.
function lifecycleRow(noun, item) {
  const s = item.status || "active";
  const opt = (v, label) => `<option value="${v}" ${s === v ? "selected" : ""}>${label}</option>`;
  return `<div class="row" style="gap:8px; margin-top:8px; align-items:center;">
    <label class="sub" for="st-${noun}-${item.id}">Status</label>
    <select id="st-${noun}-${item.id}" data-change="status:${noun}:${item.id}" style="width:auto;">
      ${opt("active", "Active")}${opt("achieved", "Achieved")}${opt("abandoned", "Abandoned")}${opt("archived", "Archived")}
    </select>
    <span class="grow"></span>
    <button class="ghost danger" data-act="delete:${noun}:${item.id}" title="Delete permanently">${icon("purge",15)}<span>Delete</span></button>
  </div>`;
}

// A toggle shown when there are archived items to reveal/hide.
function archivedToggle(items) {
  const n = items.filter(isArchived).length;
  if (!n && !SHOW_ARCHIVED) return "";
  return `<button class="ghost" data-act="toggle:archived" style="margin-top:6px;">${SHOW_ARCHIVED ? "Hide" : "Show"} archived (${n})</button>`;
}

// An experiment is due for review if a review was scheduled at or before now
// and it is not already concluded (mirrors the domain's is_review_due).
const isReviewDue = (e) => e.review_by_ms != null && e.review_by_ms <= Date.now() && e.status !== "concluded";
function reviewLabel(e) {
  if (e.review_by_ms == null) return "No review scheduled.";
  const when = new Date(e.review_by_ms).toLocaleDateString();
  return isReviewDue(e) ? `${icon("clock", 14)} Review due (${when})` : `Review scheduled for ${when}`;
}

// ---- views ----------------------------------------------------------------
function crumbs(parts) {
  return `<div class="crumbs">` +
    parts.map((p, i) => i < parts.length - 1
      ? `<a data-act="${p.act}">${esc(p.label)}</a> › `
      : `<span>${esc(p.label)}</span>`).join("") +
    `</div>`;
}

function listOr(items, emptyText) {
  return items.length ? items.join("") : `<div class="empty">${esc(emptyText)}</div>`;
}

// A North Star card, reused across the value groups on the home view.
function dirCard(d) {
  return `
    <div class="card click" data-act="go:direction:${d.id}">
      <div class="row"><div class="grow"><div class="title">${esc(d.title)}${statusPill(d)}</div>
      <div class="sub">${DB.targets.filter((g) => g.direction_id === d.id).length} target(s)</div></div>
      <span class="sub">${icon("chevron",16)}</span></div>
    </div>`;
}

// What needs the person's attention, from /v1/attention. Each item can be
// deferred ("Later") with backoff — snoozing repeatedly asks less often.
const ATTENTION_ICON = { review_due: "clock", unfiled_north_star: "tag", empty_north_star: "target" };
// Where clicking an attention item takes you: to the thing that needs the work.
// Review-due points at the experiment's assumption; the North-Star items point
// at the North Star itself (to file it under a value, or add a target).
function attentionAct(a) {
  if (a.kind === "review_due") {
    const e = (DB.experiments || []).find((x) => x.id === a.subject);
    return e ? `go:assumption:${e.assumption_id}` : null;
  }
  return `go:direction:${a.subject}`;
}
function attentionSection() {
  if (!ATTENTION.length) return "";
  const rows = ATTENTION.map((a) => {
    const act = attentionAct(a);
    const clickable = act ? `data-act="${act}" role="button" tabindex="0"` : "";
    return `
    <div class="attn-item">
      <span class="attn-go" ${clickable}>${icon(ATTENTION_ICON[a.kind] || "target", 16)}<span>${esc(a.headline)}</span></span>
      <button class="ghost" data-act="snooze:attention:${a.kind}:${a.subject}" title="ask me later">Later</button>
    </div>`;
  }).join("");
  return `<div class="card attn-card">
    <div class="attn-head">Needs your attention</div>${rows}</div>`;
}

function viewHome() {
  const dueBanner = attentionSection();
  // North Stars grouped under the value they serve, then the unfiled ones.
  const groups = (DB.values || []).map((v) => {
    const ds = visible(DB.directions.filter((d) => d.value_id === v.id));
    return `
      <div class="row" style="align-items:baseline; margin-top:16px;">
        <h3 style="margin:0; flex:1;">${esc(v.name)}</h3>
        <button class="ghost danger" data-act="delete:value:${v.id}" title="delete value">${icon("purge", 15)}</button>
      </div>
      ${ds.length ? ds.map(dirCard).join("") : `<div class="empty">Nothing filed under this value yet.</div>`}`;
  }).join("");
  const unfiled = visible(DB.directions.filter((d) => !d.value_id));
  const unfiledHtml = `
    <h3 style="margin-top:16px;">${(DB.values || []).length ? "Unfiled" : "Goals"}</h3>
    ${listOr(unfiled.map(dirCard), "No goals yet. Create one below.")}`;
  return `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: "Goals" }])}
    ${dueBanner}
    <h2>Goals by value</h2>
    ${groups}
    ${unfiledHtml}
    ${archivedToggle(DB.directions)}
    <div class="form">
      <input id="new-direction" placeholder="A goal…" />
      <button class="primary" data-act="create:direction">Add goal</button>
    </div>
    <div class="form">
      <input id="new-value" placeholder="A value it serves (e.g. Health)…" />
      <button data-act="create:value">Add value</button>
    </div>`;
}

// The live activity feed: newest first, updated in place by the change stream.
const ACTIVITY_ICON = { observation: "note", decision: "scale", action: "sparkle" };
function activityFeed() {
  if (!ACTIVITY.length) return `<div class="empty">No activity yet.</div>`;
  return ACTIVITY.map((a) => `
    <div class="card"><div class="row">
      <div class="grow"><div class="title">${icon(ACTIVITY_ICON[a.kind] || "note", 15)} ${esc(a.summary)}</div>
      <div class="sub">${esc(a.kind)}</div></div>
      <span class="sub">${esc(new Date(a.at_ms).toLocaleString())}</span>
    </div></div>`).join("");
}

function viewDirection(id) {
  const d = byId(DB.directions, id);
  if (!d) return viewHome();
  const allTargets = DB.targets.filter((g) => g.direction_id === id);
  const targets = visible(allTargets).map((g) => `
    <div class="card click" data-act="go:target:${g.id}">
      <div class="row"><div class="grow"><div class="title">${esc(g.statement)}${statusPill(g)}</div>
      <div class="sub">${DB.assumptions.filter((a) => a.target_id === g.id).length} assumption(s) ·
      ${DB.reflections.filter((r) => r.target_id === g.id).length} reflection(s)</div></div>
      <span class="sub">${icon("chevron",16)}</span></div>
      ${lifecycleRow("target", g)}
    </div>`);
  const valueOptions = `<option value="">(unfiled)</option>` +
    (DB.values || []).map((v) => `<option value="${v.id}" ${d.value_id === v.id ? "selected" : ""}>${esc(v.name)}</option>`).join("");
  return `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: d.title }])}
    <div class="card"><div class="title">${esc(d.title)}${statusPill(d)}</div>
      <div class="row" style="gap:6px; margin-top:8px; align-items:center;">
        <span class="sub">Serves value:</span>
        <select id="val-${d.id}" style="width:auto;">${valueOptions}</select>
        <button data-act="file:direction:${d.id}">File</button>
      </div>
      ${lifecycleRow("direction", d)}</div>
    <h2>Targets under “${esc(d.title)}”</h2>
    ${listOr(targets, "No targets yet.")}
    ${archivedToggle(allTargets)}
    <div class="form">
      <input id="new-target" placeholder="A target under this goal…" />
      <button class="primary" data-act="create:target:${id}">Add target</button>
    </div>`;
}

function viewTarget(id) {
  const g = byId(DB.targets, id);
  if (!g) return viewHome();
  const d = byId(DB.directions, g.direction_id);
  const assumptions = DB.assumptions.filter((a) => a.target_id === id).map((a) => `
    <div class="card click" data-act="go:assumption:${a.id}">
      <div class="row"><div class="grow"><div class="title">${esc(a.statement)}</div>
      <div class="sub">${DB.experiments.filter((e) => e.assumption_id === a.id).length} experiment(s)</div></div>
      <span class="sub">${icon("chevron",16)}</span></div>
    </div>`);
  const reflections = DB.reflections.filter((r) => r.target_id === id).map((r) => `
    <div class="card click" data-act="go:reflection:${r.id}">
      <div class="row"><div class="grow"><div class="title">${esc(r.summary)}</div>
      <div class="sub">${r.evidence.length} observation(s) cited ·
      ${DB.process_changes.filter((c) => c.reflection_id === r.id).length} proposed change(s)</div></div>
      <span class="sub">${icon("chevron",16)}</span></div>
    </div>`);
  const obs = observationsForTarget(id);
  const eviBoxes = obs.length
    ? obs.map((o) => `<label class="evi"><input type="checkbox" class="evi-box" value="${o.id}" /> ${esc(o.note)}</label>`).join("")
    : `<div class="sub">Record observations under an experiment first to cite them here.</div>`;
  return `
    ${crumbs([{ label: "Home", act: "go:chat" },
              { label: d ? d.title : "…", act: "go:direction:" + g.direction_id },
              { label: g.statement }])}
    <h2>Assumptions</h2>
    ${listOr(assumptions, "No assumptions yet.")}
    <div class="form">
      <input id="new-assumption" placeholder="A belief this target rests on…" />
      <button class="primary" data-act="create:assumption:${id}">Add assumption</button>
    </div>
    <h2>Reflections</h2>
    ${listOr(reflections, "No reflections yet.")}
    <div class="card">
      <div class="stack">
        <textarea id="new-reflection" placeholder="What the evidence says so far…"></textarea>
        <div class="sub">Cite evidence:</div>
        ${eviBoxes}
        <div><button class="primary" data-act="create:reflection:${id}">Add reflection</button></div>
      </div>
    </div>`;
}

function viewAssumption(id) {
  const a = byId(DB.assumptions, id);
  if (!a) return viewHome();
  const g = byId(DB.targets, a.target_id);
  const experiments = DB.experiments.filter((e) => e.assumption_id === id).map((e) => {
    const obs = DB.observations.filter((o) => o.experiment_id === e.id);
    const obsHtml = obs.map((o) => `<div class="sub mono">• ${esc(o.note)}</div>`).join("");
    return `
    <div class="card">
      <div class="row"><div class="grow"><div class="title">${esc(e.hypothesis)}</div></div>
        <span class="pill ${e.status}">${e.status}</span></div>
      <div class="row" style="margin-top:8px; gap:6px;">
        <button data-act="start:experiment:${e.id}" ${e.status !== "proposed" ? "disabled" : ""}>Start</button>
        <button data-act="conclude:experiment:${e.id}" ${e.status !== "running" ? "disabled" : ""}>Conclude</button>
      </div>
      ${e.status !== "concluded" ? `
      <div class="row" style="margin-top:8px; gap:6px; align-items:center;">
        <span class="sub grow">${esc(reviewLabel(e))}</span>
        <input id="rev-${e.id}" type="number" min="1" value="7" style="width:70px;" title="days from now" />
        <button data-act="review:experiment:${e.id}">Remind me</button>
      </div>` : ""}
      <div style="margin-top:8px;">${obsHtml || '<div class="sub">No observations.</div>'}</div>
      <div class="form">
        <input id="obs-${e.id}" placeholder="Record an observation…" />
        <button data-act="record:observation:${e.id}">Note</button>
      </div>
    </div>`;
  });
  return `
    ${crumbs([{ label: "Home", act: "go:chat" },
              { label: g ? g.statement : "…", act: "go:target:" + a.target_id },
              { label: a.statement }])}
    <h2>Experiments testing “${esc(a.statement)}”</h2>
    ${listOr(experiments, "No experiments yet.")}
    <div class="form">
      <input id="new-experiment" placeholder="A small, bounded test…" />
      <button class="primary" data-act="propose:experiment:${id}">Propose experiment</button>
    </div>`;
}

function viewReflection(id) {
  const r = byId(DB.reflections, id);
  if (!r) return viewHome();
  const g = byId(DB.targets, r.target_id);
  const changes = DB.process_changes.filter((c) => c.reflection_id === id).map((c) => `
    <div class="card">
      <div class="row"><div class="grow"><div class="title">${esc(c.description)}</div></div>
        <span class="pill ${c.approval}">${c.approval}</span></div>
      <div class="row" style="margin-top:8px; gap:6px; flex-wrap: wrap;">
        <button data-act="approve:change:${c.id}" ${c.approval !== "pending" ? "disabled" : ""}>Approve</button>
        <button data-act="reject:change:${c.id}" ${c.approval !== "pending" ? "disabled" : ""}>Reject</button>
        <span class="grow"></span>
        <select id="actor-${c.id}" title="actor autonomy level">
          <option value="act_within_policy">act within policy</option>
          <option value="confirm_each_action">confirm each action</option>
          <option value="suggest">suggest</option>
          <option value="observe">observe</option>
        </select>
        <button data-act="decide:change:${c.id}">Run policy</button>
      </div>
    </div>`);
  return `
    ${crumbs([{ label: "Home", act: "go:chat" },
              { label: g ? g.statement : "…", act: "go:target:" + r.target_id },
              { label: "Reflection" }])}
    <h2>Reflection</h2>
    <div class="card"><div class="title">${esc(r.summary)}</div>
      <div class="sub">Cites ${r.evidence.length} observation(s).</div></div>
    <h2>Proposed process changes</h2>
    ${listOr(changes, "No proposed changes yet.")}
    <div class="form">
      <input id="new-change" placeholder="A process change this reflection suggests…" />
      <button class="primary" data-act="propose:change:${id}">Propose</button>
      <button data-act="draft:change:${id}" title="let the local model draft one">${icon("sparkle")}<span>Draft with model</span></button>
    </div>`;
}

function viewAudit() {
  const rows = (DB.audit || []).map((a) => `
    <div class="card"><div class="sub mono">${new Date(a.at_ms).toLocaleString()}</div>
      <div>${esc(a.summary)}</div></div>`);
  return `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: "Activity" }])}
    <h2>Recent activity</h2>
    ${activityFeed()}
    <h2 style="margin-top:22px;">Audit trail (newest first)</h2>
    ${listOr(rows, "No decisions recorded yet.")}`;
}

// ---- voice (browser Web Speech API; STT may use a cloud service) ---
let SPEAK = localStorage.getItem("endora.speak") === "1";      // read replies aloud (default OFF)
let PTT_HAPTIC = localStorage.getItem("endora.haptic") !== "0"; // buzz on push-to-talk (default on)
const TTS = window.speechSynthesis;
const STT = window.SpeechRecognition || window.webkitSpeechRecognition;
let STT_AVAILABLE = false; // a Whisper STT server is configured (set from /health)
// iOS Safari only lets speech start after a user gesture unlocks it; speaking a
// silent utterance the moment the person taps "Speak" unlocks it for the later,
// async replies. (This is best-effort — a fully reliable mobile voice is the
// local Kokoro TTS path.)
function unlockSpeech() {
  if (!TTS) return;
  try {
    const u = new SpeechSynthesisUtterance(" ");
    u.volume = 0;
    TTS.speak(u);
  } catch (_) {}
}
function speak(text) {
  if (!SPEAK || !TTS || !text) return;
  TTS.cancel();
  const u = new SpeechSynthesisUtterance(text);
  TTS.speak(u);
  // iOS sometimes pauses the queue; a nudge keeps it going.
  if (TTS.paused) TTS.resume();
}
function listen() {
  if (!STT) { flash("Speech recognition isn't available in this browser (try Chrome/Edge).", "err"); return; }
  if (!window.isSecureContext) {
    flash("Voice input needs a secure page (HTTPS or localhost). Over plain http the browser blocks the mic — the butler can still speak its replies.", "err");
    return;
  }
  const rec = new STT();
  rec.lang = "en-US"; rec.interimResults = false; rec.maxAlternatives = 1;
  rec.onresult = (e) => {
    const input = document.getElementById("chat-input");
    if (input) { input.value = e.results[0][0].transcript; input.focus(); }
  };
  rec.onerror = (e) => {
    const err = e && e.error;
    if (err === "not-allowed" || err === "service-not-allowed") {
      flash("The browser blocked the mic. Allow microphone access, and use an HTTPS/localhost page.", "err");
    } else {
      flash("Couldn't capture speech" + (err ? " (" + err + ")" : "") + ".", "err");
    }
  };
  try { rec.start(); } catch (_) { flash("Couldn't start voice input.", "err"); }
}

// Push-to-talk: hold the mic to dictate (interim text fills the box live),
// release to send. Uses pointer events so it works with mouse and touch.
let PTT_REC = null;
let PTT_MEDIA = null; // active Whisper recording { recorder, chunks, stream, btn }

// When a Whisper STT server is configured, record real audio and transcribe it
// server-side (accurate, works in any browser) instead of the flaky Web Speech
// API. Falls back to Web Speech when no server is set.
async function startWhisperPTT(btn) {
  if (PTT_MEDIA || !navigator.mediaDevices || !window.MediaRecorder) return;
  if (!window.isSecureContext) { flash("Voice input needs a secure page (HTTPS or localhost).", "err"); return; }
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    const recorder = new MediaRecorder(stream);
    const chunks = [];
    recorder.ondataavailable = (e) => { if (e.data && e.data.size) chunks.push(e.data); };
    recorder.start();
    PTT_MEDIA = { recorder, chunks, stream, btn };
    if (btn) btn.classList.add("recording");
    if (PTT_HAPTIC && navigator.vibrate) navigator.vibrate(25);
  } catch (_) { flash("Couldn't start the mic — allow microphone access.", "err"); }
}
async function stopWhisperPTT(andSend) {
  const m = PTT_MEDIA; if (!m) return; PTT_MEDIA = null;
  if (m.btn) m.btn.classList.remove("recording");
  const done = new Promise((res) => { m.recorder.onstop = res; });
  try { m.recorder.stop(); } catch (_) {}
  await done;
  m.stream.getTracks().forEach((t) => t.stop());
  if (!andSend) return;
  const blob = new Blob(m.chunks, { type: m.recorder.mimeType || "audio/webm" });
  if (!blob.size) return;
  const input = document.getElementById("chat-input");
  const ph = input ? input.placeholder : "";
  if (input) input.placeholder = "Transcribing…";
  try {
    const res = await fetch("/v1/transcribe", { method: "POST", headers: { "content-type": "application/octet-stream" }, body: blob });
    if (!res.ok) throw new Error("transcription failed");
    const data = await res.json();
    if (input && data.text) { input.value = data.text; growInput(input); sendChat(); }
  } catch (e) { flash("Couldn't transcribe that" + (e.message ? " (" + e.message + ")" : "") + ".", "err"); }
  finally { if (input) input.placeholder = ph || "Talk to your butler…"; }
}

function startPTT(btn) {
  if (STT_AVAILABLE) return startWhisperPTT(btn);
  if (!STT) { flash("Speech recognition isn't available in this browser (try Chrome/Edge).", "err"); return; }
  if (!window.isSecureContext) { flash("Voice input needs a secure page (HTTPS or localhost).", "err"); return; }
  if (PTT_REC) return;
  const rec = new STT();
  rec.lang = "en-US"; rec.interimResults = true; rec.continuous = true; rec.maxAlternatives = 1;
  rec.onresult = (e) => {
    let t = "";
    for (let i = 0; i < e.results.length; i++) t += e.results[i][0].transcript;
    const input = document.getElementById("chat-input");
    if (input) { input.value = t; input.style.height = "auto"; input.style.height = input.scrollHeight + "px"; }
  };
  rec.onerror = (e) => {
    const err = e && e.error;
    if (err === "not-allowed" || err === "service-not-allowed") flash("The browser blocked the mic. Allow microphone access.", "err");
    else if (err && err !== "no-speech" && err !== "aborted") flash("Couldn't capture speech (" + err + ").", "err");
  };
  rec.onend = () => { PTT_REC = null; if (btn) btn.classList.remove("recording"); };
  try {
    rec.start(); PTT_REC = rec;
    if (btn) btn.classList.add("recording");
    if (PTT_HAPTIC && navigator.vibrate) navigator.vibrate(25); // haptic cue (Android; iOS Safari ignores)
  } catch (_) { PTT_REC = null; flash("Couldn't start voice input.", "err"); }
}
function stopPTT(andSend) {
  if (PTT_MEDIA) { stopWhisperPTT(andSend); return; }
  const rec = PTT_REC;
  if (!rec) return;
  try { rec.stop(); } catch (_) {}
  // Let the final transcript land, then send if there's anything to send.
  if (andSend) setTimeout(() => {
    const input = document.getElementById("chat-input");
    if (input && input.value.trim()) sendChat();
  }, 250);
}

// The butler chat: the conversation, the last reply's proposals (each
// confirmable), and an input. The butler proposes; you confirm; the normal
// create endpoints execute — the model never acts on its own.
function viewChat() {
  const list = DB.messages || [];
  const msgs = list.map((m) => {
    const mine = m.role === "user";
    const bubble = `<div class="row" style="justify-content:${mine ? "flex-end" : "flex-start"}; margin:6px 0;">
      <div class="bubble ${mine ? "me" : "butler"}">${esc(m.text)}</div></div>`;
    // A butler reply carries its persisted action trail + sources (if any), so
    // you can expand a PAST answer to see what it did and where it came from.
    if (!mine && m.actions) {
      return bubble + stepsHtml(m.actions.steps) + sourcesHtml(m.actions.sources);
    }
    return bubble;
  }).join("");
  // Derived from persisted state, so it survives a reload: if the newest message
  // is yours, the butler still owes a reply — show the thinking indicator. (The
  // reply is always appended within the node's model timeout, so this can't
  // hang forever.)
  const awaiting = list.length > 0 && list[list.length - 1].role === "user";
  const pending = awaiting
    ? `<div class="row" style="justify-content:flex-start; margin:6px 0;" id="chat-pending">
         <div class="bubble butler thinking"><span class="dots"><i></i><i></i><i></i></span></div></div>`
    : "";
  // A subtle note of what Endora did behind the scenes on the latest turn —
  // learnings and inbox additions — so you can see what the conversation changed.
  const last = list[list.length - 1];
  const showActivity = SHOW_ACTIVITY && LAST_ACTIVITY.length && last && last.role === "butler" && last.id === LAST_ACTIVITY_MSG;
  const activity = showActivity
    ? `<div class="activity">${icon("sparkle", 13)} ${LAST_ACTIVITY.map(esc).join(" · ")}</div>`
    : "";
  const proposals = CHAT_PROPOSALS.map((p, i) => `
    <div class="card"><div class="row"><div class="grow"><div class="title">${esc(p.label)}</div>
      <div class="sub">the butler proposes this — you decide</div></div>
      <button class="primary" data-act="confirm:proposal:${i}">Confirm</button>
      <button data-act="dismiss:proposal:${i}">Dismiss</button></div></div>`).join("");
  const speakBtn = TTS
    ? `<button class="ghost" data-act="toggle:speak" title="read replies aloud">${icon(SPEAK ? "speakerOn" : "speakerOff")}<span>${SPEAK ? "Speaking" : "Speak"}</span></button>`
    : "";
  const micBtn = (STT || STT_AVAILABLE)
    ? (window.isSecureContext
        ? `<button class="ptt" data-mic="1" title="push to talk, release to send">${icon("mic")}<span>Push to talk</span></button>`
        : `<button data-act="chat:mic" title="voice input needs HTTPS or localhost">${icon("mic")}<span>needs HTTPS</span></button>`)
    : "";
  return `
    <div class="chat">
      <div id="chat-thread" class="chat-thread">${(msgs || `<div class="empty">Say what you'd like to work on — the butler will help organize it.</div>`) + pending + activity}</div>
      ${awaiting ? "" : proposals}
      <div class="composer">
        <textarea id="chat-input" rows="1" placeholder="Talk to your butler…"></textarea>
        <div class="composer-actions">
          <div class="composer-secondary">
            ${speakBtn}
            ${micBtn}
            ${DEEP_MODEL.configured ? `<button class="ghost" data-act="deepask" title="send this question to your bigger model">${icon("sparkle", 15)}<span>Ask deep</span></button>` : ""}
          </div>
          <button class="primary" id="send-btn" data-act="${CHAT_STREAMING ? "chat:stop" : "chat:send"}">${CHAT_STREAMING ? `${icon("stop")}<span>Stop</span>` : `${icon("send")}<span>Send</span>`}</button>
        </div>
      </div>
    </div>`;
}

// Provider presets for the butler models. Each fills the endpoint + example
// model names + author-recommended sampling (router cold for reliable skill
// selection, synthesizer warmer for natural prose). top_k / repeat_penalty are
// Ollama-only extensions, so cloud presets leave them blank (strict OpenAI
// endpoints reject them). Anthropic runs via OpenRouter — Endora speaks OpenAI.
const MODEL_PRESETS = {
  ollama: { label: "Ollama (local)", base_url: "http://host.docker.internal:11434/v1", key: false,
    single: { model: "qwen2.5:7b",  temperature: 0.5, top_p: 0.8, top_k: 20, repeat_penalty: 1.05 },
    router: { model: "hermes3:8b",  temperature: 0.1, top_p: 0.9, top_k: 20, repeat_penalty: 1.05 },
    synth:  { model: "qwen2.5:7b",  temperature: 0.6, top_p: 0.8, top_k: 20, repeat_penalty: 1.05 } },
  openai: { label: "OpenAI", base_url: "https://api.openai.com/v1", key: true,
    single: { model: "gpt-4o-mini", temperature: 0.5 },
    router: { model: "gpt-4o-mini", temperature: 0.1 },
    synth:  { model: "gpt-4o",      temperature: 0.6 } },
  openrouter: { label: "OpenRouter", base_url: "https://openrouter.ai/api/v1", key: true,
    single: { model: "anthropic/claude-sonnet-5",   temperature: 0.5 },
    router: { model: "anthropic/claude-3.5-haiku",  temperature: 0.1 },
    synth:  { model: "anthropic/claude-sonnet-5",   temperature: 0.6 } },
  anthropic: { label: "Anthropic (via OpenRouter)", base_url: "https://openrouter.ai/api/v1", key: true,
    single: { model: "anthropic/claude-sonnet-5",   temperature: 0.5 },
    router: { model: "anthropic/claude-3.5-haiku",  temperature: 0.1 },
    synth:  { model: "anthropic/claude-sonnet-5",   temperature: 0.6 } },
  groq: { label: "Groq", base_url: "https://api.groq.com/openai/v1", key: true,
    single: { model: "llama-3.3-70b-versatile", temperature: 0.5 },
    router: { model: "llama-3.3-70b-versatile", temperature: 0.1 },
    synth:  { model: "llama-3.3-70b-versatile", temperature: 0.6 } },
};

// Show the single-model fields or the router+synth fields as the mixture toggle
// flips, without a re-render (so typed-but-unsaved values survive).
function toggleMixture(on) {
  const set = (id, show, mode) => { const el = document.getElementById(id); if (el) el.style.display = show ? (mode || "block") : "none"; };
  set("m-single", !on, "flex");      // single model name (top of card)
  set("m-single-adv", !on);          // single model's sampling (in Advanced)
  set("m-mixslots", on);             // router + synth (in Advanced)
}

// Fill the Models form from a preset (leaves the API-key field untouched).
function applyModelPreset(name) {
  const p = MODEL_PRESETS[name];
  if (!p) return;
  const set = (id, v) => { const el = document.getElementById(id); if (el) el.value = (v === undefined || v === null) ? "" : v; };
  set("m-base", p.base_url);
  for (const slot of ["single", "router", "synth"]) {
    const s = p[slot] || {};
    set(`m-${slot}-model`, s.model);
    set(`m-${slot}-temperature`, s.temperature);
    set(`m-${slot}-top_p`, s.top_p);
    set(`m-${slot}-top_k`, s.top_k);
    set(`m-${slot}-repeat_penalty`, s.repeat_penalty);
  }
}

// Fill the Deep-model form from a provider preset (leaves the key untouched).
function applyDeepPreset(name) {
  const p = MODEL_PRESETS[name];
  if (!p) return;
  const set = (id, v) => { const el = document.getElementById(id); if (el && v != null) el.value = v; };
  set("deep-url", p.base_url);
  set("deep-model", (p.single && p.single.model) || "");
}

// One model's name field (stacked, full-width). Backed by the shared "m-models"
// datalist, so a Discover populates a picker for every everyday slot at once.
function modelName(prefix, slot, ph) {
  return `<div class="field"><label>Model</label>
    <input id="m-${prefix}-model" list="m-models" placeholder="${ph}" value="${esc((slot && slot.model) || "")}" /></div>`;
}

// Ask the endpoint for its model list and fill the matching datalist. Uses the
// key typed in the form, or the stored key for the role when that's blank.
async function discoverModels(role) {
  const isDeep = role === "deep";
  const base = ((document.getElementById(isDeep ? "deep-url" : "m-base") || {}).value || "").trim();
  const key = ((document.getElementById(isDeep ? "deep-key" : "m-key") || {}).value || "").trim();
  if (!base) { flash("Enter the endpoint first.", "err"); return; }
  const body = { base_url: base, role };
  if (key) body.api_key = key;
  try {
    const r = await api("POST", "/v1/models/discover", body);
    const models = r.models || [];
    const list = document.getElementById(isDeep ? "deep-models" : "m-models");
    if (list) list.innerHTML = models.map((m) => `<option value="${esc(m)}"></option>`).join("");
    flash(models.length ? `Found ${models.length} models — tap the Model field to pick.` : "No models returned.", models.length ? "ok" : "err");
  } catch (e) { flash("Couldn't list models: " + e.message, "err"); }
}

// Test that an endpoint + API key actually work: sends a minimal completion with
// the chosen model (a real auth check, not just a /models listing) and reports the
// result. Uses the key typed in the card, or the stored key when that's blank.
async function testConnection(role) {
  const isDeep = role === "deep";
  const base = ((document.getElementById(isDeep ? "deep-url" : "m-base") || {}).value || "").trim();
  const key = ((document.getElementById(isDeep ? "deep-key" : "m-key") || {}).value || "").trim();
  let model;
  if (isDeep) {
    model = ((document.getElementById("deep-model") || {}).value || "").trim();
  } else {
    // Everyday: test the synthesizer in a mixture, else the single model.
    const mixOn = !!(document.getElementById("m-mix") || {}).checked;
    const id = mixOn ? "m-synth-model" : "m-single-model";
    model = ((document.getElementById(id) || {}).value || "").trim();
  }
  if (!base) { flash("Enter the endpoint first.", "err"); return; }
  const body = { base_url: base, role, model };
  if (key) body.api_key = key;
  flash("Testing the connection…", "ok");
  try {
    const r = await api("POST", "/v1/models/test", body);
    flash(r.detail || (r.ok ? "Connected." : "Test failed."), r.ok ? "ok" : "err");
  } catch (e) { flash("Test failed: " + e.message, "err"); }
}

// One model's optional sampling knobs (blank = provider default).
function samplingRow(prefix, slot) {
  const nv = (x) => (x === undefined || x === null) ? "" : x;
  const num = (key, label, ph) => `<label>${label}
    <input id="m-${prefix}-${key}" type="number" step="0.05" min="0" placeholder="${ph}" value="${nv((slot || {})[key])}" /></label>`;
  return `<div class="sampling-row">
    ${num("temperature", "temp", "0.5")}${num("top_p", "top_p", "auto")}${num("top_k", "top_k", "off")}${num("repeat_penalty", "repeat", "off")}
  </div>`;
}

// The model configuration — two consistent cards (Everyday + Deep). The everyday
// brain answers day-to-day (local by default); the deep model is a bigger brain
// you escalate to per question. Mixture + sampling live under "Advanced" so the
// common case stays clean (ADR 0027; runtime-swappable, no restart).
function modelsSection() {
  const mc = MODEL_CONFIG || {};
  const dm = DEEP_MODEL || {};
  const options = Object.entries(MODEL_PRESETS)
    .map(([k, v]) => `<option value="${k}">${esc(v.label)}</option>`).join("");
  const mix = !!mc.mixture;
  return `
    <h3>Models</h3>
    <div class="note">A local <b>everyday</b> model, and an optional <b>deep</b> one for hard questions. Any OpenAI-compatible endpoint.</div>

    <div class="card model-card">
      <div class="model-role">Everyday${mc.configured ? "" : ` · <span class="sub" style="font-weight:400;">using deployment default</span>`}</div>
      <div class="field"><label>Provider preset</label>
        <select id="m-preset" onchange="applyModelPreset(this.value)"><option value="">Choose a provider…</option>${options}</select></div>
      <div class="field"><label>Endpoint</label>
        <input id="m-base" placeholder="http://host.docker.internal:11434/v1" value="${esc(mc.base_url || "")}" /></div>
      <div class="field"><label>API key <span style="opacity:.7;">· cloud only</span></label>
        <input id="m-key" type="password" autocomplete="off" placeholder="${mc.key_set ? "•••••• (unchanged)" : "stored securely, never shown"}" /></div>
      <div class="row" style="gap:8px;"><button class="ghost" data-act="discover:everyday" style="font-size:13px;">${icon("sparkle", 14)} Discover models</button><button class="ghost" data-act="testconn:everyday" style="font-size:13px;">${icon("check", 14)} Test connection</button></div>
      <datalist id="m-models"></datalist>
      <div id="m-single" style="display:${mix ? "none" : "flex"};flex-direction:column;gap:12px;">
        ${modelName("single", mc.single, "e.g. qwen2.5:7b")}
      </div>
      <details class="adv" ${mix ? "open" : ""}>
        <summary>Advanced — mixture &amp; sampling</summary>
        <label class="mix-toggle" title="A cold, tool-tuned router picks the skill; a warmer synthesizer writes the reply. Beats a single model at less VRAM.">
          <input id="m-mix" type="checkbox" ${mix ? "checked" : ""} onchange="toggleMixture(this.checked)" />
          <span>Split into a <b>router</b> + <b>synthesizer</b> <span style="opacity:.6;">(advanced)</span></span>
        </label>
        <div id="m-single-adv" style="display:${mix ? "none" : "block"};">
          <div class="sub" style="margin:2px 0 4px;">Sampling (blank = provider default)</div>
          ${samplingRow("single", mc.single)}
        </div>
        <div id="m-mixslots" style="display:${mix ? "block" : "none"};">
          <div class="sub" style="margin:8px 0 2px;">Router — cold, picks the skill</div>
          ${modelName("router", mc.router, "e.g. hermes3:8b")}
          ${samplingRow("router", mc.router)}
          <div class="sub" style="margin:10px 0 2px;">Synthesizer — warm, writes the reply</div>
          ${modelName("synth", mc.synth, "e.g. qwen2.5:7b")}
          ${samplingRow("synth", mc.synth)}
        </div>
      </details>
      <div class="row" style="justify-content:flex-end;"><button class="primary" data-act="modelsave">Save everyday</button></div>
    </div>

    <div class="card model-card" style="margin-top:14px;">
      <div class="model-role">Deep <span class="sub" style="font-weight:400;">· a bigger brain for hard questions</span></div>
      <div class="sub" style="margin:-4px 0 2px;">Optional, opt-in per question. It leaves your device, so it passes the same egress guard.</div>
      <div class="field"><label>Provider preset</label>
        <select id="d-preset" onchange="applyDeepPreset(this.value)"><option value="">Choose a provider…</option>${options}</select></div>
      <div class="field"><label>Endpoint</label>
        <input id="deep-url" placeholder="https://api.provider.com/v1" value="${esc(dm.url || "")}" /></div>
      <div class="field"><label>API key</label>
        <input id="deep-key" type="password" autocomplete="off" placeholder="${dm.key_set ? "•••••• (unchanged)" : "stored securely, never shown"}" /></div>
      <div class="row" style="gap:8px;"><button class="ghost" data-act="discover:deep" style="font-size:13px;">${icon("sparkle", 14)} Discover models</button><button class="ghost" data-act="testconn:deep" style="font-size:13px;">${icon("check", 14)} Test connection</button></div>
      <datalist id="deep-models"></datalist>
      <div class="field"><label>Model</label>
        <input id="deep-model" list="deep-models" placeholder="e.g. gpt-4o, claude-sonnet-5" value="${esc(dm.model || "")}" /></div>
      <div class="row" style="justify-content:flex-end;"><button class="primary" data-act="deepsave">Save deep</button></div>
    </div>

    <h3>Auto-tune <span class="sub" style="font-weight:400;">· experimental</span></h3>
    <div class="note">Scores the models on your endpoint and adopts the best local one on its own. Takes a few minutes and uses the GPU — watch <a class="link" data-act="go:audit">Activity</a> for the result.</div>
    <div class="card model-card">
      <label class="mix-toggle">
        <input id="tune-nightly" type="checkbox" ${TUNE_SCHED.enabled ? "checked" : ""} />
        <span>Run it <b>automatically overnight</b> and adopt the best local model on its own.</span>
      </label>
      <div class="field"><label>Hour to run (UTC)</label>
        <select id="tune-hour">${Array.from({ length: 24 }, (_, h) => `<option value="${h}" ${h === (TUNE_SCHED.hour_utc ?? 4) ? "selected" : ""}>${String(h).padStart(2, "0")}:00 UTC</option>`).join("")}</select></div>
      <div class="row" style="justify-content:space-between;align-items:center;">
        <button class="ghost" data-act="modeltune">Run now</button>
        <button class="primary" data-act="tunesave">Save schedule</button>
      </div>
    </div>`;
}

// When Endora reaches out on its own — the daily brief, the overnight review, and
// the check-in cadence. Local hours shown, saved as UTC. Lives in Settings so the
// chat screen stays just the conversation.
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
  const cadence = CHECKIN.enabled ? String(CHECKIN.interval_ms) : "off";
  const row = (label, note, control) => `
    <div class="row" style="align-items:center; gap:10px; margin-bottom:12px;">
      <div class="grow"><div class="title" style="font-weight:500;">${label}</div><div class="sub">${note}</div></div>
      ${control}
    </div>`;
  return `
    <h3>When Endora reaches out</h3>
    <div class="card">
      ${row("Daily brief", "weather · safety · news, once a day", hourSelect("brief-time", "briefsched", BRIEF_SCHED.hour_utc, BRIEF_SCHED.enabled))}
      ${row("Nightly review", "reviews the day and reflects, overnight", hourSelect("night-time", "nightsched", NIGHT_SCHED.hour_utc, NIGHT_SCHED.enabled))}
      ${row("Check-ins", "starts a conversation now and then", `<select id="checkin-cadence" data-change="checkin" style="width:auto;"><option value="off" ${cadence === "off" ? "selected" : ""}>Off</option><option value="120000" ${cadence === "120000" ? "selected" : ""}>Every 2 min</option><option value="3600000" ${cadence === "3600000" ? "selected" : ""}>Hourly</option><option value="86400000" ${cadence === "86400000" ? "selected" : ""}>Daily</option></select>`)}
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
  return `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: "Settings" }])}
    <h2>Settings</h2>
    <div class="card" style="display:flex;flex-direction:column;gap:14px;">
      ${row(SPEAK, "toggle:speak", "Read replies aloud", TTS ? "" : "not supported in this browser")}
      ${row(PTT_HAPTIC, "toggle:haptic", "Vibrate on push-to-talk", "haptic cue on Android; iOS ignores it")}
      ${row(SHOW_ACTIVITY, "toggle:activity", "Show Endora's actions", "a note of what it did each turn")}
    </div>
    ${proactivitySection()}
    <h3>Manage</h3>
    <div class="card nav-list">
      <button class="ghost" data-act="go:understanding">${icon("sparkle")}<span>What Endora understands about you</span>${icon("chevron", 15)}</button>
      <button class="ghost" data-act="go:learning">${icon("target")}<span>What Endora is learning</span>${icon("chevron", 15)}</button>
      <button class="ghost" data-act="go:prefs">${icon("prefs")}<span>Your preferences</span>${icon("chevron", 15)}</button>
      <button class="ghost" data-act="go:skills">${icon("skills")}<span>Skills</span>${icon("chevron", 15)}</button>
      <button class="ghost" data-act="export">${icon("export")}<span>Export my data</span>${icon("chevron", 15)}</button>
    </div>
    ${modelsSection()}`;
}

// What the butler has learned — visible, correctable, deletable memory.
function viewPrefs() {
  // Friendly labels — the internal kind names ("taste") mean nothing to a person.
  const KIND_LABEL = { taste: "preference", authority: "permission", context: "context" };
  const rows = (DB.preferences || []).map((p) => `
    <div class="card"><div class="row">
      <div class="grow"><div class="title">${esc(p.text)}</div>
      <div class="sub">${esc(KIND_LABEL[p.kind] || p.kind)}</div></div>
      <button class="ghost danger" data-act="delete:pref:${p.id}" title="forget this">${icon("purge",15)}</button>
    </div></div>`);
  return `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: "Preferences" }])}
    <h2>What the butler knows about you</h2>
    ${listOr(rows, "Nothing yet. The butler will propose things to remember as you talk — or add one below.")}
    <div class="form">
      <input id="new-pref" placeholder="e.g. I prefer mornings…" />
      <button class="primary" data-act="create:pref">Remember</button>
    </div>`;
}

// The butler's proposal inbox: everything it has suggested from your chats,
// waiting to be applied to your profile or dismissed — durable, not lost when the
// conversation moves on. Applying runs the deterministic create (you authorize).
function viewSuggestions() {
  const cards = (SUGGESTIONS || []).map((s) => `
    <div class="card"><div class="row">
      <div class="grow"><div class="title">${esc(s.label)}</div>
      <div class="sub">the butler proposed this — you decide</div></div>
      <button class="primary" data-act="apply:suggestion:${s.id}">Apply</button>
      <button class="ghost" data-act="dismiss:suggestion:${s.id}">Dismiss</button>
    </div></div>`);
  return `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: "Inbox" }])}
    <h2>Suggestions from your conversations</h2>
    ${listOr(cards, "Nothing waiting. As you chat, the butler's proposals collect here.")}`;
}

// The butler's skills — the modules it can reach for. Ready ones work now;
// others are declared and waiting on setup (a key, a model, a data source).
function viewSkills() {
  const card = (c) => {
    const ext = c.reaches_external;
    const enabled = c.enabled !== false;
    // The irreversible band (ADR 0024): blocked deny-by-default until the person
    // opens it, and even then confirmed on every use — never autonomous.
    const irreversible = c.reversibility === "irreversible";
    const opened = c.open_irreversible === true;
    const status = c.usable
      ? `<span class="pill concluded">On</span>`
      : (!enabled ? `<span class="pill">Off</span>` : `<span class="pill">Needs setup</span>`);
    // A settings form for skills that need configuring (a model, a key, a URL).
    const settingsForm = (c.settings && c.settings.length) ? `
      <div style="margin-top:8px;border-top:1px solid var(--line);padding-top:8px;">
        ${c.settings.map((s) => `
          <div class="form" style="margin-bottom:6px;align-items:center;">
            <label class="sub" style="min-width:130px;">${esc(s.label)}${s.set ? ` <span class="pill concluded">set</span>` : ""}</label>
            <input id="setting-${c.id}-${s.key}" type="${s.secret ? "password" : "text"}" placeholder="${s.set ? "•••••• (unchanged)" : (s.secret ? "enter a value" : "")}" autocomplete="off" />
          </div>`).join("")}
        <button class="primary" data-act="skillcfg:${c.id}">Save settings</button>
      </div>` : "";
    return `
      <div class="card">
        <div class="row">
          <div class="grow">
            <div class="title">${esc(c.name)} ${status}${ext ? ` <span class="pill">leaves device</span>` : ""}${irreversible ? (opened ? ` <span class="pill concluded">irreversible · confirmed</span>` : ` <span class="pill">irreversible · blocked</span>`) : ""}</div>
            <div class="sub">${esc(c.description)}</div>
            ${(enabled && !c.configured) ? `<div class="sub" style="margin-top:4px;">Needs: ${esc(c.needs)}</div>` : ""}
          </div>
          <button class="ghost" data-act="skill:enable:${c.id}:${enabled ? "0" : "1"}">${enabled ? "Turn off" : "Turn on"}</button>
        </div>
        ${irreversible ? `
        <div class="row" style="align-items:flex-start;gap:10px;margin-top:8px;border-top:1px solid var(--line);padding-top:8px;">
          <div class="grow">
            <div class="title" style="font-weight:500;">Irreversible actions</div>
            <div class="sub">${opened
              ? "Allowed — but Endora asks before every use and never does it on its own."
              : "This skill can spend, send, or delete — blocked until you allow it. Even then it always asks first."}</div>
          </div>
          <button class="${opened ? "primary" : "ghost"}" data-act="skill:open:${c.id}:${opened ? "0" : "1"}">${opened ? "Block again" : "Allow (with confirmation)"}</button>
        </div>` : ""}
        ${settingsForm}
      </div>`;
  };
  const env = AUTONOMY || {};
  const toggle = (on, act, label, sub) => `
    <div class="row" style="align-items:flex-start;gap:10px;margin-top:8px;">
      <div class="grow"><div class="title" style="font-weight:500;">${label}</div><div class="sub">${sub}</div></div>
      <button class="${on ? "primary" : "ghost"}" data-act="${act}:${on ? "0" : "1"}">${on ? "On" : "Off"}</button>
    </div>`;
  const envelope = `
    <div class="card">
      <div class="title">How independently should Endora act?</div>
      <div class="sub" style="margin:4px 0 2px;">Endora acts on its own inside the boundary you set here, and asks you at its edges. You can widen or narrow it any time.</div>
      ${toggle(env.auto_external !== false, "autonomy:external", "Use read-only skills on its own", "Check weather, news, safety alerts and web pages without asking. (Recommended)")}
      ${toggle(env.auto_consequential === true, "autonomy:consequential", "Take consequential actions on its own", "Let it carry out actions that normally need your confirmation — spending, sending, or things that can't be undone. Off is safer; turn on only if you're sure.")}
    </div>`;
  return `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: "Skills" }])}
    <h2>What Endora can do</h2>

    ${envelope}
    <h3 style="margin-top:18px;">Skills</h3>
    ${listOr((CAPS || []).map(card), "No skills registered.")}`;
}

// The home surface: what Endora currently understands about you. Not a task list —
// beliefs it has formed (with the evidence and how sure it is), which you can
// affirm or correct. This is the point of the product (ADR 0020).
const BELIEF_KIND_LABEL = {
  intent: "What you're really after", value: "What you value", preference: "Preferences",
  pattern: "Patterns", motivation: "What drives you", frustration: "Frustrations",
  stressor: "Stressors", relationship: "People who matter", other: "Other",
};
const BELIEF_KIND_ORDER = ["intent","value","motivation","pattern","preference","frustration","stressor","relationship","other"];
// The learning loop, made visible: what Endora is trying and what it's concluded.
// Read-mostly on purpose — this is the butler's own work, not a to-do list you
// manage. The North Star/Goal/Target scaffolding stays internal to the butler.
function viewLearning() {
  const trying = (DB.experiments || []).filter((e) => e.status === "running" || e.status === "proposed");
  const tryingCards = trying.map((e) => `
    <div class="card"><div class="row">
      <div class="grow"><div class="title">${esc(e.hypothesis)}</div></div>
      <span class="pill ${e.status}">${e.status}</span></div></div>`);
  const learned = (DB.reflections || []).slice().reverse().slice(0, 12).map((r) => `
    <div class="card"><div class="title">${esc(r.summary)}</div>
      <div class="sub">weighed ${(r.evidence || []).length} observation(s)</div></div>`);
  const beliefs = (UNDERSTANDING || []).length;
  return `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: "Learning" }])}
    <h2>What Endora is learning</h2>
    <div class="note">It tries small things and reflects on how they went, to grow more useful over time.</div>
    <h3>What it's trying</h3>
    ${listOr(tryingCards, "Nothing running yet — Endora proposes small experiments as it gets to know you.")}
    <h3>What it's learned</h3>
    ${listOr(learned, "No reflections yet.")}
    <div class="note" style="margin-top:18px;">It's formed <a class="link" data-act="go:understanding">${beliefs} belief${beliefs === 1 ? "" : "s"} about you</a> — review or correct them any time.</div>`;
}

function viewUnderstanding() {
  const byKind = {};
  for (const b of (UNDERSTANDING || [])) (byKind[b.kind] = byKind[b.kind] || []).push(b);
  const groups = BELIEF_KIND_ORDER.filter(k => byKind[k]).map((k) => {
    const rows = byKind[k].map((b) => `
      <div class="card"><div class="row">
        <div class="grow">
          <div class="title">${esc(b.statement)} <span class="pill ${b.confidence === "high" ? "active" : b.confidence === "low" ? "pending" : ""}">${b.confidence} confidence</span></div>
          ${b.evidence ? `<div class="sub">because ${esc(b.evidence)}</div>` : ""}
        </div>
        <button class="ghost" data-act="affirm:belief:${b.id}" title="that's right">That's right</button>
        <button class="ghost" data-act="correct:belief:${b.id}" title="not quite">Not quite</button>
      </div></div>`).join("");
    return `<h3 style="margin-top:16px;">${BELIEF_KIND_LABEL[k] || k}</h3>${rows}`;
  }).join("");
  // A gentle one-line setup for basic context (location) so skills like weather
  // and the guard dog have somewhere to start. Stored as a preference, which the
  // butler already reads — no separate onboarding.
  const knowsLocation = (DB.preferences || []).some((p) => /^\s*based in\b/i.test(p.text));
  const knowsAddress = (DB.preferences || []).some((p) => /^\s*address me as\b/i.test(p.text));
  const locationSetup = knowsLocation ? "" : `
    <div class="card" style="border-color: color-mix(in srgb, var(--accent) 40%, var(--line));">
      <div class="title" style="margin-bottom:8px;">${icon("target", 15)} Where are you based?</div>
      <div class="form">
        <input id="setup-location" placeholder="a city, e.g. Boston, MA" />
        <button class="primary" data-act="setlocation">Save</button>
      </div>
    </div>`;
  const addressSetup = knowsAddress ? "" : `
    <div class="card" style="border-color: color-mix(in srgb, var(--accent) 40%, var(--line));">
      <div class="title" style="margin-bottom:8px;">${icon("sparkle", 15)} What should Endora call you?</div>
      <div class="form">
        <input id="setup-address" placeholder="a name, or sir / ma'am" />
        <button class="primary" data-act="setaddress">Save</button>
      </div>
    </div>`;
  const setup = locationSetup + addressSetup;
  return `
    <h2>What Endora understands about you</h2>

    ${setup}
    ${groups || `<div class="empty">Nothing yet. Talk with Endora and it will start to understand you — you'll see it here.</div>`}
    <div class="note" style="margin-top:24px;"><a class="link" data-act="go:goals">Goals ›</a> — optional.</div>`;
}

// Append a chat bubble to the thread and keep the newest in view.
function appendBubble(html, cls, id) {
  const thread = document.getElementById("chat-thread");
  if (!thread) return null;
  const mine = cls.split(" ").includes("me");
  const wrap = document.createElement("div");
  wrap.className = "row";
  wrap.style.cssText = `justify-content:${mine ? "flex-end" : "flex-start"}; margin:6px 0;`;
  if (id) wrap.id = id;
  wrap.innerHTML = `<div class="bubble ${cls}">${html}</div>`;
  thread.appendChild(wrap);
  scrollBubbleIntoView(wrap);
  return wrap;
}

// Scroll a chat row fully into view ABOVE the sticky composer (which otherwise
// overlaps the newest message). Reserves the composer's real height.
function scrollBubbleIntoView(el) {
  if (!el) return;
  const composer = document.querySelector(".composer");
  const pad = (composer ? composer.offsetHeight : 110) + 28;
  el.style.scrollMarginBottom = pad + "px";
  el.scrollIntoView({ block: "end", behavior: "smooth" });
}

// Render the live action trail into its panel. Collapsible via native <details>:
// open while working, collapsed to a one-line summary once the turn finishes.
const STEP_ICON = { running: '<span class="sspin"></span>', done: '<span class="sdone">✓</span>',
  failed: '<span class="sfail">✕</span>', blocked: '<span class="sblock">⊘</span>' };
function renderSteps(wrap, steps, running) {
  if (!wrap) return;
  if (!steps.length) { wrap.innerHTML = ""; wrap.style.display = "none"; return; }
  wrap.style.display = "";
  const rows = steps.map((s) => {
    const ic = STEP_ICON[s.status] || "";
    // A step with output is expandable — click to reveal what the skill returned.
    if (s.output) {
      return `<details class="step-item"><summary class="step-row">${ic}<span>${esc(s.label)}</span><span class="step-more">details</span></summary>` +
        `<pre class="step-out">${esc(s.output)}</pre></details>`;
    }
    return `<div class="step-row">${ic}<span>${esc(s.label)}</span></div>`;
  }).join("");
  const n = steps.length;
  const head = running ? "Working…" : `${n} action${n > 1 ? "s" : ""}`;
  const lead = running ? '<span class="sspin"></span>' : icon("sparkle", 13);
  wrap.innerHTML =
    `<details class="steps" ${running ? "open" : ""}><summary>${lead} ${head}</summary>` +
    `<div class="step-list">${rows}</div></details>`;
}

// The http(s) URLs a skill returned — real sources, never guessed. Trailing
// punctuation is trimmed so a URL at the end of a sentence stays clean.
function extractUrls(text) {
  if (!text) return [];
  return (text.match(/https?:\/\/[^\s"'<>)]+/g) || []).map((u) => u.replace(/[.,;:]+$/, ""));
}

// Render a "Sources" chip row (deduped) beneath the reply, from the URLs the
// turn's skills actually returned. Nothing shows when no skill returned a link —
// so a source list only ever appears when there genuinely was one.
function renderSources(afterEl, steps) {
  const seen = new Set();
  const urls = [];
  for (const s of steps) for (const u of extractUrls(s.output)) if (!seen.has(u)) { seen.add(u); urls.push(u); }
  if (!urls.length || !afterEl) return;
  const host = (u) => { try { return new URL(u).hostname.replace(/^www\./, ""); } catch (_) { return u; } };
  const links = urls.slice(0, 8)
    .map((u) => `<a class="src-link" href="${esc(u)}" target="_blank" rel="noopener noreferrer" title="${esc(u)}">${esc(host(u))}</a>`)
    .join("");
  const box = document.createElement("div");
  box.className = "row";
  box.style.cssText = "justify-content:flex-start; margin:2px 0 10px;";
  box.innerHTML = `<div class="sources"><span class="src-label">Sources</span>${links}</div>`;
  afterEl.insertAdjacentElement("afterend", box);
}

// HTML-string versions for rendering a PAST message's persisted actions in the
// chat history (collapsed; click to expand). Same look as the live panel.
function stepsHtml(steps) {
  if (!steps || !steps.length) return "";
  const rows = steps.map((s) => {
    const ic = STEP_ICON[s.status] || "";
    if (s.output) {
      return `<details class="step-item"><summary class="step-row">${ic}<span>${esc(s.label)}</span><span class="step-more">details</span></summary><pre class="step-out">${esc(s.output)}</pre></details>`;
    }
    return `<div class="step-row">${ic}<span>${esc(s.label)}</span></div>`;
  }).join("");
  const n = steps.length;
  return `<div class="row" style="justify-content:flex-start;margin:2px 0;"><details class="steps"><summary>${icon("sparkle", 13)} ${n} action${n > 1 ? "s" : ""}</summary><div class="step-list">${rows}</div></details></div>`;
}
function sourcesHtml(urls) {
  if (!urls || !urls.length) return "";
  const host = (u) => { try { return new URL(u).hostname.replace(/^www\./, ""); } catch (_) { return u; } };
  const links = urls.slice(0, 8).map((u) => `<a class="src-link" href="${esc(u)}" target="_blank" rel="noopener noreferrer" title="${esc(u)}">${esc(host(u))}</a>`).join("");
  return `<div class="row" style="justify-content:flex-start;margin:2px 0 10px;"><div class="sources"><span class="src-label">Sources</span>${links}</div></div>`;
}

// Send a message to the butler and stream the reply token-by-token (SSE): the
// person's message shows at once, then the butler's bubble grows live as the
// model produces prose, and proposals arrive with the final "done" event. On the
// server the message is persisted first and the reply when complete, so a reload
// always reflects the true state (that's why we just reload() at the end).
// Toggle the composer's primary button between Send and Stop to match state.
function updateComposerButton() {
  const btn = document.getElementById("send-btn");
  if (!btn) return;
  btn.dataset.act = CHAT_STREAMING ? "chat:stop" : "chat:send";
  btn.innerHTML = CHAT_STREAMING
    ? `${icon("stop")}<span>Stop</span>`
    : `${icon("send")}<span>Send</span>`;
}

// Stop the in-flight turn and drop anything still queued.
function stopChat() {
  CHAT_QUEUE = [];
  if (CHAT_ABORT) CHAT_ABORT.abort();
}

// Enqueue a message. Turns are SERIALIZED: a single local GPU (especially the
// router+synth mixture) thrashes and turns cross-talk if two run at once, so we
// never fire a second /v1/chat/stream while one is in flight — we show the
// message, queue it, and drain in order.
function sendChat() {
  const input = document.getElementById("chat-input");
  const msg = input ? input.value.trim() : "";
  if (!msg) return;
  const thread = document.getElementById("chat-thread");
  if (thread && thread.querySelector(".empty")) thread.innerHTML = "";
  if (input) { input.value = ""; growInput(input); }
  appendBubble(esc(msg), "me"); // show it immediately, even if it waits its turn
  CHAT_QUEUE.push(msg);
  drainChat();
}

// Process the queue one turn at a time.
async function drainChat() {
  if (CHAT_STREAMING || !CHAT_QUEUE.length) return;
  const msg = CHAT_QUEUE.shift();
  // A live action trail sits just above the reply bubble; it fills in as the
  // butler routes to skills, and collapses to a summary when the turn ends.
  STEP_LIST = [];
  const thread0 = document.getElementById("chat-thread");
  const stepsWrap = document.createElement("div");
  stepsWrap.className = "row";
  stepsWrap.style.cssText = "justify-content:flex-start; margin:6px 0; display:none;";
  if (thread0) thread0.appendChild(stepsWrap);
  // A butler bubble that starts as a "thinking" indicator and grows with tokens.
  const live = appendBubble('<span class="dots"><i></i><i></i><i></i></span>', "butler", "chat-live");
  const body = live && live.querySelector(".bubble");
  let acc = "";
  CHAT_STREAMING = true;
  CHAT_ABORT = new AbortController();
  updateComposerButton();
  try {
    const res = await fetch("/v1/chat/stream", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ message: msg }),
      signal: CHAT_ABORT.signal,
    });
    if (!res.ok || !res.body) throw new Error("stream unavailable");
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buf = "";
    let finished = false;
    while (!finished) {
      const { value, done } = await reader.read();
      if (done) break;
      buf += decoder.decode(value, { stream: true });
      // SSE frames are separated by a blank line; each carries a `data:` JSON.
      let sep;
      while ((sep = buf.indexOf("\n\n")) >= 0) {
        const frame = buf.slice(0, sep);
        buf = buf.slice(sep + 2);
        const dataLine = frame.split("\n").find((l) => l.startsWith("data:"));
        if (!dataLine) continue;
        let ev;
        try { ev = JSON.parse(dataLine.slice(5).trim()); } catch (_) { continue; }
        if (ev.type === "token") {
          acc += ev.text;
          if (body) body.textContent = acc;
          if (live) scrollBubbleIntoView(live);
        } else if (ev.type === "step") {
          if (ev.status === "running") {
            STEP_LIST.push({ skill: ev.skill, label: ev.label, status: "running", output: null });
          } else {
            // Terminal: finalize the last still-running step, else record it fresh
            // (a "blocked" step arrives with no prior "running").
            let i = STEP_LIST.length - 1;
            while (i >= 0 && STEP_LIST[i].status !== "running") i--;
            if (i >= 0) { STEP_LIST[i].status = ev.status; STEP_LIST[i].output = ev.output || null; }
            else STEP_LIST.push({ skill: ev.skill, label: ev.label, status: ev.status, output: ev.output || null });
          }
          renderSteps(stepsWrap, STEP_LIST, true);
          if (live) scrollBubbleIntoView(live);
        } else if (ev.type === "done") {
          CHAT_PROPOSALS = ev.proposals || [];
          LAST_ACTIVITY = ev.activity || [];
          LAST_ACTIVITY_MSG = ev.reply && ev.reply.id;
          renderSteps(stepsWrap, STEP_LIST, false); // collapse to a summary
          renderSources(live, STEP_LIST); // real source links beneath the reply
          speak(ev.reply && ev.reply.text);
          finished = true;
        } else if (ev.type === "error") {
          throw new Error(ev.message || "the butler couldn't reply");
        }
      }
    }
  } catch (e) {
    if (e && e.name === "AbortError") {
      // Stopped by the person: leave a note; the server keeps whatever it saved.
      if (body) body.textContent = "(stopped)";
      renderSteps(stepsWrap, STEP_LIST, false);
    } else {
      // Don't re-send (the server may have already saved the turn) — reload to the
      // true persisted state below.
      flash("The butler's reply was interrupted — your message was saved.", "err");
    }
  } finally {
    CHAT_STREAMING = false;
    CHAT_ABORT = null;
    if (live) live.removeAttribute("id"); // free "chat-live" for the next turn
    updateComposerButton();
    if (CHAT_QUEUE.length) {
      // More queued: don't full-reload (it would wipe the not-yet-persisted queued
      // bubbles); settle this reply in place and process the next turn.
      drainChat();
    } else {
      try { await reload(); } catch (_) {}
      const again = document.getElementById("chat-input");
      if (again) again.focus();
    }
  }
}

function render() {
  updateInboxBadge();
  const v = NAV.v;
  app.innerHTML =
      v === "direction" ? viewDirection(NAV.id)
    : v === "target" ? viewTarget(NAV.id)
    : v === "assumption" ? viewAssumption(NAV.id)
    : v === "experiment" ? viewAssumption(NAV.id)
    : v === "reflection" ? viewReflection(NAV.id)
    : v === "audit" ? viewAudit()
    : v === "chat" ? viewChat()
    : v === "suggestions" ? viewSuggestions()
    : v === "skills" ? viewSkills()
    : v === "prefs" ? viewPrefs()
    : v === "settings" ? viewSettings()
    : v === "goals" ? viewHome()
    : v === "learning" ? viewLearning()
    : v === "understanding" ? viewUnderstanding()
    : viewUnderstanding();
  // On the chat, jump to the newest message (kept clear of the sticky composer).
  if (v === "chat") {
    const thread = document.getElementById("chat-thread");
    const last = thread && thread.lastElementChild;
    if (last) requestAnimationFrame(() => scrollBubbleIntoView(last));
  }
}

// Show the pending-suggestion count on the Inbox nav button.
function updateInboxBadge() {
  const n = (SUGGESTIONS || []).length;
  // A subtle dot on the menu button when the inbox has something.
  const btn = document.getElementById("menu-btn");
  if (btn) btn.classList.toggle("has-badge", n > 0);
  const count = document.getElementById("menu-inbox-count");
  if (count) count.textContent = n ? String(n) : "";
  const act = document.getElementById("menu-activity-state");
  if (act) act.textContent = SHOW_ACTIVITY ? "on" : "off";
}

// ---- actions (event delegation) -------------------------------------------
async function dispatch(act) {
  const [verb, noun, id, arg] = act.split(":");
  try {
    if (verb === "reload") { location.reload(); return; }
    if (verb === "go") {
      if (noun === "home") return go("home");
      if (noun === "audit") return go("audit");
      return go(noun, id);
    }
    if (verb === "toggle" && noun === "archived") {
      SHOW_ARCHIVED = !SHOW_ARCHIVED; return render();
    }
    if (verb === "status" && (noun === "direction" || noun === "target")) {
      await api("POST", `/v1/${noun}s/${id}`, { status: arg }); return reload();
    }
    if (verb === "delete" && (noun === "direction" || noun === "target")) {
      if (!confirm(`Delete this ${noun}? This cannot be undone — archive instead to keep it.`)) return;
      await api("DELETE", `/v1/${noun}s/${id}`); return reload();
    }
    if (verb === "create" && noun === "direction") {
      await api("POST", "/v1/directions", { title: val("new-direction") }); return reload();
    }
    if (verb === "create" && noun === "value") {
      await api("POST", "/v1/values", { name: val("new-value") }); return reload();
    }
    if (verb === "delete" && noun === "value") {
      if (!confirm("Delete this value? Goals serving it must be re-filed first.")) return;
      await api("DELETE", `/v1/values/${id}`); return reload();
    }
    if (verb === "file" && noun === "direction") {
      const sel = document.getElementById("val-" + id).value;
      await api("POST", `/v1/directions/${id}/value`, { value_id: sel || null }); return reload();
    }
    if (verb === "chat" && noun === "send") { await sendChat(); return; }
    if (verb === "chat" && noun === "mic") { listen(); return; }
    if (verb === "chat" && noun === "stop") { stopChat(); return; }
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
      return render();
    }
    if (verb === "toggle" && noun === "haptic") {
      PTT_HAPTIC = !PTT_HAPTIC;
      localStorage.setItem("endora.haptic", PTT_HAPTIC ? "1" : "0");
      if (PTT_HAPTIC && navigator.vibrate) navigator.vibrate(25);
      return render();
    }
    if (verb === "toggle" && noun === "menu") {
      const m = document.getElementById("menu");
      if (m) m.hidden = !m.hidden;
      return;
    }
    if (verb === "toggle" && noun === "activity") {
      SHOW_ACTIVITY = !SHOW_ACTIVITY;
      localStorage.setItem("endora.showActivity", SHOW_ACTIVITY ? "1" : "0");
      closeMenu();
      return render();
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
    if (verb === "confirm" && noun === "proposal") {
      const p = CHAT_PROPOSALS[Number(id)];
      if (!p) return;
      // Suggestions are persisted; applying runs the deterministic create
      // server-side (and resolves a North Star named in a target).
      try { await api("POST", `/v1/suggestions/${p.id}/apply`); flash("Done — added.", "ok"); }
      catch (e) { flash("Couldn't add that: " + e.message, "err"); }
      CHAT_PROPOSALS.splice(Number(id), 1);
      return reload();
    }
    if (verb === "dismiss" && noun === "proposal") {
      const p = CHAT_PROPOSALS[Number(id)];
      if (p && p.id) { try { await api("POST", `/v1/suggestions/${p.id}/dismiss`); } catch (_) {} }
      CHAT_PROPOSALS.splice(Number(id), 1);
      return reload();
    }
    // Apply / dismiss a suggestion from the inbox, by its id.
    if (verb === "apply" && noun === "suggestion") {
      try { await api("POST", `/v1/suggestions/${id}/apply`); flash("Done — added.", "ok"); }
      catch (e) { flash("Couldn't add that: " + e.message, "err"); }
      return reload();
    }
    if (verb === "dismiss" && noun === "suggestion") {
      try { await api("POST", `/v1/suggestions/${id}/dismiss`); } catch (_) {}
      return reload();
    }
    // Turn a skill on or off (ADR 0021). `id` is the capability id; `arg` is 1/0.
    if (verb === "skill" && noun === "enable") {
      const enabled = arg === "1";
      try { await api("POST", `/v1/capabilities/${id}/enable`, { enabled }); }
      catch (e) { flash("Couldn't change that skill: " + e.message, "err"); }
      return reload();
    }
    // Open or re-block a skill's irreversible actions (ADR 0024). `arg` is 1/0.
    // Opening only ever moves it from blocked to confirm-each-use — never to
    // autonomous — so we confirm the intent, not fake a bigger promise.
    if (verb === "skill" && noun === "open") {
      const open = arg === "1";
      if (open && !confirm("Allow this skill's irreversible actions?\n\nEndora will still ask you before every single use, and will never do it on its own.")) return;
      try { await api("POST", `/v1/capabilities/${id}/open`, { open }); }
      catch (e) { flash("Couldn't change that skill: " + e.message, "err"); }
      return reload();
    }
    // Save a skill's settings (ADR 0021). Only non-empty fields are sent, so a
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
    // Kick off the self-improving model layer (background; results go to Activity).
    if (verb === "modeltune") {
      try { await api("POST", "/v1/model-layer/run", {}); flash("Evaluating your local models — watch Activity for scores and the result.", "ok"); }
      catch (e) { flash("Couldn't start the tune: " + e.message, "err"); }
      return;
    }
    // Save the nightly auto-tune schedule.
    if (verb === "tunesave") {
      const enabled = !!(document.getElementById("tune-nightly") || {}).checked;
      const hour_utc = Number((document.getElementById("tune-hour") || {}).value || 4);
      try { await api("POST", "/v1/model-tune/schedule", { enabled, hour_utc }); flash(enabled ? "Nightly auto-tune on." : "Nightly auto-tune off.", "ok"); }
      catch (e) { flash("Couldn't save the schedule: " + e.message, "err"); }
      return reload();
    }
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
      const body = { url: url.trim(), model: model.trim() };
      if (key.trim()) body.api_key = key.trim();
      try { await api("POST", "/v1/deep-model", body); flash("Deep model saved.", "ok"); }
      catch (e) { flash("Couldn't save: " + e.message, "err"); }
      return reload();
    }
    // Escalate the typed question to the deep model.
    if (verb === "deepask") {
      const input = document.getElementById("chat-input");
      const q = input && input.value.trim();
      if (!q) { flash("Type a question first, then ask the bigger model.", "err"); return; }
      // Show the question right away, but DON'T clear the box yet — if the deep
      // model is unconfigured or unreachable we keep the text so it isn't lost.
      appendBubble(esc(q), "me");
      flash("Asking the deep model…", "ok");
      try {
        const r = await api("POST", "/v1/deep-ask", { question: q });
        if (r && r.answered === false) { flash(r.note || "No deep model configured.", "err"); return reload(); }
        if (input) input.value = ""; // cleared only once it actually went through
      } catch (e) { flash("Deep model: " + e.message, "err"); return reload(); }
      return reload();
    }
    // Widen/narrow the autonomy envelope (ADR 0022). `noun` is the lever; `id` is 1/0.
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
    // Set the proactive check-in cadence (noun is "off" or an interval in ms).
    if (verb === "checkin") {
      const enabled = noun !== "off";
      const interval_ms = enabled ? Number(noun) : (CHECKIN.interval_ms || 86400000);
      await api("POST", "/v1/checkin", { enabled, interval_ms });
      flash(enabled ? "The butler will check in with you." : "Check-ins off.", "ok");
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
    // Nightly self-improvement loop (ADR 0024): `noun` is "off" or a LOCAL hour.
    if (verb === "nightsched") {
      const enabled = noun !== "off";
      const tzOff = new Date().getTimezoneOffset() / 60;
      const hour_utc = enabled ? ((Number(noun) + tzOff) % 24 + 24) % 24 : (NIGHT_SCHED.hour_utc || 3);
      await api("POST", "/v1/nightly-loop/schedule", { enabled, hour_utc });
      flash(enabled ? "The butler will review the day and reflect overnight." : "Nightly review off.", "ok");
      return reload();
    }
    if (verb === "snooze" && noun === "attention") {
      // data-act = snooze:attention:<kind>:<subject>
      await api("POST", "/v1/attention/snooze", { kind: id, subject: arg }); return reload();
    }
    if (verb === "create" && noun === "pref") {
      const text = val("new-pref");
      if (!text) return;
      await api("POST", "/v1/preferences", { text }); return reload();
    }
    if (verb === "delete" && noun === "pref") {
      await api("DELETE", `/v1/preferences/${id}`); return reload();
    }
    if (verb === "create" && noun === "target") {
      await api("POST", `/v1/directions/${id}/targets`, { statement: val("new-target") }); return reload();
    }
    if (verb === "create" && noun === "assumption") {
      await api("POST", `/v1/targets/${id}/assumptions`, { statement: val("new-assumption") }); return reload();
    }
    if (verb === "propose" && noun === "experiment") {
      await api("POST", `/v1/assumptions/${id}/experiments`, { hypothesis: val("new-experiment") }); return reload();
    }
    if (verb === "start" && noun === "experiment") {
      await api("POST", `/v1/experiments/${id}/start`); return reload();
    }
    if (verb === "conclude" && noun === "experiment") {
      await api("POST", `/v1/experiments/${id}/conclude`); return reload();
    }
    if (verb === "review" && noun === "experiment") {
      const days = parseInt(val("rev-" + id), 10);
      if (!(days >= 1)) { flash("Enter a number of days (1 or more).", "err"); return; }
      await api("POST", `/v1/experiments/${id}/review`, { in_days: days }); return reload();
    }
    if (verb === "record" && noun === "observation") {
      await api("POST", `/v1/experiments/${id}/observations`, { note: val("obs-" + id) }); return reload();
    }
    if (verb === "create" && noun === "reflection") {
      const evidence = [...document.querySelectorAll(".evi-box:checked")].map((b) => b.value);
      await api("POST", `/v1/targets/${id}/reflections`, { summary: val("new-reflection"), evidence }); return reload();
    }
    if (verb === "propose" && noun === "change") {
      await api("POST", `/v1/reflections/${id}/process-changes`, { description: val("new-change") }); return reload();
    }
    if (verb === "draft" && noun === "change") {
      await api("POST", `/v1/reflections/${id}/process-changes/draft`); return reload();
    }
    if (verb === "approve" && noun === "change") {
      await api("POST", `/v1/process-changes/${id}/approve`); return reload();
    }
    if (verb === "reject" && noun === "change") {
      await api("POST", `/v1/process-changes/${id}/reject`); return reload();
    }
    if (verb === "decide" && noun === "change") {
      const actor = document.getElementById("actor-" + id).value;
      const d = await api("POST", `/v1/process-changes/${id}/decision`, { actor });
      flash("Policy: " + d.decision.replace(/_/g, " ") + (d.reason ? " — " + d.reason : ""),
            d.decision === "permit" ? "ok" : "err");
      return reload();
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

// Push-to-talk: press-and-hold the mic ([data-mic]) to dictate, release to send.
document.body.addEventListener("pointerdown", (ev) => {
  const b = ev.target.closest("[data-mic]");
  if (b) { ev.preventDefault(); startPTT(b); }
});
document.body.addEventListener("pointerup", () => stopPTT(true));
document.body.addEventListener("pointercancel", () => stopPTT(false));

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
  STT_AVAILABLE = !!(health && health.stt); // a Whisper server ⇒ real push-to-talk
  const menuBtn = document.getElementById("menu-btn");
  if (menuBtn) menuBtn.innerHTML = icon("menu");
  const item = (act, name, label, extra = "", cls = "") =>
    `<button class="${cls}" data-act="${act}">${icon(name)}<span>${label}</span>${extra}</button>`;
  const menu = document.getElementById("menu");
  if (menu) {
    // A short, focused menu: the everyday destinations. Everything else
    // (Understanding, Skills, Goals, preferences, export) lives inside Settings.
    menu.innerHTML =
      item("go:chat", "chat", "Talk to Endora") +
      item("go:suggestions", "inbox", "Inbox", `<span class="menu-count" id="menu-inbox-count"></span>`) +
      item("go:settings", "prefs", "Settings") +
      item("go:audit", "audit", "Activity & audit") +
      `<div class="divider"></div>` +
      item("purge", "purge", "Delete everything", "", "danger");
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

// ---- boot -----------------------------------------------------------------
(async function () {
  try {
    const health = await api("GET", "/health");
    setupHeader(health);
    await reload();
    subscribeToActivity();
  } catch (e) {
    app.innerHTML = `<div class="msg show err">Couldn't reach the node: ${esc(e.message)}</div>`;
  }
})();
