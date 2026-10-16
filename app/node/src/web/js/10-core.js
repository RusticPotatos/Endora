// ---- tiny helpers ---------------------------------------------------------
const esc = (s) => String(s).replace(/[&<>"']/g, (c) =>
  ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

// Butler prose, rendered as the markdown the models actually write. Escaped FIRST —
// everything below works on already-escaped text, so nothing a headline or a tool result
// carries can become markup — and only then dressed:
//   blocks:  paragraphs, "- " / "* " / "1." lists, # headings, > quotes, --- rules and
//            ``` fenced code (kept verbatim, never dressed inside);
//   inline:  [label](url) and bare links (shown as their site's name — a 200-character
//            news URL used to punch every bubble wide open), **bold**, *italic*, `code`.
// A fence still open at the end of the text is closed for display, so a reply that is
// halfway through a code block while streaming reads as code, not as stray backticks.
function inlineRich(text) {
  const spans = [];
  // Code spans first, held aside so nothing inside them is dressed.
  let s = text.replace(/`([^`\n]+)`/g, (_m, code) => {
    spans.push(`<code>${code}</code>`);
    return `\u0001${spans.length - 1}\u0001`;
  });
  // [label](url) first, so its url is not also linkified as a bare one below.
  s = s.replace(/\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)/g,
    (_m, label, url) => `<a href="${url}" target="_blank" rel="noopener noreferrer">${label}</a>`);
  // Bare links, shown as their host so the words break, not the bubble. The guard
  // group skips urls already sitting inside an href="..." from the pass above (a
  // raw quote cannot appear in escaped text, so the only `"` here is ours).
  s = s.replace(/(^|[^"'>])(https?:\/\/[^\s<)]+)/g, (_m, before, url) => {
    const trimmed = url.replace(/[.,;:!?]+$/, "");
    const tail = url.slice(trimmed.length);
    let shown = trimmed;
    try { shown = new URL(trimmed).hostname.replace(/^www\./, ""); } catch (_) { /* shown stays the url */ }
    return `${before}<a href="${trimmed}" target="_blank" rel="noopener noreferrer">${shown}</a>${tail}`;
  });
  s = s.replace(/\*\*([^*\n]+)\*\*/g, "<b>$1</b>");
  // One asterisk, hugging its words: "2 * 3 * 4" is arithmetic, not emphasis.
  s = s.replace(/(^|[^*\w])\*([^*\s][^*\n]*?)\*(?![*\w])/g, "$1<i>$2</i>");
  return s.replace(/\u0001(\d+)\u0001/g, (_m, i) => spans[Number(i)]);
}
function rich(text) {
  const fences = [];
  const held = esc(text).replace(/\r\n/g, "\n")
    .replace(/```[^\n`]*\n([\s\S]*?)(?:```|$)/g, (_m, code) => {
      fences.push(code.replace(/\n$/, ""));
      return `\n\u0000${fences.length - 1}\u0000\n`;
    });
  const out = [];
  let para = [];
  let list = null; // { tag, items: [html] }
  const flushPara = () => {
    if (para.length) out.push(`<p>${para.map(inlineRich).join("<br>")}</p>`);
    para = [];
  };
  const flushList = () => {
    if (list) out.push(`<${list.tag}${list.start > 1 ? ` start="${list.start}"` : ""}>${list.items.map((i) => `<li>${i}</li>`).join("")}</${list.tag}>`);
    list = null;
  };
  const flush = () => { flushPara(); flushList(); };
  for (const line of held.split("\n")) {
    let m;
    if ((m = /^\u0000(\d+)\u0000$/.exec(line))) {
      flush();
      out.push(`<pre class="code"><code>${fences[Number(m[1])]}</code></pre>`);
    } else if (/^\s*$/.test(line)) {
      flush();
    } else if ((m = /^#{1,6}\s+(.+)$/.exec(line))) {
      flush();
      out.push(`<p class="h">${inlineRich(m[1])}</p>`);
    } else if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      flush();
      out.push("<hr>");
    } else if ((m = /^\s*[-*•]\s+(.+)$/.exec(line))) {
      flushPara();
      if (!list || list.tag !== "ul") { flushList(); list = { tag: "ul", start: 1, items: [] }; }
      list.items.push(inlineRich(m[1]));
    } else if ((m = /^\s*(\d+)[.)]\s+(.+)$/.exec(line))) {
      flushPara();
      if (!list || list.tag !== "ol") { flushList(); list = { tag: "ol", start: Number(m[1]), items: [] }; }
      list.items.push(inlineRich(m[2]));
    } else if ((m = /^&gt;\s?(.*)$/.exec(line))) {
      flush();
      const last = out[out.length - 1];
      const html = inlineRich(m[1]);
      if (last && last.startsWith("<blockquote>")) out[out.length - 1] = last.replace(/<\/blockquote>$/, `<br>${html}</blockquote>`);
      else out.push(`<blockquote>${html}</blockquote>`);
    } else if (list && /^\s+\S/.test(line)) {
      // An indented line under a list item belongs to that item.
      list.items[list.items.length - 1] += `<br>${inlineRich(line.trim())}`;
    } else {
      flushList();
      para.push(line);
    }
  }
  flush();
  return out.join("");
}
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
  // Whether the node is mid-turn, asked every reload — the answer that outlives this tab.
  try { NODE_BUSY = !!(await api("GET", "/health")).busy; } catch (_) { NODE_BUSY = false; }
  const [db, activity, caps, autonomy] = await Promise.all([
    api("GET", "/v1/export"),
    api("GET", "/v1/activity?limit=30"),
    api("GET", "/v1/capabilities"),
    api("GET", "/v1/autonomy"),
  ]);
  try { BRIEF_SCHED = await api("GET", "/v1/brief/schedule"); } catch (_) {}
  try { DEEP_MODEL = await api("GET", "/v1/deep-model"); } catch (_) {}
  try { MODEL_CONFIG = await api("GET", "/v1/model-config"); } catch (_) {}
  try { MCP_SERVERS = await api("GET", "/v1/mcp/servers"); } catch (_) {}
  // What the next message would send. Only asked for on the chat, since it builds the same
  // context a turn does.
  if (NAV.v === "chat") { try { CTX = await api("GET", "/v1/chat/context"); } catch (_) { CTX = null; } }
  await loadView(NAV.v, NAV.id);
  try { RECIPES = await api("GET", "/v1/recipes"); } catch (_) { RECIPES = []; }
  DB = db;
  // Attach each butler reply's persisted action trail (steps + sources) from the
  // chat endpoint, so past answers keep their expandable actions after a reload.
  // Just the day being shown, with its action trails already attached — rather than
  // every message there has ever been so the browser can throw most of them away.
  await loadChatDay(CHAT_DAY);
  ACTIVITY = activity;
  CAPS = caps;
  AUTONOMY = autonomy;
  try { OUTCOMES = await api("GET", "/v1/outcomes"); } catch (_) { OUTCOMES = []; }
  try { RESEARCH = await api("GET", "/v1/research"); } catch (_) { RESEARCH = { items: [] }; }
  try { RESEARCH_READY = await api("GET", "/v1/research/readiness"); } catch (_) {}
  try { CONNECTED = await api("GET", "/v1/connect/connected"); } catch (_) { CONNECTED = []; }
  try { SIGNIN = await api("GET", "/v1/session/setup"); } catch (_) { SIGNIN = null; }
  try { REPAIRS = await api("GET", "/v1/repairs"); } catch (_) { REPAIRS = []; }
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

// While research runs unattended, refresh every so often so its
// progress and its ending show up without the person having to reload.
function pollActiveWork() {
  setInterval(() => {
    const researching = ((RESEARCH && RESEARCH.items) || []).some((item) =>
      !["delivered", "blocked", "budget_exhausted", "cancelled", "failed"].includes(item.status));
    const ae = document.activeElement;
    if (!researching || CHAT_STREAMING || (ae && /^(INPUT|TEXTAREA|SELECT)$/.test(ae.tagName))) return;
    reload().catch(() => {});
  }, 15000);
}

// What one screen shows that nothing else needs, fetched when it opens and on a reload while it
// is showing. Opening Mail from the menu used to show "Loading…" until something else happened
// to reload the console, because only `reload` fetched it.
async function loadView(v, id) {
  // The mail digest: only on its own screen, so the rest of the console never carries it.
  if (v === "mail") { try { MAIL = await api("GET", "/v1/mail"); } catch (_) { MAIL = null; } }
  // What Endora knows about you: the brain, on About you and on a page's own screen.
  if (v === "understanding" || v === "prefs") {
    try { BRAIN = await api("GET", "/v1/brain"); } catch (_) { BRAIN = null; }
  }
  if (v === "brainpage") {
    BRAIN_EDITING = false;
    try { BRAIN_PAGE = await api("GET", `/v1/brain/pages/${id}`); } catch (_) { BRAIN_PAGE = null; }
  }
  // What Endora runs: it asks the model server, so only on the screen that shows it.
  if (v === "models") { try { AI = await api("GET", "/v1/ai"); } catch (_) { AI = null; } }
  // One sign-in for every device: shown wherever it was started.
  if (v === "connections" || v === "spotify") {
    try { SPOTIFY = await api("GET", "/v1/connections/spotify"); } catch (_) { SPOTIFY = null; }
  }
  // What the deep model has cost, and what is left. The balance asks the provider, so it is
  // fetched only on the screens that show it, never on a timer.
  if ((v === "models" || v === "settings") && DEEP_MODEL.configured) { await loadDeepSpend(); }
  await loadLab(v, id);
  if (v === "hq") await loadHQ();
}

// The screens `loadView` fetches for.
const FETCHES_ITS_OWN = ["mail", "models", "settings", "connections", "spotify",
  "understanding", "prefs", "brainpage",
  "lab", "containers", "logs", "jobs", "job", "media", "house", "music", "hq"];

function go(v, id) {
  // One container's logs must never show under another's name while the new ones load.
  if (v === "logs") LAB.logs = null;
  if (v === "job") LAB.job = null;
  // A page must never show under another's name while the new one loads.
  if (v === "brainpage") BRAIN_PAGE = null;
  NAV = { v, id }; clearMsg(); closeMenu(); render();
  if (!FETCHES_ITS_OWN.includes(v)) return;
  // Drawn at once with what is known, then again with what the screen fetched — unless the
  // person has moved on meanwhile.
  loadView(v, id).then(() => { if (NAV.v === v && NAV.id === id) render(); }).catch(() => {});
}

// Observations reachable under a target (target → assumptions → experiments → obs).
const val = (id) => document.getElementById(id).value.trim();

