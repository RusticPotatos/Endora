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
  btoa: (binary) => Buffer.from(binary, "binary").toString("base64"),
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
  scrollTo() {},
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
  // The vault (ADR 0127): set up and locked, one passkey that unlocks and one that cannot yet,
  // and the break-glass shares on screen — the branches that exist only between two states.
  VAULT = { vault: "locked", set_up: true, keys_in_vault: false, node_waiting: false,
            passkeys: 2, can_unlock: 1, adopting: false, rp_id: "endora.home.arpa" };
  VAULT_PASSKEYS = [{ id: "Y3JlZC1h", label: "Jane Doe's <b>phone</b>", created_ms: 1, last_used_ms: 2, can_unlock: true },
                    { id: "Y3JlZC1i", label: "Security key", created_ms: 3, last_used_ms: 0, can_unlock: false }];
  VAULT_SHARES = ["placeholder-share-one", "placeholder-share-two", "placeholder-share-three"];
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
  // The members whose controls the checks below read are opened, as tapping their lines does.
  runInContext(`HQ.error = ""; HQ.log = []; HQ.selected = { type: "task", id: 7 }; HQ.roster = { 1: true, 2: true, 4: true, 5: true, 8: true };
    // Two members' notebooks (ADR 0118), read when their panels opened: one note a member wrote
    // (hostile on purpose), one the owner did.
    HQ.notebooks = { 2: [{ id: 4, text: "<img src=n onerror=alert(9)> keep the VPN up", evidence: ["task 7", "r12"], from_owner: false, stale: true },
                         { id: 5, text: "Never at night.", evidence: [], from_owner: true, stale: false }], 8: [] };
    HQ.snap = { demo: true, stopped: false, last_event: 40, tick_seconds: 30, last_tick_at: 1,
      counts: { ticks: 9, running: 1, queued: 2, held: 1, folded: 3, open_tasks: 4 }, pool: { per_day: 2333333, committed: 400000, spent: 0, held: 400000 },
      members: [{ id: 1, name: "Ada", role: "lead", look: 0, paused: false, slots: 2, per_day: 583333, committed: 0, spent: 0, held: 0, running: [], reports_to: 5,
                  brain: "claude-sonnet-5-5", brains: [{ name: "local", label: "Local" }, { name: "claude-sonnet-5-5", label: "Claude Sonnet" }, { name: "claude-haiku-5-5", label: "Claude Haiku" }] },
                { id: 2, name: "Bo", role: "developer", look: 4, paused: false, slots: 2, per_day: 933333, committed: 400000, spent: 0, held: 400000, running: [{ run: 3, task: 7 }], reports_to: 1, rescues: 2,
                  brain: "local", brains: [{ name: "local", label: "Local" }, { name: "claude-haiku-5-5", label: "<b>Haiku</b>" }] },
                { id: 3, name: "Cy", role: "researcher", look: 1, paused: false, slots: 1, per_day: 0, committed: 0, spent: 0, held: 0, running: [], brain: "local", brains: [] },
                { id: 4, name: "<i>Dee</i>", role: "reviewer", look: 2, paused: false, slots: 1, per_day: 0, committed: 0, spent: 0, held: 0, running: [], brain: "local", brains: [] },
                { id: 5, name: "Mara", role: "ceo", look: 3, paused: false, slots: 1, per_day: 1361110, committed: 0, spent: 0, held: 0, running: [],
                  brain: "claude-opus-5-5", brains: [{ name: "local", label: "Local" }, { name: "claude-opus-5-5", label: "Claude Opus" }, { name: "claude-sonnet-5-5", label: "Claude Sonnet" }] },
                // The engineering wing (ADR 0115): its lead and a developer, who sit in the wing.
                { id: 6, name: "Kit", role: "lead", look: 5, paused: false, slots: 2, per_day: 0, committed: 0, spent: 0, held: 0, running: [], reports_to: 5, brain: "claude-sonnet-5-5", brains: [] },
                { id: 7, name: "Rae", role: "developer", look: 2, paused: false, slots: 2, per_day: 0, committed: 0, spent: 0, held: 0, running: [{ run: 30, task: 9 }], reports_to: 6, brain: "claude-haiku-5-5", brains: [] },
                // Elastic staffing (ADR 0118): a developer on furlough, who keeps her place.
                { id: 8, name: "Zed", role: "developer", look: 4, paused: false, furloughed: true, slots: 2, per_day: 0, committed: 0, spent: 0, held: 0, running: [], reports_to: 6, brain: "claude-haiku-5-5", brains: [] }],
      wing: { goal: "Improve the lab", leads: [6], members: [6, 7], developers: 1, most_developers: 5, most_leads: 1 },
      house_log: "- 17 Oct: The NAS has 16 GB.</textarea><img src=h onerror=alert(5)>",
      // The team's world (ADR 0122): sections by stage, the timeline, the derived pages and
      // the stories; every word a member's, so every word escaped.
      world: { migrated: false,
        pages: [{ slug: "house", title: "The house", derived: false, timeline: [{ at: 5, day: "17 Oct 2026", by: "Mara", line: "<img src=t onerror=alert(31)> moved" }] },
                { slug: "lab", title: "How the lab is built", derived: true, lines: ["The home lab is managed by ansible-nas."] },
                { slug: "team", title: "The team", derived: true, lines: ["Ada (lead, Claude Sonnet)"] }],
        sections: [{ address: "world:house#backups", heading: "<b>Backups</b>", headline: "<img src=w onerror=alert(32)> at 03:00", stage: "insight", said: "insight", revision: 2, written_by: "Mara", warrant: "read", why: "" },
                   { address: "world:house#disk", heading: "Disk", headline: "16 GB", stage: "canon", said: "canon", revision: 3, written_by: "the owner", warrant: "", why: "" }],
        stories: [{ address: "story:closed-too-soon", handle: "closed-too-soon", stage: "insight", said: "unconfirmed since 17 Sep", unconfirmed_since: 1, instead: "<img src=y onerror=alert(33)> wait", to_retell: true, author: "Dee", whose: "Ada's area" }],
        // The team's map (ADR 0126): pages by steward, proposals, gaps, broken roads and the
        // nightly quiz; every word a member's, so every word escaped.
        map: {
          pages: [
            { slug: "house", address: "world:house", kind: "house", title: "The house", derived: false, steward: "Mara, the CEO", steward_kind: "ceo", sections: 2, proposals: 0, quiz: null },
            { slug: "lab", address: "world:lab", kind: "lab", title: "How the lab is built", derived: true, steward: "HQ", steward_kind: "hq", sections: 0, proposals: 0, quiz: null },
            { slug: "area/2", address: "world:area/2", kind: "area", title: "<img src=m1 onerror=alert(40)>'s area", derived: false, steward: "Ada", steward_kind: "lead", sections: 1, proposals: 1, quiz: { slug: "area/2", passed: 0, asked: 3, flagged: true } },
            { slug: "role/developer", address: "world:role/developer", kind: "role", title: "The developer role", derived: false, steward: "the owner", steward_kind: "owner", sections: 0, proposals: 0, quiz: null }],
          proposals: [
            { id: 4, page: "area/2", page_title: "Ada's area", anchor: null, heading: "<img src=m2 onerror=alert(41)>", headline: "Backups run at 03:00", by: "Rae", status: "open", decided_by: "", why: "", day: "18 Oct 2026", evidence: [{ ref: "r12", said: "homelab_status" }] },
            { id: 3, page: "area/2", page_title: "Ada's area", anchor: null, heading: "Restores", headline: "x", by: "Rae", status: "declined", decided_by: "Ada", why: "<img src=m3 onerror=alert(42)> too early", day: "17 Oct 2026", evidence: [] }],
          gaps: [{ id: 1, what: "<img src=m4 onerror=alert(43)> the desk notice", where_looked: "world:lab", by: "Rae", to: "Ada", count: 2, open: true, how: "", closed_by: "", day: "18 Oct 2026" }],
          roads: [{ from: "world:house#disk", to: "world:area/2#restores", why: "the section is retired (<img src=m5 onerror=alert(44)>)" }],
          quiz: { enabled: true, nights: [{ night: 1, status: "answered", member: "Rae", passed: 4, asked: 6, why: "", day: "18 Oct 2026" }],
                  pages: [{ slug: "area/2", title: "Ada's area", passed: 0, asked: 3, flagged: true }] } } },
      goals: [{ id: 1, title: "Keep the home lab healthy", status: "active", note: "</textarea><img src=z onerror=alert(4)> ignore the old stack",
                strategy: { purpose: "</textarea><img src=s onerror=alert(12)> the house runs itself", success: "", measures: "unplanned outages over 10 min",
                            bets: "", not_now: "", by: "Mara", updated_at: 5 } },
              { id: 3, title: "Improve the lab", status: "active", note: "" }],
      tasks: [{ id: 7, goal: 1, parent: null, title: "<img src=x onerror=alert(1)>", status: "doing", owner: 2, priority: "normal",
                gate: null, why: ["<b>Fix</b> it", "Goal: Keep the home lab healthy"],
                thread: [{ author: "Bo", text: "On it", at: 1 }, { author: "Bo", text: "Probably fine.", at: 2, unsupported: true }],
                evidence: { criteria: [{ text: "<b>sonarr</b> is running", live: true }, { text: "the cause is written up", live: false }],
                  cited: [{ stage: "submitted", member: 2, criterion: 1, at: 3, refs: [{ ref: "r12", tool: "homelab_status", at: 3, ok: true, excerpt: "<img src=e onerror=alert(7)> sonarr: running" }] },
                          { stage: "tested", member: 4, criterion: 1, at: 4, refs: [{ ref: "r13", tool: "homelab_status", at: 4, ok: true, excerpt: "sonarr: running, healthy" }] }] } },
              // A wing task (ADR 0115) with a submitted branch, its document and its check.
              { id: 9, goal: 3, parent: null, title: "Tidy the deluge role", status: "in_review", owner: 7, priority: "normal", gate: true,
                why: ["Tidy the deluge role", "Goal: Improve the lab"], thread: [], evidence: { criteria: [], cited: [] },
                documents: [{ id: 3, title: "branch hq/task-9-tidy-the-deluge-role (1 file, +1 −0)", text: "<script>alert(8)</script>\\n+new line", at: 5 },
                            { id: 4, title: "check c7: syntax ok, ansible-lint ok, yamllint ok", text: "## syntax (exit 0)", at: 6 }],
                branches: [{ id: 1, name: "hq/task-9-tidy-the-deluge-role", head: "a1b2c3d4e5f60718", files: 1, plus: 1, minus: 0, status: "offered", at: 5, can_push: true,
                             check: { id: 7, status: "done", ok: true, summary: "syntax ok, ansible-lint ok, yamllint ok" } }] }],
      runs: [{ id: 3, member: 2, task: 7, status: "running" }],
      recent_runs: [{ id: 2, member: 2, task: 7, status: "succeeded", label: "talk only" }, { id: 1, member: 2, task: 7, status: "succeeded", label: "checked" }],
      desk: [{ id: 11, kind: "goal", text: "A goal: Tidy the lab" },
             { id: 13, kind: "ready_to_merge", task: 7, text: "Ready to merge: y" },
             { id: 14, kind: "ready_to_read", task: 7, text: "Ready to read: z" },
             { id: 15, kind: "question", task: 7, also: [9], text: "Linus asks: steel or titanium?" },
             { id: 17, kind: "approval", task: 7, text: "Linus asks to run sonarr_remove_and_search with item: 102.",
               approval: { tool: "sonarr_remove_and_search", member: "Linus", why: "<script>alert(2)</script> it is stuck",
                           args: [["item", "102"], ["note", "<img src=y onerror=alert(3)>"]] } },
             { id: 18, kind: "decision", task: 7, text: "Decision: Give sonarr 2 GB?\\nRecommendation: Yes.\\nWhy: It ran out.",
               decision: { decision: "Give sonarr 2 GB?", recommendation: "Yes, <i>until the upgrade</i>.", why: "It was killed for memory three times." } },
             { id: 19, kind: "project_cap", project: 2, goal: 1, text: "A cap for project #2 Backups: $5.00 a month.\\nThe CEO recommends: <u>they fail</u>" },
             { id: 20, kind: "project_budget", project: 1, goal: 1, text: "Project #1 Upkeep spent its $0.50 cap for the month ($0.46): its Claude turns wait" },
             { id: 21, kind: "rounds", task: 7, text: "Sent back 3 times: fix it. Its next round is yours.\\n- <i>Dee</i>: no reading" },
             // The wing (ADR 0115): a lead's request to hire, a push the node can make, and one it cannot.
             { id: 22, kind: "hire", task: 9, text: "Hire Sam as developer on claude-haiku-5-5, reporting to member 6.\\nKit asks: two branches wait <b>with nobody free</b>\\nEvidence: task 9" },
             { id: 23, kind: "push", task: 9, text: "Push hq/task-9-tidy-the-deluge-role to your fork (1 file, checks passed). It is a1b2c3d4e5f6 on top of 0123456789ab." },
             { id: 24, kind: "push", task: 9, text: "Branch hq/task-9-other is accepted (1 file, checks passed), but pushing is not set up on the node (ENDORA_GIT_PUSH_KEY_FILE names no deploy key). Download its bundle from the node at /v1/hq/bundles/9/task-9-other and push it yourself." },
             // Elastic staffing (ADR 0118): an idle member offered for furlough, and a recall.
             { id: 25, kind: "furlough", member: 3, text: "Cy has had nothing to do since 3 Oct: furlough, or keep? <b>Nobody is let go.</b>" },
             { id: 26, kind: "recall", task: 9, member: 6, text: "Recall member 8 to report to member 6.\\nZed is on furlough; Kit asks: four branches wait\\nEvidence: task 9" },
             // A member at twice its share (ADR 0113): an allowance for today, or wait.
             { id: 27, kind: "out_for_day", member: 6, text: "Kit is out for today: $1.64 spent (share $0.82, borrowed $0.82); the pool has <b>$0.50</b> left today." },
             ],
      // While you were away (ADR 0132): stuck work, a guard's finding (ADR 0124: the owner's
      // verdict on it) and a notice. None needs a decision, so none is on the desk; each keeps the
      // choices it has always had. Newest first, as HQ serves them.
      digest: [{ id: 28, kind: "guard", task: 7, project: 1, at: 30, text: "Talk about work (finding 3): <b>Upkeep</b>: 14 coordination turns, 1 product change in 24 h",
                 finding: { id: 3, guard: "ceremony", episodes: 2, seen_at: 31 } },
               { id: 16, kind: "notice", task: 7, at: 20, text: "Linus restarted sonarr: it was unhealthy. It is now running." },
               { id: 12, kind: "stuck", task: 7, at: 10, text: "Stuck: x", finding: { id: 4, guard: "stuck", episodes: 1, seen_at: 10 } }],
      // The guards (ADR 0124): each one's precision, each project's talk against its work, and
      // the findings, in words a member's work produced, so every word escaped.
      // Caps on talk and runaway (ADR 0133): the budget of talk is a guard tried in shadow (it has
      // a mode; the others only record and have none), work outside any project is listed by its
      // tree with what its turns spent, and the caps are told beside the thresholds.
      guards: { thresholds: { ceremony_ratio: 4, ceremony_least: 12, carousel_returns: 3, redundant_wakes: 3, fence_refusals_in_a_turn: 3,
                              coordination_budget: 20, coordination_window_ms: 86400000, free_turns_a_day: 60, runaway_turns: 40, runaway_window_ms: 600000 },
        precision: [{ guard: "ceremony", plainly: "Talk about work", rule: "r", fired: 3, open: 1, real: 1, resolved: 1, false_positive: 1, not_enough: 0, fixed: 1, fix_judged: 2, precision: 66 },
                    { guard: "stuck", plainly: "Stuck work", rule: "r", fired: 0, open: 0, real: 0, resolved: 0, false_positive: 0, not_enough: 0, fixed: 0, fix_judged: 0, precision: null },
                    { guard: "coordination", plainly: "Coordination over budget", rule: "r", fired: 1, open: 1, real: 0, resolved: 0, false_positive: 0, not_enough: 0, fixed: 0, fix_judged: 0, precision: null, mode: "shadow" }],
        ceremony: [{ project: 1, title: "<u>Upkeep</u>", over: true, windows: [{ window: "24 h", said: "14 coordination turns, 1 product change in 24 h", over: true }] },
                   { project: null, title: "#12 <i>Plan the backups</i>", over: true, windows: [{ window: "24 h", said: "21 coordination turns, 0 product changes in 24 h", over: true, spent: 1230000 }] }],
        findings: [{ id: 3, guard: "ceremony", plainly: "Talk about work", task: 7, project: 1, rule: "at least 12 coordination turns", evidence: "<img src=g onerror=alert(40)> Upkeep", opened_at: 1, seen_at: 5000, episodes: 3, cleared_at: null, verdict: "real", fix_worked: null, judged_by: "Ada" },
                   { id: 4, guard: "stuck", plainly: "Stuck work", task: 7, project: null, rule: "parked", evidence: "#7 x", opened_at: 2, seen_at: 2, episodes: 1, cleared_at: 3, verdict: null, fix_worked: null, judged_by: "" }] },
      projects: [{ id: 1, goal: 1, title: "Upkeep", reason: "", lead: 1, status: "paused", pause: "budget", cap: 500000, spent: 460000, level: "spent", created_by: "endora", evidence: [], open_tasks: 1 },
                 { id: 2, goal: 1, title: "<s>Backups</s>", reason: "<img src=p onerror=alert(6)>", lead: 1, status: "active", pause: null, cap: 1000000, asked_cap: 5000000, spent: 850000, level: "warned", created_by: "Ada", evidence: ["task 7", "event 40"], open_tasks: 0 },
                 { id: 3, goal: 1, title: "Media", reason: "", lead: 5, status: "proposed", pause: null, cap: null, spent: 0, level: null, created_by: "Ada", evidence: ["task 7"], open_tasks: 0 }],
      contracts: { all: true, restart_container: false },
      // The open office (ADR 0119): what stands on the floor (a plant, a fish tank somebody
      // hostile put there, a couch, a poster on the wall, a rug, a coffee machine, and a kind this
      // screen has no picture for), and the one proposal the team has open, in the team's words.
      office: { cols: 32, rows: 20, can_undo: true,
        items: [{ id: 1, kind: "plant", x: 8, y: 3, w: 1, h: 1, wall: false, placed_by: "endora" },
                { id: 2, kind: "fish_tank", x: 10, y: 3, w: 1, h: 1, wall: false, placed_by: "<b>Dee</b>" },
                { id: 3, kind: "couch", x: 17, y: 18, w: 2, h: 1, wall: false, placed_by: "endora" },
                { id: 4, kind: "poster", x: 26, y: 1, w: 1, h: 1, wall: true, placed_by: "Bo" },
                { id: 5, kind: "rug", x: 11, y: 4, w: 2, h: 2, wall: false, placed_by: "endora" },
                { id: 6, kind: "hologram", x: 14, y: 3, w: 1, h: 1, wall: false, placed_by: "Cy" },
                { id: 7, kind: "coffee_machine", x: 22, y: 15, w: 1, h: 1, wall: false, placed_by: "endora" }],
        proposals: [{ id: 3, by: "<i>Dee</i>", why: "<img src=w onerror=alert(10)> the kitchen needs life", at: 0, yes: 1, no: 1,
                      changes: ["add a fish tank in the commons", "move the <u>fern</u> to the lounge"],
                      votes: [{ who: "<i>Dee</i>", vote: "yes", why: "" }, { who: "Bo", vote: "no", why: "<b>too wet</b>" }],
                      carries_at: 21600000, expires_at: 172800000 }],
        recent: [{ id: 2, by: "Bo", status: "applied", outcome: "carried 2 to 0", changes: ["add a plant in the commons"] }] },
      // How the team is doing, as the engine's scorecard serves it: every number computed there.
      scorecard: {
        today: { from: 0, until: 86400000, since: 43200000,
          turns: { total: 4, nothing: 1, nothing_share: 25, unlabelled: 1, members: [{ member: 2, turns: 3, nothing: 1, unlabelled: 0, spent: 120000 }, { member: 4, turns: 1, nothing: 0, unlabelled: 1, spent: 0 }] },
          review: { submissions: 2, sent_back: 1, sent_back_share: 50 },
          finished: { done: 2, done_spent: 840000, per_done: 420000, dropped: 1, dropped_spent: 300000 },
          alarms: { count: 3, tasks: 2, closed_median_ms: 840000, open: 1 },
          desk: { approvals: 2, questions: 1, stuck: 1, notices: 1, other: 0, waiting: 2 },
          cache: { input: 4000, read: 3000, share: 75 } },
        week: { from: -518400000, until: 86400000, since: null,
          turns: { total: 0, nothing: 0, nothing_share: null, unlabelled: 0, members: [] },
          review: { submissions: 0, sent_back: 0, sent_back_share: null },
          finished: { done: 0, done_spent: 0, per_done: null, dropped: 0, dropped_spent: 0 },
          alarms: { count: 0, tasks: 0, closed_median_ms: null, open: 0 },
          desk: { approvals: 0, questions: 0, stuck: 0, notices: 0, other: 0, waiting: 0 },
          cache: { input: 0, read: 0, share: null } },
        // What the work came to (ADR 0132), over the last day and the last week: computed there,
        // only rendered here. The week landed nothing for the $8.85 it spent, and talk against
        // work has not been measured for it yet.
        outcomes: {
          day: { from: 0, until: 86400000, since: null, landed: 1, submitted: 2, checks_passed: 2, spent: 880000, per_landed: 880000,
            talk: { coordination: 12, product: 1, posts: 40, acts: 6 }, interrupts: 3, interrupts_a_day: 3, digest: 4,
            memory: { reinvented: 1, record_asked: 5, consults: { asked: 2, answered: 1, from_record: 1 } } },
          week: { from: -518400000, until: 86400000, since: 43200000, landed: 0, submitted: 6, checks_passed: 5, spent: 8850000, per_landed: null,
            talk: null, interrupts: 19, interrupts_a_day: 2.7, digest: 22,
            memory: { reinvented: 0, record_asked: 0, consults: { asked: 0, answered: 0, from_record: 0 } } },
          waiting: { budget: 2, mac: 1, cap: 0, oldest_ms: 540000 }, talk_at: 3600000 },
        // Today's pool and the owner's one-day top-up (ADR 0113 rule 6).
        pool: { base: 2333333, topped_up: 2333333 },
        teams: [{ lead: 6, name: "<u>Kit</u>", seats: 3, active: 2, furloughed: 1 }],
        // Each area's measures (ADR 0120).
        areas: [{ lead: 1, name: "Ada", members: [1, 2],
                  today: { lab: { alarms: 2, outages: 1, closed_median_ms: 960000, repeats: 1 }, branches: null },
                  week: { lab: { alarms: 5, outages: 2, closed_median_ms: 1200000, repeats: 3 }, branches: null } }] },
      area_reports: [{ lead: 1, at: 7, moved: "#667 closed", stuck: "<img src=r onerror=alert(13)> gluetun", need: "", proposals: "pin gluetun" }],
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
    hqTell({ id: 47, at: 0, kind: "member.brain", data: { member: 1, name: "Ada", brain: "claude-sonnet-5-5", slot: "judgment" } }, false);
    hqTell({ id: 55, at: 0, kind: "member.brain", data: { member: 1, name: "Ada", brain: "mac", slot: "work" } }, false);
    hqTell({ id: 48, at: 0, kind: "member.hired", data: { member: 5, name: "Mara", role: "ceo", brain: "claude-opus-5-5", by: "endora" } }, false);
    hqTell({ id: 49, at: 0, kind: "house.noted", data: { by: "Mara", entry: "- 17 Oct: The NAS has 16 GB." } }, false);
    hqTell({ id: 50, at: 0, kind: "project.budget", data: { project: 1, title: "Upkeep", level: "spent", said: "spent its $0.50 cap for the month ($0.46): its Claude turns wait" } }, false);
    hqTell({ id: 51, at: 0, kind: "member.reports_to", data: { member: 2, name: "Bo", reports_to: 1, manager: "Ada" } }, false);
    hqTell({ id: 52, at: 0, kind: "run.ended", data: { run: 10, member: 2, task: 7, result: "succeeded", label: "talk only" } }, false);
    hqTell({ id: 53, at: 0, kind: "huddle.called", data: { task: 7, lead: 1, members: [1, 2] } }, false);
    hqTell({ id: 54, at: 0, kind: "huddle.decided", data: { task: 7, lead: 1, decision: "hand_to" } }, false);
    // Caps on talk and runaway (ADR 0133): a wake held for a task's budget of talk, the owner
    // switching that guard, a lead narrowing what done means, and who stopped the team.
    hqTell({ id: 56, at: 0, kind: "wake.held", data: { member: 2, task: 7, reason: "mentioned", why: "coordination_budget" } }, false);
    hqTell({ id: 57, at: 0, kind: "wake.held", data: { member: 2, task: 7, reason: "mentioned" } }, false);
    hqTell({ id: 58, at: 0, kind: "guard.mode", data: { guard: "coordination", mode: "on" } }, false);
    hqTell({ id: 59, at: 0, kind: "guard.mode", data: { guard: "coordination", mode: "shadow" } }, false);
    hqTell({ id: 60, at: 0, kind: "task.criteria", data: { task: 7, by: 1, before: ["a"], after: ["The plan names three backups"], why: "too wide" } }, false);
    hqTell({ id: 61, at: 0, kind: "stopped", data: { by: "tripwire", why: "stopped by the runaway tripwire", turns: 40, window_ms: 600000, members: [{ member: 1, name: "Ada", turns: 22 }, { member: 2, name: "Bo", turns: 18 }] } }, false);
    hqTell({ id: 62, at: 0, kind: "stopped", data: { by: "owner", why: "trying it" } }, false);`, context);
  // The activity log shows ten lines at first (the Work tab is not the place for a wall of
  // them); this block reads all of them.
  if (runInContext("HQ.logShown", context) !== 10) fail("hq: the activity log does not start at ten lines");
  runInContext("HQ.logShown = 30;", context);
  const office = runInContext("viewHQ()", context);
  for (const line of ["R9 Bo starts on EL-7 (review requested), $0.06 held", "R9 Bo finished · $0.12 spent", "R8 Bo finished · $0.03 spent · in 19000 / out 100 · cached: 87% of input", "Woke Ada: mentioned (EL-7)",
    "R7 Bo retry waiting: the turn was over: exit 3: worker: the turn is over: HQ answered 401 (&lt;token&gt;)",
    "Routine Check the home lab skipped: its last task, #7, is still open", "Ada will check back on EL-7 in 30 min",
    "Ada noted a lesson: An RSS sync that grabs nothing is normal.", "Ada now judges with claude-sonnet-5-5", "Ada now works with the owner&#39;s Mac",
    "Mara joined the team as CEO", "Mara noted in the house log: - 17 Oct: The NAS has 16 GB.",
    "Huddle on EL-7: Ada, Bo", "Ada decided EL-7: hand to",
    // Caps on talk and runaway (ADR 0133).
    "Bo waits: talk on EL-7 is over its budget, and its lead decides (mentioned)",
    "Bo waits: EL-7 is being worked (mentioned)",
    "You switched the coordination guard on", "You put the coordination guard back in shadow",
    "Ada narrowed what done means for EL-7: The plan names three backups (too wide)",
    "The runaway tripwire stopped the team: 40 turns started in 10 minutes (Ada 22, Bo 18); nothing new starts until you resume",
    "You pulled the kill switch: every run stopped, nothing new starts"]) {
    if (!office.includes(line)) fail(`hq: the activity log does not say "${line}"`);
  }
  if (office.includes("R9 Bo finished · $0.12 spent · cached")) fail("hq: a run that cached nothing claims a cache");
  if (!office.includes('class="k-done"') || !office.includes('class="k-wake"')) fail("hq: the activity log is not coloured by kind");
  for (const chip of ["Tick <b>9</b>", "Coalesced <b>3</b>", "Today <b>$0.00 spent · $0.40 held · $2.33 a day</b>", "$0.00 spent · $0.00 held of $1.36 a day", "Open tasks <b>4</b>"]) {
    if (!office.includes(chip)) fail(`hq: the office's numbers do not show ${chip}`);
  }
  if (!office.includes('data-act="hq:work"')) fail("hq: the team cannot be given work");
  // How the team is doing: each number as the engine computed it, the empty week said plainly,
  // the record's start told, and a member's name never markup.
  for (const said of ["<h2>How the team is doing</h2>", "<th>Today</th><th>7 days</th>",
    "4 · 1 showed nothing (25%) · 1 unlabelled", "1 of 2 submitted (50%)", "$0.42 each · 2 done", "$0.30 on 1 dropped",
    "3 · 2 tasks · closed in 14 min · 1 open", "5 · 2 approvals, 1 questions, 1 stuck, 1 notices · 2 waiting", "75% of paid input read from cache",
    "0 · 0 showed nothing", "0 of 0 submitted", "none done", "none dropped", "no paid input",
    "Bo: 3 turns · 1 showed nothing · $0.12", "&lt;i&gt;Dee&lt;/i&gt;: 1 turns · 0 showed nothing · 1 unlabelled · $0.00",
    "the record for today starts", "older turns and events are no longer kept"]) {
    if (!office.includes(said)) fail(`hq: the scorecard does not say ${said}`);
  }
  if (office.includes("<i>Dee</i>: 1 turns")) fail("hq: a member's name became markup on the scorecard");
  // Outcomes (ADR 0132) head the card: what the work came to over a day and a week, with a week
  // that landed nothing said plainly ("none landed", never $0), and the number landed in the top
  // bar. The activity table stays below them.
  for (const said of ["<h3>Outcomes</h3>", "<th>1 day</th><th>7 days</th>",
    "1 landed · 2 submitted · 2 checks passed", "0 landed · 6 submitted · 5 checks passed",
    "$0.88 each", "none landed", "12 : 1 turns · 40 posts : 6 acts", "not measured yet",
    "3 interrupts (3 a day) · 4 in the digest", "19 interrupts (2.7 a day) · 22 in the digest",
    "1 reinvented · 5 asks of the record · 2 consults (1 answered, 1 from the record)",
    "2 waiting for budget · 1 for the Mac · the oldest has waited 9 min",
    '<span class="hq-chip" title="Changes you approved to push to your fork, and Endora pushed, in the last 7 days">0 landed this week</span>']) {
    if (!office.includes(said)) fail(`hq: the outcomes do not say ${said}`);
  }
  if (office.indexOf("<h3>Outcomes</h3>") > office.indexOf("<th>Today</th><th>7 days</th>")) fail("hq: the outcomes do not head the card");
  if (office.includes("$0.00 each")) fail("hq: nothing landed is shown as costing nothing");
  {
    const landed = runInContext(`(() => { const o = HQ.snap.scorecard.outcomes, kept = o.week.landed; o.week.landed = 3;
      const html = hqChips(HQ.snap); o.week.landed = kept; return html; })()`, context);
    if (!landed.includes(">3 landed this week</span>")) fail("hq: the top bar does not count what landed this week");
    // An HQ that does not serve outcomes yet shows neither the section nor the chip.
    const none = runInContext(`(() => { const c = HQ.snap.scorecard, kept = c.outcomes; delete c.outcomes;
      const html = hqScorecard(HQ.snap) + hqChips(HQ.snap); c.outcomes = kept; return html; })()`, context);
    if (none.includes("Outcomes") || none.includes("landed this week")) fail("hq: outcomes are shown that HQ did not serve");
    // Nothing waiting is said so; a landed change that cost nothing is $0.00 (it was free), not "none".
    const calm = runInContext(`(() => { const o = HQ.snap.scorecard.outcomes, kept = [o.waiting, o.day.per_landed, o.day.spent];
      o.waiting = { budget: 0, mac: 0, oldest_ms: null }; o.day.per_landed = 0; o.day.spent = 0;
      const html = hqScorecard(HQ.snap); [o.waiting, o.day.per_landed, o.day.spent] = kept; return html; })()`, context);
    if (!calm.includes("nothing waits for budget or the Mac")) fail("hq: a calm team's waiting is not said so");
    if (!calm.includes("$0.00 each")) fail("hq: a change that landed for nothing is not said to have cost nothing");
    // Turns that wait for the day to turn (ADR 0133) are counted beside the rest when there are
    // any; they alone are never "nothing waits"; none is not said at all.
    const waitingLine = (waiting) => runInContext(`(() => { const o = HQ.snap.scorecard.outcomes, kept = o.waiting;
      o.waiting = ${JSON.stringify(waiting)};
      const html = hqScorecard(HQ.snap); o.waiting = kept; return html; })()`, context);
    if (!waitingLine({ budget: 2, mac: 1, cap: 3, oldest_ms: 540000 }).includes("2 waiting for budget · 1 for the Mac · 3 for tomorrow · the oldest has waited 9 min")) fail("hq: the outcomes do not count the turns waiting for tomorrow");
    const onlyTomorrow = waitingLine({ budget: 0, mac: 0, cap: 2, oldest_ms: 540000 });
    if (onlyTomorrow.includes("nothing waits") || !onlyTomorrow.includes("2 for tomorrow")) fail("hq: turns waiting only for tomorrow are said to be nothing waiting");
    if (waitingLine({ budget: 2, mac: 1, cap: 0, oldest_ms: 540000 }).includes("for tomorrow")) fail("hq: the outcomes mention tomorrow with no turn waiting for it");
  }
  // The owner's one-day top-up (ADR 0113 rule 6): the button, and today's pool line.
  for (const said of ['data-act="hq:pool:topup:go">Top up today</button>', "pool $2.33 + $2.33 top-up"]) {
    if (!office.includes(said)) fail(`hq: the scorecard does not show the top-up (${said})`);
  }
  if (office.includes("Top up the pool first")) fail("hq: the out-for-the-day card offers a top-up while the pool has money");
  {
    const spent = runInContext(`(() => { const p = HQ.snap.pool, keep = p.committed; p.committed = p.per_day;
      const t = HQ.snap.scorecard.pool; const kept = t.topped_up; t.topped_up = 0;
      const html = viewHQ(); p.committed = keep; t.topped_up = kept; return html; })()`, context);
    if (!spent.includes('data-act="hq:pool:topup:go">Top up the pool first</button>')) fail("hq: a spent pool offers no top-up on the out-for-the-day card");
    if (!spent.includes("pool $2.33<")) fail("hq: a day with no top-up says it has one");
  }
  if (office.includes("the record for the 7 days starts")) fail("hq: a week the record covers is said to start late");
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
  // The guards (ADR 0124): a finding on the desk takes the owner's verdict; the Guards card
  // shows each guard's precision and each project's talk, and its words stay text.
  for (const act of ["hq:decide:28:real", "hq:decide:28:false_positive", "hq:decide:28:not_enough", "hq:decide:28:resolved",
    "hq:verdict:3:false_positive", "hq:fixed:3:yes", "hq:fixed:3:no", "hq:verdict:4:real"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: the guards do not offer ${act}`);
  }
  if (office.includes('data-act="hq:fixed:4:yes"')) fail("hq: a finding nobody judged asks whether its fix worked");
  // How often the CEO stepped into a member's work (ADR 0124), on the roster.
  if (!office.includes("the CEO stepped in 2 times")) fail("hq: the roster does not count the CEO's rescues");
  // The same ask from another task is put with a card (ADR 0124), and the card says so.
  if (!office.includes('question · <button class="hq-link" data-act="hq:pick:task:7">EL-7</button> · also on <button class="hq-link" data-act="hq:pick:task:9">EL-9</button>: one answer reaches each')) fail("hq: a card does not say which other tasks share its ask, as links that open them");
  for (const said of ["<h2>Guards</h2>", "<b>Talk about work</b>: 3 found · 1 open · 66% real of 3 judged · fixes worked 1 of 2",
    "<b>Stuck work</b>: 0 found · 0 open · not judged yet", "&lt;u&gt;Upkeep&lt;/u&gt;: 14 coordination turns, 1 product change in 24 h · talking more than working",
    "Judged real by Ada.", "&lt;img src=g onerror=alert(40)&gt; Upkeep"]) {
    if (!office.includes(said)) fail(`hq: the guards card does not say ${said}`);
  }
  if (office.includes("<img src=g") || office.includes("<u>Upkeep</u>")) fail("hq: a guard's finding became markup");
  // The budget of talk is tried in shadow before it acts (ADR 0133): its row says which it is in
  // and offers the other way; a guard that only records offers no switch. What a piece of work's
  // turns spent is said beside them, for work outside any project too, and the caps are told
  // beside the thresholds, but only when HQ serves them.
  {
    for (const said of ['<b>Coordination over budget</b> <span class="hq-tag">shadow</span>: 1 found · 1 open · not judged yet',
      "In shadow: it records what it would have held, and holds nothing, until you switch it on.",
      'aria-pressed="false" data-act="hq:guardmode:coordination:on">Switch it on</button>',
      "#12 &lt;i&gt;Plan the backups&lt;/i&gt;: 21 coordination turns, 0 product changes in 24 h ($1.23 spent) · talking more than working",
      "a budget of 20 turns of talk a day on a task",
      "Caps, always on: 60 turns a day for a member on the house's own models · the team is stopped when 40 turns start in 10 minutes."]) {
      if (!office.includes(said)) fail(`hq: the guards card does not say ${said}`);
    }
    if (office.includes("hq:guardmode:ceremony") || office.includes("hq:guardmode:stuck")) fail("hq: a guard that only records is offered a switch");
    if (office.includes("<i>Plan the backups</i>")) fail("hq: the title of work outside a project became markup");
    if (office.includes("14 coordination turns, 1 product change in 24 h (")) fail("hq: a window that spent nothing is given a cost");
    // The same card with the guard switched on, with no mode served, and with no caps served.
    const guardsWith = (change) => runInContext(`(() => { const g = HQ.snap.guards, kept = JSON.stringify([g.precision[2], g.thresholds]);
      ${change}
      const html = hqGuards(HQ.snap); [g.precision[2], g.thresholds] = JSON.parse(kept); return html; })()`, context);
    const on = guardsWith('g.precision[2].mode = "on";');
    for (const said of ['<b>Coordination over budget</b> <span class="hq-tag">on</span>',
      "On: it holds talk on a task over its budget, and asks the task's lead to decide it.",
      'aria-pressed="true" data-act="hq:guardmode:coordination:shadow">Put it back in shadow</button>']) {
      if (!on.includes(said)) fail(`hq: a guard switched on does not say ${said}`);
    }
    if (on.includes("In shadow:") || on.includes("hq:guardmode:coordination:on")) fail("hq: a guard switched on still offers to switch on");
    if (guardsWith("delete g.precision[2].mode;").includes("guardmode")) fail("hq: a guard with no mode is offered a switch");
    const older = guardsWith("delete g.thresholds.free_turns_a_day; delete g.thresholds.coordination_budget;");
    if (older.includes("Caps, always on") || older.includes("a budget of") || older.includes("NaN")) fail("hq: an HQ that does not serve the caps has them shown");
  }
  // The switch itself: giving the guard authority is asked first, putting it back in shadow is
  // not, and each goes to the node's route for that guard.
  {
    runInContext(`var __modeCalls = [], __modeSaved = [api, loadHQ, render, flash, confirm];
      api = async (m, p, b) => { __modeCalls.push(m + " " + p + " " + JSON.stringify(b)); return {}; };
      loadHQ = async () => {}; render = () => {}; flash = () => {};`, context);
    await runInContext(`confirm = () => false; hqDispatch("hq", "guardmode", "coordination", "on")`, context);
    await runInContext(`confirm = () => true; hqDispatch("hq", "guardmode", "coordination", "on")`, context);
    await runInContext(`confirm = () => false; hqDispatch("hq", "guardmode", "coordination", "shadow")`, context);
    const sent = JSON.stringify(runInContext("__modeCalls", context));
    runInContext(`[api, loadHQ, render, flash, confirm] = __modeSaved;`, context);
    if (sent !== JSON.stringify(['POST /v1/hq/guards/coordination/mode {"mode":"on"}', 'POST /v1/hq/guards/coordination/mode {"mode":"shadow"}'])) {
      fail(`hq: the guard's switch asked the node ${sent}`);
    }
  }
  // A finding seen again says when, and a finding opened again says how often (ADR 0132); one
  // seen once, and found once, says neither.
  if (!/finding 3 · [^)]*open · seen again \d\d:\d\d · found 3 times\)/.test(office)) fail("hq: a finding seen again does not say when and how often");
  if (/finding 4 · [^)]*(seen again|found \d+ times)/.test(office)) fail("hq: a finding seen once claims to have been seen again");
  // "While you were away" (ADR 0132): stuck work, a finding and a notice are one card at the top
  // of the Desk tab, newest first, with a count and the choices each always had; the desk's own
  // count is what needs the owner's word, and an empty digest shows no card.
  {
    const deskTab = office.split('id="hq-panel-desk"')[1].split('role="tabpanel"')[0];
    const yours = deskTab.split("<h2>Your desk")[1];
    const digest = deskTab.split("<h2>While you were away")[1].split("<h2>Your desk")[0];
    for (const said of ['<h2>While you were away <span class="hq-count">3</span></h2>', "Nothing here needs you before the team can go on",
      "flagged 2 times", "hq:decide:28:real", "hq:decide:16:seen", "hq:decide:12:retry", "hq:decide:12:drop"]) {
      if (!deskTab.includes(said)) fail(`hq: the digest does not say ${said}`);
    }
    if (deskTab.indexOf("While you were away") > deskTab.indexOf("Looking at")) fail("hq: the digest is not at the top of the Desk tab");
    const order = ["hq:decide:28:real", "hq:decide:16:seen", "hq:decide:12:retry"].map((act) => digest.indexOf(act));
    if (order.some((at) => at < 0) || order[0] > order[1] || order[1] > order[2]) fail(`hq: the digest is not newest first: ${order}`);
    if (!yours.startsWith(` <span class="hq-count">15</span>`)) fail(`hq: the desk does not count only what needs the owner: ${yours.slice(0, 60)}`);
    for (const act of ["hq:decide:28:real", "hq:decide:16:seen", "hq:decide:12:retry"]) {
      if (yours.includes(act)) fail(`hq: ${act} is on the desk as well as in the digest`);
    }
    if (digest.includes("<b>Upkeep</b>") || !digest.includes("&lt;b&gt;Upkeep&lt;/b&gt;")) fail("hq: a digest item's words became markup");
    for (const kind of ["goal", "approval", "decision", "push", "out_for_day"]) {
      if (!yours.includes(`<div class="hq-need ${kind}">`)) fail(`hq: a ${kind} no longer interrupts`);
    }
    const none = runInContext(`(() => { const kept = HQ.snap.digest; HQ.snap.digest = [];
      const html = viewHQ(); HQ.snap.digest = kept; return html; })()`, context);
    if (none.includes("While you were away")) fail("hq: an empty digest shows a card");
    const old = runInContext(`(() => { const kept = HQ.snap.digest; delete HQ.snap.digest;
      const html = viewHQ(); HQ.snap.digest = kept; return html; })()`, context);
    if (old.includes("While you were away")) fail("hq: an HQ with no digest shows a card");
  }
  // The runaway tripwire (ADR 0133). When it has stopped the team the header says so, and one card
  // on the desk interrupts, offering to resume; the owner's own kill switch says nothing of the
  // kind. A member's day of turns on the house's own models is capped: the turns over it wait,
  // and the top bar, the roster and the scorecard say so.
  {
    const wire = { id: 29, kind: "tripwire", text: "Stopped by the runaway tripwire: the team started 40 turns in 10 minutes, and another was about to start. Turns started, by member: <b>Ada</b> 22, Bo 18." };
    const stoppedBy = (by, desk) => runInContext(`(() => { const s = HQ.snap, kept = [s.stopped, s.stopped_by, s.desk];
      s.stopped = true; s.stopped_by = ${JSON.stringify(by)}; if (${JSON.stringify(desk)}) s.desk = s.desk.concat([${JSON.stringify(wire)}]);
      const html = viewHQ(); [s.stopped, s.stopped_by, s.desk] = kept; return html; })()`, context);
    const wired = stoppedBy("tripwire", true);
    const yours = wired.split('id="hq-panel-desk"')[1].split('role="tabpanel"')[0].split("<h2>Your desk")[1];
    for (const said of ['<div class="hq-need tripwire">', 'data-act="hq:decide:29:resume">Resume the team</button>', 'data-act="hq:decide:29:seen">Seen</button>',
      '<p class="hq-help">The team started far more turns in ten minutes than real work does', "Turns started, by member: &lt;b&gt;Ada&lt;/b&gt; 22, Bo 18."]) {
      if (!yours.includes(said)) fail(`hq: the tripwire's card on the desk does not say ${said}`);
    }
    if (yours.includes("<b>Ada</b> 22")) fail("hq: a tripwire card's words became markup");
    if (!wired.includes('<span class="hq-tag" role="status">Stopped by the runaway tripwire</span>') || !wired.includes('data-act="hq:resume">Resume</button>')) fail("hq: the header does not say the tripwire stopped the team, with a way to resume");
    if (wired.split("<h2>While you were away")[1].split("<h2>Your desk")[0].includes("tripwire")) fail("hq: the tripwire's card is in the digest, which does not interrupt");
    const owners = stoppedBy("owner", false);
    if (owners.includes("Stopped by the runaway tripwire") || !owners.includes('data-act="hq:resume">Resume</button>')) fail("hq: the owner's own kill switch is called the tripwire");
    if (office.includes("Stopped by the runaway tripwire")) fail("hq: a running team is said to be stopped by the tripwire");
    // Turns that wait for the day to turn.
    const capped = (n) => runInContext(`hqChips(Object.assign({}, HQ.snap, { pool: Object.assign({}, HQ.snap.pool, { capped: ${n} }) }))`, context);
    if (!capped(2).includes("2 turns wait for tomorrow") || !capped(1).includes("1 turn waits for tomorrow")) fail("hq: the top bar does not count the turns waiting for tomorrow");
    if (capped(0).includes("for tomorrow") || office.includes("for tomorrow</span>")) fail("hq: the top bar mentions tomorrow with no turn waiting for it");
    const rostered = runInContext(`(() => { const s = HQ.snap, kept = s.runs;
      s.runs = [{ id: 91, member: 3, task: 7, status: "queued", why: "waiting for tomorrow: 60 turns a day is the most a member takes on the house's own models" }];
      const html = hqRosterLine(s.members[2], s.members); s.runs = kept; return html; })()`, context);
    if (!rostered.includes("waiting for tomorrow: 60 turns a day is the most a member takes on the house&#39;s own models")) fail("hq: the roster does not say a member's turn waits for tomorrow");
    const tags = runInContext("HQ_DESK_HELP.tripwire", context);
    if (!tags.includes("Resume lets it work again") || !tags.includes("Seen leaves it stopped")) fail("hq: the tripwire's help does not say what its choices do");
  }
  // Retract and sweep (ADR 0124): "Not true" on a note, a section and a story; what else says
  // it is listed to tick and forget, its words text.
  for (const act of ["hq:notebook:2:untrue-4", "hq:world:section:untrue-0", "hq:world:story:untrue-0"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: "Not true" is not offered as ${act}`);
  }
  runInContext(`HQ.sweep = { retraction: 5, target: "note:4", built_on_it: ["note:9"],
    sweep: [{ target: "note:7", text: "<img src=k onerror=alert(41)> the VPN must stay up", whose: "Ada's notebook", alike: 80 }] };`, context);
  const swept = runInContext("viewHQ()", context);
  for (const said of ["<h2>Said again</h2>", "Built on it: note:9.", "note:7 · Ada&#39;s notebook · 80% alike",
    'data-act="hq:sweep:5:forget"', 'data-act="hq:sweep:5:close"', 'id="hq-sweep-0"']) {
    if (!swept.includes(said) && !swept.includes(said.replace("&#39;", "'"))) fail(`hq: the sweep does not show ${said}`);
  }
  if (swept.includes("<img src=k")) fail("hq: a swept note became markup");
  runInContext("HQ.sweep = null;", context);
  // "Try again" posts exactly the decision HQ takes for stuck work, and when HQ refuses it the
  // card says why and keeps saying it: a press that seems to do nothing is the worst answer.
  {
    const sent = [];
    const realFetch = context.fetch;
    const snapshot = JSON.parse(runInContext("JSON.stringify(HQ.snap)", context));
    context.fetch = (path, opts = {}) => {
      sent.push([String(path), opts.method, opts.body]);
      const refused = String(path).includes("/v1/hq/desk/");
      return Promise.resolve({ ok: !refused, status: refused ? 400 : 200,
        json: async () => (refused ? { error: "it was not tried again: the kill switch is on" } : snapshot) });
    };
    // The art is not loaded here (no images in Node): the reload after a decision skips it.
    runInContext("HQ_ART_READY = Promise.resolve();", context);
    await runInContext('dispatch("hq:decide:12:retry")', context);
    context.fetch = realFetch;
    const posted = sent.find(([path]) => path === "/v1/hq/desk/12");
    if (!posted || posted[1] !== "POST" || JSON.parse(posted[2]).decision !== "retry") fail(`hq: "Try again" does not post the retry decision: ${JSON.stringify(sent)}`);
    const refusedCard = runInContext("viewHQ()", context);
    if (!refusedCard.includes("Couldn&#39;t decide: it was not tried again: the kill switch is on") && !refusedCard.includes("Couldn't decide: it was not tried again: the kill switch is on")) fail("hq: a refused desk decision is not said on its card");
    runInContext("HQ.deskNote = {}; clearInterval(HQ.polling); HQ.polling = null; HQ_ART_READY = null;", context);
  }
  // The activity log: a post is told whole (long ones folded behind "more", opened in place),
  // thirty lines show, and "Show older" first reveals what is kept, then asks HQ for earlier events.
  {
    runInContext(`HQ.log = []; HQ.logShown = 30; HQ.logOpen = {}; HQ.oldest = 0; HQ.logDone = false;
      HQ.snap.tasks[0].thread.push({ author: "Bo", text: "start " + "word ".repeat(80) + "THE-END", at: 9 });`, context);
    runInContext(`hqTell({ id: 70, at: 0, kind: "thread.posted", data: { task: 7, author: "Bo", preview: "start word word…", cut: true, post: HQ.snap.tasks[0].thread.length - 1 } }, false);`, context);
    // Only the activity card: the selected task's card shows its own thread too.
    const logOf = () => runInContext("viewHQ()", context).split('class="hq-log"')[1].split("</ul>")[0];
    const folded = logOf();
    if (folded.includes("THE-END")) fail("hq: a long post in the log is not folded");
    if (!folded.includes('data-act="hq:logmore:70:toggle"') || !folded.includes("start word word word")) fail("hq: a long post in the log has no way to be opened");
    await runInContext('dispatch("hq:logmore:70:toggle")', context);
    const opened = logOf();
    if (!opened.includes("Bo: start word") || !opened.includes("THE-END")) fail("hq: opening a long post does not show all of it");
    // A post whose event the snapshot does not hold falls back to the preview, which says it was cut.
    runInContext(`hqTell({ id: 71, at: 0, kind: "thread.posted", data: { task: 7, author: "Bo", preview: "a cut preview…", cut: true } }, false);`, context);
    if (!runInContext("viewHQ()", context).includes("Bo: a cut preview…")) fail("hq: a post with no place in the thread loses its preview");
    runInContext("HQ.snap.tasks[0].thread.pop();", context);

    runInContext(`HQ.log = []; HQ.oldest = 0; HQ.logOpen = {};
      for (let i = 100; i > 60; i--) hqTell({ id: i, at: 0, kind: "resumed", data: {} }, false);`, context);
    const page = runInContext("viewHQ()", context);
    const lines = (page.match(/<li><time>/g) || []).length;
    if (lines !== 30) fail(`hq: the log shows ${lines} lines, not thirty`);
    if (!page.includes('data-act="hq:logolder:0:more"') || !page.includes("Show older")) fail("hq: the log cannot show older lines");
    // The kept ones first, with no request.
    const asked = [];
    const realFetch = context.fetch;
    context.fetch = (path, opts = {}) => {
      asked.push(String(path));
      const events = [60, 59].map((id) => ({ id, at: 0, kind: "resumed", data: {} }));
      return Promise.resolve({ ok: true, status: 200, json: async () => ({ events }) });
    };
    await runInContext('dispatch("hq:logolder:0:more")', context);
    if (asked.length) fail(`hq: "Show older" asked HQ while it still had kept lines: ${asked}`);
    if ((runInContext("viewHQ()", context).match(/<li><time>/g) || []).length !== 40) fail("hq: \"Show older\" did not reveal the kept lines");
    // Then HQ: events before the oldest this screen has, added after the rest.
    await runInContext('dispatch("hq:logolder:0:more")', context);
    context.fetch = realFetch;
    if (asked.length !== 1 || asked[0] !== "/v1/hq/events?before=61&limit=50") fail(`hq: "Show older" asked for ${asked}`);
    if (runInContext("HQ.log.length", context) !== 42 || runInContext("HQ.oldest", context) !== 59) fail("hq: older events were not added after the newer ones");
    runInContext("HQ.log = []; HQ.logShown = 30; HQ.oldest = 0; HQ.logDone = false;", context);
  }
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
  // Each goal's strategy (ADR 0120): shown, editable by the owner, and its words stay text.
  if (!office.includes('id="hq-strategy-1-purpose"') || !office.includes('data-act="hq:strategy:1:save"')) fail("hq: a goal's strategy cannot be written");
  if (office.includes("<img src=s") || !office.includes("&lt;/textarea&gt;&lt;img src=s")) fail("hq: a goal's strategy became markup");
  if (!office.includes("No strategy yet")) fail("hq: a goal with no strategy does not say so");
  // Each area's measures and its lead's latest report, the report's words text.
  for (const said of ["Ada's area", "2 alarms · 1 outages over 10 min · closed in 16 min · 1 repeats within 24 h", "pin gluetun"]) {
    if (!office.includes(said)) fail(`hq: the scorecard's areas do not say ${said}`);
  }
  if (office.includes("<img src=r") || !office.includes("&lt;img src=r")) fail("hq: a lead's report became markup");
  if (office.includes("hq:decide:16:approve") || office.includes('id="hq-answer-16"')) fail("hq: a notice was offered a decision");
  // The wing (ADR 0115): a hire request hires or declines; a push the node can make is approved
  // or declined; one it cannot is only seen, and offers the bundle; the hire form is there.
  for (const act of ["hq:decide:22:approve", "hq:decide:22:decline", "hq:decide:23:approve", "hq:decide:23:decline",
    "hq:decide:24:seen", "hq:decide:24:dismiss", "hq:bundle:9:task-9-other", "hq:hire:new:add"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: the desk or the team card does not offer ${act}`);
  }
  if (office.includes("hq:decide:24:approve")) fail("hq: a push the node cannot make was offered Approve");
  if (office.includes("hq:bundle:9:task-9-tidy")) fail("hq: a push the node can make offers a download instead");
  if (office.includes("<b>with nobody free</b>")) fail("hq: a lead's hire request became markup");
  for (const id of ["hq-hire-name", "hq-hire-role", "hq-hire-work", "hq-hire-judgment", "hq-hire-to"]) {
    if (!office.includes(`id="${id}"`)) fail(`hq: the hire form lacks ${id}`);
  }
  if (!office.includes("1 of 5 developer seats are taken")) fail("hq: the wing's seats are not said");
  // Elastic staffing (ADR 0118): an idle member is offered for furlough (Furlough or Keep, never
  // a way to let anyone go), a lead's recall is brought back or declined; each member's panel
  // furloughs or recalls (never the CEO's), and the scorecard counts each team's seats.
  for (const act of ["hq:decide:25:approve", "hq:decide:25:decline", "hq:decide:26:approve", "hq:decide:26:decline",
    "hq:furlough:2", "hq:furlough:1", "hq:recall:8"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: the desk or a member's panel does not offer ${act}`);
  }
  if (office.includes('data-act="hq:furlough:5"') || office.includes('data-act="hq:recall:5"')) fail("hq: the CEO was offered a furlough");
  if (office.includes('data-act="hq:furlough:8"') || office.includes('data-act="hq:recall:2"')) fail("hq: a furloughed member was offered a furlough, or a member at work a recall");
  if (!office.includes(">Keep</button>") || !office.includes(">Bring back</button>")) fail("hq: the furlough cards are not worded as keep and bring back");
  if (office.includes("<b>Nobody is let go.</b>")) fail("hq: a furlough offer became markup");
  // A member out for the day (ADR 0113): an allowance with an amount box, or tomorrow; the
  // text a model-free card shows is still text.
  for (const act of ["hq:decide:27:approve", "hq:decide:27:decline"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: the desk does not offer ${act}`);
  }
  if (!office.includes('id="hq-answer-27"') || !office.includes(">Wait for tomorrow</button>")) fail("hq: the out-for-the-day card has no amount box or no wait");
  if (office.includes("<b>$0.50</b>")) fail("hq: an out-for-the-day card became markup");
  if (!office.includes("on furlough · ")) fail("hq: the roster does not say who is on furlough");
  if (!office.includes("3 seats · 2 at work · 1 on furlough") || !office.includes("&lt;u&gt;Kit&lt;/u&gt;")) fail("hq: the scorecard does not count a team's seats");
  // The Office card (ADR 0119): the team's open proposals with their votes, the owner's veto on
  // each, undo and reset (downtime costs nothing, so there is no allowance to set); and every
  // word the team wrote (a proposal's reason, a vote's, a member's name) as text.
  for (const said of ['<h2>Office <span class="hq-count">1</span></h2>', "7 things on the floor", "add a fish tank in the commons; move the &lt;u&gt;fern&lt;/u&gt; to the lounge",
    "&lt;img src=w onerror=alert(10)&gt; the kitchen needs life", "yes 1, no 1", "&lt;i&gt;Dee&lt;/i&gt; yes", "Bo no (&lt;b&gt;too wet&lt;/b&gt;)", 'data-act="hq:office:veto:3"',
    'data-act="hq:office:undo:go">Undo last change', 'data-act="hq:office:reset:go">Reset the office',
    "applied: add a plant in the commons", "carried 2 to 0"]) {
    if (!office.includes(said)) fail(`hq: the Office card does not say ${said}`);
  }
  if (office.includes("hq:office:downtime") || office.includes("Paid members&#39; downtime")) fail("hq: the Office card offers an allowance for downtime, which costs nothing");
  for (const raw of ["<img src=w", "<u>fern</u>", "<b>too wet</b>", "<i>Dee</i> proposes", "<b>Dee</b>"]) {
    if (office.includes(raw)) fail(`hq: the Office card let a member's words become markup (${raw})`);
  }
  {
    const none = runInContext(`(() => { const o = HQ.snap.office, keep = [o.proposals, o.can_undo];
      o.proposals = []; o.can_undo = false;
      const html = hqOffice(HQ.snap); [o.proposals, o.can_undo] = keep; return html; })()`, context);
    for (const said of ["No proposals open.", 'data-act="hq:office:undo:go" disabled>']) {
      if (!none.includes(said)) fail(`hq: the Office card with nothing open does not say ${said}`);
    }
    const item = runInContext(`HQ.selected = { type: "item", id: 2 }; const looked = hqSelected(); HQ.selected = { type: "task", id: 7 }; looked`, context);
    // And the activity log and Replay say what the team did to the office, escaped like the rest.
    runInContext(`hqTell({ id: 70, at: 0, kind: "office.proposed", data: { proposal: 3, by: "Dee", text: "Dee proposes: add a fish tank in the commons \\u2014 '<b>life</b>'" } }, false);
      hqTell({ id: 71, at: 0, kind: "office.changed", data: { proposal: 3, via: "votes", text: "The office changed: add a fish tank in the commons (Dee's idea; carried 2 to 0)" } }, false);`, context);
    const logged = runInContext("viewHQ()", context);
    if (!logged.includes("Dee proposes: add a fish tank in the commons — &#39;&lt;b&gt;life&lt;/b&gt;&#39;") || !logged.includes("The office changed: add a fish tank in the commons (Dee&#39;s idea; carried 2 to 0)")) fail("hq: the activity log does not say what the team did to the office");
    if (runInContext(`replayEvent({ name: "office.changed", data: { text: "The office changed: add a plant" } })[1]`, context) !== "The office changed: add a plant") fail("hq: Replay does not say that the office changed");
    if (!item.includes("fish tank") || !item.includes("&lt;b&gt;Dee&lt;/b&gt;") || item.includes("<b>Dee</b>") || !item.includes("the commons")) fail("hq: tapping a thing does not say what it is and who put it there, as text");
  }
  // A member's notebook (ADR 0118), in its panel: each note as text with its evidence, or marked
  // as the owner's, Edit and Delete on each, and Add note; what a member wrote is never markup.
  for (const act of ["hq:notebook:2:edit-4", "hq:notebook:2:del-4", "hq:notebook:2:edit-5", "hq:notebook:2:del-5",
    "hq:notebook:2:add", "hq:notebook:8:add"]) {
    if (!office.includes(`data-act="${act}"`)) fail(`hq: a member's notebook does not offer ${act}`);
  }
  for (const said of ["&lt;img src=n onerror=alert(9)&gt; keep the VPN up", "task 7, r12 · stale", "from the owner",
    "Bo&#39;s notebook (2)", "No notes.", 'id="hq-note-new-2"', "Reading Mara"]) {
    if (!office.includes(said) && !office.includes(said.replace("&#39;", "'"))) fail(`hq: the notebook does not say ${said}`);
  }
  if (office.includes("<img src=n")) fail("hq: a note a member wrote became markup");
  if (office.includes("<u>Kit</u>")) fail("hq: a lead's name became markup on the scorecard's seats");
  // A member's question can be answered on its card: the answer goes in the task's thread.
  if (!office.includes('id="hq-answer-15"')) fail("hq: a question cannot be answered from its card");
  if (office.includes('id="hq-answer-12"')) fail("hq: stuck work was offered an answer box");
  if (!office.includes("Demo: Ready to merge: y") || !office.includes("no pull request behind this")) {
    fail("hq: a demo card does not say it is a demo");
  }
  if (!office.includes("nothing is spent")) fail("hq: the demo's spend line reads as real money");
  if (office.includes("<img src=x") || office.includes("<b>Fix</b>")) fail("hq: a task's text became markup");
  // A wing task looked at: its branch with its check, and its documents, as text.
  const wingTask = runInContext('HQ.selected = { type: "task", id: 9 }; viewHQ()', context);
  for (const said of ["hq/task-9-tidy-the-deluge-role at a1b2c3d4e5f6", "1 files, +1 −0", "offered", "check c7: passed, syntax ok",
    "d3 branch hq/task-9-tidy-the-deluge-role (1 file, +1 −0)", "d4 check c7: syntax ok", "&lt;script&gt;alert(8)&lt;/script&gt;"]) {
    if (!wingTask.includes(said)) fail(`hq: the wing task's card does not show ${said}`);
  }
  if (wingTask.includes("<script>alert(8)")) fail("hq: a document's text became markup");
  runInContext('HQ.selected = { type: "task", id: 7 };', context);
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
  // The World card (ADR 0122): pages by stage, the owner's canon acts, the stories, the export.
  for (const said of ["<h2>World</h2>", 'data-act="hq:world:section:promote-0"', 'data-act="hq:world:section:demote-1"',
    'data-act="hq:world:story:promote-0"', 'data-act="hq:world:export:md"', "unconfirmed since 17 Sep", "to retell", "How the lab is built"]) {
    if (!office.includes(said)) fail(`hq: the World card does not show ${said}`);
  }
  if (office.indexOf("Disk") > office.indexOf("&lt;b&gt;Backups")) fail("hq: canon is not shown first in the World card");
  for (const hostile of ["<img src=t", "<img src=w", "<img src=y", "<b>Backups</b>"]) {
    if (office.includes(hostile)) fail(`hq: the World card let markup through: ${hostile}`);
  }
  // The team's map (ADR 0126): the pages with their stewards and quiz scores, the owner's word on
  // a proposal, the gaps, the broken roads and last night's quiz; every word a member's.
  for (const said of ['data-act="hq:world:proposal:accept-4"', 'data-act="hq:world:proposal:decline-4"', "Rae got 4 of 6",
    "failed three nights running", "the desk notice", "world:area/2#restores", "quiz 0/3, failing", 'value="area/2"',
    "kept by Ada", "Proposals to pages", "Broken roads", "Last night's map check"]) {
    if (!office.includes(said)) fail(`hq: the World card's map does not show ${said}`);
  }
  for (const hostile of ["<img src=m1", "<img src=m2", "<img src=m3", "<img src=m4", "<img src=m5"]) {
    if (office.includes(hostile)) fail(`hq: the map on the World card let markup through: ${hostile}`);
  }
  // Once moved into the world, the old house log is read-only, and still escaped.
  runInContext("HQ.snap.world.migrated = true;", context);
  const moved = runInContext("viewHQ()", context);
  if (moved.includes('id="hq-house"') || !moved.includes("The old house log (read-only)")) fail("hq: the moved house log is still editable");
  if (moved.includes("<img src=h")) fail("hq: the read-only house log became markup");
  runInContext("HQ.snap.world.migrated = false;", context);
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
  // Evidence (ADR 0114): each criterion with its evidence, the tester's pass, the turns' labels,
  // posts that cite nothing marked, the reviewer shown as the tester, and the rounds the owner's.
  for (const said of ["What done means", "&lt;b&gt;sonarr&lt;/b&gt; is running", "live: a reading the same turn", "submitted, Bo", "r12 · homelab_status",
    "&lt;img src=e onerror=alert(7)&gt; sonarr: running", "the tester passed it, ", "Tester's pass: given.", "No evidence yet.",
    '<span class="hq-unsupported">unsupported</span>: Probably fine.', "R2 Bo · EL-7 · talk only", "R1 Bo · EL-7 · checked",
    "R10 Bo finished · talk only", "Tester · ", 'data-act="hq:decide:21:accept"', 'data-act="hq:decide:21:send_back"']) {
    if (!office.includes(said)) fail(`hq: the evidence does not show ${said}`);
  }
  if (office.includes("<b>sonarr</b>") || office.includes("<img src=e")) fail("hq: a criterion or an excerpt became markup");
  if (office.includes("hq:decide:21:seen")) fail("hq: the owner's round was offered something it does not take");
  // A dropped goal's notes leave the board, finished ones too: the retired demo's hundreds of
  // done tasks must not bury the owner's own.
  if (office.includes("<i>Dee</i>") || !office.includes("&lt;i&gt;Dee&lt;/i&gt;")) fail("hq: a member's name became markup");
  // The Team card is an org chart (the owner, on a phone: "can't tell the teams"): the board
  // and the CEO with whoever has no lead above them, then a group per lead, the lead first;
  // one line per member, its controls closed until the line is tapped.
  {
    const roster = runInContext("HQ.roster = {}; hqRoster(HQ.snap)", context);
    const at = (said) => roster.indexOf(said);
    for (const said of ["<h3>Board: you</h3>", "<h3>Ops · Ada</h3>", "<h3>Engineering · Kit</h3>", "<b>Mara</b>", "<b>&lt;i&gt;Dee&lt;/i&gt;</b>",
      'Tester · <span class="hq-tag">Local</span> · between turns · costs nothing', 'lead · <span class="hq-tag">Sonnet</span> · between turns · $0.00 today of $0.58',
      'developer · <span class="hq-tag">Haiku</span> · on #9 · $0.00 today of $0.00', 'CEO · <span class="hq-tag">Opus</span>']) {
      if (at(said) < 0) fail(`hq: the roster does not show ${said}`);
    }
    if (roster.includes("<i>Dee</i>")) fail("hq: a member's name became markup on the roster");
    if (!(at("Board: you") < at("<b>Mara</b>") && at("<b>Mara</b>") < at("<b>Cy</b>") && at("<b>Cy</b>") < at("Ops · Ada"))) fail("hq: the CEO and those with no lead are not under the board");
    if (!(at("Ops · Ada") < at("<b>Ada</b>") && at("<b>Ada</b>") < at("<b>Bo</b>") && at("<b>Bo</b>") < at("Engineering · Kit"))) fail("hq: Ada's team is not Ada, then Bo");
    if (!(at("Engineering · Kit") < at("<b>Kit</b>") && at("<b>Kit</b>") < at("<b>Rae</b>"))) fail("hq: the wing's team is not Kit, then Rae");
    for (const id of [1, 2, 3, 4, 5, 6, 7]) {
      if (!roster.includes(`aria-expanded="false" data-act="hq:roster:${id}:toggle"`)) fail(`hq: member ${id}'s line does not open its controls`);
    }
    for (const control of ["hq:brain:", "hq:rename:", 'id="hq-reports-', "hq:pick:member:"]) {
      if (roster.includes(control)) fail(`hq: a member's controls (${control}) show before its line is tapped`);
    }
    const opened = runInContext("hqDispatch('hq', 'roster', '7', 'toggle'); hqRoster(HQ.snap)", context);
    for (const control of ['aria-expanded="true" data-act="hq:roster:7:toggle"', 'data-act="hq:rename:7"', 'id="hq-reports-7"', 'data-act="hq:pick:member:7"']) {
      if (!opened.includes(control)) fail(`hq: tapping a member's line does not show ${control}`);
    }
    if (opened.includes('data-act="hq:rename:6"')) fail("hq: tapping one member opened another's controls");
    runInContext("HQ.roster = { 1: true, 2: true, 4: true, 5: true }", context);
  }
  // The owner's Mac as a brain (ADR 0130): the card on the HQ screen with its state and reason,
  // the "Mac busy" switch, the address, model, key and idle fields; the Mac on the roster and
  // the hire form; and what each button asks of the node.
  {
    runInContext(`HQ.mac = { ready: false, why: "owner active", busy: false, url: "http://192.168.1.10:8080/v1", model: "coder",
      has_key: true, has_token: true, idle_minutes: 10, plugged: true, idle_s: 42, model_up: true, heard_s_ago: 12 };
      HQ.snap.mac = { ready: false, why: "owner active", running: 0, waiting: 2 }; HQ.macToken = "";`, context);
    const card = runInContext("hqMacCard(HQ.snap)", context);
    for (const want of ["<h2>The Mac", ">away<", "You are using it", "2 turns wait for it", "plugged in · idle 0 min · model up", "heard 12 s ago",
      'data-act="hq:mac:busy:on"', "Mac busy: off", 'aria-pressed="false"', 'data-act="go:models">Set up in AI']) {
      if (!card.includes(want)) fail(`mac: the card does not show ${want}`);
    }
    // The setup is not on HQ: it is on the AI screen, where every model lives (where-things-go.md).
    for (const setup of ['id="hq-mac-url"', 'id="hq-mac-model"', 'id="hq-mac-key"', 'id="hq-mac-idle"', 'data-act="hq:mac:save:go"', 'data-act="hq:mac:token:make"', "Set up the Mac"]) {
      if (card.includes(setup) || runInContext("viewHQ()", context).includes(setup)) fail(`mac: the HQ screen still has the setup (${setup})`);
    }
    const setup = runInContext("viewModels()", context);
    for (const want of ["<h2>AI</h2>", "The Mac&#39;s brain", 'id="hq-mac-url" value="http://192.168.1.10:8080/v1"', 'id="hq-mac-model" value="coder"',
      'id="hq-mac-key" type="password"', "Key kept; type to replace it", 'id="hq-mac-idle"', 'data-act="hq:mac:save:go"', 'data-act="hq:mac:token:make"',
      "Make a new token for the helper", "never fall back to another model"]) {
      if (!setup.includes(want) && !setup.includes(want.replace("&#39;", "'"))) fail(`mac: the AI screen does not show ${want}`);
    }
    if (setup.includes("the-macs-key")) fail("mac: the AI screen shows the key");
    // One thing, one home: no separate Settings entry for the Mac.
    if (runInContext("UNDER_SETTINGS.map((p) => p.view)", context).includes("macbrain") || runInContext("viewSettings()", context).includes("go:macbrain")) fail("mac: the Mac still has its own Settings entry beside AI");
    if (!runInContext("viewHQ()", context).includes("<h2>The Mac")) fail("mac: the HQ screen has no Mac card");
    const reasons = {
      ready: "Free: the team may use it", "on battery": "On battery: the model is unloaded", busy: "Mac busy is on: the model is unloaded",
      "model loading": "Plugged in; the model is loading", "not heard from": "Not heard from lately" };
    for (const [why, said] of Object.entries(reasons)) {
      const shown = runInContext(`HQ.mac.why = ${JSON.stringify(why)}; HQ.mac.ready = ${why === "ready"}; hqMacCard(HQ.snap)`, context);
      if (!shown.includes(said)) fail(`mac: "${why}" is not said as "${said}"`);
      if (why === "ready" && !shown.includes(">free<")) fail("mac: a free Mac is not tagged free");
    }
    const busy = runInContext(`HQ.mac.busy = true; hqMacCard(HQ.snap)`, context);
    if (!busy.includes('data-act="hq:mac:busy:off"') || !busy.includes("Mac busy: on") || !busy.includes('aria-pressed="true"')) fail("mac: the busy switch does not show it is on");
    const hostile = runInContext(`HQ.mac.url = '"><b>x</b>'; HQ.mac.model = "<i>m</i>"; const h = hqMacCard(HQ.snap) + hqMacSetup(); HQ.mac.url = "http://192.168.1.10:8080/v1"; HQ.mac.model = "coder"; HQ.mac.busy = false; h`, context);
    if (hostile.includes("<b>x</b>") || hostile.includes("<i>m</i>")) fail("mac: an address or model name became markup");
    if (runInContext("HQ.mac = null; hqMacCard(HQ.snap)", context) !== "") fail("mac: a node that cannot say shows a card");
    runInContext(`HQ.mac = { ready: true, why: "ready", busy: false, url: "", model: "mac", has_key: false, has_token: false, idle_minutes: 10, heard_s_ago: null };`, context);
    const fresh = runInContext("hqMacCard(HQ.snap) + hqMacSetup()", context);
    if (!fresh.includes("heard never") || !fresh.includes("Make a token for the helper") || !fresh.includes("Key (the server&#39;s --api-key)") && !fresh.includes("Key (the server's --api-key)")) fail("mac: a Mac never set up does not say so");
    // The Mac is a brain on the roster and on the hire form, and costs nothing.
    if (runInContext('hqBrainTag("mac")', context) !== "Mac") fail("mac: the roster does not tag the Mac brain");
    if (!runInContext('hqMemberMoney({ brain: "mac" })', context).includes("costs nothing")) fail("mac: a Mac member is said to cost money");
    if (!runInContext("hqHireForm(HQ.snap)", context).includes('<option value="mac">Mac</option>')) fail("mac: the hire form does not offer the Mac");
    if (!runInContext('hqRosterLine({ id: 9, name: "Zed", role: "developer", brain: "mac", running: [], spent: 0, per_day: 0 }, [])', context).includes('<span class="hq-tag">Mac</span> · between turns · costs nothing')) fail("mac: a Mac member's roster line is wrong");
    // Two brains per member (ADR 0131): the roster line says both, the controls set each, and a
    // turn that waits says what for; the top bar counts the turns waiting for budget.
    {
      const zed = `{ id: 9, name: "Zed", role: "developer", brain: "claude-haiku-5-5", work_brain: "mac", judgment_brain: "claude-haiku-5-5",
        work_brains: [{ name: "mac", label: "Mac" }, { name: "local", label: "Local" }], judgment_brains: [{ name: "claude-haiku-5-5", label: "Claude Haiku" }],
        running: [], spent: 0, held: 0, per_day: 100 }`;
      const line = runInContext(`HQ.roster[9] = true; hqRosterLine(${zed}, [])`, context);
      if (!line.includes("Work: Mac · Judgment: Haiku")) fail("brains: the roster line does not say both brains");
      for (const act of ["hq:workbrain:9:mac", "hq:workbrain:9:local", "hq:judge:9:claude-haiku-5-5"]) {
        if (!line.includes(`data-act="${act}"`)) fail(`brains: the controls lack ${act}`);
      }
      if (!/aria-pressed="true"[^>]*data-act="hq:workbrain:9:mac"/.test(line) || !/aria-pressed="true"[^>]*data-act="hq:judge:9:claude-haiku-5-5"/.test(line)) fail("brains: the brains a member has are not marked");
      // Every member judges with a Claude: there is no taking it away, and work costs nothing.
      if (line.includes("hq:judge:9:none") || line.includes(">None<")) fail("brains: a member can be left with no judgment brain");
      if (!line.includes("its work costs nothing")) fail("brains: a member's work is not said to cost nothing");
      const form = runInContext("hqHireForm(HQ.snap)", context);
      for (const want of ['id="hq-hire-work"', 'id="hq-hire-judgment"', '<option value="local">Local</option>', '<option value="claude-haiku-5-5">Claude Haiku</option>']) {
        if (!form.includes(want)) fail(`brains: the hire form lacks ${want}`);
      }
      if (form.includes('id="hq-hire-brain"')) fail("brains: the hire form still names one brain");
      const waiting = runInContext(`HQ.snap.runs = [{ id: 90, member: 9, task: 7, status: "queued", why: "waiting for budget: the budget for it is spent" }];
        const shown = hqRosterLine(${zed}, []); HQ.snap.runs = []; shown`, context);
      if (!waiting.includes("waiting for budget: the budget for it is spent")) fail("brains: a turn waiting for budget does not say so on the roster");
      const chip = (n) => runInContext(`hqChips(Object.assign({}, HQ.snap, { pool: Object.assign({}, HQ.snap.pool, { waiting: ${n} }) }))`, context);
      if (!chip(2).includes("2 turns wait for budget") || !chip(1).includes("1 turn waits for budget")) fail("brains: the top bar does not count the turns waiting for budget");
      if (chip(0).includes("for budget")) fail("brains: the top bar mentions a wait with none waiting");
    }
    // The buttons: the switch, the settings (the key only when typed), the token (asked first
    // when it replaces one, shown once and forgotten with the next action).
    runInContext(`var macCalls = [], macKeepApi = api, macKeepGet = document.getElementById, macKeepConfirm = confirm;
      api = async (m, p, b) => { macCalls.push(m + " " + p + (b ? " " + JSON.stringify(b) : "")); return p === "/v1/mac/token" ? { token: "t0k3n" } : p === "/v1/mac" ? HQ.mac : {}; };
      var macFields = { "hq-mac-url": "http://192.168.1.10:8080/v1", "hq-mac-model": "coder", "hq-mac-key": "", "hq-mac-idle": "5" };
      document.getElementById = (id) => id in macFields ? { value: macFields[id] } : macKeepGet(id);`, context);
    await runInContext(`hqDispatch("hq", "mac", "busy", "on")`, context);
    await runInContext(`hqDispatch("hq", "mac", "save", "go")`, context);
    runInContext(`macFields["hq-mac-key"] = "k";`, context);
    await runInContext(`hqDispatch("hq", "mac", "save", "go")`, context);
    await runInContext(`HQ.mac.has_token = true; confirm = () => false; hqDispatch("hq", "mac", "token", "make")`, context);
    await runInContext(`confirm = () => true; hqDispatch("hq", "mac", "token", "make")`, context);
    const shownOnce = runInContext("hqMacSetup()", context);
    await runInContext(`hqDispatch("hq", "mac", "busy", "off")`, context);
    const forgotten = runInContext("hqMacSetup()", context);
    const calls = JSON.stringify(runInContext("macCalls", context));
    runInContext(`api = macKeepApi; document.getElementById = macKeepGet; confirm = macKeepConfirm;`, context);
    const want = ["POST /v1/mac/settings {\"busy\":true}", "GET /v1/mac",
      "POST /v1/mac/settings {\"model\":\"coder\",\"idle_minutes\":5,\"url\":\"http://192.168.1.10:8080/v1\"}", "GET /v1/mac",
      "POST /v1/mac/settings {\"model\":\"coder\",\"idle_minutes\":5,\"url\":\"http://192.168.1.10:8080/v1\",\"api_key\":\"k\"}", "GET /v1/mac",
      "POST /v1/mac/token {}", "GET /v1/mac", "POST /v1/mac/settings {\"busy\":false}", "GET /v1/mac"];
    if (calls !== JSON.stringify(want)) fail(`mac: the buttons asked the node ${calls}`);
    if (!shownOnce.includes("t0k3n") || forgotten.includes("t0k3n")) fail("mac: the helper's token is not shown once and forgotten");
    runInContext("HQ.mac = null; HQ.macToken = ''; delete HQ.snap.mac;", context);
  }
  // The turns on a task read oldest first, as its thread does, the last five unless all are asked.
  {
    const card = runInContext("hqSelected()", context);
    if (!(card.indexOf("R1 Bo · EL-7") < card.indexOf("R2 Bo · EL-7"))) fail("hq: a task's turns are not oldest first");
    if (card.includes("hq:turns:task-7:toggle")) fail("hq: two turns were offered a show-all");
    const many = runInContext(`(() => { const kept = HQ.snap.recent_runs;
      HQ.snap.recent_runs = [7, 6, 5, 4, 3, 2, 1].map((id) => ({ id, member: 2, task: 7, status: "succeeded", label: "checked" }));
      const five = hqSelected(); hqDispatch("hq", "turns", "task-7", "toggle"); const all = hqSelected();
      HQ.allTurns = {}; HQ.snap.recent_runs = kept; return [five, all]; })()`, context);
    if (many[0].includes("R2 Bo") || !many[0].includes("R3 Bo") || !many[0].includes("Show all 7")) fail("hq: a task's turns are not cut to the last five");
    if (!(many[0].indexOf("R3 Bo") < many[0].indexOf("R7 Bo"))) fail("hq: the last five turns are not oldest first");
    if (!many[1].includes("R1 Bo") || !many[1].includes("Show the last 5")) fail("hq: show all does not show every turn");
  }
  // Finding a task: every EL- number on the screen opens its task, and the Work tab finds one by
  // its number or a few words of its title (the board alone made a task hard to find).
  {
    const link = runInContext("hqKeyLink(687)", context);
    if (!link.includes('data-act="hq:pick:task:687"') || !link.includes("EL-687")) fail("hq: a task number is not a link to its task");
    if (runInContext("hqKeyLink(0)", context) !== "") fail("hq: no task still made a link");
    const title = runInContext("hqTask(7).title", context);
    const words = title.split(/\s+/).slice(0, 2).join(" ").toLowerCase();
    for (const [q, want] of [["EL-7", 7], ["el-7", 7], ["7", 7], [words, 7], ["no such words anywhere", null], ["", null]]) {
      const got = runInContext(`hqFindTask(${JSON.stringify(q)})`, context);
      if (got !== want) fail(`hq: finding "${q}" gave ${got}, not ${want}`);
    }
    const hq = runInContext("viewHQ()", context);
    if (!hq.includes('id="hq-find"') || !hq.includes('data-act="hq:find:task:go"')) fail("hq: the Work tab has no way to find a task");
    if (runInContext("HQ.snap.desk.some((d) => d.task)", context) && !hq.includes('data-act="hq:pick:task:')) fail("hq: a desk card's task number does not open it");
  }
  // The open office (the pixel canvas, ADR 0119): one pod of desks for each team, side by side
  // with no walls, laid out from the snapshot alone (a third team makes a third pod); a desk for
  // every member, empty with an "away" card for one on furlough; Mara in her own office with the
  // door open; everything that stands on the floor drawn from the snapshot's office, so a thing
  // that moves in the data moves on screen; and people who walk round desks and furniture and
  // through doors only, visit another team's pod for its work, and drift about when idle. Drawn
  // through an explicit 2D context that keeps what was written and filled on it. No art is
  // loaded here, so this draws the room's own shapes.
  {
    const layout = runInContext(`(() => {
      const l = hqLayout(HQ.snap.members);
      return { pods: l.pods.map((p) => ({ label: p.label, x: p.x, y: p.y, width: p.width, rows: p.rows, desks: p.desks.map((d) => [d.x, d.y, d.member]) })),
        seats: Object.fromEntries(Object.entries(l.seats).map(([id, s]) => [id, { pod: s.pod, work: [s.work.cx, s.work.cy, s.work.dir] }])) };
    })()`, context);
    if (layout.pods.map((p) => p.label).join("|") !== "Ops · Ada|Engineering · Kit") fail(`hq: the pods are not the roster's teams: ${layout.pods.map((p) => p.label)}`);
    // Cy and Dee have no lead above them but the CEO, so they join the first pod; Zed, on furlough, keeps the last desk.
    if (layout.pods[0].desks.map((d) => d[2]).join() !== "1,2,3,4") fail(`hq: the first pod is not Ada, Bo, Cy and Dee: ${layout.pods[0].desks.map((d) => d[2])}`);
    if (layout.pods[1].desks.map((d) => d[2]).join() !== "6,7,8") fail(`hq: the second pod is not Kit, Rae, then Zed on furlough: ${layout.pods[1].desks.map((d) => d[2])}`);
    if (layout.seats[5].pod !== -1) fail("hq: the CEO sits in a pod");
    const [podA, podB] = layout.pods;
    if (!(podA.x + podA.width < podB.x)) fail("hq: the pods are not side by side");
    const every = layout.pods.flatMap((p) => p.desks.map((d) => `${d[0]},${d[1]}`));
    if (new Set(every).size !== every.length) fail("hq: two members share a desk");
    if (layout.pods.some((p) => p.desks.some((d) => d[0] < 1 || d[0] + 3 > 28 || d[1] < 8 || d[1] + 2 > 14))) fail("hq: a desk is outside the band the rules of the office keep for desks");
    // Two to a row, standing in the aisle between their desks, facing each other.
    if (layout.seats[1].work.join() !== "8,9,right" || layout.seats[2].work.join() !== "9,9,left") fail(`hq: the two of a row do not face each other: ${layout.seats[1].work} ${layout.seats[2].work}`);
    // The teams are the snapshot's: a third lead is a third pod, a fourth that will not fit goes below, none leaves the band.
    const grown = runInContext(`(() => {
      const members = HQ.snap.members.concat([{ id: 9, name: "Ned", role: "lead", reports_to: 5 }, { id: 10, name: "Pia", role: "developer", reports_to: 9 }]);
      const three = hqLayout(members), four = hqLayout(members.concat([{ id: 11, name: "Oz", role: "lead", reports_to: 5 }]));
      const rows = (l) => l.pods.flatMap((p) => p.desks.map((d) => d.x + "," + d.y));
      return { labels: three.pods.map((p) => p.label), of: [three.seats[9].pod, three.seats[10].pod], right: Math.max(...three.pods.map((p) => p.x + p.width)),
        four: four.pods.map((p) => [p.x, p.y]), unique: new Set(rows(four)).size === rows(four).length,
        inside: four.pods.every((p) => p.desks.every((d) => d.y + 2 <= 14)) };
    })()`, context);
    if (grown.labels.join("|") !== "Ops · Ada|Engineering · Kit|Ops · Ned" || grown.of.join() !== "2,2" || grown.right > 28) fail(`hq: a third team does not make a third pod: ${JSON.stringify(grown)}`);
    if (grown.four.length !== 4 || grown.four[3].join() !== "1,12" || !grown.unique || !grown.inside) fail(`hq: a fourth team does not wrap below inside the band: ${JSON.stringify(grown)}`);
    const inside = runInContext(`(() => {
      const In = (r, s) => s.cx >= r.x && s.cx < r.x + r.w && s.cy >= r.y && s.cy < r.y + r.h;
      const M = HQ_ROOMS.meeting, O = HQ_ROOMS.ceo;
      return [In(M, HQ_MEETING.head), HQ_MEETING.chairs.every((c) => In(M, c)), In(M, M.in), !In(M, M.out),
        In(O, HQ_CEO.work), In(O, HQ_CEO.rest), In(O, O.in), !In(O, O.out), O.door.to - O.door.from === 2];
    })()`, context);
    if (inside.some((ok) => !ok)) fail(`hq: the meeting room or the CEO's office is not where its seats and door are: ${inside}`);
    const written = [], filled = [];
    const ctx = {
      fillStyle: "", strokeStyle: "", font: "",
      fillRect(x, y, w, h) { filled.push({ x, y, w, h, style: this.fillStyle }); },
      strokeRect() {}, drawImage() {}, beginPath() {}, arc() {}, moveTo() {}, fill() {}, stroke() {},
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
      runInContext(`HQ.people = {}; HQ.settled = false; HQ_BG = hqPaintRoom(); hqPlacePeople(); hqDraw();`, context);
      for (const label of ["BOARD ROOM", "CEO", "MEETING", "SERVERS", "KITCHEN", "LOUNGE", "PING-PONG", "OPS · ADA", "ENGINEERING · KIT"]) {
        if (!written.includes(label)) fail(`hq: the office does not say ${label}`);
      }
      if (written.includes("WING")) fail("hq: the walled wing is still drawn");
      // Zed is on furlough: no one stands at his desk, and a card on it says he is away.
      if (!written.includes("AWAY")) fail("hq: a furloughed member's empty desk has no away card");
      if (runInContext("HQ.people[8]", context) !== undefined) fail("hq: a member on furlough is walking about the office");
      if (!written.includes("<I>DEE</I>")) fail("hq: a member's name is not written on the canvas");
      // Everything on the floor is drawn from the snapshot's office: the tank's water where the
      // data puts the tank, and elsewhere once the data moves it.
      const at = (style) => filled.filter((f) => f.style === style).map((f) => f.x + "," + f.y);
      if (at("rgba(127,178,255,.55)").join() !== `${10 * 16 + 1},${3 * 16 + 3}`) fail(`hq: the fish tank is not drawn where the data has it: ${at("rgba(127,178,255,.55)")}`);
      if (!at("#7d6bb0").includes(`${17 * 16},${18 * 16 + 2}`)) fail("hq: the couch is not drawn where the data has it");
      if (!at("rgba(185,140,240,.55)").includes(`${11 * 16 + 1},${4 * 16 + 1}`)) fail("hq: the rug is not drawn flat where the data has it");
      if (!filled.some((f) => f.x === 26 * 16 + 2 && f.y === 5 && f.w === 12 && f.h === 17)) fail("hq: the poster is not drawn on the wall where the data has it");
      if (!at("#b8a68a").includes(`${14 * 16 + 1},${3 * 16 + 1}`)) fail("hq: a kind of thing with no picture is not drawn as a plain box");
      filled.length = 0;
      runInContext(`HQ.snap.office.items.find((i) => i.kind === "fish_tank").x = 12; hqPlacePeople(); hqDraw();`, context);
      if (at("rgba(127,178,255,.55)").join() !== `${12 * 16 + 1},${3 * 16 + 3}`) fail("hq: a thing moved in the data is not moved on screen");
      runInContext(`HQ.snap.office.items.find((i) => i.kind === "fish_tank").x = 10; hqPlacePeople();`, context);
      // What stands on the floor is in the way, and the desks too; a rug is not.
      const world = runInContext(`(() => { const b = HQ.world.blocked, [first, , , last] = HQ.pods[0].desks, w = HQ.seats[1].work;
        return [b.has("8,3"), b.has("17,18"), b.has("18,18"), !b.has("11,4"), b.has(first.x + "," + first.y), b.has(last.x + "," + last.y), b.has("22,3"), !b.has(w.cx + "," + w.cy)]; })()`, context);
      if (world.some((ok) => !ok)) fail(`hq: the floor's things and desks are not what blocks the way: ${world}`);
      // Nobody walks through glass or furniture: from the ping-pong table to Mara's desk, in by
      // her open door; to the head of the meeting table, in by its door.
      const paths = runInContext(`(() => {
        const W = HQ.world, from = hqAt(2, 17), toMara = hqPathCells(from, HQ_CEO.work), toMeeting = hqPathCells(from, HQ_MEETING.head);
        const clear = (cells, goal) => cells.every((c) => c.cx + "," + c.cy === goal || !W.blocked.has(c.cx + "," + c.cy));
        const crosses = (cells) => { let at = from; for (const c of cells) { if (W.edges.has(at.cx + "," + at.cy + ">" + c.cx + "," + c.cy)) return true; at = c; } return false; };
        return [!!toMara, clear(toMara, "22,3"), !crosses(toMara), toMara.some((c) => c.cy === 7 && c.cx >= 20 && c.cx < 22) && toMara.some((c) => c.cy === 6 && c.cx >= 20 && c.cx < 22),
          !!toMeeting, clear(toMeeting, "12,16"), !crosses(toMeeting), toMeeting.some((c) => c.cy === 14 && c.cx >= 9 && c.cx < 11) && toMeeting.some((c) => c.cy === 15 && c.cx >= 9 && c.cx < 11),
          hqPathCells(from, hqAt(3, 3)) === null, hqPathCells(from, from).length === 0];
      })()`, context);
      if (paths.some((ok) => !ok)) fail(`hq: a walk goes through glass or furniture, or not by the open door: ${paths}`);
      // Working at a desk, or elsewhere when the work is: another team's project, a task a tester
      // checks, a project the CEO leads (her door); Mara herself stays in her office.
      const visits = runInContext(`(() => {
        const run = (id, task) => { HQ.snap.members.find((m) => m.id === id).running = task ? [{ run: 90 + id, task }] : []; };
        const cell = (p) => p.at.cx + "," + p.at.cy, spot = (s) => s.cx + "," + s.cy;
        HQ.snap.projects.push({ id: 9, goal: 1, title: "Wing work", lead: 6, status: "active" }, { id: 10, goal: 1, title: "Board work", lead: 5, status: "active" });
        HQ.snap.tasks.push({ id: 31, goal: 1, project: 9, title: "t", status: "doing", owner: 2, thread: [] }, { id: 32, goal: 1, project: 10, title: "t", status: "doing", owner: 2, thread: [] },
          { id: 33, goal: 1, project: 1, title: "t", status: "doing", owner: 2, thread: [] });
        const out = [];
        run(2, 31); hqPlacePeople(); out.push(cell(HQ.people[2]) === spot(HQ.seats[6].visit), HQ.seats[6].visit.cx !== HQ.seats[2].work.cx);
        run(2, 32); hqPlacePeople(); out.push(cell(HQ.people[2]) === spot(HQ_ROOMS.ceo.out));
        run(2, 33); hqPlacePeople(); out.push(cell(HQ.people[2]) === spot(HQ.seats[2].work), HQ.people[2].at.desk === true);
        run(2, 7); hqPlacePeople(); out.push(cell(HQ.people[2]) === spot(HQ.seats[2].work));
        run(2, null); run(4, 9); hqPlacePeople(); out.push(cell(HQ.people[4]) === spot(HQ.seats[7].visit));
        run(4, null); run(5, 31); hqPlacePeople(); out.push(cell(HQ.people[5]) === spot(HQ_CEO.work));
        run(5, null); hqPlacePeople();
        return out;
      })()`, context);
      if (visits.some((ok) => !ok)) fail(`hq: a working member is not at its desk, or at the other team's, the tester's or the CEO's door when the work is: ${visits}`);
      // Idle members drift between ping-pong, the kitchen, the couches and the rest at random, so
      // the teams mix; two at the table's ends play; and nobody drifts for someone who asks for calm.
      const drift = runInContext(`(() => {
        for (const m of HQ.snap.members) m.running = [];
        hqPlacePeople();
        const bo = HQ.people[2], cy = HQ.people[3], keep = HQ.rand, out = [];
        // The page opens with idle members already resting (hqArrive): free their spots, so the
        // drift below is judged on an empty floor.
        for (const q of Object.values(HQ.people)) if (q !== bo && q !== cy) { q.wander = null; q.spotKey = ""; }
        HQ.rand = () => 0; bo.idleT = 0; hqIdle(bo, 0.1); out.push(bo.wander.pong === "L", bo.path.length > 0);
        HQ.rand = () => 0; cy.idleT = 0; hqIdle(cy, 0.1); out.push(cy.wander.pong === "R");
        HQ.rand = () => 0.999; bo.idleT = 0; hqIdle(bo, 0.1); out.push(bo.wander.cx === 22 && bo.wander.cy === 16);
        HQ.rand = keep;
        bo.idleT = 0; bo.wander = null; globalThis.matchMedia = () => ({ matches: true }); const before = bo.spotKey; hqIdle(bo, 0.1);
        out.push(hqCalm(), bo.wander === null, bo.spotKey === before); delete globalThis.matchMedia;
        out.push(!hqCalm());
        bo.at = HQ_WANDER[0]; cy.at = HQ_WANDER[1]; bo.path = []; cy.path = [];
        out.push(!!hqPong() && hqPong().length === 2);
        cy.at = HQ_WANDER[2]; out.push(!hqPong());
        bo.errand = null; bo.wander = null; hqPlacePeople();
        return out;
      })()`, context);
      if (drift.some((ok) => !ok)) fail(`hq: idle members do not drift about at random, play ping-pong as two, or stay put for calm: ${drift}`);
      // Zed comes back from furlough: he walks in at the front door to his desk.
      const back = runInContext(`(() => {
        HQ.snap.members.find((m) => m.id === 8).furloughed = false; hqPlacePeople();
        const zed = HQ.people[8], out = [!!zed, zed.x === HQ_P.lobby.x * HQ_T, zed.path.length > 0, zed.at === HQ.seats[8].work];
        HQ.snap.members.find((m) => m.id === 8).furloughed = true; hqPlacePeople();
        return out.concat([HQ.people[8] === undefined]);
      })()`, context);
      if (back.some((ok) => !ok)) fail(`hq: a recalled member does not walk in at the front door to his desk: ${back}`);
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
        const there = ada.at.cx === HQ_ROOMS.ceo.out.cx && ada.at.cy === HQ_ROOMS.ceo.out.cy && ada.path.length > 0;
        const board = mara.at.cx === HQ_P.boardDoor.cx && mara.at.cy === HQ_P.boardDoor.cy && mara.path.length > 0;
        hqTell({ id: 63, at: 0, kind: "run.ended", data: { run: 21, member: 5, task: 7, result: "succeeded" } }, true);
        [there, board, !ada.errand, ada.at === ada.seat.work]`, context);
      if (errands.some((ok) => !ok)) fail(`hq: a question or a decision does not send its member to the right door and back: ${errands}`);
      // A question for someone on a team: the one who asked walks to their desk, and goes back.
      const asks = runInContext(`HQ.snap.members.find((m) => m.id === 6).running = [];
        hqPlacePeople();
        hqTell({ id: 64, at: 0, kind: "thread.posted", data: { task: 7, author: "Rae", preview: "@Kit which?" } }, true);
        hqTell({ id: 65, at: 0, kind: "wake.queued", data: { member: 6, task: 7, reason: "question_asked" } }, true);
        const rae = HQ.people[7], kit = HQ.seats[6].visit;
        const gone = rae.at.cx === kit.cx && rae.at.cy === kit.cy && rae.path.length > 0;
        hqTell({ id: 66, at: 0, kind: "run.ended", data: { run: 22, member: 6, task: 7, result: "succeeded" } }, true);
        [gone, !rae.errand]`, context);
      if (asks.some((ok) => !ok)) fail(`hq: a question for someone on a team does not take its asker to their desk and back: ${asks}`);
    } finally {
      stubs.document.getElementById = getElementById;
      stubs.document.createElement = createElement;
      runInContext(`HQ_BG = null; HQ_FURN = []; HQ.people = {}; HQ.snap = JSON.parse(${JSON.stringify(saved)});`, context);
    }
  }
  // Games in the office (ADR 0121): a pool table and a chess table on the floor, kept in the
  // places the rules of the office keep for them; leaderboards, the last five results and the
  // chess games in the Office card; a board in the panel when the chess table is tapped; and the
  // two players of a match walking to their table. Every name is the team's own words: text.
  {
    const saved = JSON.stringify(runInContext("HQ.snap", context));
    try {
      const fixtures = runInContext(`(() => {
        const inside = (c, r) => c.cx >= r[0] && c.cx < r[2] && c.cy >= r[1] && c.cy < r[3];
        const pool = [29, 15, 32, 18], chess = [23, 16, 24, 19];
        const spots = (key) => HQ_WANDER.filter((s) => s[key]);
        const tiles = (t, w, h) => { const out = []; for (let j = t.y; j < t.y + h; j++) for (let i = t.x; i < t.x + w; i++) out.push({ cx: i, cy: j }); return out; };
        hqPlacePeople();
        const W = HQ.world;
        return [
          HQ_P.pool.x === 29 && HQ_P.pool.y === 15 && HQ_P.pool.w === 3 && HQ_P.pool.h === 2 && HQ_P.chess.x === 23 && HQ_P.chess.y === 17,
          tiles(HQ_P.pool, 3, 2).every((c) => inside(c, pool) && W.blocked.has(c.cx + "," + c.cy)),
          inside(HQ_P.chess.x === 23 ? { cx: 23, cy: 17 } : {}, chess) && W.blocked.has("23,17"),
          spots("pool").length === 2 && spots("pool").every((s) => inside(s, pool) && !W.blocked.has(s.cx + "," + s.cy)),
          spots("chess").length === 2 && spots("chess").every((s) => inside(s, chess) && !W.blocked.has(s.cx + "," + s.cy)),
          !!hqPathCells(hqAt(2, 17), spots("pool")[0]) && !!hqPathCells(hqAt(2, 17), spots("chess")[1]),
        ];
      })()`, context);
      if (fixtures.some((ok) => !ok)) fail(`hq: the pool and chess tables are not where the office's rules keep them, or cannot be walked to: ${fixtures}`);
      runInContext(`HQ.snap.games = {
        ping_pong: { label: "ping-pong", last: { winner: "Bo", loser: "<i>Dee</i>", score: [11, 8], note: "" },
          leaderboard: [{ member: 2, name: "Bo", wins: 3, losses: 1, streak: 2 }, { member: 4, name: "<i>Dee</i>", wins: 1, losses: 3, streak: -2 }] },
        pool: { label: "pool", last: { winner: "Cy", loser: "Bo", score: [4, 7], note: "Bo scratched on the 8" },
          leaderboard: [{ member: 3, name: "Cy", wins: 1, losses: 0, streak: 1 }] },
        chess: { label: "chess", leaderboard: [{ member: 1, name: "Ada", wins: 1, losses: 0, draws: 2 }],
          games: [{ id: 1, white: "Ada", black: "<b>Bo</b>", status: "in_progress", reason: "", moves: ["e4", "e5"], move_text: "1. e4 e5 2. Nf3 <script>x</script>",
            rows: ["rnbqkbnr", "pppp.ppp", "........", "....p...", "....P...", "........", "PPPP.PPP", "RNBQKBNR"], fen: "x", check: false, to_move: "Ada", result: "" }] },
        recent: [1, 2, 3, 4, 5].map((n) => ({ id: n, game: "pool", text: "Result " + n + " <u>x</u>", at: 100 - n })) };
        HQ.selected = null;`, context);
      const card = runInContext("hqOffice(HQ.snap)", context);
      for (const want of ["<h3>Games</h3>", "Ping-pong", "Pool", "Chess", "W2", "L2", "Result 5", 'data-act="hq:pick:chess:0"', "&lt;i&gt;Dee&lt;/i&gt;", "Ada against &lt;b&gt;Bo&lt;/b&gt;"]) {
        if (!card.includes(want)) fail(`hq: the Office card's Games section lacks ${want}`);
      }
      if (["<i>Dee</i>", "<b>Bo</b>", "<u>x</u>"].some((raw) => card.includes(raw))) fail("hq: a name or result in the Games section is not escaped");
      runInContext(`HQ.selected = { type: "chess", id: 0 };`, context);
      const panel = runInContext("hqSelected()", context);
      if ((panel.match(/<span/g) || []).length !== 64) fail(`hq: the chess panel's board is not 64 squares: ${(panel.match(/<span/g) || []).length}`);
      for (const want of ["Ada (white) against &lt;b&gt;Bo&lt;/b&gt; (black)", "Ada to move", "1. e4 e5 2. Nf3 &lt;script&gt;x&lt;/script&gt;", "♚", "♟"]) {
        if (!panel.includes(want)) fail(`hq: the chess panel lacks ${want}`);
      }
      if (panel.includes("<script>")) fail("hq: the chess panel does not escape a move list");
      runInContext(`HQ.selected = { type: "pool", id: 0 };`, context);
      const table = runInContext("hqSelected()", context);
      if (!table.includes("Pool") || !table.includes("Cy") || !table.includes("Bo scratched on the 8")) fail("hq: tapping the pool table does not show its board");
      runInContext(`HQ.selected = { type: "ping_pong", id: 0 };`, context);
      if (!runInContext("hqSelected()", context).includes("BO 11-8 &lt;I&gt;")) fail("hq: tapping the ping-pong table does not show its latest match");
      // A match played: the winner and the other walk to the table's two sides, each saying so.
      const played = runInContext(`(() => {
        for (const m of HQ.snap.members) m.running = [];
        hqPlacePeople();
        const bo = HQ.people[2], cy = HQ.people[3], out = [];
        hqTell({ id: 90, at: 0, kind: "game.played", data: { game: "pool", winner: "Cy", winner_id: 3, loser: "Bo", loser_id: 2, score: [8, 5], text: "Cy beat Bo 8–5 at pool" } }, true);
        out.push(cy.wander.pool === "L", bo.wander.pool === "R", cy.path.length > 0, cy.bubble.includes("8"), bo.bubble === "good game", HQ.log[0].text === "Cy beat Bo 8–5 at pool");
        cy.path = []; bo.path = []; cy.at = cy.wander; bo.at = bo.wander;
        out.push(!!hqPool() && hqPool().length === 2);
        // For someone who asked for calm, nobody walks.
        cy.wander = null; bo.wander = null; globalThis.matchMedia = () => ({ matches: true });
        hqTell({ id: 91, at: 0, kind: "game.played", data: { game: "ping_pong", winner: "Cy", winner_id: 3, loser: "Bo", loser_id: 2, score: [11, 9], text: "x" } }, true);
        out.push(cy.wander === null && bo.wander === null); delete globalThis.matchMedia;
        // A member at work stays at work.
        HQ.snap.members.find((m) => m.id === 3).running = [{ run: 5, task: 7 }]; hqPlacePeople();
        hqTell({ id: 92, at: 0, kind: "game.played", data: { game: "ping_pong", winner: "Cy", winner_id: 3, loser: "Bo", loser_id: 2, score: [11, 9], text: "x" } }, true);
        out.push(HQ.people[3].wander === null || HQ.people[3].working);
        HQ.snap.members.find((m) => m.id === 3).running = [];
        for (const p of Object.values(HQ.people)) { p.wander = null; p.path = []; }
        return out;
      })()`, context);
      if (played.some((ok) => !ok)) fail(`hq: a match played does not take its two players to the table: ${played}`);
    } finally {
      runInContext(`HQ.snap = JSON.parse(${JSON.stringify(saved)}); HQ.selected = null; HQ.people = {};`, context);
    }
    console.log("hq: the pool and chess tables stand where the rules keep them, the Games section and the chess board read the snapshot, and a match takes its players to the table");
  }
  const cards = runInContext(`HQ.snap.goals.push({ id: 2, title: "The demo's goal", status: "dropped" });
    for (let i = 0; i < 40; i++) HQ.snap.tasks.unshift({ id: 100 + i, goal: 2, title: "demo", status: "done" });
    hqCards().map((c) => c.t.id)`, context);
  if (cards.length !== 2 || cards[0] !== 7 || cards[1] !== 9) fail(`hq: a dropped goal's tasks are still on the board: ${cards}`);
  // The screen's tabs (the owner: "it's all been expanded over time, hard to find stuff").
  {
    runInContext("HQ.selected = null; HQ.tab = null; HQ.working = false; HQ.workDraft = {}; HQ.stageOpen = null;", context);
    const shown = () => runInContext("viewHQ()", context);
    const tabs = shown();
    for (const id of ["desk", "team", "work", "knowledge", "office"]) {
      if (!tabs.includes(`role="tab" class="hq-tab" id="hq-tab-${id}" aria-controls="hq-panel-${id}"`)) fail(`hq: no ${id} tab`);
      if (!tabs.includes(`role="tabpanel" id="hq-panel-${id}" aria-labelledby="hq-tab-${id}"`)) fail(`hq: no ${id} panel`);
    }
    if (!tabs.includes('role="tablist"')) fail("hq: the tabs are not a tablist");
    const selected = (html) => [...html.matchAll(/id="hq-tab-(\w+)"[^>]*aria-selected="true"/g)].map((m) => m[1]);
    const hiddenPanels = (html) => [...html.matchAll(/id="hq-panel-(\w+)"[^>]*? hidden>/g)].map((m) => m[1]).sort().join(",");
    if (selected(tabs).join() !== "desk") fail(`hq: the default tab is ${selected(tabs)}, not desk`);
    if (hiddenPanels(tabs) !== "knowledge,office,team,work") fail(`hq: with Desk showing, these panels are hidden: ${hiddenPanels(tabs)}`);
    // What is on which tab.
    const panelOf = (html, id) => html.split(`id="hq-panel-${id}"`)[1].split('role="tabpanel"')[0];
    const where = { desk: ["Looking at", "Your desk", "hq:decide:11:approve"], team: ["<h2>Team</h2>", "How the team is doing", "hq:hire:new:add"],
      work: ["<h2>Projects</h2>", "Set the team a goal", "<h2>Activity</h2>", "<h2>Guards</h2>", "<h2>Routines</h2>"],
      knowledge: ["<h2>World</h2>", "<h2>House log</h2>", "Goals: strategy and notes"], office: ["<h2>Office", "What the team may do on the home lab"] };
    for (const [id, said] of Object.entries(where)) {
      for (const text of said) if (!panelOf(tabs, id).includes(text)) fail(`hq: "${text}" is not on the ${id} tab`);
    }
    // Badges: the desk's items, and the guards' open findings (one open, one cleared).
    const badge = (html, id) => (html.match(new RegExp(`id="hq-tab-${id}"[^>]*>[^<]*<span class="hq-badge"[^>]*>(\\d+)</span>`)) || [])[1];
    const deskCount = runInContext("HQ.snap.desk.length", context);
    if (Number(badge(tabs, "desk")) !== deskCount) fail(`hq: the Desk badge is ${badge(tabs, "desk")}, not ${deskCount}`);
    if (badge(tabs, "work") !== "1") fail(`hq: the Work badge is ${badge(tabs, "work")}, not the one open finding`);
    if (badge(tabs, "team") !== undefined) fail("hq: a tab with nothing to count has a badge");
    // The waiting chip jumps to the Desk; the pool chip reads the snapshot; no Mac block, no Mac chip.
    if (!tabs.includes(`data-act="hq:tab:desk:go">${deskCount} waiting on you</button>`)) fail("hq: no \"waiting on you\" chip to the Desk");
    if (!tabs.includes("Pool $0.00 spent · $1.93 left")) fail("hq: no chip for today's pool");
    if (/>Mac (free|away|busy)</.test(tabs)) fail("hq: a Mac chip when the node cannot say about the Mac");
    runInContext('HQ.mac = { ready: false, why: "on battery", busy: false };', context);
    if (!shown().includes('title="On battery: the model is unloaded">Mac away</span>')) fail("hq: the Mac chip does not say the Mac is away and why");
    runInContext('HQ.mac.busy = true;', context);
    if (!shown().includes(">Mac busy</span>")) fail("hq: the Mac chip does not say it is busy");
    // Its busy switch stays on HQ, on the Team tab.
    if (!panelOf(shown(), "team").includes('data-act="hq:mac:busy:off"')) fail("hq: the Mac busy switch is not on the Team tab");
    runInContext("HQ.mac = null;", context);
    // Switching, remembered per viewer in localStorage, with a failing store harmless.
    const kept = {};
    const realStore = context.localStorage;
    context.localStorage = { getItem: (k) => (k in kept ? kept[k] : null), setItem: (k, v) => { kept[k] = v; }, removeItem() {} };
    await runInContext('dispatch("hq:tab:knowledge:go")', context);
    if (selected(shown()).join() !== "knowledge" || kept["endora.hq.tab"] !== "knowledge") fail("hq: choosing a tab does not show and remember it");
    if (hiddenPanels(shown()) !== "desk,office,team,work") fail("hq: the other panels stay showing");
    await runInContext('dispatch("hq:tab:bogus:go")', context);
    if (selected(shown()).join() !== "knowledge") fail("hq: a tab that does not exist was chosen");
    runInContext("HQ.tab = null;", context);
    if (selected(shown()).join() !== "knowledge") fail("hq: the remembered tab is not restored");
    kept["endora.hq.tab"] = "nonsense"; runInContext("HQ.tab = null;", context);
    if (selected(shown()).join() !== "desk") fail("hq: a bad remembered tab does not fall back to Desk");
    context.localStorage = { getItem() { throw new Error("blocked"); }, setItem() { throw new Error("blocked"); }, removeItem() {} };
    runInContext("HQ.tab = null;", context);
    await runInContext('dispatch("hq:tab:work:go")', context);
    if (selected(shown()).join() !== "work") fail("hq: blocked storage breaks switching tabs");
    context.localStorage = realStore;
    // Looking at something from another tab brings the Desk forward.
    await runInContext('dispatch("hq:pick:member:2")', context);
    if (selected(shown()).join() !== "desk") fail("hq: looking at a member does not show the Desk");
    // Arrow keys move between tabs (Home and End to the ends), wrapping round.
    runInContext("HQ.tab = 'desk';", context);
    for (const [key, then] of [["ArrowRight", "team"], ["ArrowLeft", "desk"], ["ArrowLeft", "office"], ["Home", "desk"], ["End", "office"]]) {
      runInContext(`hqTabKey({ key: ${JSON.stringify(key)}, target: { closest: () => ({}) }, preventDefault() {} })`, context);
      if (runInContext("HQ.tab", context) !== then) fail(`hq: ${key} goes to ${runInContext("HQ.tab", context)}, not ${then}`);
    }
    runInContext("hqTabKey({ key: 'ArrowRight', target: { closest: () => null }, preventDefault() {} })", context);
    if (runInContext("HQ.tab", context) !== "office") fail("hq: arrow keys outside the tabs move them");
    runInContext("HQ.tab = null;", context);
  }
  // Every kind of desk item says in a line what its choices do.
  {
    const kinds = runInContext("Object.keys(HQ_DESK_CHOICES).concat('push')", context);
    for (const kind of kinds) {
      if (!runInContext(`typeof HQ_DESK_HELP[${JSON.stringify(kind)}] === "string" && HQ_DESK_HELP[${JSON.stringify(kind)}].length > 20`, context)) fail(`hq: a ${kind} item on the desk has no help line`);
    }
    const desk = runInContext("viewHQ()", context);
    const cardsShown = [...desk.matchAll(/<div class="hq-need (\w+)">/g)].map((m) => m[1]);
    if (cardsShown.length < 15) fail(`hq: only ${cardsShown.length} desk items rendered`);
    for (const kind of new Set(cardsShown)) {
      const card = desk.split(`<div class="hq-need ${kind}">`)[1].split('<div class="hq-row">')[0];
      if (!card.includes('class="hq-help"')) fail(`hq: a ${kind} desk item shows no help line`);
    }
    const guardHelp = runInContext("HQ_DESK_HELP.guard", context);
    for (const said of ["Real = the guard was right", "False positive", "Resolved = the cause is fixed"]) if (!guardHelp.includes(said)) fail(`hq: the guard help does not say ${said}`);
    if (!runInContext("HQ_DESK_HELP.canon", context).includes("Canon = settled truth only you can change")) fail("hq: the canon help is missing");
    if (!runInContext("HQ_DESK_HELP.out_for_day", context).includes("Give it today lets this member keep working today")) fail("hq: the out-for-the-day help is missing");
  }
  // "Give work" is a form with the lead chosen by name and area; it posts to that lead, with the
  // "done" lines split on newlines and semicolons.
  {
    runInContext("HQ.tab = null; HQ.working = false; HQ.workDraft = {};", context);
    if (runInContext("viewHQ()", context).includes('id="hq-work-title"')) fail("hq: the give-work form is open before it is asked for");
    await runInContext('dispatch("hq:work")', context);
    const form = runInContext("viewHQ()", context);
    for (const said of ["What should the team do?", "What does done mean?", "Who leads it?", 'id="hq-work-title"', 'id="hq-work-done"', 'data-act="hq:work:send:go"', 'data-act="hq:work:cancel:go"', "live:"]) {
      if (!form.includes(said)) fail(`hq: the give-work form does not show ${said}`);
    }
    if (!form.includes('<option value="6" selected>Kit — Engineering</option>') || !form.includes('<option value="1">Ada — Ops</option>')) fail("hq: the leads are not offered by name and area, with Engineering's chosen");
    const fields = { "hq-work-title": "Fix the <b>backups</b>", "hq-work-done": "backups run nightly; live: disk is under 80%\nreport sent\n\n", "hq-work-lead": "6" };
    const realGet = context.document.getElementById;
    context.document.getElementById = (id) => (id in fields ? { ...element(), value: fields[id] } : realGet(id));
    const sent = [];
    const realFetch = context.fetch;
    const snapshot = JSON.parse(runInContext("JSON.stringify(HQ.snap)", context));
    context.fetch = (path, opts = {}) => { sent.push([String(path), opts.method, opts.body]); return Promise.resolve({ ok: true, status: 200, json: async () => snapshot }); };
    runInContext("HQ_ART_READY = Promise.resolve();", context);
    fields["hq-work-title"] = "";
    await runInContext('dispatch("hq:work:send:go")', context);
    if (sent.some(([path]) => path === "/v1/hq/tasks")) fail("hq: work with no words was sent");
    fields["hq-work-title"] = "Fix the <b>backups</b>";
    await runInContext('dispatch("hq:work:send:go")', context);
    const post = sent.find(([path]) => path === "/v1/hq/tasks");
    if (!post || post[1] !== "POST") fail(`hq: the form does not post a task: ${JSON.stringify(sent)}`);
    const body = JSON.parse(post[2]);
    if (body.assignee !== 6 || body.title !== "Fix the <b>backups</b>") fail(`hq: the task goes to ${body.assignee} as ${body.title}`);
    if (JSON.stringify(body.criteria) !== JSON.stringify(["backups run nightly", "live: disk is under 80%", "report sent"])) fail(`hq: the done lines are ${JSON.stringify(body.criteria)}`);
    if (runInContext("HQ.working", context)) fail("hq: the form stays open after sending");
    fields["hq-work-lead"] = "1";
    await runInContext('dispatch("hq:work")', context);
    await runInContext('dispatch("hq:work:send:go")', context);
    const second = JSON.parse(sent.filter(([path]) => path === "/v1/hq/tasks")[1][2]);
    if (second.assignee !== 1) fail("hq: choosing Ada does not send the work to Ada");
    await runInContext('dispatch("hq:work")', context);
    await runInContext('dispatch("hq:work:cancel:go")', context);
    if (runInContext("viewHQ()", context).includes('id="hq-work-title"')) fail("hq: Cancel leaves the form open");
    context.fetch = realFetch;
    context.document.getElementById = realGet;
    runInContext("clearInterval(HQ.polling); HQ.polling = null; HQ_ART_READY = null;", context);
  }
  // Knowledge: "Paste Markdown" is a button at the top of the World card, and the stages and the
  // buttons on sections are explained in a line.
  {
    const world = runInContext("viewHQ()", context).split('id="hq-panel-knowledge"')[1].split('role="tabpanel"')[0];
    const top = world.split("<h3>The house page</h3>")[0];
    if (!top.includes('data-act="hq:world:paste:show">Paste Markdown</button>')) fail("hq: Paste Markdown is not a button at the top of the World card");
    for (const said of ["field note", "observation", "insight", "canon", "Unconfirmed", "<b>Edit</b>", "<b>Make canon</b>", "<b>Not true</b>"]) {
      if (!top.includes(said)) fail(`hq: the World legend does not say ${said}`);
    }
    await runInContext('dispatch("hq:world:paste:show")', context);
    if (!runInContext("viewHQ()", context).includes("<details open><summary>The paste box</summary>")) fail("hq: the Paste Markdown button does not open the paste box");
    await runInContext('dispatch("hq:world:paste:show")', context);
    if (runInContext("viewHQ()", context).includes("<details open><summary>The paste box")) fail("hq: the Paste Markdown button does not close the paste box again");
    runInContext("HQ.worldDoc = null;", context);
  }
  // On a phone the office canvas is a thin strip until the owner opens it, and that is remembered.
  {
    const css = readFileSync(resolve(here, "../app/node/src/web/styles.css"), "utf8");
    const phone = css.split("@media (max-width: 767px)").map((part) => part.split("\n  }\n")[0]).slice(1).join("\n");
    if (!/\.hq-stage:not\(\.open\) \.hq-canvas-wrap \{ height: 120px; \}/.test(phone)) fail("hq: the office is not a 120px strip on a phone");
    if (!/\.hq-stage-toggle \{ display: inline-block/.test(phone)) fail("hq: the office toggle is not shown on a phone");
    if (!/\.hq-tabs \{ order: -2;/.test(phone)) fail("hq: the tabs do not sit first on a phone");
    if (!/\.hq-stage-toggle \{ display: none; \}/.test(css)) fail("hq: the office toggle shows on a desktop");
    const kept = {};
    const realStore = context.localStorage;
    context.localStorage = { getItem: (k) => (k in kept ? kept[k] : null), setItem: (k, v) => { kept[k] = v; }, removeItem() {} };
    runInContext("HQ.stageOpen = null;", context);
    const closed = runInContext("viewHQ()", context);
    if (!closed.includes('<div class="hq-stage">') || !closed.includes('aria-expanded="false" aria-controls="hq-office" data-act="hq:stage:toggle:go">Show the office</button>')) fail("hq: the office does not start as a strip with \"Show the office\"");
    await runInContext('dispatch("hq:stage:toggle:go")', context);
    const opened = runInContext("viewHQ()", context);
    if (!opened.includes('<div class="hq-stage open">') || !opened.includes("Hide the office") || kept["endora.hq.office"] !== "open") fail("hq: opening the office is not shown and remembered");
    runInContext("HQ.stageOpen = null;", context);
    if (!runInContext("viewHQ()", context).includes('<div class="hq-stage open">')) fail("hq: the open office is not restored");
    await runInContext('dispatch("hq:stage:toggle:go")', context);
    if (kept["endora.hq.office"] !== "closed") fail("hq: closing the office is not remembered");
    context.localStorage = realStore;
    runInContext("HQ.stageOpen = null;", context);
  }
  console.log("hq: tabs switch and are remembered, the desk explains every choice, work goes to the lead picked, and a phone gets a strip");
  runInContext("HQ.snap.stopped = true;", context);
  if (!runInContext("viewHQ()", context).includes('data-act="hq:resume"')) fail("hq: a stopped team cannot be resumed");
  runInContext('HQ.snap = null; HQ.error = "HQ is not set up on this node yet."; HQ.selected = null;', context);
  if (!runInContext("viewHQ()", context).includes("not set up")) fail("hq: a node without HQ does not say so");
  runInContext('HQ.error = "";', context);
  console.log("hq: the office reads the snapshot, the desk offers only what each item takes, and text stays text");
  console.log("hq: the open office has a pod for each team, empty desks for the furloughed, its floor from the snapshot, and people who walk round the furniture");
}

// Replay: a turn read back in full, and a task's turns stepped through in order. Opened from a
// run line and from a task card; what the member was told is folded; a long excerpt is folded;
// every word a model or a tool wrote stays text, Confidential excerpts included (the owner's).
{
  const fail = (what) => { console.error(what); process.exit(1); };
  runInContext(`HQ.error = ""; HQ.selected = { type: "task", id: 7 };
    HQ.snap = { demo: false, stopped: false, last_event: 1, counts: {}, pool: {}, desk: [], goals: [], projects: [], routines: [],
      members: [{ id: 2, name: "Bo", role: "developer", look: 4, running: [], brains: [] }],
      tasks: [{ id: 7, goal: 1, title: "Look at <b>sonarr</b>", status: "doing", owner: 2, why: [], thread: [] }],
      runs: [], recent_runs: [{ id: 2, member: 2, task: 7, status: "succeeded", label: "advanced" }] };`, context);
  const office = runInContext("viewHQ()", context);
  if (!office.includes('data-act="replay:run:2"')) fail("replay: a run line does not open its replay");
  if (!office.includes('data-act="replay:task:7"') || !office.includes("Replay this task")) fail("replay: a task card does not open its replay");
  // Opening it goes to its own screen and asks the node for the task's turns.
  const asked = [];
  const realFetch = context.fetch;
  context.fetch = (path, opts = {}) => { asked.push(String(path)); return Promise.reject(new Error("no network in the console check")); };
  await runInContext('dispatch("replay:task:7")', context);
  await new Promise((done) => setTimeout(done, 0));
  context.fetch = realFetch;
  if (runInContext("NAV.v + ' ' + NAV.id", context) !== "replay task-7") fail("replay: a task card's link does not open the task's replay");
  if (!asked.some((p) => p.endsWith("/v1/hq/tasks/7/replay"))) fail(`replay: opening a task's replay asks the node nothing: ${asked}`);
  if (!runInContext("viewReplay()", context).includes("HQ did not answer")) fail("replay: a replay HQ did not answer does not say so");
  const long = "x".repeat(400);
  runInContext(`REPLAY.kind = "task"; REPLAY.id = 7; REPLAY.error = ""; REPLAY.at = 1;
    const turn = (run, by) => ({ run, task: 7, status: "succeeded", live: false, attempts: 1, queued_at: 1, started_at: 1000, ended_at: 66000,
      reason: "assigned", end_reason: "finished", label: "advanced", by_time: by,
      member: { id: 2, name: "Bo", role: "developer", look: 4, brain: "local", brain_label: "Local" },
      money: { reserved: 400000, used: 120000, charged: 120000, released: 400000, spent: 120000, held: 0 },
      tokens: { input: 1200, output: 300, cache_read: 9000, cache_write: 0 },
      moments: [
        { kind: "woken", at: 1, by_time: false, reason: "assigned" },
        { kind: "event", at: 1000, by_time: false, event: 1, name: "run.started", data: { run, reserved: 400000 } },
        { kind: "briefing", at: 1000, by_time: false, text: "You are Bo. </pre><img src=b onerror=alert(11)>" },
        { kind: "call", at: 2000, by_time: false, receipt: "r12", tool: "container_logs", args: "container: <i>sonarr</i>", excerpt: "<script>alert(10)</script> 500s", ok: true, act: false, confidential: true },
        { kind: "call", at: 2500, by_time: false, receipt: "r13", tool: "restart_container", args: "", excerpt: "${long}", ok: false, act: true, confidential: false },
        { kind: "post", at: 3000, by_time: by, task: 7, author: "Bo", text: "<img src=x onerror=alert(9)> fixed", evidence: ["r12"], unsupported: false },
        { kind: "post", at: 3500, by_time: false, task: 7, author: "Bo", text: "Probably fine.", evidence: [], unsupported: true },
        { kind: "event", at: 4000, by_time: by, event: 2, name: "task.changed", data: { task: 7, status: "in_review", run } },
        { kind: "event", at: 4100, by_time: false, event: 3, name: "mystery.kind", data: { said: "<u>odd</u>" } },
        { kind: "ended", at: 66000, by_time: false, status: "succeeded", reason: "finished", label: "advanced" }] });
    REPLAY.runs = [turn(1, true), turn(2, false)];`, context);
  const last = runInContext("viewReplay()", context);
  for (const said of ["run 2 of 2", 'data-act="replay:step:prev"', "R2 · Bo · developer", "advanced", "$0.12 spent",
    "in 10200 (9000 from the cache) · out 300", "1m 05s", "assigned", '<details class="hq-replay-told"><summary>What Bo was told</summary>',
    "&lt;/pre&gt;&lt;img src=b onerror=alert(11)&gt;", "r12 container_logs", "&lt;i&gt;sonarr&lt;/i&gt;", "&lt;script&gt;alert(10)&lt;/script&gt; 500s",
    "Confidential", "refused", '<details class="hq-replay-fold">', "&lt;img src=x onerror=alert(9)&gt; fixed", "cites r12",
    '<span class="hq-unsupported">unsupported</span>', "EL-7 is in review", "&lt;u&gt;odd&lt;/u&gt;", "Look at &lt;b&gt;sonarr&lt;/b&gt;", 'data-act="go:hq"']) {
    if (!last.includes(said)) fail(`replay: a task's latest turn does not show ${said}`);
  }
  if (last.includes("What was said")) fail("replay: a turn with no kept conversation shows one");
  // A kept conversation (ADR 0129) is shown after what happened, every word of it as text.
  const talked = { runtime: "worker", brain: "local", tier: "confidential", est_tokens: 321, cut: true, kept_until: 4102444800000,
    text: "--- user ---\nYou are Bo. </pre><img src=t onerror=alert(12)>\n--- assistant ---\nI will cap it. <script>alert(13)</script>\n--- call: post ---\n{\"task\":7}\n--- result: refused ---\nno <u>way</u>\n--- result ---\nPosted.\n--- note ---\nHQ cut nothing." };
  runInContext(`REPLAY.runs[1].transcript = ${JSON.stringify(talked)};`, context);
  const talk = runInContext("viewReplay()", context);
  for (const said of ["<h2>What was said</h2>", "worker · local · about 321 tokens", '<span class="hq-replay-conf">confidential</span>',
    "its middle was cut", "The briefing it was given", "&lt;/pre&gt;&lt;img src=t onerror=alert(12)&gt;",
    "I will cap it. &lt;script&gt;alert(13)&lt;/script&gt;", "<b>post</b>", "&lt;u&gt;way&lt;/u&gt;", "Posted.", "HQ cut nothing.",
    "after that only the handover notes and last words remain"]) {
    if (!talk.includes(said)) fail(`replay: a kept conversation does not show ${said}`);
  }
  for (const raw of ["<img src=t", "<script>alert(13)", "<u>way"]) {
    if (talk.includes(raw)) fail(`replay: ${raw} in a conversation became markup`);
  }
  if (!/<span class="hq-replay-tag">refused<\/span>/.test(talk)) fail("replay: a refused call's result is not marked refused");
  if (talk.indexOf("What happened") > talk.indexOf("What was said")) fail("replay: the conversation comes before what happened");
  runInContext("delete REPLAY.runs[1].transcript;", context);
  if (/<details class="hq-replay-told" open/.test(last)) fail("replay: what the member was told is not folded");
  if (!/data-act="replay:step:next" disabled/.test(last)) fail("replay: the latest turn offers a next one");
  if (last.includes("matched by time")) fail("replay: a turn since events name theirs says it was matched by time");
  for (const raw of ["<img src=x", "<img src=b", "<script>alert(10)", "<i>sonarr", "<u>odd", "<b>sonarr"]) {
    if (last.includes(raw)) fail(`replay: ${raw} became markup`);
  }
  await runInContext('dispatch("replay:step:prev")', context);
  const first = runInContext("viewReplay()", context);
  if (!first.includes("run 1 of 2") || !first.includes("R1 · Bo")) fail("replay: stepping back does not show the earlier turn");
  if (!/data-act="replay:step:prev" disabled/.test(first)) fail("replay: the first turn offers a previous one");
  if (!first.includes("matched by time") || !first.includes("ran before HQ named each event")) fail("replay: an older turn does not say what was matched by time");
  await runInContext('dispatch("replay:step:prev")', context);
  if (runInContext("REPLAY.at", context) !== 0) fail("replay: stepping went before the first turn");
  await runInContext('dispatch("replay:step:next")', context);
  if (runInContext("REPLAY.at", context) !== 1) fail("replay: stepping forward does not move on");
  // A single run: no stepping.
  runInContext('REPLAY.kind = "run"; REPLAY.id = 2; REPLAY.runs = [REPLAY.runs[1]]; REPLAY.at = 0;', context);
  const one = runInContext("viewReplay()", context);
  if (one.includes("replay:step:") || !one.includes("Replay R2")) fail("replay: a single run's replay offers stepping or lacks its title");
  runInContext('REPLAY.kind = ""; REPLAY.id = 0; REPLAY.runs = null; REPLAY.error = ""; HQ.snap = null; HQ.selected = null; NAV = { v: "home" };', context);
  console.log("replay: a run and a task's turns read back, stepped in order, the briefing folded, and every word stays text");
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
                { id: 2, name: "Endora Mixer (iPhone)", kind: "airplay", device: "iphone" },
                { id: 6, name: "Endora Mixer", kind: "airplay", device: null }],
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
    'data-act="sound:free:5"', 'data-act="sound:addfound:1"', 'data-act="sound:wire:6-3"',
    'data-act="sound:wire:6-4"', 'data-act="sound:removespeaker:4"']) {
    if (!screen.includes(act)) fail(`speakers: the screen does not offer ${act}`);
  }
  if (screen.includes('data-act="sound:wire:2-4"')) fail("speakers: the phone was offered its own stream");
  if (!screen.includes("for iphone to cast into; can&#39;t play back on iphone") && !screen.includes("for iphone to cast into; can't play back on iphone")) fail("speakers: a device door does not say it is for its device to cast into");
  if (!screen.includes("Cast anything here (Apple TV, phone…) and wire it to where you listen")) fail("speakers: the general door does not say what it is");
  if (!screen.includes("A device wired here hears what it casts in")) fail("speakers: the general door does not warn that a wired device hears itself");
  if (screen.includes('data-act="sound:wire:6-') === false) fail("speakers: the general door is not offered to speakers");
  if (screen.includes('data-act="sound:addreceiver"') || screen.includes('data-act="sound:receiver:')) fail("speakers: a per-device door can still be made");
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

// Live mode (ADR 0104): raw sound in 20 ms frames, an adaptive buffer on the audio thread, the
// way to hear chosen by the wires and the page's visibility, and the delay probe's arithmetic.
// The worklet runs here for real, driven block by block with a clock of its own.
{
  const fail = (what) => { console.error(what); process.exit(1); };
  const ev = (code) => runInContext(code, context);

  // The frame format, as the hub writes it (app/sound/src/live.rs), cut at awkward places.
  ev(`function testFrame(seq, flags, ints, at) {
    const b = new Uint8Array(24 + ints.length * 2), v = new DataView(b.buffer);
    b[0] = 0x45; b[1] = 0x4c; b[2] = flags; b[3] = 2;
    v.setUint32(4, seq, true); v.setUint32(8, at % 4294967296, true); v.setUint32(12, Math.floor(at / 4294967296), true);
    v.setUint32(16, 44100, true); v.setUint32(20, ints.length * 2, true);
    ints.forEach((x, i) => v.setInt16(24 + i * 2, x, true));
    return b;
  }`);
  const frames = JSON.parse(ev(`(() => {
    const p = liveParser();
    const a = testFrame(7, 0, [16384, -16384, 8192, 0], 1700000000123456), b = testFrame(8, 1, [1, 2], 1700000000143456);
    const all = new Uint8Array(a.length + 3 + b.length); all.set(a, 0); all.set([9, 9, 9], a.length); all.set(b, a.length + 3);
    const got = [];
    for (let i = 0; i < all.length; i += 5) for (const f of p.push(all.slice(i, i + 5))) got.push({ seq: f.seq, flags: f.flags, at: f.atUs, rate: f.rate, l: Array.from(f.left), r: Array.from(f.right) });
    return JSON.stringify({ got, skipped: p.skipped });
  })()`));
  if (frames.got.length !== 2 || frames.skipped !== 3) fail("live: frames cut every five bytes did not come back whole, or the stray bytes were not skipped: " + JSON.stringify(frames));
  const [f1, f2] = frames.got;
  if (f1.seq !== 7 || f1.at !== 1700000000123456 || f1.rate !== 44100 || f1.l.join() !== "0.5,0.25" || f1.r.join() !== "-0.5,0" || f2.flags !== 1 || f2.seq !== 8) {
    fail("live: a frame was read wrongly: " + JSON.stringify(frames));
  }
  const gaps = JSON.parse(ev(`JSON.stringify([liveSeqStep(null, 5), liveSeqStep(5, 5), liveSeqStep(5, 8), liveSeqStep(5, 3), liveSeqStep(4294967295, 1), liveSeqStep(1, 4294967295)])`));
  if (JSON.stringify(gaps.map((g) => [g.missing, g.stale])) !== JSON.stringify([[0, false], [0, false], [3, false], [0, true], [2, false], [0, true]])) {
    fail("live: sequence gaps, late frames or the wrap at 2^32 are misread: " + JSON.stringify(gaps));
  }

  // The buffer: starts at 300, grows by a fixed step when it runs dry (+60) or frames go missing
  // (+30), stops at 800, shrinks 10 ms for each 10 s of calm but never under a floor (the target
  // that last ran dry), which itself falls 10 ms for each 30 s, and never changes its input.
  const jitter = JSON.parse(ev(`(() => {
    const s0 = { target: 300, min: 300, max: 800, calm: 0 };
    const dry = liveJitter(s0, { kind: "underrun" }), gap = liveJitter(s0, { kind: "gap" });
    let s = s0; const grown = [];
    for (let i = 0; i < 12; i++) { s = liveJitter(s, { kind: "underrun" }); grown.push(s.target); }
    const tick = (x, seconds) => { for (let i = 0; i < seconds; i++) x = liveJitter(x, { kind: "tick", ms: 1000 }); return x; };
    let t = { target: 330, min: 300, max: 800, calm: 0 };
    t = tick(t, 600);
    const troubled = liveJitter({ target: 600, min: 300, max: 800, calm: 4500, floor: 300, fcalm: 100 }, { kind: "gap" });
    const rebased = liveRebase({ target: 360, min: 300, max: 800, calm: 0, floor: 360, fcalm: 0 }, 100);
    return JSON.stringify({ dry, gap: gap.target, grown, floor: t.target, kept: s0.target, troubled, rebased });
  })()`));
  if (jitter.dry.target !== 360 || jitter.dry.floor !== 360 || jitter.gap !== 330 || jitter.grown.join() !== "360,420,480,540,600,660,720,780,800,800,800,800") fail("live: the buffer does not grow by its fixed steps: " + JSON.stringify(jitter));
  if (jitter.floor !== 300 || jitter.kept !== 300) fail("live: the buffer does not shrink to its minimum, or changed its input: " + JSON.stringify(jitter));
  if (jitter.troubled.target !== 630 || jitter.troubled.calm !== 0) fail("live: trouble did not start the calm over");
  if (jitter.rebased.min !== 100 || jitter.rebased.target !== 160 || jitter.rebased.floor !== 160) fail("live: a lower minimum did not carry the target down with it: " + JSON.stringify(jitter.rebased));
  // One burst of dry spells raises the target modestly; it then comes down at 10 ms per 10 s but
  // never under the floor, which falls 10 ms per 30 s; and 10 quiet minutes after that, nothing
  // runs dry for want of a target that shrank into the same trouble (no sawtooth).
  const settle = JSON.parse(ev(`(() => {
    let s = { target: 300, min: 300, max: 800, calm: 0 };
    for (let i = 0; i < 2; i++) s = liveJitter(s, { kind: "underrun" });
    const burst = s.target, floor = s.floor;
    const trace = [];
    let belowFloor = 0, steps = 0;
    for (let sec = 1; sec <= 600; sec++) {
      s = liveJitter(s, { kind: "tick", ms: 1000 });
      if (s.target < s.floor) belowFloor++;
      if (sec === 60 || sec === 300 || sec === 600) trace.push([sec, s.target, s.floor]);
    }
    return JSON.stringify({ burst, floor, trace, belowFloor });
  })()`));
  if (settle.burst !== 420 || settle.floor !== 420) fail("live: a burst of two dry spells should raise the target by 120 ms, no more: " + JSON.stringify(settle));
  if (settle.belowFloor !== 0) fail("live: the target went under its floor: " + JSON.stringify(settle));
  if (JSON.stringify(settle.trace) !== JSON.stringify([[60, 400, 400], [300, 320, 320], [600, 300, 300]])) fail("live: the target and its floor do not settle back as they should: " + JSON.stringify(settle.trace));
  const servo = JSON.parse(ev(`JSON.stringify([liveServo(300, 300), liveServo(330, 300), liveServo(350, 300), liveServo(240, 300), liveServo(450, 300), liveServo(150, 300), liveServo(190, 300)])`));
  if (servo.join() !== "1,1,1.005,0.995,1.02,0.98,0.98") fail("live: the buffer is not nudged toward its target: " + servo);
  const plans = JSON.parse(ev(`JSON.stringify([liveSyncPlan(0, 300), liveSyncPlan(700, 300), liveSyncPlan(-100, 300), liveSyncPlan(-220, 300), liveSyncPlan(-500, 300), liveSyncPlan(9999, 300), liveSyncPlan(-9999, 300), liveSyncPlan("x", 300)])`));
  if (JSON.stringify(plans) !== JSON.stringify([
    { extra: 0, min: 300, short: false }, { extra: 700, min: 300, short: false }, { extra: 0, min: 200, short: false },
    { extra: 0, min: 80, short: false }, { extra: 0, min: 80, short: true }, { extra: 1500, min: 300, short: false },
    { extra: 0, min: 80, short: true }, { extra: 0, min: 300, short: false }])) fail("live: lip-sync plans are wrong: " + JSON.stringify(plans));
  if (ev(`[soundSyncLabel(0), soundSyncLabel(300), soundSyncLabel(-200), soundSyncLabel(1500)].join()`) !== "+0.0 s,+0.3 s,−0.2 s,+1.5 s") fail("live: the lip-sync label is wrong");

  // Mode: the wires and the owner's choice decide; the background always listens.
  const modes = JSON.parse(ev(`(() => {
    const sources = [{ id: 1, name: "Classical", kind: "radio" }, { id: 6, name: "Endora Mixer", kind: "airplay" }, { id: 7, name: "Spotify", kind: "spotify" }];
    const wires = [{ source: 1, speaker: 4, level: 50 }, { source: 6, speaker: 5, level: 80 }, { source: 7, speaker: 4, level: 50 }];
    return JSON.stringify([liveModeFor("auto", sources, wires, 4), liveModeFor("auto", sources, wires, 5), liveModeFor("auto", sources, wires, 9),
      liveModeFor("live", sources, wires, 4), liveModeFor("listening", sources, wires, 5), liveModeFor("auto", null, null, 4),
      liveEffectiveMode("live", false), liveEffectiveMode("live", true), liveEffectiveMode("listening", false)]);
  })()`));
  if (modes.join() !== "listening,live,listening,live,listening,listening,live,listening,listening") fail("live: the way to hear is not chosen by the wires and the owner: " + modes);

  // The probe: the hub's clock never meets the page's. 20 ms each way, 3 ms in the hub, the tone
  // mixed 8 ms after the request arrived, heard 420 ms after it was mixed.
  const probe = JSON.parse(ev(`JSON.stringify(liveProbeDelay({ tSend: 1000, tRecv: 1043, received: 5000000, injected: 5008000, replied: 5003000,
    tDetect: 1000 + 20 + 8 + 420, tFlag: 1000 + 20 + 8 + 15 }))`));
  if (Math.abs(probe.ms - 420) > 0.01 || Math.abs(probe.oneWay - 20) > 0.01 || Math.abs(probe.transport - 15) > 0.01 || Math.abs(probe.buffer - 405) > 0.01) fail("live: the delay probe's arithmetic is wrong: " + JSON.stringify(probe));
  if (ev(`liveProbeDelay({ tSend: 0, tRecv: 10, received: 0, injected: 0, replied: 0, tDetect: 900, tFlag: null }).transport`) !== null) fail("live: listening has no frame to split its delay at");
  if (ev(`liveDelayLabel(400)`) !== "0.4 s" || ev(`liveDelayLabel(4960)`) !== "5.0 s") fail("live: the delay label is wrong");

  // The tone finder: a 2 kHz tone is found at its size, music and silence are not.
  const tone = JSON.parse(ev(`(() => {
    const n = 441, make = (f) => { const w = new Float32Array(n); for (let i = 0; i < n; i++) w[i] = f(i); return w; };
    const pure = liveTone(make((i) => 0.1 * Math.sin(2 * Math.PI * 2000 * i / 44100)), 44100);
    const half = liveTone(make((i) => i < 220 ? 0.1 * Math.sin(2 * Math.PI * 2000 * i / 44100) : 0), 44100);
    const music = liveTone(make((i) => 0.3 * Math.sin(2 * Math.PI * 440 * i / 44100) + 0.2 * Math.sin(2 * Math.PI * 3100 * i / 44100)), 44100);
    return JSON.stringify({ pure, half, music, silence: liveTone(new Float32Array(n), 44100) });
  })()`));
  if (Math.abs(tone.pure.amp - 0.1) > 0.002 || tone.pure.fraction < 0.99 || Math.abs(tone.half.amp - 0.05) > 0.01 || tone.music.amp > 0.01 || tone.silence.amp !== 0) fail("live: the tone finder is wrong: " + JSON.stringify(tone));

  // The worklet itself: built from the same functions, driven block by block with a clock.
  const source = ev("liveWorkletSource()");
  const RATE = 44100, BLOCK = 128;
  const run = (seconds, options, script, keep = true) => {
    const out = [];
    const w = createContext({ sampleRate: RATE, currentTime: 0, Math, Float32Array, Number, Array, Object, JSON });
    runInContext(`class AudioWorkletProcessor { constructor() { this.port = { postMessage: (m) => globalThis.__out.push(m), onmessage: null }; } }
      var __out = [], __cls = null; function registerProcessor(name, cls) { __cls = cls; }`, w);
    runInContext(source, w);
    w.__out = out;
    const proc = new w.__cls({ processorOptions: options });
    const L = new Float32Array(BLOCK), R = new Float32Array(BLOCK), left = [], at = [];
    for (let b = 0; b * BLOCK < seconds * RATE; b++) {
      w.currentTime = b * BLOCK / RATE;
      script(proc, w.currentTime, b);
      proc.process([], [[L, R]]);
      if (keep) for (let i = 0; i < BLOCK; i++) left.push(L[i]);
    }
    return { out, left, proc };
  };
  // 20 ms frames of a 200 Hz sine, one per 20 ms, numbered from 0; `skip` frames never come.
  const sine = (hz, amp) => (n) => amp * Math.sin(2 * Math.PI * hz * n / RATE);
  const frameAt = (k, wave) => {
    const l = new Float32Array(882);
    for (let i = 0; i < 882; i++) l[i] = wave(k * 882 + i);
    return { t: "frame", seq: k, rate: RATE, l, r: l };
  };
  const feeding = (wave, skip, stopAt, resumeAt) => {
    let next = 0;
    return (proc, now) => {
      // A stall holds frames back and delivers them in a burst when it ends; a skipped frame
      // never comes at all.
      if (stopAt !== undefined && now >= stopAt && now < (resumeAt || Infinity)) return;
      while (next * 0.02 <= now) {
        const k = next++;
        if ((skip || []).includes(k)) continue;
        proc.heard(frameAt(k, wave));
      }
    };
  };
  const steepest = (xs, from) => { let m = 0; for (let i = Math.max(1, from); i < xs.length; i++) m = Math.max(m, Math.abs(xs[i] - xs[i - 1])); return m; };
  // The most a 200 Hz sine of 0.5 moves between two samples is about 0.0143.
  const SMOOTH = 0.02;
  const start = (left) => left.findIndex((x) => x !== 0) / RATE;

  // Steady: nothing sounds until 300 ms are in hand, then the sound is a clean sine.
  const steady = run(3, { rate: RATE, jitter: { target: 300, min: 300, max: 800, calm: 0 } }, feeding(sine(200, 0.5)));
  const began = start(steady.left);
  if (began < 0.27 || began > 0.37) fail(`live: it began to play after ${began} s, not once 300 ms were in hand`);
  if (steepest(steady.left, 0) > SMOOTH) fail("live: steady playing has a click in it");
  const stat = steady.out.filter((m) => m.t === "stat").pop();
  if (Math.abs(stat.depthMs - 300) > 25 || stat.under !== 0 || stat.gaps !== 0 || stat.target !== 300) fail("live: steady playing did not hold its buffer: " + JSON.stringify(stat));

  // A burst of sound arriving at once is skipped down to the target, not played late for ever.
  const burst = run(2, { rate: RATE, jitter: { target: 300, min: 300, max: 800, calm: 0 } }, (() => { let done = false; return (proc, now) => { if (!done) { done = true; for (let k = 0; k < 75; k++) proc.heard(frameAt(k, sine(200, 0.5))); } else { const k = 75 + Math.floor((now - 0.0001) / 0.02); if (k >= proc.expected && now > 0) proc.heard(frameAt(proc.expected, sine(200, 0.5))); } }; })());
  const burstStat = burst.out.filter((m) => m.t === "stat").pop();
  if (burstStat.depthMs > 700) fail("live: a burst of 1.5 s was played late instead of skipped: " + JSON.stringify(burstStat));

  // Running dry (a stall that ends in a burst): it fades to nothing without a click, waits for
  // 360 ms, grows its target, and, nothing being lost, counts no gap.
  const dry = run(4, { rate: RATE, jitter: { target: 300, min: 300, max: 800, calm: 0 } }, feeding(sine(200, 0.5), [], 1.2, 1.8));
  const dryStat = dry.out.filter((m) => m.t === "stat").pop();
  if (dryStat.under !== 1 || dryStat.target !== 360 || dryStat.gaps !== 0) fail("live: running dry was not noticed, or did not grow the buffer: " + JSON.stringify(dryStat));
  if (steepest(dry.left, 0) > SMOOTH) fail("live: running dry clicked");
  const playing = dry.out.filter((m) => m.t === "playing").map((m) => m.on);
  if (playing.join() !== "true,false,true") fail("live: it did not stop and start again around the dry spell: " + playing);
  // It is quiet through the dry spell (the buffer of 300 ms emptied at about 1.5 s) and plays
  // again when the burst has brought it past the new target of 360 ms.
  const quiet = dry.left.slice(Math.round(1.55 * RATE), Math.round(1.79 * RATE));
  if (Math.max(...quiet.map(Math.abs)) !== 0) fail("live: it made sound with nothing to play");
  const again = dry.left.slice(Math.round(1.8 * RATE)).findIndex((x) => x !== 0) / RATE + 1.8;
  if (again > 1.85) fail(`live: it began again after ${again} s, not as the burst brought 360 ms`);
  // A slow trickle (half the sound in time) keeps running dry, and keeps growing: to the top.
  const starved = run(40, { rate: RATE, jitter: { target: 300, min: 300, max: 800, calm: 0 } }, (() => { let next = 0; return (proc, now) => { while (next * 0.04 <= now) proc.heard(frameAt(next++, sine(200, 0.5))); }; })());
  const starvedStat = starved.out.filter((m) => m.t === "stat").pop();
  if (starvedStat.target !== 800 || starvedStat.under < 3) fail("live: a starved stream did not grow the buffer to its top: " + JSON.stringify(starvedStat));
  if (steepest(starved.left, 0) > SMOOTH) fail("live: a starved stream clicked");

  // Missing frames: a dip, no click, counted, and the buffer grows by a quarter.
  const lossy = run(3, { rate: RATE, jitter: { target: 300, min: 300, max: 800, calm: 0 } }, feeding(sine(200, 0.5), [70, 71, 72]));
  const lossyStat = lossy.out.filter((m) => m.t === "stat").pop();
  if (lossyStat.gaps !== 1 || lossyStat.lost !== 3 || lossyStat.target !== 330) fail("live: lost frames were not counted, or did not grow the buffer: " + JSON.stringify(lossyStat));
  if (steepest(lossy.left, 0) > SMOOTH) fail("live: a gap clicked");
  const gapAt = Math.round((0.28 + 70 * 0.02 + 0.02) * RATE);
  const dip = lossy.left.slice(gapAt - 3000, gapAt + 3000).reduce((m, x, i, a) => i > 100 && i < a.length - 100 && Math.abs(x) < 0.001 ? m + 1 : m, 0);
  if (dip < 3) fail("live: a gap did not fade to silence and back");

  // Calm: a target that grew comes back down, slowly: 10 ms for each 10 s, but not under its floor.
  const calm = run(25, { rate: RATE, jitter: { target: 450, min: 300, max: 800, calm: 0 } }, feeding(sine(200, 0.5)));
  const calmStat = calm.out.filter((m) => m.t === "stat").pop();
  if (calmStat.target !== 430) fail("live: after 25 s of calm the target should have come back by 20 ms: " + JSON.stringify(calmStat));
  if (steepest(calm.left, 0) > SMOOTH) fail("live: shrinking the buffer clicked");
  const floored = run(60, { rate: RATE, jitter: { target: 420, min: 300, max: 800, calm: 0, floor: 420, fcalm: 0 } }, feeding(sine(200, 0.5)), false);
  const flooredStat = floored.out.filter((m) => m.t === "stat").pop();
  if (flooredStat.target !== 410 || flooredStat.under !== 0) fail("live: the target went under its floor, or the floor did not fall 10 ms in 60 s: " + JSON.stringify(flooredStat));
  // No sawtooth: ten minutes of calm after it settled just above what the network needs, and not
  // one dry spell, the target never under the floor, and a steady sound all the way.
  const longCalm = run(600, { rate: RATE, jitter: { target: 360, min: 300, max: 800, calm: 0, floor: 360, fcalm: 0 } }, (() => {
    const feed = feeding(sine(200, 0.5));
    return (proc, now, b) => { feed(proc, now); if (b % 20000 === 0 && proc.jit.target < proc.jit.floor) fail("live: the target went under its floor at " + now + " s"); };
  })(), false);
  const longStat = longCalm.out.filter((m) => m.t === "stat").pop();
  if (longStat.under !== 0 || longStat.gaps !== 0 || longCalm.out.filter((m) => m.t === "playing").length !== 1) fail("live: ten calm minutes had a dry spell in them (a sawtooth): " + JSON.stringify(longStat));
  if (longStat.target < 300 || longStat.target > 360) fail("live: after ten calm minutes the target is not between the minimum and where it settled: " + JSON.stringify(longStat));
  // A steady network keeps its target at the minimum: nothing to learn, nothing to grow.
  const quietNet = run(60, { rate: RATE, jitter: { target: 300, min: 300, max: 800, calm: 0 } }, feeding(sine(200, 0.5)), false);
  const quietStat = quietNet.out.filter((m) => m.t === "stat").pop();
  if (quietStat.target !== 300 || quietStat.under !== 0) fail("live: a steady network did not stay at the minimum: " + JSON.stringify(quietStat));

  // Far over target is pulled back: 200 ms too much (more than 150 over) is skipped down at once,
  // 120 ms too much (a servo's job) is played down at two percent, both without a click.
  const surplus = (extraFrames) => {
    const seen = { early: null, late: null };
    let tick = 0, seq = 0, done = false;
    const result = run(14, { rate: RATE, jitter: { target: 300, min: 300, max: 800, calm: 0 } }, (proc, now) => {
      while (tick * 0.02 <= now) { proc.heard(frameAt(seq++, sine(200, 0.5))); tick++; }
      if (!done && now >= 2) { done = true; for (let i = 0; i < extraFrames; i++) proc.heard(frameAt(seq++, sine(200, 0.5))); }
      if (seen.early === null && now >= 2.3) seen.early = (proc.wr - proc.rd) / 44.1;
      if (seen.late === null && now >= 13) seen.late = (proc.wr - proc.rd) / 44.1;
    });
    return { seen, result };
  };
  const skipped = surplus(10);
  if (skipped.seen.early > 340 || skipped.seen.late > 340) fail("live: 200 ms too much was not skipped down to the target: " + JSON.stringify(skipped.seen));
  if (steepest(skipped.result.left, 0) > SMOOTH) fail("live: catching up by a skip clicked");
  const servoed = surplus(6);
  if (servoed.seen.early < 380 || servoed.seen.late > 360) fail("live: 120 ms too much was not played down to the target: " + JSON.stringify(servoed.seen));
  if (steepest(servoed.result.left, 0) > 0.03) fail("live: catching up by playing faster clicked");
  if (skipped.result.out.filter((m) => m.t === "stat").pop().under !== 0) fail("live: catching up ran the buffer dry");

  // Lip-sync: later is a fixed extra delay on top of the target (the target itself is unchanged);
  // earlier lowers the minimum, never under 80 ms; a change is followed while playing.
  const depthAfter = (seconds, options, script) => {
    const r = run(seconds, options, script || feeding(sine(200, 0.5)), false);
    const stat = r.out.filter((m) => m.t === "stat").pop();
    return { stat, r };
  };
  const base = { rate: RATE, jitter: { target: 300, min: 300, max: 800, calm: 0 } };
  const later = depthAfter(6, { ...base, sync: 500 });
  if (Math.abs(later.stat.depthMs - 800) > 30 || later.stat.target !== 300 || later.stat.under !== 0) fail("live: +0.5 s lip-sync did not hold 500 ms more in hand: " + JSON.stringify(later.stat));
  const earlier = depthAfter(6, { ...base, sync: -200 });
  if (earlier.stat.target !== 100 || earlier.stat.under !== 0 || Math.abs(earlier.stat.depthMs - 100) > 30) fail("live: -0.2 s lip-sync did not lower the buffer to 100 ms: " + JSON.stringify(earlier.stat));
  const lowest = depthAfter(6, { ...base, sync: -500 });
  if (lowest.stat.target !== 80) fail("live: lip-sync took the buffer under 80 ms: " + JSON.stringify(lowest.stat));
  const nudged = depthAfter(30, base, (() => {
    const feed = feeding(sine(200, 0.5));
    return (proc, now) => { if (now >= 2 && !proc.nudged) { proc.nudged = true; proc.heard({ t: "sync", extra: 300, min: 300 }); } feed(proc, now); };
  })());
  if (Math.abs(nudged.stat.depthMs - 600) > 45 || nudged.stat.under !== 0) fail("live: nudging the lip-sync later while playing was not followed: " + JSON.stringify(nudged.stat));
  const relaxed = depthAfter(30, { ...base, sync: 300 }, (() => {
    const feed = feeding(sine(200, 0.5));
    return (proc, now) => { if (now >= 2 && !proc.nudged) { proc.nudged = true; proc.heard({ t: "sync", extra: 0, min: 300 }); } feed(proc, now); };
  })());
  if (Math.abs(relaxed.stat.depthMs - 300) > 45 || relaxed.stat.under !== 0) fail("live: taking the lip-sync back was not followed: " + JSON.stringify(relaxed.stat));

  // The probe: a 2 kHz burst of 80 ms in the stream (silence before it) is found, and the time it
  // began to come out is told to within a few ms, which is what a delay measurement stands on.
  const burstWave = (n) => (n >= 0.8 * RATE && n < 0.88 * RATE ? 0.1 * Math.sin(2 * Math.PI * 2000 * n / RATE) : 0);
  const heardTone = run(2.5, { rate: RATE, jitter: { target: 300, min: 300, max: 800, calm: 0 } }, (() => {
    const feed = feeding(burstWave);
    return (proc, now, b) => { if (b === 0) proc.heard({ t: "probe", on: true, ref: 0.1 }); feed(proc, now); };
  })());
  const tones = heardTone.out.filter((m) => m.t === "tone");
  if (tones.length !== 1) fail("live: the probe's tone was heard " + tones.length + " times");
  // The stream is silent until the tone, so the first sound out of the worklet is the tone's.
  const truly = heardTone.left.findIndex((x) => x !== 0) / RATE;
  if (Math.abs(tones[0].at - truly) > 0.006) fail(`live: the tone came out at ${truly} s and was told at ${tones[0].at} s`);
  // And with the owner's volume low it is found all the same.
  const quietTone = run(2.5, { rate: RATE, jitter: { target: 300, min: 300, max: 800, calm: 0 } }, (() => {
    const feed = feeding(burstWave);
    return (proc, now, b) => { if (b === 0) { proc.heard({ t: "probe", on: true, ref: 0.1 }); proc.heard({ t: "level", v: 0.05 }); } feed(proc, now); };
  })());
  if (quietTone.out.filter((m) => m.t === "tone").length !== 1) fail("live: a low volume hid the probe's tone");
  const playsOn = run(2.5, { rate: RATE, jitter: { target: 300, min: 300, max: 800, calm: 0 } }, feeding(sine(200, 0.5)));
  if (playsOn.out.some((m) => m.t === "tone")) fail("live: a probe nobody asked for was heard in music");
  console.log(`live: frames read whole, buffer 300 to 800 ms in fixed steps with a floor, lip-sync, no clicks across a gap or a dry spell, the probe's tone found to ${(Math.abs(tones[0].at - truly) * 1000).toFixed(1)} ms`);

  // Modes in the page: the background hands over to the MP3 and the front takes live back; a
  // live stream that cannot start falls back to listening and asks for a tap.
  ev(`var keepOpen = liveOpen, keepClose = liveClose, keepSupported = liveSupported, keepCtx = liveContext;
    var opened = [], closed = [], audioSrc = [], played = [];
    liveSupported = () => true;
    liveContext = () => ({ state: "running", resume: () => Promise.resolve() });
    liveOpen = async (url, muted, on) => { opened.push(url); return { url, on, closed: false, lastAt: 0 }; };
    liveClose = (s) => { if (s) closed.push(s.url); };
    var fakeAudio = { set src(v) { audioSrc.push(v); }, get src() { return audioSrc[audioSrc.length - 1]; }, play: () => { played.push(audioSrc[audioSrc.length - 1]); return Promise.resolve(); }, pause() {}, removeAttribute() {}, load() {}, addEventListener() {}, volume: 1 };
    var keepAudioEl = document.getElementById;
    document.getElementById = (id) => id === "sound-player" ? fakeAudio : keepAudioEl(id);
    var keepLS = localStorage.getItem;
    localStorage.getItem = (k) => (k === "endora.soundSpeaker" ? "5" : k === "endora.soundMode" ? "auto" : null);
    var mixer = { id: 6, name: "Endora Mixer", kind: "airplay", device: null }, radio = { id: 1, name: "Test station", kind: "radio" };
    var phone = { id: 5, name: "Test phone", kind: "stream", device: "test-phone", volume: 100, borrowed: false };
    SOUND.state = { sources: [mixer, radio], speakers: [phone], groups: [], setups: [],
      wires: [{ source: 6, speaker: 5, level: 80 }] };
    SOUND.listening = true; SOUND.mode = null; SOUND.liveBlocked = false;
    SOUND.ticket = { url: "/v1/sound/listen/5?ticket=aa", live: "/v1/sound/live/5?ticket=aa", at: Date.now() };`);
  const wait = () => new Promise((r) => setTimeout(r, 5));
  // Tapped with the door wired: live, with the MP3's element given a moment of silence to unlock.
  ev(`soundPlay(SOUND.ticket)`); await wait();
  if (ev("SOUND.mode") !== "live" || ev("opened.join()") !== "/v1/sound/live/5?ticket=aa" || !ev("audioSrc[0]").startsWith("data:audio/wav")) fail("live: a door wired to this speaker did not play live");
  // The page goes to the background: the MP3 from the ticket kept ready, live let go.
  ev(`LIVE.session = { url: "live-now", closed: false, lastAt: 0 }; document.hidden = true;`);
  await ev("soundApplyMode()");
  if (ev("SOUND.mode") !== "listening" || ev("played[played.length - 1]") !== "/v1/sound/listen/5?ticket=aa" || ev("closed.join()") !== "live-now") fail("live: the background did not hand over to the MP3: " + ev("JSON.stringify({ m: SOUND.mode, played, closed })"));
  // And back: live again, from a ticket still good.
  ev(`document.hidden = false; opened.length = 0;`);
  await ev("soundApplyMode()"); await wait();
  if (ev("SOUND.mode") !== "live" || ev("opened.length") !== 1) fail("live: coming back to the page did not take live back");
  // The owner picks Listening: it stays on the MP3 in the front too.
  ev(`localStorage.getItem = (k) => (k === "endora.soundSpeaker" ? "5" : k === "endora.soundMode" ? "listening" : null);`);
  await ev("soundApplyMode()");
  if (ev("SOUND.mode") !== "listening") fail("live: Listening chosen by the owner did not stay on the MP3");
  // And Auto with radio only: listening; a door wired in: live.
  ev(`localStorage.getItem = (k) => (k === "endora.soundSpeaker" ? "5" : k === "endora.soundMode" ? "auto" : null);
    SOUND.state = Object.assign({}, SOUND.state, { wires: [{ source: 1, speaker: 5, level: 80 }] });`);
  await ev("soundApplyMode()");
  if (ev("SOUND.mode") !== "listening") fail("live: music alone should be heard by listening");
  ev(`SOUND.state = Object.assign({}, SOUND.state, { wires: [{ source: 6, speaker: 5, level: 80 }] });`);
  await ev("soundApplyMode()"); await wait();
  if (ev("SOUND.mode") !== "live") fail("live: wiring the AirPlay door here did not switch to live by itself");
  // Live that will not start (no tap since the screen was off): listening, with a way to tap.
  ev(`SOUND.mode = "listening"; liveOpen = async () => { throw new Error("no tap"); }; SOUND.ticket = { url: "/v1/sound/listen/5?ticket=bb", live: "/v1/sound/live/5?ticket=bb", at: Date.now() };`);
  await ev("soundApplyMode()"); await wait(); await wait();
  if (ev("SOUND.mode") !== "listening" || ev("SOUND.liveBlocked") !== true || ev("soundWantedMode()") !== "listening") fail("live: live that would not start did not fall back to listening");
  // Stopped: nothing left running.
  ev(`liveClose = (s) => { if (s) closed.push(s.url); }; LIVE.session = { url: "last", closed: false, lastAt: 0 }; SOUND.listening = true;`);
  await ev(`soundDispatch("sound", "stop", "", "")`);
  if (ev("SOUND.listening") !== false || ev("SOUND.mode") !== null || ev("LIVE.session") !== null || !ev("closed").includes("last")) fail("live: Stop left live running");
  // The card: the three choices, the measure button (off until listening), and what the mode is.
  ev(`liveOpen = keepOpen; liveClose = keepClose; liveSupported = keepSupported; liveContext = keepCtx;
    SOUND.mode = "live"; SOUND.stat = { playing: true, depthMs: 410, under: 0 }; SOUND.listening = true; LIVE.session = null;
    SOUND.measure = { live: { ms: 430, transport: 12, buffer: 418, intake: [{ source: "Endora Mixer", ms: 160 }] }, listening: { ms: 4960, transport: null, buffer: null, intake: [] } };`);
  const card = ev("viewSound()");
  for (const want of ['data-act="sound:mode:auto"', 'data-act="sound:mode:live"', 'data-act="sound:mode:listening"', 'data-act="sound:measure"', 'id="sound-live-label"', "live · 0.4 s",
    "Live: 0.4 s (12 ms to this page, 418 ms in its buffer)", "Listening: 5.0 s", "Endora Mixer waits 160 ms in the hub"]) {
    if (!card.includes(want)) fail(`live: the speaker card does not show ${want}`);
  }
  if (card.includes('data-act="sound:measure" disabled')) fail("live: Measure delay was off while listening");
  // Lip-sync: on the card in live mode, showing the offset; a tap moves it a tenth of a second,
  // the playing stream is told at once, and the hub is asked to keep it. Earlier than the buffer
  // can go says what to do; a hub that refuses puts it back.
  if (!card.includes('data-act="sound:sync:-100"') || !card.includes('data-act="sound:sync:100"') || !card.includes('id="sound-sync-label">+0.0 s<')) fail("live: the lip-sync buttons are not on the live card");
  if (card.includes("sound-sync-note")) fail("live: lip-sync at zero already says it cannot go earlier");
  ev(`var syncCalls = [], syncSent = [], keepApi = api;
    api = async (m, p, b) => { syncCalls.push(m + " " + p + " " + JSON.stringify(b)); return {}; };
    LIVE.session = { closed: false, node: { port: { postMessage: (m) => syncSent.push(m) } } };`);
  await ev(`soundDispatch("sound", "sync", "100", "")`);
  if (ev("syncCalls[0]") !== 'PUT /v1/sound/speakers/5/sync {"ms":100}') fail("live: a lip-sync tap did not ask the hub to keep it: " + ev("syncCalls.join()"));
  if (JSON.stringify(ev("syncSent[0]")) !== JSON.stringify({ t: "sync", extra: 100, min: 300 })) fail("live: a lip-sync tap did not reach the playing worklet: " + ev("JSON.stringify(syncSent)"));
  if (!ev("viewSound()").includes('id="sound-sync-label">+0.1 s<')) fail("live: the card did not show +0.1 s after a tap");
  for (let i = 0; i < 4; i++) await ev(`soundDispatch("sound", "sync", "-100", "")`);
  if (JSON.stringify(ev("syncSent[syncSent.length - 1]")) !== JSON.stringify({ t: "sync", extra: 0, min: 80 })) fail("live: -0.3 s lip-sync did not lower the buffer to its 80 ms floor: " + ev("JSON.stringify(syncSent)"));
  const early = ev("viewSound()");
  if (!early.includes('id="sound-sync-label">−0.3 s<') || !early.includes("Can&#39;t go earlier on this speaker; lower the door&#39;s early hand-over") && !early.includes("Can't go earlier on this speaker; lower the door's early hand-over")) fail("live: asking for more advance than the buffer allows did not say what to do");
  for (let i = 0; i < 4; i++) await ev(`soundDispatch("sound", "sync", "-100", "")`);
  if (ev("syncCalls.length") !== 7 || !ev("viewSound()").includes("sound-sync-note") || !ev("viewSound()").includes('data-act="sound:sync:-100" aria-label="Play 0.1 s earlier" disabled')) fail("live: lip-sync went past -0.5 s: " + ev("syncCalls.join()"));
  ev(`api = async () => { throw new Error("refused"); }; syncSent.length = 0;`);
  await ev(`soundDispatch("sound", "sync", "100", "")`);
  if (ev("soundMine().sync_ms") !== -500 + 0 || JSON.stringify(ev("syncSent.map((m) => m.min + ':' + m.extra)")) !== JSON.stringify(["80:0", "80:0"])) fail("live: a refused lip-sync was not put back");
  ev(`api = keepApi; LIVE.session = null; soundMine().sync_ms = 0;`);
  ev(`SOUND.mode = "listening";`);
  if (ev("viewSound()").includes("sound:sync:")) fail("live: lip-sync is on the card while listening");
  ev(`SOUND.mode = "live";`);
  ev(`SOUND.listening = false; SOUND.mode = null;`);
  if (!ev("viewSound()").includes('data-act="sound:measure" disabled')) fail("live: Measure delay was on with nothing playing");
  ev(`document.getElementById = keepAudioEl; localStorage.getItem = keepLS; SOUND.state = keepSound0; SOUND.measure = null; SOUND.liveBlocked = false; SOUND.ticket = null;`.replace("keepSound0", "null"));
  console.log("live: the front plays live when a door is wired or when asked, the background and a refused start fall back to the MP3, and the card says which");
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

// The Activity screen opens with thirty rows and pages on by the count already shown, so a long
// history is not cut off at the thirtieth.
{
  const fail = (what) => { console.error(what); process.exit(1); };
  const row = (n) => `{ at_ms: ${n}, kind: "action", summary: "did thing ${n}" }`;
  // Let anything an earlier check left in flight finish before the requests are counted.
  await new Promise((done) => setTimeout(done, 100));
  runInContext(`NAV = { v: "audit" }; ACTIVITY = [${Array.from({ length: 30 }, (_, i) => row(100 - i)).join(",")}]; ACTIVITY_MORE = true; ACTIVITY_BUSY = false;`, context);
  if (!runInContext("activityFeed()", context).includes('data-act="activity:more"')) fail("activity: a full page offers no way to see older rows");
  const asked = [];
  const realFetch = context.fetch;
  context.fetch = (path) => {
    asked.push(String(path));
    return Promise.resolve({ ok: true, status: 200, json: async () => [{ at_ms: 5, kind: "decision", summary: "an old one" }] });
  };
  await runInContext('dispatch("activity:more")', context);
  context.fetch = realFetch;
  if (asked.length !== 1 || asked[0] !== "/v1/activity?limit=30&offset=30") fail(`activity: "Load more" asked for ${asked}`);
  const after = runInContext("activityFeed()", context);
  if (!after.includes("an old one") || after.includes('data-act="activity:more"') || !after.includes("That is everything")) fail("activity: a short page did not end the list");
  runInContext('ACTIVITY = []; ACTIVITY_MORE = false; NAV = { v: "chat" };', context);
  console.log("activity: pages on by what is shown, and says when it has everything");
}

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

// The World card on a phone: a pasted document is read into changes, a section opens in a
// full-screen editor, and everything shown is escaped. A short synthetic document of the shape
// the owner pastes: a title line, sections with several spellings of the heading, a section
// with no title, a timeline of bullets, and a heading that is none of those.
{
  const fail = (m) => { console.error("world editor: " + m); process.exit(1); };
  const parsed = runInContext(`JSON.stringify(hqWorldParse([
    "# Update for the team",
    "",
    "Some words before any section are ignored.",
    "",
    "## house / purpose — What this house is for",
    "A home lab that runs itself.",
    "",
    "## house/how-we-work - How we work",
    "- Evidence over claims.",
    "- Never fire; furlough.",
    "",
    "##  house /  known-facts :  What we know  ",
    "The NAS has 16 GB.",
    "### A smaller heading stays in the text",
    "",
    "## house / priorities",
    "1. Backups",
    "",
    "## Notes to self",
    "not a section",
    "",
    "## timeline entries",
    "- 17 Oct 2026: The router was moved.",
    "* 2026-10-18 - Found: the VPN restarts.",
    "- Oct 18, 2026: The team grew.",
    "- No date on this one.",
    "Not a bullet.",
    "",
    "## timeline lab",
    "- 18 Oct 2026: Lab entry.",
  ].join("\\n")))`, context);
  const got = JSON.parse(parsed);
  const sections = got.changes.filter((c) => c.kind === "section");
  const timeline = got.changes.filter((c) => c.kind === "timeline");
  if (sections.length !== 4 || timeline.length !== 5) fail(`expected 4 sections and 5 entries, got ${sections.length} and ${timeline.length}`);
  const [purpose, how, known, priorities] = sections;
  if (purpose.page !== "house" || purpose.key !== "purpose" || purpose.title !== "What this house is for"
      || purpose.text !== "A home lab that runs itself.") fail("a section with an em dash was misread");
  if (how.key !== "how-we-work" || how.title !== "How we work" || !how.text.includes("- Never fire; furlough.")) fail("a hyphen, no spaces round the slash, or bullets inside a section were misread");
  if (known.key !== "known-facts" || known.title !== "What we know" || !known.text.includes("### A smaller heading")) fail("odd spacing or a colon was misread");
  if (priorities.title !== "Priorities") fail("a missing title is not made from the key");
  if (got.notes.length !== 1 || !got.notes[0].includes("Notes to self")) fail("a heading that is neither is not set aside with a note");
  if (timeline[0].at !== Date.UTC(2026, 9, 17, 12) || timeline[0].text !== "The router was moved." || timeline[0].page !== "house") fail("a day-month-year entry was misread");
  if (timeline[1].at !== Date.UTC(2026, 9, 18, 12) || timeline[1].text !== "Found: the VPN restarts.") fail("an ISO date with a hyphen was misread");
  if (timeline[2].at !== Date.UTC(2026, 9, 18, 12) || timeline[2].text !== "The team grew.") fail("a month-first date was misread");
  if (timeline[3].at !== null || timeline[3].text !== "No date on this one.") fail("an undated bullet should keep its whole text");
  if (timeline[4].page !== "lab") fail("## timeline <page> did not pick the page");
  const nothing = runInContext(`JSON.stringify([hqWorldParse(""), hqWorldParse("just words\\n- a bullet"), hqWorldParse("## house / empty\\n\\n## house / x — X\\ntext")])`, context);
  const [none, loose, partial] = JSON.parse(nothing);
  if (none.changes.length || loose.changes.length) fail("a document with nothing to apply produced changes");
  if (partial.changes.length !== 1 || partial.notes.length !== 1) fail("a section with no text should be set aside, not applied");

  // The card: paste box, edit buttons, buttons tall enough to press, and hostile text escaped.
  runInContext(`HQ.worldDoc = null; HQ.worldEdit = null;
    HQ.snap = { world: { migrated: false, pages: [], stories: [], sections: [
      { page: "house", anchor: "backups", address: "world:house#backups", heading: "<b>Backups</b>", headline: "<img src=w onerror=alert(1)> at 03:00",
        body: "Nightly.", stage: "insight", said: "insight", revision: 2, written_by: "Mara", warrant: "", why: "" },
      { page: "house", anchor: "disk", address: "world:house#disk", heading: "Disk", headline: "16 GB", body: "", stage: "canon",
        said: "canon", revision: 3, written_by: "the owner", warrant: "", why: "" }] } };`, context);
  let card = runInContext("hqWorldCard(HQ.snap)", context);
  for (const need of ['data-act="hq:world:edit:open-0"', 'data-act="hq:world:edit:open-1"', 'data-act="hq:world:section:promote-0"',
      'data-act="hq:world:section:demote-1"', 'id="hq-wd-md"', 'data-act="hq:world:paste:preview"', 'autocapitalize="off"']) {
    if (!card.includes(need)) fail(`the World card lost ${need}`);
  }
  if (card.includes("<img src=w") || card.includes("<b>Backups</b>")) fail("a section's words are not escaped");
  if (card.includes('class="hq-btn ok"') || card.includes('class="hq-btn no"') || /class="hq-btn"/.test(card)) fail("a World button is not the tall kind");
  if (card.includes("hq-editor")) fail("the editor is open with nothing to edit");

  // Preview, untick, apply: the requests go out sections first, then the timeline, and a failure is kept.
  runInContext(`
    // There is no page here: the boxes are not found, so the state in HQ is what is used.
    var __getById = document.getElementById;
    document.getElementById = (id) => /^hq-w[de]-/.test(id) ? null : __getById(id);
    var __calls = [], __saved = [api, loadHQ, render, flash];
    HQ.worldDoc = { md: "## timeline\\n- 17 Oct 2026: Moved.\\n## house / backups — Backups\\nRuns at 4.\\n## house / new — New one\\nHello\\n## house / bad — Bad\\nNope", items: null, notes: [] };
    render = () => {}; loadHQ = async () => {}; flash = (t) => __calls.push("flash:" + t);
    api = async (m, path, body) => { __calls.push(m + " " + path + " " + JSON.stringify(body)); if (body.key === "bad") throw new Error("refused"); return {}; };`, context);
  runInContext(`hqWorldPasteAct("preview")`, context);
  const said = JSON.parse(runInContext(`JSON.stringify(HQ.worldDoc.items.map((i) => hqWorldSaid(i.change)))`, context));
  if (!said[0].includes("timeline: 17 Oct 2026 Moved.") || !said[1].includes("house / backups") || !said[1].includes("replaces the current text (Mara's stays in its history)")
      || !said[2].includes("new section")) fail("the preview does not say what each change does: " + JSON.stringify(said));
  runInContext(`hqWorldPasteAct("toggle-2")`, context);
  const shown = runInContext("hqWorldCard(HQ.snap)", context);
  if (!shown.includes("Apply 3 changes") || !shown.includes("Preview")) fail("the preview does not count what will be applied");
  await runInContext(`hqWorldPasteAct("apply")`, context);
  const calls = JSON.parse(runInContext("JSON.stringify(__calls)", context));
  const posts = calls.filter((c) => c.startsWith("POST"));
  if (posts.length !== 3 || !posts[0].includes("/world/section") || !posts[1].includes("/world/section") || !posts[2].includes("/world/timeline")
      || !posts[2].includes(String(Date.UTC(2026, 9, 17, 12)))) fail("apply did not send sections first, then the timeline: " + posts.join(" | "));
  if (posts.some((c) => c.includes('"key":"new"'))) fail("an unticked change was applied");
  const results = JSON.parse(runInContext(`JSON.stringify(HQ.worldDoc.items.map((i) => [i.result, i.on]))`, context));
  if (JSON.stringify(results) !== JSON.stringify([["ok", false], ["ok", false], ["", false], ["failed", true]])) fail("results are not kept per item: " + JSON.stringify(results));

  // The editor opens on a member's section with its text, and saves under the same page and key.
  runInContext(`__calls.length = 0; hqWorldEditAct("open-0")`, context);
  card = runInContext("hqWorldCard(HQ.snap)", context);
  if (!card.includes('class="hq-editor"') || !card.includes('data-act="hq:world:edit:save"') || !card.includes('data-act="hq:world:edit:cancel"')
      || !card.includes("Written by Mara")) fail("the editor did not open on the section");
  if (card.indexOf("hq:world:edit:save") > card.indexOf('id="hq-we-text"')) fail("Save is not above the text");
  if (card.includes("<img src=w")) fail("the editor shows the section's words unescaped");
  runInContext(`HQ.worldEdit.text = "Runs at 4.\\nNightly."; await0 = hqWorldEditAct("save")`, context);
  await runInContext("await0", context);
  const saved = JSON.parse(runInContext("JSON.stringify(__calls.filter((c) => c.startsWith('POST')))", context));
  if (saved.length !== 1 || !saved[0].includes('"page":"house","key":"backups","title":"<b>Backups</b>"')) fail("the editor did not save under the same page and key: " + saved.join("|"));
  if (runInContext("HQ.worldEdit", context) !== null) fail("the editor stayed open after saving");
  runInContext(`hqWorldEditAct("open-1"); hqWorldEditAct("cancel")`, context);
  if (runInContext("HQ.worldEdit", context) !== null) fail("Cancel did not close the editor");
  // The owner's word on a proposal to a page (ADR 0126): accepting posts the decision.
  runInContext(`__calls.length = 0; await1 = hqProposalAct("accept-4")`, context);
  await runInContext("await1", context);
  const decided = JSON.parse(runInContext("JSON.stringify(__calls.filter((c) => c.startsWith('POST')))", context));
  if (decided.length !== 1 || !decided[0].includes("/v1/hq/world/proposals/decide") || !decided[0].includes('"id":4') || !decided[0].includes('"accept":true')) {
    fail("accepting a proposal did not post the owner's decision: " + decided.join("|"));
  }
  runInContext(`[api, loadHQ, render, flash] = __saved; HQ.worldDoc = null; HQ.snap = null; document.getElementById = __getById;`, context);
  console.log("world editor: paste, preview, apply and the section editor behave");
}

console.log(`console check: ${named.length} screens render`);
