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

import { readFileSync } from "node:fs";
import { createContext, runInContext } from "node:vm";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const appJs = resolve(here, "../app/node/src/web/app.js");
const source = readFileSync(appJs, "utf8");

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
  // Open, with evidence — an empty fixture once missed the very bug it was written for, and a
  // notion carrying no evidence would skip the one line here that can actually throw.
  // One offered service already set up, and one the person added themselves — both branches
  // of the Connect screen, which previously rendered only the "nothing is connected" case.
  CONNECTED = ["caldav", "hue", "tplink"];
  // One enabled with a declared input (exercises the "needs" line and the "On"
  // pill) and one still off (the "Off" pill and its own button label) — both
  // branches of the enable toggle, which an empty list would never reach.
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
  sent.push({ path, headers: opts.headers || {}, method: opts.method || "GET" });
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
if (unsignedSends.length) {
  console.error(
    "requests: sending a message went out without a credential — the node will refuse it, " +
      "which is exactly how the chat broke"
  );
  process.exit(1);
}
console.log(`requests: sending a message signs its ${chat.length} request(s)`);

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
  if (!bullets.includes("• jane is not home")) {
    console.error(`chat: a dash bullet did not render as a bullet: ${bullets}`);
    process.exit(1);
  }
  console.log("chat: prose renders links, bullets and bold — and hostile text stays text");
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
