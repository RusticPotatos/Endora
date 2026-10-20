"use strict";

let DB = null;                 // latest /v1/export snapshot — NULL until one lands
let MAIL = null;               // the mail digest (/v1/mail), fetched only on the Mail screen
let AI = null;                 // every model Endora uses (/v1/ai), fetched only on the AI screen
let SPOTIFY = null;            // the Spotify sign-in, held by the node (/v1/connections/spotify)
let LAB = {};                  // the home lab, media and smart-home pages, by name (/v1/terminal/*)
let MUSIC = { now: null, devices: [], found: [], lists: [], query: "", error: "" }; // what Spotify says

// The conversation, safely. `DB` is null before the first export and stays null when
// the node is unreachable, so anything that runs on every render — the menu badge, in
// particular — must not assume a snapshot exists. Reading `DB.messages` there threw
// "null is not an object", and that error then replaced the real one on screen: the
// person saw a JavaScript failure instead of "couldn't reach the node".
function messages() {
  return (DB && DB.messages) || [];
}
let ACTIVITY = [];             // latest /v1/activity feed (newest first)
const ACTIVITY_PAGE = 30;      // rows the Activity screen opens with, and each "Load more" adds
let ACTIVITY_MORE = false;     // the last page was full, so there may be older rows
let ACTIVITY_BUSY = false;     // a page is being fetched
let CAPS = [];                 // the butler's capabilities/skills (modules)
let MCP_SERVERS = { servers: [] }; // connected MCP servers + their tools (ADR 0054)
let RECIPES = [];              // the person's own capabilities — five fields, no code (ADR 0071)
let AUTONOMY = { auto_external: true, auto_consequential: false }; // the autonomy envelope (ADR 0051)
let BRIEF_SCHED = { enabled: false, hour_utc: 12 }; // daily-brief schedule
let DEEP_MODEL = { configured: false, key_set: false, url: "", model: "" }; // optional bigger AI
let MODEL_LISTS = null;        // providers and the ordered everyday and deep lists (ADR 0105)
let PROVIDER_EDITING = null;   // the provider whose form is open for changing, if any
let FOUND_MODELS = {};         // what Discover found, by list: { provider, models }, kept across re-renders
let BRAIN = null;              // What Endora knows about you: memory and its pages (ADR 0101)
let BRAIN_PAGE = null;         // The page open on its own screen, with its history
let BRAIN_EDITING = false;     // Whether that page is open as text to edit
let OUTCOMES = [];             // what Endora DID, and what it saw afterwards (ADR 0053)
let RESEARCH = { items: [] };   // explicit durable research assignments (ADR 0077)
let RESEARCH_READY = { ready: false, modes: [], search_tools: [] };
let RESEARCH_MODE = localStorage.getItem("endora.researchmode") === "1";
let RESEARCH_PENDING = (() => {
  try { return JSON.parse(localStorage.getItem("endora.researchPending") || "null"); }
  catch (_) { return null; }
})();
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
let CTX = null;                // what the next message would send, and how full the window is (ADR 0088)
let DEEP_USAGE = null;         // what the deep model's calls have used, and what that costs if priced (ADR 0091)
let DEEP_BALANCE = null;       // what the deep model's provider says is left, when it can say
let CTX_OPEN = false;          // whether the inspector is showing
let CHAT_DAYS = [];            // [{day, messages}] — which days have anything, for the bar
let LAST_VIEW = null;          // which screen was showing, so a change can reset the scroll
let REPAIRS = [];              // tooling Endora has noticed keeps not working (ADR 0054)
let LANDING = null;            // how Endora's recent actions actually landed (ADR 0053)
let CONNECT = null;            // a setup form a service is asking us to fill in (ADR 0054)
let CONFIG_WRITES = [];        // changes Endora made to your services' own settings (ADR 0054)
let LAST_ACTIVITY = [];        // what Endora did behind the scenes on the last turn
let LAST_ACTIVITY_MSG = null;  // the butler message id that activity belongs to
let STEP_LIST = [];            // the live action trail for the turn currently streaming
let SHOW_ACTIVITY = localStorage.getItem("endora.showActivity") !== "0"; // default on
let CHAT_STREAMING = false;    // true while a reply is streaming in (guards live-render)
// Whether the NODE says it is taking a turn — not whether this tab is. A turn that failed
// left the last stored message being the person's, and the screen showed a thinking bubble
// for ever, through a reload, because the only thing it could ask was itself.
let NODE_BUSY = false;
let HAPTIC = localStorage.getItem("endora.haptic") !== "0"; // buzz on reply + mic (default on)
let CHAT_STOPPED = false;      // the person stopped the last turn, so no reply is coming
let CHAT_QUEUE = [];           // messages awaiting their turn — turns are SERIALIZED
let LAST_CHAT_REQUEST = null;  // reuse the key when retrying an interrupted turn
let CHAT_INFLIGHT = null;      // the user message whose reply is streaming now (not yet persisted)
let LIVE_REPLY = "";           // the reply text accumulated so far, so a re-render can rebuild it
let CHAT_ABORT = null;         // AbortController for the in-flight turn (the Stop button)
// The current view. Every device opens on HQ: Endora is HQ (ADR 0136).
let NAV = { v: "hq" };

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
  up: '<path d="M12 19V6M6 12l6-6 6 6"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  close: '<path d="M6 6l12 12M18 6L6 18"/>',
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
  person: '<circle cx="12" cy="8.5" r="3.6"/><path d="M4.5 20a7.5 7.5 0 0 1 15 0"/>',
  lock: '<rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3"/>',
  music: '<path d="M9 18V5.5l11-2V16"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="17.5" cy="16" r="2.5"/>',
  house: '<path d="M3.5 11L12 4l8.5 7"/><path d="M6 9.5V20h12V9.5"/><path d="M10 20v-5h4v5"/>',
  film: '<rect x="3.5" y="5" width="17" height="14" rx="2"/><path d="M3.5 9.5h17M3.5 14.5h17M8 5v14M16 5v14"/>',
  server: '<rect x="4" y="4" width="16" height="7" rx="1.5"/><rect x="4" y="13" width="16" height="7" rx="1.5"/><path d="M7.5 7.5h.01M7.5 16.5h.01"/>',
  box: '<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/>',
};
function icon(name, size = 17) {
  const p = ICONS[name];
  if (!p) return "";
  return `<svg class="ic" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${p}</svg>`;
}

