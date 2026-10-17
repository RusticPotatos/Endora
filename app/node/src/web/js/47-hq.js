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
  t: 0,                // seconds of animation
  polling: null,
  looping: false,
};

const HQ_T = 16, HQ_COLS = 28, HQ_ROWS = 16, HQ_W = HQ_COLS * HQ_T, HQ_H = HQ_ROWS * HQ_T;
const HQ_C = {
  wall: "#efece6", wallTop: "#e2ddd3", trim: "#c9c1b3", glass: "rgba(150,205,235,.22)", glassEdge: "#9cc7de",
  board: "#fbfbf8", boardEdge: "#b9c2c9", rack: "#232833", neon: "#ff4fa3", lamp: "#ffb347", red: "#ef5a5f",
  green: "#45c486", blue: "#7fb2ff", violet: "#b98cf0", sticky: "#fff2a8", text: "#1f1d29", shadow: "rgba(60,40,20,.18)",
};
const HQ_STATUS_COL = { todo: HQ_C.sticky, doing: HQ_C.lamp, in_review: HQ_C.blue, done: HQ_C.green, blocked: HQ_C.red, dropped: "#cfcac0" };
const HQ_P = {
  board: { x: 8, y: 0, w: 12, h: 2 }, owner: { x: 2, y: 3 }, lever: { x: 6.1, y: 2.3 },
  engine: { x: 22, y: 2, w: 6, h: 3.6 }, rack: { x: 25, y: 2.2 }, jar: { x: 22.7, y: 2.6 },
  door: { x: 25, y: 13 }, pong: { x: 2.5, y: 13.2 }, tap: { x: 13.5, y: 12.6 },
  upperLane: 6, lowerLane: 11, aisles: [7.5, 13.5, 19.5],
};
// Four desks and four places to rest; members take them in the order they joined.
const HQ_DESKS = [{ x: 3, y: 8 }, { x: 9, y: 8 }, { x: 15, y: 8 }, { x: 21, y: 8 }];
const HQ_RESTS = [{ x: 1.2, y: 13 }, { x: 7, y: 13 }, { x: 9.2, y: 14 }, { x: 11.2, y: 14 }];
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
      `R${d.run} ${name} ${d.result === "succeeded" ? "finished" : result}${d.spent ? " · " + hqDollars(d.spent) + " spent" : ""}`],
    "gate.result": [d.passed ? "done" : "bad", `Gate on ${key}: ${d.passed ? "passed" : "failed"}`],
    "desk.added": ["you", `On your desk: ${String(d.kind || "").replace(/_/g, " ")}${on}`],
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
    "checkback.fired": ["wake", `${name} checks back on ${key}`],
    "checkback.dropped": ["run", `Check-back on ${key} dropped: it is over`],
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
  }
  if (e.kind === "task.claimed" && p) hqSay(p, "checkout " + key);
  if (e.kind === "run.ended" && p) hqSay(p, d.result === "succeeded" ? "done" : result);
  if (e.kind === "stopped" || e.kind === "resumed") HQ.doorFlash = 1.5;
}

// A bubble holds a few words: a status change keeps only what was said about it.
function hqShort(text) { return String(text || "").replace(/^[a-z_]+ → [a-z_]+: /, "").toLowerCase().slice(0, 26); }
function hqSay(p, text) { p.bubble = hqShort(text); p.bubbleT = 3.2; }

// Where each member is, kept across redraws: at their desk while a turn runs, resting otherwise.
function hqPlacePeople() {
  const members = (HQ.snap && HQ.snap.members) || [];
  members.forEach((m, i) => {
    const desk = HQ_DESKS[i % HQ_DESKS.length], rest = HQ_RESTS[i % HQ_RESTS.length];
    let p = HQ.people[m.id];
    if (!p) {
      p = HQ.people[m.id] = { id: m.id, x: rest.x * HQ_T, y: rest.y * HQ_T, path: [], frame: 0, face: 1, bubble: "", bubbleT: 0, working: false };
    }
    p.look = m.look || 0; p.name = m.name; p.role = m.role; p.paused = m.paused; p.desk = desk; p.slot = i;
    const run = (m.running || [])[0];
    const task = run && run.task ? hqTask(run.task) : null;
    // What a working member is doing, said above their head: their latest word on the task.
    const last = task && (task.thread || []).filter((t) => t.author === m.name).pop();
    p.doing = task ? (last ? last.text : "on " + hqKey(task.id)) : "";
    const working = !!run;
    if (working !== p.working) {
      p.working = working;
      hqWalk(p, working ? { x: desk.x + 1.6, y: desk.y - 0.5 } : rest);
    }
  });
}

function hqWalk(p, tile) {
  const T = HQ_T, tx = tile.x * T, ty = tile.y * T, upper = (y) => y <= 8 * T;
  const lane = (u) => (u ? HQ_P.upperLane : HQ_P.lowerLane) * T, su = upper(p.y), tu = upper(ty);
  if (su === tu) { p.path = [{ x: p.x, y: lane(su) }, { x: tx, y: lane(su) }, { x: tx, y: ty }]; return; }
  const ax = HQ_P.aisles.map((a) => a * T).sort((a, b) => Math.abs(a - p.x) - Math.abs(b - p.x))[0];
  p.path = [{ x: p.x, y: lane(su) }, { x: ax, y: lane(su) }, { x: ax, y: lane(tu) }, { x: tx, y: lane(tu) }, { x: tx, y: ty }];
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
  floor("grid", "#d9e1ea", 0, 2, 7, 6);
  floor("plain", "#c3c8d0", 22, 2, HQ_COLS, 6);
  floor("grid", "#8fa6c7", 0, 12, 13, HQ_ROWS);
  floor("check", "#efe9de", 20, 12, HQ_COLS, HQ_ROWS);
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
  if (HQ_IMG.LARGE_PAINTING) g.drawImage(HQ_IMG.LARGE_PAINTING, 20.4 * T, 0);
  // Glass rooms: the board room and the server closet.
  g.fillStyle = C.glass; g.fillRect(0, 2 * T, 7 * T, 3.6 * T); g.fillStyle = C.glassEdge; g.fillRect(7 * T - 2, 2 * T, 2, 3.6 * T); g.fillRect(0, 5.6 * T - 2, 7 * T - 22, 2);
  g.fillStyle = "rgba(60,70,90,.08)"; g.fillRect(P.engine.x * T, P.engine.y * T, P.engine.w * T, P.engine.h * T);
  g.fillStyle = C.glassEdge; g.fillRect(P.engine.x * T, P.engine.y * T, 2, P.engine.h * T); g.fillRect(P.engine.x * T + 22, (P.engine.y + P.engine.h) * T - 2, P.engine.w * T - 22, 2);
  // The lounge: ping-pong, cold brew, the snack wall.
  const px = P.pong.x * T, py = P.pong.y * T;
  g.fillStyle = C.shadow; g.fillRect(px + 2, py + 13, 66, 4); g.fillStyle = "#1f7a4f"; g.fillRect(px, py, 66, 13); g.fillStyle = "#ffffff"; g.fillRect(px, py + 6, 66, 1); g.fillRect(px + 32, py - 3, 2, 16);
  g.fillStyle = "#3b4048"; g.fillRect(px + 4, py + 13, 2, 4); g.fillRect(px + 60, py + 13, 2, 4);
  g.fillStyle = "#cfd6dd"; g.fillRect(P.tap.x * T, P.tap.y * T - 6, 14, 20); g.fillStyle = "#3a2a1e"; g.fillRect(P.tap.x * T + 3, P.tap.y * T - 3, 8, 7); g.fillStyle = "#222"; g.fillRect(P.tap.x * T + 6, P.tap.y * T + 5, 2, 3);
  g.fillStyle = "#b8a68a"; g.fillRect(15 * T, 12 * T, 4 * T, 3); g.fillRect(15 * T, 12.8 * T, 4 * T, 3);
  const snacks = ["#ff4fa3", "#ffb347", "#45c486", "#7fb2ff", "#ef5a5f", "#fff2a8"];
  for (let i = 0; i < 14; i++) { g.fillStyle = snacks[i % snacks.length]; g.fillRect(15 * T + 2 + i * 4.4, 12 * T - 5 + (i % 2) * 13, 3, 5); }
  // The lobby: reception and the door.
  g.fillStyle = "#ffffff"; g.fillRect(21 * T, 13 * T, 2.6 * T, 11); g.fillStyle = C.trim; g.fillRect(21 * T, 13 * T + 10, 2.6 * T, 2);
  g.fillStyle = "#3d3a44"; g.fillRect(P.door.x * T, P.door.y * T, 2 * T, 3 * T); g.fillStyle = "#9fd3f0"; g.fillRect(P.door.x * T + 4, P.door.y * T + 4, 2 * T - 8, 3 * T - 4);
  // Labels.
  g.font = HQ_PIXEL; g.fillStyle = "rgba(31,29,41,.6)";
  g.fillText("BOARD ROOM", 4, 2 * T + 9); g.fillText("SERVERS", (P.engine.x + 3.4) * T, (P.engine.y + P.engine.h) * T - 4);
  g.fillStyle = "rgba(255,255,255,.8)"; g.fillText("PING-PONG", 4, 12 * T + 8); g.fillStyle = "rgba(31,29,41,.6)"; g.fillText("COLD BREW", 12.4 * T, 12 * T - 2);
  g.fillText("LOBBY", 20.4 * T, 12 * T + 9); g.fillText("ENDORA", 21.1 * T, 13 * T + 8);
  return bg;
}

function hqFurniture() {
  const T = HQ_T, out = [];
  const f = (key, x, y, extra = {}) => { const i = HQ_IMG[key]; if (i) out.push({ key, x, y, base: y + i.height + (extra.lift || 0), ...extra }); };
  HQ_DESKS.forEach((d, i) => {
    f("DESK_FRONT", d.x * T, d.y * T);
    f("PC_FRONT_OFF", d.x * T + 4, d.y * T - 6, { lift: 0.5, pc: i });
    f("BIN", (d.x + 3) * T, (d.y + 1) * T);
  });
  f("SMALL_TABLE_FRONT", HQ_P.owner.x * T, HQ_P.owner.y * T);
  f("COFFEE", (HQ_P.owner.x + 1.3) * T, HQ_P.owner.y * T + 4, { lift: 40 });
  f("CUSHIONED_CHAIR_BACK", (HQ_P.owner.x + 0.1) * T, (HQ_P.owner.y + 1.9) * T);
  f("CUSHIONED_CHAIR_BACK", (HQ_P.owner.x + 1.1) * T, (HQ_P.owner.y + 1.9) * T);
  f("SOFA_FRONT", 8.6 * T, 13.2 * T); f("SOFA_FRONT", 10.6 * T, 13.2 * T);
  f("PLANT", 0.1 * T, 9.6 * T); f("PLANT_2", 26.7 * T, 6.6 * T); f("CACTUS", 21.1 * T, 6.4 * T);
  f("LARGE_PLANT", 19.1 * T, 12.4 * T); f("BOOKSHELF", 22.2 * T, 15 * T);
  return out;
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

// The first two members (the lead and the developer) play ping-pong when both are idle.
function hqPong() {
  const two = Object.values(HQ.people).filter((p) => p.slot === 0 || p.slot === 1);
  return two.length === 2 && two.every((p) => !p.working && !p.path.length && !p.paused) ? two : null;
}

function hqFrame(p) {
  if (p.path.length) {
    const n = p.path[0], dx = n.x - p.x, dy = n.y - p.y;
    const dir = Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? "right" : "left") : dy > 0 ? "down" : "up";
    return { dir, frame: [0, 1, 2, 1][Math.floor(p.frame) % 4] };
  }
  const reads = p.role === "researcher" || p.role === "reviewer";
  if (p.working) return { dir: "down", frame: reads ? 5 + (Math.floor(HQ.t * 1.5) % 2) : 3 + (Math.floor(HQ.t * 6) % 2) };
  const pong = hqPong();
  if (pong && pong.includes(p)) return { dir: p.slot === 0 ? "right" : "left", frame: 3 + (Math.floor(HQ.t * 4) % 2) };
  return { dir: "down", frame: p.paused ? 1 : 0 };
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
  // The wall clock: the arc to the next tick of the real engine.
  const cx = 23 * T + 8, cy = 14, every = (snap.tick_seconds || 30) * 1000;
  const since = snap.last_tick_at ? Math.max(0, Date.now() - snap.last_tick_at) % every : 0;
  ctx.fillStyle = "#ffffff"; ctx.beginPath(); ctx.arc(cx, cy, 8, 0, Math.PI * 2); ctx.fill(); ctx.strokeStyle = C.trim; ctx.stroke();
  ctx.fillStyle = snap.stopped ? C.red : C.neon; ctx.beginPath(); ctx.moveTo(cx, cy);
  ctx.arc(cx, cy, 7, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * (since / every)); ctx.fill();
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
  // A ping-pong ball between the two idle founders.
  if (hqPong()) { const k = (Math.sin(HQ.t * 4) + 1) / 2, bx = P.pong.x * T + 6 + k * 56, by = P.pong.y * T + 2 - Math.abs(Math.sin(HQ.t * 8)) * 9; ctx.fillStyle = "#ffffff"; ctx.fillRect(Math.round(bx), Math.round(by), 2, 2); }
  // People and furniture, in depth order.
  const people = Object.values(HQ.people);
  const items = HQ_FURN.map((f) => ({ base: f.base, f }));
  for (const p of people) items.push({ base: p.y + 16, p });
  items.sort((a, b) => a.base - b.base);
  for (const it of items) {
    if (it.p) { hqDrawPerson(ctx, it.p); continue; }
    let key = it.f.key;
    if (it.f.pc !== undefined) {
      const at = people.find((p) => p.working && p.slot % HQ_DESKS.length === it.f.pc && !p.path.length);
      key = at ? "PC_FRONT_ON_" + (1 + (Math.floor(HQ.t * 4) % 3)) : "PC_FRONT_OFF";
    }
    if (HQ_IMG[key]) ctx.drawImage(HQ_IMG[key], Math.round(it.f.x), Math.round(it.f.y));
  }
  ctx.font = HQ_PIXEL;
  for (const p of people) { ctx.fillStyle = "rgba(31,29,41,.85)"; ctx.fillText(String(p.name || "").toUpperCase(), Math.round(p.x) + 1, Math.round(p.y) + 24); }
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
  const idle = !p.working && !playing && !p.path.length ? (p.paused ? "over budget" : HQ_IDLE[p.role] || "") : "";
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
      if (!p.path.length) continue;
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
      approval: [["approve", "Approve", "ok"], ["decline", "Decline", "no"]] }[d.kind] || [];
    // In the demo nothing behind a card is real: say so on the card, not only in the header.
    const demoNote = s.demo ? (d.kind === "ready_to_merge" ? " · demo: there is no pull request behind this"
      : d.kind === "ready_to_read" ? " · demo: there is no write-up behind this" : " · demo") : "";
    return `<div class="hq-need ${esc(d.kind)}"><p>${s.demo ? "Demo: " : ""}${esc(d.text)}</p>
      <p class="hq-who">${esc(String(d.kind).replace(/_/g, " "))}${d.task ? ` · ${hqKey(d.task)}` : ""}${demoNote}</p>
      ${d.kind === "approval" ? hqApproval(d.approval) : ""}
      ${d.kind === "question" ? `<textarea id="hq-answer-${d.id}" rows="2" placeholder="Your answer, for the task's thread (optional)" aria-label="Your answer"></textarea>` : ""}
      <div class="hq-row">${choices.map(([v, l, cls]) => `<button class="hq-btn ${cls}" data-act="hq:decide:${d.id}:${v}">${l}</button>`).join("")}</div></div>`;
  }).join("");
  // The latest twenty, newest first: enough to follow the team, short enough to read.
  const log = HQ.log.slice(0, 20).map((l) => `<li><time>${esc(l.at)}</time><span class="k-${l.kind}">${esc(l.text)}</span></li>`).join("");
  const team = (s.members || []).map((m) => {
    const on = (m.running || []).map((r) => r.task ? hqKey(r.task) : "a look round").join(", ");
    return `<li><button class="hq-link" data-act="hq:pick:member:${m.id}">${esc(m.name)}</button>
      <span>${esc(m.role)}${m.paused ? " · paused" : ""} · ${on ? "on " + esc(on) : "between turns"} · ${hqMoney(m, `of ${hqDollars(m.per_day)} a day`)}</span>
      ${hqBrains(m)}</li>`;
  }).join("");
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
          aria-label="The team's office: members at their desks, the board of tasks, the server closet, the board room"></canvas>
        <div class="hq-stats">${stats}</div>
        <div class="hq-legend"><span style="--c:#fff2a8">To do</span><span style="--c:#ffb347">Doing</span><span style="--c:#7fb2ff">In review</span><span style="--c:#45c486">Done</span><span style="--c:#ef5a5f">Blocked</span><span style="--c:#b98cf0">Needs you</span></div>
        ${s.demo ? `<p class="hq-note">Demo: nothing is spent; turns reserve as real ones will.</p>` : ""}
      </div>
      <aside class="hq-side">
        <section class="hq-card" aria-live="polite"><h2>Looking at</h2>${hqSelected()}</section>
        <section class="hq-card"><h2>Your desk <span class="hq-count">${(s.desk || []).length}</span></h2>
          <div class="hq-needs">${desk || `<p class="hq-empty">Nothing waiting.</p>`}</div></section>
        <section class="hq-card"><h2>Activity</h2><ul class="hq-log" data-keep-scroll="hq-log">${log || `<li class="hq-empty">Nothing yet.</li>`}</ul></section>
        <section class="hq-card"><h2>Team</h2><ul class="hq-team">${team}</ul></section>
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

// What a member thinks with (ADR 0109): the house's own model, or a Claude its role may use,
// as HQ lists them for it. The one it has is pressed; each other is a tap away.
function hqBrains(m) {
  const brains = m.brains || [];
  if (brains.length < 2) return "";
  const btn = (b) => `<button class="hq-btn${b.name === m.brain ? " ok" : ""}" aria-pressed="${b.name === m.brain ? "true" : "false"}"
    data-act="hq:brain:${m.id}:${esc(b.name)}">${esc(b.label)}</button>`;
  return `<div class="hq-row" role="group" aria-label="What ${esc(m.name)} thinks with">Brain: ${brains.map(btn).join("")}</div>`;
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
      <span>${esc(r.rhythm)} · ${esc(hqName(r.assignee))} · ${r.last_fired_at ? "last ran " + esc(when(r.last_fired_at)) : "not run yet"}${r.last_task_open ? ` · ${hqKey(r.last_task)} open` : ""}${r.on_lab_trouble ? " · takes home-lab alarms" : ""}</span>
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
    return `<div class="hq-sel">${esc(m.name)} · ${esc(m.role)}</div><dl class="hq-kv">
      <dt>Doing</dt><dd>${m.paused ? "Paused" : t ? esc((HQ.people[m.id] || {}).doing || "starting") : "Waiting for a wake"}</dd>
      <dt>Run</dt><dd>${run ? "R" + run.run + (t ? " · " + hqKey(t.id) : "") : "none"}</dd>
      <dt>Brain</dt><dd>${esc(((m.brains || []).find((b) => b.name === m.brain) || { label: m.brain || "Local" }).label)}</dd>
      <dt>Budget</dt><dd>${hqMoney(m, `of ${hqDollars(m.per_day)} today`)} · ${m.slots} at once</dd></dl>${t ? why(t) : ""}`;
  }
  const t = hqTask(sel.id); if (!t) return "";
  const runner = ((HQ.snap && HQ.snap.runs) || []).find((r) => r.task === t.id && r.status === "running");
  const thread = (t.thread || []).slice(-3).map((p) => `<li>${esc(p.author)}: ${esc(p.text)}</li>`).join("");
  return `<div class="hq-sel">${hqKey(t.id)} · ${esc(t.title)}</div><dl class="hq-kv">
    <dt>Status</dt><dd>${esc(String(t.status).replace("_", " "))}${t.gate === true ? " · gate passed" : t.gate === false ? " · gate failed" : ""}</dd>
    <dt>Owner</dt><dd>${t.owner ? esc(hqName(t.owner)) : "nobody"}</dd>
    <dt>Lock</dt><dd>${runner ? "held by R" + runner.id : "free"}</dd>
    <dt>Blocked by</dt><dd>${(t.waits_on || []).length ? t.waits_on.map(hqKey).join(", ") : "nothing"}</dd>
    <dt>Depth</dt><dd>${t.depth || 0}</dd></dl>${why(t)}${thread ? `<ul class="hq-thread">${thread}</ul>` : ""}
    <textarea id="hq-reply-${t.id}" rows="2" placeholder="Reply in the thread: whoever has it reads it now" aria-label="Your reply"></textarea>
    <div class="hq-row"><button class="hq-btn ok" data-act="hq:reply:${t.id}:send">Send</button></div>`;
}

// The verbs this screen uses. Returns true when it handled one.
async function hqDispatch(verb, noun, id, arg) {
  if (!(verb === "hq")) return false;
  if (noun === "pick") { HQ.selected = { type: id, id: Number(arg) }; render(); return true; }
  if (noun === "decide") {
    // A question's answer, if the owner wrote one, goes in the task's thread before the member wakes.
    const box = document.getElementById(`hq-answer-${id}`), answer = arg === "seen" && box ? box.value.trim() : "";
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
    try { await api("POST", "/v1/hq/tasks", { goal: goal.id, title, assignee: lead ? lead.id : null }); flash("Handed to the team.", "ok"); }
    catch (e) { flash("Couldn't hand it over: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  return false;
}
