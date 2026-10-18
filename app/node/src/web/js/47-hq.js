// ---- Endora HQ: the office (ADR 0102, spec section 13) ---------------------------------------
// The team as a pixel startup office, drawn from HQ's snapshot and the events after it. The
// console talks to the node only; the node signs every request to HQ and records the owner's
// actions in Endora's own audit log. Approvals and the kill switch are buttons here that call
// the node's routes, never anything the office view decides.
//
// Office art: Pixel Agents (MIT) and MetroCity by JIK-A-4 (CC0), served from /hq-art/ with its
// credits and licence beside it.

const HQ = {
  snap: null,          // the latest snapshot
  error: "",           // why it could not be loaded
  last: 0,             // the last event id seen: the stream resumes after it
  log: [],             // what happened, newest first, in words
  selected: null,      // { type: "member" | "task", id }
  people: {},          // member id → where they are and what they are doing on screen
  envelopes: [],       // wakes in flight
  lastWord: {},        // task id → the member who last wrote in its thread
  roster: {},          // member id → its controls are open on the Team card
  notebooks: {},       // member id → its notes, read when its panel is opened (ADR 0118)
  allTurns: {},        // "task-N" / "member-N" → every turn shown, not only the last five
  pods: [],            // one pod of desks per team, laid out from the snapshot (ADR 0119)
  seats: {},           // member id → its desk, where it works and where a visitor stands
  world: null,         // the cells nobody walks through, and the walls between cells
  rand: Math.random,   // where idle members drift: replaceable, so a check can fix it
  settled: false,      // set once the first snapshot has put everyone at a desk
  t: 0,                // seconds of animation
  polling: null,
  looping: false,
};

const HQ_T = 16, HQ_COLS = 32, HQ_ROWS = 20, HQ_W = HQ_COLS * HQ_T, HQ_H = HQ_ROWS * HQ_T;
const HQ_C = {
  wall: "#efece6", wallTop: "#e2ddd3", trim: "#c9c1b3", glass: "rgba(150,205,235,.22)", glassEdge: "#9cc7de",
  board: "#fbfbf8", boardEdge: "#b9c2c9", rack: "#232833", neon: "#ff4fa3", lamp: "#ffb347", red: "#ef5a5f",
  green: "#45c486", blue: "#7fb2ff", violet: "#b98cf0", sticky: "#fff2a8", text: "#1f1d29", shadow: "rgba(60,40,20,.18)",
};
const HQ_STATUS_COL = { todo: HQ_C.sticky, doing: HQ_C.lamp, in_review: HQ_C.blue, done: HQ_C.green, blocked: HQ_C.red, dropped: "#cfcac0" };
// The open office (ADR 0119), in tiles; a person stands on a tile (a cell), and `hqAt` says
// where their sprite goes. Along the back wall: the board room (the owner's), the task board,
// Mara's office with its door open, and the server closet. The open floor under them holds
// one pod of desks for each team, side by side with no walls (row 8 down, laid out from the
// snapshot); under that, ping-pong, the meeting room, and the kitchen and lounge that
// everyone shares. The things that can move are not here: they come from the snapshot.
function hqAt(cx, cy, extra) { return { cx, cy, x: cx, y: cy - 0.5, ...extra }; }
const HQ_P = {
  board: { x: 8, y: 0, w: 12, h: 2 }, owner: { x: 2, y: 3 }, lever: { x: 6.1, y: 2.3 },
  boardRoom: { x: 0, y: 2, w: 7, h: 5 }, boardDoor: hqAt(6, 7, { dir: "up" }),
  engine: { x: 27, y: 2, w: 5, h: 5 }, rack: { x: 30.2, y: 2.4 }, jar: { x: 27.8, y: 2.8 },
  door: { x: 30, y: 10.5 }, lobby: hqAt(29, 12, { dir: "left" }), pong: { x: 3, y: 17 }, tap: { x: 16.4, y: 15.6 },
};
// The pods' ground: the band the teams' desks stand in, each desk three tiles by two, two
// desks to a row with a two-tile aisle between them where their members stand facing each
// other. Nothing else is ever put there (the rules of the office say so too).
const HQ_POD = { top: 8, left: 1, right: 28, bottom: 14, desk: { w: 3, h: 2 }, aisle: 2, between: 1 };
// The two rooms people walk into, each with its door open: a member goes through the gap in
// the wall (cells `from` to `to` on that side), `out` the step outside it and `in` inside.
const HQ_ROOMS = {
  ceo: { x: 20, y: 2, w: 7, h: 5, door: { side: "bottom", from: 20, to: 22 }, out: hqAt(21, 7, { dir: "up" }), in: hqAt(21, 6, { room: "ceo", dir: "down" }) },
  meeting: { x: 9, y: 15, w: 7, h: 5, door: { side: "top", from: 9, to: 11 }, out: hqAt(10, 14, { dir: "down" }), in: hqAt(10, 15, { room: "meeting", dir: "down" }) },
};
// Where people sit in the meeting room: the CEO at the head, behind the table; the others on
// the chairs before it, their backs to us.
const HQ_MEETING = {
  table: { x: 12, y: 16 },
  head: hqAt(12, 16, { room: "meeting", dir: "down" }),
  chairs: [hqAt(12, 18, { room: "meeting", dir: "up" }), hqAt(13, 18, { room: "meeting", dir: "up" })],
};
// The CEO's desk in its own office, where it works, and the sofa where it rests.
const HQ_CEO = {
  desk: { x: 21, y: 3 },
  work: hqAt(22, 3, { room: "ceo", dir: "down", desk: true }),
  rest: hqAt(25, 5, { room: "ceo", dir: "down" }),
};
// Where idle members drift, picking one at random so the teams mix: the ping-pong table's two
// ends, the kitchen table's chairs, the tap. The couches, the coffee machine and the rest come
// from what stands on the floor now (`hqWanderSpots`).
const HQ_WANDER = [
  hqAt(2, 17, { dir: "right", pong: "L" }), hqAt(7, 17, { dir: "left", pong: "R" }),
  hqAt(26, 18, { dir: "up" }), hqAt(27, 18, { dir: "up" }), hqAt(17, 17, { dir: "up" }),
];
const HQ_ART = {
  floors: { plank: "floors/floor_5.png", grid: "floors/floor_3.png", plain: "floors/floor_1.png", check: "floors/floor_7.png" },
  f: ["DESK_FRONT", "PC_FRONT_OFF", "PC_FRONT_ON_1", "PC_FRONT_ON_2", "PC_FRONT_ON_3", "CUSHIONED_CHAIR_BACK",
    "SOFA_FRONT", "PLANT", "PLANT_2", "LARGE_PLANT", "COFFEE", "BIN", "BOOKSHELF", "SMALL_TABLE_FRONT",
    "LARGE_PAINTING", "HANGING_PLANT", "CACTUS"],
};
const HQ_IMG = {};
let HQ_ART_READY = null;
let HQ_BG = null;
let HQ_FURN = [];

function hqLoadArt() {
  if (HQ_ART_READY) return HQ_ART_READY;
  const jobs = [];
  const one = (key, src) => jobs.push(new Promise((ok) => {
    const i = new Image(); i.onload = () => { HQ_IMG[key] = i; ok(); }; i.onerror = () => ok(); i.src = "/hq-art/" + src;
  }));
  for (let i = 0; i < 6; i++) one("char" + i, `characters/char_${i}.png`);
  for (const [k, s] of Object.entries(HQ_ART.floors)) one("floor_" + k, s);
  for (const f of HQ_ART.f) one(f, `furniture/${f}.png`);
  HQ_ART_READY = Promise.all(jobs).then(() => { HQ_BG = hqPaintRoom(); HQ_FURN = hqFurniture(); });
  return HQ_ART_READY;
}

// ---- loading and the stream ------------------------------------------------------------

async function loadHQ() {
  try {
    const snap = await api("GET", "/v1/hq");
    const first = !HQ.snap;
    HQ.snap = snap; HQ.error = "";
    if (first) {
      // The recent past, in words, without replaying it on screen.
      const from = Math.max(0, (snap.last_event || 0) - 60);
      try {
        const back = await api("GET", `/v1/hq/events?after=${from}`);
        for (const e of back.events || []) hqTell(e, false);
      } catch (_) { /* the log starts empty */ }
      HQ.last = snap.last_event || 0;
    }
    hqPlacePeople();
  } catch (e) {
    HQ.snap = null;
    const said = String((e && e.message) || e);
    HQ.error = /not set up/i.test(said) ? "HQ is not set up on this node yet." : "HQ did not answer: " + said;
  }
  hqLoadArt();
  hqStartPolling();
}

function hqStartPolling() {
  if (HQ.polling) return;
  HQ.polling = setInterval(async () => {
    if (NAV.v !== "hq") { clearInterval(HQ.polling); HQ.polling = null; return; }
    try {
      const got = await api("GET", `/v1/hq/events?after=${HQ.last}`);
      const events = got.events || [];
      if (!events.length) return;
      for (const e of events) { hqTell(e, true); HQ.last = Math.max(HQ.last, e.id); }
      HQ.snap = await api("GET", "/v1/hq");
      hqPlacePeople();
      if (NAV.v === "hq" && !hqTyping()) render();
    } catch (_) { /* the next poll tries again */ }
  }, 3000);
}

// A re-render would throw away what the person is typing; wait for them.
function hqTyping() {
  const a = document.activeElement;
  return !!(a && /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName));
}

function hqMember(id) { return ((HQ.snap && HQ.snap.members) || []).find((m) => m.id === id); }
function hqTask(id) { return ((HQ.snap && HQ.snap.tasks) || []).find((t) => t.id === id); }
function hqName(id) { const m = hqMember(id); return m ? m.name : "someone"; }

// A task's key, as the office writes it: EL-7.
function hqKey(id) { return id ? "EL-" + id : ""; }
// Money in dollars, from the ledger's micro-dollars: $0.40.
function hqDollars(micros) { return "$" + ((micros || 0) / 1e6).toFixed(2); }
// What was really spent apart from what is only held for turns: a free local turn holds money
// while it runs and spends none.
function hqMoney(m, perDay) { return `${hqDollars(m.spent)} spent · ${hqDollars(m.held)} held ${perDay}`; }
// A member on the house's model costs nothing and takes no share of the pool (ADR 0111).
function hqMemberMoney(m) { return m.brain === "local" ? "the house's model: costs nothing" : hqMoney(m, `of ${hqDollars(m.per_day)} a day`); }
// A role as the owner reads it.
// The reviewer's role is shown as the tester (ADR 0114).
function hqRole(role) { return role === "ceo" ? "CEO" : role === "reviewer" ? "Tester" : String(role || ""); }

// A moment as the activity log says it: hours and minutes.
function hqClock(at) { return new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }); }

// What done means for a task, and what was cited for each criterion at each stage (ADR 0114):
// every receipt with its tool, its time and the start of what it gave back. All of it a model
// or a tool wrote, so all of it text.
function hqEvidence(t) {
  const ev = t.evidence || {}, crit = ev.criteria || [], cited = ev.cited || [];
  if (!crit.length && !cited.length) return "";
  const stage = { submitted: "submitted", tested: "the tester passed it", accepted: "the lead accepted it", closed: "closed" };
  const refs = (c) => (c.refs || []).map((r) => `<li>${esc(r.ref)}${r.tool ? ` · ${esc(r.tool)}` : ""}${r.at ? ` · ${esc(hqClock(r.at))}` : ""}${r.ok === false ? " · refused" : ""}${r.excerpt ? `: ${esc(r.excerpt)}` : ""}</li>`).join("");
  const rows = (n) => cited.filter((c) => c.criterion === n)
    .map((c) => `<li>${esc(stage[c.stage] || c.stage)}, ${esc(hqName(c.member))}<ul>${refs(c)}</ul></li>`).join("");
  const items = crit.length
    ? crit.map((c, i) => `<li><b>${esc(c.text)}</b>${c.live ? " · live: a reading the same turn" : ""}<ul>${rows(i + 1) || `<li class="hq-empty">No evidence yet.</li>`}</ul></li>`).join("")
    : `<li><b>The work</b><ul>${rows(0)}</ul></li>`;
  const passed = cited.some((c) => c.stage === "tested") ? "Tester's pass: given." : "Tester's pass: not yet.";
  return `<div class="hq-evidence"><h3>What done means</h3><ul>${items}</ul><p class="hq-who">${passed}</p></div>`;
}

// The turns that ended last on a task, or of a member, with what each came to (ADR 0114): in
// reading order, oldest first like the thread below them, the last five unless the owner asks
// for all. `key` names the card, so "show all" is remembered for it alone.
function hqTurns(match, key) {
  const all = ((HQ.snap && HQ.snap.recent_runs) || []).filter(match).reverse();
  if (!all.length) return "";
  const every = !!HQ.allTurns[key], runs = every ? all : all.slice(-5);
  const toggle = all.length > 5
    ? `<div class="hq-row"><button class="hq-btn" aria-expanded="${every ? "true" : "false"}" data-act="hq:turns:${key}:toggle">${every ? "Show the last 5" : `Show all ${all.length}`}</button></div>` : "";
  return `${toggle}<ul class="hq-thread">${runs.map((r) => `<li>${replayRunLink(r.id, `R${r.id} ${esc(hqName(r.member))}${r.task ? " · " + hqKey(r.task) : ""} · ${esc(r.label || String(r.status || "").replace(/_/g, " "))}`)}</li>`).join("")}</ul>`;
}

// One event, in words for the activity log, and on screen if it is live.
function hqTell(e, live) {
  const d = e.data || {}, key = hqKey(d.task), on = key ? ` (${key})` : "";
  const name = hqName(d.member), reason = String(d.reason || "").replace(/_/g, " ");
  const result = String(d.result || "").replace(/_/g, " ");
  const said = {
    "goal.proposed": ["you", `Endora proposes a goal: ${d.title}`],
    "goal.changed": ["run", `Goal ${d.status}: ${d.title}`],
    "task.created": ["run", `New task ${key}`],
    "task.changed": [d.status === "done" ? "done" : d.status === "blocked" ? "warn" : "run", `${key} is ${String(d.status || "").replace("_", " ")}`],
    "task.claimed": ["run", `${name} checked out ${key}`],
    "task.released": ["warn", `${name} handed ${key} back`],
    "thread.posted": ["run", `${d.author}: ${d.preview}`],
    "wake.queued": ["wake", `Woke ${name}: ${reason}${on}`],
    "wake.folded": ["wake", `${reason} → folded into ${name}'s live run${on}`],
    "wake.held": ["wake", `${name} waits: ${key} is being worked (${reason})`],
    "wake.refused": ["warn", `Wake for ${name} skipped: ${String(d.why || "").replace(/_/g, " ")}`],
    "run.started": ["run", `R${d.run} ${name} starts${key ? " on " + key : ""}${reason ? ` (${reason})` : ""}, ${hqDollars(d.reserved)} held`],
    "run.ended": [d.result === "succeeded" ? "done" : d.result === "retry_waiting" ? "warn" : "bad",
      `R${d.run} ${name} ${d.result === "succeeded" ? "finished" : result}${d.result !== "succeeded" && d.why ? `: ${d.why}` : ""}${d.label ? " · " + d.label : ""}${d.spent ? " · " + hqDollars(d.spent) + " spent" : ""}${d.input != null ? ` · in ${d.input} / out ${d.output || 0}` : ""}${d.cached != null ? ` · cached: ${d.cached}% of input` : ""}`],
    "gate.result": [d.passed ? "done" : "bad", `Gate on ${key}: ${d.passed ? "passed" : "failed"}`],
    "desk.added": ["you", d.kind === "push" ? `On your desk: push ${d.branch || "a branch"}${on}` : d.kind === "hire" ? `On your desk: ${d.name ? d.name + ", " : ""}a ${hqRole(d.role)} to hire${on}` : `On your desk: ${String(d.kind || "").replace(/_/g, " ")}${on}`],
    "desk.decided": ["you", `You decided: ${String(d.decision || "").replace(/_/g, " ")}`],
    "budget.crossed": ["bad", `${name} crossed its limit; R${d.run} stopped`],
    "stopped": ["bad", "You pulled the kill switch: every run stopped, nothing new starts"],
    "resumed": ["you", "You resumed the engine"],
    // Routines and check-backs (ADR 0107): how the team keeps working.
    "routine.changed": ["you", `Routine ${d.title}: ${d.enabled ? "on" : "off"}`],
    "routine.fired": ["wake", `Routine ${d.title} handed out ${key}`],
    "routine.skipped": ["warn", `Routine ${d.title} skipped: ${d.why}`],
    "routine.posted": ["wake", `Alarm into ${key}: ${d.said}`],
    "checkback.set": ["run", `${name} will check back on ${key} in ${d.minutes} min`],
    "lesson.noted": ["you", `${name} noted a lesson: ${d.lesson || ""}`],
    "member.brain": ["you", `${d.name || name} now thinks with ${d.brain === "local" ? "the house's model" : d.brain}`],
    // The CEO and the chain of command (ADR 0111).
    "member.hired": ["you", `${d.name} joined the team as ${hqRole(d.role)}`],
    "member.renamed": ["you", `${d.was} is now ${d.name}`],
    "house.noted": ["you", d.entry ? `${d.by} noted in the house log: ${d.entry}` : `The house log was rewritten by ${d.by === "owner" ? "you" : d.by}`],
    "checkback.fired": ["wake", `${name} checks back on ${key}`],
    "checkback.dropped": ["run", `Check-back on ${key} dropped: it is over`],
    // Projects, their budgets, and whom members report to (ADR 0113).
    "project.changed": [d.status === "paused" || d.status === "dropped" ? "warn" : "you", `Project ${d.title} is ${d.status}: ${d.what || ""}${d.by && d.by !== "owner" ? ", by " + d.by : ""}`],
    "project.budget": [d.level === "spent" ? "bad" : "warn", `Project ${d.title} ${d.said || ""}`],
    "member.reports_to": ["you", `${d.name} now reports to ${d.manager}`],
    // The engineering wing (ADR 0115): branches, their checks and the owner's pushes.
    "branch.submitted": ["run", `${name} submitted ${d.name} (${d.files} files) on ${key}: check c${d.check} queued`],
    "check.done": [d.ok ? "done" : "bad", `Check c${d.check} on ${d.branch}: ${d.summary || (d.ok ? "passed" : "failed")}`],
    // Elastic staffing (ADR 0118): nobody is fired; they are put on furlough and recalled.
    "member.furloughed": ["you", `${d.name} is on furlough`],
    "member.recalled": ["you", `${d.name} is back from furlough`],
    // Notebooks (ADR 0118): what a member wrote down, and what it let go.
    "note.written": ["run", d.by === "owner" ? `You ${d.edited ? "edited a note of" : "added a note to"} ${d.name}'s notebook` : `${d.name} noted: ${d.text}`],
    "note.forgotten": ["run", `${d.by === "owner" ? "You deleted" : d.name + " forgot"} a note${d.by === "owner" ? " of " + d.name + "'s" : ""}: ${d.text}${d.by === "owner" ? "" : " (" + d.reason + ")"}`],
    "branch.pushed": [d.ok ? "you" : "bad", d.ok ? `You pushed ${d.branch} to the fork` : `Pushing ${d.branch} failed`],
    // The open office (ADR 0119): the team decides its decorations together. The words come from
    // HQ, and a member's reason is in them, so the log escapes them like any other.
    "office.proposed": ["run", d.text || ""],
    "office.voted": ["run", d.text || ""],
    "office.changed": [d.via === "owner_undo" || d.via === "owner_reset" ? "you" : "done", d.text || "The office changed"],
    "office.dropped": ["warn", d.text || ""],
    "office.vetoed": ["you", d.text || ""],
  }[e.kind];
  if (said) {
    const at = new Date(e.at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
    HQ.log.unshift({ at, kind: said[0], text: said[1] });
    HQ.log.length = Math.min(HQ.log.length, 80);
  }
  if (e.kind === "tick") HQ.spark = 1;
  if (!live) return;
  const p = HQ.people[d.member];
  if (e.kind.startsWith("wake.") && e.kind !== "wake.refused" && p) {
    HQ.envelopes.push({ x: HQ_P.rack.x * HQ_T + 8, y: HQ_P.rack.y * HQ_T + 10, who: d.member, t: 0 });
  }
  if (e.kind === "thread.posted") {
    const author = ((HQ.snap && HQ.snap.members) || []).find((m) => m.name === d.author);
    if (author && HQ.people[author.id]) hqSay(HQ.people[author.id], d.preview);
    if (author) HQ.lastWord[d.task] = author.id;
  }
  if (e.kind === "task.claimed" && p) hqSay(p, "checkout " + key);
  if (e.kind === "run.ended" && p) hqSay(p, d.result === "succeeded" ? "done" : result);
  if (e.kind === "stopped" || e.kind === "resumed") HQ.doorFlash = 1.5;
  const ceo = hqCeo(), boss = ceo && HQ.people[ceo.id];
  // A member's question woke the one it went to: the one who asked, if between turns, goes to
  // them (the CEO's open door, or the desk of someone on a team) and waits there until their
  // turn on it is over.
  if (e.kind === "wake.queued" && d.reason === "question_asked") {
    const task = hqTask(d.task), who = HQ.lastWord[d.task] || (task && task.owner);
    const asker = who !== d.member && HQ.people[who], there = HQ.seats[d.member] && HQ.seats[d.member].visit;
    if (asker && there && !asker.working) {
      asker.errand = { kind: "ask", task: d.task, to: d.member, hold: 40, spot: { ...there, dir: "up" } };
      hqSay(asker, "a question for " + hqName(d.member)); hqGo(asker);
    }
  }
  if (e.kind === "run.ended") {
    for (const q of Object.values(HQ.people)) if (q.errand && q.errand.kind === "ask" && q.errand.task === d.task && q.errand.to === d.member) { q.errand = null; hqGo(q); }
  }
  // Someone proposed a change to the office: they say so over their head.
  if (e.kind === "office.proposed" && p) hqSay(p, "an idea for the office");
  // The CEO's decision for the board: it takes it to the board room's door, and back.
  if (e.kind === "desk.added" && d.kind === "decision" && boss) {
    boss.errand = { kind: "board", hold: 2.5, say: "for the board", spot: { ...HQ_P.boardDoor, dir: "up" } };
    hqGo(boss);
  }
}

function hqCeo() { return ((HQ.snap && HQ.snap.members) || []).find((m) => m.role === "ceo"); }

// A bubble holds a few words: a status change keeps only what was said about it.
function hqShort(text) { return String(text || "").replace(/^[a-z_]+ → [a-z_]+: /, "").toLowerCase().slice(0, 26); }
function hqSay(p, text) { p.bubble = hqShort(text); p.bubbleT = 3.2; }

// A team's name, on the roster and over its pod alike: Engineering for the engineering wing's
// lead, Ops for any other.
function hqTeamLabel(lead, wingLeads) {
  return `${(wingLeads || []).includes(lead.id) ? "Engineering" : "Ops"} · ${lead.name}`;
}

// The teams, from the snapshot alone (ADR 0119): a lead and those who report to it, up the
// chain; members with no lead above them but the CEO join the first team; the CEO sits in its
// office, outside every team. Within a team the lead first, then those at work, then those on
// furlough, whose desks stay and are drawn empty. With no lead at all there is one team.
function hqTeams(members) {
  const wingLeads = ((HQ.snap && HQ.snap.wing) || {}).leads || [];
  const leads = members.filter((m) => m.role === "lead");
  const rest = members.filter((m) => m.role !== "ceo" && m.role !== "lead");
  if (!leads.length) return rest.length ? [{ lead: null, label: "Team", people: rest }] : [];
  const teams = leads.map((l) => ({ lead: l, label: hqTeamLabel(l, wingLeads), people: [] }));
  for (const m of rest) {
    const lead = hqLeadOf(m, members);
    (teams.find((t) => lead && t.lead.id === lead.id) || teams[0]).people.push(m);
  }
  for (const t of teams) {
    t.people = [t.lead].concat(t.people.filter((m) => !m.furloughed), t.people.filter((m) => m.furloughed));
  }
  return teams;
}

// The pods and the seats (ADR 0119): one pod for each team, side by side from the left of the
// open floor with no walls between, each a cluster of desks two to a row, a desk for every
// member of the team. A desk is three tiles by two; between the two desks of a row is an
// aisle where their members stand facing each other, one above the other's visitors. A pod
// that would not fit in the row starts a new one below; none ever leaves the band the rules
// of the office keep clear. Everything comes from the members: no name is in this.
function hqLayout(members) {
  const P = HQ_POD, seats = {}, pods = [];
  // Two to a row; a team of more than six, whose third row would leave the band, three.
  const sized = hqTeams(members || []).map((team) => {
    const n = team.people.length, cols = n > 6 ? 3 : n > 1 ? 2 : 1;
    return { team, cols, rows: Math.ceil(n / cols), width: cols > 1 ? cols * P.desk.w + (cols - 1) * P.aisle : P.desk.w + 1 };
  });
  // When all the pods fit in one row they are spread across the floor, centred, with room
  // between them, so that two small teams do not leave the other half of the floor bare.
  const total = sized.reduce((sum, s) => sum + s.width, 0), span = P.right - P.left;
  const fits = total + P.between * Math.max(0, sized.length - 1) <= span, free = fits ? span - total : 0;
  const between = fits && sized.length > 1 ? Math.max(P.between, Math.min(4, Math.floor(free / (sized.length + 1)))) : P.between;
  let x = fits ? P.left + Math.floor((free - between * (sized.length - 1)) / 2) : P.left, y = P.top, shelf = 0;
  for (const { team, cols, rows, width } of sized) {
    if (x > P.left && x + width > P.right) { x = P.left; y += shelf; shelf = 0; }
    const top = Math.max(P.top, Math.min(y, P.bottom - rows * P.desk.h));
    const pod = { label: team.label, lead: team.lead ? team.lead.id : null, x, y: top, cols, rows, width, desks: [] };
    team.people.forEach((m, i) => {
      const col = i % cols, row = Math.floor(i / cols);
      const dx = x + col * (P.desk.w + P.aisle), dy = top + row * P.desk.h;
      const stand = cols === 1 ? dx + P.desk.w : col === 0 ? dx + P.desk.w : dx - 1;
      const dir = cols === 1 ? "down" : col === 0 ? "right" : "left";
      seats[m.id] = {
        desk: { x: dx, y: dy, w: P.desk.w, h: P.desk.h, member: m.id },
        // A visitor stands on the desk's outer side, away from where its member sits.
        work: hqAt(stand, dy + 1, { dir, desk: true }),
        visit: cols === 1 || col === 0 ? hqAt(dx - 1, dy + 1, { dir: "right" }) : hqAt(dx + P.desk.w, dy + 1, { dir: "left" }),
        pod: pods.length,
      };
      pod.desks.push(seats[m.id].desk);
    });
    pods.push(pod);
    x += width + between; shelf = Math.max(shelf, rows * P.desk.h);
  }
  const ceo = (members || []).find((m) => m.role === "ceo");
  if (ceo) {
    seats[ceo.id] = { desk: { ...HQ_CEO.desk, w: 3, h: 2, member: ceo.id }, work: HQ_CEO.work, visit: HQ_ROOMS.ceo.out, rest: HQ_CEO.rest, pod: -1 };
  }
  return { pods, seats };
}

// Each member's seat, by id.
function hqSeats(members) { return hqLayout(members).seats; }

// The CEO's daily review: its turn on the task of a routine of its own. The CEO takes the
// head of the meeting table for it.
function hqReview(members) {
  const ceo = (members || []).find((m) => m.role === "ceo");
  const run = ceo && (ceo.running || [])[0], task = run && run.task ? hqTask(run.task) : null;
  if (!task || !task.routine) return false;
  const routine = ((HQ.snap && HQ.snap.routines) || []).find((r) => r.id === task.routine);
  return !!routine && routine.assignee === ceo.id;
}

// Where a working member goes when its turn's work is not at its own desk (ADR 0119): a tester
// on someone's task goes to whose it is; work in another team's project goes to that project
// lead's desk (the CEO's door, for a project the CEO leads). Otherwise nowhere: its own desk.
function hqVisit(m, task) {
  if (!task || m.role === "ceo") return null;
  const seats = HQ.seats, mine = seats[m.id], owner = task.owner || task.assignee;
  if (m.role === "reviewer" && owner && owner !== m.id && seats[owner]) return seats[owner].visit;
  const project = task.project ? ((HQ.snap && HQ.snap.projects) || []).find((x) => x.id === task.project) : null;
  const there = project && project.lead !== m.id ? seats[project.lead] : null;
  return there && mine && there.pod !== mine.pod ? there.visit : null;
}

// Where each member is, kept across redraws: on an errand, in the review, at their desk (or
// where its work is) while a turn runs, drifting about the office otherwise. A furloughed
// member is not there: its desk stands empty. Someone who joins after the office has been
// seen walks in at the front door.
function hqPlacePeople() {
  const members = (HQ.snap && HQ.snap.members) || [], review = hqReview(members);
  const layout = hqLayout(members);
  HQ.pods = layout.pods; HQ.seats = layout.seats; HQ.world = hqWorld(); HQ_FURN = hqFurniture();
  for (const m of members) {
    const seat = HQ.seats[m.id];
    if (m.furloughed) { delete HQ.people[m.id]; continue; }
    let p = HQ.people[m.id];
    if (!p) {
      const from = HQ.settled ? HQ_P.lobby : seat.work;
      p = HQ.people[m.id] = { id: m.id, x: from.x * HQ_T, y: from.y * HQ_T, path: [], frame: 0, bubble: "", bubbleT: 0,
        working: false, errand: null, spotKey: "", at: from, wander: null, idleT: 2 + HQ.rand() * 6 };
    }
    p.look = m.look || 0; p.name = m.name; p.role = m.role; p.paused = m.paused; p.seat = seat;
    const run = (m.running || [])[0];
    const task = run && run.task ? hqTask(run.task) : null;
    // What a working member is doing, said above their head: their latest word on the task.
    const last = task && (task.thread || []).filter((t) => t.author === m.name).pop();
    p.doing = task ? (last ? last.text : "on " + hqKey(task.id)) : "";
    const was = p.working;
    p.working = !!run;
    p.visit = p.working ? hqVisit(m, task) : null;
    if (p.working) p.wander = null;
    if (was && !p.working) p.idleT = 3 + HQ.rand() * 5;
    // The review: the CEO at the head of the table, the lead on a chair if between turns.
    p.meeting = !review ? null : m.role === "ceo" ? HQ_MEETING.head : m.role === "lead" && !p.working ? HQ_MEETING.chairs[0] : null;
    // One who went to ask the CEO and has since been woken goes back to work.
    if (p.working && p.errand && p.errand.kind === "ask") p.errand = null;
    hqGo(p);
  }
  for (const id of Object.keys(HQ.people)) if (!members.some((m) => String(m.id) === id)) delete HQ.people[id];
  HQ.settled = true;
}

// Where a member should be now, as a spot: its cell, the room it is in, and which way they face.
function hqSpot(p) {
  if (p.errand) return p.errand.spot;
  if (p.meeting) return p.meeting;
  const s = p.seat;
  if (p.working) return p.visit || s.work;
  if (p.paused) return s.work;
  return p.wander || s.rest || s.work;
}

// Sets a member walking to where they should be, if that is not where they are going already.
function hqGo(p) {
  if (!p.seat) return;
  const spot = hqSpot(p), key = spot.cx + "," + spot.cy;
  p.at = spot;
  if (key === p.spotKey) return;
  p.spotKey = key;
  hqWalk(p, spot);
}

// Whether the person asked their device for calm: then nobody drifts about the office.
function hqCalm() {
  try { return typeof matchMedia === "function" && !!matchMedia("(prefers-reduced-motion: reduce)").matches; } catch (_) { return false; }
}

// The places an idle member may drift to: the fixed ones, and by what stands on the floor now
// (the couches' seats, the bean bag, a spot before the coffee machine, the fish tank, the
// whiteboard and the shelf), so a thing the team moved moves where people gather.
function hqWanderSpots() {
  const spots = HQ_WANDER.slice();
  for (const it of hqItems()) {
    if (it.kind === "couch") spots.push(hqAt(it.x, it.y, { dir: "down" }), hqAt(it.x + 1, it.y, { dir: "down" }));
    else if (it.kind === "bean_bag") spots.push(hqAt(it.x, it.y, { dir: "down" }));
    else if (["coffee_machine", "fish_tank", "whiteboard", "bookshelf"].includes(it.kind)) spots.push(hqAt(it.x, it.y + 1, { dir: "up" }));
  }
  return spots;
}

// An idle member at rest picks somewhere else to be, now and then, at random: so the teams mix.
function hqIdle(p, dt) {
  if (p.working || p.errand || p.meeting || p.paused || !p.seat || hqCalm()) return;
  p.idleT -= dt;
  if (p.idleT > 0) return;
  // Not where somebody else is going already: two do not take the same couch seat.
  const taken = new Set(Object.values(HQ.people).filter((q) => q !== p).map((q) => q.spotKey));
  const spots = hqWanderSpots().filter((s) => !taken.has(s.cx + "," + s.cy));
  p.idleT = 8 + HQ.rand() * 12;
  if (!spots.length) return;
  p.wander = spots[Math.floor(HQ.rand() * spots.length)];
  hqGo(p);
}

// The cells nobody walks into, and the glass between cells (ADR 0119): the back wall, the rooms
// nobody enters, the desks and everything that stands on the floor, and the walls of the two
// rooms people walk into, but for their open doors. Where a person is going is always allowed:
// they sit at a desk, on a couch and on a chair.
function hqWorld() {
  const blocked = new Set(), edges = new Set(), key = (x, y) => x + "," + y;
  const block = (x, y, w = 1, h = 1) => { for (let j = y; j < y + h; j++) for (let i = x; i < x + w; i++) blocked.add(key(i, j)); };
  const wall = (a, b) => { edges.add(key(a[0], a[1]) + ">" + key(b[0], b[1])); edges.add(key(b[0], b[1]) + ">" + key(a[0], a[1])); };
  block(0, 0, HQ_COLS, 2);
  const B = HQ_P.boardRoom, E = HQ_P.engine;
  block(B.x, B.y, B.w, B.h); block(E.x, E.y, E.w, E.h);
  for (const r of Object.values(HQ_ROOMS)) {
    for (let j = r.y; j < r.y + r.h; j++) { wall([r.x - 1, j], [r.x, j]); wall([r.x + r.w - 1, j], [r.x + r.w, j]); }
    for (let i = r.x; i < r.x + r.w; i++) {
      if (i >= r.door.from && i < r.door.to) continue;
      if (r.door.side === "bottom") wall([i, r.y + r.h - 1], [i, r.y + r.h]); else wall([i, r.y - 1], [i, r.y]);
    }
  }
  const c = HQ_CEO.desk, m = HQ_MEETING.table;
  block(c.x, c.y, 3, 2); block(24, 2, 2, 1); block(24, 5, 2, 1); block(26, 4); block(22, 5);
  block(m.x, m.y, 2, 2); block(12, 18); block(13, 18); block(14, 17, 2, 1);
  block(26, 16, 2, 2); block(26, 18); block(27, 18); block(28, 16);
  block(HQ_P.pong.x, HQ_P.pong.y, 4, 1); block(16, 15, 4, 2);
  for (const pod of HQ.pods) for (const d of pod.desks) block(d.x, d.y, d.w, d.h);
  for (const it of hqItems()) if (it.kind !== "rug" && !it.wall) block(it.x, it.y, it.w, it.h);
  return { blocked, edges };
}

// The cells from where a member stands to a spot, the way round what is in the way: a search
// over the floor's cells a step at a time. `null` if there is no way; empty if already there.
function hqPathCells(from, to) {
  const W = HQ.world || hqWorld(), key = (x, y) => x + "," + y, goal = key(to.cx, to.cy);
  const prev = new Map([[key(from.cx, from.cy), null]]), queue = [[from.cx, from.cy]];
  for (let at = 0; at < queue.length && !prev.has(goal); at++) {
    const [x, y] = queue[at];
    for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
      const nx = x + dx, ny = y + dy, k = key(nx, ny);
      if (nx < 0 || ny < 2 || nx >= HQ_COLS || ny >= HQ_ROWS || prev.has(k)) continue;
      if (W.edges.has(key(x, y) + ">" + k) || (W.blocked.has(k) && k !== goal)) continue;
      prev.set(k, [x, y]); queue.push([nx, ny]);
    }
  }
  if (!prev.has(goal)) return null;
  const cells = [];
  for (let c = [to.cx, to.cy]; prev.get(key(c[0], c[1])); c = prev.get(key(c[0], c[1]))) cells.unshift({ cx: c[0], cy: c[1] });
  return cells;
}

// The way from where a member is to a spot, out of the room they are in by its open door,
// round the desks and the furniture, in by the door of the room the spot is in. Nobody walks
// through glass. Where there is no way, they go straight.
function hqWalk(p, spot) {
  const T = HQ_T, from = { cx: Math.round(p.x / T), cy: Math.round(p.y / T + 0.5) };
  const cells = hqPathCells(from, spot);
  p.path = cells ? cells.map((c) => ({ x: c.cx * T, y: (c.cy - 0.5) * T })) : [{ x: spot.x * T, y: spot.y * T }];
}

// An errand's end: once there, a member says what they came to say, waits, and goes back.
function hqErrand(p, dt) {
  const e = p.errand;
  if (!e || p.path.length || p.spotKey !== e.spot.cx + "," + e.spot.cy) return;
  if (!e.arrived) { e.arrived = true; if (e.say) hqSay(p, e.say); }
  e.hold -= dt;
  if (e.hold > 0) return;
  p.errand = null; hqGo(p);
}

// ---- drawing ---------------------------------------------------------------------------

function hqTinted(img, color) {
  const c = document.createElement("canvas"); c.width = img.width; c.height = img.height;
  const g = c.getContext("2d"); g.drawImage(img, 0, 0);
  g.globalCompositeOperation = "multiply"; g.fillStyle = color; g.fillRect(0, 0, c.width, c.height);
  g.globalCompositeOperation = "destination-in"; g.drawImage(img, 0, 0);
  return c;
}

const HQ_PIXEL = "8px Silkscreen, monospace";

function hqPaintRoom() {
  const T = HQ_T, P = HQ_P, C = HQ_C, W = HQ_W, H = HQ_H;
  const bg = document.createElement("canvas"); bg.width = W; bg.height = H;
  const g = bg.getContext("2d"); g.imageSmoothingEnabled = false;
  const floor = (key, color, x0, y0, x1, y1) => {
    const img = HQ_IMG["floor_" + key]; if (!img) return;
    const t = hqTinted(img, color);
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) g.drawImage(t, x * T, y * T);
  };
  g.fillStyle = "#d6b07a"; g.fillRect(0, 0, W, H);
  floor("plank", "#e2b57a", 0, 2, HQ_COLS, HQ_ROWS);
  floor("grid", "#d9e1ea", 0, 2, 7, 7);
  floor("plain", "#d9b9a0", 20, 2, 27, 7);
  floor("plain", "#c3c8d0", 27, 2, HQ_COLS, 7);
  // The ping-pong corner, the meeting room, the lounge and the kitchen along the bottom.
  floor("grid", "#8fa6c7", 0, 15, 9, HQ_ROWS);
  floor("grid", "#d9e1ea", 9, 15, 16, HQ_ROWS);
  floor("plain", "#d9c3a5", 16, 15, 24, HQ_ROWS);
  floor("check", "#efe9de", 24, 15, HQ_COLS, HQ_ROWS);
  // The wall.
  g.fillStyle = C.wall; g.fillRect(0, 0, W, 2 * T); g.fillStyle = C.wallTop; g.fillRect(0, 0, W, 5); g.fillStyle = C.trim; g.fillRect(0, 2 * T - 3, W, 3);
  // A window onto the bay, with the bridge.
  const wx = 6, wy = 6; g.fillStyle = "#9fd3f0"; g.fillRect(wx, wy, 52, 20); g.fillStyle = "#cfeaf8"; g.fillRect(wx, wy, 52, 8);
  g.fillStyle = "#4f7fa3"; g.fillRect(wx, wy + 15, 52, 5); g.fillStyle = "#c8452e"; g.fillRect(wx + 14, wy + 4, 2, 12); g.fillRect(wx + 34, wy + 4, 2, 12); g.fillRect(wx + 6, wy + 12, 40, 1);
  for (let i = 0; i < 9; i++) { g.fillRect(wx + 15 + i * 2, wy + 5 + Math.round(Math.abs(i - 4) * 1.4), 1, 1); g.fillRect(wx + 35 - i * 2, wy + 5 + Math.round(Math.abs(i - 4) * 1.4), 1, 1); }
  g.fillStyle = "#ffffff"; g.fillRect(wx + 25, wy, 2, 20); g.strokeStyle = C.trim; g.strokeRect(wx - 0.5, wy - 0.5, 53, 21);
  // The whiteboard: the task board.
  g.fillStyle = C.boardEdge; g.fillRect(P.board.x * T - 2, 3, P.board.w * T + 4, 26); g.fillStyle = C.board; g.fillRect(P.board.x * T, 5, P.board.w * T, 22);
  g.fillStyle = "#2a64c9"; g.fillRect((P.board.x + P.board.w) * T - 22, 23, 14, 2); g.fillStyle = "#d33"; g.fillRect((P.board.x + P.board.w) * T - 22, 20, 8, 1);
  if (HQ_IMG.HANGING_PLANT) g.drawImage(HQ_IMG.HANGING_PLANT, 7 * T - 2, 1);
  // The CEO's painting, on the wall of its office.
  if (HQ_IMG.LARGE_PAINTING) g.drawImage(HQ_IMG.LARGE_PAINTING, 22.2 * T, 0);
  // Glass rooms: the board room, the CEO's office and the server closet along the wall, the
  // meeting room by the lounge. Each has a gap in its glass for a door; the two people walk
  // into have the door itself swung open.
  const glass = (r, tint) => { g.fillStyle = tint || C.glass; g.fillRect(r.x * T, r.y * T, r.w * T, r.h * T); };
  const edge = (x, y, w, h) => { g.fillStyle = C.glassEdge; g.fillRect(x, y, w, h); };
  const swung = (x, y, dir) => {
    const y0 = dir < 0 ? y - 18 : y;
    g.fillStyle = "rgba(150,205,235,.5)"; g.fillRect(x, y0, 3, 18); g.fillStyle = C.glassEdge; g.fillRect(x + 2, y0, 1, 18);
    g.fillStyle = C.trim; g.fillRect(x, y0 + (dir < 0 ? 3 : 13), 2, 2);
  };
  const B = P.boardRoom, O = HQ_ROOMS.ceo, M = HQ_ROOMS.meeting, E = P.engine;
  glass(B); edge((B.x + B.w) * T - 2, B.y * T, 2, B.h * T); edge(0, (B.y + B.h) * T - 2, (B.x + B.w) * T - 22, 2);
  // Mara's office: its door stands open, a two-tile gap in the glass with the leaf swung in.
  glass(O); edge(O.x * T, O.y * T, 2, O.h * T);
  edge((O.x + 2) * T, (O.y + O.h) * T - 2, (O.w - 2) * T, 2);
  swung((O.x + 2) * T - 3, (O.y + O.h) * T - 2, -1);
  glass(E, "rgba(60,70,90,.08)"); edge(E.x * T, E.y * T, 2, E.h * T); edge(E.x * T + 22, (E.y + E.h) * T - 2, E.w * T - 22, 2);
  glass(M); edge(M.x * T, M.y * T, 2, M.h * T); edge((M.x + M.w) * T - 2, M.y * T, 2, M.h * T);
  edge((M.x + 2) * T, M.y * T, (M.w - 2) * T, 2);
  swung((M.x + 2) * T - 3, M.y * T + 2, 1);
  // Ping-pong: the table, with a player at each end.
  const px = P.pong.x * T, py = P.pong.y * T;
  g.fillStyle = C.shadow; g.fillRect(px + 2, py + 13, 66, 4); g.fillStyle = "#1f7a4f"; g.fillRect(px, py, 66, 13); g.fillStyle = "#ffffff"; g.fillRect(px, py + 6, 66, 1); g.fillRect(px + 32, py - 3, 2, 16);
  g.fillStyle = "#3b4048"; g.fillRect(px + 4, py + 13, 2, 4); g.fillRect(px + 60, py + 13, 2, 4);
  // The kitchen and lounge: cold brew and the snack shelf on the lounge's back, shared by all.
  g.fillStyle = "#cfd6dd"; g.fillRect(P.tap.x * T, P.tap.y * T - 6, 14, 20); g.fillStyle = "#3a2a1e"; g.fillRect(P.tap.x * T + 3, P.tap.y * T - 3, 8, 7); g.fillStyle = "#222"; g.fillRect(P.tap.x * T + 6, P.tap.y * T + 5, 2, 3);
  g.fillStyle = "#b8a68a"; g.fillRect(17.8 * T, 15.6 * T, 2.2 * T, 3); g.fillRect(17.8 * T, 16.4 * T, 2.2 * T, 3);
  const snacks = ["#ff4fa3", "#ffb347", "#45c486", "#7fb2ff", "#ef5a5f", "#fff2a8"];
  for (let i = 0; i < 8; i++) { g.fillStyle = snacks[i % snacks.length]; g.fillRect(17.8 * T + 2 + i * 4.4, 15.6 * T - 5 + (i % 2) * 13, 3, 5); }
  // The front door, on the floor's right wall.
  g.fillStyle = "#3d3a44"; g.fillRect(P.door.x * T, P.door.y * T, 2 * T, 3 * T); g.fillStyle = "#9fd3f0"; g.fillRect(P.door.x * T + 4, P.door.y * T + 4, 2 * T - 8, 3 * T - 4);
  // Labels.
  g.font = HQ_PIXEL; g.fillStyle = "rgba(31,29,41,.6)";
  g.fillText("BOARD ROOM", 4, 2 * T + 9); g.fillText("CEO", O.x * T + 5, O.y * T + 9);
  g.fillText("SERVERS", (E.x + 1.6) * T, (E.y + E.h) * T - 4); g.fillText("MEETING", M.x * T + 5, (M.y + M.h) * T - 4);
  g.fillStyle = "rgba(255,255,255,.8)"; g.fillText("PING-PONG", 4, 15 * T + 9); g.fillStyle = "rgba(31,29,41,.6)";
  g.fillText("KITCHEN", 24.6 * T, HQ_ROWS * T - 4); g.fillText("LOUNGE", 16.3 * T, HQ_ROWS * T - 4);
  return bg;
}

// The things on the floor now, from the snapshot's office (ADR 0119): where each stands, how
// big it is (the catalog's size comes with it), who put it there. Nothing here is fixed.
function hqItems() {
  const office = (HQ.snap && HQ.snap.office) || {};
  return (office.items || []).map((i) => ({ ...i, w: i.w || 1, h: i.h || 1, wall: !!i.wall }));
}

// A sprite from the loaded art at a pixel, or false if it is not loaded.
function hqSprite(g, key, x, y, w, h) {
  const img = HQ_IMG[key]; if (!img) return false;
  if (w) g.drawImage(img, Math.round(x), Math.round(y), w, h); else g.drawImage(img, Math.round(x), Math.round(y));
  return true;
}
// A pot with leaves: what a plant is when its art is not loaded.
function hqPot(g, it, leaf) {
  const x = it.x * HQ_T, y = it.y * HQ_T;
  g.fillStyle = "#b5651d"; g.fillRect(x + 4, y + 10, 8, 6); g.fillStyle = leaf; g.fillRect(x + 3, y + 2, 10, 9);
}
// How each kind of thing is drawn, at its tile. Plants, the couch and the shelf use the office's
// art; for the kinds with none, a small pixel shape. Each is given the 2D context and the item.
const HQ_ITEM_ART = {
  plant: (g, it) => hqSprite(g, "PLANT", it.x * HQ_T, (it.y + 1) * HQ_T - 32) || hqPot(g, it, "#3f9b5b"),
  fern: (g, it) => hqSprite(g, "PLANT_2", it.x * HQ_T, (it.y + 1) * HQ_T - 32) || hqPot(g, it, "#5fbf7a"),
  cactus: (g, it) => hqSprite(g, "CACTUS", it.x * HQ_T, (it.y + 1) * HQ_T - 32) || hqPot(g, it, "#6aa84f"),
  palm: (g, it) => hqSprite(g, "LARGE_PLANT", it.x * HQ_T - 8, (it.y + 1) * HQ_T - 48) || hqPot(g, it, "#2e8b57"),
  couch: (g, it) => {
    if (hqSprite(g, "SOFA_FRONT", it.x * HQ_T, it.y * HQ_T)) return;
    g.fillStyle = "#7d6bb0"; g.fillRect(it.x * HQ_T, it.y * HQ_T + 2, it.w * HQ_T, 14); g.fillStyle = "#9a89c9"; g.fillRect(it.x * HQ_T + 2, it.y * HQ_T, it.w * HQ_T - 4, 6);
  },
  bookshelf: (g, it) => {
    if (hqSprite(g, "BOOKSHELF", it.x * HQ_T, it.y * HQ_T)) return;
    g.fillStyle = "#8a5a2b"; g.fillRect(it.x * HQ_T, it.y * HQ_T, it.w * HQ_T, 16);
  },
  lamp: (g, it) => {
    const x = it.x * HQ_T, y = it.y * HQ_T;
    g.fillStyle = HQ_C.shadow; g.fillRect(x + 4, y + 14, 8, 2);
    g.fillStyle = "#3b4048"; g.fillRect(x + 7, y + 4, 2, 10); g.fillRect(x + 5, y + 13, 6, 2);
    g.fillStyle = HQ_C.lamp; g.fillRect(x + 3, y, 10, 5); g.fillStyle = "#fff2a8"; g.fillRect(x + 5, y + 1, 6, 2);
  },
  rug: (g, it) => {
    const x = it.x * HQ_T, y = it.y * HQ_T, w = it.w * HQ_T, h = it.h * HQ_T;
    g.fillStyle = "rgba(185,140,240,.55)"; g.fillRect(x + 1, y + 1, w - 2, h - 2);
    g.fillStyle = "rgba(255,255,255,.4)"; g.fillRect(x + 3, y + 3, w - 6, 1); g.fillRect(x + 3, y + h - 4, w - 6, 1);
    g.fillRect(x + 3, y + 3, 1, h - 6); g.fillRect(x + w - 4, y + 3, 1, h - 6);
  },
  whiteboard: (g, it) => {
    const x = it.x * HQ_T, y = it.y * HQ_T;
    g.fillStyle = "#9aa"; g.fillRect(x + 3, y + 11, 2, 5); g.fillRect(x + 11, y + 11, 2, 5);
    g.fillStyle = HQ_C.boardEdge; g.fillRect(x, y + 1, 16, 11); g.fillStyle = HQ_C.board; g.fillRect(x + 1, y + 2, 14, 9);
    g.fillStyle = "#2a64c9"; g.fillRect(x + 3, y + 4, 8, 1); g.fillStyle = "#d33"; g.fillRect(x + 3, y + 7, 5, 1);
  },
  coffee_machine: (g, it) => {
    const x = it.x * HQ_T, y = it.y * HQ_T;
    g.fillStyle = HQ_C.shadow; g.fillRect(x + 2, y + 14, 12, 2);
    g.fillStyle = "#3b4048"; g.fillRect(x + 3, y + 2, 10, 12); g.fillStyle = "#59606c"; g.fillRect(x + 4, y + 3, 8, 4);
    g.fillStyle = HQ_C.red; g.fillRect(x + 10, y + 4, 2, 2); g.fillStyle = "#fff"; g.fillRect(x + 6, y + 10, 4, 4);
  },
  fish_tank: (g, it) => {
    const x = it.x * HQ_T, y = it.y * HQ_T, swim = Math.round((Math.sin(HQ.t * 1.5 + it.id) + 1) * 3);
    g.fillStyle = "#6b4f33"; g.fillRect(x, y + 11, 16, 5);
    g.fillStyle = "rgba(127,178,255,.55)"; g.fillRect(x + 1, y + 3, 14, 8); g.fillStyle = "#e8d7a0"; g.fillRect(x + 1, y + 10, 14, 1);
    g.fillStyle = HQ_C.lamp; g.fillRect(x + 3 + swim, y + 6, 3, 2); g.fillStyle = HQ_C.neon; g.fillRect(x + 10 - swim, y + 8, 2, 1);
    g.fillStyle = "rgba(255,255,255,.5)"; g.fillRect(x + 2, y + 4, 1, 5);
  },
  bean_bag: (g, it) => {
    const x = it.x * HQ_T + 8, y = it.y * HQ_T + 9;
    g.fillStyle = HQ_C.shadow; g.fillRect(x - 6, y + 5, 12, 2);
    g.fillStyle = "#ef7aa6"; g.beginPath(); g.arc(x, y, 6, 0, Math.PI * 2); g.fill();
    g.fillStyle = "#f7a3c2"; g.fillRect(x - 3, y - 4, 4, 2);
  },
  poster: (g, it) => {
    const x = it.x * HQ_T + 2;
    g.fillStyle = "#ffffff"; g.fillRect(x, 5, 12, 17); g.fillStyle = HQ_C.neon; g.fillRect(x + 1, 6, 10, 7);
    g.fillStyle = "#2a64c9"; g.fillRect(x + 2, 15, 8, 1); g.fillRect(x + 2, 18, 5, 1);
  },
  art: (g, it) => {
    if (HQ_IMG.LARGE_PAINTING) { g.drawImage(HQ_IMG.LARGE_PAINTING, it.x * HQ_T, 6, 16, 16); return; }
    g.fillStyle = "#8a5a2b"; g.fillRect(it.x * HQ_T + 1, 6, 14, 14); g.fillStyle = HQ_C.blue; g.fillRect(it.x * HQ_T + 3, 8, 10, 10);
  },
  // The wall clock shows the arc to the next tick of the real engine.
  clock: (g, it) => {
    const snap = HQ.snap || {}, cx = it.x * HQ_T + 8, cy = 14, every = (snap.tick_seconds || 30) * 1000;
    const since = snap.last_tick_at ? Math.max(0, Date.now() - snap.last_tick_at) % every : 0;
    g.fillStyle = "#ffffff"; g.beginPath(); g.arc(cx, cy, 8, 0, Math.PI * 2); g.fill(); g.strokeStyle = HQ_C.trim; g.stroke();
    g.fillStyle = snap.stopped ? HQ_C.red : HQ_C.neon; g.beginPath(); g.moveTo(cx, cy);
    g.arc(cx, cy, 7, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * (since / every)); g.fill();
  },
  string_lights: (g, it) => {
    const x0 = it.x * HQ_T, y0 = it.y * HQ_T + 3, span = it.w * HQ_T, colors = [HQ_C.red, HQ_C.lamp, HQ_C.green, HQ_C.blue, HQ_C.neon];
    const sag = (dx) => Math.round(Math.sin((dx / span) * Math.PI) * 4);
    g.fillStyle = "#6b6577"; for (let dx = 0; dx < span; dx += 2) g.fillRect(x0 + dx, y0 + sag(dx), 2, 1);
    for (let i = 0; i < 6; i++) {
      const dx = 2 + i * Math.floor((span - 4) / 5);
      g.fillStyle = (Math.floor(HQ.t * 2) + i) % 3 !== 0 ? colors[i % colors.length] : "#8a8396"; g.fillRect(x0 + dx, y0 + 1 + sag(dx), 2, 3);
    }
  },
};
// A kind this screen has no picture for yet is drawn as a plain box, so nothing vanishes.
function hqDrawItem(g, it) {
  (HQ_ITEM_ART[it.kind] || ((ctx, i) => { ctx.fillStyle = "#b8a68a"; ctx.fillRect(i.x * HQ_T + 1, i.y * HQ_T + 1, i.w * HQ_T - 2, i.h * HQ_T - 2); }))(g, it);
}

function hqFurniture() {
  const T = HQ_T, out = [], c = HQ_CEO.desk, m = HQ_MEETING.table, members = (HQ.snap && HQ.snap.members) || [];
  const f = (key, x, y, extra = {}) => { const i = HQ_IMG[key]; if (i) out.push({ key, x, y, base: y + i.height + (extra.lift || 0), ...extra }); };
  const ceo = members.find((x) => x.role === "ceo");
  // Every pod's desks, a PC on each, lit while its member works; a furloughed member's stands
  // empty with a small card that says it is away (ADR 0119).
  for (const pod of HQ.pods) {
    for (const d of pod.desks) {
      f("DESK_FRONT", d.x * T, d.y * T);
      f("PC_FRONT_OFF", d.x * T + 4, d.y * T - 6, { lift: 0.5, pcOf: d.member });
      const away = members.find((x) => x.id === d.member && x.furloughed);
      if (away) out.push({ base: d.y * T + 33, draw: (g) => hqAwayCard(g, d) });
    }
  }
  // The board room: the owner's table, with the decisions on it.
  f("SMALL_TABLE_FRONT", HQ_P.owner.x * T, HQ_P.owner.y * T);
  f("COFFEE", (HQ_P.owner.x + 1.3) * T, HQ_P.owner.y * T + 4, { lift: 40 });
  f("CUSHIONED_CHAIR_BACK", (HQ_P.owner.x + 0.1) * T, (HQ_P.owner.y + 1.9) * T);
  f("CUSHIONED_CHAIR_BACK", (HQ_P.owner.x + 1.1) * T, (HQ_P.owner.y + 1.9) * T);
  f("PLANT_2", 0.2 * T, 4.8 * T);
  // Mara's office: its desk and PC, a visitor's chair before it, the bookshelf, a sofa, a plant.
  f("DESK_FRONT", c.x * T, c.y * T);
  f("PC_FRONT_OFF", c.x * T + 4, c.y * T - 6, { lift: 0.5, pcOf: ceo ? ceo.id : -1 });
  f("CUSHIONED_CHAIR_BACK", 22 * T, 5 * T);
  f("BOOKSHELF", 24 * T, 2 * T); f("SOFA_FRONT", 24 * T, 5 * T); f("PLANT", 26 * T, 4 * T);
  // The meeting room: the table, two chairs before it, a large plant in the corner.
  f("SMALL_TABLE_FRONT", m.x * T, m.y * T);
  f("CUSHIONED_CHAIR_BACK", 12 * T, 18 * T);
  f("CUSHIONED_CHAIR_BACK", 13 * T, 18 * T);
  f("LARGE_PLANT", 14 * T, 15 * T);
  // The kitchen: a table, two chairs, a bin. The couches, the coffee machine and the rest of
  // what makes it a lounge are the team's to place.
  f("SMALL_TABLE_FRONT", 26 * T, 16 * T);
  f("CUSHIONED_CHAIR_BACK", 26 * T, 18 * T); f("CUSHIONED_CHAIR_BACK", 27 * T, 18 * T);
  f("BIN", 28 * T, 16 * T);
  // Whatever stands on the floor now, as the snapshot has it: not the rugs and what hangs on the
  // wall, which are drawn flat on the ground.
  for (const it of hqItems()) {
    if (it.kind === "rug" || it.wall) continue;
    out.push({ base: (it.y + it.h) * T, item: it, draw: (g) => hqDrawItem(g, it) });
  }
  return out;
}

// The card on an empty desk: its member is away on furlough (ADR 0118), and keeps the desk.
function hqAwayCard(g, d) {
  const x = d.x * HQ_T + 12, y = d.y * HQ_T + 13;
  g.fillStyle = HQ_C.shadow; g.fillRect(x + 1, y + 1, 24, 9);
  g.fillStyle = "#ffffff"; g.fillRect(x, y, 24, 9); g.fillStyle = HQ_C.blue; g.fillRect(x, y, 24, 1);
  g.font = HQ_PIXEL; g.fillStyle = "rgba(31,29,41,.75)"; g.fillText("AWAY", x + 3, y + 8);
}

// The sticky notes: the newest tasks that are not long finished, two rows of seventeen. A
// dropped goal's tasks are gone from the board, finished or not: the retired demo's are
// hundreds of done notes that would bury the owner's own work.
function hqCards() {
  const goals = (HQ.snap && HQ.snap.goals) || [];
  const dropped = new Set(goals.filter((g) => g.status === "dropped").map((g) => g.id));
  const tasks = ((HQ.snap && HQ.snap.tasks) || []).filter((t) => t.status !== "dropped" && !dropped.has(t.goal));
  const shown = tasks.filter((t, i) => t.status !== "done" || i >= tasks.length - 34).slice(-34);
  return shown.map((t, i) => ({ t, x: HQ_P.board.x * HQ_T + 4 + (i % 17) * 11, y: 7 + Math.floor(i / 17) * 10, w: 8, h: 8 }));
}

// Whoever is at the two ends of the ping-pong table, both of them idle and standing there,
// play. Any two: the teams mix at the table as anywhere else.
function hqPong() {
  const at = (end) => Object.values(HQ.people).find((p) => p.at && p.at.pong === end && !p.working && !p.path.length && !p.paused && !p.errand && !p.meeting);
  const left = at("L"), right = at("R");
  return left && right ? [left, right] : null;
}

function hqFrame(p) {
  if (p.path.length) {
    const n = p.path[0], dx = n.x - p.x, dy = n.y - p.y;
    const dir = Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? "right" : "left") : dy > 0 ? "down" : "up";
    return { dir, frame: [0, 1, 2, 1][Math.floor(p.frame) % 4] };
  }
  const at = p.at || {}, dir = at.dir || "down";
  const reads = p.role === "researcher" || p.role === "reviewer" || !at.desk;
  if (p.working) return { dir, frame: reads ? 5 + (Math.floor(HQ.t * 1.5) % 2) : 3 + (Math.floor(HQ.t * 6) % 2) };
  const pong = hqPong();
  if (pong && pong.includes(p)) return { dir: at.pong === "L" ? "right" : "left", frame: 3 + (Math.floor(HQ.t * 4) % 2) };
  return { dir, frame: p.paused ? 1 : 0 };
}

// When idle, who says what.
const HQ_IDLE = { researcher: "zZ", reviewer: "heads down", lead: "", developer: "" };

function hqDraw() {
  const cv = document.getElementById("hq-office");
  if (!cv || !HQ_BG) return;
  const ctx = cv.getContext("2d"), T = HQ_T, P = HQ_P, C = HQ_C, W = HQ_W, H = HQ_H, snap = HQ.snap || {};
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(HQ_BG, 0, 0);
  // The board: a sticky note per task, coloured by where it is; a pink corner while it is worked.
  for (const r of hqCards()) {
    const asks = (snap.desk || []).some((d) => d.task === r.t.id);
    ctx.fillStyle = "rgba(0,0,0,.12)"; ctx.fillRect(r.x + 1, r.y + 1, r.w, r.h);
    ctx.fillStyle = asks ? C.violet : HQ_STATUS_COL[r.t.status] || C.sticky; ctx.fillRect(r.x, r.y, r.w, r.h);
    ctx.fillStyle = "rgba(0,0,0,.18)"; ctx.fillRect(r.x + 2, r.y + 3, r.w - 4, 1); ctx.fillRect(r.x + 2, r.y + 5, r.w - 5, 1);
    if ((snap.runs || []).some((run) => run.task === r.t.id && run.status === "running")) { ctx.fillStyle = C.neon; ctx.fillRect(r.x + r.w - 2, r.y, 2, 2); }
    if (HQ.selected && HQ.selected.type === "task" && HQ.selected.id === r.t.id) { ctx.strokeStyle = C.neon; ctx.strokeRect(r.x - 1.5, r.y - 1.5, r.w + 3, r.h + 3); }
  }
  // The neon sign, with a flicker.
  const flick = Math.floor(HQ.t * 3) % 23 === 0;
  ctx.font = HQ_PIXEL; ctx.fillStyle = flick ? "rgba(255,79,163,.35)" : C.neon;
  ctx.shadowColor = C.neon; ctx.shadowBlur = flick ? 0 : 6; ctx.fillText("SHIP IT", 66, 19); ctx.shadowBlur = 0;
  // The server rack: its lights spark on each tick, red when stopped.
  const rx = P.rack.x * T, ry = P.rack.y * T;
  ctx.fillStyle = C.shadow; ctx.fillRect(rx + 2, ry + 38, 18, 4);
  ctx.fillStyle = C.rack; ctx.fillRect(rx, ry, 18, 40); ctx.fillStyle = "#333b4a"; ctx.fillRect(rx + 2, ry + 2, 14, 36);
  const ticks = (snap.counts && snap.counts.ticks) || 0;
  for (let i = 0; i < 7; i++) {
    const on = (ticks + i) % 3 === 0 || (HQ.spark || 0) > 0.5;
    ctx.fillStyle = snap.stopped ? C.red : on ? C.green : "#24412f"; ctx.fillRect(rx + 4, ry + 5 + i * 5, 2, 2);
    ctx.fillStyle = snap.stopped ? "#5a2224" : "#4a5a6a"; ctx.fillRect(rx + 8, ry + 5 + i * 5, 6, 2);
  }
  // What lies flat on the floor or hangs on the back wall, from the snapshot's office (ADR 0119):
  // rugs, posters, art, string lights, and the wall clock with its arc to the next tick.
  for (const it of hqItems()) if (it.kind === "rug" || it.wall) hqDrawItem(ctx, it);
  // The pods: a faint patch of floor under each team's desks and its name, no walls.
  ctx.font = HQ_PIXEL;
  for (const pod of HQ.pods) {
    ctx.fillStyle = "rgba(255,255,255,.12)"; ctx.fillRect(pod.x * T - 4, pod.y * T - 18, pod.width * T + 8, pod.rows * HQ_POD.desk.h * T + 20);
    ctx.fillStyle = "rgba(31,29,41,.7)"; ctx.fillText(String(pod.label).toUpperCase(), pod.x * T, pod.y * T - 9);
  }
  // The burn gauge: today's spend against the pool's day.
  const pool = snap.pool || { per_day: 1, committed: 0 }, fill = Math.min(1, (pool.committed || 0) / Math.max(1, pool.per_day));
  const jx = P.jar.x * T, jy = P.jar.y * T, hgt = 30;
  ctx.fillStyle = "#ffffff"; ctx.fillRect(jx, jy, 8, hgt); ctx.strokeStyle = C.trim; ctx.strokeRect(jx - 0.5, jy - 0.5, 9, hgt + 1);
  ctx.fillStyle = fill > .85 ? C.red : fill > .6 ? C.lamp : C.green; ctx.fillRect(jx + 1, jy + hgt - Math.round(hgt * fill), 6, Math.round(hgt * fill));
  ctx.font = HQ_PIXEL; ctx.fillStyle = "rgba(31,29,41,.65)"; ctx.fillText("BURN", jx + 12, jy + hgt);
  // The kill switch on the board room's glass.
  const lx = P.lever.x * T, ly = P.lever.y * T, up = !snap.stopped;
  ctx.fillStyle = "#3a3a46"; ctx.fillRect(lx + 2, ly + 14, 12, 4);
  ctx.fillStyle = "#9aa"; ctx.fillRect(lx + 7, up ? ly + 4 : ly + 10, 2, up ? 10 : 6); ctx.fillStyle = C.red; ctx.fillRect(lx + 5, up ? ly + 1 : ly + 14, 6, 4);
  if ((HQ.doorFlash || 0) > 0) { ctx.fillStyle = `rgba(255,79,163,${Math.min(.35, HQ.doorFlash / 3)})`; ctx.fillRect(P.door.x * T - 4, P.door.y * T - 4, 2 * T + 8, 3 * T + 4); }
  // A ping-pong ball between the two at the table.
  if (hqPong()) { const k = (Math.sin(HQ.t * 4) + 1) / 2, bx = P.pong.x * T + 6 + k * 56, by = P.pong.y * T + 2 - Math.abs(Math.sin(HQ.t * 8)) * 9; ctx.fillStyle = "#ffffff"; ctx.fillRect(Math.round(bx), Math.round(by), 2, 2); }
  // People and furniture, in depth order.
  const people = Object.values(HQ.people);
  const items = HQ_FURN.map((f) => ({ base: f.base, f }));
  for (const p of people) items.push({ base: p.y + 16, p });
  items.sort((a, b) => a.base - b.base);
  for (const it of items) {
    if (it.p) { hqDrawPerson(ctx, it.p); continue; }
    if (it.f.draw) { it.f.draw(ctx); continue; }
    let key = it.f.key;
    if (it.f.pcOf !== undefined) {
      const at = people.find((p) => p.working && !p.path.length && p.at && p.at.desk && p.id === it.f.pcOf);
      key = at ? "PC_FRONT_ON_" + (1 + (Math.floor(HQ.t * 4) % 3)) : "PC_FRONT_OFF";
    }
    if (HQ_IMG[key]) ctx.drawImage(HQ_IMG[key], Math.round(it.f.x), Math.round(it.f.y));
  }
  // Names under their people; at a pod's aisle they run out over their own desk, so the two
  // who face each other do not write over one another.
  ctx.font = HQ_PIXEL;
  for (const p of people) {
    const name = String(p.name || "").toUpperCase(), at = p.at || {}, w = Math.ceil(ctx.measureText(name).width);
    const x = at.desk && at.dir === "right" ? Math.round(p.x) + 16 - w : at.desk && at.dir === "left" ? Math.round(p.x) : Math.round(p.x) + 1;
    ctx.fillStyle = "rgba(31,29,41,.85)"; ctx.fillText(name, x, Math.round(p.y) + 24);
  }
  // Decisions waiting on the board room table.
  (snap.desk || []).slice(0, 5).forEach((n, i) => {
    ctx.fillStyle = n.kind === "goal" ? C.violet : n.kind === "stuck" ? C.red : C.lamp;
    ctx.fillRect(P.owner.x * T + 3 + i * 6, P.owner.y * T + 6 + (i % 2) * 3, 5, 6);
    ctx.fillStyle = "rgba(0,0,0,.25)"; ctx.fillRect(P.owner.x * T + 4 + i * 6, P.owner.y * T + 8 + (i % 2) * 3, 3, 1);
  });
  // Wakes in flight, from the server closet to whoever was woken.
  for (const e of HQ.envelopes) {
    const p = HQ.people[e.who]; if (!p) continue;
    const k = Math.min(1, e.t / 0.9), x = e.x + (p.x + 6 - e.x) * k, y = e.y + (p.y - 22 - e.y) * k - Math.sin(k * Math.PI) * 18;
    ctx.fillStyle = "#ffffff"; ctx.fillRect(Math.round(x), Math.round(y), 7, 5); ctx.fillStyle = C.neon; ctx.fillRect(Math.round(x) + 1, Math.round(y) + 1, 5, 1);
  }
  if (snap.stopped) { ctx.fillStyle = "rgba(239,90,95,.12)"; ctx.fillRect(0, 0, W, H); ctx.font = HQ_PIXEL; ctx.fillStyle = C.red; ctx.fillText("KILL SWITCH PULLED", W / 2 - 44, 2 * T + 22); }
  for (const p of people) hqBubble(ctx, p);
}

// A speech bubble: what was just said, else what they are doing, else their idle word.
function hqBubble(ctx, p) {
  const pong = hqPong(), playing = pong && pong.includes(p);
  const said = p.bubbleT > 0 ? p.bubble : "";
  const doing = p.working && !p.path.length ? hqShort(p.doing) : "";
  const idle = !p.working && !playing && !p.path.length && !p.errand && !p.meeting ? (p.paused ? "over budget" : HQ_IDLE[p.role] || "") : "";
  const text = said || doing || idle;
  if (!text) return;
  const quiet = !said && !doing;
  ctx.font = HQ_PIXEL;
  const tw = Math.ceil(ctx.measureText(text).width) + 6, x = Math.round(Math.min(HQ_W - tw - 2, Math.max(2, p.x + 8 - tw / 2))), y = Math.round(p.y - 30);
  ctx.fillStyle = quiet ? "rgba(255,255,255,.75)" : "#ffffff"; ctx.fillRect(x, y, tw, 10);
  ctx.fillStyle = quiet ? "rgba(31,29,41,.25)" : "#1f1d29"; ctx.fillRect(x, y + 10, tw, 1); ctx.fillRect(Math.round(p.x + 7), y + 10, 2, 2);
  ctx.fillStyle = quiet ? "#6b6577" : "#1f1d29"; ctx.fillText(text, x + 3, y + 8);
}

function hqDrawPerson(ctx, p) {
  const img = HQ_IMG["char" + (p.look % 6)], x = Math.round(p.x), y = Math.round(p.y);
  ctx.fillStyle = HQ_C.shadow; ctx.fillRect(x + 3, y + 13, 10, 3);
  if (img) {
    const { dir, frame } = hqFrame(p), row = dir === "up" ? 1 : dir === "down" ? 0 : 2;
    if (dir === "left") { ctx.save(); ctx.translate(x + 16, 0); ctx.scale(-1, 1); ctx.drawImage(img, frame * 16, row * 32, 16, 32, 0, y - 16, 16, 32); ctx.restore(); }
    else ctx.drawImage(img, frame * 16, row * 32, 16, 32, x, y - 16, 16, 32);
  }
  if (p.paused) { ctx.fillStyle = HQ_C.lamp; ctx.fillRect(x + 13, y - 20, 6, 6); ctx.fillStyle = "#8a5a1c"; ctx.fillRect(x + 15, y - 19, 2, 4); }
  if (HQ.selected && HQ.selected.type === "member" && HQ.selected.id === p.id) { ctx.strokeStyle = HQ_C.neon; ctx.lineWidth = 1; ctx.strokeRect(x + 0.5, y - 16.5, 16, 33); }
}

function hqLoop() {
  // Only where something draws: not in a background tab's sandbox, nor in the console check.
  if (HQ.looping || typeof requestAnimationFrame !== "function") return;
  HQ.looping = true;
  let last = Date.now();
  const step = () => {
    if (NAV.v !== "hq") { HQ.looping = false; return; }
    const now = Date.now(), dt = Math.min(0.1, (now - last) / 1000); last = now; HQ.t += dt;
    HQ.spark = Math.max(0, (HQ.spark || 0) - dt * 2); HQ.doorFlash = Math.max(0, (HQ.doorFlash || 0) - dt);
    for (const p of Object.values(HQ.people)) {
      if (p.bubbleT > 0) p.bubbleT -= dt;
      if (!p.path.length) { hqErrand(p, dt); hqIdle(p, dt); continue; }
      const n = p.path[0], dx = n.x - p.x, dy = n.y - p.y, d = Math.hypot(dx, dy), v = 46 * dt;
      if (d <= v) { p.x = n.x; p.y = n.y; p.path.shift(); } else { p.x += (dx / d) * v; p.y += (dy / d) * v; }
      p.frame += dt * 8;
    }
    HQ.envelopes.forEach((e) => (e.t += dt)); HQ.envelopes = HQ.envelopes.filter((e) => e.t < 0.9);
    hqDraw();
    requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

// A tap on the office: a person, a sticky note, or the kill switch.
function hqTap(ev) {
  const cv = document.getElementById("hq-office"); if (!cv) return;
  const b = cv.getBoundingClientRect(), x = (ev.clientX - b.left) * (HQ_W / b.width), y = (ev.clientY - b.top) * (HQ_H / b.height);
  for (const p of Object.values(HQ.people)) {
    if (x >= p.x && x <= p.x + 16 && y >= p.y - 16 && y <= p.y + 16) { HQ.selected = { type: "member", id: p.id }; render(); return; }
  }
  for (const r of hqCards()) {
    if (x >= r.x - 2 && x <= r.x + r.w + 2 && y >= r.y - 2 && y <= r.y + r.h + 2) { HQ.selected = { type: "task", id: r.t.id }; render(); return; }
  }
  if (x >= HQ_P.lever.x * HQ_T && x <= HQ_P.lever.x * HQ_T + 16 && y >= HQ_P.lever.y * HQ_T && y <= HQ_P.lever.y * HQ_T + 20) {
    dispatch(HQ.snap && HQ.snap.stopped ? "hq:resume" : "hq:stop"); return;
  }
  // A thing on the floor: who put it there.
  for (const it of hqItems()) {
    if (x >= it.x * HQ_T && x <= (it.x + it.w) * HQ_T && y >= (it.wall ? 0 : it.y * HQ_T - 16) && y <= (it.y + it.h) * HQ_T) {
      HQ.selected = { type: "item", id: it.id }; render(); return;
    }
  }
  HQ.selected = null; render();
}

// The office's fonts, the same as its design: loaded once, only when HQ opens.
function hqFonts() {
  if (typeof document === "undefined" || !document.head || document.getElementById("hq-fonts")) return;
  const link = document.createElement("link");
  link.id = "hq-fonts"; link.rel = "stylesheet";
  link.href = "https://fonts.googleapis.com/css2?family=Silkscreen&family=IBM+Plex+Mono:wght@400;500;600&display=swap";
  document.head.appendChild(link);
}

// ---- the screen ------------------------------------------------------------------------

function viewHQ() {
  if (HQ.error) return `<div class="hq"><h1>Endora HQ</h1><section class="hq-card"><p class="hq-empty">${esc(HQ.error)}</p></section></div>`;
  const s = HQ.snap;
  if (!s) return `<div class="hq"><h1>Endora HQ</h1><p class="hq-empty">Loading…</p></div>`;
  setTimeout(() => {
    hqFonts(); hqLoop();
    const cv = document.getElementById("hq-office");
    if (cv && !cv.dataset.tapped) { cv.dataset.tapped = "1"; cv.addEventListener("click", hqTap); }
  }, 0);
  const pool = s.pool || {}, c = s.counts || {};
  const stats = [["Tick", c.ticks || 0], ["Running", c.running || 0], ["Queued", c.queued || 0], ["Waiting", c.held || 0],
    ["Coalesced", c.folded || 0], ["Today", hqMoney(pool, `· ${hqDollars(pool.per_day)} a day`)], ["Open tasks", c.open_tasks || 0]]
    .map(([k, v]) => `<span class="hq-stat">${k} <b>${v}</b></span>`).join("");
  const desk = (s.desk || []).map((d) => {
    const choices = { goal: [["approve", "Approve", "ok"], ["decline", "Decline", "no"]], stuck: [["retry", "Try again", "ok"], ["drop", "Drop it", "no"]],
      ready_to_merge: [["merged", "Mark merged", "ok"], ["not_now", "Not now", "no"]],
      ready_to_read: [["read", "Mark read", "ok"], ["not_now", "Not now", "no"]],
      // A member's question (the team protocol's ask_owner): seeing it wakes the member who
      // asked, to read the task's thread, where the answer typed on the card is written first.
      question: [["seen", "Answer & wake", "ok"], ["dismiss", "Dismiss", "no"]],
      // Something a member did through Endora's contracts, such as a restart: only to know of.
      notice: [["seen", "Seen", "ok"], ["dismiss", "Dismiss", "no"]],
      // An action a member proposed through Endora's bridge (ADR 0108): approving runs exactly
      // what the card shows, once; declining runs nothing.
      approval: [["approve", "Approve", "ok"], ["decline", "Decline", "no"]],
      // The CEO's decision for the board (ADR 0111): approve, decline, or answer in words; what
      // the owner writes goes in the task's thread, and the CEO is woken to act on it.
      decision: [["approve", "Approve", "ok"], ["decline", "Decline", "no"], ["answer", "Answer", ""]],
      // A cap the CEO asks for a project (ADR 0113): granted as asked, or at what the owner
      // writes; or not at all, and the project runs on the pool's limits alone.
      project_cap: [["approve", "Grant the cap", "ok"], ["decline", "No cap", "no"]],
      // A project that spent its cap: its Claude turns wait until the owner raises the cap, or
      // the month turns and the pause lifts itself.
      project_budget: [["raise", "Raise the cap", "ok"], ["wait", "Wait for next month", "no"]],
      // Work sent back three times (ADR 0114): its next round is the owner's alone.
      rounds: [["accept", "Accept as done", "ok"], ["send_back", "Send it back", "no"]],
      // The wing (ADR 0115): a lead's request to hire, and an accepted branch to push to the
      // fork. Without a deploy key on the node there is nothing to approve: the card offers
      // the branch's bundle to download instead.
      hire: [["approve", "Hire", "ok"], ["decline", "Decline", "no"]],
      // Elastic staffing (ADR 0118): an idle member offered for furlough (never fired), and a
      // lead's request to bring a furloughed member back.
      furlough: [["approve", "Furlough", "ok"], ["decline", "Keep", "no"]],
      recall: [["approve", "Bring back", "ok"], ["decline", "Decline", "no"]],
      // A member at twice its share with the pool's day not spent (ADR 0113): an allowance for
      // the rest of today, at the amount written on the card, or wait for tomorrow.
      out_for_day: [["approve", "Give it today", "ok"], ["decline", "Wait for tomorrow", "no"]],
      push: hqCanPush(d) ? [["approve", "Push it", "ok"], ["decline", "Decline", "no"]] : [["seen", "Seen", "ok"], ["dismiss", "Dismiss", "no"]] }[d.kind] || [];
    // In the demo nothing behind a card is real: say so on the card, not only in the header.
    const demoNote = s.demo ? (d.kind === "ready_to_merge" ? " · demo: there is no pull request behind this"
      : d.kind === "ready_to_read" ? " · demo: there is no write-up behind this" : " · demo") : "";
    const shown = d.kind === "decision" && d.decision ? "The CEO asks the board to decide:" : d.text;
    return `<div class="hq-need ${esc(d.kind)}"><p>${s.demo ? "Demo: " : ""}${esc(shown)}</p>
      <p class="hq-who">${esc(String(d.kind).replace(/_/g, " "))}${d.task ? ` · ${hqKey(d.task)}` : ""}${d.project ? ` · project ${esc(hqProjectTitle(d.project))}` : ""}${demoNote}</p>
      ${d.kind === "approval" ? hqApproval(d.approval) : ""}
      ${d.kind === "decision" ? hqDecision(d.decision) : ""}
      ${d.kind === "push" && !hqCanPush(d) && hqBundleOf(d) ? `<div class="hq-row"><button class="hq-btn" data-act="hq:bundle:${d.task}:${esc(hqBundleOf(d))}">Download the bundle</button></div>` : ""}
      ${d.kind === "question" || d.kind === "decision" ? `<textarea id="hq-answer-${d.id}" rows="2" placeholder="Your answer, for the task's thread (optional)" aria-label="Your answer"></textarea>` : ""}
      ${d.kind === "project_cap" || d.kind === "project_budget" ? `<input id="hq-answer-${d.id}" inputmode="decimal" placeholder="${d.kind === "project_cap" ? "Dollars a month (empty: as asked)" : "The new cap, in dollars a month"}" aria-label="The cap, in dollars a month" />` : ""}
      ${d.kind === "out_for_day" ? `<input id="hq-answer-${d.id}" inputmode="decimal" placeholder="Dollars for today (empty: as offered)" aria-label="The allowance, in dollars for today" />` : ""}
      <div class="hq-row">${choices.map(([v, l, cls]) => `<button class="hq-btn ${cls}" data-act="hq:decide:${d.id}:${v}">${l}</button>`).join("")}</div></div>`;
  }).join("");
  // The latest twenty, newest first: enough to follow the team, short enough to read.
  const log = HQ.log.slice(0, 20).map((l) => `<li><time>${esc(l.at)}</time><span class="k-${l.kind}">${esc(l.text)}</span></li>`).join("");
  const wing = s.wing || {};
  const wingLine = wing.goal ? `<p class="hq-note">The engineering wing (goal "${esc(wing.goal)}") seats ${wing.most_leads || 1} lead on Claude Sonnet and at most ${wing.most_developers || 5} developers on Claude Haiku or the house's model when its lead asks to hire: ${wing.developers || 0} of ${wing.most_developers || 5} developer seats are taken. Your own hiring here is not limited.</p>` : "";
  const team = hqRoster(s);
  return `<div class="hq">
    <div class="hq-bar">
      <h1>Endora HQ</h1>
      ${s.demo ? `<span class="hq-demo">Demo · scripted team</span>` : ""}
      <span class="hq-grow"></span>
      <div class="hq-row">
        <button class="hq-btn" data-act="hq:work">Give the team work</button>
        <button class="hq-btn stop" aria-pressed="${s.stopped ? "true" : "false"}" data-act="${s.stopped ? "hq:resume" : "hq:stop"}">${s.stopped ? "Resume" : "Kill switch"}</button>
      </div>
    </div>
    <div class="hq-main">
      <div class="hq-stage">
        <canvas id="hq-office" width="${HQ_W}" height="${HQ_H}"
          aria-label="The team's open-plan office: a pod of desks for each team, the board of tasks, the CEO's office with its door open, the meeting room, the kitchen and lounge, ping-pong, the server closet, the board room"></canvas>
        <div class="hq-stats">${stats}</div>
        <div class="hq-legend"><span style="--c:#fff2a8">To do</span><span style="--c:#ffb347">Doing</span><span style="--c:#7fb2ff">In review</span><span style="--c:#45c486">Done</span><span style="--c:#ef5a5f">Blocked</span><span style="--c:#b98cf0">Needs you</span></div>
        ${s.demo ? `<p class="hq-note">Demo: nothing is spent; turns reserve as real ones will.</p>` : ""}
      </div>
      <aside class="hq-side">
        <section class="hq-card" aria-live="polite"><h2>Looking at</h2>${hqSelected()}</section>
        <section class="hq-card"><h2>Your desk <span class="hq-count">${(s.desk || []).length}</span></h2>
          <div class="hq-needs">${desk || `<p class="hq-empty">Nothing waiting.</p>`}</div></section>
        ${hqScorecard(s)}
        <section class="hq-card"><h2>Activity</h2><ul class="hq-log" data-keep-scroll="hq-log">${log || `<li class="hq-empty">Nothing yet.</li>`}</ul></section>
        <section class="hq-card"><h2>Team</h2>${team}${wingLine}${hqHireForm(s)}</section>
        ${hqOffice(s)}
        ${hqProjects(s)}
        ${hqHouse(s)}
        <details class="hq-card"><summary>Set the team a goal</summary>
          <div class="hq-row" style="margin-top:8px;"><input id="hq-goal" placeholder="e.g. Keep the home lab healthy" /><button class="hq-btn ok" data-act="hq:goal">Set it</button></div></details>
        ${hqNotes(s)}
        ${hqRoutines(s)}
        ${hqContracts(s.contracts)}
      </aside>
    </div>
    <p class="hq-note">Office art: Pixel Agents (MIT) and MetroCity by JIK-A-4 (CC0); <a href="/hq-art/CREDITS.txt">credits</a>. The board is you: decisions land on your desk, and the kill switch is yours.</p>
  </div>`;
}

// What a member thinks with, in a word for the roster line.
function hqBrainTag(brain) {
  const b = String(brain || "local");
  return b.includes("opus") ? "Opus" : b.includes("sonnet") ? "Sonnet" : b.includes("haiku") ? "Haiku" : "Local";
}

// The lead a member works under: the nearest lead up whom it reports to, or none when the
// chain reaches the CEO (or nobody) first.
function hqLeadOf(m, members) {
  let up = members.find((o) => o.id === m.reports_to);
  for (let hops = 0; up && hops < members.length; hops++) {
    if (up.role === "lead") return up;
    if (up.role === "ceo") return null;
    const next = up.reports_to;
    up = members.find((o) => o.id === next);
  }
  return null;
}

// One member on the roster: name, role, brain, what it is doing and its money today, all on
// one line; tapping it opens its controls (the brain, whom it reports to, a new name, a look at
// it), closed until then.
function hqRosterLine(m, members) {
  const on = (m.running || []).map((r) => r.task ? "#" + r.task : "a look round").join(", ");
  const money = m.brain === "local" ? "costs nothing" : `${hqDollars(m.spent)} today of ${hqDollars(m.per_day)}`;
  const open = !!HQ.roster[m.id];
  const controls = open ? `<div class="hq-controls">
      <p class="hq-who">${hqMemberMoney(m)}</p>${hqBrains(m)}${hqReports(m, members)}
      <div class="hq-row"><button class="hq-btn" data-act="hq:rename:${m.id}">Rename</button>
        <button class="hq-btn" data-act="hq:pick:member:${m.id}">Look at ${esc(m.name)}</button>${hqStaffing(m)}</div>${hqNotebook(m)}</div>` : "";
  return `<li class="hq-member"><button class="hq-line" aria-expanded="${open ? "true" : "false"}" data-act="hq:roster:${m.id}:toggle"><b>${esc(m.name)}</b>
      <span>${esc(hqRole(m.role))} · <span class="hq-tag">${hqBrainTag(m.brain)}</span> · ${m.furloughed ? "on furlough · " : ""}${m.paused ? "paused · " : ""}${m.furloughed ? "" : (on ? "on " + esc(on) : "between turns") + " · "}${money}</span></button>${controls}</li>`;
}

// A member's notebook (ADR 0118), in its panel: each note as text (a member wrote it, so it is
// escaped), with the evidence it rests on or "from the owner", and Edit and Delete; and a box
// to add one. The owner's notes need no evidence. Read from HQ when the panel is opened.
function hqNotebook(m) {
  const notes = HQ.notebooks[m.id];
  if (!notes) return `<p class="hq-note">Reading ${esc(m.name)}'s notebook…</p>`;
  const rows = notes.map((n) => `<li><span>${esc(n.text)}</span>
      <small> ${n.from_owner ? "from the owner" : esc((n.evidence || []).join(", "))}${n.stale ? " · stale" : ""}</small>
      <button class="hq-btn" data-act="hq:notebook:${m.id}:edit-${Number(n.id)}">Edit</button>
      <button class="hq-btn no" data-act="hq:notebook:${m.id}:del-${Number(n.id)}">Delete</button></li>`).join("");
  return `<div class="hq-notebook"><p class="hq-who">${esc(m.name)}'s notebook (${notes.length})</p>
    ${rows ? `<ul class="hq-team">${rows}</ul>` : `<p class="hq-empty">No notes.</p>`}
    <div class="hq-row"><input id="hq-note-new-${m.id}" maxlength="300" placeholder="A note from you, up to 300 characters" aria-label="A note for ${esc(m.name)}" />
      <button class="hq-btn ok" data-act="hq:notebook:${m.id}:add">Add note</button></div></div>`;
}

// Reads one member's notebook from HQ and draws it.
async function hqLoadNotebook(id) {
  try { HQ.notebooks[id] = (await api("GET", `/v1/hq/members/${id}/notebook`)).notes || []; }
  catch (e) { HQ.notebooks[id] = []; flash("Couldn't read the notebook: " + e.message, "err"); }
  render();
}

// A member's furlough (ADR 0118), in its panel: put on furlough, or recalled. The CEO is never
// furloughed; nobody is ever fired, so there is no button for that.
function hqStaffing(m) {
  if (m.role === "ceo") return "";
  return m.furloughed
    ? `<button class="hq-btn ok" data-act="hq:recall:${m.id}">Recall ${esc(m.name)}</button>`
    : `<button class="hq-btn" data-act="hq:furlough:${m.id}">Furlough ${esc(m.name)}</button>`;
}

// The Team card as an org chart (the owner, on a phone: "super cluttered, can't tell the
// teams"): the board, which is the owner, and the CEO with whoever has no lead above them; then
// one group per lead, the lead first and then those who work under it. The wing's lead's is
// Engineering; any other lead's, Ops.
function hqRoster(s) {
  const members = s.members || [], wingLeads = (s.wing && s.wing.leads) || [];
  const line = (m) => hqRosterLine(m, members);
  const ceo = members.filter((m) => m.role === "ceo");
  const leads = members.filter((m) => m.role === "lead");
  const rest = members.filter((m) => m.role !== "ceo" && m.role !== "lead");
  const loose = rest.filter((m) => !hqLeadOf(m, members));
  const group = (title, people) => `<div class="hq-group"><h3>${title}</h3><ul class="hq-team">${people.map(line).join("")}</ul></div>`;
  return group("Board: you", ceo.concat(loose)) + leads.map((l) =>
    group(esc(hqTeamLabel(l, wingLeads)),
      [l].concat(rest.filter((m) => (hqLeadOf(m, members) || {}).id === l.id)))).join("");
}

// Roughly where a thing stands, in words: the same names HQ gives its proposals.
function hqAreaOf(it) {
  const x = it.x, y = it.y, within = (l, t, r, b) => x >= l && x < r && y >= t && y < b;
  if (y < 2) return "the back wall";
  if (within(16, 15, 24, 20)) return "the lounge";
  if (within(24, 15, 32, 20)) return "the kitchen";
  if (within(0, 15, 9, 20)) return "the ping-pong corner";
  if (within(9, 15, 16, 20)) return "the meeting room side";
  if (within(7, 2, 20, 7)) return "the commons";
  if (within(0, 2, 7, 8)) return "the board room side";
  if (within(20, 2, 32, 8)) return "the CEO's side";
  return "the open floor";
}

// The Office card (ADR 0119): what the team proposes to change, who has voted how, and the
// owner's say as landlord: veto any open proposal, undo the last change, reset to how the
// office started, and what paid members' downtime may spend a day (nothing, until you say). The
// proposals are the team's own words, so every one is escaped.
function hqOffice(s) {
  const o = s.office;
  if (!o) return "";
  const open = o.proposals || [], recent = (o.recent || []).slice(0, 3);
  const proposals = open.map((p) => {
    const votes = (p.votes || []).map((v) => `${esc(v.who)} ${esc(v.vote)}${v.why ? ` (${esc(v.why)})` : ""}`).join(", ");
    return `<li><b>${esc(p.by)}</b> proposes: ${(p.changes || []).map((c) => esc(c)).join("; ")}${p.why ? ` — <i>${esc(p.why)}</i>` : ""}
      <small> yes ${Number(p.yes)}, no ${Number(p.no)} · ${votes} · carries on a second yes, or at ${esc(hqClock(p.carries_at))} if nobody objects</small>
      <button class="hq-btn no" data-act="hq:office:veto:${Number(p.id)}">Veto</button></li>`;
  }).join("");
  const done = recent.map((r) => `<li>${esc(r.status)}: ${(r.changes || []).map((c) => esc(c)).join("; ")}${r.outcome ? ` <small>(${esc(r.outcome)})</small>` : ""}</li>`).join("");
  const cents = o.downtime_paid_cents_per_day || 0;
  return `<section class="hq-card hq-office-card"><h2>Office <span class="hq-count">${open.length}</span></h2>
    <p class="hq-note">${(o.items || []).length} things on the floor. The team decides what to change together, in its downtime; you can veto, undo or reset.</p>
    ${proposals ? `<ul class="hq-team">${proposals}</ul>` : `<p class="hq-empty">No proposals open.</p>`}
    ${done ? `<details><summary>Lately</summary><ul class="hq-team">${done}</ul></details>` : ""}
    <div class="hq-row"><button class="hq-btn" data-act="hq:office:undo:go"${o.can_undo ? "" : " disabled"}>Undo last change</button>
      <button class="hq-btn no" data-act="hq:office:reset:go">Reset the office</button></div>
    <p class="hq-who">Paid members' downtime: ${hqDollars(cents * 10000)} a day${cents ? ` (${hqDollars((o.downtime_paid_cents_used || 0) * 10000)} used today)` : ""} — <button class="hq-btn" data-act="hq:office:downtime:set">change</button></p>
  </section>`;
}

// Whether a push on the desk can be approved: the node has a deploy key, so the card says
// "Push ..."; without one it says the branch is accepted but pushing is not set up.
function hqCanPush(d) { return String(d.text || "").startsWith("Push "); }

// The bundle a push card offers to download when the node cannot push: `hq/<slug>` from the
// path the card names, as the slug alone (letters, digits and dashes, which is all a slug is).
function hqBundleOf(d) {
  const m = /\/v1\/hq\/bundles\/\d+\/([a-z0-9-]+)/.exec(String(d.text || ""));
  return m ? m[1] : "";
}

// The owner hires (ADR 0115): a name, a role, a brain from the models the team knows, and whom
// the member reports to. HQ decides the rules (one CEO, a brain the role may use) and refuses
// the rest; this only asks.
function hqHireForm(s) {
  const roles = [["developer", "Developer"], ["lead", "Lead"], ["researcher", "Researcher"], ["reviewer", "Tester"], ["ceo", "CEO"]]
    .map(([v, l]) => `<option value="${v}">${l}</option>`).join("");
  const brains = [["local", "Local"], ["claude-haiku-5-5", "Claude Haiku"], ["claude-sonnet-5-5", "Claude Sonnet"], ["claude-opus-5-5", "Claude Opus (the CEO's alone)"]]
    .map(([v, l]) => `<option value="${v}">${l}</option>`).join("");
  const to = `<option value="">by role</option>` + (s.members || []).map((m) => `<option value="${m.id}">${esc(m.name)} (${esc(hqRole(m.role))})</option>`).join("");
  return `<details><summary>Hire</summary><div class="hq-row" style="margin-top:8px;">
      <input id="hq-hire-name" placeholder="Name" aria-label="The new member's name" />
      <select id="hq-hire-role" aria-label="Role">${roles}</select>
      <select id="hq-hire-brain" aria-label="Brain">${brains}</select>
      <label>reports to <select id="hq-hire-to" aria-label="Whom the member reports to">${to}</select></label>
      <button class="hq-btn ok" data-act="hq:hire:new:add">Hire</button></div></details>`;
}

// A task's documents and branches (ADR 0115): a submitted branch's diff and stat, a check's
// report, each a document the tester and the lead read; and each branch with where it is and
// how its check went. Every word is a diff's or a tool's, so all of it is escaped.
function hqDocuments(t) {
  const docs = t.documents || [], branches = t.branches || [];
  if (!docs.length && !branches.length) return "";
  const rows = branches.map((b) => `<li>${esc(b.name)} at ${esc(String(b.head || "").slice(0, 12))} · ${b.files} files, +${b.plus} −${b.minus} · ${esc(String(b.status || "").replace(/_/g, " "))}${b.check ? ` · check c${b.check.id}: ${esc(b.check.status === "done" ? (b.check.ok ? "passed" : "failed") + (b.check.summary ? ", " + b.check.summary : "") : b.check.status)}` : " · no check yet"}</li>`).join("");
  const shown = docs.map((d) => `<details class="hq-doc"><summary>d${d.id} ${esc(d.title)}</summary><pre>${esc(d.text || "")}</pre></details>`).join("");
  return `<div class="hq-evidence"><h3>Branches and documents</h3>${rows ? `<ul class="hq-thread">${rows}</ul>` : ""}${shown}</div>`;
}

// What a member thinks with (ADR 0109): the house's own model, or a Claude its role may use,
// as HQ lists them for it. The one it has is pressed; each other is a tap away.
function hqBrains(m) {
  const brains = m.brains || [];
  if (brains.length < 2) return "";
  const btn = (b) => `<button class="hq-btn${b.name === m.brain ? " ok" : ""}" aria-pressed="${b.name === m.brain ? "true" : "false"}"
    data-act="hq:brain:${m.id}:${esc(b.name)}">${esc(b.label)}</button>`;
  return `<div class="hq-row" role="group" aria-label="What ${esc(m.name)} thinks with">Brain: ${brains.map(btn).join("")}</div>`;
}

// The CEO's decision for the board (ADR 0111), in its three parts: every word is the CEO's,
// so all of it is escaped.
function hqDecision(c) {
  if (!c) return "";
  return `<dl class="hq-kv"><dt>Decision</dt><dd>${esc(c.decision)}</dd>
    <dt>Recommendation</dt><dd>${esc(c.recommendation)}</dd><dt>Why</dt><dd>${esc(c.why)}</dd></dl>`;
}

// A project's title, by its id.
function hqProjectTitle(id) {
  const p = ((HQ.snap && HQ.snap.projects) || []).find((x) => x.id === id);
  return p ? p.title : "#" + id;
}

// Whom a member reports to (ADR 0113): the CEO reports to the board, which is the owner; any
// other member to another, which the owner may change here. HQ refuses itself and circles.
function hqReports(m, members) {
  if (m.role === "ceo") return `<div class="hq-row"><span>Reports to the board: you.</span></div>`;
  const others = members.filter((o) => o.id !== m.id)
    .map((o) => `<option value="${o.id}"${o.id === m.reports_to ? " selected" : ""}>${esc(o.name)} (${esc(hqRole(o.role))})</option>`).join("");
  return `<div class="hq-row"><label>Reports to <select id="hq-reports-${m.id}" aria-label="Whom ${esc(m.name)} reports to">${others}</select></label>
    <button class="hq-btn" data-act="hq:reports:${m.id}:set">Set</button></div>`;
}

// Projects (ADR 0113), grouped by the goal each serves: its lead and status, and this month's
// spend against its cap, amber from 80% and red once spent. The owner, who is the board, makes
// them, caps them, hands them work, pauses, resumes, drops and reopens them. Every word a
// member wrote (a proposal's title, its reason) is escaped.
function hqProjects(s) {
  const projects = s.projects || [];
  const goals = (s.goals || []).filter((g) => g.status === "active" || projects.some((p) => p.goal === g.id));
  const status = (p) => p.pause === "budget" ? "paused: it spent its cap" : p.pause === "owner" ? "paused by you"
    : p.status === "proposed" ? `proposed by ${p.created_by}, waits for the CEO` : p.status;
  const money = (p) => {
    if (!p.cap) return `<span>${hqDollars(p.spent)} this month · no cap</span>`;
    const share = Math.min(100, Math.round((p.spent / p.cap) * 100));
    return `<div class="hq-gauge ${esc(p.level || "under")}" role="img" aria-label="${share}% of its cap spent"><i style="width:${share}%"></i></div>
      <span>${hqDollars(p.spent)} of ${hqDollars(p.cap)} this month</span>`;
  };
  const acts = (p) => {
    const btn = (act, label, cls) => `<button class="hq-btn ${cls}" data-act="hq:project:${p.id}:${act}">${label}</button>`;
    const open = p.status === "active" || p.status === "paused";
    return [
      p.status === "active" ? btn("work", "Hand it work", "ok") : "",
      p.status === "active" ? btn("pause", "Pause", "") : "",
      p.status === "paused" ? btn("resume", "Resume", "ok") : "",
      open || p.status === "proposed" ? btn("cap", p.cap ? "Change the cap" : "Set a cap", "") : "",
      ["proposed", "dropped", "done"].includes(p.status) ? btn("reopen", p.status === "proposed" ? "Start it" : "Reopen", "ok") : "",
      open || p.status === "proposed" ? btn("drop", "Drop", "no") : "",
    ].join("");
  };
  const row = (p) => `<li><b>${esc(p.title)}</b>
      <span>${esc(hqName(p.lead))} leads · ${esc(status(p))}${p.open_tasks ? ` · ${p.open_tasks} open` : ""}${p.asked_cap ? ` · asks ${hqDollars(p.asked_cap)} a month` : ""}</span>
      ${money(p)}${p.reason ? `<span>${esc(p.reason)}</span>` : ""}${(p.evidence || []).length ? `<span>Evidence: ${esc(p.evidence.join(", "))}</span>` : ""}
      <div class="hq-row">${acts(p)}</div></li>`;
  const groups = goals.map((g) => {
    const mine = projects.filter((p) => p.goal === g.id);
    return `<li><b>${esc(g.title)}</b><ul class="hq-team hq-projects">${mine.map(row).join("") || `<li class="hq-empty">No project yet.</li>`}</ul></li>`;
  }).join("");
  const goalOpts = (s.goals || []).filter((g) => g.status === "active").map((g) => `<option value="${g.id}">${esc(g.title)}</option>`).join("");
  const leadOpts = (s.members || []).filter((m) => m.role === "lead" || m.role === "ceo")
    .map((m) => `<option value="${m.id}"${m.role === "lead" ? " selected" : ""}>${esc(m.name)}</option>`).join("");
  return `<section class="hq-card"><h2>Projects</h2>
    <ul class="hq-team">${groups || `<li class="hq-empty">No goal yet: set the team a goal first.</li>`}</ul>
    <details><summary>New project</summary><div class="hq-row" style="margin-top:8px;">
      <select id="hq-project-goal" aria-label="The goal it serves">${goalOpts}</select>
      <input id="hq-project-title" placeholder="e.g. Media stack upkeep" aria-label="What it is" />
      <select id="hq-project-lead" aria-label="Who leads it">${leadOpts}</select>
      <label>cap $<input id="hq-project-cap" inputmode="decimal" placeholder="none" style="width:5em" aria-label="Monthly cap in dollars, or none" /> a month</label>
      <button class="hq-btn ok" data-act="hq:project:new:add">Make it</button></div></details></section>`;
}

// A span of time in the owner's words: 40 s, 14 min, 2.5 h, 3 d.
function hqSpan(ms) {
  if (ms == null) return "";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.round(s / 60)} min`;
  if (s < 86400) return `${(s / 3600).toFixed(1).replace(/\.0$/, "")} h`;
  return `${(s / 86400).toFixed(1).replace(/\.0$/, "")} d`;
}

// How the team is doing: the numbers HQ computed from its own record for today and the last
// seven days, by the owner's days (the engine's `scorecard`). This only renders them: every
// share, median and cost arrives computed; nothing a model wrote is in it, but every name goes
// through esc all the same. Light on purpose: a phone reads this first.
function hqScorecard(s) {
  const card = s.scorecard;
  if (!card || !card.today || !card.week) return "";
  const n = (v) => (v == null ? "—" : String(v));
  const pct = (v) => (v == null ? "" : ` (${v}%)`);
  const turns = (w) => `${n(w.turns.total)} · ${n(w.turns.nothing)} showed nothing${pct(w.turns.nothing_share)}${w.turns.unlabelled ? ` · ${w.turns.unlabelled} unlabelled` : ""}`;
  const review = (w) => `${n(w.review.sent_back)} of ${n(w.review.submissions)} submitted${pct(w.review.sent_back_share)}`;
  const done = (w) => (w.finished.per_done == null ? "none done" : `${hqDollars(w.finished.per_done)} each · ${w.finished.done} done`);
  const dropped = (w) => (w.finished.dropped ? `${hqDollars(w.finished.dropped_spent)} on ${w.finished.dropped} dropped` : "none dropped");
  const alarms = (w) => `${n(w.alarms.count)} · ${n(w.alarms.tasks)} tasks${w.alarms.closed_median_ms == null ? "" : ` · closed in ${hqSpan(w.alarms.closed_median_ms)}`}${w.alarms.open ? ` · ${w.alarms.open} open` : ""}`;
  const desk = (w) => {
    const d = w.desk, all = d.approvals + d.questions + d.stuck + d.notices + d.other;
    const parts = [[d.approvals, "approvals"], [d.questions, "questions"], [d.stuck, "stuck"], [d.notices, "notices"], [d.other, "other"]]
      .filter(([k]) => k).map(([k, name]) => `${k} ${name}`).join(", ");
    return `${all}${parts ? ` · ${parts}` : ""}${d.waiting ? ` · ${d.waiting} waiting` : ""}`;
  };
  // HQ keeps the tokens, not the brains' prices, so the share is shown and not the dollars.
  const cache = (w) => (w.cache.share == null ? "no paid input" : `${w.cache.share}% of paid input read from cache`);
  const rows = [["Turns", turns], ["Sent back", review], ["Per done task", done], ["Waste", dropped], ["Alarms", alarms], ["Desk", desk], ["Cache", cache]]
    .map(([k, f]) => `<tr><th>${k}</th><td>${esc(f(card.today))}</td><td>${esc(f(card.week))}</td></tr>`).join("");
  const members = (w) => (w.turns.members || []).map((m) => `<li>${esc(hqName(m.member))}: ${m.turns} turns · ${m.nothing} showed nothing${m.unlabelled ? ` · ${m.unlabelled} unlabelled` : ""} · ${hqDollars(m.spent)}</li>`).join("")
    || `<li class="hq-empty">No turns.</li>`;
  const when = (at) => new Date(at).toLocaleString([], { weekday: "short", hour: "2-digit", minute: "2-digit", hour12: false });
  const since = [["today", card.today.since], ["the 7 days", card.week.since]].filter(([, at]) => at != null)
    .map(([w, at]) => `the record for ${w} starts ${esc(when(at))}`).join("; ");
  // Each team's seats (ADR 0118): who is at work and who is on furlough. Never anyone gone.
  const teams = (card.teams || []).map((t) => `<li>${esc(t.name || "A lead")}'s team: ${Number(t.seats)} seats · ${Number(t.active)} at work · ${Number(t.furloughed)} on furlough</li>`).join("");
  return `<section class="hq-card"><h2>How the team is doing</h2>
    <table class="hq-score"><thead><tr><th></th><th>Today</th><th>7 days</th></tr></thead><tbody>${rows}</tbody></table>
    ${teams ? `<details><summary>Seats</summary><ul class="hq-team">${teams}</ul></details>` : ""}
    <details><summary>Per member</summary><div class="hq-score-members">
      <div><b>Today</b><ul class="hq-team">${members(card.today)}</ul></div>
      <div><b>7 days</b><ul class="hq-team">${members(card.week)}</ul></div></div></details>
    ${since ? `<p class="hq-note">${since}: older turns and events are no longer kept.</p>` : ""}</section>`;
}

// The house log (ADR 0111): the CEO's own document of the house, which it and the lead read
// every turn. The owner reads it here and may rewrite it.
function hqHouse(s) {
  const log = s.house_log || "";
  return `<section class="hq-card"><h2>House log</h2>
    <p class="hq-note">Kept by the CEO; the CEO and the lead read it every turn.</p>
    <textarea id="hq-house" rows="6" maxlength="6000" aria-label="The house log"
      placeholder="Nothing yet: the CEO adds what it learns about the house">${esc(log)}</textarea>
    <div class="hq-row"><button class="hq-btn ok" data-act="hq:house:save">Save the house log</button></div></section>`;
}

// What an approval will run, from what the node stored: the tool, its arguments one a line,
// and the member's reason. Every word is the member's or a tool's, so all of it is escaped.
function hqApproval(a) {
  if (!a) return "";
  const args = (a.args || []).map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("");
  return `<dl class="hq-kv"><dt>Runs</dt><dd>${esc(a.tool)}</dd>${args}</dl>
    <p class="hq-who">${esc(a.member || "A member")} says why: ${esc(a.why)}</p>`;
}

// What the team may do on the home lab: Endora's contracts (ADR 0106), and the owner's two
// switches. Each button offers the other way.
function hqContracts(c) {
  if (!c) return "";
  const btn = (key, on, label) => `<button class="hq-btn ${on ? "ok" : "no"}" aria-pressed="${on ? "true" : "false"}"
    data-act="hq:contracts:${key}:${on ? "off" : "on"}">${label}: ${on ? "on" : "off"}</button>`;
  return `<details class="hq-card"><summary>What the team may do on the home lab</summary>
    <p class="hq-note">Look at the home lab and its logs (secrets replaced), and restart a container: one an hour each, three in all.
      You are told of every restart. Endora's other skills are the team's too, as each skill's own setting says:
      on runs (three an hour, and you are told), ask puts it on your desk for approval, off is never offered.</p>
    <div class="hq-row">${btn("all", c.all, "Contracts")}${c.all ? btn("restart_container", c.restart_container, "Restarts") : ""}</div></details>`;
}

// Each active goal's notes from the owner: standing facts every member working towards it reads
// near the top of each briefing. Escaped like everything a person typed.
function hqNotes(s) {
  const goals = (s.goals || []).filter((g) => g.status === "active");
  if (!goals.length) return "";
  const rows = goals.map((g) => `<li><b>${esc(g.title)}</b>
      <textarea id="hq-note-${g.id}" rows="3" maxlength="2000" aria-label="Notes for the team on ${esc(g.title)}"
        placeholder="What every member working on this should know">${esc(g.note || "")}</textarea>
      <div class="hq-row"><button class="hq-btn ok" data-act="hq:note:${g.id}:save">Save notes</button></div></li>`).join("");
  return `<details class="hq-card"><summary>Notes for the team</summary><ul class="hq-team">${rows}</ul></details>`;
}

// Routines (ADR 0107): work that comes round again, each a task for one member. Each shows
// its rhythm, when it last ran, and whether it takes the home lab's alarms; each switches the
// other way and runs now. A small form adds one under the team's first active goal.
function hqRoutines(s) {
  const when = (at) => new Date(at).toLocaleString([], { weekday: "short", hour: "2-digit", minute: "2-digit", hour12: false });
  const rows = (s.routines || []).map((r) => `<li><b>${esc(r.title)}</b>
      <span>${esc(r.rhythm)} · ${esc(hqName(r.assignee))} · ${r.last_fired_at ? "last ran " + esc(when(r.last_fired_at)) : "not run yet"}${r.last_task_open ? ` · ${hqKey(r.last_task)} open` : ""}${r.on_lab_trouble ? " · takes home-lab alarms" : ""}${r.project ? ` · in ${esc(hqProjectTitle(r.project))}` : ""}</span>
      <div class="hq-row"><button class="hq-btn ${r.enabled ? "ok" : "no"}" aria-pressed="${r.enabled ? "true" : "false"}" data-act="hq:routine:${r.id}:${r.enabled ? "off" : "on"}">${r.enabled ? "On" : "Off"}</button>
        <button class="hq-btn" data-act="hq:routine:${r.id}:fire">Run now</button></div></li>`).join("");
  const who = (s.members || []).map((m) => `<option value="${m.id}"${m.role === "lead" ? " selected" : ""}>${esc(m.name)}</option>`).join("");
  return `<section class="hq-card"><h2>Routines</h2>
    <ul class="hq-team">${rows || `<li class="hq-empty">None yet: the team works only on what it is handed.</li>`}</ul>
    <details><summary>Add a routine</summary><div class="hq-row" style="margin-top:8px;">
      <input id="hq-routine-title" placeholder="e.g. Check the home lab" aria-label="What it does" />
      <label>every <input id="hq-routine-hours" type="number" min="1" max="720" value="6" style="width:4em" aria-label="Every how many hours" /> hours</label>
      <select id="hq-routine-who" aria-label="Who does it">${who}</select>
      <button class="hq-btn ok" data-act="hq:routine:new:add">Add</button></div></details></section>`;
}

// What was tapped, in detail: a member's turn and budget, or a task and its chain of why.
function hqSelected() {
  const sel = HQ.selected;
  if (!sel) return `<p class="hq-empty">Tap a worker or a card on the board.</p>`;
  const why = (t) => `<ul class="hq-why">${(t.why || []).map((w) => `<li>${esc(w)}</li>`).join("")}</ul>`;
  if (sel.type === "member") {
    const m = hqMember(sel.id); if (!m) return "";
    const run = (m.running || [])[0], t = run && run.task ? hqTask(run.task) : null;
    return `<div class="hq-sel">${esc(m.name)} · ${esc(hqRole(m.role))}</div><dl class="hq-kv">
      <dt>Doing</dt><dd>${m.paused ? "Paused" : t ? esc((HQ.people[m.id] || {}).doing || "starting") : "Waiting for a wake"}</dd>
      <dt>Run</dt><dd>${run ? "R" + run.run + (t ? " · " + hqKey(t.id) : "") : "none"}</dd>
      <dt>Brain</dt><dd>${esc(((m.brains || []).find((b) => b.name === m.brain) || { label: m.brain || "Local" }).label)}</dd>
      <dt>Budget</dt><dd>${hqMemberMoney(m)} · ${m.slots} at once</dd></dl>${t ? why(t) : ""}${hqTurns((r) => r.member === m.id, "member-" + m.id)}`;
  }
  if (sel.type === "item") {
    const it = hqItems().find((x) => x.id === sel.id); if (!it) return "";
    const by = it.placed_by === "endora" ? "Endora, when the office was set up" : it.placed_by === "owner" ? "you" : it.placed_by;
    return `<div class="hq-sel">${esc(String(it.kind).replace(/_/g, " "))}</div><dl class="hq-kv">
      <dt>Put there by</dt><dd>${esc(by)}</dd><dt>Where</dt><dd>${esc(hqAreaOf(it))}</dd></dl>
      <p class="hq-note">The team moves what stands here by proposing it together; the Office card below lets you undo, reset or veto.</p>`;
  }
  const t = hqTask(sel.id); if (!t) return "";
  const runner =((HQ.snap && HQ.snap.runs) || []).find((r) => r.task === t.id && r.status === "running");
  const thread = (t.thread || []).slice(-3).map((p) => `<li>${esc(p.author)}${p.unsupported ? ` <span class="hq-unsupported">unsupported</span>` : ""}: ${esc(p.text)}</li>`).join("");
  return `<div class="hq-sel">${hqKey(t.id)} · ${esc(t.title)}</div><dl class="hq-kv">
    <dt>Status</dt><dd>${esc(String(t.status).replace("_", " "))}${t.gate === true ? " · gate passed" : t.gate === false ? " · gate failed" : ""}</dd>
    <dt>Owner</dt><dd>${t.owner ? esc(hqName(t.owner)) : "nobody"}</dd>
    <dt>Lock</dt><dd>${runner ? "held by R" + runner.id : "free"}</dd>
    <dt>Blocked by</dt><dd>${(t.waits_on || []).length ? t.waits_on.map(hqKey).join(", ") : "nothing"}</dd>
    <dt>Depth</dt><dd>${t.depth || 0}</dd></dl>${why(t)}${hqEvidence(t)}${hqDocuments(t)}${hqTurns((r) => r.task === t.id, "task-" + t.id)}${thread ? `<ul class="hq-thread">${thread}</ul>` : ""}
    ${replayTaskLink(t.id)}
    <textarea id="hq-reply-${t.id}" rows="2" placeholder="Reply in the thread: whoever has it reads it now" aria-label="Your reply"></textarea>
    <div class="hq-row"><button class="hq-btn ok" data-act="hq:reply:${t.id}:send">Send</button></div>`;
}

// The verbs this screen uses. Returns true when it handled one.
async function hqDispatch(verb, noun, id, arg) {
  if (!(verb === "hq")) return false;
  if (noun === "pick") { HQ.selected = { type: id, id: Number(arg) }; render(); return true; }
  if (noun === "roster") {
    HQ.roster[id] = !HQ.roster[id]; render();
    if (HQ.roster[id]) hqLoadNotebook(Number(id));
    return true;
  }
  // The owner writes in a member's notebook (ADR 0118): add a note, or edit or delete one.
  if (noun === "notebook") {
    const member = Number(id), what = String(arg || "").split("-"), note = Number(what[1]);
    try {
      if (what[0] === "add") {
        const text = val(`hq-note-new-${member}`); if (!text) return true;
        await api("POST", `/v1/hq/members/${member}/notebook`, { text }); flash("Note added.", "ok");
      } else if (what[0] === "edit") {
        const old = ((HQ.notebooks[member] || []).find((n) => n.id === note) || {}).text || "";
        const text = (prompt("Edit the note:", old) || "").trim(); if (!text || text === old) return true;
        await api("POST", `/v1/hq/members/${member}/notebook/${note}`, { text }); flash("Note saved.", "ok");
      } else if (what[0] === "del") {
        if (!confirm("Delete this note?")) return true;
        await api("DELETE", `/v1/hq/members/${member}/notebook/${note}`); flash("Note deleted.", "ok");
      } else return true;
    } catch (e) { flash("Couldn't change the notebook: " + e.message, "err"); }
    await hqLoadNotebook(member); return true;
  }
  if (noun === "turns") { HQ.allTurns[id] = !HQ.allTurns[id]; render(); return true; }
  if (noun === "decide") {
    // A question's answer, if the owner wrote one, goes in the task's thread before the member
    // wakes; so do the board's words on the CEO's decision.
    const box = document.getElementById(`hq-answer-${id}`);
    const item = ((HQ.snap && HQ.snap.desk) || []).find((d) => d.id === Number(id));
    const amount = item && (item.kind === "project_cap" || item.kind === "project_budget" || item.kind === "out_for_day");
    const takesWords = arg === "seen" || amount || (item && item.kind === "decision");
    const answer = takesWords && box ? box.value.trim() : "";
    if (arg === "answer" && !answer) { flash("Write your answer first.", "warn"); return true; }
    if (arg === "raise" && !answer) { flash("Write the new cap first, in dollars a month.", "warn"); return true; }
    try { await api("POST", `/v1/hq/desk/${id}`, { decision: arg, answer }); flash(answer ? "Answered." : "Decided.", "ok"); }
    catch (e) { flash("Couldn't decide: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // The owner's reply on a task: whoever has it is woken to read it.
  if (noun === "reply") {
    const text = val(`hq-reply-${id}`); if (!text) return true;
    try { await api("POST", `/v1/hq/tasks/${id}/post`, { text }); flash("Sent.", "ok"); }
    catch (e) { flash("Couldn't send: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // What a member thinks with: a Claude is paid from the team's pool, and what the member reads
  // goes to Anthropic, so the owner says yes to that first.
  if (noun === "brain") {
    const m = hqMember(Number(id)), b = ((m && m.brains) || []).find((x) => x.name === arg);
    if (!m || !b || m.brain === arg) return true;
    if (arg !== "local" && !confirm(`${m.name} will think with ${b.label}: each turn is paid from the team's pool, and what ${m.name} reads goes to Anthropic (Confidential results stay home, but for what you allowed the lead). Go ahead?`)) return true;
    try { await api("POST", `/v1/hq/members/${id}/brain`, { brain: arg }); flash(`${m.name} thinks with ${b.label}.`, "ok"); }
    catch (e) { flash("Couldn't change it: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // The owner renames a member: HQ keeps names apart, as members are named in threads.
  if (noun === "rename") {
    const m = hqMember(Number(id)); if (!m) return true;
    const name = (prompt(`A new name for ${m.name}:`, m.name) || "").trim();
    if (!name || name === m.name) return true;
    try { await api("POST", `/v1/hq/members/${id}/name`, { name }); flash(`${m.name} is now ${name}.`, "ok"); }
    catch (e) { flash("Couldn't rename: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // The owner hires (ADR 0115): HQ refuses a second CEO or a brain the role may not use.
  if (noun === "hire") {
    const name = val("hq-hire-name");
    const role = (document.getElementById("hq-hire-role") || {}).value || "developer";
    const brain = (document.getElementById("hq-hire-brain") || {}).value || "local";
    const to = Number((document.getElementById("hq-hire-to") || {}).value || 0) || null;
    if (!name) { flash("Give the new member a name.", "warn"); return true; }
    try { await api("POST", "/v1/hq/members", { name, role, brain, reports_to: to }); flash(`${name} joined the team.`, "ok"); }
    catch (e) { flash("Couldn't hire: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // A branch's bundle, when the node cannot push it: fetched with this device's token and
  // saved, for the owner to `git fetch` into their own clone.
  if (noun === "bundle") {
    const slug = String(arg || "").replace(/[^a-z0-9-]/g, "");
    if (!slug) return true;
    try {
      const res = await fetch(`/v1/hq/bundles/${Number(id)}/${slug}`, { headers: signed({}) });
      if (!res.ok) throw new Error("HTTP " + res.status);
      const url = URL.createObjectURL(await res.blob());
      const a = document.createElement("a"); a.href = url; a.download = `${slug}.bundle`; a.click();
      setTimeout(() => URL.revokeObjectURL(url), 10000);
      flash("Bundle saved. In your clone: git fetch <the file> refs/submissions/hq/" + slug + ":hq/" + slug, "ok");
    } catch (e) { flash("Couldn't fetch the bundle: " + e.message, "err"); }
    return true;
  }
  // Furlough or recall a member (ADR 0118): HQ refuses the CEO, and a member with open work.
  if (noun === "furlough" || noun === "recall") {
    const m = hqMember(Number(id)); if (!m) return true;
    if (noun === "furlough" && !confirm(`Put ${m.name} on furlough? Nobody is let go: ${m.name} is not woken, keeps the record and notebook, and you can recall them.`)) return true;
    try {
      if (noun === "furlough") await api("POST", `/v1/hq/members/${id}/furlough`, {});
      else await api("POST", `/v1/hq/members/${id}/recall`, {});
      flash(noun === "furlough" ? `${m.name} is on furlough.` : `${m.name} is back at work.`, "ok");
    }
    catch (e) { flash("Couldn't do it: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // The owner as the office's landlord (ADR 0119): undo the last change, reset the office to how
  // it started, veto an open proposal, and say what paid members' downtime may spend a day.
  if (noun === "office") {
    try {
      if (id === "undo") {
        const done = await api("POST", "/v1/hq/office/undo", {});
        flash(done.undone ? `Undone: ${done.undone}` : "Nothing to undo.", "ok");
      } else if (id === "reset") {
        if (!confirm("Put the office back as it started? The team's changes go; you can undo the reset.")) return true;
        await api("POST", "/v1/hq/office/reset", {}); flash("The office is back as it started.", "ok");
      } else if (id === "veto") {
        await api("POST", `/v1/hq/office/proposals/${Number(arg)}/veto`, {}); flash("Vetoed.", "ok");
      } else if (id === "downtime") {
        const now = (((HQ.snap || {}).office || {}).downtime_paid_cents_per_day || 0) / 100;
        const said = prompt("Dollars a day that paid members' downtime may spend, together (0 for none, the default):", now.toFixed(2));
        if (said === null) return true;
        const cents = Math.round(parseFloat(said) * 100);
        if (!Number.isFinite(cents) || cents < 0) { flash("That is not an amount in dollars.", "err"); return true; }
        await api("POST", "/v1/hq/office/downtime", { cents }); flash(`Paid members' downtime: ${hqDollars(cents * 10000)} a day.`, "ok");
      }
    } catch (e) { flash("Couldn't change the office: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // Whom a member reports to.
  if (noun === "reports") {
    const box = document.getElementById(`hq-reports-${id}`), to = box ? Number(box.value) : 0;
    if (!to) return true;
    try { await api("POST", `/v1/hq/members/${id}/reports_to`, { to }); flash(`${hqName(Number(id))} reports to ${hqName(to)}.`, "ok"); }
    catch (e) { flash("Couldn't change it: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // Projects: make one, cap it, hand it work, or change where it is.
  if (noun === "project") {
    const s = HQ.snap || {}, p = (s.projects || []).find((x) => x.id === Number(id));
    try {
      if (arg === "add") {
        const title = val("hq-project-title"), cap = val("hq-project-cap");
        const goal = Number((document.getElementById("hq-project-goal") || {}).value || 0);
        const lead = Number((document.getElementById("hq-project-lead") || {}).value || 0);
        if (!title || !goal || !lead) { flash("Say what it is, its goal and who leads it.", "warn"); return true; }
        await api("POST", "/v1/hq/projects", { goal, title, lead, cap: cap || null }); flash("Project made.", "ok");
      } else if (!p) {
        return true;
      } else if (arg === "cap") {
        const said = prompt(`A monthly cap for ${p.title}, in dollars (empty: no cap):`, p.cap ? (p.cap / 1e6).toFixed(2) : "");
        if (said === null) return true;
        await api("POST", `/v1/hq/projects/${id}/cap`, { cap: said.trim() || null }); flash("Cap set.", "ok");
      } else if (arg === "work") {
        const title = (prompt(`What should ${hqName(p.lead)} do in ${p.title}?`) || "").trim(); if (!title) return true;
        await api("POST", "/v1/hq/tasks", { goal: p.goal, project: p.id, title, assignee: p.lead }); flash("Handed to the team.", "ok");
      } else {
        if (arg === "drop" && !confirm(`Drop ${p.title}? Its open tasks are dropped with it.`)) return true;
        const done = await api("POST", `/v1/hq/projects/${id}`, { action: arg }); flash(`${p.title} is ${done.status}.`, "ok");
      }
    } catch (e) { flash("Couldn't do it: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // The owner rewrites the CEO's house log; empty clears it.
  if (noun === "house") {
    const box = document.getElementById("hq-house"), log = box ? box.value.trim() : "";
    try { await api("POST", "/v1/hq/house", { log }); flash("House log saved.", "ok"); }
    catch (e) { flash("Couldn't save it: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // A goal's notes for the team; empty clears them.
  if (noun === "note") {
    const box = document.getElementById(`hq-note-${id}`), note = box ? box.value.trim() : "";
    try { await api("POST", `/v1/hq/goals/${id}/note`, { note }); flash("Notes saved.", "ok"); }
    catch (e) { flash("Couldn't save them: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  if (noun === "stop") {
    if (!confirm("Pull the kill switch? Every turn stops now, and nothing starts until you resume.")) return true;
    try { await api("POST", "/v1/hq/stop", { why: "the owner pulled it" }); flash("Stopped.", "ok"); }
    catch (e) { flash("Couldn't stop: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  if (noun === "resume") {
    try { await api("POST", "/v1/hq/resume", {}); flash("The team works again.", "ok"); }
    catch (e) { flash("Couldn't resume: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  if (noun === "goal") {
    const title = val("hq-goal"); if (!title) return true;
    try { await api("POST", "/v1/hq/goals", { title, reason: "" }); flash("Goal set.", "ok"); }
    catch (e) { flash("Couldn't set it: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // The owner's switches for the team's contracts.
  if (noun === "contracts") {
    try { await api("POST", "/v1/hq/contracts", { [id]: arg === "on" }); flash(arg === "on" ? "Switched on." : "Switched off.", "ok"); }
    catch (e) { flash("Couldn't switch it: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // Routines: add one, switch one, or run one now.
  if (noun === "routine") {
    const s = HQ.snap || {};
    if (arg === "add") {
      const title = val("hq-routine-title"), hours = Number(val("hq-routine-hours")) || 0;
      const box = document.getElementById("hq-routine-who"), assignee = box ? Number(box.value) : 0;
      const goal = (s.goals || []).find((g) => g.status === "active");
      if (!title || hours < 1) { flash("Say what it does, and every how many hours.", "warn"); return true; }
      if (!goal) { flash("Set the team a goal first.", "warn"); return true; }
      try { await api("POST", "/v1/hq/routines", { title, goal: goal.id, assignee, every_ms: Math.round(hours * 3600000) }); flash("Routine added.", "ok"); }
      catch (e) { flash("Couldn't add it: " + e.message, "err"); }
    } else if (arg === "fire") {
      try {
        const done = await api("POST", `/v1/hq/routines/${id}/fire`, {});
        flash(done.fired === "skipped" ? "Not run: " + done.why : "Handed to the team.", done.fired === "skipped" ? "warn" : "ok");
      } catch (e) { flash("Couldn't run it: " + e.message, "err"); }
    } else {
      try { await api("POST", `/v1/hq/routines/${id}`, { enabled: arg === "on" }); flash(arg === "on" ? "Switched on." : "Switched off.", "ok"); }
      catch (e) { flash("Couldn't switch it: " + e.message, "err"); }
    }
    await loadHQ(); render(); return true;
  }
  // Hand the team a piece of work, under the first goal it is working towards, to its lead.
  if (noun === "work") {
    const title = (prompt("What should the team do?") || "").trim(); if (!title) return true;
    const s = HQ.snap || {}, goal = (s.goals || []).find((g) => g.status === "active");
    const lead = (s.members || []).find((m) => m.role === "lead");
    if (!goal) { flash("Set the team a goal first.", "warn"); return true; }
    // What done means, if the owner says (ADR 0114): separated by semicolons, "live:" before one
    // about the lab as it is now.
    const said = prompt("What does done mean? (optional; separate with ;, and put live: before one about the lab now)") || "";
    const criteria = said.split(";").map((c) => c.trim()).filter(Boolean);
    try { await api("POST", "/v1/hq/tasks", { goal: goal.id, title, assignee: lead ? lead.id : null, criteria }); flash("Handed to the team.", "ok"); }
    catch (e) { flash("Couldn't hand it over: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  return false;
}
