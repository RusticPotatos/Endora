"use strict";

let DB = null;                 // latest /v1/export snapshot — NULL until one lands

// The conversation, safely. `DB` is null before the first export and stays null when
// the node is unreachable, so anything that runs on every render — the menu badge, in
// particular — must not assume a snapshot exists. Reading `DB.messages` there threw
// "null is not an object", and that error then replaced the real one on screen: the
// person saw a JavaScript failure instead of "couldn't reach the node".
function messages() {
  return (DB && DB.messages) || [];
}
let ACTIVITY = [];             // latest /v1/activity feed (newest first)
let CHECKIN = { enabled: false, interval_ms: 0 }; // proactive check-in cadence
let CAPS = [];                 // the butler's capabilities/skills (modules)
let MCP_SERVERS = { servers: [] }; // connected MCP servers + their tools (ADR 0054)
let AUTONOMY = { auto_external: true, auto_consequential: false }; // the autonomy envelope (ADR 0051)
let BRIEF_SCHED = { enabled: false, hour_utc: 12 }; // daily-brief schedule
let NIGHT_SCHED = { enabled: false, hour_utc: 3 }; // nightly self-improvement loop (ADR 0051)
let DEEP_MODEL = { configured: false, key_set: false, url: "", model: "" }; // optional bigger AI
let MODEL_CONFIG = { configured: false, key_set: false, base_url: "", mixture: false,
  single: {}, router: {}, synth: {} }; // the butler's own models, editable at runtime (ADR 0055)
let TUNE_SCHED = { enabled: false, hour_utc: 4 }; // nightly self-improving model tune (ADR 0055)
let UNDERSTANDING = [];        // Endora's beliefs about the person (the home surface)
let OUTCOMES = [];             // what Endora DID, and what it saw afterwards (ADR 0053)
let INTENTIONS = [];           // what Endora is pursuing, and has pursued (ADR 0052)
let NOTIONS = [];              // what Endora is still thinking about (ADR 0057)
let CONNECTED = [];            // integrations already set up behind the channel (ADR 0058)
let NEEDS_TOKEN = false;       // this device cannot prove who it is to the node
let SIGNIN = null;             // whether a password and authenticator are set up
let SIGNIN_EXISTS = null;      // has this node ever been claimed? (asked without a token)
let JUST_ENROLLED = "";        // the otpauth link, shown once right after claiming
// The same secret as a scannable QR. Inline SVG straight from the node — never fetched from a
// URL, because a secret at an address is a secret somebody can request.
let JUST_ENROLLED_QR = "";
let NEW_AUTHENTICATOR = "";    // a replacement being set up, not yet locked in
let NEW_AUTHENTICATOR_QR = "";
let DEV_TOKEN = "";            // a token just minted or revealed, shown until the screen changes
let DEV_TOKEN_NOTE = "";
let MCP_NEEDS = { fields: [], docs: "" }; // what the chosen catalogue entry says it needs
let WORTH_KNOWING = { models: [], fits_gb: 12, asked: false }; // hub models that would fit
let CHAT_DAY = null;           // which day's conversation is showing; null = today
let CHAT_MSGS = [];            // just that day's messages, fetched rather than filtered
let CHAT_DAYS = [];            // [{day, messages}] — which days have anything, for the bar
let LAST_VIEW = null;          // which screen was showing, so a change can reset the scroll
let REPAIRS = [];              // tooling Endora has noticed keeps not working (ADR 0054)
let TROUBLE = [];              // things in your world that stopped answering (ADR 0056)
let LANDING = null;            // how Endora's recent actions actually landed (ADR 0053)
let CONNECT = null;            // a setup form a service is asking us to fill in (ADR 0054)
let CONFIG_WRITES = [];        // changes Endora made to your services' own settings (ADR 0054)
let LAST_ACTIVITY = [];        // what Endora did behind the scenes on the last turn
let LAST_ACTIVITY_MSG = null;  // the butler message id that activity belongs to
let STEP_LIST = [];            // the live action trail for the turn currently streaming
let SHOW_ACTIVITY = localStorage.getItem("endora.showActivity") !== "0"; // default on
let CHAT_STREAMING = false;    // true while a reply is streaming in (guards live-render)
let HAPTIC = localStorage.getItem("endora.haptic") !== "0"; // buzz on reply + mic (default on)
let CHAT_STOPPED = false;      // the person stopped the last turn, so no reply is coming
let CHAT_QUEUE = [];           // messages awaiting their turn — turns are SERIALIZED
let CHAT_INFLIGHT = null;      // the user message whose reply is streaming now (not yet persisted)
let LIVE_REPLY = "";           // the reply text accumulated so far, so a re-render can rebuild it
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
  menu: '<line x1="4" y1="7" x2="20" y2="7"/><line x1="4" y1="12" x2="20" y2="12"/><line x1="4" y1="17" x2="20" y2="17"/>',
  inbox: '<path d="M3.5 13.5L6 5h12l2.5 8.5v5.5h-17z"/><path d="M3.5 13.5H9a3 3 0 0 0 6 0h5.5"/>',
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

// The node's token, kept on this device only. The node prints it to its log once, on first
// run; it is never sent to anybody but the node itself.
function token() {
  try { return localStorage.getItem("endora-token") || ""; } catch (_) { return ""; }
}

// Every request to this node goes through here, so signing is not something a call site can
// forget.
//
// It was: `/v1/chat/stream` was fetched directly with only a content-type, so the moment the
// node started requiring a credential, **every message failed to send**. Nothing caught it —
// the console harness renders screens and makes no requests, and the Rust tests exercise the
// router rather than the browser. It took a screenshot.
//
// Signing a request that does not need one costs nothing, so sign-in goes through it too
// rather than being a second path somebody has to remember exists.
function signed(headers) {
  const t = token();
  return t ? { ...headers, authorization: "Bearer " + t } : { ...headers };
}

async function api(method, path, body) {
  const headers = signed(body !== undefined ? { "content-type": "application/json" } : {});
  const res = await fetch(path, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  let data = null;
  try { data = await res.json(); } catch (_) {}
  // A 401 means this browser has no usable token. Say so once, plainly, rather than letting
  // every screen fail with its own error — the node is fine, this device just cannot prove
  // who it is.
  if (res.status === 401) { NEEDS_TOKEN = true; render(); throw new Error("unauthorized"); }
  if (!res.ok) throw new Error((data && data.error) || ("HTTP " + res.status));
  NEEDS_TOKEN = false;
  return data;
}

// The screen that shows before anything else can load.
//
// **Which one depends on whether this node has ever been set up**, and getting that wrong was
// the bug: a brand-new Endora asked people to sign in to an account that did not exist, then
// showed a bare field labelled "the node's token" with no hint of where to find one.
//
// The token step cannot go away. Something has to prove that whoever opened this page owns
// the machine, or the first stranger to reach a new node claims somebody's butler — and
// reading the log needs a shell on the box, which is exactly that proof. What it can do is
// say so, and say where to look.
function viewNeedsToken() {
  // Just claimed: the secret is handed over once, in the reply to claiming, and never again.
  if (JUST_ENROLLED) {
    return `
      <h2>Scan this with your authenticator</h2>
      <div class="card">
        <div class="note" style="margin-bottom:10px;">Google Authenticator, 1Password, whichever you use — point it at this. <b>It is shown once.</b></div>
        <div style="background:#fff;padding:12px;border-radius:8px;display:inline-block;line-height:0;">${JUST_ENROLLED_QR}</div>
        <details style="margin-top:10px;">
          <summary class="sub">Can't scan it?</summary>
          <div class="sub" style="word-break:break-all;user-select:all;margin-top:6px;font-family:ui-monospace,monospace;">${esc(JUST_ENROLLED)}</div>
        </details>
        <div style="margin-top:12px;"><button class="primary" data-act="enrolled:done">I've added it</button></div>
      </div>`;
  }
  if (SIGNIN_EXISTS === false) {
    return `
      <h2>Set up Endora</h2>
      <div class="card">
        <div class="note" style="margin-bottom:10px;">Nobody has claimed this node yet. Choose a password and it's yours.</div>
        <div class="form" style="flex-direction:column;align-items:stretch;gap:8px;">
          <input id="setup-password" type="password" autocomplete="new-password" placeholder="a password (12 characters or more)" />
          <button class="primary" data-act="signin:setup">Claim this node</button>
        </div>
        <div class="sub" style="margin-top:10px;">Next you'll add Endora to your authenticator, so signing in needs both.</div>
      </div>
      <details style="margin-top:12px;">
        <summary class="sub">This didn't work?</summary>
        <div class="card" style="margin-top:8px;">
          <div class="note" style="margin-bottom:10px;">Claiming is only open for a quarter of an hour after Endora starts, so a node left running unclaimed can't be taken by whoever finds it. If that window has passed, restart it — or use the token from its log, which always works.</div>
          <div class="sub" style="user-select:all;margin-bottom:10px;font-family:ui-monospace,monospace;">docker logs endora-node 2>&amp;1 | grep -A3 "sign-in is not set up"</div>
          <div class="form">
            <input id="token-input" type="password" autocomplete="off" placeholder="paste the token" />
            <button class="ghost" data-act="token:save">Use it</button>
          </div>
        </div>
      </details>`;
  }
  return `
    <h2>Sign in</h2>
    <div class="card">
      <div class="form" style="flex-direction:column;align-items:stretch;gap:8px;">
        <input id="signin-password" type="password" autocomplete="current-password" placeholder="password" />
        <input id="signin-code" type="text" inputmode="numeric" autocomplete="one-time-code" placeholder="6-digit code from your authenticator" />
        <button class="primary" data-act="signin">Sign in</button>
      </div>
    </div>
    <details style="margin-top:12px;">
      <summary class="sub">Lost your phone, or setting this up for the first time?</summary>
      <div class="card" style="margin-top:8px;">
        <div class="note" style="margin-bottom:10px;">Endora printed a token to its log the first time it started. It always works, and it is how you set a password up. Kept on this device only.</div>
        <div class="form">
          <input id="token-input" type="password" autocomplete="off" placeholder="paste the node's token" />
          <button class="ghost" data-act="token:save">Use it</button>
        </div>
      </div>
    </details>`;
}

// Managing your own credentials, in Settings.
//
// Three things that used to need a shell on the NAS: change the password, move to a new
// authenticator, and get a token for scripts. Each is behind being signed in, and the two that
// change something ask for the current password as well — a session lives in a browser and can
// be taken, and the password is the thing only the person knows.
// Sign-in has its own screen, because it is not understanding.
//
// It sat on "What Endora understands about you" for one bad reason: the location-setup card
// was already there, and this was written next to it. ADR 0052 cut that screen from six
// sections to one precisely so it would hold beliefs and nothing else — *understanding is a
// model of the person; the rest is a receipt* — and three credential cards is the same drift
// arriving again.
function viewSignin() {
  return `
    ${settingsCrumbs("signin")}
    <h2>Your sign-in</h2>
    ${viewSetUpSignIn()}
    ${viewYourCredentials()}`;
}

function viewYourCredentials() {
  if (SIGNIN_EXISTS !== true) return "";
  const rotating = NEW_AUTHENTICATOR
    ? `<div class="card" style="margin-top:8px;border-color: color-mix(in srgb, var(--accent) 40%, var(--line));">
         <div class="sub" style="margin-bottom:8px;">Scan this, then type a code <b>from the new one</b> to lock it in. Your current authenticator keeps working until you do — nothing has changed yet.</div>
         <div style="background:#fff;padding:12px;border-radius:8px;display:inline-block;line-height:0;margin-bottom:8px;">${NEW_AUTHENTICATOR_QR}</div>
         <details style="margin-bottom:8px;"><summary class="sub">Can't scan it?</summary><div class="sub" style="word-break:break-all;margin-top:6px;user-select:all;">${esc(NEW_AUTHENTICATOR)}</div></details>
         <div class="form">
           <input id="rotate-code" type="text" inputmode="numeric" autocomplete="one-time-code" placeholder="code from the new authenticator" />
           <button class="primary" data-act="auth:confirm">Lock it in</button>
         </div>
       </div>`
    : `<div class="form" style="margin-top:8px;">
         <input id="rotate-password" type="password" autocomplete="current-password" placeholder="your current password" />
         <button class="ghost" data-act="auth:begin">Move to a new authenticator</button>
       </div>`;
  const dev = DEV_TOKEN
    ? `<div class="sub" style="word-break:break-all;user-select:all;margin-top:8px;font-family:ui-monospace,monospace;">${esc(DEV_TOKEN)}</div>
       <div class="sub" style="margin-top:4px;">${esc(DEV_TOKEN_NOTE)}</div>`
    : "";
  return `
    <h3 style="margin-top:22px;">Your sign-in</h3>
    <div class="card">
      <div class="title" style="font-weight:500;margin-bottom:6px;">Change your password</div>
      <div class="form" style="flex-direction:column;align-items:stretch;gap:8px;">
        <input id="pw-current" type="password" autocomplete="current-password" placeholder="current password" />
        <input id="pw-new" type="password" autocomplete="new-password" placeholder="new password (12 characters or more)" />
        <button class="ghost" data-act="password:change">Change it</button>
      </div>
    </div>
    <div class="card" style="margin-top:8px;">
      <div class="title" style="font-weight:500;margin-bottom:6px;">Your authenticator</div>
      <div class="sub">Moving to a new phone, or starting over. The new one only takes effect once you've proved you scanned it.</div>
      ${rotating}
    </div>
    <div class="card" style="margin-top:8px;">
      <div class="title" style="font-weight:500;margin-bottom:6px;">Tokens for scripts</div>
      <div class="sub" style="margin-bottom:8px;">A checking token reads only and can't touch anything — that's the one for <code>make smoke</code> and anything else automated. The recovery token is what gets you in if you lose your phone; keep it somewhere safe rather than in a file.</div>
      <div class="row" style="gap:8px;">
        <button class="ghost" data-act="token:checks">Mint a checking token</button>
        <button class="ghost" data-act="token:recovery">Show the recovery token</button>
      </div>
      ${dev}
    </div>`;
}

// Setting up sign-in, shown once the person is in with the node's token but has no password.
//
// The secret is only ever offered before a password exists — after that it is never sent
// again, so an authenticator not enrolled by then needs a fresh secret rather than a peek at
// the old one.
function viewSetUpSignIn() {
  if (!SIGNIN || SIGNIN.password_set) return "";
  return `
    <div class="card" style="border-color: color-mix(in srgb, var(--accent) 40%, var(--line));">
      <div class="title" style="margin-bottom:8px;">${icon("sparkle", 15)} Set up signing in</div>
      <div class="sub" style="margin-bottom:8px;">So you don't have to paste the long token on every device. Add this to Google Authenticator, then choose a password.</div>
      <div style="background:#fff;padding:12px;border-radius:8px;display:inline-block;line-height:0;margin-bottom:8px;">${SIGNIN.qr || ""}</div>
      <details style="margin-bottom:8px;"><summary class="sub">Can't scan it?</summary><div class="sub" style="word-break:break-all;margin-top:6px;user-select:all;">${esc(SIGNIN.otpauth || "")}</div></details>
      <div class="form" style="flex-direction:column;align-items:stretch;gap:8px;">
        <input id="setup-password" type="password" autocomplete="new-password" placeholder="a password (12 characters or more)" />
        <button class="primary" data-act="signin:setup">Save</button>
      </div>
    </div>`;
}

async function reload() {
  // First, and without a credential: has this node ever been claimed? Everything below is
  // about to 401 on a fresh install, and `render` needs this answer *before* that happens or
  // it shows a sign-in form for an account nobody has created.
  try {
    const state = await api("GET", "/v1/session");
    SIGNIN_EXISTS = !!(state && state.set_up);
  } catch (_) { /* an older node has no such route; the sign-in screen is the safe fallback */ }
  // The export snapshot, the derived activity feed, and the everyday settings.
  const [db, activity, checkin, caps, autonomy] = await Promise.all([
    api("GET", "/v1/export"),
    api("GET", "/v1/activity?limit=30"),
    api("GET", "/v1/checkin"),
    api("GET", "/v1/capabilities"),
    api("GET", "/v1/autonomy"),
  ]);
  try { BRIEF_SCHED = await api("GET", "/v1/brief/schedule"); } catch (_) {}
  try { NIGHT_SCHED = await api("GET", "/v1/nightly-loop/schedule"); } catch (_) {}
  try { DEEP_MODEL = await api("GET", "/v1/deep-model"); } catch (_) {}
  try { MODEL_CONFIG = await api("GET", "/v1/model-config"); } catch (_) {}
  try { TUNE_SCHED = await api("GET", "/v1/model-tune/schedule"); } catch (_) {}
  try { MCP_SERVERS = await api("GET", "/v1/mcp/servers"); } catch (_) {}
  DB = db;
  // Attach each butler reply's persisted action trail (steps + sources) from the
  // chat endpoint, so past answers keep their expandable actions after a reload.
  // Just the day being shown, with its action trails already attached — rather than
  // every message there has ever been so the browser can throw most of them away.
  await loadChatDay(CHAT_DAY);
  ACTIVITY = activity;
  CHECKIN = checkin;
  CAPS = caps;
  AUTONOMY = autonomy;
  try { UNDERSTANDING = await api("GET", "/v1/understanding"); } catch (_) { UNDERSTANDING = []; }
  try { OUTCOMES = await api("GET", "/v1/outcomes"); } catch (_) { OUTCOMES = []; }
  try { INTENTIONS = await api("GET", "/v1/intentions"); } catch (_) { INTENTIONS = []; }
  try { NOTIONS = await api("GET", "/v1/notions"); } catch (_) { NOTIONS = []; }
  try { CONNECTED = await api("GET", "/v1/connect/connected"); } catch (_) { CONNECTED = []; }
  try { SIGNIN = await api("GET", "/v1/session/setup"); } catch (_) { SIGNIN = null; }
  try { REPAIRS = await api("GET", "/v1/repairs"); } catch (_) { REPAIRS = []; }
  try { TROUBLE = await api("GET", "/v1/standing-trouble"); } catch (_) { TROUBLE = []; }
  try { LANDING = await api("GET", "/v1/reliability"); } catch (_) { LANDING = null; }
  try { CONFIG_WRITES = await api("GET", "/v1/config-writes"); } catch (_) { CONFIG_WRITES = []; }
  render();
}

// Subscribe to the node's change stream; every "changed" event refreshes the
// snapshot and feed live. Reconnection is handled by the browser's EventSource.
function subscribeToActivity() {
  try {
    const es = new EventSource("/v1/activity/stream?token=" + encodeURIComponent(token()));
    // A background "changed" event refreshes the snapshot — but a full re-render
    // must never yank unsaved work out from under the person:
    //  - not while a reply is streaming (it would wipe the live bubble),
    //  - not while they're on Settings (unsaved form input — an endpoint, a typed
    //    API key, a picked preset — would be reset to stored values),
    //  - not while they're typing in any field.
    // Their own saves call reload() explicitly, so the view still updates on action.
    es.addEventListener("changed", () => {
      if (CHAT_STREAMING) return;
      if (NAV.v === "settings") return;
      const ae = document.activeElement;
      if (ae && /^(INPUT|TEXTAREA|SELECT)$/.test(ae.tagName)) return;
      reload().catch(() => {});
    });
  } catch (_) { /* SSE unavailable: the UI still works, just not live. */ }
}

function go(v, id) { NAV = { v, id }; clearMsg(); closeMenu(); render(); }

// Observations reachable under a target (target → assumptions → experiments → obs).
const val = (id) => document.getElementById(id).value.trim();

// ---- lifecycle (North Stars & Targets) ------------------------------------
// Everything that lives under Settings, in one place.
//
// The Settings list and each screen's breadcrumb are rendered from **this** rather than
// written out twice, because written out twice is how they drifted: four screens said
// "Home › Skills" when they were two levels down, one had no breadcrumb at all, and two
// different screens both called themselves "Preferences" in the trail — the one actually
// named Preferences, and the one the menu calls "Things Endora remembers about you".
//
// `short` is what the breadcrumb says when the full label is too long to sit in a trail. Two
// renderings of one entry, side by side, so they cannot quietly disagree.
const UNDER_SETTINGS = [
  { view: "display", icon: "prefs", label: "Preferences", note: "reading replies aloud, vibration, showing actions" },
  { view: "models", icon: "sparkle", label: "Models", note: "which model answers, and the bigger one behind it" },
  { view: "proactive", icon: "target", label: "Reaching out", note: "check-ins, the daily brief, the overnight loop" },
  { view: "skills", icon: "skills", label: "Skills", note: "what Endora can do, and the servers it connects to" },
  { view: "understanding", icon: "sparkle", label: "What Endora understands about you", short: "Understanding", note: "beliefs, and what it's working on" },
  { view: "signin", icon: "prefs", label: "Your sign-in", short: "Your sign-in", note: "password, authenticator, and tokens for scripts" },
  { view: "prefs", icon: "prefs", label: "Things Endora remembers about you", short: "What it remembers" },
  { view: "audit", icon: "audit", label: "What Endora has done", short: "What it has done", note: "what it tried, what changed, every decision" },
];

// The trail for a screen under Settings. Anything not under Settings gets the short trail, so
// a top-level screen cannot accidentally claim to be nested.
function settingsCrumbs(view) {
  const page = UNDER_SETTINGS.find((p) => p.view === view);
  const here = page ? (page.short || page.label) : view;
  return crumbs([
    { label: "Home", act: "go:chat" },
    { label: "Settings", act: "go:settings" },
    { label: here },
  ]);
}

function crumbs(parts) {
  return `<div class="crumbs">` +
    parts.map((p, i) => i < parts.length - 1
      ? `<a data-act="${p.act}">${esc(p.label)}</a> › `
      : `<span>${esc(p.label)}</span>`).join("") +
    `</div>`;
}

// Takes rows as an array OR as already-joined html, because callers do both and the
// difference is invisible until it throws: `"".length` is 0, so a joined-string caller looks
// fine while it has nothing to show and breaks the moment it has something. Found exactly
// that way — the Models screen threw as soon as a model search returned a result.
function listOr(items, emptyText) {
  const html = Array.isArray(items) ? items.join("") : String(items ?? "");
  return html ? html : `<div class="empty">${esc(emptyText)}</div>`;
}

// A North Star card, reused across the value groups on the home view.
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

// Everything Endora has actually done, in one place.
//
// These were spread over two screens for no reason a person would recognise: how its actions
// landed, what it changed inside somebody else's service, and what it claimed against what it
// saw all sat under "What Endora understands" — beside beliefs about the person, which they
// have nothing to do with. Understanding is a model of you; this is a receipt.
//
// Read-only, apart from saying how an outcome landed. Nothing here is a task and nothing
// accumulates that must be cleared: the record grows because things happened.
function viewAudit() {
  const rows = (DB.audit || []).map((a) => `
    <div class="card"><div class="sub mono">${new Date(a.at_ms).toLocaleString()}</div>
      <div>${esc(a.summary)}</div></div>`);
  return `
    ${settingsCrumbs("audit")}
    <h2>What Endora has done</h2>
    <div class="note">Its own account of its work — what it tried, whether it checked, and what it changed. Nothing here needs anything from you.</div>
    ${viewHowItLands()}
    ${viewOutcomes()}
    ${viewConfigWrites()}
    <h3 style="margin-top:22px;">Everything, newest first</h3>
    <div class="note">What it did, in its own words, and every decision policy made about it.</div>
    ${activityFeed()}
    ${listOr(rows, "No decisions recorded yet.")}`;
}

// ---- voice (browser Web Speech API; STT may use a cloud service) ---
let SPEAK = localStorage.getItem("endora.speak") === "1";      // read replies aloud (default OFF)
let DEEP_MODE = localStorage.getItem("endora.deepmode") === "1"; // route sends to the deep model (default OFF)
let CONVO_REC = null;          // the active voice-activity recording, if listening
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
  // In a hands-free conversation, resume listening once the reply is spoken — or
  // right away if there's nothing to speak, so the loop never stalls.
  if (!SPEAK || !TTS || !text) return;
  TTS.cancel();
  const u = new SpeechSynthesisUtterance(text);
  TTS.speak(u);
  // iOS sometimes pauses the queue; a nudge keeps it going.
  if (TTS.paused) TTS.resume();
}
function readAloud(text) {
  if (!TTS || !text) { flash("This browser can't read text aloud.", "err"); return; }
  TTS.cancel();
  TTS.speak(new SpeechSynthesisUtterance(text));
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
  rec.onstart = () => buzz(15); // the mic is live — worth knowing without looking
  try { rec.start(); } catch (_) { flash("Couldn't start voice input.", "err"); }
}


// The butler chat: the conversation and an input. Anything the butler does in
// the world runs through the policy boundary as it happens — there is no queue
// of proposals for you to approve afterwards.
// Which local day a moment falls in, as YYYY-MM-DD.
//
// Computed on the CLIENT because the server has no idea what timezone you are in — it
// stores a moment, and only the browser knows which day that was where you are.
function dayOf(at) {
  const d = new Date(at);
  const local = new Date(d.getTime() - d.getTimezoneOffset() * 60000);
  return local.toISOString().slice(0, 10);
}

// The days that actually have conversation, oldest first.
//
// Derived, never stored: a message already carries the moment it happened, so a "day" is
// a filter and not a thing to archive. No job to run at midnight, nothing to migrate,
// nothing to go wrong while you sleep — and no day can be lost, because none was ever
// moved.
function chatDays() {
  return (CHAT_DAYS || []).map((d) => d.day);
}

// The moment a local day starts, and the one after it — the window the server is asked
// for. The browser knows where its own midnight falls; the server never has to.
function dayWindow(day) {
  const from = new Date(`${day}T00:00:00`).getTime();
  return { from, to: from + 86400000 };
}

// Load one day's conversation, and which days exist. Everything the console shows comes
// from these two, so a five-year-old install costs exactly as much as a fresh one.
async function loadChatDay(day) {
  const showing = day || dayOf(Date.now());
  const { from, to } = dayWindow(showing);
  try {
    CHAT_MSGS = await api("GET", `/v1/chat?from=${from}&to=${to}`);
  } catch (_) { CHAT_MSGS = []; }
  try {
    CHAT_DAYS = await api("GET", `/v1/chat/days?offset_minutes=${-new Date().getTimezoneOffset()}`);
  } catch (_) { CHAT_DAYS = []; }
}

// A day in words, for the header.
function dayInWords(day) {
  if (day === dayOf(Date.now())) return "Today";
  const yesterday = dayOf(Date.now() - 86400000);
  if (day === yesterday) return "Yesterday";
  return new Date(`${day}T12:00:00`).toLocaleDateString(undefined, {
    weekday: "long", month: "short", day: "numeric",
  });
}

function viewChat() {
  const days = chatDays();
  const today = dayOf(Date.now());
  // Default to today even before it has anything in it, so a new day starts clean.
  const showing = CHAT_DAY || today;
  const list = CHAT_MSGS || [];
  const msgs = list.map((m) => {
    const mine = m.role === "user";
    const bubble = `<div class="row" style="justify-content:${mine ? "flex-end" : "flex-start"}; margin:6px 0;">
      <div class="bubble ${mine ? "me" : "butler"}">${esc(m.text)}</div></div>`;
    // A butler reply carries its persisted action trail + sources (if any), so
    // you can expand a PAST answer to see what it did and where it came from.
    if (!mine && m.actions) {
      const newest = m.id === (list[list.length - 1] || {}).id;
      return bubble + activityHtml(m.actions.activity)
        + actionsTakenHtml(m.actions.actions_taken, newest)
        + stepsHtml(m.actions.steps) + sourcesHtml(m.actions.sources);
    }
    return bubble;
  }).join("");
  // Derived from persisted state, so it survives a reload: if the newest message is
  // yours, the butler still owes a reply — show the thinking indicator.
  //
  // Unless the person STOPPED it. Then no reply is coming, and the dots would sit
  // there forever waiting for something that was cancelled. Say what happened.
  // Only today can be awaiting a reply; an older day is finished by definition.
  const awaiting = showing === today && list.length > 0 && list[list.length - 1].role === "user";
  const pending = awaiting
    ? (CHAT_STOPPED
        ? `<div class="row" style="justify-content:flex-start; margin:6px 0;">
             <div class="bubble butler"><span class="sub">You stopped this one. Send again if you'd like an answer.</span></div></div>`
        : `<div class="row" style="justify-content:flex-start; margin:6px 0;" id="chat-pending">
             <div class="bubble butler thinking"><span class="dots"><i></i><i></i><i></i></span></div></div>`)
    : "";
  // While a turn is streaming, DB.messages doesn't include it yet. Rebuild the
  // in-flight exchange from live state — the just-sent message(s) and the reply
  // growing in (id="chat-live" so the token loop keeps writing into it) — so a
  // re-render mid-stream (e.g. toggling Speak) can't wipe the current turn.
  let liveTurn = "";
  if (CHAT_STREAMING) {
    const users = [CHAT_INFLIGHT, ...CHAT_QUEUE].filter(Boolean).map((t) =>
      `<div class="row" style="justify-content:flex-end; margin:6px 0;"><div class="bubble me">${esc(t)}</div></div>`).join("");
    const replyInner = LIVE_REPLY ? esc(LIVE_REPLY) : `<span class="dots"><i></i><i></i><i></i></span>`;
    liveTurn = users +
      `<div class="row" style="justify-content:flex-start; margin:6px 0;" id="chat-live"><div class="bubble butler">${replyInner}</div></div>`;
  }
  // The in-flight turn only. Every finished turn renders its own note from the stored
  // record above, which is what makes it survive coming back to the chat; this is for the
  // moment between the stream ending and the reply appearing in the persisted history.
  const streamingActivity = SHOW_ACTIVITY && CHAT_STREAMING && LAST_ACTIVITY.length
    ? `<div class="activity">${icon("sparkle", 13)} ${LAST_ACTIVITY.map(esc).join(" · ")}</div>`
    : "";
  const speakBtn = TTS
    ? `<button class="ghost" data-act="toggle:speak" title="read replies aloud">${icon(SPEAK ? "speakerOn" : "speakerOff")}<span>${SPEAK ? "Speaking" : "Speak"}</span></button>`
    : "";
  // Dictation puts your words in the box and leaves them there — you read them and
  // hit send. Browser-native recognition, so there is no upload and no wait for a
  // transcription server; press-and-hold-to-send and the hands-free loop were slower
  // and less predictable than the thing they replaced.
  const micBtn = STT
    ? (window.isSecureContext
        ? `<button class="ghost" data-act="chat:mic" title="dictate into the box — you still press send">${icon("mic")}<span>Dictate</span></button>`
        : `<button data-act="chat:mic" title="voice input needs HTTPS or localhost">${icon("mic")}<span>needs HTTPS</span></button>`)
    : "";
  // A way back through the days that have conversation. Only shown once there IS a
  // yesterday — on a fresh install it would be a control that does nothing.
  const older = days.filter((d) => d < showing).pop();
  const newer = days.filter((d) => d > showing).shift();
  const dayBar = days.length > 1 || showing !== today
    ? `<div class="row" style="gap:8px;align-items:center;justify-content:center;padding:4px 0;">
         ${older ? `<button class="ghost" data-act="chat:day:${older}" title="${esc(dayInWords(older))}"><span style="display:inline-block;transform:rotate(180deg);">${icon("chevron", 14)}</span></button>` : ""}
         <span class="sub">${esc(dayInWords(showing))}${showing === today ? "" : ` · ${list.length} message${list.length === 1 ? "" : "s"}`}</span>
         ${newer ? `<button class="ghost" data-act="chat:day:${newer}">${icon("chevron", 14)}</button>` : ""}
         ${showing === today ? "" : `<button class="ghost" data-act="chat:day:${today}">Today</button>`}
       </div>`
    : "";
  const emptyToday = `<div class="empty">${showing === today && days.length > 1
      ? "A new day. Yesterday's conversation is a tap back."
      : "Say anything — Endora is listening."}</div>`;
  return `
    <div class="chat">
      ${dayBar}
      <div id="chat-thread" class="chat-thread">${(msgs || (CHAT_STREAMING ? "" : emptyToday)) + (CHAT_STREAMING ? liveTurn : pending) + streamingActivity}</div>
      ${CHAT_STREAMING ? "" : viewNeedsYou()}
      <div class="composer">
        <textarea id="chat-input" rows="1" placeholder="Talk to your butler…"></textarea>
        <div class="composer-actions">
          <div class="composer-secondary">
            ${speakBtn}
            ${micBtn}
            ${DEEP_MODEL.configured ? `<button class="ghost${DEEP_MODE ? " active" : ""}" data-act="toggle:deep" title="when on, your messages go to the bigger model">${icon("sparkle", 15)}<span>${DEEP_MODE ? "Deep: on" : "Ask deep"}</span></button>` : ""}
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
  // DeepSeek is OpenAI-compatible. deepseek-chat (V3) is the general model;
  // deepseek-reasoner (R1) is the heavier reasoning one — a good "deep" pick you
  // can type into the Model field.
  deepseek: { label: "DeepSeek", base_url: "https://api.deepseek.com/v1", key: true,
    single: { model: "deepseek-chat",     temperature: 0.5 },
    router: { model: "deepseek-chat",     temperature: 0.1 },
    synth:  { model: "deepseek-chat",     temperature: 0.6 } },
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
  const picker = document.getElementById(isDeep ? "deep-model-picker" : "m-model-picker");
  if (picker) picker.innerHTML = `<div class="sub">Listing models…</div>`;
  try {
    const r = await api("POST", "/v1/models/discover", body);
    const models = r.models || [];
    // Keep the datalist (autocomplete on the model fields) …
    const list = document.getElementById(isDeep ? "deep-models" : "m-models");
    if (list) list.innerHTML = models.map((m) => `<option value="${esc(m)}"></option>`).join("");
    // … and render a visible, tappable picker (the datalist alone is invisible on
    // mobile, so a discovered list was easy to miss).
    if (picker) {
      picker.innerHTML = models.length
        ? `<div class="sub" style="margin:6px 0 4px;">Tap a model to use it:</div>`
          + `<div class="model-picker">` + models.map((m) =>
              `<button type="button" class="pill pick" data-model="${esc(m)}" onclick="pickModel('${role}', this.dataset.model)">${esc(m)}</button>`
            ).join("") + `</div>`
        : `<div class="sub">No models returned.</div>`;
    }
    flash(models.length ? `Found ${models.length} models — tap one below to pick it.` : "No models returned.", models.length ? "ok" : "err");
  } catch (e) {
    if (picker) picker.innerHTML = "";
    flash("Couldn't list models: " + e.message, "err");
  }
}

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
// Test connection checks; for deep it's the one deep model.
function pickModel(role, name) {
  if (!name) return;
  let id;
  if (role === "deep") {
    id = "deep-model";
  } else {
    const mixOn = !!(document.getElementById("m-mix") || {}).checked;
    id = mixOn ? "m-synth-model" : "m-single-model";
  }
  const el = document.getElementById(id);
  if (el) el.value = name;
  // Highlight the chosen chip.
  const picker = document.getElementById(role === "deep" ? "deep-model-picker" : "m-model-picker");
  if (picker) picker.querySelectorAll(".pill.pick").forEach((b) => {
    b.classList.toggle("active", b.dataset.model === name);
  });
  flash("Model set to " + name, "ok");
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
// common case stays clean (ADR 0055; runtime-swappable, no restart).
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

function modelsSection() {
  const mc = MODEL_CONFIG || {};
  const dm = DEEP_MODEL || {};
  const mix = !!mc.mixture;
  // When the everyday model isn't overridden, name the deployment-default brain that
  // is actually running, so the card shows what's active instead of just "default".
  const activeDefault = mc.default_mixture
    ? `${esc(mc.default_router || "")} + ${esc(mc.default_synth || "")}`
    : esc(mc.default_model || "");
  return `
    <h3>Models</h3>
    <div class="note">A local <b>everyday</b> model, and an optional <b>deep</b> one for hard questions. Any OpenAI-compatible endpoint.</div>

    <div class="card model-card">
      <div class="model-role">Everyday${mc.configured ? "" : ` · <span class="sub" style="font-weight:400;">using deployment default${activeDefault ? `: <b>${activeDefault}</b>` : ""}</span>`}</div>
      <div class="field"><label>Provider preset</label>
        <select id="m-preset" onchange="applyModelPreset(this.value)"><option value="">Choose a provider…</option>${presetOptions(presetFor(mc.base_url))}</select></div>
      <div class="field"><label>Endpoint</label>
        <input id="m-base" placeholder="${esc(mc.default_base_url || "http://host.docker.internal:11434/v1")}" value="${esc(mc.base_url || "")}" /></div>
      <div class="field"><label>API key <span style="opacity:.7;">· cloud only</span></label>
        <input id="m-key" type="password" autocomplete="off" placeholder="${mc.key_set ? "•••••• (unchanged)" : "stored securely, never shown"}" /></div>
      <div class="row" style="gap:8px;"><button class="ghost" data-act="discover:everyday" style="font-size:13px;">${icon("sparkle", 14)} Discover models</button><button class="ghost" data-act="testconn:everyday" style="font-size:13px;">${icon("check", 14)} Test connection</button></div>
      <div id="m-model-picker"></div>
      <datalist id="m-models"></datalist>
      <div id="m-single" style="display:${mix ? "none" : "flex"};flex-direction:column;gap:12px;">
        ${modelName("single", mc.single, mc.default_model || "e.g. qwen2.5:7b")}
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

    <details class="adv" style="margin-top:14px;">
      <summary>Also use a stronger model for hard questions</summary>
    <div class="card model-card">
      <div class="model-role">Deep${dm.configured ? ` · <span class="sub" style="font-weight:400;">using <b>${esc(dm.model || "")}</b></span>` : ` <span class="sub" style="font-weight:400;">· a bigger brain for hard questions</span>`}</div>
      <div class="sub" style="margin:-4px 0 2px;">Optional, opt-in per question. It leaves your device, so it passes the same egress guard.</div>
      <div class="field"><label>Provider preset</label>
        <select id="d-preset" onchange="applyDeepPreset(this.value)"><option value="">Choose a provider…</option>${presetOptions(presetFor(dm.url))}</select></div>
      <div class="field"><label>Endpoint</label>
        <input id="deep-url" placeholder="https://api.provider.com/v1" value="${esc(dm.url || "")}" /></div>
      <div class="field"><label>API key</label>
        <input id="deep-key" type="password" autocomplete="off" placeholder="${dm.key_set ? "•••••• (unchanged)" : "stored securely, never shown"}" /></div>
      <div class="row" style="gap:8px;"><button class="ghost" data-act="discover:deep" style="font-size:13px;">${icon("sparkle", 14)} Discover models</button><button class="ghost" data-act="testconn:deep" style="font-size:13px;">${icon("check", 14)} Test connection</button></div>
      <div id="deep-model-picker"></div>
      <datalist id="deep-models"></datalist>
      <div class="field"><label>Model</label>
        <input id="deep-model" list="deep-models" placeholder="e.g. gpt-4o, claude-sonnet-5" value="${esc(dm.model || "")}" /></div>
      <label class="mix-toggle">
        <input id="deep-escalate" type="checkbox" ${dm.escalate ? "checked" : ""} />
        <span>Let Endora <b>use this on its own</b> — to fall back when the local model can't answer, and to word your daily brief.</span>
      </label>
      <div class="sub" style="margin:-6px 0 2px;">Off by default. In chat the local model is always tried first, three times, and when this steps in the reply says so. Your name, place names and appointment titles are swapped for placeholders before anything leaves and put back here — but <b>the shape of your day still goes out</b>: that you have something at 6:30, that the house is empty. Nothing else travels: no beliefs, no device names.</div>
      <div class="row" style="justify-content:flex-end;"><button class="primary" data-act="deepsave">Save deep</button></div>
    </div>
    </details>

    <details class="adv" style="margin-top:14px;">
      <summary>Auto-tune · experimental</summary>
    <div class="note">Scores the models on your endpoint and adopts the best local one on its own. Takes a few minutes and uses the GPU — watch <a class="link" data-act="go:audit">what it has done</a> for the result.</div>
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
    </div>
    </details>`;
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
      ${nav("export", "export", "Export my data")}
    </div>
    <div class="card nav-list" style="margin-top:14px;">
      <button class="ghost danger" data-act="purge">${icon("purge")}
        <span class="grow" style="text-align:left;">Delete everything<span class="sub" style="display:block;font-weight:400;">every message, belief and record — this cannot be undone</span></span>
      </button>
    </div>`;
}

// Things the butler said WITHOUT being asked — check-ins, the daily brief, the
// overnight note — collected like voicemail: read them, or have them read to you.
//
// Derived, not stored: a butler message is unprompted when the message before it is
// not the person's. A reply always follows something you said; a check-in does not.
// So the inbox needs no new table and no flag anyone has to remember to set — the
// same reason repair proposals are derived (ADR 0054).
function unpromptedMessages() {
  const list = messages();
  const out = [];
  for (let i = 0; i < list.length; i++) {
    if (list[i].role !== "butler") continue;
    const before = i > 0 ? list[i - 1] : null;
    if (before && before.role === "user") continue; // a reply, not an approach
    if (isDegraded(list[i].text)) continue; // a failure, not an approach
    out.push(list[i]);
  }
  return out.reverse(); // newest first: the latest at the top, older below
}

// Whether a message is the butler reporting it could not reach its model.
//
// Those are real and belong in the conversation, but an inbox is what Endora chose to
// say to you — and "I couldn't reach my language model" is the opposite: it is what
// happened when it could not choose anything. Four of the twenty items in this inbox
// were that sentence, which is how an inbox becomes something nobody opens.
function isDegraded(text) {
  return (text || "").startsWith("Sorry — I couldn't reach my language model");
}

// How much of the inbox has been seen. Kept on the device rather than the server: it
// is a reading marker, not something Endora knows about the person, and it should not
// end up in an export of their memory.
function inboxSeenAt() {
  return Number(localStorage.getItem("endora.inboxSeen") || 0);
}

function unreadCount() {
  const seen = inboxSeenAt();
  return unpromptedMessages().filter((m) => (m.at_ms || 0) > seen).length;
}

// Which day a message belongs to, in words — "Today", "Yesterday", or the date. An
// inbox that fills up through the day reads better in days than in timestamps.
function inboxDay(at) {
  const when = new Date(at);
  const midnight = (d) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((midnight(new Date()) - midnight(when)) / 86400000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return when.toLocaleDateString(undefined, { weekday: "long", month: "short", day: "numeric" });
}

function viewInbox() {
  const msgs = unpromptedMessages();
  const seen = inboxSeenAt();
  let day = "";
  const rows = msgs.map((m) => {
    const unread = (m.at_ms || 0) > seen;
    // A day heading whenever the day changes. The list is newest first, so these fall
    // in order without sorting anything twice.
    const today = inboxDay(m.at_ms);
    const heading = today === day ? "" : `<h3 style="margin:18px 0 8px;">${esc(today)}</h3>`;
    day = today;
    const time = new Date(m.at_ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
    return `${heading}
    <div class="card"${unread ? ` style="border-color: color-mix(in srgb, var(--accent) 45%, var(--line));"` : ""}>
      <div class="row" style="align-items:flex-start;gap:10px;">
        <div class="grow">
          <div class="sub">${esc(time)}${unread ? ` · <strong>new</strong>` : ""}</div>
          <div class="title" style="font-weight:400;white-space:pre-wrap;">${esc(m.text)}</div>
        </div>
        <button class="ghost" data-act="play:msg:${m.id}" title="read this aloud">${icon("speakerOn", 15)}</button>
      </div>
    </div>`;
  });
  // Opening the inbox is what marks it read — nothing to click, and it cannot mark
  // something read that arrived after this render.
  if (msgs.length) {
    localStorage.setItem("endora.inboxSeen", String(msgs[0].at_ms || Date.now()));
  }
  return `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: "Inbox" }])}
    <h2>Inbox</h2>
    <div class="note" style="margin-bottom:10px;">What Endora sent you through the day, on its own — check-ins, your brief, and what it looked into overnight. Replies to things you asked stay in the conversation on Home.</div>
    ${listOr(rows, "Nothing yet. When Endora has something worth saying unprompted, it lands here.")}`;
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
    </div>`;
}

function viewModels() {
  return `
    ${settingsCrumbs("models")}
    ${modelsSection()}
    ${worthKnowingSection()}`;
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

function viewProactive() {
  return `
    ${settingsCrumbs("proactive")}
    ${proactivitySection()}`;
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
    ${settingsCrumbs("prefs")}
    <h2>What the butler knows about you</h2>
    ${listOr(rows, "Nothing yet. The butler will propose things to remember as you talk — or add one below.")}
    <div class="form">
      <input id="new-pref" placeholder="e.g. I prefer mornings…" />
      <button class="primary" data-act="create:pref">Remember</button>
    </div>`;
}

// The butler's skills — the modules it can reach for. Ready ones work now;
// others are declared and waiting on setup (a key, a model, a data source).
function viewSkills() {
  const card = (c) => {
    const ext = c.reaches_external;
    const enabled = c.enabled !== false;
    // The irreversible band (ADR 0051): blocked deny-by-default until the person
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
        <div class="row" style="gap:8px;">
          <button class="primary" data-act="skillcfg:${c.id}">Save settings</button>
          <button class="ghost" data-act="skilltest:${c.id}" title="check these settings actually work">${icon("check", 14)} Test</button>
        </div>
      </div>` : "";
    return `
      <div class="card">
        <div class="row">
          <div class="grow">
            <div class="title">${esc(c.name)} ${status}${ext ? ` <span class="pill">leaves device</span>` : ""}${irreversible ? ` <span class="pill">can't be undone</span>` : ""}</div>
            <div class="sub">${esc(c.description)}</div>
            ${(enabled && !c.configured) ? `<div class="sub" style="margin-top:4px;">Needs: ${esc(c.needs)}</div>` : ""}
          </div>
          <button class="ghost" data-act="skill:enable:${c.id}:${enabled ? "0" : "1"}">${enabled ? "Turn off" : "Turn on"}</button>
        </div>
        ${irreversible ? `
        <div class="row" style="align-items:flex-start;gap:10px;margin-top:8px;border-top:1px solid var(--line);padding-top:8px;">
          <div class="grow">
            <div class="title" style="font-weight:500;">Actions that can't be undone</div>
            <div class="sub">${opened
              ? "Allowed — Endora still asks before every use, never on its own."
              : "Spending, sending or deleting — blocked until you allow it, and it always asks first."}</div>
          </div>
          <button class="${opened ? "primary" : "ghost"}" data-act="skill:open:${c.id}:${opened ? "0" : "1"}">${opened ? "Block again" : "Allow (with confirmation)"}</button>
        </div>` : (enabled ? `
        <div class="row" style="gap:8px;margin-top:6px;">
          <button class="ghost" style="font-size:12px;padding:2px 8px;" data-act="skill:confirm:${c.id}:${c.confirm ? "0" : "1"}"
            title="${c.confirm ? "Endora asks before every use. Click to let it decide." : "Endora may use this on its own. Click to be asked each time."}">${c.confirm ? "Asks first" : "Uses freely"}</button>
        </div>` : "")}
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
  // MCP servers (ADR 0054): external tool sources. Each tool is deny-by-default —
  // visible but blocked until allowed, and even then confirmed every use.
  const mcpServerCard = (s) => {
    const addr = s.transport === "http" ? s.url : `${s.command} ${(s.args || []).join(" ")}`.trim();
    const health = s.tools_live > 0
      ? `<span class="pill concluded">${s.tools_live} tool${s.tools_live === 1 ? "" : "s"}</span>`
      : `<span class="pill">not connected</span>`;
    // A withdrawn tool (ADR 0054) is not offered to the butler at all — a different
    // thing from blocked, which still shows it the tool and refuses each use. Shown
    // here so it is never a mystery why a tool stopped being used, with one click back.
    const toolRow = (t) => `
      <div class="row" style="align-items:flex-start;gap:10px;margin-top:6px;border-top:1px solid var(--line);padding-top:6px;${t.enabled === false ? "opacity:.6;" : ""}">
        <div class="grow">
          <div class="title" style="font-weight:500;">${esc(t.id)}${t.enabled === false ? ` <span class="pill">turned off</span>` : ""}</div>
          <div class="sub">${esc(t.description || "")}</div>
          <div class="sub">${t.enabled === false
            ? "Turned off — the butler isn't offered this tool at all."
            : t.opened
              ? (AUTONOMY.auto_consequential
                  ? "Allowed — and because you let consequential skills act on their own, it runs without asking."
                  : "Allowed — asks before every use.")
              : "Blocked — the butler can see it but cannot use it."}</div>
        </div>
        ${t.enabled === false
          ? `<button class="ghost" data-act="skill:enable:${t.id}:1">Offer it again</button>`
          : `<button class="${t.opened ? "primary" : "ghost"}" data-act="skill:open:${t.id}:${t.opened ? "0" : "1"}">${t.opened ? "Block" : "Allow"}</button>`}
      </div>`;
    // Stacked, not side by side. Four buttons beside a name is a row on a laptop and a
    // disaster on a phone: the actions took the width they needed and left the name a column
    // about eight characters wide, so "brave-search" broke across two lines, the pills came
    // out as "8" above "tools", and the launch command rendered one word per line. Adding a
    // fifth button made it worse.
    //
    // Second time for this exact shape — the reader dropdown did it to its own label, for the
    // same reason, and the fix was the same: full width, one thing under another. A `.row` is
    // only safe when nothing in it needs room to breathe.
    return `
      <div class="card">
        <div>
          <div>
            <div class="title">${esc(s.name)} ${health} <span class="pill">${esc(s.transport)}</span>${s.auth_set ? ` <span class="pill concluded">token set</span>` : ""}${(s.env_keys || []).length ? ` <span class="pill concluded">${(s.env_keys || []).length} env</span>` : ""}</div>
            <div class="sub" style="overflow-wrap:anywhere;">${esc(addr)}</div>
            ${s.tools_live === 0 ? `<div class="sub" style="margin-top:4px;color:var(--danger,#c33);"><strong>Connected to nothing — no tools.</strong> Endora can't use this server. Most often that's a missing or wrong environment variable rather than a broken server${(s.env_keys || []).length ? ` — it has ${(s.env_keys || []).map(esc).join(", ")} set` : " — it has none set"}. ${s.transport === "stdio" ? "Some servers also need a one-time setup with a browser before they can run headless; check their docs." : "Check the endpoint is reachable."} Retrying every couple of minutes.</div>` : ""}
          </div>
          <div class="row" style="gap:6px;flex-wrap:wrap;margin-top:8px;">
            <button class="ghost" data-act="mcp:edit:${esc(s.name)}" title="Load this server into the form below to change its URL or settings">Edit</button>
            <button class="ghost" data-act="mcp:test:${esc(s.name)}" title="Call its state-reading tool for real, so a wrong key says so instead of going quiet">Test</button>
            <button class="ghost" data-act="mcp:reconnect:${esc(s.name)}" title="Retry the connection using its saved settings">Reconnect</button>
            <button class="ghost danger" data-act="mcp:remove:${esc(s.name)}">Remove</button>
          </div>
        </div>
        ${toggle(s.trust_all, `mcp:trust:${esc(s.name)}`, "Allow all its tools", "Opens every tool this server exposes, so you don't allow them one by one — and if you have widened autonomy, an opened tool can act without asking. Turning this off closes them all again; reopen the ones you want.")}
        ${readerRow(s)}
        ${(s.tools || []).length
          ? `<details class="steps" style="margin-top:6px;"><summary>${(s.tools || []).length} tool${(s.tools || []).length === 1 ? "" : "s"}</summary>
               ${(s.tools || []).length > 1 ? `
                 <div class="row" style="gap:8px;align-items:center;margin:6px 0 2px;">
                   <div class="grow sub">${(s.tools || []).filter((t) => t.opened).length} of ${(s.tools || []).length} allowed</div>
                   <button class="ghost" data-act="mcp:tools:${esc(s.name)}:1">Allow all</button>
                   <button class="ghost" data-act="mcp:tools:${esc(s.name)}:0">Block all</button>
                 </div>` : ""}
               <div class="step-list">${(s.tools || []).map(toolRow).join("")}</div></details>`
          : ""}
      </div>`;
  };
  const servers = (MCP_SERVERS && MCP_SERVERS.servers) || [];
  // Browse/search the catalog: curated entries + (best effort) the community
  // registry. Picking one prefills the form below — everything stays editable, so a
  // stale launch command can be corrected before it's registered.
  const mcpBrowse = `
    <div class="card">
      <div class="title">Find a server</div>
      <div class="sub" style="margin:4px 0 8px;">Search well-known servers and the community registry. Choosing one fills in the form below — you can edit anything before adding it.</div>
      <div class="row" style="gap:8px;">
        <input id="mcp-search" placeholder="e.g. files, github, home assistant" style="flex:1;" />
        <button class="ghost" data-act="mcp:search">${icon("sparkle", 14)} Search</button>
      </div>
      <div id="mcp-catalog-results"></div>
    </div>`;
  // Folded, because most people arrive at this screen to see what Endora can do — not to
  // hand-configure a server. Browsing the catalogue above fills this in for you, so typing
  // it out is the fallback rather than the front door.
  const mcpAddForm = `
    <details class="adv">
      <summary>Add a server by hand</summary>
    <div class="card">
      <div class="sub" style="margin:4px 0 8px;">A local command (stdio) or a networked endpoint (e.g. a Docker MCP Gateway). Its tools appear above, blocked until you allow each one.</div>
      <div class="field"><label>Name</label><input id="mcp-name" placeholder="e.g. filesystem" /></div>
      <div class="field"><label>Connection</label>
        <select id="mcp-transport" onchange="mcpTransportChange(this.value)">
          <option value="stdio">Local command (stdio)</option>
          <option value="http">HTTP endpoint</option>
        </select></div>
      <div id="mcp-stdio-fields">
        <div class="field"><label>Command</label><input id="mcp-command" placeholder="e.g. npx" /></div>
        <div class="field"><label>Arguments <span class="sub" style="font-weight:400;">· one per line</span></label>
          <textarea id="mcp-args" rows="3" placeholder="-y&#10;@modelcontextprotocol/server-filesystem&#10;/data"></textarea></div>
        <div id="mcp-needs"></div>
        <details class="steps" style="margin-top:6px;">
          <summary>Advanced · other environment variables</summary>
          <div class="field" style="margin-top:6px;">
            <label>KEY=value, one per line</label>
            <textarea id="mcp-env" rows="2" placeholder="TOKEN=… (only if this server needs one)"></textarea></div>
        </details>
      </div>
      <div id="mcp-http-fields" style="display:none;">
        <div class="field"><label>Endpoint URL</label><input id="mcp-url" placeholder="http://mcp-gateway:8080/" /></div>
        <div class="field"><label>Access token <span class="sub" style="font-weight:400;">· optional, sent as a bearer token</span></label>
          <input id="mcp-auth" type="password" autocomplete="off" placeholder="stored securely, never shown" /></div>
      </div>
      <label class="row" style="gap:8px;align-items:center;margin-top:4px;">
        <input type="checkbox" id="mcp-trust" />
        <span class="sub">Allow all its tools at once. Off by default — each tool stays blocked until you allow it, and if you have widened autonomy an allowed tool can act without asking.</span>
      </label>
      <div class="row" style="justify-content:flex-end;"><button class="primary" data-act="mcp:add">Save server</button></div>
    </div>
    </details>`;
  const mcpSection = `
    <h3>MCP servers <span class="sub" style="font-weight:400;">· connect external tools</span></h3>
    <div class="note">Tools from an MCP server are off-limits by default: the butler can see them, but each stays blocked until you allow it. Whether an allowed tool then asks first depends on your autonomy setting above.</div>
    ${servers.map(mcpServerCard).join("")}
    ${mcpBrowse}
    ${mcpAddForm}`;
  return `
    ${settingsCrumbs("skills")}
    <h2>What Endora can do</h2>

    ${envelope}
    ${viewConnect()}
    <h3>Skills</h3>
    ${listOr((CAPS || []).map(card), "No skills registered.")}
    ${mcpSection}`;
}

// The home surface: what Endora currently understands about you. Not a task list —
// beliefs it has formed (with the evidence and how sure it is), which you can
// affirm or correct. This is the point of the product (ADR 0052).
const BELIEF_KIND_LABEL = {
  intent: "What you're really after", value: "What you value", preference: "Preferences",
  pattern: "Patterns", motivation: "What drives you", frustration: "Frustrations",
  stressor: "Stressors", relationship: "People who matter", other: "Other",
};
const BELIEF_KIND_ORDER = ["intent","value","motivation","pattern","preference","frustration","stressor","relationship","other"];

function viewUnderstanding() {
  const byKind = {};
  for (const b of (UNDERSTANDING || [])) (byKind[b.kind] = byKind[b.kind] || []).push(b);
  const groups = BELIEF_KIND_ORDER.filter(k => byKind[k]).map((k) => {
    const rows = byKind[k].map((b) => `
      <div class="card"><div class="row">
        <div class="grow">
          <div class="title">${esc(b.statement)} <span class="pill ${b.confidence === "high" ? "active" : b.confidence === "low" ? "pending" : ""}">${b.settled ? "settled" : b.confidence + " confidence"}</span></div>
          ${b.evidence ? `<div class="sub">because ${esc(b.evidence)}</div>` : ""}
          ${b.contradicts ? `<div class="sub" style="margin-top:6px;color:var(--warn,#c9a227);">This sits oddly with something else I think: &ldquo;${esc(b.contradicts)}&rdquo; — one of us is wrong.</div>` : ""}
        </div>
        ${b.settled
          ? `<button class="ghost" data-act="correct:belief:${b.id}" title="not quite">Not quite</button>`
          : `<button class="ghost" data-act="affirm:belief:${b.id}" title="that's right">That's right</button>
             <button class="ghost" data-act="correct:belief:${b.id}" title="not quite">Not quite</button>`}
      </div></div>`).join("");
    return `<h3 style="margin-top:16px;">${BELIEF_KIND_LABEL[k] || k}</h3>${rows}`;
  }).join("");
  // A gentle one-line setup for basic context (location) so skills like weather
  // and the guard dog have somewhere to start. Stored as a preference, which the
  // butler already reads — no separate onboarding.
  // Location is asked in the chat now, where questions belong — see `viewNeedsYou`. Asking
  // in both places would be the same question twice, which is how a prompt becomes a chore.
  const knowsLocation = true;
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
    ${settingsCrumbs("understanding")}
    <h2>What Endora understands about you</h2>

    ${setup}
    ${groups || `<div class="empty">Nothing yet. Talk with Endora and it will start to understand you — you'll see it here.</div>`}
    ${viewIntention()}
    ${viewNotions()}`;
}

// What Endora is still turning over (ADR 0057).
//
// Deliberately the plainest thing on this screen: no buttons, no count, no badge. The person
// may look at what their butler is chewing on — holding private theories about somebody is not
// something this system does — but there is nothing here for them to answer, dismiss or keep
// tidy. A notion that comes to something becomes a belief above; one that does not lets itself
// go. Both happen without anyone doing anything.
function viewNotions() {
  const open = (NOTIONS || []).filter((n) => n.status === "open");
  if (!open.length) return "";
  const rows = open.map((n) => `
    <div class="card"><div class="row"><div class="grow">
      <div class="title" style="font-weight:500;">${esc(n.statement)}</div>
      <div class="sub">from ${esc((n.because || []).join(", "))}</div>
      ${n.settles_when ? `<div class="sub">watching for: ${esc(n.settles_when)}</div>` : ""}
    </div></div></div>`).join("");
  return `<h3 style="margin-top:16px;">Still wondering</h3>
    <div class="sub" style="margin-bottom:8px;">Not settled — nothing for you to do with these.</div>
    ${rows}`;
}

// What Endora is currently working on (ADR 0052).
//
// One thing at a time — a cursor, not a queue — and the person's only verb is to stop
// it. There is deliberately no "add" control here: Endora forms its own intentions from
// what it understands, and a console that let you file work would make this the goal
// tracker ADR 0052 deleted.
function viewIntention() {
  const current = (INTENTIONS || []).find((i) => i.active);
  if (!current) return "";
  // An intention taken up from a belief reuses its wording, so showing "because it
  // believes <the same sentence>" reads as a stutter. Only name the belief when it
  // actually adds something.
  const source = (UNDERSTANDING || []).find((b) => b.id === current.motivating_belief);
  const belief = source && source.statement.trim() !== current.statement.trim() ? source : null;
  return `
    <h3 style="margin-top:22px;">What Endora is working on</h3>
    <div class="card"><div class="row">
      <div class="grow">
        <div class="title">${esc(current.statement)} <span class="pill">night ${current.steps_taken + 1}</span></div>
        ${belief ? `<div class="sub">because it believes: ${esc(belief.statement)}</div>` : ""}
        ${current.note ? `<div class="sub">last night: ${esc(current.note)}</div>` : `<div class="sub">hasn't got to it yet.</div>`}
      </div>
      <button class="ghost" data-act="dropintention::${current.id}" title="stop working on this">Leave it</button>
    </div></div>`;
}

// Tooling Endora has noticed keeps not working (ADR 0054, 0040).
//
// Derived from what it observed, never stored — so there is no badge, no count to clear
// and nothing to dismiss. It states the pattern and asks; it does not guess the answer,
// because guessing would mean parsing one server's format, which is the per-integration
// patching ADR 0054 exists to stop.
//
// Two findings, two different questions. A tool that works elsewhere but not on this
// thing is probably being given the wrong name — so it asks for the name. A tool that
// has never worked on anything is the wrong tool, and no name will fix it — so it offers
// to stop offering it.
function viewRepairs() {
  if (!(REPAIRS || []).length) return "";
  const rows = REPAIRS.map((r) => r.remedy === "stop_offering_it"
    ? `
    <div class="card"><div class="row">
      <div class="grow">
        <div class="title">${esc(r.capability)}</div>
        <div class="sub">${r.attempts} attempts, none of them on anything, and it has never once worked. This looks like the wrong tool rather than the wrong name — turning it off makes the butler reach for one that works.</div>
      </div>
      <button class="ghost danger" data-act="skill:enable:${esc(r.capability)}:0" title="Stop offering this tool to the butler. You can turn it back on any time in Skills.">Stop offering it</button>
    </div></div>`
    : `
    <div class="card"><div class="row">
      <div class="grow">
        <div class="title">${esc(r.capability)}</div>
        <div class="sub">${r.attempts} attempts aimed at “${esc(r.target)}” didn't work. What is it actually called?</div>
        <div class="form" style="margin-top:6px;">
          <input id="alias-${esc(r.capability)}-${esc(r.target)}" placeholder="the real name, e.g. Kitchen Main" />
          <button class="primary" data-act="alias:${esc(r.capability)}:${esc(r.target)}">Remember</button>
        </div>
      </div>
    </div></div>`).join("");
  return `
    <h3 style="margin-top:22px;">Something Endora can't get working</h3>
    <div class="note" style="margin-bottom:10px;">It checked before and after each time. Nothing moved — so either it's aiming at the wrong name, or reaching for the wrong tool.</div>
    ${rows}`;
}

// How Endora's own actions have actually landed (ADR 0053).
//
// Deliberately four numbers rather than one percentage. A claim of success that changed
// nothing is not the same kind of miss as an outright error, and "couldn't be checked" is
// genuinely unknown — counting an unknown as a success is how a system starts lying to
// itself about how well it works.
//
// It names the worst offender because a number nobody can act on is decoration. That is also
// where a tool Endora has no way to know is read-only shows up: it looks like an actuator
// that never changes anything, which is exactly what it is from Endora's side.
function viewHowItLands() {
  if (!LANDING || !LANDING.considered) return "";
  const worst = LANDING.worst_offender;
  return `
    <h3 style="margin-top:22px;">How its actions have landed</h3>
    <div class="card">
      <div class="title" style="font-weight:500;">${LANDING.changed} of ${LANDING.considered} verified as doing what was asked</div>
      <div class="sub" style="margin-top:4px;">${esc(LANDING.in_words)}</div>
      ${worst ? `<div class="sub" style="margin-top:6px;">Most often claims success and changes nothing: <b>${esc(worst.capability)}</b> (${worst.times}×). If that one only ever reads, tell Endora it's this server's reader in <a class="link" data-act="go:skills">Skills</a> — then it stops being treated as an action.</div>` : ""}
    </div>`;
}

// Connect something new to a service Endora already reaches (ADR 0054).
//
// Endora does not know what a calendar or a mail account needs — the service does, and it
// says so. The form below is rendered from whatever came back, so a kind of thing nobody
// here has heard of works exactly like one that ships today.
//
// The suggestions are a convenience, not a list of what is supported: anything the service
// can set up can be typed in. They are what a butler is most likely to be asked for.
const WORTH_CONNECTING = [
  ["caldav", "Calendar", "iCloud, Fastmail, Nextcloud — anything CalDAV"],
  ["local_calendar", "A calendar kept here", "no account needed"],
  ["imap", "Email (read only)", "so it can tell you what arrived"],
  ["mqtt", "Sensors over MQTT", "doors, motion, whatever you add"],
];

// Endpoints Endora knows, so it stops asking for what it could have said.
//
// The form renders exactly what the service declares — and Home Assistant's CalDAV form
// declares no default for `url`, so it asked for an address that Endora could name perfectly
// well. Naming iCloud, Fastmail and Nextcloud in the description and then not offering their
// endpoints is the gap.
//
// These are **suggestions**, not knowledge about the service: choosing one fills the fields
// and every one stays editable, which is the same shape the model settings already use for
// provider presets. Field names come from the live flows, not from memory.
const KNOWN_PROVIDERS = {
  caldav: [
    ["iCloud", { url: "https://caldav.icloud.com" }],
    ["Fastmail", { url: "https://caldav.fastmail.com" }],
    ["Nextcloud", { url: "https://your-server/remote.php/dav" }],
  ],
  imap: [
    ["iCloud", { server: "imap.mail.me.com", port: "993" }],
    ["Gmail", { server: "imap.gmail.com", port: "993" }],
    ["Fastmail", { server: "imap.fastmail.com", port: "993" }],
    ["Outlook", { server: "outlook.office365.com", port: "993" }],
  ],
};

// Fills the form from a chosen provider. Nothing is submitted and nothing is locked — this
// is typing on the person's behalf, which they can undo by typing over it.
function useProvider(kind, index) {
  const chosen = (KNOWN_PROVIDERS[kind] || [])[index];
  if (!chosen) return;
  const [label, values] = chosen;
  for (const [field, value] of Object.entries(values)) {
    const el = document.getElementById(`connect-${field}`);
    if (el) el.value = value;
  }
  flash(`Filled in what ${label} uses — check it and sign in.`, "ok");
}

function viewConnect() {
  if (CONNECT && CONNECT.fields) {
    const fields = CONNECT.fields.map((f) => `
      <div class="field">
        <label>${esc(f.name)}${f.required ? "" : " <span class=\"sub\">(optional)</span>"}</label>
        <input id="connect-${esc(f.name)}" type="${f.secret ? "password" : (f.kind === "boolean" ? "text" : "text")}"
               autocomplete="off" value="${esc(f.default == null ? "" : String(f.default))}"
               placeholder="${f.secret ? "never stored by Endora" : ""}" />
      </div>`).join("");
    const providers = KNOWN_PROVIDERS[CONNECT.kind] || [];
    const presets = providers.length
      ? `<div class="field"><label>Who is it with?</label>
           <select onchange="useProvider('${esc(CONNECT.kind)}', this.value)">
             <option value="">Choose to fill in the address…</option>
             ${providers.map(([label], i) => `<option value="${i}">${esc(label)}</option>`).join("")}
           </select></div>`
      : "";
    return `
      <h3 style="margin-top:22px;">Connecting ${esc(CONNECT.kind || "something")}</h3>
      <div class="card">
        <div class="note" style="margin-bottom:8px;">These are the questions <b>your own service</b> asked — Endora is passing them on. Anything you type here goes straight to it and is never written down here.</div>
        ${presets}
        ${fields}
        <div class="row" style="gap:8px;justify-content:flex-end;">
          <button class="ghost" data-act="connect:cancel">Cancel</button>
          <button class="primary" data-act="connect:submit">Connect</button>
        </div>
      </div>`;
  }
  // Offering to set up something already working reads as though the last attempt failed, and
  // the screen had no other way to answer "is my calendar connected?" (ADR 0058). A service
  // already there says so and offers nothing; adding a second account is still possible from
  // the box below, by name.
  const isConnected = (kind) => (CONNECTED || []).includes(kind);
  const buttons = WORTH_CONNECTING.map(([kind, label, why]) => `
    <div class="row" style="align-items:center;gap:10px;margin-bottom:8px;">
      <div class="grow"><div class="title" style="font-weight:500;">${esc(label)}</div><div class="sub">${isConnected(kind) ? "Already set up — Endora can use it." : esc(why)}</div></div>
      ${isConnected(kind)
        ? `<span class="pill active">connected</span>`
        : `<button class="ghost" data-act="connect:start:${esc(kind)}">Connect</button>`}
    </div>`).join("");
  // Everything else the person has set up themselves. Without this the screen could only ever
  // talk about the four things Endora happens to suggest, and stayed silent about the rest.
  const offered = WORTH_CONNECTING.map(([kind]) => kind);
  const alsoThere = (CONNECTED || []).filter((k) => !offered.includes(k));
  const alsoHtml = alsoThere.length
    ? `<div class="sub" style="margin-top:10px;">Also set up: ${alsoThere.map(esc).join(", ")}</div>`
    : "";
  return `
    <h3 style="margin-top:22px;">Connect something to your home</h3>
    <div class="note" style="margin-bottom:10px;">Endora sets it up in Home Assistant for you — you only sign in. Whatever you add here, it can use straight away.</div>
    <div class="card">
      ${buttons}
      <div class="form" style="margin-top:6px;">
        <input id="connect-other" placeholder="or something else, by its Home Assistant name" />
        <button class="ghost" data-act="connect:other">Start</button>
      </div>
      ${alsoHtml}
    </div>`;
}

// The one thing Endora would like an answer about, shown where you already are.
//
// It used to live on a screen reached by menu → Settings → Understanding → scroll: three
// taps away from the conversation, on a page holding six unrelated things. A question you
// have to go and find is a question that does not get answered — 109 outcomes carried zero
// reactions for exactly that reason.
//
// **One at a time, never a list.** Seven problem statements rendered together is the pile of
// chores ADR 0052 exists to prevent, and putting the pile in the chat would only make it
// harder to ignore. This is the same "a cursor, not a queue" rule that governs intentions:
// one question, answering it is the dismissal, and the next appears when it appears.
//
// No badge and no count anywhere. Both are how a queue announces itself, and a screen with
// "3 waiting" on it is a chore list whatever the cards say.
function viewNeedsYou() {
  // Before anything else: where the person is.
  //
  // One line of text, and four skills are silent without it — weather, the news, safety
  // alerts and what the city is doing are all "near you", and near nowhere is nothing. They
  // report themselves as configured the whole time, so the failure looks like working.
  //
  // It was already being asked, three taps away on Settings > Understanding, which is exactly
  // the placement the comment above this function was written about. A question you have to
  // go and find is a question that does not get answered — and this one is the difference
  // between a butler that knows where it lives and one that does not.
  const knowsWhere = (DB.preferences || []).some((p) => /^\s*based in\b/i.test(p.text));
  if (!knowsWhere) {
    return `
      <div class="card" style="margin:6px 0;border-color:color-mix(in srgb, var(--accent) 30%, var(--line));">
        <div class="title" style="font-weight:500;">Where are you based?</div>
        <div class="sub" style="margin-bottom:6px;">The weather, the news, safety alerts and what's on locally all need this. A town or city is enough.</div>
        <div class="form">
          <input id="setup-location" placeholder="a city, e.g. New York, NY" />
          <button class="primary" data-act="setlocation">Save</button>
        </div>
      </div>`;
  }

  // The person's own world first — a broken lamp is theirs, a misfiring tool is Endora's.
  const trouble = (TROUBLE || [])[0];
  if (trouble) {
    return `
      <div class="card" style="margin:6px 0;border-color:color-mix(in srgb, var(--accent) 30%, var(--line));">
        <div class="row" style="align-items:center;gap:10px;">
          <div class="grow">
            <div class="title" style="font-weight:500;">${esc(trouble.thing)}</div>
            <div class="sub">Hasn't answered ${trouble.days === 1 ? "since yesterday" : `for ${trouble.days} days`}. Still yours?</div>
          </div>
          <button class="ghost" data-act="trouble:gone:${esc(trouble.server)}:${esc(trouble.thing)}" title="Hide it in the service that owns it. Nothing is deleted, and you can put it back.">It's gone</button>
          <button class="ghost" data-act="trouble:fine:${esc(trouble.server)}:${esc(trouble.thing)}" title="Leave it exactly as it is and stop mentioning it.">It's fine</button>
        </div>
      </div>`;
  }
  const repair = (REPAIRS || [])[0];
  if (!repair) return "";
  if (repair.remedy === "stop_offering_it") {
    return `
      <div class="card" style="margin:6px 0;">
        <div class="row" style="align-items:center;gap:10px;">
          <div class="grow">
            <div class="title" style="font-weight:500;">${esc(repair.capability)}</div>
            <div class="sub">${repair.attempts} attempts, and it has never once worked. Looks like the wrong tool rather than the wrong name.</div>
          </div>
          <button class="ghost danger" data-act="skill:enable:${esc(repair.capability)}:0">Stop offering it</button>
        </div>
      </div>`;
  }
  // "Leave it" is not decoration. Answering is meant to be the dismissal, but somebody who
  // does not know the real name has no answer to give — and with no way out the card sits
  // there for good. The trouble card has had "It's fine" for exactly this reason; this one
  // never did.
  return `
    <div class="card" style="margin:6px 0;">
      <div class="grow">
        <div class="title" style="font-weight:500;">${esc(repair.capability)}</div>
        <div class="sub">${repair.attempts} attempts aimed at &ldquo;${esc(repair.target)}&rdquo; didn't work. What is it actually called?</div>
        <div class="form" style="margin-top:6px;">
          <input id="alias-${esc(repair.capability)}-${esc(repair.target)}" placeholder="the real name, e.g. Kitchen Main" />
          <button class="primary" data-act="alias:${esc(repair.capability)}:${esc(repair.target)}">Remember</button>
        </div>
        <div class="row" style="justify-content:flex-end;margin-top:6px;">
          <button class="ghost" data-act="repair:leave:${esc(repair.capability)}:${esc(repair.target)}" title="Stop asking about this one. Nothing is turned off.">Leave it</button>
        </div>
      </div>
    </div>`;
}

// Things in YOUR world that stopped answering — as opposed to Endora's own tooling
// (ADR 0056).
//
// The same shape as a repair finding and deliberately so: a deterministic trigger, a
// specific remedy, and answering is the dismissal. What is different is the subject. A
// butler that reports "13 entities unavailable" has added an item to your day; one that
// says "these have not answered since Tuesday — gone, or shall I hide them?" has removed
// one. The difference is not the observation, it is having watched long enough to say
// since when, and having somewhere for the answer to go.
//
// Nothing accumulates: the record exists only while the thing is still not answering, so
// a device that comes back takes its own row with it.
function viewStandingTrouble() {
  if (!(TROUBLE || []).length) return "";
  const rows = TROUBLE.map((t) => `
    <div class="card"><div class="row">
      <div class="grow">
        <div class="title">${esc(t.thing)}</div>
        <div class="sub">Hasn't answered ${t.days === 1 ? "since yesterday" : `for ${t.days} days`}, in ${esc(t.server)}. Still yours?</div>
      </div>
      <button class="ghost" data-act="trouble:gone:${esc(t.server)}:${esc(t.thing)}" title="Hide it in the service that owns it. Nothing is deleted, and you can put it back.">It's gone</button>
      <button class="ghost" data-act="trouble:fine:${esc(t.server)}:${esc(t.thing)}" title="Leave it exactly as it is and stop mentioning it.">It's fine</button>
    </div></div>`).join("");
  return `
    <h3 style="margin-top:22px;">Things that stopped answering</h3>
    <div class="note" style="margin-bottom:10px;">Endora watches your services and keeps track of when something went quiet. Hiding is never deleting — it comes back from the change log below.</div>
    ${rows}`;
}

// Changes Endora has made to your services' own settings (ADR 0054).
//
// The memory right to SEE what it changed about the world, next to the right to see what
// it believes and what it did — and, unlike those, a button to put each one back. Undone
// changes stay listed: what Endora changed about your house is not something it should be
// able to make disappear.
function viewConfigWrites() {
  if (!(CONFIG_WRITES || []).length) return "";
  const rows = CONFIG_WRITES.map((w) => `
    <div class="card"><div class="row">
      <div class="grow">
        <div class="title">${esc(w.what)}${w.undone ? ` <span class="pill">put back</span>` : ""}</div>
        <div class="sub">in ${esc(w.server)}${w.at_ms ? ` · ${new Date(w.at_ms).toLocaleString()}` : ""}</div>
      </div>
      ${w.undone ? "" : `<button class="ghost" data-act="unwrite::${esc(w.id)}" title="Put this back exactly as it was">Undo</button>`}
    </div></div>`).join("");
  return `
    <h3 style="margin-top:22px;">Changes Endora made to your services</h3>
    <div class="note" style="margin-bottom:10px;">Names it wrote into a connected service so the fix works everywhere, not just here. Each one can be put back.</div>
    ${rows}`;
}

// What Endora has actually DONE, and what it saw afterwards (ADR 0053).
//
// The tool's claim and Endora's observation are shown SEPARATELY, exactly as they
// are stored — the console must not merge them either, because a tool that reports
// success while nothing changed is the whole reason the record exists.
//
// Saying how it landed is optional and never solicited: no badge, no counter, no
// "N awaiting your feedback". An outcome nobody comments on is complete.
function viewOutcomes() {
  if (!(OUTCOMES || []).length) return "";
  const rows = OUTCOMES.map((o) => {
    const seen = o.observed
      ? `<div class="sub">Endora then looked: ${esc(o.observation)}</div>`
      : `<div class="sub">Endora couldn't check this one for itself.</div>`;
    const picked = (r) => o.reaction === r ? " active" : "";
    return `
      <div class="card"><div class="row">
        <div class="grow">
          <div class="title">${esc(o.capability)} <span class="pill${o.observed ? "" : " pending"}">${o.observed ? "checked" : "unconfirmed"}</span></div>
          <div class="sub">It reported: ${esc(o.claim)}</div>
          ${seen}
        </div>
        <button class="ghost${picked("helped")}" data-act="react:helped:${o.id}" title="that helped">Helped</button>
        <button class="ghost${picked("did_not_help")}" data-act="react:did_not_help:${o.id}" title="that didn't help">Didn't</button>
      </div></div>`;
  }).join("");
  return `
    <h3 style="margin-top:22px;">What Endora has done</h3>
    <div class="note" style="margin-bottom:10px;">Only what it changed — reading things isn't listed. Say how one landed if you like; there's no need to.</div>
    ${rows}`;
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

// Trim a long tool result to something readable, keeping the start (where the useful
// part is) and saying how much was left out rather than trailing off silently.
function clip(text, max) {
  const s = String(text || "");
  if (s.length <= max) return s;
  return s.slice(0, max) + `… (+${s.length - max} more characters)`;
}

// Nominate the tool that reads a server's state (ADR 0054). Blank clears it.
async function setReader(el) {
  const server = el.getAttribute("data-reader-for");
  try {
    await api("POST", "/v1/mcp/servers/" + encodeURIComponent(server) + "/reader", { reader_tool: el.value });
    flash(el.value ? `Endora will check ${server} with ${el.value}.` : `No read-back for ${server}.`, "ok");
  } catch (e) {
    flash("Couldn't set that: " + e.message, "err");
  }
  return reload();
}

// Which of a server's tools READS its state (ADR 0054).
//
// One answer settles two things: that tool's result becomes an observation rather than
// a receipt, and everything else on the server is checked through it after it acts.
// Without it Endora can only repeat what a tool claimed about its own work — honest,
// but unverifiable. Endora never picks this itself: a server's own say-so about what
// it does is not evidence.
function readerRow(s) {
  // Tools arrive namespaced as `server.tool`; the nomination is the bare tool name.
  const prefix = s.name + ".";
  const tools = (s.tools || [])
    .map((t) => (t.id || "").startsWith(prefix) ? t.id.slice(prefix.length) : t.id)
    .filter(Boolean);
  if (!tools.length) return "";
  const opts = ['<option value="">— nobody has said —</option>']
    .concat(tools.map((t) => `<option value="${esc(t)}"${t === s.reader_tool ? " selected" : ""}>${esc(t)}</option>`))
    .join("");
  // Stacked, not side by side. A `<select>` sizes itself to its longest option, and
  // `HassMediaSearchAndPlay` is long enough to squeeze the label beside it down to one word
  // per line — which is what it did on a real screen. Label above, control below, full
  // width: the same shape as every other field, and nothing to squeeze.
  return `
    <div class="field" style="margin-top:8px;">
      <label>Which tool reads this server's state?</label>
      <div class="sub" style="margin:-2px 0 4px;">Endora uses it to check what an action actually did, instead of taking the tool's word for it.</div>
      <select data-reader-for="${esc(s.name)}" onchange="setReader(this)" style="width:100%;">${opts}</select>
    </div>`;
}

// HTML-string versions for rendering a PAST message's persisted actions in the
// chat history (collapsed; click to expand). Same look as the live panel.
// What Endora actually DID this turn, and whether it confirmed the effect (ADR 0053).
//
// Not collapsible and not buried under "details", unlike the step trail: the whole
// point is that it is visible without being asked for. The model ignores the read-back
// roughly two runs in three and asserts unverified success every time, so the reply
// above this may well claim the opposite of what it says here. Both are shown; the
// person judges. Nothing here edits the reply.
// A subtle note of what Endora did behind the scenes on THIS turn — what it looked up and
// what it learned. Rendered per message from the stored record, so it is still there when
// you come back to the chat (ADR 0056). It used to be drawn from a live stream event held
// in memory, which meant the note existed only while you stayed on the screen.
function activityHtml(activity) {
  if (!SHOW_ACTIVITY || !Array.isArray(activity) || !activity.length) return "";
  return `<div class="activity">${icon("sparkle", 13)} ${activity.map(esc).join(" · ")}</div>`;
}

// `latest` adds the one question worth asking: did that help?
//
// It is shown on the NEWEST turn only, and never anywhere else. The machinery for judging
// an outcome has existed for months and had never once been used, because the only place
// to say so was a section further down a screen nobody opens — which, by its own design,
// never asked. A loop with no input is not a loop.
//
// Still no badge, no counter, nothing that accumulates: the ask is gone by the next turn
// whether or not it was answered, so ignoring it stays free. That is the anti-queue rule
// (ADR 0052) kept, while actually asking once, where you already are.
function actionsTakenHtml(actions, latest) {
  if (!actions || !actions.length) return "";
  const rows = actions.map((a) => {
    const ask = (latest && a.outcome)
      ? `<span class="step-more" style="margin-left:8px;">
           <button class="ghost" data-act="react:helped:${a.outcome}" title="that helped" style="padding:1px 7px;font-size:12px;">Helped</button>
           <button class="ghost" data-act="react:did_not_help:${a.outcome}" title="that didn't help" style="padding:1px 7px;font-size:12px;">Didn't</button>
         </span>`
      : "";
    const what = `<span class="step-more">${esc(a.skill)}</span>`;
    if (a.confirmed) {
      // Bounded: a read-back can run to thousands of characters, and pasting the whole
      // thing under the reply buries it. The full text is still in the step trail.
      return `<div class="step-row">${icon("check", 13)}<span>${what} — Endora checked afterwards: ${esc(clip(a.observed, 240))}</span>${ask}</div>`;
    }
    return `<div class="step-row">${icon("target", 13)}<span>${what} — reported ${esc(clip(a.claimed, 240))}, <strong>not confirmed</strong>. Endora couldn't check this one for itself.</span>${ask}</div>`;
  }).join("");
  return `<div class="row" style="justify-content:flex-start;margin:2px 0;"><div class="steps" style="padding:8px 10px;"><div class="step-list">${rows}</div></div></div>`;
}
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
// model produces prose, finishing with the "done" event. On the
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

// Update the Speak button in place (icon + label) without a full render, so
// toggling it never disturbs a streaming reply.
function updateSpeakButton() {
  const b = document.querySelector('[data-act="toggle:speak"]');
  if (b) b.innerHTML = `${icon(SPEAK ? "speakerOn" : "speakerOff")}<span>${SPEAK ? "Speaking" : "Speak"}</span>`;
}

// Update the Deep-mode toggle in place (same reasoning as the Speak button).
function updateDeepButton() {
  const b = document.querySelector('[data-act="toggle:deep"]');
  if (b) {
    b.classList.toggle("active", DEEP_MODE);
    b.innerHTML = `${icon("sparkle", 15)}<span>${DEEP_MODE ? "Deep: on" : "Ask deep"}</span>`;
  }
}

// Send a question to the deep (bigger) model. It persists both the question and the
// answer server-side, so a reload shows the exchange. Keeps the input until it goes
// through, so nothing is lost if the deep model is off or unreachable.
async function askDeep(q, input) {
  appendBubble(esc(q), "me");
  flash("Asking the deep model…", "ok");
  try {
    const r = await api("POST", "/v1/deep-ask", { question: q });
    if (r && r.answered === false) { flash(r.note || "No deep model configured.", "err"); return; }
    if (input) { input.value = ""; growInput(input); }
  } catch (e) { flash("Deep model: " + e.message, "err"); return; }
  return reload();
}

// Stop the in-flight turn and drop anything still queued.
// A short buzz for the moments you may not be looking at the screen: a reply
// landing, and the mic actually starting to listen. Android honours it; iOS ignores
// vibrate entirely, so this is a bonus rather than a signal anything depends on.
function buzz(ms) {
  if (HAPTIC && navigator.vibrate) navigator.vibrate(ms);
}

function stopChat() {
  CHAT_QUEUE = [];
  // Remember that THIS was deliberate. The reload in the stream's `finally` renders
  // from persisted state, which ends with the person's message and no reply — so the
  // thinking indicator comes back and never leaves, because the reply it is waiting
  // for was cancelled. Marking the stop lets the render say so instead.
  CHAT_STOPPED = true;
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
  // A new turn: whatever was stopped before is history.
  CHAT_STOPPED = false;
  // Talking always happens today, so a reply can never land in a day you were reading.
  CHAT_DAY = null;
  const thread = document.getElementById("chat-thread");
  if (thread && thread.querySelector(".empty")) thread.innerHTML = "";
  // Deep mode on: route to the bigger model instead of the everyday butler. Keep the
  // text until it lands (askDeep clears it on success).
  if (DEEP_MODE && DEEP_MODEL && DEEP_MODEL.configured) { askDeep(msg, input); return; }
  if (input) { input.value = ""; growInput(input); }
  appendBubble(esc(msg), "me"); // show it immediately, even if it waits its turn
  CHAT_QUEUE.push(msg);
  drainChat();
}

// Process the queue one turn at a time.
async function drainChat() {
  if (CHAT_STREAMING || !CHAT_QUEUE.length) return;
  const msg = CHAT_QUEUE.shift();
  // Remember the in-flight turn in module state so a re-render (e.g. toggling
  // Speak) can rebuild the just-sent message and the reply growing in, instead of
  // wiping them (they aren't in the persisted DB.messages until the turn ends).
  CHAT_INFLIGHT = msg;
  LIVE_REPLY = "";
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
  let acc = "";
  CHAT_STREAMING = true;
  CHAT_ABORT = new AbortController();
  updateComposerButton();
  try {
    const res = await fetch("/v1/chat/stream", {
      method: "POST",
      headers: signed({ "content-type": "application/json" }),
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
          LIVE_REPLY = acc;
          // Look the bubble up fresh each token: if a re-render replaced the thread
          // mid-stream, viewChat rebuilt a #chat-live from LIVE_REPLY and we keep
          // writing into that one rather than a now-detached node.
          const liveRow = document.getElementById("chat-live");
          const bod = liveRow && liveRow.querySelector(".bubble");
          if (bod) bod.textContent = acc;
          if (liveRow) scrollBubbleIntoView(liveRow);
        } else if (ev.type === "step") {
          if (ev.status === "running") {
            STEP_LIST.push({ skill: ev.skill, label: ev.label, status: "running", output: null });
          } else {
            // Terminal: finalize the last still-running step, else record it fresh
            // (a "blocked" step arrives with no prior "running").
            // Match on skill as well as state: a blocked call reports no "running"
            // at all, so finalising whatever happened to be in flight would overwrite
            // an unrelated call and hide the refusal.
            let i = STEP_LIST.length - 1;
            while (i >= 0 && !(STEP_LIST[i].status === "running" && STEP_LIST[i].skill === ev.skill)) i--;
            if (i >= 0) { STEP_LIST[i].status = ev.status; STEP_LIST[i].output = ev.output || null; }
            else STEP_LIST.push({ skill: ev.skill, label: ev.label, status: ev.status, output: ev.output || null });
          }
          renderSteps(stepsWrap, STEP_LIST, true);
          if (live) scrollBubbleIntoView(live);
        } else if (ev.type === "done") {
          // A reply landed, so nothing is outstanding.
          CHAT_STOPPED = false;
          buzz(25);
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
      const liveRow = document.getElementById("chat-live");
      const bod = liveRow && liveRow.querySelector(".bubble");
      if (bod) bod.textContent = "(stopped)";
      renderSteps(stepsWrap, STEP_LIST, false);
    } else {
      // Don't re-send (the server may have already saved the turn) — reload to the
      // true persisted state below.
      flash("The butler's reply was interrupted — your message was saved.", "err");
    }
  } finally {
    CHAT_STREAMING = false;
    CHAT_ABORT = null;
    CHAT_INFLIGHT = null;
    LIVE_REPLY = "";
    const liveRow = document.getElementById("chat-live");
    if (liveRow) liveRow.removeAttribute("id"); // free "chat-live" for the next turn
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
  updateMenuState();
  // Nothing else can load without a token, so nothing else is worth drawing.
  if (NEEDS_TOKEN) { app.innerHTML = viewNeedsToken(); return; }
  const v = NAV.v;
  app.innerHTML =
      v === "audit" ? viewAudit()
    : v === "chat" ? viewChat()
    : v === "skills" ? viewSkills()
    : v === "prefs" ? viewPrefs()
    : v === "settings" ? viewSettings()
    : v === "inbox" ? viewInbox()
    : v === "display" ? viewDisplay()
    : v === "models" ? viewModels()
    : v === "proactive" ? viewProactive()
    : v === "understanding" ? viewUnderstanding()
    : v === "signin" ? viewSignin()
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
      try { await api("DELETE", "/v1/mcp/servers/" + encodeURIComponent(id)); flash("Server removed.", "ok"); }
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
    if (verb === "trouble") {
      // Re-split from the raw action: a thing's name is the last field and may itself
      // contain a colon, so the fixed four-way destructure above would truncate it.
      const parts = act.split(":");
      const answer = String(noun || "");
      const server = parts[2] || "";
      const thing = parts.slice(3).join(":");
      try {
        const said = await api("POST", "/v1/standing-trouble/answer", { server, thing, answer });
        flash(said.done ? said.done.charAt(0).toUpperCase() + said.done.slice(1) : "Noted.", "ok");
      } catch (e) { flash("Couldn't do that: " + e.message, "err"); }
      return reload();
    }
    // Stop working on something. The person's ONLY verb over an intention — there is
    // deliberately no way to create or edit one (ADR 0052).
    if (verb === "dropintention") {
      try { await api("POST", `/v1/intentions/${id}/drop`); flash("Alright — I'll leave that.", "ok"); }
      catch (e) { flash("Couldn't stop that: " + e.message, "err"); }
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
    // Nightly self-improvement loop (ADR 0051): `noun` is "off" or a LOCAL hour.
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
      item(
        "go:inbox",
        "inbox",
        "Inbox",
        // A count only when there is something unread — a badge showing "0" is just
        // furniture, and one that never clears is a nag.
        unreadCount() ? `<span class="pill active">${unreadCount()}</span>` : "",
      ) +
      item("go:settings", "prefs", "Settings") +
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

// ---- boot -----------------------------------------------------------------
(async function () {
  try {
    const health = await api("GET", "/health");
    setupHeader(health);
    await reload();
    subscribeToActivity();
  } catch (e) {
    // Say which half failed. This catch covers reaching the node AND drawing the page, and
    // it reported both as "couldn't reach the node" — so a missing function read as a
    // network problem and sent the diagnosis in the wrong direction for a while.
    const unreachable = e instanceof TypeError && /fetch|network|load failed/i.test(e.message);
    app.innerHTML = unreachable
      ? `<div class="msg show err">Couldn't reach the node: ${esc(e.message)}</div>`
      : `<div class="msg show err">Endora is running, but the console failed to draw:
           ${esc(e.message)}</div>`;
  }
})();
