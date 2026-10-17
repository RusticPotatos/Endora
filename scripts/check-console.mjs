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
  BRAIN = { memory: "# Memory\\n\\n## What you told me\\n\\n- Based in: New York\\n\\n## What I know best\\n\\n- Busy Monday to Friday\\n",
            pages: [{ path: "hobbies/watches", title: "Watches", line: "Collects dive watches" }] };
  BRAIN_PAGE = { path: "hobbies/watches",
                 text: "# Watches\\n\\nCollects <b>dive</b> watches.\\n\\n## Timeline\\n\\n- 2026-10-16: Collects dive watches\\n",
                 history: [{ at: "2026-10-16T12:00:00-04:00", why: "noted from chat" }] };
  OUTCOMES = [{ id: "42", capability: "home.HassTurnOff", claim: "done",
                observation: "still on", observed: true, reaction: null, at_ms: 1 }];
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
  // Every model role, one on a cloud endpoint, and a server that gives less context than
  // Endora assumes, so every branch of the AI screen renders.
  AI = { context_tokens: 16384, mail_reader_model: null, roles: [
    { id: "chat", name: "Conversation", does: "Answers you.", model: "qwen2.5:7b", endpoint: "ollama:11434",
      local: true, on: true, change: "models", sees: "Your messages.",
      runtime: { processor: "graphics card", memory_gb: 5.6, context: 4096, size: "7.6B, Q4_K_M" } },
    { id: "deep", name: "Deep model", does: "Hard questions.", model: "big-model", endpoint: "api.example.com",
      local: false, on: true, change: "deep", sees: "What you send it.", runtime: null,
      calls_this_month: 3, cost_usd_this_month: 0.42 },
    { id: "mail_reader", name: "Reading your mail", does: "One email at a time.", model: "qwen2.5:7b",
      endpoint: "ollama:11434", local: true, on: true, change: "mail_reader", sees: "One email.", runtime: null },
    { id: "vision", name: "Looking at images", does: "Images.", model: "", endpoint: "ollama:11434",
      local: true, on: false, change: "vision", sees: "The image.", runtime: null },
    { id: "speech", name: "Hearing you", does: "Speech.", model: "", endpoint: "", local: false, on: true,
      change: null, sees: "Nothing: your browser's own speech recognition is used.", runtime: null }] };
  // A Spotify sign-in started elsewhere, waiting to be finished here.
  SPOTIFY = { state: "waiting", detail: "Waiting for you to approve Endora on Spotify", client_id_set: true,
    sign_in_url: "https://accounts.spotify.com/authorize?client_id=example", redirect: "http://127.0.0.1:8888/callback",
    started_ms: 1, account: null };
  // The home lab, media and smart home as the node's typed endpoints give them.
  LAB = {
    dashboard: { stats: ["CPU 7%", "Free 13.8 TB"], alerts: ["DiskLow"], groups: [
      { name: "Media", services: [
        { name: "Overseerr", state: "running", id: "${"1".repeat(64)}", detail: "0 pending" },
        { name: "<b>Plex</b>", state: "unhealthy" }] }] },
    containers: { summary: "CPU 7% | Mem 54%", items: [
      { id: "${"2".repeat(64)}", label: "endora-node", detail: "running Up 4 minutes", protected: true },
      { id: "${"3".repeat(64)}", label: "sonarr", detail: "running Up 2 days", protected: false },
      { id: "${"4".repeat(64)}", label: "radarr", detail: "exited Exited (0) 1 hour ago", protected: false }] },
    logs: { summary: "line one\\n<script>no</script>", items: [] },
    jobs: { summary: "Allowlisted Ansible jobs", items: [
      { id: "check", label: "Check the NAS", detail: "complete", latest_id: "web-abc" },
      { id: "apply", label: "Apply the NAS", detail: "not run", latest_id: null }] },
    job: { summary: "PLAY RECAP ok=12", state: "complete", items: [] },
    mediaQuery: "dune",
    found: { summary: "", items: [
      { id: "438631", label: "Dune (2021)", detail: "movie | Availability: not requested", media_type: "movie" },
      { id: "90228", label: "Dune: Prophecy (2024)", detail: "tv | Availability: available", media_type: "tv" }] },
    requests: { summary: "Recent media requests", items: [{ id: "1", label: "Fall (2022)", detail: "movie | completed", media_type: "movie" }] },
    favorites: { summary: "Home favorites", items: [
      { id: "light.kitchen", label: "Kitchen", detail: "on " },
      { id: "scene.movie", label: "Movie night", detail: "scening " },
      { id: "sensor.porch", label: "Porch", detail: "21 °C" }] },
    catalogQuery: "",
    catalog: { summary: "", items: [{ id: "light.kitchen", label: "Kitchen", detail: "on", favorite: true },
      { id: "switch.fan", label: "Fan", detail: "off", favorite: false }] },
  };
  MUSIC = { now: { is_playing: true, device: { name: "Living room", volume_percent: 40 },
      item: { name: "A <i>song</i>", artists: [{ name: "Jane Doe" }], album: { images: [{ url: "https://i.scdn.co/image/example" }] } } },
    devices: [{ id: "abc123", name: "Living room", is_active: true }, { id: "def456", name: "Handheld", is_active: false }],
    found: [{ uri: "spotify:track:abc", name: "Found", kind: "track", who: "Jane Doe" }],
    lists: [{ uri: "spotify:playlist:def", name: "Mine", kind: "playlist", who: "John Doe" }], query: "x", error: "" };
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
  REPAIRS = [{ capability: "home.HassTurnOff", target: "kitchen", attempts: 3,
               remedy: "name_the_target" }];
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
            settings: [{ key: "url", label: "Home Assistant URL", secret: false, set: true }] },
          // An entry that only holds settings for the handheld: "Set up", and no on/off.
          { id: "terminal-portainer", name: "Portainer", description: "Portainer on the handheld",
            category: "information", enabled: true, configured: false, settings_only: true,
            settings_complete: true, blocked: false, confirm: false, open_irreversible: false,
            reaches_external: true, reversibility: "observe", needs: "set up here",
            settings: [{ key: "origin", label: "Portainer address", secret: false, set: true }] },
          // One not set up yet, and a skill that rides it: "needs a connection", not "needs setup".
          { id: "homelab-docker", name: "Docker", description: "Docker on the handheld",
            category: "information", enabled: true, configured: false, settings_only: true,
            settings_complete: false, blocked: false, confirm: false, open_irreversible: false,
            reaches_external: true, reversibility: "observe", needs: "set up here",
            settings: [{ key: "origin", label: "Docker observer address", secret: false, set: false }] },
          { id: "containers", name: "What the containers are doing", description: "Which containers run. Read-only.",
            category: "information", enabled: true, configured: false, usable: false,
            borrows_from: "homelab-docker", blocked: false, confirm: false, open_irreversible: false,
            reaches_external: true, reversibility: "observe", needs: "set up Docker under Settings → Connections",
            settings: [] }];
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
// Settings holds settings. The record of what Endora did, the two halves of "about you", and
// recipes were each their own entry once, and the list grew one row per feature.
{
  const settings = runInContext("viewSettings()", context);
  for (const gone of ["go:audit", "go:prefs", "go:recipes"]) {
    if (settings.includes(gone)) {
      console.error(`settings: ${gone} is back in the Settings list`);
      process.exit(1);
    }
  }
  // Export and delete are on Your data, not loose at the foot of Settings.
  const data = runInContext("viewData()", context);
  if (settings.includes('data-act="purge"') || !data.includes('data-act="purge"') || !data.includes('data-act="export"')) {
    console.error("data: export and delete are not on Your data");
    process.exit(1);
  }
  for (const here of ["go:connections", "go:skills", "go:permissions", "go:data"]) {
    if (!settings.includes(here)) {
      console.error(`settings: ${here} is missing from the Settings list`);
      process.exit(1);
    }
  }
  // Skills is a list: no forms on it, no entries that only hold settings, and each row opens
  // its own screen.
  // A line written for people is what the screen shows, not the model's instructions.
  runInContext('CAPS.find((c) => c.id === "home_assistant").summary = "Reads your home, for people."', context);
  const skills = runInContext("viewSkills()", context);
  if (!skills.includes("Reads your home, for people.") || skills.includes("reads your home<")) {
    console.error("skills: the list shows the model's description rather than the line for people");
    process.exit(1);
  }
  if (skills.includes("skillcfg:") || skills.includes("go:skill:terminal-portainer")
      || !skills.includes("go:skill:home_assistant") || !skills.includes("go:recipes")
      || !skills.includes("go:connections")) {
    console.error("skills: the list grew a form or a connection, or lost a way somewhere");
    process.exit(1);
  }
  if (!skills.includes("Needs a connection")) {
    console.error("skills: a skill whose connection is not set up says only that it needs setup");
    process.exit(1);
  }
  runInContext("globalThis.__nav = NAV", context);
  const one = runInContext('NAV = { v: "skill", id: "home_assistant" }; viewSkill()', context);
  if (!one.includes("skillcfg:home_assistant") || !one.includes("skill:enable:home_assistant")) {
    console.error("skill: a skill's own screen lost its settings or its switch");
    process.exit(1);
  }
  const rider = runInContext('NAV = { v: "skill", id: "containers" }; viewSkill()', context);
  if (!rider.includes("Uses your Docker connection") || !rider.includes("go:connections")) {
    console.error("skill: a skill that rides a connection does not say which, or where");
    process.exit(1);
  }
  // Connections list the home lab's services, set up or not; each opens to its own settings,
  // with no switch that does nothing.
  const connections = runInContext("viewConnections()", context);
  if (!connections.includes("go:skill:terminal-portainer") || connections.includes("skillcfg:")
      || !connections.includes("Set up") || !connections.includes("mcp:add")) {
    console.error("connections: a home-lab service is missing, its form is open on the list, or tool servers are missing");
    process.exit(1);
  }
  const lab = runInContext('NAV = { v: "skill", id: "terminal-portainer" }; viewSkill()', context);
  if (!lab.includes("skillcfg:terminal-portainer") || lab.includes("skill:enable:terminal-portainer")
      || lab.includes("Needs setup") || !lab.includes("go:connections")) {
    console.error("connections: a home-lab service's own screen says it needs setup, offers an on/off, or does not lead back");
    process.exit(1);
  }
  // Tool servers are rows too, each opening to its own screen with its tools and the form
  // its Edit fills; the Home Assistant wizard is folded until wanted.
  if (!connections.includes("go:toolserver:home-assistant") || connections.includes("mcp:trust:")
      || connections.indexOf("<details") > connections.indexOf("connect:start")) {
    console.error("connections: a tool server's card is open on the list, or the wizard is not folded");
    process.exit(1);
  }
  const server = runInContext('NAV = { v: "toolserver", id: "home-assistant" }; viewToolServer()', context);
  if (!server.includes("mcp:trust:home-assistant") || !server.includes("mcp-name")
      || !server.includes("go:connections")) {
    console.error("connections: a tool server's own screen lost its tools, its edit form, or the way back");
    process.exit(1);
  }
  // The AI screen names every role, where it runs and what it sees, offers the one choice it
  // holds, and warns when the model server gives less context than Endora assumes. Dictation
  // is the browser's, so it says so and offers nothing to change.
  const ai = runInContext("viewModels()", context);
  for (const needed of ["Reading your mail", "Hearing you", "<b>your browser</b>", "on your network", ">cloud<",
    "ai:mailreader", "go:skill:image_review", "long turns lose their beginning", "$0.42"]) {
    if (!ai.includes(needed)) {
      console.error(`ai: the AI screen is missing ${needed}`);
      process.exit(1);
    }
  }
  if ((ai.match(/Change it below/g) || []).length !== 2 || ai.includes("ai:speech")) {
    console.error("ai: dictation offers a change it does not have");
    process.exit(1);
  }
  // Spotify: whichever device started the sign-in, this one shows it and can finish it.
  const spotify = (state) => runInContext(`SPOTIFY = Object.assign({}, SPOTIFY, { state: "${state}" }); viewSpotify()`, context);
  const waiting = spotify("waiting");
  if (!waiting.includes("spotify:finish") || !waiting.includes("accounts.spotify.com/authorize")
      || !waiting.includes("/connect/spotify/callback")) {
    console.error("spotify: a waiting sign-in cannot be finished here, or the app's addresses are not shown");
    process.exit(1);
  }
  if (!spotify("connected").includes("spotify:disconnect") || !spotify("not_set_up").includes("spotify:begin:endora")
      || !runInContext("viewConnections()", context).includes("go:spotify")) {
    console.error("spotify: connecting or disconnecting is missing, or Connections does not lead to it");
    process.exit(1);
  }
  runInContext('SPOTIFY = Object.assign({}, SPOTIFY, { state: "waiting" })', context);
  if (!runInContext("viewPermissions()", context).includes("autonomy:external")) {
    console.error("permissions: the autonomy switches are not on Permissions");
    process.exit(1);
  }
  runInContext("NAV = globalThis.__nav", context);
  console.log("settings: 8 entries; skills a list, each with its own screen; connections and permissions their own");
}

// A long link must wrap inside a card on every screen, not run off a phone's edge. It did on
// Activity, Inbox and others, which the chat bubbles never did because they had the rule.
{
  const css = readFileSync(resolve(here, "../app/node/src/web/styles.css"), "utf8");
  const card = (css.match(/\n\s*\.card \{[^}]*\}/) || [""])[0];
  if (!/overflow-wrap:\s*anywhere/.test(card)) {
    console.error("styles: .card lost overflow-wrap, so long links run off the screen again");
    process.exit(1);
  }
}

// The clean slate (owner, 2026-10-16): no Inbox, no check-in or nightly review to switch on,
// no durable goals to watch or stop, no beliefs listed kind by kind.
{
  for (const gone of ["go:inbox", "nightsched", "data-change=\"checkin\"", "tunesave", "affirm:belief",
    "viewGoals", "/v1/goals", "/v1/understanding"]) {
    if (source.includes(gone)) {
      console.error(`clean slate: ${gone} is back`);
      process.exit(1);
    }
  }
}

// About you is the brain (ADR 0101): what it knows best, its pages one tap away, and a way to
// start fresh; a page reads as text, edits as text, says what happened to it, and only a page
// that is not memory can be deleted.
{
  const fail = (what) => { console.error(what); process.exit(1); };
  const about = runInContext("viewUnderstanding()", context);
  if (!about.includes("Busy Monday to Friday")) fail("about you: what it knows best is missing");
  if (!about.includes('data-act="go:brainpage:hobbies/watches"')) fail("about you: a page cannot be opened");
  if (!about.includes("forgetlearned")) fail("about you: the way to start fresh is missing");
  runInContext('NAV = { v: "brainpage", id: "hobbies/watches" }; BRAIN_EDITING = false;', context);
  const page = runInContext("viewBrainPage()", context);
  if (page.includes("<b>dive</b>")) fail("brain page: the page's text became markup");
  if (!page.includes("noted from chat")) fail("brain page: its history is missing");
  if (!page.includes('data-act="braindelete:hobbies/watches"')) fail("brain page: it cannot be deleted");
  runInContext("BRAIN_EDITING = true;", context);
  const editing = runInContext("viewBrainPage()", context);
  if (!editing.includes('id="brain-text"') || editing.includes("<b>dive</b>")) fail("brain page: editing does not show the text safely");
  runInContext('NAV = { v: "brainpage", id: "memory" }; BRAIN_EDITING = false;', context);
  if (runInContext("viewBrainPage()", context).includes("braindelete")) fail("brain page: memory can be deleted");
  runInContext('NAV = { v: "chat" }; BRAIN_EDITING = false;', context);
  console.log("about you: the brain's pages open, edit as text, and memory stays");
}

// Endora HQ (ADR 0102): the office reads the snapshot; every decision is a button the owner
// presses, each offering only what that kind of item takes; nothing a task says becomes markup.
{
  const fail = (what) => { console.error(what); process.exit(1); };
  runInContext(`HQ.error = ""; HQ.log = []; HQ.selected = { type: "task", id: 7 };
    HQ.snap = { demo: true, stopped: false, last_event: 40, tick_seconds: 30, last_tick_at: 1,
      counts: { ticks: 9, running: 1, queued: 2, held: 1, folded: 3, open_tasks: 4 }, pool: { per_day: 2333333, committed: 400000, spent: 0, held: 400000 },
      members: [{ id: 1, name: "Ada", role: "lead", look: 0, paused: false, slots: 2, per_day: 583333, committed: 0, spent: 0, held: 0, running: [], reports_to: 5,
                  brain: "claude-sonnet-5-5", brains: [{ name: "local", label: "Local" }, { name: "claude-sonnet-5-5", label: "Claude Sonnet" }, { name: "claude-haiku-5-5", label: "Claude Haiku" }] },
                { id: 2, name: "Bo", role: "developer", look: 4, paused: false, slots: 2, per_day: 933333, committed: 400000, spent: 0, held: 400000, running: [{ run: 3, task: 7 }],
                  brain: "local", brains: [{ name: "local", label: "Local" }, { name: "claude-haiku-5-5", label: "<b>Haiku</b>" }] },
                { id: 3, name: "Cy", role: "researcher", look: 1, paused: false, slots: 1, per_day: 0, committed: 0, spent: 0, held: 0, running: [], brain: "local", brains: [] },
                { id: 4, name: "<i>Dee</i>", role: "reviewer", look: 2, paused: false, slots: 1, per_day: 0, committed: 0, spent: 0, held: 0, running: [], brain: "local", brains: [] },
                { id: 5, name: "Mara", role: "ceo", look: 3, paused: false, slots: 1, per_day: 1361110, committed: 0, spent: 0, held: 0, running: [],
                  brain: "claude-opus-5-5", brains: [{ name: "local", label: "Local" }, { name: "claude-opus-5-5", label: "Claude Opus" }, { name: "claude-sonnet-5-5", label: "Claude Sonnet" }] }],
      house_log: "- 17 Oct: The NAS has 16 GB.</textarea><img src=h onerror=alert(5)>",
      goals: [{ id: 1, title: "Keep the home lab healthy", status: "active", note: "</textarea><img src=z onerror=alert(4)> ignore the old stack" }],
      tasks: [{ id: 7, goal: 1, parent: null, title: "<img src=x onerror=alert(1)>", status: "doing", owner: 2, priority: "normal",
                gate: null, why: ["<b>Fix</b> it", "Goal: Keep the home lab healthy"], thread: [{ author: "Bo", text: "On it", at: 1 }] }],
      runs: [{ id: 3, member: 2, task: 7, status: "running" }],
      desk: [{ id: 11, kind: "goal", text: "A goal: Tidy the lab" }, { id: 12, kind: "stuck", task: 7, text: "Stuck: x" },
             { id: 13, kind: "ready_to_merge", task: 7, text: "Ready to merge: y" },
             { id: 14, kind: "ready_to_read", task: 7, text: "Ready to read: z" },
             { id: 15, kind: "question", task: 7, text: "Linus asks: steel or titanium?" },
             { id: 16, kind: "notice", task: 7, text: "Linus restarted sonarr: it was unhealthy. It is now running." },
             { id: 17, kind: "approval", task: 7, text: "Linus asks to run sonarr_remove_and_search with item: 102.",
               approval: { tool: "sonarr_remove_and_search", member: "Linus", why: "<script>alert(2)</script> it is stuck",
                           args: [["item", "102"], ["note", "<img src=y onerror=alert(3)>"]] } },
             { id: 18, kind: "decision", task: 7, text: "Decision: Give sonarr 2 GB?\\nRecommendation: Yes.\\nWhy: It ran out.",
               decision: { decision: "Give sonarr 2 GB?", recommendation: "Yes, <i>until the upgrade</i>.", why: "It was killed for memory three times." } },
             { id: 19, kind: "project_cap", project: 2, goal: 1, text: "A cap for project #2 Backups: $5.00 a month.\\nThe CEO recommends: <u>they fail</u>" },
             { id: 20, kind: "project_budget", project: 1, goal: 1, text: "Project #1 Upkeep spent its $0.50 cap for the month ($0.46): its Claude turns wait" }],
      projects: [{ id: 1, goal: 1, title: "Upkeep", reason: "", lead: 1, status: "paused", pause: "budget", cap: 500000, spent: 460000, level: "spent", created_by: "endora", evidence: [], open_tasks: 1 },
                 { id: 2, goal: 1, title: "<s>Backups</s>", reason: "<img src=p onerror=alert(6)>", lead: 1, status: "active", pause: null, cap: 1000000, asked_cap: 5000000, spent: 850000, level: "warned", created_by: "Ada", evidence: ["task 7", "event 40"], open_tasks: 0 },
                 { id: 3, goal: 1, title: "Media", reason: "", lead: 5, status: "proposed", pause: null, cap: null, spent: 0, level: null, created_by: "Ada", evidence: ["task 7"], open_tasks: 0 }],
      contracts: { all: true, restart_container: false },
      routines: [{ id: 3, title: "Check the home lab", rhythm: "every 6 hours", assignee: 1, enabled: true,
                   on_lab_trouble: true, last_fired_at: null, last_task: null, last_task_open: false, project: 1 },
                 { id: 4, title: "<b>Tidy</b>", rhythm: "every day", assignee: 2, enabled: false,
                   on_lab_trouble: false, last_fired_at: 1, last_task: 7, last_task_open: true }] };`, context);
  // The activity log reads like the office's own: who, which task, why, and what it cost.
  runInContext(`hqTell({ id: 41, at: 0, kind: "run.started", data: { run: 9, member: 2, task: 7, reason: "review_requested", reserved: 60000 } }, false);
    hqTell({ id: 42, at: 0, kind: "run.ended", data: { run: 9, member: 2, task: 7, result: "succeeded", spent: 120000 } }, false);
    hqTell({ id: 40, at: 0, kind: "run.ended", data: { run: 8, member: 2, task: 7, result: "succeeded", spent: 30000, cached: 87, input: 19000, output: 100 } }, false);
    hqTell({ id: 39, at: 0, kind: "run.ended", data: { run: 7, member: 2, task: 7, result: "retry_waiting", why: "the turn was over: exit 3: worker: the turn is over: HQ answered 401 (<token>)" } }, false);
    hqTell({ id: 43, at: 0, kind: "wake.queued", data: { member: 1, task: 7, reason: "mentioned" } }, false);
    hqTell({ id: 44, at: 0, kind: "routine.skipped", data: { routine: 3, title: "Check the home lab", why: "its last task, #7, is still open" } }, false);
    hqTell({ id: 45, at: 0, kind: "checkback.set", data: { member: 1, task: 7, minutes: 30, why: "see it held" } }, false);
    hqTell({ id: 46, at: 0, kind: "lesson.noted", data: { member: 1, task: 7, goal: 1, lesson: "An RSS sync that grabs nothing is normal." } }, false);
    hqTell({ id: 47, at: 0, kind: "member.brain", data: { member: 1, name: "Ada", brain: "claude-sonnet-5-5" } }, false);
    hqTell({ id: 48, at: 0, kind: "member.hired", data: { member: 5, name: "Mara", role: "ceo", brain: "claude-opus-5-5", by: "endora" } }, false);
    hqTell({ id: 49, at: 0, kind: "house.noted", data: { by: "Mara", entry: "- 17 Oct: The NAS has 16 GB." } }, false);
    hqTell({ id: 50, at: 0, kind: "project.budget", data: { project: 1, title: "Upkeep", level: "spent", said: "spent its $0.50 cap for the month ($0.46): its Claude turns wait" } }, false);
    hqTell({ id: 51, at: 0, kind: "member.reports_to", data: { member: 2, name: "Bo", reports_to: 1, manager: "Ada" } }, false);`, context);
  const office = runInContext("viewHQ()", context);
  for (const line of ["R9 Bo starts on EL-7 (review requested), $0.06 held", "R9 Bo finished · $0.12 spent", "R8 Bo finished · $0.03 spent · in 19000 / out 100 · cached: 87% of input", "Woke Ada: mentioned (EL-7)",
    "R7 Bo retry waiting: the turn was over: exit 3: worker: the turn is over: HQ answered 401 (&lt;token&gt;)",
    "Routine Check the home lab skipped: its last task, #7, is still open", "Ada will check back on EL-7 in 30 min",
    "Ada noted a lesson: An RSS sync that grabs nothing is normal.", "Ada now thinks with claude-sonnet-5-5",
    "Mara joined the team as CEO", "Mara noted in the house log: - 17 Oct: The NAS has 16 GB."]) {
    if (!office.includes(line)) fail(`hq: the activity log does not say "${line}"`);
  }
  if (office.includes("R9 Bo finished · $0.12 spent · cached")) fail("hq: a run that cached nothing claims a cache");
  if (!office.includes('class="k-done"') || !office.includes('class="k-wake"')) fail("hq: the activity log is not coloured by kind");
  for (const chip of ["Tick <b>9</b>", "Coalesced <b>3</b>", "Today <b>$0.00 spent · $0.40 held · $2.33 a day</b>", "$0.00 spent · $0.00 held of $1.36 a day", "Open tasks <b>4</b>"]) {
    if (!office.includes(chip)) fail(`hq: the office's numbers do not show ${chip}`);
  }
  if (!office.includes('data-act="hq:work"')) fail("hq: the team cannot be given work");
  if (!office.includes("Demo · scripted team")) fail("hq: demo mode is not labelled");
  if (!office.includes('id="hq-office"')) fail("hq: the office is not drawn");
  // A live update redraws the screen; the activity log keeps its scroll through it.
  if (!office.includes('class="hq-log" data-keep-scroll="hq-log"')) fail("hq: the activity log loses its place on every update");
  for (const act of ["hq:decide:11:approve", "hq:decide:11:decline", "hq:decide:12:retry", "hq:decide:12:drop",
    "hq:decide:13:merged", "hq:decide:13:not_now", "hq:decide:14:read", "hq:decide:14:not_now",
    "hq:decide:15:seen", "hq:decide:15:dismiss", "hq:decide:16:seen", "hq:decide:16:dismiss", "hq:stop",
    // The owner's switches for the team's contracts (ADR 0106): each offers the other way.
    "hq:contracts:all:off", "hq:contracts:restart_container:on",
    // Routines (ADR 0107): each switches the other way and runs now; the owner can add one.
    "hq:routine:3:off", "hq:routine:3:fire", "hq:routine:4:on", "hq:routine:4:fire", "hq:routine:new:add"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: the desk does not offer ${act}`);
  }
  if (office.includes("hq:decide:11:retry")) fail("hq: a goal was offered a choice for stuck work");
  // The bridge's approval (ADR 0108): the tool, its arguments in plain words, the why, and
  // Approve / Decline; none of what the member wrote becomes markup.
  for (const act of ["hq:decide:17:approve", "hq:decide:17:decline"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: an approval does not offer ${act}`);
  }
  if (office.includes("hq:decide:17:seen") || office.includes('id="hq-answer-17"')) fail("hq: an approval was offered something other than yes or no");
  for (const said of ["sonarr_remove_and_search", '<dt>item</dt><dd>102</dd>', "Linus says why"]) {
    if (!office.includes(said)) fail(`hq: the approval card does not show ${said}`);
  }
  if (office.includes("<img src=y") || office.includes("<script>alert(2)")) fail("hq: an approval's arguments or reason became markup");
  if (!office.includes("&lt;img src=y onerror=alert(3)&gt;")) fail("hq: an approval's argument is not shown as text");
  // The owner replies on the task they are looking at, and writes each active goal's notes.
  if (!office.includes('id="hq-reply-7"') || !office.includes('data-act="hq:reply:7:send"')) fail("hq: the task looked at has no reply box");
  if (!office.includes('id="hq-note-1"') || !office.includes('data-act="hq:note:1:save"')) fail("hq: a goal's notes cannot be written");
  if (office.includes("<img src=z") || !office.includes("&lt;/textarea&gt;&lt;img src=z")) fail("hq: a goal's notes became markup");
  if (office.includes("hq:decide:16:approve") || office.includes('id="hq-answer-16"')) fail("hq: a notice was offered a decision");
  // A member's question can be answered on its card: the answer goes in the task's thread.
  if (!office.includes('id="hq-answer-15"')) fail("hq: a question cannot be answered from its card");
  if (office.includes('id="hq-answer-12"')) fail("hq: stuck work was offered an answer box");
  if (!office.includes("Demo: Ready to merge: y") || !office.includes("no pull request behind this")) {
    fail("hq: a demo card does not say it is a demo");
  }
  if (!office.includes("nothing is spent")) fail("hq: the demo's spend line reads as real money");
  if (office.includes("<img src=x") || office.includes("<b>Fix</b>")) fail("hq: a task's text became markup");
  if (!office.includes("Goal: Keep the home lab healthy")) fail("hq: a task's chain of why is missing");
  for (const said of ["every 6 hours", "not run yet", "takes home-lab alarms", 'id="hq-routine-title"', 'id="hq-routine-hours"', 'id="hq-routine-who"']) {
    if (!office.includes(said)) fail(`hq: the routines card does not show ${said}`);
  }
  if (office.includes("<b>Tidy</b>")) fail("hq: a routine's title became markup");
  // What each member thinks with (ADR 0109): only the brains HQ lists for its role, the one it
  // has marked pressed, each a button through the node.
  for (const act of ["hq:brain:1:local", "hq:brain:1:claude-sonnet-5-5", "hq:brain:1:claude-haiku-5-5", "hq:brain:2:local", "hq:brain:2:claude-haiku-5-5"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: a member's card does not offer ${act}`);
  }
  if (office.includes("hq:brain:2:claude-sonnet-5-5")) fail("hq: a member was offered a brain its role may not use");
  if (!/aria-pressed="true"[^>]*data-act="hq:brain:1:claude-sonnet-5-5"/.test(office)) fail("hq: the brain a member has is not marked");
  if (!/aria-pressed="false"[^>]*data-act="hq:brain:1:local"/.test(office)) fail("hq: a brain a member does not have is marked");
  if (!office.includes("Brain:")) fail("hq: the brain choice is not labelled");
  // The CEO (ADR 0111): in the Team card, offered only what its role allows: Opus, Sonnet, Local.
  for (const act of ["hq:brain:5:local", "hq:brain:5:claude-opus-5-5", "hq:brain:5:claude-sonnet-5-5", "hq:rename:5", "hq:rename:1"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: the team card does not offer ${act}`);
  }
  if (office.includes("hq:brain:5:claude-haiku-5-5") || office.includes("hq:brain:1:claude-opus-5-5")) fail("hq: a brain was offered that the role may not use");
  if (!office.includes("CEO · ")) fail("hq: the CEO's role is not shown as CEO");
  if (!office.includes("the house's model: costs nothing")) fail("hq: a local member reads as if it cost money");
  // The house log: readable and editable, and its text stays text.
  if (!office.includes('id="hq-house"') || !office.includes('data-act="hq:house:save"') || !office.includes("House log")) fail("hq: the house log cannot be read and edited");
  if (office.includes("<img src=h") || !office.includes("&lt;/textarea&gt;&lt;img src=h")) fail("hq: the house log became markup");
  // The CEO's decision for the board: decision, recommendation and why, with Approve, Decline
  // and an answer box.
  for (const act of ["hq:decide:18:approve", "hq:decide:18:decline", "hq:decide:18:answer"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: a decision does not offer ${act}`);
  }
  if (!office.includes('id="hq-answer-18"')) fail("hq: a decision cannot be answered from its card");
  for (const said of ["<dt>Decision</dt><dd>Give sonarr 2 GB?</dd>", "<dt>Recommendation</dt>", "<dt>Why</dt><dd>It was killed for memory three times.</dd>", "Yes, &lt;i&gt;until the upgrade&lt;/i&gt;."]) {
    if (!office.includes(said)) fail(`hq: the decision card does not show ${said}`);
  }
  if (office.includes("<i>until")) fail("hq: a decision's words became markup");
  // Projects (ADR 0113): grouped by goal, each with its lead, status and money against its cap
  // (amber from 80%, red once spent); the owner's buttons offer only what each status takes;
  // whatever a member wrote stays text.
  for (const act of ["hq:project:1:resume", "hq:project:1:cap", "hq:project:1:drop", "hq:project:2:work", "hq:project:2:pause",
    "hq:project:2:cap", "hq:project:2:drop", "hq:project:3:reopen", "hq:project:3:drop", "hq:project:new:add",
    "hq:decide:19:approve", "hq:decide:19:decline", "hq:decide:20:raise", "hq:decide:20:wait", "hq:reports:1:set", "hq:reports:2:set"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: the projects card or the desk does not offer ${act}`);
  }
  for (const wrong of ["hq:project:1:pause", "hq:project:2:resume", "hq:project:3:work", "hq:project:2:reopen", "hq:reports:5:set", "hq:decide:19:seen", "hq:decide:20:approve"]) {
    if (office.includes(`data-act="${wrong}"`)) fail(`hq: something offers ${wrong}, which its item does not take`);
  }
  for (const said of ["<h2>Projects</h2>", 'class="hq-gauge spent"', 'class="hq-gauge warned"', "$0.46 of $0.50 this month", "$0.00 this month · no cap",
    "paused: it spent its cap", "proposed by Ada, waits for the CEO", "asks $5.00 a month", "Evidence: task 7, event 40", 'id="hq-answer-19"', 'id="hq-answer-20"',
    'id="hq-project-title"', 'id="hq-project-cap"', 'id="hq-project-lead"', 'id="hq-reports-1"', "Reports to the board: you.", "in Upkeep",
    '<option value="5" selected>Mara (CEO)</option>', "Project Upkeep spent its $0.50 cap", "Bo now reports to Ada", "project Upkeep"]) {
    if (!office.includes(said)) fail(`hq: the projects card does not show ${said}`);
  }
  if (office.includes("<s>Backups") || office.includes("<img src=p") || office.includes("<u>they")) fail("hq: a project's words became markup");
  if (!office.includes("&lt;s&gt;Backups&lt;/s&gt;")) fail("hq: a project's title is not shown as text");
  if (office.includes("<b>Haiku</b>")) fail("hq: a brain's label became markup");
  // A dropped goal's notes leave the board, finished ones too: the retired demo's hundreds of
  // done tasks must not bury the owner's own.
  if (office.includes("<i>Dee</i>") || !office.includes("&lt;i&gt;Dee&lt;/i&gt;")) fail("hq: a member's name became markup");
  // The office itself (the pixel canvas): a desk for each of five members, the CEO's office and
  // the meeting room where people walk, and the whole room drawn, through an explicit 2D context
  // that keeps what was written on it. No art is loaded here, so this draws the room's own shapes.
  {
    const seats = runInContext("hqSeats(HQ.snap.members)", context);
    const desks = Object.values(seats).map((s) => `${s.desk.x},${s.desk.y}`);
    if (desks.length !== 5 || new Set(desks).size !== 5) fail(`hq: five members do not have five desks: ${desks}`);
    if (seats[5].desk.room !== "ceo") fail("hq: the CEO's desk is not in the CEO's office");
    const inside = runInContext(`(() => {
      const In = (r, s) => s.x >= r.x && s.x < r.x + r.w && s.y >= r.y && s.y < r.y + r.h;
      const M = HQ_ROOMS.meeting, O = HQ_ROOMS.ceo;
      return [!!M && !!O, In(M, HQ_MEETING.head), HQ_MEETING.chairs.every((c) => In(M, c)), In(M, M.in), !In(M, M.out),
        In(O, HQ_CEO_DESK), In(O, HQ_CEO_REST), In(O, O.in), !In(O, O.out)];
    })()`, context);
    if (inside.some((ok) => !ok)) fail(`hq: the meeting room or the CEO's office is not where its seats and door are: ${inside}`);
    const written = [];
    const ctx = {
      fillRect() {}, strokeRect() {}, drawImage() {}, beginPath() {}, arc() {}, moveTo() {}, fill() {}, stroke() {},
      save() {}, restore() {}, translate() {}, scale() {},
      fillText(text) { written.push(String(text)); },
      measureText: (text) => ({ width: String(text).length * 6 }),
    };
    const canvas = () => ({ ...element(), width: 0, height: 0, getContext: () => ctx });
    const { getElementById, createElement } = stubs.document;
    stubs.document.getElementById = (id) => (id === "hq-office" ? canvas() : getElementById(id));
    stubs.document.createElement = (tag) => (tag === "canvas" ? canvas() : createElement(tag));
    const saved = runInContext("JSON.stringify(HQ.snap)", context);
    try {
      runInContext(`HQ.people = {}; HQ_BG = hqPaintRoom(); HQ_FURN = hqFurniture(); hqPlacePeople(); hqDraw();`, context);
      for (const label of ["BOARD ROOM", "CEO", "MEETING", "SERVERS"]) {
        if (!written.includes(label)) fail(`hq: the office does not say ${label}`);
      }
      if (!written.includes("<I>DEE</I>")) fail("hq: a member's name is not written on the canvas");
      // The CEO's daily review: the CEO at the head of the meeting table, the lead on a chair.
      const review = runInContext(`HQ.snap.routines.push({ id: 9, title: "Daily review", assignee: 5 });
        HQ.snap.tasks.push({ id: 8, goal: 1, title: "Daily review", status: "doing", routine: 9, thread: [] });
        HQ.snap.members.find((m) => m.id === 5).running = [{ run: 20, task: 8 }];
        hqPlacePeople(); hqDraw();
        [HQ.people[5].at === HQ_MEETING.head, HQ.people[1].at === HQ_MEETING.chairs[0], HQ.people[2].at.desk === true]`, context);
      if (review.some((ok) => !ok)) fail(`hq: the daily review does not take the CEO and the lead to the meeting room: ${review}`);
      // A question for the CEO: the one who asked goes to the CEO's open door; a decision for
      // the board: the CEO takes it to the board room's door and says so there.
      const errands = runInContext(`HQ.snap.members.find((m) => m.id === 5).running = []; hqPlacePeople();
        hqTell({ id: 60, at: 0, kind: "thread.posted", data: { task: 7, author: "Ada", preview: "@Mara <b>which</b>?" } }, true);
        hqTell({ id: 61, at: 0, kind: "wake.queued", data: { member: 5, task: 7, reason: "question_asked" } }, true);
        hqTell({ id: 62, at: 0, kind: "desk.added", data: { item: 30, kind: "decision", task: 7 } }, true);
        const ada = HQ.people[1], mara = HQ.people[5];
        const there = ada.at.x === HQ_ROOMS.ceo.out.x && ada.at.y === HQ_ROOMS.ceo.out.y && ada.path.length > 0;
        const board = mara.at.x === HQ_P.boardDoor.x && mara.at.y === HQ_P.boardDoor.y && mara.path.length > 0;
        hqTell({ id: 63, at: 0, kind: "run.ended", data: { run: 21, member: 5, task: 7, result: "succeeded" } }, true);
        [there, board, !ada.errand, ada.at === ada.seat.rest]`, context);
      if (errands.some((ok) => !ok)) fail(`hq: a question or a decision does not send its member to the right door and back: ${errands}`);
    } finally {
      stubs.document.getElementById = getElementById;
      stubs.document.createElement = createElement;
      runInContext(`HQ_BG = null; HQ_FURN = []; HQ.people = {}; HQ.snap = JSON.parse(${JSON.stringify(saved)});`, context);
    }
  }
  const cards = runInContext(`HQ.snap.goals.push({ id: 2, title: "The demo's goal", status: "dropped" });
    for (let i = 0; i < 40; i++) HQ.snap.tasks.unshift({ id: 100 + i, goal: 2, title: "demo", status: "done" });
    hqCards().map((c) => c.t.id)`, context);
  if (cards.length !== 1 || cards[0] !== 7) fail(`hq: a dropped goal's tasks are still on the board: ${cards}`);
  runInContext("HQ.snap.stopped = true;", context);
  if (!runInContext("viewHQ()", context).includes('data-act="hq:resume"')) fail("hq: a stopped team cannot be resumed");
  runInContext('HQ.snap = null; HQ.error = "HQ is not set up on this node yet."; HQ.selected = null;', context);
  if (!runInContext("viewHQ()", context).includes("not set up")) fail("hq: a node without HQ does not say so");
  runInContext('HQ.error = "";', context);
  console.log("hq: the office reads the snapshot, the desk offers only what each item takes, and text stays text");
  console.log("hq: the office draws five desks, the CEO's office and the meeting room, and walks people to them");
}

// The agenda (ADR 0103): the Now line says it is working and opens Activity; only what needs the
// owner is pinned, each with Done, Dismiss and Stop; what a mail says stays text.
{
  const fail = (what) => { console.error(what); process.exit(1); };
  runInContext(`AGENDA = { now: { watching: 4, needs_you: 2, checked: [{ source: "endora", at: Date.now() }, { source: "docker", at: Date.now() - 120000 }, { source: "mail", at: Date.now() - 900000 }] },
    items: [
      { id: 5, kind: "mail_money_due", kind_said: "Mail with money due", level: "needs_you", level_said: "Needs you", from_mail: true,
        sentence: "<img src=x onerror=alert(1)> wants $12", why: "", opened_at: Date.now(), stop_said: "Don't tell me about mail from billing@example.com" },
      { id: 6, kind: "event", kind_said: "On the calendar", level: "fyi", level_said: "FYI", sentence: "Today: Dentist", why: "", opened_at: Date.now(), stop_said: "x" },
      { id: 7, kind: "container_crashed", kind_said: "A container crashed", level: "handled", level_said: "Handled", sentence: "sonarr crashed (exit 1)", why: "Restarted it.", opened_at: Date.now(), stop_said: "x" },
      { id: 8, kind: "drive_failing", kind_said: "A drive is failing", level: "needs_you", level_said: "Needs you", from_mail: false,
        sentence: "/dev/sda is failing its SMART health check", why: "", opened_at: Date.now() - 3600000, stop_said: "Don't tell me about DiskFailingSmart on /dev/sda" }] };
    NAV = { v: "chat" };`, context);
  const chat = runInContext("viewChat()", context);
  if (!chat.includes("Watching 4 things · checked the NAS 2 min ago")) fail("agenda: the Now line does not say what it is doing");
  if (!chat.includes('class="now-line" data-act="go:audit"')) fail("agenda: the Now line does not open Activity");
  if (!chat.includes('data-act="go:needs">2 need you ›')) fail("agenda: the Now line does not say how many need you, or open them");
  // Above the chat, the most pressing one only: the house before the mail.
  for (const act of ["agenda:done:8", "agenda:dismiss:8", "agenda:stop:8"]) {
    if (!chat.includes(`data-act="${act}"`)) fail(`agenda: the pinned card does not offer ${act}`);
  }
  if (chat.includes("agenda:done:5")) fail("agenda: more than one card took the chat's screen");
  if (chat.includes("agenda:done:6") || chat.includes("agenda:done:7")) fail("agenda: something that does not need the owner was pinned");
  runInContext('NAV = { v: "needs" };', context);
  const needs = runInContext("viewNeeds()", context);
  for (const act of ["agenda:done:8", "agenda:done:5", "agenda:dismiss:6"]) {
    if (!needs.includes(`data-act="${act}"`)) fail(`agenda: Needs you does not offer ${act}`);
  }
  if (needs.indexOf("agenda:done:8") > needs.indexOf("agenda:done:5")) fail("agenda: the mail came before the house");
  if (needs.includes("agenda:done:7")) fail("agenda: something handled is listed as needing you");
  if (needs.includes("<img src=x")) fail("agenda: an item's text became markup");
  runInContext('NAV = { v: "chat" };', context);
  runInContext(`AGENDA_RULES = { rules: [{ id: 2, said: "Leave the container ebooks alone", at: Date.now() }],
    kinds: [{ kind: "container_crashed", said: "A container crashed", response: "handle", default: "handle",
      allowed: [{ name: "handle", said: "Restart it" }, { name: "needs_you", said: "Needs you" }] }] };
    NAV = { v: "watching" };`, context);
  const watching = runInContext("viewWatching()", context);
  for (const act of ["agenda:forget:2", "agenda:set:container_crashed:needs_you", "agenda:set:container_crashed:handle"]) {
    if (!watching.includes(`data-act="${act}"`)) fail(`agenda: Settings does not offer ${act}`);
  }
  if (!watching.includes("Restart it (usual)")) fail("agenda: Settings does not say which is usual");
  runInContext('AGENDA = null; AGENDA_RULES = null; NAV = { v: "chat" };', context);
  if (runInContext("viewChat()", context).includes("now-line")) fail("agenda: a Now line was drawn with nothing to say");
  console.log("agenda: the Now line opens Activity, only what needs you is pinned, and Settings can undo and re-level");
}

// Speakers (ADR 0104): every source offers its speakers to tap, never the device's own; a wire
// has its slider; a speaker says when another device has it; setups play; text stays text.
{
  const fail = (what) => { console.error(what); process.exit(1); };
  runInContext(`SOUND.error = ""; SOUND.ticket = null; SOUND.listening = false; SOUND.found = ["Kitchen", "Living room"];
    SOUND.state = { senders: 4, listening: [4],
      sources: [{ id: 1, name: "<b>Classical</b>", kind: "radio", device: null, address: "https://radio.example.com/c" },
                { id: 2, name: "Endora Mixer (iPhone)", kind: "airplay", device: "iphone" }],
      speakers: [{ id: 3, name: "Kitchen", kind: "airplay", device: null, volume: 40, borrowed: false },
                 { id: 4, name: "iPhone", kind: "stream", device: "iphone", volume: 100, borrowed: false },
                 { id: 5, name: "Office", kind: "airplay", device: null, volume: 70, borrowed: true }],
      wires: [{ source: 1, speaker: 3, level: 60 }, { source: 1, speaker: 4, level: 10 }, { source: 2, speaker: 5, level: 90 }],
      groups: [{ speakers: [3], mix: [{ source: 1, level: 60 }], needs_sender: true },
               { speakers: [4], mix: [{ source: 1, level: 10 }], needs_sender: false }],
      setups: [{ name: "Kitchen mornings", wires: [{ source: 1, speaker: 3, level: 60 }] }] };`, context);
  const screen = runInContext("viewSound()", context);
  for (const act of ['data-act="sound:wire:1-3"', 'data-act="sound:wire:1-5"', 'data-act="sound:wire:2-3"',
    'data-change="sound:level:1-3"', 'data-change="sound:all:1"', 'data-change="sound:volume:3"',
    'data-act="sound:play:0"', 'data-act="sound:forget:0"', 'data-act="sound:mine"',
    'data-act="sound:free:5"', 'data-act="sound:addfound:1"']) {
    if (!screen.includes(act)) fail(`speakers: the screen does not offer ${act}`);
  }
  if (screen.includes('data-act="sound:wire:2-4"')) fail("speakers: the phone was offered its own stream");
  if (!screen.includes("pick “Endora Mixer (iPhone)” in its AirPlay menu")) fail("speakers: a receiver does not say how to use it");
  if (!screen.includes('data-act="sound:addreceiver"')) fail("speakers: a receiver cannot be added");
  if (screen.includes('data-act="sound:addfound:0"')) fail("speakers: a speaker already added was offered again");
  if (screen.includes('data-act="sound:free:3"')) fail("speakers: a speaker nobody borrowed offered to be taken back");
  // A borrowed speaker says so where it is tapped, and the way back is above the sources.
  if (!/Office · 90 · in use/.test(screen)) fail("speakers: a borrowed speaker's wire does not say it is in use");
  if (screen.indexOf('data-act="sound:free:5"') > screen.indexOf('data-act="sound:wire:1-3"')) fail("speakers: taking a speaker back is below the sources");
  if (!screen.includes("in use by another device")) fail("speakers: a borrowed speaker does not say so");
  if (screen.includes("<b>Classical</b>")) fail("speakers: a source's name became markup");
  if (!/aria-pressed="true">Kitchen · 60/.test(screen)) fail("speakers: a wire does not show its level");
  if ((screen.match(/id="sound-radio-name"/g) || []).length !== 1) fail("speakers: the station form shows twice, or not at all");
  // With nothing to play, adding a station is in front, not folded away; and a device that
  // listens to nothing wired says so.
  runInContext(`SOUND.listening = true; var keepGet = localStorage.getItem;
    localStorage.getItem = (k) => (k === "endora.soundSpeaker" ? "4" : keepGet(k));
    SOUND.state = Object.assign({}, SOUND.state, { sources: [], wires: [], groups: [] });`, context);
  const empty = runInContext("viewSound()", context);
  if (!empty.includes("Add something to play") || (empty.match(/id="sound-radio-name"/g) || []).length !== 1) {
    fail("speakers: with no sources, adding a station is not in front");
  }
  if (!empty.includes("nothing is wired to this iPhone yet")) fail("speakers: a device listening to nothing does not say why it is silent");
  runInContext("SOUND.listening = false; localStorage.getItem = keepGet;", context);
  runInContext('SOUND.state = null; SOUND.error = "The sound hub is not set up on this node yet.";', context);
  if (!runInContext("viewSound()", context).includes("not set up")) fail("speakers: a node without the hub does not say so");
  runInContext('SOUND.error = "";', context);
  console.log("speakers: every source offers its speakers but never its own device, wires carry their levels, and text stays text");
}

// Music (ADR 0104): Spotify plays on the hub's speakers through its Endora receiver, listed
// beside Spotify's own devices; the receiver itself is not a choice.
{
  const fail = (what) => { console.error(what); process.exit(1); };
  runInContext(`var keepMusic = JSON.stringify(MUSIC), keepSpotify = SPOTIFY, keepSound = SOUND.state;
    SPOTIFY = { state: "connected" };
    MUSIC.devices = [{ id: "d-mac", name: "Mac", is_active: false }, { id: "d-endora", name: "Endora", is_active: true }];
    MUSIC.now = null;
    SOUND.state = { sources: [{ id: 1, name: "Spotify", kind: "spotify" }],
      speakers: [{ id: 3, name: "Office HomePod", kind: "airplay", borrowed: false }, { id: 4, name: "iPhone", kind: "stream", borrowed: false }],
      wires: [{ source: 1, speaker: 4, level: 80 }], groups: [] };`, context);
  const music = runInContext("viewMusic()", context);
  if (!music.includes('data-act="music:endora:3"') || !music.includes('data-act="music:endora:4"')) fail("music: the hub's speakers are not offered");
  if (music.includes('data-act="music:device:d-endora"')) fail("music: the Endora receiver is offered as a speaker of its own");
  if (!music.includes('data-act="music:device:d-mac"')) fail("music: Spotify's own devices went missing");
  runInContext(`MUSIC.now = { is_playing: true, item: { id: "t1", type: "track", name: "A song", artists: [{ name: "Someone" }], album: { images: [] } } };
    MUSIC.liked = false;`, context);
  if (!runInContext("viewMusic()", context).includes('data-act="music:like" aria-pressed="false"')) fail("music: a song cannot be liked");
  if (!runInContext("viewMusic()", context).includes('data-act="music:nope"')) fail("music: a song cannot be turned down");
  if (!runInContext("viewMusic()", context).includes('data-act="music:discover"')) fail("music: there is no Discover");
  runInContext("MUSIC.liked = true;", context);
  if (!runInContext("viewMusic()", context).includes('aria-pressed="true" aria-label="In Liked Songs')) fail("music: a liked song does not show it");
  // Through Endora: each speaker's volume, not the receiver's.
  runInContext(`MUSIC.now = { is_playing: true, device: { name: "Endora", volume_percent: 100 }, item: { id: "t1", type: "track", name: "A song", artists: [], album: { images: [] } } };
    SOUND.state = Object.assign({}, SOUND.state, { speakers: [{ id: 3, name: "Office HomePod", kind: "airplay", volume: 40 }, { id: 4, name: "iPhone", kind: "stream", volume: 100 }] });`, context);
  const viaEndora = runInContext("viewMusic()", context);
  if (!viaEndora.includes('data-change="sound:volume:4"') || viaEndora.includes("music:volume:10")) fail("music: through Endora, the volume is not the speakers'");
  runInContext("MUSIC.now = null; MUSIC.liked = null;", context);
  if (!/pill pick active" data-act="music:endora:4"/.test(music)) fail("music: the speaker Spotify plays on is not shown as playing");
  if (!/data-act="music:endora:4">iPhone · not listening/.test(music)) fail("music: a phone that is not listening does not say so");
  // On that phone itself, it is this device, and a tap starts it listening.
  runInContext(`var keepGet2 = localStorage.getItem;
    localStorage.getItem = (k) => (k === "endora.soundSpeaker" ? "4" : keepGet2(k));`, context);
  const onPhone = runInContext("viewMusic()", context);
  if (!/data-act="music:endora:4">iPhone · this device/.test(onPhone)) fail("music: this device is not shown as this device");
  runInContext("localStorage.getItem = keepGet2;", context);
  runInContext("Object.assign(MUSIC, JSON.parse(keepMusic)); SPOTIFY = keepSpotify; SOUND.state = keepSound;", context);
  console.log("music: Spotify's devices and the hub's speakers are one list, and the receiver is not a choice");
}

// Devices that stopped answering are never asked about: the owner turned it off for good
// (2026-10-16), so nothing may bring the card or its actions back.
if (/Still yours\?|data-act="trouble:|standing-trouble/.test(source)) {
  console.error("chat: the \"Still yours?\" card for devices that stopped answering is back");
  process.exit(1);
}

// The handheld's features on the web (plan phase 4): the web console is the full set.
{
  const fail = (what) => { console.error(what); process.exit(1); };
  const id = (digit) => digit.repeat(64);
  const lab = runInContext("viewLab()", context);
  if (!lab.includes("go:containers") || !lab.includes("go:jobs") || !lab.includes("CPU 7%")
      || !lab.includes(`go:logs:${id("1")}`) || lab.includes("<b>Plex</b>")) {
    fail("home lab: the figures, a service's logs or the way to containers and jobs is missing, or a name is not escaped");
  }
  const containers = runInContext("viewContainers()", context);
  // Endora's own path is never offered a stop; a running container can stop, a stopped one start.
  if (containers.includes(`stop:${id("2")}`) || containers.includes(`restart:${id("2")}`)
      || !containers.includes(`lab:container:stop:${id("3")}`) || !containers.includes(`lab:container:start:${id("4")}`)) {
    fail("containers: Endora's own path can be stopped, or a container cannot be stopped or started");
  }
  if (containers.indexOf("radarr") > containers.indexOf("sonarr")) fail("containers: a stopped one is not listed first");
  runInContext('LAB.dashboard.groups.unshift({ name: "Alerts", services: [{ name: "DiskLow", state: "unhealthy" }] })', context);
  if (!runInContext("viewLab()", context).includes("1 firing")) fail("home lab: firing alerts are counted as services that are down");
  if (runInContext("viewLogs()", context).includes("<script>")) fail("logs: a log line became markup");
  const jobs = runInContext("viewJobs()", context);
  if (!jobs.includes("lab:job:run:check") || !jobs.includes("go:job:web-abc") || jobs.includes("go:job:null")) {
    fail("jobs: a job cannot be run, its last run opened, or one never run offers a run to open");
  }
  const media = runInContext("viewMedia()", context);
  if (!media.includes("media:request:movie:438631") || media.includes("media:request:tv:90228") || !media.includes("Fall (2022)")) {
    fail("media: only what is not requested yet may be requested, and recent requests show");
  }
  const house = runInContext("viewHouse()", context);
  // Only what can be switched is offered a switch; a sensor is only read.
  if (!house.includes("house:home:on:light.kitchen") || !house.includes("house:home:off:light.kitchen")
      || !house.includes("house:home:scene:scene.movie") || house.includes(":sensor.porch")
      || !house.includes("house:favorite:remove:light.kitchen") || !house.includes("house:favorite:add:switch.fan")) {
    fail("smart home: a favorite's switch, a scene, or choosing favorites is wrong");
  }
  runInContext('SPOTIFY = Object.assign({}, SPOTIFY, { state: "connected" })', context);
  const music = runInContext("viewMusic()", context);
  if (!music.includes("music:toggle") || !music.includes("Pause") || !music.includes("music:device:def456")
      || !music.includes("music:play:f0") || !music.includes("music:play:l0") || music.includes("<i>song</i>")) {
    fail("music: playing, choosing a device or playing a find or a playlist is missing, or a title is not escaped");
  }
  runInContext('SPOTIFY = Object.assign({}, SPOTIFY, { state: "not_set_up" })', context);
  if (!runInContext("viewMusic()", context).includes("go:spotify")) fail("music: not connected, it does not lead to connecting");
  runInContext('SPOTIFY = Object.assign({}, SPOTIFY, { state: "waiting" })', context);
  for (const view of ["music", "house", "media", "lab"]) {
    if (!source.includes(`item("go:${view}"`)) fail(`menu: ${view} is not in the menu`);
  }
  // A screen that fetches its own data must be fetched when it is opened, or it shows
  // "Loading…" until something else happens to reload the console — which Mail did.
  const fetched = new Set();
  for (const fn of ["loadView", "loadLab"]) {
    const body = (source.match(new RegExp(`async function ${fn}\\([\\s\\S]*?\\n}\\n`)) || [""])[0];
    for (const m of body.matchAll(/v === "([a-z]+)"/g)) fetched.add(m[1]);
  }
  const opened = runInContext("FETCHES_ITS_OWN", context);
  const missing = [...fetched].filter((v) => !opened.includes(v));
  if (fetched.size < 10 || missing.length) fail(`navigation: these screens fetch data that opening them does not: ${missing.join(", ")}`);
  console.log(`home lab, media, smart home, music: render, act through the allowlist, and fetch when opened (${fetched.size} screens)`);
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
// Another site's `/v1` (Spotify's) is not this node, and is the opposite case: see below.
const unsigned = [...source.matchAll(/fetch\(\s*["'`]([^"'`]*\/v1[^"'`]*)[\s\S]{0,200}?headers:\s*\{/g)]
  .map((m) => m[1])
  .filter((url) => !/^https?:\/\//.test(url));
// And this node's credential goes to this node only. Music calls Spotify from the browser with
// a token the node lends; signing that request would hand Endora's own token to Spotify.
const leaks = [...source.matchAll(/fetch\(\s*["'`](https?:\/\/[^"'`]*)[\s\S]{0,300}?\)\s*;/g)]
  .filter((m) => /signed\(|token\(\)/.test(m[0]))
  .map((m) => m[1]);
if (leaks.length) {
  console.error(`requests: these send Endora's credential to another site: ${leaks.join(", ")}`);
  process.exit(1);
}
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

// Private on: what is typed goes to the chat as a private message, never to the deep model and
// never to research, whatever else is switched on.
{
  const fail = (what) => { console.error(what); process.exit(1); };
  const typed = { ...element(), value: "about my results" };
  const realGet = context.document.getElementById;
  context.document.getElementById = (id) => (id === "chat-input" ? typed : element());
  const before = sent.length;
  runInContext("PRIVATE_MODE = true; DEEP_MODE = true; RESEARCH_MODE = true; DEEP_MODEL = { configured: true };", context);
  await runInContext("startResearch()", context);
  typed.value = "about my results";
  await runInContext("sendChat()", context);
  context.document.getElementById = realGet;
  runInContext("PRIVATE_MODE = false; DEEP_MODE = false; RESEARCH_MODE = false; DEEP_MODEL = { configured: false };", context);
  const after = sent.slice(before);
  if (after.some((r) => /\/v1\/(research|deep-ask)/.test(String(r.path)))) fail("requests: a private message went to research or the deep model");
  const asked = after.filter((r) => String(r.path).includes("/v1/chat/stream"));
  if (asked.length !== 1 || !JSON.parse(asked[0].body).private) fail("requests: a private message was not sent as one");
  console.log("requests: a private message goes to the chat as one, never to research or the deep model");
}

// Discover keeps what it found for its list, so the buttons survive the re-render that follows.
{
  const realFetch = context.fetch;
  context.fetch = (path, opts = {}) => {
    sent.push({ path, headers: opts.headers || {}, method: opts.method || "GET", body: opts.body });
    return Promise.resolve({ ok: true, status: 200, json: async () => ({ models: ["qwen2.5:14b"] }) });
  };
  const chosen = { ...element(), value: "2" };
  const realGet = context.document.getElementById;
  context.document.getElementById = (id) => (id === "ml-everyday-provider" ? chosen : element());
  runInContext("FOUND_MODELS = {};", context);
  await runInContext('dispatch("mlist:discover:everyday")', context);
  context.document.getElementById = realGet;
  context.fetch = realFetch;
  const kept = runInContext("JSON.stringify(FOUND_MODELS.everyday)", context);
  if (kept !== JSON.stringify({ provider: 2, models: ["qwen2.5:14b"] })) {
    console.error(`models: Discover did not keep what it found for its list: ${kept}`);
    process.exit(1);
  }
  runInContext("FOUND_MODELS = {};", context);
  console.log("models: Discover keeps what it found, by list and provider");
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

// Private messages (ADR 0105): a 🔒 message and Endora's reply show with a lock and without the
// mark the door reads; Private mode says so in the composer and keeps the deep model out of it.
{
  const fail = (what) => { console.error(what); process.exit(1); };
  runInContext(`CHAT_MSGS = [{ id: "1", role: "user", text: "[private] about my results", at_ms: Date.now() },
    { id: "2", role: "butler", text: "[private] They look fine.", at_ms: Date.now() }];
    PRIVATE_MODE = true; DEEP_MODE = true; DEEP_MODEL = { configured: true }; AGENDA = null; NAV = { v: "chat" };`, context);
  const chat = runInContext("viewChat()", context);
  if (chat.includes("[private]")) fail("private: the mark reached the screen");
  if ((chat.match(/class="private-lock"/g) || []).length < 2) fail("private: a private message or its reply shows no lock");
  // The chips, not the "+" menu, which always offers the switch.
  if (!runInContext("composerChipsHtml()", context).includes('data-act="toggle:private"')) fail("private: Private mode does not say so in the composer");
  if (chat.includes('data-act="toggle:deep" title="Your messages go to the bigger model')) fail("private: the deep model is offered for a private message");
  // Research goes out to web search, so with Private on the send button sends a private message.
  runInContext("RESEARCH_MODE = true;", context);
  const both = runInContext("viewChat()", context);
  if (/id="send-btn" data-act="research:start"/.test(both)) fail("private: with Private on, send still commissions research");
  if (both.includes('data-act="toggle:research" title="Commissioning')) fail("private: research is offered for a private message");
  runInContext('PRIVATE_MODE = false; DEEP_MODE = false; RESEARCH_MODE = false; DEEP_MODEL = { configured: false }; CHAT_MSGS = [];', context);
  if (runInContext("composerChipsHtml()", context).includes('data-act="toggle:private"')) fail("private: the composer says Private when it is off");
  console.log("private: a private message shows a lock without its mark, and never goes to the deep model or to research");
}

// Models in ordered lists (ADR 0105): each list in order with its moves, a cloud entry said to
// see your messages disguised, providers that say whether a key is set and never show one, the
// deep list's own switch, and Anthropic directly on offer.
{
  const fail = (what) => { console.error(what); process.exit(1); };
  runInContext(`MODEL_LISTS = {
    providers: [{ id: 1, name: "Anthropic", url: "https://api.anthropic.com/v1", key_set: true, local: false, api_key: "sk-ant-fictional" },
                { id: 2, name: "Ollama", url: "http://localhost:11434/v1", key_set: false, local: true }],
    everyday: [{ id: 10, provider: 1, provider_name: "Anthropic", model: "<b>claude</b>", enabled: true, local: false, thinks: true },
               { id: 11, provider: 2, provider_name: "Ollama", model: "qwen2.5:7b", enabled: false, local: true, thinks: false }],
    deep: [], escalate: true, serving: { model: "qwen2.5:7b", local: true } };
    PROVIDER_EDITING = null;`, context);
  const ai = runInContext("modelsSection()", context);
  for (const act of ["mlist:down:10", "mlist:on:11", "mlist:quiet:10", "mlist:think:11", "mlist:test:10", "mlist:remove:11", "mlist:add:everyday", "mlist:add:deep",
    "mlist:discover:deep", "mlist:escalate:0", "mlist:provideredit:1", "mlist:providerremove:2", "mlist:providersave"]) {
    if (!ai.includes(`data-act="${act}"`)) fail(`models: the AI screen does not offer ${act}`);
  }
  if (!/data-act="mlist:up:10"[^>]*disabled/.test(ai)) fail("models: the first entry can still move up");
  // ADR 0105: each entry says whether it thinks, and the switch is a dispatched action.
  if (!/data-act="mlist:think:11"[^>]*aria-pressed="false"[^>]*>Thinks: off</.test(ai)) fail("models: an entry does not say it does not think");
  if (!/data-act="mlist:quiet:10"[^>]*aria-pressed="true"[^>]*>Thinks: on</.test(ai)) fail("models: an entry does not say it thinks");
  if (!runInContext("dispatch.toString()", context).includes('"think", "quiet"')) fail("models: the Thinks switch is not sent to the node");
  if (!ai.includes("cloud, sees your messages disguised") || !ai.includes("switched off")) fail("models: an entry does not say where it runs or that it is off");
  if (!ai.includes("key set") || !ai.includes("no key")) fail("models: a provider does not say whether a key is set");
  if (ai.includes("<b>claude</b>")) fail("models: a model name became markup");
  if (ai.includes("sk-ant-fictional")) fail("models: a key reached the screen");
  if (!ai.includes("answering now: <b>qwen2.5:7b</b>")) fail("models: what answers now is not said");
  if (!runInContext("presetOptions('')", context).includes("Anthropic (direct, your API key)")) fail("models: Anthropic directly is not offered");
  // Discover shows what it found as buttons to tap (a datalist is all but invisible on a phone),
  // kept across a re-render, under the provider it came from. A model name holds ':', so it
  // rides in a data attribute, never in data-act.
  runInContext(`FOUND_MODELS = { deep: { provider: 2, models: ["qwen2.5:14b", "<b>x</b>"] } };`, context);
  const found = runInContext("modelsSection()", context);
  if (!found.includes("Tap a model to use it") || !found.includes('data-model="qwen2.5:14b"')) fail("models: what Discover found is not offered to tap");
  if (found.includes("<b>x</b>")) fail("models: a found model name became markup");
  if (/data-act="[^"]*qwen2\.5:14b/.test(found)) fail("models: a model name went into data-act, which splits on ':'");
  if (!/id="ml-deep-provider"[^>]*>[\s\S]*?<option value="2" selected>/.test(found)) fail("models: the list forgot which provider it discovered");
  if ((found.match(/Discover models/g) || []).length !== 2) fail("models: each list does not offer Discover models");
  // A provider being changed shows which, with what it holds; one that is gone is not changed.
  runInContext("FOUND_MODELS = {}; PROVIDER_EDITING = 1;", context);
  const editing = runInContext("modelsSection()", context);
  if (!/id="pv-name"[^>]*value="Anthropic"/.test(editing) || !/id="pv-url"[^>]*value="https:\/\/api.anthropic.com\/v1"/.test(editing)) fail("models: the provider form opens blank when changing one");
  runInContext("PROVIDER_EDITING = 99;", context);
  if (!runInContext("modelsSection()", context).includes("Add a provider")) fail("models: a removed provider is still being changed");
  runInContext("MODEL_LISTS = null; PROVIDER_EDITING = null;", context);
  console.log("models: lists in order with their moves, cloud entries said to be disguised, Discover's finds to tap, and no key ever shown");
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

// A fact Endora noted about the owner stays out of the chat's activity line (it lives in About
// you); what Endora did still shows.
{
  const line = runInContext(`SHOW_ACTIVITY = true;
    activityHtml(["Noted in tech: You value reliability", "Searched the web"])`, context);
  if (line.includes("Noted in")) { console.error("chat: a noted fact is shown in the activity line"); process.exit(1); }
  if (!line.includes("Searched the web")) { console.error("chat: the activity line lost what Endora did"); process.exit(1); }
  if (runInContext(`activityHtml(["Noted in tech: You value reliability"])`, context) !== "") {
    console.error("chat: an activity line of noted facts alone still shows"); process.exit(1);
  }
}

console.log(`console check: ${named.length} screens render`);
