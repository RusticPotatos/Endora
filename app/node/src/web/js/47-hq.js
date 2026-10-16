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
  tab: "desk",         // desk | activity | team
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

// One event, in words for the activity log, and on screen if it is live.
function hqTell(e, live) {
  const d = e.data || {}, task = d.task ? ` on #${d.task}` : "";
  const reason = String(d.reason || "").replace(/_/g, " ");
  const said = {
    "goal.proposed": ["you", `Endora proposes a goal: ${d.title}`],
    "goal.changed": ["run", `Goal ${d.status}: ${d.title}`],
    "task.created": ["run", `New task #${d.task}`],
    "task.changed": ["run", `#${d.task} is ${String(d.status || "").replace("_", " ")}`],
    "task.claimed": ["run", `${hqName(d.member)} took #${d.task}`],
    "task.released": ["warn", `${hqName(d.member)} handed #${d.task} back`],
    "thread.posted": ["run", `${d.author}: ${d.preview}`],
    "wake.queued": ["wake", `Woke ${hqName(d.member)}: ${reason}${task}`],
    "wake.folded": ["wake", `${reason} folded into ${hqName(d.member)}'s turn${task}`],
    "wake.held": ["wake", `${hqName(d.member)} waits${task}: someone else is on it`],
    "wake.refused": ["warn", `Wake for ${hqName(d.member)} turned away: ${String(d.why || "").replace(/_/g, " ")}`],
    "run.started": ["run", `${hqName(d.member)} started a turn${task}`],
    "run.ended": [d.result === "succeeded" ? "done" : "warn", `${hqName(d.member)}'s turn ended: ${String(d.result || "").replace("_", " ")}`],
    "gate.result": [d.passed ? "done" : "bad", `The gate ${d.passed ? "passed" : "failed"} on #${d.task}`],
    "desk.added": ["you", `On your desk: ${String(d.kind || "").replace(/_/g, " ")}`],
    "desk.decided": ["you", `You decided: ${d.decision}`],
    "budget.crossed": ["bad", `${hqName(d.member)} crossed its limit; the turn stopped`],
    "stopped": ["bad", "The kill switch: every turn stopped"],
    "resumed": ["you", "The team works again"],
  }[e.kind];
  if (said) {
    const at = new Date(e.at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    HQ.log.unshift({ at, kind: said[0], text: said[1] });
    HQ.log.length = Math.min(HQ.log.length, 80);
  }
  if (!live) return;
  const p = HQ.people[d.member];
  if (e.kind.startsWith("wake.") && e.kind !== "wake.refused" && p) {
    HQ.envelopes.push({ x: HQ_P.rack.x * HQ_T + 8, y: HQ_P.rack.y * HQ_T + 10, who: d.member, t: 0 });
  }
  if (e.kind === "thread.posted") {
    const author = ((HQ.snap && HQ.snap.members) || []).find((m) => m.name === d.author);
    if (author && HQ.people[author.id]) hqSay(HQ.people[author.id], d.preview);
  }
  if (e.kind === "run.ended" && p) hqSay(p, d.result === "succeeded" ? "done" : String(d.result || "").replace("_", " "));
}

function hqSay(p, text) { p.bubble = String(text || "").slice(0, 28); p.bubbleT = 3; }

// Where each member is, kept across redraws: at their desk while a turn runs, resting otherwise.
function hqPlacePeople() {
  const members = (HQ.snap && HQ.snap.members) || [];
  members.forEach((m, i) => {
    const desk = HQ_DESKS[i % HQ_DESKS.length], rest = HQ_RESTS[i % HQ_RESTS.length];
    let p = HQ.people[m.id];
    if (!p) {
      p = HQ.people[m.id] = { id: m.id, x: rest.x * HQ_T, y: rest.y * HQ_T, path: [], frame: 0, face: 1, bubble: "", bubbleT: 0, working: false };
    }
    p.look = m.look || 0; p.name = m.name; p.paused = m.paused; p.desk = desk;
    const working = (m.running || []).length > 0;
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

function hqPaintRoom() {
  const T = HQ_T, P = HQ_P, C = HQ_C;
  const bg = document.createElement("canvas"); bg.width = HQ_W; bg.height = HQ_H;
  const g = bg.getContext("2d"); g.imageSmoothingEnabled = false;
  const floor = (key, color, x0, y0, x1, y1) => {
    const img = HQ_IMG["floor_" + key]; if (!img) return;
    const t = hqTinted(img, color);
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) g.drawImage(t, x * T, y * T);
  };
  g.fillStyle = "#d6b07a"; g.fillRect(0, 0, HQ_W, HQ_H);
  floor("plank", "#e2b57a", 0, 2, HQ_COLS, HQ_ROWS);
  floor("grid", "#d9e1ea", 0, 2, 7, 6);
  floor("plain", "#c3c8d0", 22, 2, HQ_COLS, 6);
  floor("grid", "#8fa6c7", 0, 12, 13, HQ_ROWS);
  floor("check", "#efe9de", 20, 12, HQ_COLS, HQ_ROWS);
  g.fillStyle = C.wall; g.fillRect(0, 0, HQ_W, 2 * T); g.fillStyle = C.wallTop; g.fillRect(0, 0, HQ_W, 5); g.fillStyle = C.trim; g.fillRect(0, 2 * T - 3, HQ_W, 3);
  const wx = 6, wy = 6; g.fillStyle = "#9fd3f0"; g.fillRect(wx, wy, 52, 20); g.fillStyle = "#cfeaf8"; g.fillRect(wx, wy, 52, 8);
  g.fillStyle = "#4f7fa3"; g.fillRect(wx, wy + 15, 52, 5); g.fillStyle = "#ffffff"; g.fillRect(wx + 25, wy, 2, 20); g.strokeStyle = C.trim; g.strokeRect(wx - 0.5, wy - 0.5, 53, 21);
  g.fillStyle = C.boardEdge; g.fillRect(P.board.x * T - 2, 3, P.board.w * T + 4, 26); g.fillStyle = C.board; g.fillRect(P.board.x * T, 5, P.board.w * T, 22);
  if (HQ_IMG.HANGING_PLANT) g.drawImage(HQ_IMG.HANGING_PLANT, 7 * T - 2, 1);
  if (HQ_IMG.LARGE_PAINTING) g.drawImage(HQ_IMG.LARGE_PAINTING, 20.4 * T, 0);
  g.fillStyle = C.glass; g.fillRect(0, 2 * T, 7 * T, 3.6 * T); g.fillStyle = C.glassEdge; g.fillRect(7 * T - 2, 2 * T, 2, 3.6 * T); g.fillRect(0, 5.6 * T - 2, 7 * T - 22, 2);
  g.fillStyle = "rgba(60,70,90,.08)"; g.fillRect(P.engine.x * T, P.engine.y * T, P.engine.w * T, P.engine.h * T);
  g.fillStyle = C.glassEdge; g.fillRect(P.engine.x * T, P.engine.y * T, 2, P.engine.h * T);
  const px = P.pong.x * T, py = P.pong.y * T;
  g.fillStyle = C.shadow; g.fillRect(px + 2, py + 13, 66, 4); g.fillStyle = "#1f7a4f"; g.fillRect(px, py, 66, 13); g.fillStyle = "#ffffff"; g.fillRect(px, py + 6, 66, 1); g.fillRect(px + 32, py - 3, 2, 16);
  g.fillStyle = "#cfd6dd"; g.fillRect(P.tap.x * T, P.tap.y * T - 6, 14, 20); g.fillStyle = "#3a2a1e"; g.fillRect(P.tap.x * T + 3, P.tap.y * T - 3, 8, 7);
  g.fillStyle = "#ffffff"; g.fillRect(21 * T, 13 * T, 2.6 * T, 11); g.fillStyle = C.trim; g.fillRect(21 * T, 13 * T + 10, 2.6 * T, 2);
  g.fillStyle = "#3d3a44"; g.fillRect(P.door.x * T, P.door.y * T, 2 * T, 3 * T); g.fillStyle = "#9fd3f0"; g.fillRect(P.door.x * T + 4, P.door.y * T + 4, 2 * T - 8, 3 * T - 4);
  g.font = "8px monospace"; g.fillStyle = "rgba(31,29,41,.6)";
  g.fillText("YOUR DESK", 4, 2 * T + 9); g.fillText("SERVERS", (P.engine.x + 3.4) * T, (P.engine.y + P.engine.h) * T - 4);
  g.fillStyle = "rgba(255,255,255,.8)"; g.fillText("PING-PONG", 4, 12 * T + 8);
  g.fillStyle = "rgba(31,29,41,.6)"; g.fillText("LOBBY", 20.4 * T, 12 * T + 9); g.fillText("ENDORA", 21.1 * T, 13 * T + 8);
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
  f("SOFA_FRONT", 8.6 * T, 13.2 * T); f("SOFA_FRONT", 10.6 * T, 13.2 * T);
  f("PLANT", 0.1 * T, 9.6 * T); f("PLANT_2", 26.7 * T, 6.6 * T); f("CACTUS", 21.1 * T, 6.4 * T);
  f("LARGE_PLANT", 19.1 * T, 12.4 * T); f("BOOKSHELF", 22.2 * T, 15 * T);
  return out;
}

// The sticky notes: the newest tasks that are not long finished, two rows of seventeen.
function hqCards() {
  const tasks = ((HQ.snap && HQ.snap.tasks) || []).filter((t) => t.status !== "dropped");
  const shown = tasks.filter((t, i) => t.status !== "done" || i >= tasks.length - 34).slice(-34);
  return shown.map((t, i) => ({ t, x: HQ_P.board.x * HQ_T + 4 + (i % 17) * 11, y: 7 + Math.floor(i / 17) * 10, w: 8, h: 8 }));
}

function hqFrame(p) {
  if (p.path.length) {
    const n = p.path[0], dx = n.x - p.x, dy = n.y - p.y;
    const dir = Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? "right" : "left") : dy > 0 ? "down" : "up";
    return { dir, frame: [0, 1, 2, 1][Math.floor(p.frame) % 4] };
  }
  if (p.working) return { dir: "down", frame: 3 + (Math.floor(HQ.t * 6) % 2) };
  return { dir: "down", frame: p.paused ? 1 : 0 };
}

function hqDraw() {
  const cv = document.getElementById("hq-office");
  if (!cv || !HQ_BG) return;
  const ctx = cv.getContext("2d"), T = HQ_T, P = HQ_P, C = HQ_C, snap = HQ.snap || {};
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(HQ_BG, 0, 0);
  // The board: a sticky note per task, coloured by where it is.
  for (const r of hqCards()) {
    ctx.fillStyle = "rgba(0,0,0,.12)"; ctx.fillRect(r.x + 1, r.y + 1, r.w, r.h);
    ctx.fillStyle = HQ_STATUS_COL[r.t.status] || C.sticky; ctx.fillRect(r.x, r.y, r.w, r.h);
    ctx.fillStyle = "rgba(0,0,0,.18)"; ctx.fillRect(r.x + 2, r.y + 3, r.w - 4, 1);
    if ((snap.runs || []).some((run) => run.task === r.t.id && run.status === "running")) { ctx.fillStyle = C.neon; ctx.fillRect(r.x + r.w - 2, r.y, 2, 2); }
    if (HQ.selected && HQ.selected.type === "task" && HQ.selected.id === r.t.id) { ctx.strokeStyle = C.neon; ctx.strokeRect(r.x - 1.5, r.y - 1.5, r.w + 3, r.h + 3); }
  }
  // The server rack, blinking while the engine runs, red when stopped.
  const rx = P.rack.x * T, ry = P.rack.y * T;
  ctx.fillStyle = C.rack; ctx.fillRect(rx, ry, 18, 40); ctx.fillStyle = "#333b4a"; ctx.fillRect(rx + 2, ry + 2, 14, 36);
  for (let i = 0; i < 7; i++) {
    const on = (Math.floor(HQ.t * 2) + i) % 3 === 0;
    ctx.fillStyle = snap.stopped ? C.red : on ? C.green : "#24412f"; ctx.fillRect(rx + 4, ry + 5 + i * 5, 2, 2);
  }
  // The burn gauge: today's spend against the pool's day.
  const pool = snap.pool || { per_day: 1, committed: 0 }, fill = Math.min(1, (pool.committed || 0) / Math.max(1, pool.per_day));
  const jx = P.jar.x * T, jy = P.jar.y * T, hgt = 30;
  ctx.fillStyle = "#ffffff"; ctx.fillRect(jx, jy, 8, hgt); ctx.strokeStyle = C.trim; ctx.strokeRect(jx - 0.5, jy - 0.5, 9, hgt + 1);
  ctx.fillStyle = fill > .85 ? C.red : fill > .6 ? C.lamp : C.green; ctx.fillRect(jx + 1, jy + hgt - Math.round(hgt * fill), 6, Math.round(hgt * fill));
  ctx.font = "8px monospace"; ctx.fillStyle = "rgba(31,29,41,.65)"; ctx.fillText("BURN", jx + 12, jy + hgt);
  // The kill switch on the glass of your desk's room.
  const lx = P.lever.x * T, ly = P.lever.y * T, up = !snap.stopped;
  ctx.fillStyle = "#3a3a46"; ctx.fillRect(lx + 2, ly + 14, 12, 4);
  ctx.fillStyle = "#9aa"; ctx.fillRect(lx + 7, up ? ly + 4 : ly + 10, 2, up ? 10 : 6); ctx.fillStyle = C.red; ctx.fillRect(lx + 5, up ? ly + 1 : ly + 14, 6, 4);
  // People and furniture, in depth order.
  const people = Object.values(HQ.people);
  const items = HQ_FURN.map((f) => ({ base: f.base, f }));
  for (const p of people) items.push({ base: p.y + 16, p });
  items.sort((a, b) => a.base - b.base);
  for (const it of items) {
    if (it.p) { hqDrawPerson(ctx, it.p); continue; }
    let key = it.f.key;
    if (it.f.pc !== undefined) {
      const at = people.find((p) => p.working && p.desk === HQ_DESKS[it.f.pc] && !p.path.length);
      key = at ? "PC_FRONT_ON_" + (1 + (Math.floor(HQ.t * 4) % 3)) : "PC_FRONT_OFF";
    }
    if (HQ_IMG[key]) ctx.drawImage(HQ_IMG[key], Math.round(it.f.x), Math.round(it.f.y));
  }
  ctx.font = "8px monospace";
  for (const p of people) { ctx.fillStyle = "rgba(31,29,41,.85)"; ctx.fillText(p.name || "", Math.round(p.x) + 1, Math.round(p.y) + 24); }
  // Decisions waiting on your table.
  (snap.desk || []).slice(0, 5).forEach((n, i) => {
    ctx.fillStyle = n.kind === "goal" ? C.violet : n.kind === "stuck" ? C.red : C.green;
    ctx.fillRect(P.owner.x * T + 3 + i * 6, P.owner.y * T + 6 + (i % 2) * 3, 5, 6);
  });
  // Wakes in flight, from the server closet to whoever was woken.
  for (const e of HQ.envelopes) {
    const p = HQ.people[e.who]; if (!p) continue;
    const k = Math.min(1, e.t / 0.9), x = e.x + (p.x + 6 - e.x) * k, y = e.y + (p.y - 22 - e.y) * k - Math.sin(k * Math.PI) * 18;
    ctx.fillStyle = "#ffffff"; ctx.fillRect(Math.round(x), Math.round(y), 7, 5); ctx.fillStyle = C.neon; ctx.fillRect(Math.round(x) + 1, Math.round(y) + 1, 5, 1);
  }
  for (const p of people) {
    if (p.bubbleT <= 0 || !p.bubble) continue;
    const tw = Math.ceil(ctx.measureText(p.bubble).width) + 6, x = Math.round(Math.min(HQ_W - tw - 2, Math.max(2, p.x + 8 - tw / 2))), y = Math.round(p.y - 30);
    ctx.fillStyle = "#ffffff"; ctx.fillRect(x, y, tw, 10); ctx.fillStyle = "#1f1d29"; ctx.fillRect(x, y + 10, tw, 1); ctx.fillText(p.bubble, x + 3, y + 8);
  }
  if (snap.stopped) { ctx.fillStyle = "rgba(239,90,95,.12)"; ctx.fillRect(0, 0, HQ_W, HQ_H); ctx.fillStyle = C.red; ctx.fillText("KILL SWITCH PULLED", HQ_W / 2 - 44, 2 * T + 22); }
}

function hqDrawPerson(ctx, p) {
  const img = HQ_IMG["char" + (p.look % 6)], x = Math.round(p.x), y = Math.round(p.y);
  ctx.fillStyle = HQ_C.shadow; ctx.fillRect(x + 3, y + 13, 10, 3);
  if (img) {
    const { dir, frame } = hqFrame(p), row = dir === "up" ? 1 : dir === "down" ? 0 : 2;
    if (dir === "left") { ctx.save(); ctx.translate(x + 16, 0); ctx.scale(-1, 1); ctx.drawImage(img, frame * 16, row * 32, 16, 32, 0, y - 16, 16, 32); ctx.restore(); }
    else ctx.drawImage(img, frame * 16, row * 32, 16, 32, x, y - 16, 16, 32);
  }
  if (p.paused) { ctx.fillStyle = HQ_C.lamp; ctx.fillRect(x + 13, y - 20, 6, 6); }
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

// ---- the screen ------------------------------------------------------------------------

function viewHQ() {
  const head = `<h2>HQ</h2>`;
  if (HQ.error) return `${head}<div class="card"><div class="sub">${esc(HQ.error)}</div></div>`;
  const s = HQ.snap;
  if (!s) return `${head}<div class="empty">Loading…</div>`;
  setTimeout(() => { hqLoop(); const cv = document.getElementById("hq-office"); if (cv && !cv.dataset.tapped) { cv.dataset.tapped = "1"; cv.addEventListener("click", hqTap); } }, 0);
  const pool = s.pool || {}, money = (micros) => "$" + ((micros || 0) / 1e6).toFixed(2);
  const tab = (name, label, n) => `<button class="${HQ.tab === name ? "primary" : "ghost"}" data-act="hq:tab:${name}">${label}${n ? ` (${n})` : ""}</button>`;
  const desk = (s.desk || []).map((d) => {
    const choices = { goal: [["approve", "Approve"], ["decline", "Decline"]], stuck: [["retry", "Try again"], ["drop", "Drop it"]],
      ready_to_merge: [["merged", "Mark merged"], ["not_now", "Not now"]],
      ready_to_read: [["read", "Mark read"], ["not_now", "Not now"]] }[d.kind] || [];
    // In the demo nothing behind a card is real: say so on the card, not only in the header.
    const demoNote = s.demo ? (d.kind === "ready_to_merge" ? " · demo: there is no pull request behind this"
      : d.kind === "ready_to_read" ? " · demo: there is no write-up behind this" : " · demo") : "";
    return `<div class="card"><div class="title">${s.demo ? "Demo: " : ""}${esc(d.text)}</div>
      <div class="sub">${esc(String(d.kind).replace(/_/g, " "))}${d.task ? ` · #${d.task}` : ""}${demoNote}</div>
      <div class="row" style="gap:8px;margin-top:8px;">${choices.map(([v, l]) => `<button class="ghost" data-act="hq:decide:${d.id}:${v}">${l}</button>`).join("")}</div></div>`;
  });
  const log = HQ.log.slice(0, 50).map((l) => `<div class="row" style="gap:8px;"><span class="sub" style="min-width:44px;">${esc(l.at)}</span><span>${esc(l.text)}</span></div>`);
  const team = (s.members || []).map((m) => {
    const used = Math.min(100, Math.round(100 * (m.committed || 0) / Math.max(1, m.per_day)));
    const on = (m.running || []).map((r) => r.task ? `#${r.task}` : "a look around").join(", ");
    return `<div class="card" data-act="hq:pick:member:${m.id}" style="cursor:pointer;"><div class="row" style="justify-content:space-between;">
      <div class="title">${esc(m.name)}</div><span class="sub">${esc(m.role)}${m.paused ? " · paused" : ""}</span></div>
      <div class="sub">${on ? "Working on " + esc(on) : "Between turns"} · ${money(m.committed)} of ${money(m.per_day)} today</div>
      <div style="height:4px;background:var(--line);border-radius:2px;margin-top:6px;"><div style="height:4px;width:${used}%;background:var(--accent);border-radius:2px;"></div></div></div>`;
  });
  const body = HQ.tab === "activity" ? listOr(log, "Nothing has happened yet.")
    : HQ.tab === "team" ? listOr(team, "No team yet.")
    : listOr(desk, "Nothing waiting for you.");
  return `
    <div class="row" style="justify-content:space-between;align-items:center;gap:8px;">
      ${head}
      ${s.demo ? `<span class="pill" style="background:#e8378f;color:#fff;">Demo · scripted team</span>` : ""}
    </div>
    <canvas id="hq-office" width="${HQ_W}" height="${HQ_H}" style="width:100%;height:auto;image-rendering:pixelated;border-radius:6px;cursor:pointer;"
      aria-label="The team's office: members at their desks, the board of tasks, the server closet, your desk"></canvas>
    <div class="row" style="gap:6px;flex-wrap:wrap;margin:8px 0;">
      <span class="sub">${s.demo ? `Demo: nothing is spent; turns reserve ${money(pool.committed)} of ${money(pool.per_day)} as real ones will`
        : `Spent today ${money(pool.committed)} of ${money(pool.per_day)}`}</span>
      <span class="grow"></span>
      <button class="ghost ${s.stopped ? "" : "danger"}" data-act="${s.stopped ? "hq:resume" : "hq:stop"}">${s.stopped ? "Resume" : "Kill switch"}</button>
    </div>
    ${hqSelected()}
    <div class="row" style="gap:6px;margin:10px 0;">${tab("desk", "Your desk", (s.desk || []).length)}${tab("activity", "Activity")}${tab("team", "Team")}</div>
    ${body}
    <details class="card" style="margin-top:12px;"><summary class="title">Set the team a goal</summary>
      <div class="form"><input id="hq-goal" placeholder="e.g. Keep the home lab healthy" /><button class="primary" data-act="hq:goal">Set it</button></div></details>
    <p class="sub" style="margin-top:12px;">Office art: Pixel Agents (MIT) and MetroCity by JIK-A-4 (CC0); <a href="/hq-art/CREDITS.txt">credits</a>.</p>`;
}

// What was tapped, in detail: a member's turn and budget, or a task and its chain of why.
function hqSelected() {
  const sel = HQ.selected; if (!sel) return "";
  const money = (micros) => "$" + ((micros || 0) / 1e6).toFixed(2);
  if (sel.type === "member") {
    const m = hqMember(sel.id); if (!m) return "";
    const run = (m.running || [])[0], t = run && run.task ? hqTask(run.task) : null;
    return `<div class="card"><div class="title">${esc(m.name)} · ${esc(m.role)}</div>
      <div class="sub">${t ? "On #" + t.id + ": " + esc(t.title) : "Between turns"}${m.paused ? " · paused" : ""}</div>
      <div class="sub">${money(m.committed)} of ${money(m.per_day)} today · ${m.slots} at once</div>
      ${t ? `<ul class="why">${(t.why || []).map((w) => `<li>${esc(w)}</li>`).join("")}</ul>` : ""}</div>`;
  }
  const t = hqTask(sel.id); if (!t) return "";
  const thread = (t.thread || []).slice(-4).map((p) => `<div class="sub">${esc(p.author)}: ${esc(p.text)}</div>`).join("");
  return `<div class="card"><div class="title">#${t.id} · ${esc(t.title)}</div>
    <div class="sub">${esc(String(t.status).replace("_", " "))} · ${t.owner ? esc(hqName(t.owner)) : "nobody yet"} · ${esc(t.priority)}${t.gate === true ? " · gate passed" : t.gate === false ? " · gate failed" : ""}</div>
    <ul class="why">${(t.why || []).map((w) => `<li>${esc(w)}</li>`).join("")}</ul>${thread}</div>`;
}

// The verbs this screen uses. Returns true when it handled one.
async function hqDispatch(verb, noun, id, arg) {
  if (!(verb === "hq")) return false;
  if (noun === "tab") { HQ.tab = id; render(); return true; }
  if (noun === "pick") { HQ.selected = { type: id, id: Number(arg) }; render(); return true; }
  if (noun === "decide") {
    try { await api("POST", `/v1/hq/desk/${id}`, { decision: arg }); flash("Decided.", "ok"); }
    catch (e) { flash("Couldn't decide: " + e.message, "err"); }
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
  return false;
}
