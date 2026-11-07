// ---- Endora HQ: the studio (the owner, 4 Nov) ----------------------------------------------------
// The team at work in a small research studio with a history: a work area of oak planks under the
// board, the owner's corner one step up on the old office's blue tile, a library alcove set into the
// back wall, a lounge one step down on terracotta, and a kitchenette on the old checkerboard. Glass
// stubs and a half-wall mark the areas without shutting anyone in. Everything that changes is the
// record's: a desk's lamp is lit only while a turn runs there, a turn waiting to retry keeps its desk
// dark, a stuck worker sits at the review table in the owner's corner, chess players sit at the board,
// and whoever rests sits in the lounge. Words are on the board, in a brief bubble when a real post
// arrives, and in what a tap opens; never on the floor.

// The studio, in its own pixels; drawn at three times that for crisp words. A phone gets its own plan,
// the same areas stacked for its width.
const HQ_STUDIO_SCALE = 3;
const HQ_SAY_SECONDS = 4;
const HQ_SAY_MOST = 30;
const HQ_DONE_KINDS = { met_criteria: true, completed_unverified: false };
const HQ_WEEK_DAYS = 7;
// The studio's words are in IBM Plex Mono, which HQ already loads: no font of its own.
const HQ_FONT_8 = '8px "IBM Plex Mono", monospace', HQ_FONT_8B = '600 8px "IBM Plex Mono", monospace', HQ_FONT_7 = '7px "IBM Plex Mono", monospace', HQ_FONT_7S = '600 7px "IBM Plex Mono", monospace';
const HQ_FONT_6 = '6px "IBM Plex Mono", monospace', HQ_FONT_6B = '600 6px "IBM Plex Mono", monospace', HQ_FONT_7B = '600 7px "IBM Plex Mono", monospace';
const HQ_SIGN_FONT = "8px Silkscreen, monospace";
const HQ_STUDIO = {
  wide: {
    W: 400, H: 208, wallH: 32, board: { x: 84, w: 196 },
    zones: {
      work: { x: 96, y: 32, w: 204, h: 80, floor: "plank" },
      hall: { x: 0, y: 112, w: 80, h: 96, floor: "plank" },
      owner: { x: 0, y: 32, w: 96, h: 80, floor: "tile", raised: true },
      alcove: { x: 300, y: 24, w: 100, h: 88, floor: "parquet", recess: true },
      lounge: { x: 80, y: 112, w: 250, h: 96, floor: "terra", sunken: true },
      kitchen: { x: 330, y: 112, w: 70, h: 96, floor: "check" },
    },
    walls: [["v", 96, 32, 70], ["v", 300, 32, 66], ["half", 330, 112, 160]],
    desks: [{ x: 106, y: 40 }, { x: 166, y: 48 }, { x: 236, y: 62 }],
    shelf: { x: 308, y: 4, w: 84, rows: 4, pitch: 9 }, bench: { x: 318, y: 74, w: 54 },
    ownerDesk: { x: 6, y: 40 }, lever: { x: 44, y: 36 }, rack: { x: 62, y: 38 },
    review: { x: 40, y: 76 }, chairs: [{ x: 20, y: 84, dir: "right" }, { x: 74, y: 84, dir: "left" }],
    bookcase: { x: 112, y: 148, w: 104 }, sofas: [{ x: 120, y: 162 }, { x: 164, y: 162 }],
    rug: { x: 110, y: 160, w: 182, h: 40 }, chess: { x: 262, y: 158 }, counter: { x: 336, y: 116, w: 60 }, stools: [{ x: 340, y: 146 }, { x: 366, y: 146 }],
    door: { x: 0, y: 140 }, window: { x: 286, y: 6 }, sign: true,
    // Decor stands for nothing: never a sign of state (council round 6, after the owner's old office).
    decor: [["PLANT", 8, 174], ["LARGE_PLANT", 204, 158], ["CACTUS", 58, 118], ["BIN", 32, 190], ["PLANT_2", 378, 62], ["HANGING_PLANT", 60, 0]],
  },
  phone: {
    W: 192, H: 304, wallH: 36, board: { x: 4, w: 184 }, boardFont: 8,
    zones: {
      work: { x: 0, y: 36, w: 192, h: 80, floor: "plank" },
      owner: { x: 0, y: 116, w: 100, h: 84, floor: "tile", raised: true },
      alcove: { x: 100, y: 116, w: 92, h: 84, floor: "parquet" },
      lounge: { x: 0, y: 200, w: 136, h: 100, floor: "terra", sunken: true },
      kitchen: { x: 136, y: 200, w: 56, h: 100, floor: "check" },
    },
    walls: [["v", 100, 116, 150], ["h", 0, 116, 34], ["h", 66, 116, 100], ["half", 136, 200, 244]],
    desks: [{ x: 8, y: 42 }, { x: 72, y: 56 }, { x: 136, y: 42 }],
    shelf: { x: 106, y: 120, w: 80, rows: 4, pitch: 9 }, bench: { x: 114, y: 176, w: 44 },
    ownerDesk: { x: 6, y: 124 }, lever: { x: 44, y: 120 }, rack: { x: 62, y: 124 },
    review: { x: 40, y: 160 }, chairs: [{ x: 22, y: 166, dir: "right" }],
    bookcase: { x: 10, y: 222, w: 84 }, sofas: [{ x: 18, y: 236 }, { x: 58, y: 236 }],
    rug: { x: 8, y: 234, w: 124, h: 40 }, chess: { x: 104, y: 256 }, counter: { x: 140, y: 206, w: 48 }, stools: [{ x: 144, y: 232 }, { x: 168, y: 232 }],
    door: null, window: null, sign: false,
    decor: [["PLANT", 4, 268], ["PLANT", 170, 268], ["CACTUS", 166, 164]],
  },
};

function hqStudioMode() {
  try { return typeof matchMedia === "function" && matchMedia("(max-width: 767px)").matches ? "phone" : "wide"; } catch (_) { return "wide"; }
}
function hqWeekNow() { return HQ.weekNow || Date.now(); }
// Every item closed this week as met its criteria or as completed and not verified, oldest first.
function hqWeekDone(now = hqWeekNow()) {
  const since = now - HQ_WEEK_DAYS * 86400000;
  return ((HQ.snap && HQ.snap.tasks) || [])
    .filter((t) => t.closure && t.closure.why in HQ_DONE_KINDS && t.closure.at >= since && t.closure.at <= now)
    .sort((a, b) => a.closure.at - b.closure.at || a.id - b.id);
}
// What a project is about, in a word or two: what its title names after its last colon, else all of it.
function hqSubjectOf(t) {
  const p = t && ((HQ.snap && HQ.snap.projects) || []).find((x) => x.id === t.project);
  const title = p ? String(p.title) : "", i = title.lastIndexOf(": ");
  return i >= 0 ? title.slice(i + 2).trim() : title;
}
// The turn a member waits on: interrupted, waiting to retry. Nothing runs, so nothing is lit.
function hqWaitingRun(id) { return ((HQ.snap && HQ.snap.runs) || []).find((r) => r.member === id && r.status === "retry_waiting") || null; }
// A real post, made short for a bubble: no branch names, ids, hashes or receipts, a few words at most.
function hqSaid(text) {
  const t = String(text || "").replace(/^[a-z_]+ → [a-z_]+: /, "").replace(/^[A-Z][a-z]+: /, "")
    .replace(/\b(?:hq\/)?task-\d+[\w-]*/gi, "the branch").replace(/\bEL-\d+\b/g, "it").replace(/\(?\b[0-9a-f]{7,40}\b\)?/g, "")
    .replace(/\s*\([^)]*\b(?:receipt|document|files?)\b[^)]*\)/gi, "").replace(/[_*`]/g, "").replace(/…$/, "").replace(/\s+/g, " ").trim();
  if (t.length <= HQ_SAY_MOST) return t;
  const cut = t.slice(0, HQ_SAY_MOST), at = cut.lastIndexOf(" ");
  return (at > HQ_SAY_MOST / 2 ? cut.slice(0, at) : cut).replace(/[\s,.;:(–-]+$/, "") + "…";
}
// The goal the board names: the one a turn works for now, else the one whose item closed last this
// week, else the one with the most open work.
function hqGoalNow(snap = HQ.snap || {}) {
  const tasks = snap.tasks || [], week = hqWeekDone(), open = tasks.filter((t) => HQ_OPEN.includes(t.status));
  const live = (snap.members || []).filter((m) => !m.furloughed).flatMap((m) => (m.running || []).map((r) => r.task));
  const working = (g) => live.some((id) => (tasks.find((t) => t.id === id) || {}).goal === g.id);
  const last = (g) => Math.max(0, ...week.filter((t) => t.goal === g.id).map((t) => t.closure.at));
  const openOf = (g) => open.filter((t) => t.goal === g.id).length;
  return (snap.goals || []).filter((g) => g.status === "active").sort((a, b) => working(b) - working(a) || last(b) - last(a) || openOf(b) - openOf(a) || a.id - b.id)[0] || null;
}
// The board's words, short: the goal, how much is open and stuck, and who is on what by subject.
// Full titles wait behind a tap.
function hqBoard(snap = HQ.snap || {}) {
  const g = hqGoalNow(snap), tasks = snap.tasks || [];
  const open = tasks.filter((t) => g && t.goal === g.id && HQ_OPEN.includes(t.status)), stuck = open.filter((t) => t.status === "blocked").length;
  const lines = [];
  for (const m of (snap.members || []).filter((x) => !x.furloughed)) for (const r of m.running || []) {
    const t = r.task ? tasks.find((x) => x.id === r.task) : null;
    const blocked = open.filter((x) => x.status === "blocked" && x.owner === m.id).length;
    lines.push({ who: m.name, text: `${t ? `on ${hqSubjectOf(t) || "an item"}` : "looking round"}${blocked ? `; ${blocked} stuck` : ""}`, mine: !!(t && g && t.goal === g.id) });
  }
  for (const r of (snap.runs || []).filter((x) => x.status === "retry_waiting")) {
    const m = (snap.members || []).find((x) => x.id === r.member);
    if (m && !m.furloughed && !lines.some((l) => l.who === m.name)) lines.push({ who: m.name, text: "waits to retry", mine: false, amber: true });
  }
  lines.sort((a, b) => b.mine - a.mine);
  if (!lines.length) {
    const week = hqWeekDone(), unverified = week.filter((t) => !HQ_DONE_KINDS[t.closure.why]).length;
    const blocked = open.filter((t) => t.status === "blocked"), who = [...new Set(blocked.filter((t) => t.owner).map((t) => hqName(t.owner)))];
    lines.push({ who: "", text: blocked.length ? `Nobody is working; ${blocked.length} stuck${who.length ? ` with ${who.join(", ")}` : ""}` : "Nobody is working now." });
    if (week.length) lines.push({ who: "", text: `${week.length} tasks closed this week${unverified ? `, ${unverified} not verified` : ""}.` });
  }
  const parts = g ? [`${open.length} open${stuck ? "," : ""}`, ...(stuck ? [` ${stuck} stuck`] : [])] : [];
  return { title: g ? g.title : "No goal yet", count: parts.join(""), parts, stuck: stuck > 0, lines, stopped: !!snap.stopped };
}

// Whether a member has work that is stuck: an item blocked, or open with its latest check failed.
function hqIsStuck(id) {
  return ((HQ.snap && HQ.snap.tasks) || []).some((t) => t.owner === id && (t.status === "blocked" || (HQ_OPEN.includes(t.status) && (hqLastCheck(t) || {}).ok === false)));
}
// The scene: where everything stands, in the studio's pixels, for drawing, taps and checks alike.
function hqStudioScene(mode = hqStudioMode()) {
  const L = HQ_STUDIO[mode], snap = HQ.snap || {}, tasks = snap.tasks || [];
  const task = (id) => tasks.find((t) => t.id === id), member = (id) => (snap.members || []).find((m) => m.id === id);
  const scene = { mode, L, w: L.W, h: L.H, desks: [], people: [], taps: [], important: [] };
  // At a desk: each running turn (its lamp lit), then each turn waiting to retry whose member runs
  // nothing else (its lamp and screen dark). Four desks at most; a fourth only when needed.
  const atDesk = [];
  for (const m of (snap.members || []).filter((x) => !x.furloughed)) for (const r of (m.running || []).slice(0, 1)) atDesk.push({ m, t: r.task ? task(r.task) : null, kind: "work" });
  for (const r of (snap.runs || []).filter((x) => x.status === "retry_waiting")) {
    const m = member(r.member);
    if (m && !m.furloughed && !atDesk.some((a) => a.m.id === m.id)) atDesk.push({ m, t: task(r.task), kind: "wait" });
  }
  // In the board's order, so its first line is the left desk: work on the goal first, then other
  // work, then waits.
  const g = hqGoalNow(snap), mine = (a) => (a.kind === "work" && a.t && g && a.t.goal === g.id ? 0 : a.kind === "work" ? 1 : 2);
  atDesk.sort((a, b) => mine(a) - mine(b));
  const desks = L.desks.slice();
  if (atDesk.length > desks.length && mode === "wide") desks.push({ x: 236, y: 92 });
  desks.forEach((d, i) => {
    const who = atDesk[i] || null;
    scene.desks.push({ x: d.x, y: d.y, who, lamp: !!(who && who.kind === "work"), screen: !!(who && who.kind === "work") });
    // A worker waiting to retry has turned from the dark screen toward the hourglass: the figure says
    // "waiting", not only the props (council round 13).
    if (who) scene.people.push({ m: who.m, t: who.t, kind: who.kind, x: d.x + 16, y: d.y + 12, dir: who.kind === "wait" ? "right" : "up", seat: "desk" });
    scene.important.push({ x: d.x + 16, y: d.y - 6, w: 16, h: 22, what: "screen" });
  });
  // Everyone else, in a seat: chess players at the board, the stuck at the review table in the owner's
  // corner, then the sofas, the alcove's bench and the counter's stools.
  const game = (((snap.games || {}).chess || {}).games || []).find((x) => x.status === "in_progress");
  // The fourth to rest takes a stool at the counter rather than the last sofa seat, so a full lounge
  // also fills the kitchenette (council round 7): still only resting, never pretend work.
  const sofa = L.sofas.flatMap((s) => [{ x: s.x, y: s.y - 14, dir: "down", seat: "sofa" }, { x: s.x + 16, y: s.y - 14, dir: "down", seat: "sofa" }]);
  const stools = L.stools.map((s) => ({ x: s.x, y: s.y - 14, dir: "down", seat: "stool" }));
  const seats = [
    ...sofa.slice(0, 3), ...stools.slice(0, 1), ...sofa.slice(3),
    { x: L.bench.x + 4, y: L.bench.y - 12, dir: "down", seat: "bench" }, { x: L.bench.x + L.bench.w - 20, y: L.bench.y - 12, dir: "down", seat: "bench" },
    ...stools.slice(1),
  ];
  const chairs = L.chairs.map((c) => ({ x: c.x, y: c.y - 14, dir: c.dir, seat: "chair" }));
  for (const m of (snap.members || []).filter((x) => !x.furloughed)) {
    if (atDesk.some((a) => a.m.id === m.id)) continue;
    const plays = game && (game.white_id === m.id || game.black_id === m.id || game.white === m.name || game.black === m.name);
    let seat = null, kind = "rest";
    if (plays) { const white = game.white_id === m.id || game.white === m.name; seat = { x: white ? L.chess.x - 16 : L.chess.x + 16, y: L.chess.y - 14, dir: white ? "right" : "left", seat: "stool" }; kind = "chess"; }
    else if (hqIsStuck(m.id) && chairs.length) { seat = chairs.shift(); kind = "stuck"; }
    else seat = seats.shift() || chairs.shift();
    if (!seat) continue;
    scene.people.push({ m, t: null, kind, ...seat });
  }
  // The things a person, a bubble or a tap card must never cover.
  const B = L.board, S = L.shelf;
  scene.important.push({ x: B.x - 2, y: 0, w: B.w + 4, h: L.wallH - 2, what: "board" }, { x: S.x - 2, y: S.y - 2, w: S.w + 4, h: S.rows * S.pitch + 5, what: "shelf" },
    { x: L.ownerDesk.x, y: L.ownerDesk.y, w: 48, h: 20, what: "owner desk" }, { x: L.review.x, y: L.review.y, w: 32, h: 16, what: "review table" },
    { x: L.chess.x, y: L.chess.y, w: 16, h: 16, what: "chess" }, { x: L.rack.x, y: L.rack.y, w: 14, h: 30, what: "rack" });
  // What a tap falls on.
  for (const p of scene.people) scene.taps.push({ x: p.x, y: p.y, w: 16, h: 32, sel: { type: "member", id: p.m.id } });
  scene.taps.push({ x: L.lever.x - 2, y: L.lever.y - 2, w: 12, h: 18, act: snap.stopped ? "hq:resume" : "hq:stop" },
    { x: B.x - 2, y: 0, w: B.w + 4, h: L.wallH - 2, sel: { type: "goal", id: 0 } },
    { x: S.x - 2, y: S.y - 2, w: S.w + 4, h: S.rows * S.pitch + 5, sel: { type: "shelf", id: 0 } },
    { x: L.bookcase.x, y: L.bookcase.y, w: L.bookcase.w, h: 14, sel: { type: "knowing", id: 0 } },
    { x: L.chess.x - 14, y: L.chess.y - 4, w: 46, h: 26, sel: { type: "chess", id: 0 } },
    { x: L.zones.owner.x, y: L.zones.owner.y, w: L.zones.owner.w, h: L.zones.owner.h, sel: { type: "corner", id: 0 } });
  // A real post just made, over its author, beside the head, never over a screen, the board or a sign.
  for (const p of scene.people) {
    const said = HQ.said[p.m.id];
    if (said && said.t > 0) p.bubble = hqBubbleSpot(scene, p, said.text);
  }
  return scene;
}
// Where a bubble goes: to the right of the head, else the left, else above, the first spot that covers
// nothing important and stays inside the studio.
function hqBubbleSpot(scene, p, text) {
  const w = Math.min(120, Math.round(text.length * 3) + 6), h = 9;
  const tries = [{ x: p.x + 18, y: p.y + 2 }, { x: p.x - w - 2, y: p.y + 2 }, { x: p.x + 18, y: p.y + 14 }, { x: p.x - w - 2, y: p.y + 14 },
    { x: p.x + 8 - w / 2, y: p.y - h - 3 }, { x: p.x + 8 - w / 2, y: p.y + 33 }];
  const inside = (r) => r.x >= 1 && r.x + r.w <= scene.w - 1 && r.y >= 1 && r.y + r.h <= scene.h - 1 && !scene.important.some((o) => hqOverlap(o, r));
  const alone = (r) => !scene.people.some((q) => q !== p && hqOverlap({ x: q.x, y: q.y, w: 16, h: 32 }, r));
  // First a spot clear of everyone; else one clear of what matters, over a neighbour's shoulder.
  for (const fits of [(r) => inside(r) && alone(r), inside]) {
    for (const t of tries) { const r = { x: Math.round(t.x), y: Math.round(t.y), w, h, text }; if (fits(r)) return r; }
  }
  return null;
}
function hqOverlap(a, b) { return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h; }

// ---- drawing ------------------------------------------------------------------------------------

function hqStudioCanvasTag() {
  const mode = hqStudioMode(), L = HQ_STUDIO[mode];
  return `<canvas id="hq-office" data-mode="${mode}" width="${L.W * HQ_STUDIO_SCALE}" height="${L.H * HQ_STUDIO_SCALE}"
    aria-label="The team's studio: desks under the board, where a lit lamp means a turn is running; your corner with the decisions waiting for you, the review table and the kill switch; the library alcove with the week's done work; the lounge where the team rests and plays chess. Tap a person or a place to read about it."></canvas>`;
}
let HQ_STUDIO_WATCHED = false;
function hqStudioWatch() {
  if (HQ_STUDIO_WATCHED || typeof matchMedia !== "function") return;
  HQ_STUDIO_WATCHED = true;
  try { matchMedia("(max-width: 767px)").addEventListener("change", () => { if (NAV.v === "hq" && !hqTyping()) render(); }); } catch (_) { /* an old browser keeps the plan it opened with */ }
}
function hqDraw() {
  const cv = document.getElementById("hq-office");
  if (!cv || typeof cv.getContext !== "function") return;
  const scene = hqStudioScene(cv.dataset && cv.dataset.mode ? cv.dataset.mode : hqStudioMode());
  HQ.scene = scene;
  const g = cv.getContext("2d");
  g.setTransform(HQ_STUDIO_SCALE, 0, 0, HQ_STUDIO_SCALE, 0, 0);
  g.imageSmoothingEnabled = false;
  hqDrawRoom(g, scene);
  hqDrawPlaces(g, scene);
  hqDrawPeople(g, scene);
}
function hqR(g, x, y, w, h, c) { g.fillStyle = c; g.fillRect(Math.round(x), Math.round(y), Math.round(w), Math.round(h)); }
function hqImg(g, key, x, y) { if (HQ_IMG[key]) g.drawImage(HQ_IMG[key], Math.round(x), Math.round(y)); }
function hqFit(g, text, most) {
  let t = String(text || "");
  if (g.measureText(t).width <= most) return t;
  while (t.length > 3 && g.measureText(t + "…").width > most) t = t.slice(0, -1);
  return t.replace(/[\s,.;:(–-]+$/, "") + "…";
}

// The building: each area's floor, its threshold (a step up, a step down, a recess), the walls.
function hqDrawRoom(g, scene) {
  const L = scene.L;
  hqR(g, 0, 0, L.W, L.H, "#c99a66");
  for (const k of ["work", "hall", "alcove", "lounge", "kitchen", "owner"]) if (L.zones[k]) hqFloor(g, L.zones[k], L.W);
  hqR(g, 0, 0, L.W, L.wallH, "#efe7da"); hqR(g, 0, L.wallH - 3, L.W, 3, "#c9b9a0");
  const A = L.zones.alcove;
  if (A.recess) { hqR(g, A.x, 0, A.w, A.y, "#e4dac8"); hqR(g, A.x, A.y - 3, A.w, 3, "#c9b9a0"); hqR(g, A.x - 3, 0, 3, L.wallH, "#d8ccb6"); }
  // Light: a little cooler in the owner's corner, warmer in the lounge.
  const O = L.zones.owner, Lo = L.zones.lounge;
  hqR(g, O.x, O.y, O.w, O.h, "rgba(200,225,255,.08)"); hqR(g, Lo.x, Lo.y + 5, Lo.w, Lo.h - 5, "rgba(255,190,120,.08)");
  for (const [kind, a, b, c] of L.walls) {
    if (kind === "v") { hqR(g, a - 1, b, 3, c - b, "#9cc7de"); hqR(g, a - 1, b, 1, c - b, "rgba(255,255,255,.7)"); hqR(g, a - 2, c - 2, 5, 3, "#7d8b99"); }
    if (kind === "h") { hqR(g, a, b - 1, c - a, 3, "#9cc7de"); hqR(g, a, b - 1, c - a, 1, "rgba(255,255,255,.7)"); hqR(g, c - 2, b - 2, 3, 5, "#7d8b99"); }
    if (kind === "half") { hqR(g, a - 2, b, 5, c - b, "#d9cdb8"); hqR(g, a - 2, b, 5, 2, "#efe7da"); hqR(g, a + 2, b, 1, c - b, "rgba(0,0,0,.15)"); }
  }
  if (L.door) { const d = L.door; hqR(g, d.x, d.y, 4, 30, "#5a4630"); hqR(g, d.x + 1, d.y + 2, 2, 26, "#8a6d48"); hqR(g, d.x + 2, d.y + 14, 1, 2, "#e0c070"); }
  if (L.window) { const w = L.window; hqR(g, w.x, w.y, 10, 20, "#bcd9e8"); hqR(g, w.x, w.y, 10, 1, "#ffffff"); hqR(g, w.x + 4, w.y, 1, 20, "#e4f0f6"); hqR(g, w.x - 1, w.y + 20, 12, 2, "#c9b9a0"); }
  if (L.sign) { g.font = HQ_SIGN_FONT; g.fillStyle = HQ_C.neon; g.shadowColor = HQ_C.neon; g.shadowBlur = hqCalm() ? 0 : 4; g.fillText("ENDORA", 8, 19); g.shadowBlur = 0; }
  // The board: the one place of words.
  const B = L.board, board = hqBoard(), x0 = B.x + 4, bw = B.w - 8;
  // Larger type on the whole wall (council round 1: the board's words were too small to read).
  hqR(g, B.x - 2, 1, B.w + 4, L.wallH - 2, "#b9a888"); hqR(g, B.x, 2, B.w, L.wallH - 5, "#fbfbf8");
  // Names in the 8px bold on every size: at 7px a bold B reads as an 8 ("Bo" as "8o", council round 8).
  const big = L.boardFont === 8, lineFont = big ? HQ_FONT_8 : HQ_FONT_7, whoFont = HQ_FONT_8B, step = big ? 9 : 8;
  g.font = HQ_FONT_8B; g.fillStyle = "#3a3242"; g.fillText(hqFit(g, board.title, bw - 60), x0, 10);
  // The open count in plain ink; only its stuck part is red, to match the red folders. 8px: at 7px
  // an 8 read as an S ("18" as "1S", council round 9), and the earlier pixel font's 8 still did at 8px (round 10),
  // so the count is in Silkscreen, the headings' font, whose 8 is plainly an 8.
  g.font = HQ_SIGN_FONT; g.textAlign = "right";
  // Whole pixels, so a downscaled canvas keeps every column of a letter (the P of OPEN on a phone).
  if (board.stuck) { g.fillStyle = "#c0392b"; g.fillText(board.parts[1], x0 + bw, 10); g.fillStyle = "#6b6577"; g.fillText(board.parts[0], Math.round(x0 + bw - g.measureText(board.parts[1]).width), 10); }
  else { g.fillStyle = "#6b6577"; g.fillText(board.count, x0 + bw, 10); }
  g.textAlign = "left";
  const lines = board.stopped ? [{ who: "", text: "The kill switch is pulled: nobody starts work.", red: true }] : board.lines.slice(0, 2);
  lines.forEach((l, i) => {
    const y = 19 + (big ? 1 : 0) + i * step;
    if (l.amber) hqR(g, x0 - 2, y - 6, bw + 4, step, "rgba(255,179,71,.28)");
    g.font = whoFont; g.fillStyle = "#3a3242";
    const lw = l.who ? Math.ceil(g.measureText(l.who + " ").width) : 0;
    if (l.who) g.fillText(l.who + " ", x0, y);
    g.font = lineFont; g.fillStyle = l.red ? "#c0392b" : l.amber ? "#8a4f00" : "#4b4557"; g.fillText(hqFit(g, l.text, bw - lw), x0 + lw, y);
  });
  if (!board.stopped && board.lines.length > 2) { g.textAlign = "right"; g.fillStyle = "#6b6577"; g.fillText(`+${board.lines.length - 2}`, x0 + bw, 19 + (big ? 1 : 0) + step); g.textAlign = "left"; }
}
function hqFloor(g, z, W) {
  const { x, y, w, h } = z;
  g.save(); g.beginPath(); g.rect(x, y, w, h); g.clip();
  if (z.floor === "plank") {
    hqR(g, x, y, w, h, "#c99a66");
    for (let yy = y, row = 0; yy < y + h; yy += 6, row++) { hqR(g, x, yy + 5, w, 1, "#b0814f"); for (let xx = x + ((row * 23) % 48); xx < x + w; xx += 48) hqR(g, xx, yy, 1, 5, "#b0814f"); }
  } else if (z.floor === "tile") {
    hqR(g, x, y, w, h, "#8fa3bd");
    for (let yy = y; yy < y + h; yy += 12) hqR(g, x, yy, w, 1, "#a9bad0");
    for (let xx = x; xx < x + w; xx += 12) hqR(g, xx, y, 1, h, "#a9bad0");
  } else if (z.floor === "parquet") {
    hqR(g, x, y, w, h, "#9a6a42");
    for (let j = 0; j * 8 < h; j++) for (let i = 0; i * 16 < w + 16; i++) { const xx = x + i * 16 + (j % 2) * 8, yy = y + j * 8; hqR(g, xx, yy, 8, 4, "#a8774c"); hqR(g, xx + 8, yy + 4, 8, 4, "#a8774c"); }
  } else if (z.floor === "terra") {
    hqR(g, x, y, w, h, "#c77d5a");
    for (let j = 0; j * 10 < h; j++) { const yy = y + j * 10; hqR(g, x, yy, w, 1, "#b06b4a"); for (let xx = x + (j % 2) * 5; xx < x + w; xx += 10) hqR(g, xx, yy, 1, 10, "#b06b4a"); }
  } else if (z.floor === "check") {
    for (let j = 0; j * 8 < h; j++) for (let i = 0; i * 8 < w; i++) hqR(g, x + i * 8, y + j * 8, 8, 8, (i + j) % 2 ? "#3d3a44" : "#ece6da");
  }
  g.restore();
  if (z.raised) { hqR(g, x, y + h, w, 3, "#6d7f99"); hqR(g, x, y + h - 1, w, 1, "#c3d0e0"); if (x + w < W) hqR(g, x + w - 1, y, 1, h, "#c3d0e0"); }
  if (z.sunken) { hqR(g, x, y, w, 4, "rgba(70,30,15,.35)"); hqR(g, x, y + 4, w, 1, "rgba(255,230,210,.25)"); }
  if (z.recess) hqR(g, x, y, w, 8, "rgba(60,35,20,.25)");
}

// The places and what they hold from the record.
function hqDrawPlaces(g, scene) {
  const L = scene.L, snap = HQ.snap || {}, calm = hqCalm(), blink = calm || Math.floor(HQ.t * 6) % 2 === 0;
  // The library alcove: the week's shelves built into its back wall, a row of spines per subject, pale
  // when nothing checked it; a bench beneath.
  const S = L.shelf, rows = hqShelfRows();
  hqR(g, S.x - 2, S.y - 2, S.w + 4, S.rows * S.pitch + 5, "#b8905f"); hqR(g, S.x, S.y, S.w, S.rows * S.pitch, "#8a6440");
  const cols = ["#7fa6e8", "#e8b45a", "#7cc28f", "#c98fd6"];
  rows.slice(0, S.rows).forEach((r, i) => {
    const y = S.y + i * S.pitch, step = Math.max(2, Math.min(3, (S.w - 4) / r.items.length));
    hqR(g, S.x, y + S.pitch - 1, S.w, 1, "#c9a274");
    r.items.slice(0, Math.floor((S.w - 4) / step)).forEach((t, k) => {
      const x = S.x + 2 + k * step, tall = k % 5 === 2 ? 1 : 0;
      hqR(g, x, y + 2 + tall, Math.max(1, step - 1), S.pitch - 3 - tall, HQ_DONE_KINDS[t.closure.why] ? cols[i % 4] : "#efe8d6");
      if (blink && HQ.fresh[t.id] > 0) hqR(g, x, y + 1, Math.max(1, step - 1), 1, HQ_C.neon);
    });
  });
  const bn = L.bench; hqR(g, bn.x, bn.y + 8, bn.w, 4, "#8a6440"); hqR(g, bn.x, bn.y + 8, bn.w, 1, "#b8905f"); hqR(g, bn.x + 2, bn.y + 12, 2, 5, "#6d4f33"); hqR(g, bn.x + bn.w - 4, bn.y + 12, 2, 5, "#6d4f33");
  // The owner's corner: the desk with a paper for each decision waiting and the lever on it, the rack
  // with the day's spend, the review table with a folder for each item waiting for review.
  const od = L.ownerDesk; hqImg(g, "DESK_FRONT", od.x, od.y);
  // Each decision a sheet of paper, its top edge in the decision's colour (a solid block read as a cup).
  (snap.desk || []).slice(0, 5).forEach((d, i) => {
    const x = od.x + 5 + i * 7, y = od.y + 4 - (i % 2) * 2;
    hqR(g, x + 1, y + 1, 6, 8, "rgba(0,0,0,.25)"); hqR(g, x, y, 6, 8, "#fffdf5"); hqR(g, x, y, 6, 2, d.kind === "goal" ? HQ_C.violet : d.kind === "stuck" ? HQ_C.red : HQ_C.lamp);
    hqR(g, x + 1, y + 4, 4, 1, "#c9c2b4"); hqR(g, x + 1, y + 6, 3, 1, "#c9c2b4");
  });
  const lv = L.lever, up = !snap.stopped;
  hqR(g, lv.x, lv.y + 12, 8, 3, "#3a3a46"); hqR(g, lv.x + 3, up ? lv.y + 3 : lv.y + 8, 2, up ? 9 : 5, "#99aaaa"); hqR(g, lv.x + 2, up ? lv.y + 1 : lv.y + 12, 4, 3, HQ_C.red);
  const rk = L.rack, pool = snap.pool || { per_day: 1, committed: 0 }, fill = Math.min(1, (pool.committed || 0) / Math.max(1, pool.per_day || 1));
  hqR(g, rk.x, rk.y, 14, 30, "#232833"); hqR(g, rk.x + 2, rk.y + 10, 10, 18, "#333b4a");
  const ticks = (snap.counts && snap.counts.ticks) || 0;
  for (let i = 0; i < 3; i++) hqR(g, rk.x + 4, rk.y + 13 + i * 5, 2, 2, snap.stopped ? HQ_C.red : (ticks + i) % 3 === 0 ? HQ_C.green : "#24412f");
  hqR(g, rk.x + 2, rk.y + 2, 10, 6, "#e8e4da"); hqR(g, rk.x + 3, rk.y + 3, Math.max(1, Math.round(8 * fill)), 4, fill > .85 ? HQ_C.red : fill > .6 ? HQ_C.lamp : HQ_C.green);
  const V = L.review, review = ((snap.tasks) || []).filter((t) => t.status === "in_review");
  hqImg(g, "SMALL_TABLE_FRONT", V.x, V.y);
  // A red folder for each item the board counts stuck, then a blue one for each in review: the
  // board's red count has red things in the room to match.
  const goal = hqGoalNow(snap), stuck = ((snap.tasks) || []).filter((t) => goal && t.goal === goal.id && t.status === "blocked");
  stuck.map(() => HQ_C.red).concat(review.map(() => HQ_C.blue)).slice(0, 6).forEach((c, i) => { hqR(g, V.x + 4 + i * 4, V.y + 8 - (i % 3), 7, 6, c); hqR(g, V.x + 4 + i * 4, V.y + 8 - (i % 3), 7, 1, "rgba(255,255,255,.5)"); });
  for (const c of L.chairs) hqChair(g, c.x, c.y);
  // The lounge: the low bookcase (a spine for each of the team's notes and map lines) behind the sofas,
  // the chess table with the live game; the kitchenette's counter and stools; two plants.
  const bc = L.bookcase;
  hqR(g, bc.x, bc.y, bc.w, 14, "#a07850"); hqR(g, bc.x + 2, bc.y + 2, bc.w - 4, 9, "#7a5636"); hqR(g, bc.x, bc.y + 12, bc.w, 2, "#b8905f");
  const stage = { canon: "#4f8f6f", insight: "#6f9bdf", observation: "#c0b65e" }, world = snap.world || {};
  (world.sections || []).map((s) => stage[s.stage] || "#9f9fc8").concat((snap.map_lines || []).map(() => "#c98fd6")).slice(0, Math.floor((bc.w - 6) / 3))
    .forEach((c, i) => hqR(g, bc.x + 4 + i * 3, bc.y + 3 + (i % 4 ? 1 : 0), 2, 8 - (i % 4 ? 1 : 0), c));
  // A muted teal rug under the sofas and the chess table: decor, standing for nothing, and far from
  // the red kept for stuck work.
  if (L.rug) { const r = L.rug; hqR(g, r.x, r.y, r.w, r.h, "#3f5f66"); hqR(g, r.x + 2, r.y + 2, r.w - 4, r.h - 4, "#557a80"); hqR(g, r.x + 4, r.y + 4, r.w - 8, 1, "rgba(255,220,180,.25)"); hqR(g, r.x + 4, r.y + r.h - 5, r.w - 8, 1, "rgba(255,220,180,.25)"); }
  for (const s of L.sofas) hqImg(g, "SOFA_FRONT", s.x, s.y);
  const ch = L.chess, game = (((snap.games || {}).chess || {}).games || []).find((x) => x.status === "in_progress");
  hqR(g, ch.x - 1, ch.y + 16, 18, 2, "rgba(60,40,20,.2)"); hqR(g, ch.x + 6, ch.y + 16, 4, 5, "#6d5434"); hqR(g, ch.x, ch.y, 16, 16, "#e9dcc0");
  for (let j = 0; j < 8; j++) for (let i = 0; i < 8; i++) if ((i + j) % 2) hqR(g, ch.x + i * 2, ch.y + j * 2, 2, 2, "#9b7b52");
  if (game && game.rows) game.rows.forEach((row, r) => [...String(row)].forEach((c, k) => { if (c === ".") return; const pawn = c.toLowerCase() === "p"; hqR(g, ch.x + k * 2, ch.y + r * 2, pawn ? 1 : 2, pawn ? 1 : 2, c === c.toUpperCase() ? "#fffdf5" : "#1f1d29"); }));
  for (const sx of [ch.x - 12, ch.x + 20]) { hqR(g, sx, ch.y + 12, 8, 3, "#8a5a34"); hqR(g, sx + 3, ch.y + 15, 2, 5, "#6d5434"); }
  const ct = L.counter; hqR(g, ct.x, ct.y + 8, ct.w, 14, "#8a6d48"); hqR(g, ct.x, ct.y + 8, ct.w, 2, "#b8905f"); hqImg(g, "COFFEE", ct.x + ct.w - 18, ct.y - 4);
  for (const s of L.stools) { hqR(g, s.x + 3, s.y + 12, 10, 3, "#8a5a34"); hqR(g, s.x + 7, s.y + 15, 2, 6, "#6d5434"); }
  for (const [k, x, y] of L.decor) hqImg(g, k, x, y);
  // Nobody working: the work floor a step dimmer, so idle and busy differ even at a thumbnail.
  if (!scene.desks.some((d) => d.lamp)) { const Z = L.zones.work; hqR(g, Z.x, Z.y, Z.w, Z.h, "rgba(40,30,60,.24)"); }
  // The desks: the lamp's warm pool and the lit screen only while a turn runs there.
  for (const d of scene.desks) {
    // A turn waiting to retry: its patch of floor dimmed as if the lamp were off (nothing runs), edged
    // in hard amber pixels, the board's retry colour, so waiting differs from working at a glance and
    // not only by its sign (a soft amber glow vanished on the wood, council round 12).
    if (d.who && d.who.kind === "wait") {
      const px = d.x - 6, py = d.y - 8, pw = 60, ph = 50, a = "#ffaa3c", k = 6;
      hqR(g, px, py, pw, ph, "rgba(40,30,60,.22)");
      // Solid pixel corners, not a dashed box (which read as a selection).
      for (const [cx, cy, sx, sy] of [[px, py, 1, 1], [px + pw - 1, py, -1, 1], [px, py + ph - 1, 1, -1], [px + pw - 1, py + ph - 1, -1, -1]]) {
        hqR(g, sx > 0 ? cx : cx - k + 1, cy, k, 1, a); hqR(g, cx, sy > 0 ? cy : cy - k + 1, 1, k, a);
      }
    }
    if (d.lamp) {
      const glow = g.createRadialGradient(d.x + 24, d.y + 20, 2, d.x + 24, d.y + 20, 40);
      glow.addColorStop(0, "rgba(255,214,140,.60)"); glow.addColorStop(1, "rgba(255,214,140,0)");
      g.fillStyle = glow; g.fillRect(d.x - 18, d.y - 18, 84, 80);
    }
    hqImg(g, "DESK_FRONT", d.x, d.y);
    hqImg(g, d.screen ? "PC_FRONT_ON_" + (calm ? 1 : 1 + (Math.floor(HQ.t * 4) % 3)) : "PC_FRONT_OFF", d.x + 16, d.y - 6);
    hqR(g, d.x + 4, d.y + 2, 3, 6, "#6d5434"); hqR(g, d.x + 2, d.y, 7, 3, d.lamp ? "#ffd27a" : "#8a7a60");
    if (d.who && d.who.kind === "wait") hqR(g, d.x + 19, d.y - 3, 10, 7, "rgba(255,179,71,.45)");
    if (!d.who) hqImg(g, "CUSHIONED_CHAIR_BACK", d.x + 16, d.y + 24);
  }
  const sel = HQ.selected;
  if (sel && sel.type === "corner") hqOutline(g, L.zones.owner);
  if (sel && sel.type === "shelf") hqOutline(g, { x: S.x - 2, y: S.y - 2, w: S.w + 4, h: S.rows * S.pitch + 5 });
  if (sel && sel.type === "knowing") hqOutline(g, { x: bc.x, y: bc.y, w: bc.w, h: 14 });
  if (sel && sel.type === "chess") hqOutline(g, { x: ch.x - 1, y: ch.y - 1, w: 18, h: 18 });
}
function hqChair(g, x, y) { hqR(g, x + 2, y + 12, 12, 4, "#5c7a5a"); hqR(g, x + 2, y + 6, 12, 7, "#6f9070"); hqR(g, x + 3, y + 16, 2, 4, "#3e4a3c"); hqR(g, x + 11, y + 16, 2, 4, "#3e4a3c"); }
function hqOutline(g, r) { hqR(g, r.x - 1, r.y - 1, r.w + 2, 1, HQ_C.neon); hqR(g, r.x - 1, r.y + r.h, r.w + 2, 1, HQ_C.neon); hqR(g, r.x - 1, r.y - 1, 1, r.h + 2, HQ_C.neon); hqR(g, r.x + r.w, r.y - 1, 1, r.h + 2, HQ_C.neon); }

// The people, seated: at a desk with their back to us and the chair's back over their legs, typing
// while their turn runs; on a sofa into its seat; at the chess board; at the review table.
function hqDrawPeople(g, scene) {
  const calm = hqCalm(), people = scene.people.slice().sort((a, b) => a.y - b.y);
  for (const p of people) {
    const img = HQ_IMG["char" + ((p.m.look || 0) % 6)];
    const row = p.dir === "up" ? 1 : p.dir === "down" ? 0 : 2;
    const frame = p.kind === "work" ? 3 + (calm ? 0 : Math.floor(HQ.t * 6) % 2) : p.seat === "desk" ? 1 : 0;
    // Seen from behind at a desk, a dark outline keeps pale hair from melting into the monitor: the
    // sprite's silhouette, made once and kept, drawn a pixel to each side under it.
    const sil = p.seat === "desk" ? hqSilhouette(img) : null;
    if (sil) for (const [dx, dy] of [[-1, 0], [1, 0], [0, -1]]) g.drawImage(sil, frame * 16, row * 32, 16, 32, p.x + dx, p.y + dy, 16, 32);
    if (img) {
      if (p.dir === "left") { g.save(); g.translate(p.x + 16, 0); g.scale(-1, 1); g.drawImage(img, frame * 16, row * 32, 16, 32, 0, p.y, 16, 32); g.restore(); }
      else g.drawImage(img, frame * 16, row * 32, 16, 32, p.x, p.y, 16, 32);
    }
    if (p.seat === "desk") hqImg(g, "CUSHIONED_CHAIR_BACK", p.x, p.y + 12);
    if (p.seat === "sofa") { const s = scene.L.sofas.find((x) => p.x >= x.x && p.x < x.x + 32); if (s) { hqR(g, p.x, s.y + 10, 16, 6, "#b23a5a"); hqR(g, p.x, s.y + 10, 16, 1, "#d4607f"); } }
    if (HQ.selected && HQ.selected.type === "member" && HQ.selected.id === p.m.id) hqOutline(g, { x: p.x, y: p.y, w: 16, h: 32 });
  }
  // Signs beside the head, only where something needs attention.
  for (const p of people) {
    const sign = p.kind === "wait" ? "wait" : (() => { const b = hqTrouble(p.m.id); return b ? b.kind : ""; })();
    // The hourglass sits right of the monitor, never on it, so it reads as a sign and not a screen.
    if (sign) hqSign(g, sign, p.x + (sign === "wait" ? 18 : 13), p.y + (sign === "wait" ? 6 : 1));
  }
  for (const p of people) if (p.bubble) {
    const b = p.bubble;
    hqR(g, b.x, b.y, b.w, b.h, "#ffffff"); hqR(g, b.x, b.y + b.h, b.w, 1, "#3a3242");
    // Its tail points at the one who said it: up from below, down from above, sideways beside the head.
    const tx = Math.max(b.x + 2, Math.min(b.x + b.w - 4, p.x + 7));
    if (b.y > p.y + 20) hqR(g, tx, b.y - 2, 2, 2, "#ffffff");
    else if (b.y + b.h < p.y) hqR(g, tx, b.y + b.h, 2, 3, "#3a3242");
    else hqR(g, b.x > p.x ? b.x - 2 : b.x + b.w, b.y + 3, 2, 2, "#ffffff");
    g.font = HQ_FONT_6; g.fillStyle = "#3a3242"; g.fillText(hqFit(g, b.text, b.w - 6), b.x + 3, b.y + 7);
  }
}
const HQ_SILHOUETTES = new Map();
function hqSilhouette(img) {
  if (!img) return null;
  if (HQ_SILHOUETTES.has(img)) return HQ_SILHOUETTES.get(img);
  let made = null;
  try {
    const c = document.createElement("canvas"), x = c.getContext && c.getContext("2d");
    if (x && img.width) {
      c.width = img.width; c.height = img.height; x.drawImage(img, 0, 0);
      x.globalCompositeOperation = "source-in"; x.fillStyle = "rgba(20,16,28,.6)"; x.fillRect(0, 0, c.width, c.height); made = c;
    }
  } catch (_) { /* no outline, the sprite still draws */ }
  if (made || img.complete) HQ_SILHOUETTES.set(img, made);
  return made;
}
// A sign seven pixels square: a red "!" stuck, a red cross a failed check, a green tick recovered, an
// amber hourglass waiting to retry.
function hqSign(g, kind, x, y) {
  hqR(g, x + 1, y + 1, 7, 7, "rgba(0,0,0,.25)");
  if (kind === "blocked") { hqR(g, x, y, 7, 7, HQ_C.red); hqR(g, x + 3, y + 1, 1, 3, "#fff"); hqR(g, x + 3, y + 5, 1, 1, "#fff"); return; }
  if (kind === "check") { hqR(g, x, y, 7, 7, "#2b2f3a"); for (let i = 0; i < 5; i++) { hqR(g, x + 1 + i, y + 1 + i, 1, 1, HQ_C.red); hqR(g, x + 5 - i, y + 1 + i, 1, 1, HQ_C.red); } return; }
  // Waiting to retry is the one state with no lamp to tell it, so its sign is twice the size.
  if (kind === "wait") { hqR(g, x + 1, y - 3, 12, 14, "rgba(0,0,0,.25)"); hqR(g, x - 1, y - 5, 13, 15, "#5a3408"); hqR(g, x, y - 4, 11, 13, HQ_C.lamp); hqHourglass(g, x + 2, y - 2, 1); return; }
  hqR(g, x, y, 7, 7, HQ_C.green);
  for (const [dx, dy] of [[1, 3], [2, 4], [3, 3], [4, 2], [5, 1]]) hqR(g, x + dx, y + dy, 1, 1, "#fff");
}

// An hourglass seven pixels wide and nine tall: frame, sand above draining to sand below.
function hqHourglass(g, x, y, s) {
  const r = (dx, dy, w, h, c) => hqR(g, x + dx * s, y + dy * s, w * s, h * s, c);
  r(0, 0, 7, 1, "#5a3408"); r(0, 8, 7, 1, "#5a3408");
  r(1, 1, 5, 1, "#fff6e0"); r(2, 2, 3, 1, "#fff6e0"); r(3, 3, 1, 2, "#fff6e0"); r(2, 5, 3, 1, "#e0a040"); r(1, 6, 5, 2, "#e0a040");
  r(1, 2, 1, 1, "#5a3408"); r(5, 2, 1, 1, "#5a3408"); r(1, 6, 1, 0, "#5a3408");
}

// ---- taps ---------------------------------------------------------------------------------------

function hqStudioAt(scene, x, y) {
  const inside = (r) => x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h;
  return scene.taps.find(inside) || null;
}
function hqTap(ev) {
  const cv = document.getElementById("hq-office"); if (!cv) return;
  const scene = HQ.scene || hqStudioScene(), b = cv.getBoundingClientRect();
  const hit = hqStudioAt(scene, (ev.clientX - b.left) * (scene.w / b.width), (ev.clientY - b.top) * (scene.h / b.height));
  if (hit && hit.act) { dispatch(hit.act); return; }
  if (hit) { hqSelect(hit.sel); return; }
  HQ.selected = null; render();
}

// ---- what a tap opens ---------------------------------------------------------------------------

// The week's shelves, by subject: the subjects closed most recently get the rows, the rest one row.
function hqShelfRows(now = hqWeekNow()) {
  const by = new Map();
  for (const t of hqWeekDone(now)) {
    const name = hqSubjectOf(t) || "Other work";
    if (!by.has(name)) by.set(name, { name, items: [], last: 0 });
    const r = by.get(name); r.items.push(t); r.last = Math.max(r.last, t.closure.at);
  }
  const rows = [...by.values()].sort((a, b) => b.last - a.last), most = 4;
  if (rows.length <= most) return rows;
  const rest = rows.slice(most - 1);
  return rows.slice(0, most - 1).concat({ name: `${rest.length} other subjects`, items: rest.flatMap((r) => r.items), more: true });
}
function hqShelfPanel() {
  const all = hqWeekDone().reverse(), checked = all.filter((t) => HQ_DONE_KINDS[t.closure.why]).length;
  const rows = all.map((t) => hqLine(`hq:pick:task:${Number(t.id)}`, `${hqKey(t.id)} ${esc(t.title)}`,
    `${HQ_DONE_KINDS[t.closure.why] ? "Met its criteria" : "Completed · Not verified"} · ${esc(hqSubjectOf(t) || "no project")} · ${esc(new Date(t.closure.at).toLocaleDateString([], { weekday: "short" }))} ${esc(hqClock(t.closure.at))}`));
  const list = hqMore("shelf", rows, 8);
  return `<div class="hq-sel">Done this week</div>
    <p class="hq-who">${all.length ? `${checked} met ${checked === 1 ? "its" : "their"} criteria · ${all.length - checked} completed, not verified` : "Nothing finished in the last 7 days."}</p>
    ${rows.length ? `<ul class="hq-lines">${list.shown}</ul>${list.more}` : ""}
    <p class="hq-note">A coloured spine on the alcove's shelves is an item that met its criteria; a pale one was closed with nothing checking its evidence. Each row is a subject.</p>`;
}
function hqCornerPanel() {
  const s = HQ.snap || {}, review = (s.tasks || []).filter((t) => t.status === "in_review"), pool = s.pool || {};
  const items = review.map((t) => hqLine(`hq:pick:task:${Number(t.id)}`, `${hqKey(t.id)} ${esc(t.title)}`, t.owner ? esc(hqName(t.owner)) : "waiting"));
  return `<div class="hq-sel">Your corner</div><dl class="hq-kv">
    <dt>On your desk</dt><dd>${(s.desk || []).length ? `${s.desk.length} decision${s.desk.length === 1 ? "" : "s"} waiting; they are at the top of Work` : "Nothing waiting"}</dd>
    <dt>Kill switch</dt><dd>${s.stopped ? "Pulled: nobody starts work" : "Up: the team may work"}</dd>
    <dt>Today</dt><dd>${hqDollars(pool.committed)} of ${hqDollars(pool.per_day)}</dd></dl>
    ${items.length ? `<p class="hq-who">Waiting for review</p><ul class="hq-lines">${items.join("")}</ul>` : `<p class="hq-empty">Nothing waits for review.</p>`}`;
}
function hqKnowingPanel() {
  const s = HQ.snap || {}, w = s.world || {};
  return `<div class="hq-sel">What we know</div>
    <p class="hq-who">${(w.sections || []).length} notes in the team's world and ${(s.map_lines || []).length} lines on its map; a spine each on the lounge's bookcase.</p>
    <div class="hq-row"><button class="hq-btn" data-act="hq:place:manage:go">Open what the team knows</button></div>`;
}
// The board, tapped: the goal, and every turn running now with its whole title.
function hqGoalPanel() {
  const s = HQ.snap || {}, g = hqGoalNow(s);
  const turns = (s.members || []).filter((m) => !m.furloughed).flatMap((m) => (m.running || []).map((r) => ({ m, t: r.task ? hqTask(r.task) : null })));
  const lines = turns.map(({ m, t }) => t ? hqLine(`hq:pick:task:${Number(t.id)}`, `${esc(m.name)}: ${hqKey(t.id)} ${esc(t.title)}`, esc(hqSubjectOf(t) || "no project")) : `<li>${esc(m.name)}: looking round</li>`);
  return `<div class="hq-sel">${esc(g ? g.title : "No goal yet")}</div>
    <p class="hq-who">${esc(hqBoard(s).count)}${g && g.status === "active" ? ". Being worked towards; it is reached only when it is marked done." : ""}</p>
    ${lines.length ? `<ul class="hq-lines">${lines.join("")}</ul>` : `<p class="hq-empty">Nobody is working now.</p>`}`;
}
