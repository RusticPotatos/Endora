// Renders every screen of the console in Node, and drives the requests it makes, so a broken
// one fails a build instead of a person's phone.
//
// **Everything here runs in about 50 milliseconds.** That is not incidental: a check nobody
// wants to run is a check nobody runs, and the whole offline gate is held under ten seconds on
// purpose. Nothing in this file may touch the network or start a browser — the console's own
// code is already executing in this context, so exercising it costs nothing.
//
// The bug this exists for: a call to `activityHtml` survived an edit that deleted the
// function, the file stayed syntactically perfect, `node --check` passed, CI passed, the
// smoke check passed — and the console rendered nothing at all. Every guarantee in the Rust
// half of this repo is enforced by the compiler; the console had no equivalent, and that
// gap cost the whole interface.
//
// **It works by execution, not by parsing.** An earlier attempt tried to find "called but
// never defined" with a regex over the source, and could not be made sound: a JavaScript
// regex literal like `/https?:\/\//` reads as a line comment to a naive stripper, which then
// swallows real code and reports all-clear on a broken file. A checker that lies is worse
// than no checker. Actually calling the functions needs no parser of my own.
//
// The stubs below are deliberately **explicit and minimal**. A `Proxy` that answers any
// unknown global would make everything pass, including the exact failure this is here to
// catch. If a real browser global is missing, this reports it by name and it gets added.

import { readFileSync, readdirSync } from "node:fs";
import { createContext, runInContext } from "node:vm";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
// The console is a set of ordered parts that the node concatenates into one script (they share a
// single global scope, as the one file did). Join them the same way: by file name, in order.
const jsDir = resolve(here, "../app/node/src/web/js");
const source = readdirSync(jsDir)
  .filter((name) => name.endsWith(".js"))
  .sort()
  .map((name) => readFileSync(resolve(jsDir, name), "utf8"))
  .join("");

/** A DOM node that answers anything the renderers ask of it, without pretending to render. */
const element = () => ({
  innerHTML: "",
  textContent: "",
  value: "",
  checked: false,
  hidden: false,
  style: {},
  dataset: {},
  classList: { add() {}, remove() {}, toggle() {}, contains: () => false },
  appendChild() {},
  removeChild() {},
  setAttribute() {},
  removeAttribute() {},
  addEventListener() {},
  removeEventListener() {},
  scrollIntoView() {},
  focus() {},
  blur() {},
  click() {},
  closest: () => null,
  querySelector: () => null,
  querySelectorAll: () => [],
  remove() {},
  scrollTo() {},
  scrollHeight: 0,
  offsetHeight: 0,
});

const stubs = {
  console,
  setTimeout,
  clearTimeout,
  setInterval,
  clearInterval,
  queueMicrotask,
  URL,
  AbortController,
  Blob,
  TextDecoder,
  requestAnimationFrame: (fn) => setTimeout(fn, 0),
  // Nothing may reach the network from a build check.
  fetch: () => Promise.reject(new Error("no network in the console check")),
  EventSource: class {
    addEventListener() {}
    close() {}
  },
  localStorage: {
    getItem: () => null,
    setItem() {},
    removeItem() {},
  },
  navigator: { vibrate: undefined, userAgent: "node", mediaDevices: undefined },
  speechSynthesis: { speak() {}, cancel() {}, getVoices: () => [] },
  alert() {},
  confirm: () => false,
  document: {
    getElementById: () => element(),
    querySelector: () => element(),
    querySelectorAll: () => [],
    createElement: () => element(),
    addEventListener() {},
    removeEventListener() {},
    body: element(),
    documentElement: element(),
    hidden: false,
  },
  location: { href: "https://node.test/", origin: "https://node.test", reload() {} },
  history: { pushState() {}, replaceState() {} },
};
stubs.window = stubs;
stubs.globalThis = stubs;

const context = createContext(stubs);
runInContext(source, context, { filename: "app.js" });

// A house with something in it — which is the state that actually exercises the code.
//
// The first version of this check populated nothing and **failed to catch the very bug it
// was written for**: `activityHtml` is called inside `viewChat` only for a message that has
// an action trail, so with no messages that branch never ran and the check reported all 21
// screens fine on a file that rendered nothing. An empty state exercises the early returns
// and almost nothing else.
//
// So every screen is given one representative item. Not a fixture of convenience: each
// field here is a shape the console has actually received, so a renderer that assumes more
// than the API sends fails here rather than on a phone.
//
// Assigned by running a line in the same context rather than by setting properties: the
// console declares its state with `let`, and a `let` at the top of a VM script is a lexical
// binding, not a property of the global — so `context.DB = …` would quietly create a second
// `DB` that the real one shadows, and the check would go on testing the wrong thing.
runInContext(
  `
  DB = {
    preferences: [{ id: "1", text: "based in Boston, MA" }],
    audit: [{ id: "1", at_ms: 1, decision: "allowed", capability: "weather", reason: "" }],
    messages: [],
  };
  CHAT_MSGS = [
    { id: "1", role: "user", text: "is the kitchen table light on?", at_ms: 1 },
    { id: "2", role: "butler", text: "It is on.", at_ms: 2, actions: {
        activity: ["Learned that you prefer Fahrenheit"],
        actions_taken: [{ skill: "home.HassTurnOff", claimed: "done", observed: "still on",
                          confirmed: true, outcome: "42" }],
        steps: [{ skill: "home.GetLiveContext", status: "done", label: "Checking your home",
                  output: "Kitchen Table: on" }],
        sources: [{ title: "Home Assistant", url: "http://ha.local" }],
      } },
  ];
  CHAT_DAYS = [{ day: "2026-07-30", count: 2 }];
  UNDERSTANDING = [{ id: "1", statement: "you prefer temperatures in Fahrenheit",
                     kind: "preference", confidence: "high", evidence: "you asked for it",
                     settled: true, last_affirmed_ms: 1 }];
  OUTCOMES = [{ id: "42", capability: "home.HassTurnOff", claim: "done",
                observation: "still on", observed: true, reaction: null, at_ms: 1 }];
  INTENTIONS = [{ id: "1", summary: "find a running route", state: "active", steps_left: 3 }];
  RESEARCH = { items: ["queued", "running", "retry_wait", "delivered", "blocked",
    "budget_exhausted", "cancelled", "failed"].map((status, index) => ({
      id: String(100 + index), assignment: index === 0 ? "<img src=x onerror=alert(1)>" : "Compare backups",
      status, phase: "gathering", reason: status === "blocked" ? "A source could not be read." : "",
      budget: { model_calls_used: 2, model_calls_limit: 16 },
      report_available: status === "delivered", can_cancel: !["delivered", "blocked", "budget_exhausted", "cancelled", "failed"].includes(status),
    })) };
  RESEARCH_READY = { ready: true, modes: ["sources"], search_tools: [] };
  // Open, with evidence — an empty fixture once missed the very bug it was written for, and a
  // notion carrying no evidence would skip the one line here that can actually throw.
  // One offered service already set up, and one the person added themselves — both branches
  // of the Connect screen, which previously rendered only the "nothing is connected" case.
  CONNECTED = ["caldav", "hue", "tplink"];
  // One enabled with a declared input (exercises the "needs" line and the "On"
  // pill) and one still off (the "Off" pill and its own button label) — both
  // branches of the enable toggle, which an empty list would never reach.
  // Mail read, with something worth a look and a bucket with items, so every branch of the
  // Mail screen renders. A sentence with markup in it, which must come out escaped.
  MAIL = { mounted: true, reading: true, indexed: 412, unread_recent: 37,
    important: [{ from: "Example Dental", at_ms: 1, bucket: "money", sentence: "A bill <b>due</b>.",
                  why: ["asks for money"], amount: "180.00", dates: [{ date: "2026-10-30", what: "due" }],
                  cautions: ["replies go to example.org"], appointment: { day: "2026-10-02", title: "Dental checkup" },
                  more_like_it: 2 }],
    coming_up: [{ date: "2026-10-24", what: "hike", from: "Jane Doe", because: "from someone you write to" }],
    held_back: 2,
    buckets: [{ name: "money", title: "Money", count: 1, recent: [{ from: "Example Dental", at_ms: 1,
                bucket: "money", sentence: "A bill.", why: [], amount: null, dates: [] }] },
              { name: "other", title: "Other", count: 0, recent: [] }] };
  RECIPES = [
    { id: "air_quality", capability_id: "recipe.air_quality",
      description: "Today's air quality where you are.",
      inputs: [{ name: "lat", type: "number" }, { name: "lon", type: "number" }],
      get: "https://air-quality-api.open-meteo.com/v1/air-quality?latitude={lat}&longitude={lon}&current=us_aqi",
      say: "The air quality index is {current.us_aqi} right now.", enabled: true },
    { id: "transit_delays", capability_id: "recipe.transit_delays",
      description: "Whether the morning train is delayed.",
      inputs: [], get: "https://example.test/status", say: "Status: {status}", enabled: false },
  ];
  // Not yet enrolled, which is the branch that actually renders the setup card.
  SIGNIN = { password_set: false, enrolled: true, otpauth: "otpauth://totp/Endora:you?secret=AAAA", qr: "<svg xmlns='http://www.w3.org/2000/svg'><rect/></svg>" };
  // A node nobody has claimed yet — the branch that renders the setup screen rather than a
  // sign-in form for an account that does not exist.
  SIGNIN_EXISTS = false;
  // Mid-rotation, with a token on screen — the branches that only exist between two states and
  // would otherwise never be rendered by this check.
  NEW_AUTHENTICATOR = "otpauth://totp/Endora:you?secret=BBBB";
  NEW_AUTHENTICATOR_QR = "<svg xmlns='http://www.w3.org/2000/svg'><rect/></svg>";
  DEV_TOKEN = "0123456789abcdef";
  DEV_TOKEN_NOTE = "Reads only.";
  // The screen shown once, immediately after claiming — a branch that only exists between two
  // other screens and would otherwise never be rendered by this check.
  JUST_ENROLLED = "otpauth://totp/Endora:you?secret=AAAA&issuer=Endora";
  JUST_ENROLLED_QR = "<svg xmlns='http://www.w3.org/2000/svg'><rect/></svg>";
  NOTIONS = [{ id: "1", statement: "the Monday gym block gets cancelled",
               because: ["message:12", "reading:calendar.rustic"],
               settles_when: "whether next Monday's block survives",
               status: "open", created_ms: 1, last_supported_ms: 2 },
             { id: "2", statement: "a thought that came to nothing", because: ["message:3"],
               settles_when: "", status: "died", created_ms: 1, last_supported_ms: 1 }];
  REPAIRS = [{ capability: "home.HassTurnOff", target: "kitchen", attempts: 3,
               remedy: "name_the_target" }];
  TROUBLE = [{ server: "home-assistant", thing: "living room lamp", trouble: "unavailable",
               days: 4, statement: "living room lamp has not answered for 4 days" }];
  LANDING = { considered: 50, changed: 8, unchanged: 23, failed: 19, unchecked: 0,
              worst_offender: { capability: "home.HassTurnOff", times: 13 },
              in_words: "8 of 50 verified" };
  CONFIG_WRITES = [{ id: "1", at_ms: 1, server: "home-assistant", target: "light.kitchen",
                     described: "added the name kitchen main", undone: false,
                     kind: "name" }];
  ACTIVITY = [{ id: "1", at_ms: 1, text: "Checked your home" }];
  CAPS = [{ id: "home_assistant", name: "Home Assistant", description: "reads your home",
            category: "presence", enabled: true, configured: true, blocked: false,
            confirm: false, open_irreversible: false, reaches_external: true,
            reversibility: "observe", needs: "your Home Assistant URL",
            settings: [{ key: "url", label: "Home Assistant URL", secret: false, set: true }] }];
  MCP_SERVERS = { servers: [{ name: "home-assistant", transport: "http",
                              url: "http://ha.local", command: "", args: [], enabled: true,
                              auth_set: false, env_keys: [], trust_all: true,
                              reader_tool: "GetLiveContext", tools_live: 5,
                              tools: [{ name: "HassTurnOn", description: "Turns on a device",
                                        enabled: true }] }] };
  WORTH_KNOWING = { models: [{ id: "a/Qwen3-8B-GGUF", about_gb: 5, updated: "2026-07-20",
                               downloads: 900, how_to_get_it: "ollama pull …" }],
                    fits_gb: 12, asked: true };
  MCP_NEEDS = { fields: [{ key: "API_KEY", label: "API key" }], docs: "" };
  CONNECT = { kind: "caldav", form: "01ABC", step: "user", fields: [
      { name: "url", kind: "string", required: true, default: null, secret: false },
      { name: "username", kind: "string", required: true, default: null, secret: false },
      { name: "password", kind: "string", required: false, default: "", secret: true } ] };
  LAST_ACTIVITY = ["Checked your home"];
  LAST_ACTIVITY_MSG = "2";
  STEP_LIST = [{ skill: "weather", status: "running", label: "Checking the weather" }];
  DEEP_MODEL = { configured: true, key_set: true, url: "https://api.deepseek.com/v1",
                 model: "deepseek-v4-flash", escalate: false };
  `,
  context,
);

// Discovered by name, verified by calling. The regex only decides *what* to exercise — if it
// misses one, that screen is unchecked, which is a smaller failure than a checker that
// wrongly claims a broken file is fine.
const named = [...source.matchAll(/^function (view[A-Za-z]*|[a-z][A-Za-z]*Html)\(/gm)].map(
  (m) => m[1],
);

const failures = [];
for (const name of named) {
  const fn = context[name];
  if (typeof fn !== "function") {
    failures.push(`${name}: declared in the source but not defined after loading`);
    continue;
  }
  try {
    // Called with no arguments on purpose: every renderer has to cope with the empty state,
    // which is what a fresh install and a failed fetch both look like.
    const out = fn();
    if (typeof out !== "string") {
      failures.push(`${name}: returned ${typeof out}, not a string`);
    }
  } catch (e) {
    failures.push(`${name}: threw — ${e.message}`);
  }
}

// How many distinct sections one screen may stack before it stops being a screen and starts
// being a filing cabinet.
//
// A ratchet, not a target — tightened whenever a screen improves, which is the only way a
// budget stays honest. It began at six, where the worst screen was — `viewUnderstanding`
// stacks beliefs, an intention, how its actions landed, standing trouble, repairs, config
// writes and outcomes, seven unrelated things at identical visual weight. Two of those were
// added in a single week without anyone noticing what they were being added to, which is
// exactly the failure a budget catches and a review does not.
//
// Counted from the **rendered** screen rather than the source, so a section contributed by a
// nested call counts the same as one written inline — the person sees no difference.
//
// Lowering this is the point. Raising it should feel like a decision.
const MOST_SECTIONS_ON_ONE_SCREEN = 4;

/// How many form fields a screen may show before the person opens anything.
///
/// The companion to the section budget, and the one that encodes progressive disclosure:
/// Models had ten fields in view for a thing most people configure once, because the deep
/// model and the auto-tune schedule sat open beside the everyday one. Folding took it to
/// three, and this stops it drifting back.
///
/// Fields inside a `<details>` are not counted — they are opt-in and cost nothing until
/// wanted, which is exactly the distinction worth enforcing.
const MOST_FIELDS_BEFORE_OPENING_ANYTHING = 8;

// Budgets are measured on the screen **at rest** — what a person faces on arrival, not
// mid-task. The fixture above deliberately puts a setup form in flight so the crash check
// exercises that branch, and counting it here would charge the screen for fields that appear
// only because someone is already using it. Errors are checked with the richest state
// available; budgets with the plainest.
runInContext("CONNECT = null;", context);

for (const name of named.filter((n) => n.startsWith("view"))) {
  const fn = context[name];
  if (typeof fn !== "function") continue;
  let rendered = "";
  try {
    rendered = String(fn() ?? "");
  } catch {
    continue; // already reported above
  }
  // What a person faces before opening anything. Fields inside a <details> are opt-in and
  // cost nothing until wanted, which is the whole of progressive disclosure — so they are
  // not counted, and folding something is a real improvement rather than a rearrangement.
  const upFront = (rendered.replace(/<details[\s\S]*?<\/details>/g, "")
    .match(/<input|<select|<textarea/g) || []).length;
  if (upFront > MOST_FIELDS_BEFORE_OPENING_ANYTHING) {
    failures.push(
      `${name}: ${upFront} fields before opening anything ` +
        `(budget ${MOST_FIELDS_BEFORE_OPENING_ANYTHING}) — fold the ones most people never touch`,
    );
  }
  const sections = (rendered.match(/<h3/g) || []).length;
  if (sections > MOST_SECTIONS_ON_ONE_SCREEN) {
    failures.push(
      `${name}: ${sections} sections on one screen (budget ${MOST_SECTIONS_ON_ONE_SCREEN}) — ` +
        `split it rather than raising the number`,
    );
  }
}

if (failures.length) {
  console.error(`console check: ${failures.length} of ${named.length} screens are broken\n`);
  for (const f of failures) console.error("  " + f);
  process.exit(1);
}
// Every screen under Settings must say so in its breadcrumb.
//
// They drifted: four said "Home > Skills" when they were two levels down, one had no trail at
// all, and two different screens both called themselves "Preferences". Rendering the trail
// from one list fixed those; this stops the next screen being added without one, which is the
// only way the fix survives.
// Read through `runInContext`, not `context.UNDER_SETTINGS`: a `const` at the top of a VM
// script is a lexical binding rather than a property of the global, which is the same trap
// the fixture above is written the way it is to avoid.
const settingsViews = runInContext("UNDER_SETTINGS.map((p) => p.view)", context);
const missing = [];
for (const view of settingsViews) {
  let html = "";
  try {
    html = runInContext(`view${view[0].toUpperCase()}${view.slice(1)}()`, context) || "";
  } catch (e) {
    missing.push(`${view} (threw: ${e.message})`);
    continue;
  }
  if (!/Settings/.test(html)) missing.push(view);
}
if (missing.length) {
  console.error(`breadcrumbs: these screens are under Settings but do not say so: ${missing.join(", ")}`);
  process.exit(1);
}
// No request to this node may be made without going through `signed`.
//
// `/v1/chat/stream` was fetched directly with a bare content-type, so the moment the node
// started requiring a credential every message failed to send — and nothing caught it: this
// harness renders screens and makes no requests, and the Rust tests exercise the router rather
// than the browser. It took a screenshot of a red banner.
//
// This is a source check rather than an execution one, which the header above is rightly
// sceptical of — so it looks for one specific, unambiguous shape (`headers:` given an object
// literal on a `fetch`) rather than trying to understand the file. A false positive here is
// somebody writing headers a new way and being told to route them through `signed`, which is
// the right answer anyway.
const unsigned = [...source.matchAll(/fetch\(\s*["'`]([^"'`]*\/v1[^"'`]*)[\s\S]{0,200}?headers:\s*\{/g)]
  .map((m) => m[1]);
if (unsigned.length) {
  console.error(
    `requests: these reach /v1 without going through signed(): ${unsigned.join(", ")}`
  );
  process.exit(1);
}
console.log("requests: every fetch to /v1 is signed");

// The question a brand-new install actually faces.
//
// The fixture above has a location set, so the first-run branch never renders in the pass
// below — the same blind spot that let a blank console ship, arriving again. Four skills are
// silent without this answer while reporting themselves configured, so a screen that fails to
// ask is a failure that looks like working.
runInContext("DB.preferences = [];", context);
const firstRun = runInContext("viewNeedsYou()", context) || "";
if (!/Where are you based/.test(firstRun)) {
  console.error("first run: a node with no location does not ask for one");
  process.exit(1);
}
runInContext("DB.preferences = [{ id: '1', text: 'based in Boston, MA' }];", context);
const settled = runInContext("viewNeedsYou()", context) || "";
if (/Where are you based/.test(settled)) {
  console.error("first run: it keeps asking after being told");
  process.exit(1);
}
console.log("first run: asks where you are, once, and stops when told");


// What the browser actually sends.
//
// The gap that let the chat break entirely: 702 Rust tests exercise the router, 26 screens
// render here, 9 invariants run against the deployed node — and **not one of them made a
// request the way a browser does**. `/v1/chat/stream` went out unsigned for hours, and the
// only thing that noticed was a person looking at a red banner.
//
// So this drives the real send path with a recording `fetch`. No network, no browser, no new
// dependency: the console's own code is already running in this context, so calling it is
// free. It costs milliseconds, which is the only reason it is in the fast tier at all.
const sent = [];
runInContext("localStorage.getItem = () => 'a-test-token';", context);
context.fetch = (path, opts = {}) => {
  sent.push({ path, headers: opts.headers || {}, method: opts.method || "GET", body: opts.body });
  // A streaming reply the drain loop can read to completion without hanging.
  return Promise.resolve({
    ok: true,
    status: 200,
    json: async () => ({}),
    body: {
      getReader: () => ({
        read: async () => ({ value: undefined, done: true }),
      }),
    },
  });
};
runInContext("CHAT_QUEUE = ['does the kitchen light work?']; CHAT_STREAMING = false;", context);
await runInContext("drainChat()", context);

const chat = sent.filter((r) => String(r.path).includes("/v1/chat/stream"));
if (!chat.length) {
  console.error("requests: sending a message made no request to /v1/chat/stream at all");
  process.exit(1);
}
const unsignedSends = chat.filter((r) => !r.headers.authorization);
if (!JSON.parse(chat[0].body).request_id) {
  console.error("requests: chat did not send an idempotency key for durable work");
  process.exit(1);
}
if (unsignedSends.length) {
  console.error(
    "requests: sending a message went out without a credential — the node will refuse it, " +
      "which is exactly how the chat broke"
  );
  process.exit(1);
}
console.log(`requests: sending a message signs its ${chat.length} request(s)`);
await runInContext("CHAT_QUEUE = [LAST_CHAT_REQUEST]; drainChat()", context);
await runInContext("CHAT_QUEUE = ['does the kitchen light work?']; drainChat()", context);
const keyedChat = sent.filter((r) => String(r.path).includes("/v1/chat/stream"));
const keys = keyedChat.map((r) => JSON.parse(r.body).request_id);
if (keys.length !== 3 || keys[0] !== keys[1] || keys[0] === keys[2]) {
  console.error("requests: a retry must reuse its key and a fresh ask must use a new key");
  process.exit(1);
}

// Choosing a server from the catalogue has to show you the form it filled in.
//
// Both halves were right on their own. The add form was folded into a `<details>`, because
// almost nobody arrives at that screen to type a launch command out by hand. And "Use" had
// always filled the fields in and scrolled to them.
//
// Inside a closed `<details>` there is nothing to scroll to. The toast said "review and add
// it", the screen did not move, and the field asking for the API key was behind a fold the
// person had no reason to suspect. Two correct changes and one seam, which is where these
// live: neither half is wrong in the file it is in.
//
// So this asserts the seam and not either half — after choosing, the form is open and filled.
{
  const fold = { open: false, parentElement: null };
  const nameField = { ...element(), closest: (sel) => (sel === "details" ? fold : null) };
  const realGet = context.document.getElementById;
  context.document.getElementById = (id) => (id === "mcp-name" ? nameField : element());
  runInContext(
    `MCP_CATALOG = [{ id: "io.github.brave/brave-search-mcp-server", name: "Brave Search",
       transport: "stdio", command: "npx", args: ["-y", "@brave/brave-search-mcp-server"],
       fields: [{ key: "BRAVE_API_KEY", label: "Brave API key", secret: true, target: "env" }],
       docs: "" }];
     mcpUseCatalog(0);`,
    context,
  );
  context.document.getElementById = realGet;
  if (!nameField.value) {
    console.error("catalogue: choosing a server did not fill the form in at all");
    process.exit(1);
  }
  if (!fold.open) {
    console.error(
      "catalogue: choosing a server filled a form that is still folded shut — the person " +
        "sees a toast and no form, which is how this broke",
    );
    process.exit(1);
  }
  console.log("catalogue: choosing a server opens the form it filled in");
}

// A turn that never answered must not strand the person.
//
// Live: a reply streamed, flashed, and vanished — the turn failed after the tokens went out
// and before anything was saved. The last stored message was the person's, so the screen
// rendered a thinking bubble, and it stayed through every reload, because the only thing the
// console could ask about "is it still working?" was itself, and the tab that had been
// streaming was gone. The butler could not be spoken to again.
//
// Three states, one of which is not waiting for anything.
{
  const chatWith = (last) => {
    runInContext(
      `CHAT_MSGS = [{ id: "1", role: "user", text: ${JSON.stringify(last)},
                      at_ms: Date.now() }];
       CHAT_STREAMING = false; CHAT_STOPPED = false;
       CHAT_DAYS = []; CHAT_DAY = null;`,
      context,
    );
    return runInContext("viewChat()", context) || "";
  };

  runInContext("NODE_BUSY = true;", context);
  const working = chatWith("any events this week?");
  if (!/class="dots"/.test(working)) {
    console.error("chat: the node is taking a turn and the screen does not show it thinking");
    process.exit(1);
  }

  runInContext("NODE_BUSY = false;", context);
  const stranded = chatWith("any events this week?");
  if (/class="dots"/.test(stranded)) {
    console.error(
      "chat: a message with no reply and nothing running still shows a thinking bubble — " +
        "that is the state a person cannot get out of by reloading",
    );
    process.exit(1);
  }
  if (!/chat:retry/.test(stranded)) {
    console.error("chat: a turn that never answered offers no way to ask again");
    process.exit(1);
  }
  console.log("chat: an unanswered turn says so, and can be asked again");
}

// What is sent to the model is visible: a ring that fills with the window, an inspector that
// lists what is in it (and which of it Ask deep also sees), and a divider where a new topic
// began. Text inside it is text, never markup.
{
  runInContext(`CTX = { window_tokens: 16384, total_tokens: 12000, context_from_ms: 0, estimate: true,
    sections: [
      { key: "skills", label: "Skill definitions", tokens: 8000, count: 2, to_deep: true, note: "",
        items: [{ text: "web_search — <img src=x onerror=alert(1)>", tokens: 600 }] },
      { key: "recent", label: "Recent messages", tokens: 4000, count: 1, to_deep: false, note: "", items: [] },
      { key: "held_back", label: "Held back until needed", tokens: 0, count: 3, to_deep: false, note: "", items: [] },
    ] };
    CHAT_MSGS = [{ id: "1", role: "user", text: "old", at_ms: 100 }, { id: "2", role: "butler", text: "old answer", at_ms: 200 },
                 { id: "3", role: "user", text: "fresh", at_ms: 900 }];
    CHAT_STREAMING = false; CHAT_STOPPED = false; CHAT_DAYS = []; CHAT_DAY = null; NODE_BUSY = false;`, context);
  const view = runInContext("viewChat()", context) || "";
  if (!view.includes('id="ctx-ring"') || !view.includes("ring-warn")) {
    console.error("chat: the context ring is missing, or is not amber at 73% of the window");
    process.exit(1);
  }
  if (!view.includes("Skill definitions") || !view.includes("Ask deep sees this") || !view.includes("not sent")) {
    console.error("chat: the inspector does not say what is sent and what Ask deep sees");
    process.exit(1);
  }
  if (view.includes("<img src=x")) {
    console.error("chat: text inside the inspector became markup");
    process.exit(1);
  }
  runInContext("CTX.context_from_ms = 500;", context);
  const divided = runInContext("viewChat()", context) || "";
  if (!divided.includes("topic-divider") || divided.indexOf("topic-divider") < divided.indexOf("old answer")
    || divided.indexOf("topic-divider") > divided.indexOf("fresh")) {
    console.error("chat: the new-topic divider is not between the old messages and the new ones");
    process.exit(1);
  }
  runInContext("CTX = null;", context);
  console.log("chat: the window ring, the inspector and the new-topic divider render");
}

// What the deep model costs (ADR 0091): the provider's balance when it has one, a warning when
// it runs low, what Endora's calls have used, and dollars only once prices are known. Nothing a
// provider says is trusted as markup.
{
  runInContext(`DEEP_MODEL = { configured: true, url: "https://api.deepseek.com/v1", model: "m", key_set: true };
    DEEP_BALANCE = { supported: true, available: true,
      balances: [{ currency: "USD", total: "0.42", granted: "0.00", topped_up: "<b>0.42</b>" }] };
    DEEP_USAGE = { today: { calls: 2, prompt_tokens: 3000, completion_tokens: 500, cached_tokens: 0, cost_usd: null },
      month: { calls: 9, prompt_tokens: 30000, completion_tokens: 8200, cached_tokens: 0, cost_usd: null },
      recent: [], prices: { input: null, output: null, cached: null } };`, context);
  const low = runInContext("deepSpendHtml()", context);
  if (!low.includes("$0.42") || !low.includes("running low") || !low.includes("spend-row warn")) {
    console.error(`deep spend: a balance under a dollar is not shown as low: ${low}`);
    process.exit(1);
  }
  if (low.includes("<b>0.42</b>")) {
    console.error("deep spend: text from the provider became markup");
    process.exit(1);
  }
  if (!low.includes("38k tokens") || !low.includes("see dollars") || low.includes("today")) {
    console.error(`deep spend: usage or the prompt to set prices is missing: ${low}`);
    process.exit(1);
  }
  runInContext(`DEEP_USAGE.month.cost_usd = 0.0213; DEEP_USAGE.today.cost_usd = 0.004; DEEP_BALANCE = { supported: false };`, context);
  const priced = runInContext("deepSpendHtml()", context);
  if (!priced.includes("Spent this month") || !priced.includes("~$0.02") || priced.includes("Balance") || priced.includes("see dollars")) {
    console.error(`deep spend: a priced month, or a provider with no balance, renders wrongly: ${priced}`);
    process.exit(1);
  }
  runInContext("DEEP_MODEL = { configured: false }; DEEP_USAGE = null; DEEP_BALANCE = null;", context);
  if (runInContext("deepSpendHtml()", context) !== "") {
    console.error("deep spend: something is shown when there is no deep model");
    process.exit(1);
  }
  console.log("deep spend: balance, low warning, usage and prices render, and nothing without a deep model");
}

// Butler prose renders lightly, and nothing it carries can become markup.
//
// The brief's news section hands the chat 200-character URLs; rendered raw they
// punched every bubble wide open, and rendered carelessly they would be an
// injection surface — a headline is a third party's sentence. So both properties
// are held here: a bare URL becomes an anchor shown as its host, and a script tag
// in the text stays text.
{
  const linked = runInContext(
    "rich('1. Story — Pub (https://news.google.com/rss/articles/CBMi?oc=5)')",
    context,
  );
  if (!/<a href="https:\/\/news\.google\.com\/rss\/articles\/CBMi\?oc=5"/.test(linked)
    || !/>news\.google\.com<\/a>/.test(linked)) {
    console.error(`chat: a bare URL did not render as a host-named link: ${linked}`);
    process.exit(1);
  }
  const hostile = runInContext(
    "rich('breaking: <script>alert(1)</script> **bold** stays')",
    context,
  );
  if (hostile.includes("<script>")) {
    console.error(`chat: third-party text became markup: ${hostile}`);
    process.exit(1);
  }
  if (!hostile.includes("<b>bold</b>")) {
    console.error(`chat: the light markdown did not render: ${hostile}`);
    process.exit(1);
  }
  const bullets = runInContext("rich('- jane is not home\\n- 2 things on your list')", context);
  if (!bullets.includes("<ul><li>jane is not home</li><li>2 things on your list</li></ul>")) {
    console.error(`chat: dash bullets did not render as a list: ${bullets}`);
    process.exit(1);
  }
  const shape = (src) => runInContext(`rich(${JSON.stringify(src)})`, context);
  const numbered = shape("Next three:\n1. Mon\n2. Tue\n3. Wed\n\nThat is all.");
  if (!numbered.includes("<ol><li>Mon</li><li>Tue</li><li>Wed</li></ol>") || !numbered.includes("<p>That is all.</p>")) {
    console.error(`chat: a numbered list or its following paragraph is wrong: ${numbered}`);
    process.exit(1);
  }
  const fenced = shape("Run:\n```sh\necho **not bold** <b>x</b>\n```\nDone");
  if (!fenced.includes('<pre class="code"><code>echo **not bold** &lt;b&gt;x&lt;/b&gt;</code></pre>') || !fenced.includes("<p>Done</p>")) {
    console.error(`chat: a code block was dressed, unescaped or lost: ${fenced}`);
    process.exit(1);
  }
  const halfway = shape("Here:\n```\nline one");
  if (!halfway.includes('<pre class="code"><code>line one</code></pre>')) {
    console.error(`chat: a code block still open while streaming was not shown as code: ${halfway}`);
    process.exit(1);
  }
  const mixed = shape("## Plan\n\nA *light* touch, `a**b**c` and 2 * 3 * 4.\n> quoted\n---");
  if (!mixed.includes('<p class="h">Plan</p>') || !mixed.includes("<i>light</i>")
    || !mixed.includes("<code>a**b**c</code>") || !mixed.includes("2 * 3 * 4")
    || !mixed.includes("<blockquote>quoted</blockquote>") || !mixed.includes("<hr>")) {
    console.error(`chat: heading, emphasis, code span, arithmetic, quote or rule is wrong: ${mixed}`);
    process.exit(1);
  }
  const snuck = shape("- [x](https://e.com) <img src=x onerror=alert(1)>\n```\n<script>1</script>\n```");
  if (/<img|<script/.test(snuck)) {
    console.error(`chat: third-party text became markup inside a list or a code block: ${snuck}`);
    process.exit(1);
  }
  console.log("chat: prose renders as markdown — lists, code, quotes, links — and hostile text stays text");
}

// Every honest research state renders, terminal work cannot be cancelled, the report
// can be downloaded, and assignment text stays text.
{
  runInContext(`RESEARCH = { items: ["queued", "running", "retry_wait", "delivered", "blocked",
    "budget_exhausted", "cancelled", "failed"].map((status, index) => ({
      id: String(100 + index), assignment: index === 0 ? "<img src=x onerror=alert(1)>" : "Compare backups",
      status, phase: "gathering", reason: status === "blocked" ? "A source could not be read." : "",
      budget: { model_calls_used: 2, model_calls_limit: 16 },
      report_available: status === "delivered", partial: status === "delivered",
      report_markdown: status === "delivered" ? "# Report\\n\\n## Sources\\n\\n- [S1](https://example.com)" : null,
    })) };`, context);
  const research = runInContext("viewResearch()", context);
  for (const status of ["queued", "running", "retry wait", "delivered", "blocked",
    "budget exhausted", "cancelled", "failed"]) {
    if (!research.includes(status)) {
      console.error(`research: ${status} has no visible state`);
      process.exit(1);
    }
  }
  if (research.includes("<img src=x")) {
    console.error("research: assignment text became markup");
    process.exit(1);
  }
  if (!/research:download:103/.test(research) || !/research:cancel:100/.test(research)) {
    console.error("research: delivery or cancellation control is missing");
    process.exit(1);
  }
  if (!research.includes("Partial report saved")) {
    console.error("research: a partial report is not labelled partial");
    process.exit(1);
  }
  console.log("research: every lifecycle state renders safely with its valid controls");
}

// Work Endora does on its own is visible while it runs, can be stopped, and never just
// vanishes when it ends (ADR 0083). A goal that settled as partial used to leave no trace
// anywhere in the console: stored, finished, and invisible.
{
  const goal = (key, objective, status, extra = {}) => ({
    request_key: key, objective, status, phase: "gathering", next_step: "", reason: "",
    evidence: [], outcome: null, updated_ms: 1000, ...extra,
  });
  context.__goals = [
    goal("chat:abc 1", "Find <b>three</b> games", "running", {
      next_step: "web_fetch", evidence: [{ step: "read", output: "one" }] }),
    goal("k-complete", "Check the bus", "complete", { outcome: "The 41 arrives at 14:05.", updated_ms: 6000 }),
    goal("k-partial", "Compare backups", "partial", {
      outcome: "Here is what I found on: Compare backups\n- Product A costs 10 dollars\nI couldn't confirm this fully answers your request.", updated_ms: 5000 }),
    goal("k-blocked", "Read the manual", "blocked", { outcome: "I'm blocked on: Read the manual", updated_ms: 4000 }),
    goal("k-failed", "Water the plants", "failed", { outcome: "I couldn't do: Water the plants", updated_ms: 3000 }),
    goal("k-cancelled", "Plan the trip", "cancelled", { outcome: "You cancelled: Plan the trip", updated_ms: 2000 }),
  ];
  runInContext("globalThis.__savedDb = DB; DB = Object.assign({ messages: [] }, DB, { goals: __goals });", context);
  const goals = runInContext("viewGoals()", context);
  for (const label of ["working", "done", "partly done", "blocked", "could not do it", "cancelled"]) {
    if (!goals.toLowerCase().includes(label)) {
      console.error(`goals: ${label} has no visible state`);
      process.exit(1);
    }
  }
  if (!goals.includes("1 finding") || !goals.includes("gathering")) {
    console.error("goals: running work does not say where it is or what it has found");
    process.exit(1);
  }
  if (goals.includes("<b>three</b>") || !goals.includes("&lt;b&gt;three")) {
    console.error("goals: an objective became markup");
    process.exit(1);
  }
  for (const outcome of ["14:05", "Here is what I found", "couldn&#39;t confirm", "blocked on", "couldn&#39;t do", "You cancelled"]) {
    if (!goals.includes(outcome) && !goals.includes(outcome.replace("'", "&#39;"))) {
      console.error(`goals: an ended goal hid its outcome (${outcome})`);
      process.exit(1);
    }
  }
  const stops = goals.match(/data-act="goal:cancel:[^"]*"/g) || [];
  if (stops.length !== 1 || !stops[0].includes("goal:cancel:chat%3Aabc%201")) {
    console.error(`goals: only running work can be stopped, by its encoded key: ${stops.join(" ")}`);
    process.exit(1);
  }
  if (!runInContext("viewChat()", context).includes("Endora's own work")) {
    console.error("goals: the chat does not show the goals strip");
    process.exit(1);
  }

  // Stopping one reaches the node with the key intact (a `:` would break the dispatcher).
  context.__realReload = runInContext("reload", context);
  runInContext("reload = async () => {}", context);
  const before = sent.length;
  await runInContext('dispatch("goal:cancel:chat%3Aabc%201")', context);
  const stop = sent.slice(before).find((r) => String(r.path).includes("/cancel"));
  if (!stop || stop.method !== "POST" || stop.path !== "/v1/goals/chat%3Aabc%201/cancel") {
    console.error(`goals: Stop did not POST the goal's cancel route: ${JSON.stringify(stop)}`);
    process.exit(1);
  }
  if (!stop.headers.authorization) {
    console.error("goals: Stop went out without a credential");
    process.exit(1);
  }
  runInContext("reload = __realReload; DB = __savedDb;", context);
  console.log("goals: running work shows progress and can be stopped; ended work keeps its outcome");
}

// The recipe authoring loop shows what it actually fetched, and every field it
// offers is one you can tap into the sentence (ADR 0071).
//
// The failure this guards against is subtle and would look fine: a chip whose
// `data-act` verb nobody handles renders perfectly and does nothing when tapped,
// which reads as a dead app rather than a missing branch. The verb scan below
// catches an unhandled verb; this catches the chip losing its action entirely.
{
  const trial = runInContext(
    `recipeTrialHtml({ ok: true, url: "https://x.test/?lat=1",
       fields: [{ path: "current.us_aqi", sample: "42" }],
       preview: "The index is 42.", preview_error: null, skipped_a_list: true })`,
    context,
  );
  if (!/data-act="recipe:pick:current\.us_aqi"/.test(trial)) {
    console.error(`recipes: a trial's field is not tappable into the sentence: ${trial}`);
    process.exit(1);
  }
  if (!trial.includes("The index is 42.") || !/list/i.test(trial)) {
    console.error(`recipes: a trial hid its preview or its skipped list: ${trial}`);
    process.exit(1);
  }
  // A failed trial says why, rather than rendering an empty success.
  const failed = runInContext(`recipeTrialHtml({ ok: false, error: "that address didn't answer" })`, context);
  if (!failed.includes("that address didn&#39;t answer")) {
    console.error(`recipes: a failed trial did not say why: ${failed}`);
    process.exit(1);
  }
  console.log("recipes: a trial shows what it fetched, and its fields are tappable");

  // Every starter must be complete and internally consistent, because a starter
  // that does not work teaches the person the whole feature is broken. Each was
  // run against its real API before shipping; what a build check can still hold
  // is that none has been edited into an impossible shape since.
  const starters = runInContext("JSON.stringify(RECIPE_STARTERS)", context);
  for (const s of JSON.parse(starters)) {
    for (const field of ["id", "description", "get", "say", "note"]) {
      if (!s[field]) {
        console.error(`recipes: the "${s.id || "?"}" starter has no ${field}`);
        process.exit(1);
      }
    }
    if (!/^https:\/\//.test(s.get)) {
      console.error(`recipes: the "${s.id}" starter is not https: ${s.get}`);
      process.exit(1);
    }
    // A starter declares no inputs, so its address must carry no {slot} — one
    // would be unfillable and the recipe would refuse to save.
    if (/\{[^}]+\}/.test(s.get)) {
      console.error(`recipes: the "${s.id}" starter's address has a slot but no inputs: ${s.get}`);
      process.exit(1);
    }
    // …and its sentence must read something, or there is nothing to say.
    if (!/\{[^}]+\}/.test(s.say)) {
      console.error(`recipes: the "${s.id}" starter's sentence reads no field: ${s.say}`);
      process.exit(1);
    }
  }
  console.log(`recipes: ${JSON.parse(starters).length} starters are complete and slot-free`);
}

// Layout is still not covered, and this is the honest note about why.
//
// The shape that has broken twice on a phone — a `.row` holding a text block and a pile of
// buttons, giving the buttons their width and the text about eight characters — was worth a
// check and does not survive one. This file renders markup and cannot lay it out, so the
// only thing available is the shape in the source. The distance between a text block and its
// buttons runs from fifty characters to nearly three thousand depending on how much that
// block says, and a window wide enough for the real case swept in three unrelated rows that
// were perfectly fine.
//
// A checker that lies is worse than no checker — the sentence at the top of this file — and
// three false alarms on correct code is a check somebody turns off. What would actually
// measure this is a narrow viewport in a real browser, which is a slower tier than this one
// is allowed to be. Until that exists, this is caught by a person with a phone, and it is
// better to say so here than to pretend otherwise.

// Every button leads somewhere, and every request has somewhere to land.
//
// Two seams that nothing watched, both of which have broken here before. A `data-act` the
// dispatcher does not handle is a button that silently does nothing — the person taps and
// the screen sits there, which reads as a hung app rather than a missing branch. And a
// `/v1` path the node does not route is a 404 arriving as "couldn't reach the node", sending
// the diagnosis in exactly the wrong direction.
//
// Source checks, which the header of this file is rightly sceptical of — so both look for
// one unambiguous shape rather than trying to understand the file, and both are wrong only
// in the direction of asking somebody to look.
{
  const verbs = new Set();
  for (const m of source.matchAll(/verb === "([a-z]+)"/g)) verbs.add(m[1]);
  // Actions are `verb:noun[:rest]`; the dispatcher branches on the verb, so an unknown verb
  // is the shape that cannot possibly be handled.
  const acted = [...source.matchAll(/data-act="\$?\{?["']?([a-z]+):/g)].map((m) => m[1]);
  const unhandled = [...new Set(acted)].filter((v) => !verbs.has(v));
  if (unhandled.length) {
    console.error(
      `actions: these buttons name a verb nothing handles, so tapping them does nothing: ${unhandled.join(", ")}`,
    );
    process.exit(1);
  }
  console.log(`actions: ${verbs.size} verbs handled, every button leads somewhere`);
}

{
  const routerPath = resolve(here, "../app/node/src/api.rs");
  const router = readFileSync(routerPath, "utf8");
  // `/v1/mcp/servers/{name}/test` in the router, `/v1/mcp/servers/brave/test` in the
  // console — compare on the shape with every path segment blanked, so a real id and a
  // placeholder read the same.
  const shapeOf = (p) =>
    p
      .split("/")
      .map((seg) => (/^\{.*\}$/.test(seg) || seg.includes("$") || seg === "" ? "*" : seg))
      .join("/");
  const routed = new Set(
    [...router.matchAll(/"(\/v1\/[^"]*)"/g)].map((m) => shapeOf(m[1])),
  );
  const called = [...source.matchAll(/api\(\s*["'][A-Z]+["']\s*,\s*["'`](\/v1\/[^"'`?]*)/g)]
    .map((m) => shapeOf(m[1]));
  const missing = [...new Set(called)].filter((p) => !routed.has(p));
  if (missing.length) {
    console.error(
      `requests: the console calls these and the node routes nothing like them: ${missing.join(", ")}`,
    );
    process.exit(1);
  }
  console.log(`requests: every /v1 path the console calls is routed`);
}

console.log(`breadcrumbs: ${settingsViews.length} screens under Settings all say so`);

console.log(`console check: ${named.length} screens render`);
