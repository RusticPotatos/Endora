// ---- Endora HQ (ADR 0102, spec section 13) ---------------------------------------------------
// The team at work in a small studio, drawn from HQ's snapshot and the events after it (the studio
// itself is in 47f-hq-studio.js). The
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
  said: {},            // member id → { text, t }: a real post just made, said for t seconds
  roster: {},          // member id → its controls are open on the Team card
  deskNote: {},
  more: {},            // list key → the owner asked to see all of it        // desk item id → why the owner's last decision on it was refused
  feed: null,          // the paged activity, full screen: { at, starts, lines, next, busy, error }
  place: null,         // where HQ is: the map (where it opens), a secondary place, or "detail"
  from: "map",         // the place Looking at was opened from, for its way back
  working: false,      // the "Give work" form is open
  workDraft: {},       // what is typed in it, kept across live updates
  logOpen: {},         // event id → a long line opened to its whole
  back: null,          // where Activity was when one of its links was taken: { y, feed }
  allTurns: {},        // "task-N" / "member-N" → every turn shown, not only the last five
  whole: {},           // task id → a finished task opened whole (the snapshot has it in brief), or "asking"
  scene: null,         // the studio as last drawn: where everything stands, for taps
  t: 0,                // seconds of animation
  polling: null,
  looping: false,
  mac: null,           // the owner's Mac as the node knows it (ADR 0130): state, switch, settings
  macToken: "",        // a helper token the owner just made, shown once and then forgotten
  macTick: 0,          // polls since the Mac was last read
  landing: {},         // task id → { owner, unverified }: closed live, waiting for the snapshot to say how
  fresh: {},           // task id → seconds its spine still shines on the week's shelf, after it closed live
  weekNow: 0,          // the time the week's shelf counts back from; 0 is now (a check fixes it)
  recovered: {},       // member id → { task, why, t }: a recovery the record just showed, ticked for t seconds
};

const HQ_C = { neon: "#ff4fa3", lamp: "#ffb347", red: "#ef5a5f", green: "#45c486", blue: "#7fb2ff", violet: "#b98cf0", text: "#1f1d29" };
// The furniture the studio draws (Pixel Agents, MIT): desks and their screens, chairs, the sofas,
// the review table, the coffee machine, and decor that stands for nothing (plants, a cactus, a bin).
const HQ_ART = ["DESK_FRONT", "PC_FRONT_OFF", "PC_FRONT_ON_1", "PC_FRONT_ON_2", "PC_FRONT_ON_3", "CUSHIONED_CHAIR_BACK",
  "SOFA_FRONT", "PLANT", "PLANT_2", "LARGE_PLANT", "HANGING_PLANT", "CACTUS", "BIN", "COFFEE", "SMALL_TABLE_FRONT"];
const HQ_IMG = {};
let HQ_ART_READY = null;

function hqLoadArt() {
  if (HQ_ART_READY) return HQ_ART_READY;
  const jobs = [];
  const one = (key, src) => jobs.push(new Promise((ok) => {
    const i = new Image(); i.onload = () => { HQ_IMG[key] = i; ok(); }; i.onerror = () => ok(); i.src = "/hq-art/" + src;
  }));
  for (let i = 0; i < 6; i++) one("char" + i, `characters/char_${i}.png`);
  for (const f of HQ_ART) one(f, `furniture/${f}.png`);
  HQ_ART_READY = Promise.all(jobs);
  return HQ_ART_READY;
}

// ---- loading and the stream ------------------------------------------------------------

async function loadHQ() {
  try {
    const snap = await api("GET", "/v1/hq");
    const first = !HQ.snap;
    HQ.snap = snap; HQ.error = "";
    await hqLoadMac();
    await hqLoadCredits();
    if (first) {
      // The recent past, in words, without replaying it on screen: the Activity tab's twenty.
      try { HQ.log = (await hqLogPage((snap.last_event || 0) + 1, HQ_LOG_SHOWN)).lines; }
      catch (_) { /* the log starts empty */ }
      HQ.last = snap.last_event || 0;
    }
    hqLandings();
    // The map is HQ's first screen (M6): read with the snapshot, never after it.
    await loadAgents();
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
    // The agents map reads with HQ's poll too (M5); any other screen stops it.
    if (NAV.v !== "hq") { clearInterval(HQ.polling); HQ.polling = null; return; }
    // The Mac's state changes with the owner's keyboard, not with HQ's events: read it every
    // twelve seconds, and redraw only when it moved.
    HQ.macTick += 1;
    // A running turn that falls silent stops moving without any new event: read the map again.
    if (hqPlace() === "map" && HQ.macTick % 10 === 0) { await loadAgents(); if (NAV.v === "hq" && hqPlace() === "map" && !hqTyping()) render(); }
    if (HQ.macTick % 4 === 0) {
      const before = JSON.stringify([HQ.mac, HQ.credits]);
      await hqLoadMac();
      // The balances move with each paid call; DeepSeek's is read afresh every ten minutes.
      if (HQ.macTick % 20 === 0) await hqLoadCredits();
      if (JSON.stringify([HQ.mac, HQ.credits]) !== before && NAV.v === "hq" && !hqTyping()) render();
    }
    // A game being watched moves on its own, every six seconds.
    if (HQ.macTick % 2 === 0) {
      try { if (await hqHoloRefresh() && NAV.v === "hq" && !hqTyping()) render(); } catch (_) { /* the next poll tries again */ }
    }
    try {
      const got = await api("GET", `/v1/hq/events?after=${HQ.last}`);
      const events = got.events || [];
      if (!events.length) return;
      for (const e of events) { hqTell(e, true); HQ.last = Math.max(HQ.last, e.id); }
      HQ.snap = await api("GET", "/v1/hq");
      hqLandings();
      if (hqPlace() === "map") await loadAgents();
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
function hqTask(id) {
  const t = ((HQ.snap && HQ.snap.tasks) || []).find((x) => x.id === id), whole = HQ.whole[id];
  return t && whole && whole !== "asking" ? Object.assign({}, t, whole, { status: t.status }) : t;
}
// A finished task comes in brief in the snapshot; opened, it is asked of HQ whole, once.
async function hqAskWhole(id) {
  if (HQ.whole[id]) return;
  HQ.whole[id] = "asking";
  try { HQ.whole[id] = await api("GET", `/v1/hq/tasks/${Number(id)}`); }
  catch (e) { delete HQ.whole[id]; flash("Couldn't open the task: " + e.message, "err"); return; }
  render();
}
function hqName(id) { const m = hqMember(id); return m ? m.name : "someone"; }

// A task's key, as the office writes it: EL-7.
function hqKey(id) { return id ? "EL-" + id : ""; }

// A long list shows its first few, with a way to see the rest (the owner, 2026-10-25: sections
// too long to read). `key` keeps what the owner opened across HQ's redraws; it has no colon.
function hqMore(key, items, n) {
  if (items.length <= n) return { shown: items.join(""), more: "" };
  const all = !!HQ.more[key];
  return {
    shown: (all ? items : items.slice(0, n)).join(""),
    more: `<div class="hq-row"><button class="hq-btn hq-more" aria-expanded="${all}" data-act="hq:more:${key}:toggle">${all ? "Show fewer" : `Show all ${items.length}`}</button></div>`,
  };
}

// Long words show three lines, with "Read all" to open them in place (the owner, 2026-10-25:
// hard to read when they run so long). `text` is plain; it is escaped here.
function hqClamp(key, text, cls = "") {
  const long = String(text || "").length > 240;
  if (!long) return `<p${cls ? ` class="${cls}"` : ""}>${esc(text)}</p>`;
  const open = !!HQ.more[`clamp${key}`];
  return `<p class="${cls ? cls + " " : ""}hq-clamp${open ? " open" : ""}">${esc(text)}</p>
    <button class="hq-link hq-read" aria-expanded="${open}" data-act="hq:more:clamp${key}:toggle">${open ? "Show less" : "Read all"}</button>`;
}

// A row that is one line until tapped, then shows `body` (its details and buttons) under it
// (the owner, 2026-10-25: Manage's projects and routines). `title` and `meta` are escaped
// markup; `key` has no colon.
function hqOpenRow(key, title, meta, body) {
  const open = !!HQ.more[key];
  return `<li class="hq-open-row${open ? " open" : ""}"><button class="hq-line" aria-expanded="${open}" data-act="hq:more:${key}:toggle"><b>${title}</b> <small>${meta}</small></button>${open ? `<div class="hq-row-body">${body}</div>` : ""}</li>`;
}

// A big card or a rare part of a tab, folded to one line until tapped (the density audit of
// 2026-10-25): open, `body` (whole cards) follows the line. What is opened is kept across HQ's
// redraws in `HQ.more`; `key` has no colon.
function hqFold(key, title, meta, body) {
  const open = !!HQ.more[key];
  // One grid item, heading and body together: apart, an opened body landed in the other column
  // (tabs climb, round 0).
  // Open, a body with a heading carries the fold control in it, so the title shows once and no
  // second card is spent on the fold (tabs climb, round 2).
  const fold = `<button class="hq-link hq-fold-x" aria-expanded="true" data-act="hq:more:${key}:toggle">fold</button>`;
  if (open && /<\/h2>/.test(body)) return `<div class="hq-foldset">${body.replace(/<\/h2>/, `</h2>${fold}`)}</div>`;
  return `<div class="hq-foldset"><section class="hq-card hq-fold"><button class="hq-line" aria-expanded="${open}" data-act="hq:more:${key}:toggle"><b>${title}</b> <small>${open ? "tap to fold" : meta}</small></button></section>${open ? body : ""}</div>`;
}

// A form inside a card that is used now and then (a new project, a routine, a hire): a button
// until it is wanted.
function hqFormFold(key, label, body) {
  const open = !!HQ.more[key];
  return `<div class="hq-row"><button class="hq-btn" aria-expanded="${open}" data-act="hq:more:${key}:toggle">${open ? "Close" : label}</button></div>${open ? body : ""}`;
}

// The work in flight (Work tab): one line for each task that is not finished, newest first,
// each opening the task and its thread under Looking at; and finding any task, finished too.
function hqInFlight(s) {
  const goals = s.goals || [];
  const dropped = new Set(goals.filter((g) => g.status === "dropped").map((g) => g.id));
  const rank = { blocked: 0, doing: 1, in_review: 2, todo: 3 };
  const open = (s.tasks || []).filter((t) => t.status in rank && !dropped.has(t.goal)).reverse()
    .sort((a, b) => rank[a.status] - rank[b.status]);
  const rows = open.map((t) => hqLine(`hq:pick:task:${Number(t.id)}`, `${hqKey(t.id)} ${esc(t.title)}`,
    `${esc(String(t.status).replace(/_/g, " "))}${t.owner ? ` · ${esc(hqName(t.owner))}` : ""}`));
  const list = hqMore("inflight", rows, 5);
  return `<section class="hq-card"><h2>Open tasks <span class="hq-count">${open.length}</span></h2>
    ${rows.length ? `<ul class="hq-lines hq-open">${list.shown}</ul>${list.more}` : `<p class="hq-empty">Nothing open.</p>`}
    <div class="hq-row"><input id="hq-find" placeholder="EL-687, or a few words of its title" aria-label="A task's number or words of its title" />
    <button class="hq-btn" data-act="hq:find:task:go">Open it</button></div></section>`;
}

// The money (Manage tab): today's pool with its top-up, which figure today's base is and why
// (sized from the Claude credit, or the floor: ADR 0113, amended 2026-10-31), and what is left
// with each paid provider; the credits and their thresholds are set up in Settings → AI.
//
// What each model cost (the owner, 2026-10-31) folds under the pool, with the way to AI's
// credits: the Manage tab is at its budget of rows, so the top-up sits at the end of the pool's
// line rather than on a row of its own, and the fold's one line takes that row's place.
function hqMoneyCard(s) {
  const pool = s.pool || {}, credits = hqCreditChips();
  const left = pool.per_day == null ? null : Math.max(0, pool.per_day - (pool.committed || 0));
  const topUp = s.scorecard && s.scorecard.pool ? `<button class="hq-btn" data-act="hq:pool:topup:go">Top up today</button>` : "";
  if (left == null && !credits.length && !topUp) return "";
  const toAi = `<button class="hq-link" data-act="go:models">Credits and thresholds in AI</button>`;
  const byModel = hqByModel(s.scorecard && s.scorecard.by_model, toAi);
  const line = left == null ? (topUp ? `<p class="hq-who">${topUp}</p>` : "")
    : `<p class="hq-who">Today's pool: ${hqDollars(pool.spent)} spent · ${hqDollars(left)} left of ${hqDollars(pool.per_day)}${pool.sized && pool.sized.why ? `<br><span class="hq-note">${esc(pool.sized.why)}</span>` : ""} ${topUp}</p>`;
  return `<section class="hq-card"><h2>Money</h2>
    ${line}
    ${credits.length ? `<div class="hq-chips">${credits.join("")}</div>` : ""}
    ${byModel || `<div class="hq-row">${toAi}</div>`}</section>`;
}

// What each model cost (the owner, 2026-10-31), as HQ's scorecard computed it: a row per brain a
// turn ran on, the most spent first, with the owner's last 7 days, the 7 before and which way it
// went. Each week is one cell, its spend over its turns, cost a turn and share of that week's
// spend, so the table is four columns and wraps on a phone. One line until tapped; `after` is
// markup to end it with once open. "" without a table.
function hqByModel(t, after) {
  if (!t || !Array.isArray(t.rows)) return "";
  const perTurn = (m) => (m === 0 ? "$0" : "$" + (m / 1e6).toFixed(3));
  const spent = (m) => (m ? hqDollars(m) : "$0");
  const week = (w) => {
    if (!w || !w.turns) return `<td class="n"><small>no turns</small></td>`;
    return `<td class="n">${spent(w.spent)}<small>${w.turns}&nbsp;${w.turns === 1 ? "turn" : "turns"} · ${w.per_turn == null ? "—" : perTurn(w.per_turn)}&nbsp;a&nbsp;turn${w.spent ? ` · ${Number(w.share)}%` : ""}</small></td>`;
  };
  const marks = { up: "↑", down: "↓", same: "=" };
  const rows = t.rows.map((r) => `<tr><th scope="row">${esc(hqBrainTag(r.brain))}</th>${week(r.now)}${week(r.before)}
    <td title="${r.trend === "same" ? "the same as" : esc(String(r.trend))} on the week before">${marks[r.trend] || ""}</td></tr>`).join("");
  const body = `${rows ? `<table class="hq-board hq-cost"><thead><tr><th scope="col">Model</th><th scope="col" class="n">7 days</th><th scope="col" class="n">7 before</th><th scope="col" aria-label="Trend"></th></tr></thead>
    <tbody>${rows}</tbody></table>` : `<p class="hq-empty">No turns in the last two weeks.</p>`}
    <div class="hq-row">${after}</div>`;
  return `<ul class="hq-lines hq-by-model">${hqOpenRow("bymodel", "By model", `${spent((t.now || {}).spent)} in 7 days · ${spent((t.before || {}).spent)} the 7 before`, body)}</ul>`;
}

// What went wrong lately (Health tab): the team's recent turns that did not succeed, each
// opening its replay.
function hqFailures() {
  const failed = (r) => r.status && r.status !== "succeeded" && r.status !== "running" && r.status !== "queued";
  const turns = hqTurns(failed, "failed");
  return `<section class="hq-card"><h2>What failed</h2>${turns || `<p class="hq-empty">No recent turn failed.</p>`}
    <div class="hq-row"><button class="hq-link" data-act="go:lab">The home lab's containers and logs</button></div></section>`;
}

// A table of numbers by day and by week, as one-line rows (the owner, 2026-10-25: too long to
// read on a phone): the measure and the first words of its first window; tapped, both in full.
// `rows` is [label, first, second]; every word is escaped here.
function hqScoreRows(key, rows, first, second) {
  const head = (text) => esc(String(text).split(" · ")[0]);
  return `<ul class="hq-lines">${rows.map(([label, a, b], i) => hqOpenRow(`${key}${i}`, esc(label), head(a),
    `<span>${first}: ${esc(a)}</span>${b == null ? "" : `<span>${second}: ${esc(b)}</span>`}`)).join("")}</ul>`;
}

// One row of a list on one line: what it is and a little about it, and the whole row opens it
// (the owner, 2026-10-25: a button under every row made lists too long on a phone). `title`
// and `meta` are markup the caller has escaped.
function hqLine(act, title, meta) { return `<li><button class="hq-line" data-act="${act}"><b>${title}</b> <small>${meta}</small></button></li>`; }
// A task's number as a link that opens it under "Looking at", wherever the number is shown.
function hqKeyLink(id) {
  return id ? `<button class="hq-link" data-act="hq:pick:task:${Number(id)}">${hqKey(id)}</button>` : "";
}
// The task asked for by its number ("EL-687", "687") or by words of its title, open ones and
// the newest first; null when nothing matches.
function hqFindTask(q) {
  const asked = String(q || "").trim().toLowerCase();
  if (!asked) return null;
  const tasks = (HQ.snap && HQ.snap.tasks) || [];
  const number = asked.match(/^(?:el-?)?(\d+)$/);
  if (number) return tasks.some((t) => t.id === Number(number[1])) ? Number(number[1]) : null;
  const words = asked.split(/\s+/);
  const found = tasks
    .filter((t) => words.every((w) => String(t.title || "").toLowerCase().includes(w)))
    .sort((a, b) => (a.status === "done" || a.status === "dropped") - (b.status === "done" || b.status === "dropped") || b.id - a.id);
  return found.length ? found[0].id : null;
}
// Money in dollars, from the ledger's micro-dollars: $0.40.
function hqDollars(micros) { return "$" + ((micros || 0) / 1e6).toFixed(2); }
// What was really spent apart from what is only held for turns: a free local turn holds money
// while it runs and spends none.
function hqMoney(m, perDay) { return `${hqDollars(m.spent)} spent · ${hqDollars(m.held)} held ${perDay}`; }
// What a member spends: a member is its model (ADR 0142), and the house's models cost nothing.
function hqMemberMoney(m) {
  return m.brain === "mac" ? "the owner's Mac: costs nothing, runs only while the Mac is free" : m.brain === "local" ? "the house's model: costs nothing" : hqMoney(m, `of ${hqDollars(m.per_day)} a day`);
}

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
    .map((c) => `<li>${esc(stage[c.stage] || c.stage)}, ${esc(hqName(c.member))}${c.needs_recheck ? " · needs re-check: the criteria changed since" : ""}<ul>${refs(c)}</ul></li>`).join("");
  const items = crit.length
    ? crit.map((c, i) => `<li><b>${esc(c.text)}</b>${c.live ? " · live: a reading the same turn" : ""}${c.after ? ` · shown ${Number(c.after)} min after the last change` : ""}<ul>${rows(i + 1) || `<li class="hq-empty">No evidence yet.</li>`}</ul></li>`).join("")
    : `<li><b>The work</b><ul>${rows(0)}</ul></li>`;
  const tested = cited.filter((c) => c.stage === "tested");
  const passed = !tested.length ? "Tester's pass: not yet."
    : tested[tested.length - 1].needs_recheck ? "Tester's pass: needs re-check (the criteria changed since); it does not count." : "Tester's pass: given.";
  return `<div class="hq-evidence"><h3>What done means</h3><ul>${items}</ul><p class="hq-who">${passed}</p></div>`;
}

// The turns that ended last on a task, or of a member, with what each came to (ADR 0114): in
// reading order, oldest first like the thread below them, the last five unless the owner asks
// for all. `key` names the card, so "show all" is remembered for it alone.
// How a turn ended, then what it left (its label), in plain words; "" for a plain finish.
const HQ_TURN_ENDED = { failed: "failed", timed_out: "ran out of time", timeout: "ran out of time", cancelled: "was stopped", retry_waiting: "waits to retry", succeeded: "" };
const HQ_TURN_LEFT = { empty: "gave nothing back", advanced: "made progress", nothing: "showed nothing", checked: "checked", talk_only: "talk only" };
function hqTurnSaid(status, label) {
  const words = (map, v) => (v in map ? map[v] : String(v || "").replace(/_/g, " "));
  return [status ? words(HQ_TURN_ENDED, status) : "", label ? words(HQ_TURN_LEFT, label) : ""].filter(Boolean).join(", ");
}
function hqTurns(match, key) {
  const all = ((HQ.snap && HQ.snap.recent_runs) || []).filter(match).reverse();
  if (!all.length) return "";
  const every = !!HQ.allTurns[key], runs = every ? all : all.slice(-5);
  const toggle = all.length > 5
    ? `<div class="hq-row"><button class="hq-btn" aria-expanded="${every ? "true" : "false"}" data-act="hq:turns:${key}:toggle">${every ? "Show the last 5" : `Show all ${all.length}`}</button></div>` : "";
  // On a task, each turn also opens what it was given (the owner's notes of 31 Oct).
  const given = (r) => key.startsWith("task-") ? ` · ${replayGivenLink(r.id)}` : "";
  // The task leads and the outcome is in plain words; the turn's number is the link's last word,
  // and Show all sits under the list (tabs climb, round 1).
  // Each row leads with its task's title, the member, outcome and turn number muted under it; the
  // same task and outcome turn after turn is one row "×N", its link the latest turn (round 6).
  const rows = [];
  for (const r of runs) {
    const last = rows[rows.length - 1], said = hqTurnSaid(r.status, r.label);
    if (!key.startsWith("task-") && last && last.r.task && last.r.task === r.task && last.said === said && last.r.member === r.member) { last.n += 1; last.r = r; continue; }
    rows.push({ r, said, n: 1 });
  }
  const title = (r) => { const t = r.task && ((HQ.snap && HQ.snap.tasks) || []).find((x) => x.id === r.task); return r.task ? `${hqKey(r.task)}${t && t.title ? " " + esc(t.title) : ""}` : "No task"; };
  return `<ul class="hq-thread">${rows.map(({ r, said, n }) => `<li>${replayRunLink(r.id, `<b class="hq-turn-title">${title(r)}</b> <small>${esc(hqName(r.member))} <span class="hq-turn-id">R${r.id}</span> ${esc(said)}${n > 1 ? ` · ×${n}` : ""}</small>`)}${given(r)}</li>`).join("")}</ul>${toggle}`;
}

// A line of the activity log in full: a post is read whole from its task's thread when the
// screen has it, and is otherwise its preview, which says "…" where it was cut. A finished
// task's thread is there only once the task is opened, each post with its place (`n`).
const HQ_LONG_LINE = 240;
function hqLogText(l) {
  const w = l.whole, t = w && hqTask(w.task), p = t && (t.thread || []).find((q, i) => (q.n ?? i) === w.post);
  return p && p.author === w.author ? `${w.author}: ${p.text}` : l.text;
}
// One line as a list item. A long one is shown to its first 240 characters, with "more" to
// open it in place and "less" to fold it again; nothing is lost either way. A post HQ cut in
// its event has "more" too, which asks HQ for the post whole.
// The engine's own posts ("endora: …") are its records: shown to a short first line, the rest a
// "more" away; and one that repeats the line beside it (the same minute, the same branch) is not
// told twice (tabs climb, round 3). Nothing is reworded, only shortened or left out.
const HQ_ENGINE_LINE = 90;
const hqEnginePost = (t) => /^endora: /i.test(String(t || ""));
function hqLogLines(lines) {
  const branch = (t) => (String(t).match(/\bhq\/task-\d+[\w-]*/) || [""])[0];
  // The same action too (its verb: "pushed", "submitted"), not only the same branch and minute,
  // so a different record is never hidden (round 6).
  const verb = (t) => (String(t).replace(/^endora: /i, "").match(/^\w+/) || [""])[0].toLowerCase();
  const repeats = (l, i) => hqEnginePost(l.text) && branch(l.text) && verb(l.text) && [lines[i - 1], lines[i + 1]].some((n) => n && !hqEnginePost(n.text) && n.at === l.at && branch(n.text) === branch(l.text) && new RegExp(`\\b${verb(l.text)}\\b`, "i").test(n.text));
  // The same words in the same minute, line after line, are one line ending "×N" (round 4).
  const kept = [];
  for (const [i, l] of lines.entries()) {
    if (repeats(l, i)) continue;
    const last = kept[kept.length - 1];
    if (last && last.l.at === l.at && last.l.text === l.text) { last.n += 1; continue; }
    kept.push({ l, n: 1 });
  }
  return kept.map(({ l, n }) => n > 1 ? hqLogLine(l).replace(/<\/span><\/li>$/, ` <small class="hq-times">×${n}</small></span></li>`) : hqLogLine(l)).join("");
}
function hqLogLine(l) {
  const full = hqLogText(l), open = !!HQ.logOpen[l.key], most = hqEnginePost(full) ? HQ_ENGINE_LINE : HQ_LONG_LINE;
  const long = full.length > most, cut = !!l.whole && full === l.text && /…$/.test(full);
  const shown = long && !open ? full.slice(0, most) + "…" : full;
  const toggle = long || cut ? ` <button class="hq-more" aria-expanded="${open ? "true" : "false"}" data-act="hq:logmore:${l.key}:toggle">${open ? "less" : "more"}</button>` : "";
  return `<li><time>${esc(l.at)}</time><span class="k-${l.kind}">${hqLogLinks(esc(shown), l.refs)}${toggle}</span></li>`;
}

// What a line of Activity names, as links that open it (the owner, 1 Nov): any item (EL-955)
// as Work's "Looking at" does; the turn its event names (R4952) in its replay, with what it was
// given; and the members its event names, on the team board. `html` is the line escaped; a
// name matches only as a whole word, and only those the event names, so a routine called "R100"
// is no turn.
function hqLogLinks(html, refs) {
  const r = refs || {}, members = (HQ.snap && HQ.snap.members) || [];
  const named = new Map();
  for (const m of members) {
    if (m.name && ((r.ids || []).includes(m.id) || (r.names || []).includes(m.name))) named.set(esc(m.name), m.id);
  }
  const quote = (w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const names = [...named.keys()].sort((a, b) => b.length - a.length).map(quote);
  // Each group is always there, matching nothing when the event names nothing of its kind.
  const run = r.run ? Number(r.run) : "(?!)", who = names.length ? names.join("|") : "(?!)";
  const pattern = new RegExp(`(?<![\\w&#])(?:EL-(\\d+)|R(${run})|(${who}))(?![\\w-])`, "g");
  return html.replace(pattern, (all, task, turn, name) => {
    const [kind, id] = task ? ["task", task] : turn ? ["run", turn] : ["member", named.get(name)];
    return `<button class="hq-link" data-act="hq:from:${kind}:${Number(id)}">${all}</button>`;
  });
}

// The line of Activity with this key, from the tab or the paged view.
function hqLogFind(key) {
  return HQ.log.concat((HQ.feed && HQ.feed.lines) || []).find((l) => String(l.key) === String(key));
}

// One event as a line of the activity log, or null for one it does not tell (a tick).
function hqLogEntry(e) {
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
    // A colleague's help, step by step; its words are on the task and in the conversation.
    "help.asked": ["run", `${name} asked ${hqName(d.expert)} for help${on}`],
    "help.answered": ["done", `${hqName(d.expert)} answered ${name}${on}`],
    "help.concluded": ["done", `${name} concluded from ${hqName(d.expert)}'s answer${on}`],
    "wake.queued": ["wake", `Woke ${name}: ${reason}${on}`],
    "wake.folded": ["wake", `${reason} → folded into ${name}'s live run${on}`],
    "wake.held": ["wake", `${name} waits: ${key} is being worked (${reason})`],
    "wake.refused": ["warn", `Wake for ${name} skipped: ${String(d.why || "").replace(/_/g, " ")}`],
    "run.started": ["run", `R${d.run} ${name} starts${key ? " on " + key : ""}${reason ? ` (${reason})` : ""}, ${hqDollars(d.reserved)} held`],
    // How the turn ended and what it left, in the same plain words as What failed; its tokens and
    // cache share are the replay's, not the log line's (tabs climb, round 2).
    "run.ended": [d.result === "succeeded" ? "done" : d.result === "retry_waiting" ? "warn" : "bad",
      `R${d.run} ${name} ${d.result === "succeeded" ? ["finished", hqTurnSaid("", d.label)].filter(Boolean).join(", ") : hqTurnSaid(d.result, d.label)}${d.result !== "succeeded" && d.why && !/^(timed_out|timeout)$/.test(d.result) ? `: ${d.why}` : ""}${d.spent ? " · " + hqDollars(d.spent) + " spent" : ""}`],
    "gate.result": [d.passed ? "done" : "bad", `Gate on ${key}: ${d.passed ? "passed" : "failed"}`],
    "desk.added": ["you", d.kind === "push" ? `On your desk: push ${d.branch || "a branch"}${on}` : d.kind === "hire" ? `On your desk: ${d.name ? d.name + ", " : "a member"} to hire${on}` : `On your desk: ${String(d.kind || "").replace(/_/g, " ")}${on}`],
    "desk.decided": ["you", `You decided: ${String(d.decision || "").replace(/_/g, " ")}`],
    "budget.crossed": ["bad", `${name} crossed its limit; R${d.run} stopped`],
    "stopped": ["bad", d.by === "tripwire"
      ? `The runaway tripwire stopped the team: ${d.turns || "too many"} turns started in ${Math.round((d.window_ms || 600000) / 60000)} minutes (${(d.members || []).map((m) => `${m.name} ${m.turns}`).join(", ")}); nothing new starts until you resume`
      : "You pulled the kill switch: every run stopped, nothing new starts"],
    "resumed": ["you", "You resumed the engine"],
    // Routines and check-backs (ADR 0107): how the team keeps working.
    "routine.changed": ["you", `Routine ${d.title}: ${d.enabled ? "on" : "off"}`],
    "routine.fired": ["wake", `Routine ${d.title} handed out ${key}`],
    "routine.skipped": ["warn", `Routine ${d.title} skipped: ${d.why}`],
    "routine.posted": ["wake", `Alarm into ${key}: ${d.said}`],
    "checkback.set": ["run", `${name} will check back on ${key} in ${d.minutes} min`],
    "lesson.noted": ["you", `${name} noted a lesson: ${d.lesson || ""}`],
    "member.brain": ["you", `${d.name || name} now ${d.slot === "judgment" ? "judges" : d.slot === "work" ? "works" : "thinks"} with ${d.brain === "local" ? "the house's model" : d.brain === "mac" ? "the owner's Mac" : d.brain}`],
    // Members joining and renamed, and the house's notes (ADR 0111).
    "member.hired": ["you", `${d.name} joined the team`],
    "member.renamed": ["you", `${d.was} is now ${d.name}`],
    "house.noted": ["you", d.entry ? `${d.by} noted ${d.page ? "on the house page" : "in the house log"}: ${d.entry}` : `The house log was rewritten by ${d.by === "owner" ? "you" : d.by}`],
    // The team's world (ADR 0122): sections, canon, stories and handovers.
    "world.changed": [d.what === "demoted" ? "warn" : "you", d.what === "promoted" ? `${d.address} is canon${d.why ? ": " + d.why : ""}`
      : d.what === "demoted" ? `You demoted ${d.address} from canon: ${d.why || ""}`
      : d.what === "proposed" ? `${d.by} proposes ${d.address} for canon: ${d.warrant || ""}`
      : d.what === "migrated" ? `The house log and ${d.stories || 0} lessons moved into the team's world`
      : `${d.by || "Someone"} ${String(d.what || "changed").replace(/_/g, " ")} ${d.address || ""}`],
    "story.told": ["run", `${d.by} ${d.retold ? "retold" : "told"} the story ${d.handle}`],
    "task.handed_over": ["run", `${hqName(d.by)} handed ${key} on: ${d.note || ""}`],
    // A guard that is tried in shadow was switched on, or put back in shadow (ADR 0133).
    // A lead narrowed what done means for a task in its area (ADR 0133).
    "task.criteria": ["run", `${hqName(d.by)} narrowed what done means for ${key}: ${(d.after || []).join("; ")}${d.why ? " (" + d.why + ")" : ""}`],
    // A drain before a restart (ADR 0134): silent on the desk, one line here.
    "drain.began": ["warn", `A restart is announced: nothing new starts until it has been (${d.running || 0} running, up to ${d.cap_seconds} s)`],
    "drain.ended": [d.cut ? "bad" : "done", d.by === "restart"
      ? `The restart came: ${d.clean || 0} turn${d.clean === 1 ? "" : "s"} ended before it, ${d.cut ? d.cut + " cut and tried again" : "none cut"}`
      : d.by === "resume" ? "The announced restart is over: new turns start again"
      : "An announced restart never came: lifted after half an hour"],
    "checkback.fired": ["wake", `${name} checks back on ${key}`],
    "checkback.dropped": ["run", `Check-back on ${key} dropped: it is over`],
    // Projects, their budgets, and whom members report to (ADR 0113).
    "project.changed": [d.status === "paused" || d.status === "dropped" ? "warn" : "you", `Project ${d.title} is ${d.status}: ${d.what || ""}${d.by && d.by !== "owner" ? ", by " + d.by : ""}`],
    "project.budget": [d.level === "spent" ? "bad" : "warn", `Project ${d.title} ${d.said || ""}`],
    // The engineering wing (ADR 0115): branches, their checks and the owner's pushes.
    // The check clause only when there is a check: it read "check cundefined" (tabs climb, round 1).
    "branch.submitted": ["run", `${name} submitted ${d.name}${d.files != null ? ` (${d.files} files)` : ""} on ${key}${typeof d.check === "number" ? `: check c${d.check} queued` : ""}`],
    "check.done": [d.ok ? "done" : "bad", `Check c${d.check} on ${d.branch}: ${d.summary || (d.ok ? "passed" : "failed")}`],
    // Elastic staffing (ADR 0118): nobody is fired; they are put on furlough and recalled.
    "member.furloughed": ["you", `${d.name} is on furlough`],
    "member.recalled": ["you", `${d.name} is back from furlough`],
    // Notebooks (ADR 0118): what a member wrote down, and what it let go.
    "note.forgotten": ["run", `${d.by === "owner" ? "You deleted" : d.name + " forgot"} a note${d.by === "owner" ? " of " + d.name + "'s" : ""}: ${d.text}${d.by === "owner" ? "" : " (" + d.reason + ")"}`],
    "branch.pushed": [d.ok ? "you" : "bad", d.ok ? `You pushed ${d.branch} to the fork` : `Pushing ${d.branch} failed`],
    // The open office (ADR 0119): the team decides its decorations together. The words come from
    // HQ, and a member's reason is in them, so the log escapes them like any other.
    "office.proposed": ["run", d.text || ""],
    "office.voted": ["run", d.text || ""],
    "office.changed": [d.via === "owner_undo" || d.via === "owner_reset" ? "you" : "done", d.text || "The office changed"],
    "office.dropped": ["warn", d.text || ""],
    "office.vetoed": ["you", d.text || ""],
    // Games (ADR 0121): ping-pong and pool the engine plays, and chess the members play. The
    // words are HQ's, with the members' names in them, so the log escapes them like the rest.
    "game.played": ["run", d.text || ""],
    "chess.started": ["run", d.text || ""],
    "chess.moved": ["run", d.text || ""],
    "chess.ended": ["done", d.text || ""],
    "area.reported": ["run", `${hqName(d.lead)} reported on their area`],
  }[e.kind];
  if (!said) return null;
  // A post in a thread is told whole in the feed: the line names its place in the task's
  // thread, and the words are read from there when the line is drawn (the snapshot that has
  // them may arrive just after the event does). The preview stands in until then.
  // The turn and the members the event names, by id or by name, for the line's links.
  const refs = { run: d.run, ids: [d.member, d.expert, d.lead, d.by].filter((x) => typeof x === "number"),
    names: [d.author, d.name, d.by].filter((x) => typeof x === "string") };
  return { key: e.id, at: hqClock(e.at), kind: said[0], text: said[1], refs,
    whole: e.kind === "thread.posted" && d.post != null ? { task: d.task, post: d.post, author: d.author } : null };
}

// One event, in words for the activity log, and in the studio if it is live: a real post is said
// briefly over its author's head, an item closed waits for the snapshot to say how, and a check that
// passes where the item's last one failed is its member's recovery, read against the snapshot from
// before this event (ADR 0115).
function hqTell(e, live) {
  const d = e.data || {};
  const line = hqLogEntry(e);
  if (line) { HQ.log.unshift(line); HQ.log.length = Math.min(HQ.log.length, 500); }
  if (!live) return;
  if (e.kind === "thread.posted") {
    const author = ((HQ.snap && HQ.snap.members) || []).find((m) => m.name === d.author);
    if (author && d.preview) HQ.said[author.id] = { text: hqSaid(d.preview), t: HQ_SAY_SECONDS };
  }
  if (e.kind === "task.changed" && d.status === "done" && d.task) HQ.landing[d.task] = { owner: d.owner, unverified: d.unverified === true, troubled: hqTroubled(hqTask(d.task)) };
  if (e.kind === "check.done" && d.task) {
    const t = hqTask(d.task), was = t && hqLastCheck(t);
    if (d.ok === true && was && was.ok === false) HQ.recovered[d.member] = { task: d.task, why: "check", t: HQ_RECOVERED_SECONDS };
  }
}


// Items closed live, once the snapshot says how: one closed as met its criteria or as completed and
// not verified shines on the week's shelf for a few seconds, and one that had a problem and closed
// as met its criteria is its owner's recovery. Any other closing shows nothing.
function hqLandings() {
  for (const [id, l] of Object.entries(HQ.landing)) {
    delete HQ.landing[id];
    const t = hqTask(Number(id));
    if (!t || !t.closure || !(t.closure.why in HQ_DONE_KINDS)) continue;
    HQ.fresh[t.id] = 8;
    if (HQ_DONE_KINDS[t.closure.why] && !l.unverified && l.troubled && l.owner) HQ.recovered[l.owner] = { task: t.id, why: "closed", t: HQ_RECOVERED_SECONDS };
  }
}

// ---- problems and recoveries (the owner, 3 Nov) ---------------------------------------------
// One sign beside a worker's head, and only what the record says: a red "!" while its work is
// blocked, a dark tile with a red cross while that work's latest check failed, and a green tick
// for a while after a recovery: a check that passed where the item's last one failed, or an item
// that was blocked or failing closing as met its criteria. A retry, a turn that ended, an item
// only unblocked, or a closing nothing verified is never a recovery. Working, the badge is its
// current item's; between turns, its open items'.
const HQ_RECOVERED_SECONDS = 12;
// Whether the person asked their device for calm: then nothing in the studio blinks or types.
function hqCalm() {
  try { return typeof matchMedia === "function" && !!matchMedia("(prefers-reduced-motion: reduce)").matches; } catch (_) { return false; }
}
const HQ_OPEN = ["todo", "doing", "in_review", "blocked"];
// An item's latest check, or null: the newest branch that has one.
function hqLastCheck(t) {
  const checked = (t && t.branches || []).filter((b) => b.check).sort((a, b) => (a.at || 0) - (b.at || 0) || a.id - b.id);
  return checked.length ? checked[checked.length - 1].check : null;
}
function hqTroubled(t) { return !!t && (t.status === "blocked" || (hqLastCheck(t) || {}).ok === false); }
// -> { kind: "recovered" | "blocked" | "check", task } or null.
function hqTrouble(id) {
  const r = HQ.recovered[id];
  if (r && r.t > 0) return { kind: "recovered", task: r.task, why: r.why };
  const m = hqMember(id);
  if (!m) return null;
  const run = (m.running || [])[0];
  const items = run && run.task ? [hqTask(run.task)].filter(Boolean)
    : ((HQ.snap && HQ.snap.tasks) || []).filter((t) => t.owner === id && HQ_OPEN.includes(t.status));
  const blocked = items.find((t) => t.status === "blocked");
  if (blocked) return { kind: "blocked", task: blocked.id };
  const failing = items.find((t) => HQ_OPEN.includes(t.status) && (hqLastCheck(t) || {}).ok === false);
  return failing ? { kind: "check", task: failing.id } : null;
}
// What the badge means, in words, for the worker's panel. "" without one.
function hqTroubleSaid(id) {
  const b = hqTrouble(id);
  if (!b) return "";
  const t = hqTask(b.task), title = t ? ` ${esc(t.title)}` : "";
  if (b.kind === "recovered") return b.why === "check" ? `Check passed on ${hqKey(b.task)} after a failed one${title ? ":" + title : ""}` : `${hqKey(b.task)} closed as met its criteria after a problem${title ? ":" + title : ""}`;
  if (b.kind === "blocked") return `${hqKey(b.task)} is blocked${title ? ":" + title : ""}`;
  const c = hqLastCheck(t) || {};
  return `The latest check on ${hqKey(b.task)} failed${c.summary ? ": " + esc(c.summary) : ""}`;
}
function hqLoop() {
  // Only where something draws: not in a background tab's sandbox, nor in the console check.
  if (HQ.looping || typeof requestAnimationFrame !== "function") return;
  HQ.looping = true;
  let last = Date.now();
  const step = () => {
    if (NAV.v !== "hq" || hqPlace() !== "office") { HQ.looping = false; return; }
    const now = Date.now(), dt = Math.min(0.1, (now - last) / 1000); last = now; HQ.t += dt;
    for (const id of Object.keys(HQ.fresh)) if ((HQ.fresh[id] -= dt) <= 0) delete HQ.fresh[id];
    for (const id of Object.keys(HQ.recovered)) if ((HQ.recovered[id].t -= dt) <= 0) delete HQ.recovered[id];
    for (const id of Object.keys(HQ.said)) if ((HQ.said[id].t -= dt) <= 0) delete HQ.said[id];
    hqDraw();
    requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

// Something chosen in the studio, or a line on the map, opens on its own Looking at page.
function hqLook(sel) { HQ.back = null; HQ.selected = sel; if (hqPlace() !== "detail") HQ.from = hqPlace(); HQ.place = "detail"; }
function hqSelect(sel) { hqLook(sel); render(); }

// HQ's fonts, the same as its design, and the board's words: loaded once, only when HQ opens.
function hqFonts() {
  if (typeof document === "undefined" || !document.head || document.getElementById("hq-fonts")) return;
  const link = document.createElement("link");
  link.id = "hq-fonts"; link.rel = "stylesheet";
  link.href = "https://fonts.googleapis.com/css2?family=Silkscreen&family=IBM+Plex+Mono:wght@400;500;600&display=swap";
  document.head.appendChild(link);
}

// ---- the screen ------------------------------------------------------------------------

// The screen opens on the agents map (the owner's decision M6, 7 Nov 2026: "Agents map should be
// the primary view"): the team and what each holds, with your decisions beside it. Everything else
// is a place of its own, reached from one quiet row and back to the map, never a tab strip: open
// work, projects and the team, health, the history, the model league and the office's studio.
// Every place is drawn and the others hidden, so a live update never loses what is typed in one
// that is not showing. Looking at ("detail") is the page a member, an item or a studio tap opens.
const HQ_PLACES = [["map", "Agents map"], ["work", "Open work"], ["manage", "Projects & team"], ["health", "Health"],
  ["activity", "Activity"], ["chess", "Chess league"], ["office", "Office"]];
// Where each tab of the five-tab screen (362e467f) and the ones before it now lives, for links
// kept from before: the old Work tab's desk is the map's decisions.
const HQ_TAB_MOVED = { work: "map", now: "map", desk: "map", team: "manage", knowledge: "manage" };

// Where HQ is. A fresh entry and a reload open the map; the tab the old screen remembered in
// this browser is read once and removed, and storage that refuses changes nothing.
function hqPlace() {
  if (HQ.place) return HQ.place;
  for (const key of ["endora.hq.tab.v4", "endora.hq.tab.v3"]) {
    try { localStorage.removeItem(key); } catch (_) { /* private window or blocked storage */ }
  }
  HQ.place = "map";
  return HQ.place;
}

// Go to a place; leaving Looking at puts down what it held.
function hqGo(id) {
  if (!HQ_PLACES.some(([p]) => p === id)) return;
  HQ.place = id;
  HQ.selected = null;
}

// After a change of place, the keyboard starts at its title, as a new page would.
function hqFocusTitle() {
  const el = typeof document !== "undefined" && document.getElementById("hq-title");
  if (el && el.focus) el.focus();
}

// What each kind of desk item's choices do, in one plain line under it (the owner: "didn't know
// what each button meant"). Every kind a desk item can have is here.
const HQ_DESK_HELP = {
  goal: "Approve starts the team on this goal; Decline drops it.",
  stuck: "Try again sends the work back to the team; Drop it gives up on it.",
  ready_to_merge: "The work is done and checked. Mark merged once you have merged it yourself; Not now leaves it for later.",
  ready_to_read: "A write-up is ready for you. Mark read once you have read it; Not now keeps it for later.",
  question: "A member is asking you. Type an answer if you like, then Answer & wake sends it and wakes them; Dismiss closes it unanswered.",
  notice: "Only to know of. Seen or Dismiss clears it.",
  approval: "A member wants to run exactly what is shown, once. Approve runs it; Decline runs nothing.",
  project_cap: "The team asks for a monthly spending cap on a project. Grant the cap gives it (type another amount to change it); No cap lets the project run on the pool's limits alone.",
  project_budget: "This project spent its monthly cap, so its paid work waits. Raise the cap (type the new amount) to go on now; Wait for next month lets it resume when the month turns.",
  rounds: "This work was sent back three times. Accept as done closes it; Send it back gives it one more round.",
  hire: "A lead asks to add a member. Hire adds them; Decline does not.",
  canon: "Canon = settled truth only you can change. Make it canon once you are sure it is true; Not canon leaves it as a note.",
  out_for_day: "This member has used twice its share and is stopped. Give it today lets this member keep working today up to the amount (type one, or leave it empty for as offered); Wait stops them until tomorrow's budget.",
  tripwire: "The team started far more turns in ten minutes than real work does, so HQ stopped it. Resume lets it work again; Seen leaves it stopped for you to look first. The activity log shows what they were doing.",
  push: "Push it sends the accepted branch to your fork; Decline does not. Without a push key on the node, download the bundle and push it yourself.",
  privacy: "The privacy scan found something personal-looking in an accepted branch, so nothing was pushed. Send it back has the team take it out; Push it anyway pushes it as it is, if what was found is not yours.",
  needs_owner: "An accepted change needs something from you before it is deployed, such as a vault value it now reads. Done once you have set it; Dismiss clears it.",
};

// The choices each kind of desk item offers: [decision, label, style].
const HQ_DESK_CHOICES = {
  goal: [["approve", "Approve", "ok"], ["decline", "Decline", "no"]], stuck: [["retry", "Try again", "ok"], ["drop", "Drop it", "no"]],
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
  // A cap the team asks for a project (ADR 0113): granted as asked, or at what the owner
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
  // A piece of the world proposed for canon (ADR 0122): only the owner makes it so.
  canon: [["approve", "Make it canon", "ok"], ["decline", "Not canon", "no"]],
  // A member at twice its share with the pool's day not spent (ADR 0113): an allowance for
  // the rest of today, at the amount written on the card, or wait for tomorrow.
  out_for_day: [["approve", "Give it today", "ok"], ["decline", "Wait for tomorrow", "no"]],
  // The runaway tripwire stopped the team (ADR 0133): resume it, or look first.
  tripwire: [["resume", "Resume the team", "ok"], ["seen", "Seen", ""]],
  // Accepted work without a card (ADR 0115, amended 2026-10-31): a branch the privacy scan
  // held, and a change that needs something of the owner.
  privacy: [["send_back", "Send it back", "ok"], ["push_anyway", "Push it anyway", "no"]],
  needs_owner: [["seen", "Done", "ok"], ["dismiss", "Dismiss", "no"]],
};

function hqDeskChoices(d) {
  if (d.kind === "push") return hqCanPush(d) ? [["approve", "Push it", "ok"], ["decline", "Decline", "no"]] : [["seen", "Seen", "ok"], ["dismiss", "Dismiss", "no"]];
  return HQ_DESK_CHOICES[d.kind] || [];
}

// "Give work": a form, not pop-ups. The work goes on a loop's plan, and the loop hands it out
// (ADR 0142).
function hqWorkForm(s) {
  if (!HQ.working) return "";
  const loops = (s.projects || []).filter((p) => p.status === "active");
  const draft = HQ.workDraft || {};
  const pick = draft.loop ? Number(draft.loop) : (loops[0] || {}).id;
  const options = loops.map((p) => `<option value="${p.id}"${p.id === pick ? " selected" : ""}>${esc(p.title)}</option>`).join("");
  return `<div class="hq-work" role="group" aria-label="Give the team work">
    <label for="hq-work-title">What should the team do?</label>
    <input id="hq-work-title" maxlength="200" autocomplete="off" value="${esc(draft.title || "")}" placeholder="e.g. Check why the backups are slow" />
    <label for="hq-work-done">What does done mean? (at least one line)</label>
    <textarea id="hq-work-done" rows="3" required aria-required="true" placeholder="One per line, or separated by ;">${esc(draft.done || "")}</textarea>
    <p class="hq-note" style="margin:0">Start one with <code>live:</code> to have it checked against the lab as it is now.</p>
    <label for="hq-work-loop">Which loop?</label>
    <select id="hq-work-loop">${options || `<option value="">No loop yet: make a project first</option>`}</select>
    <div class="hq-row"><button class="hq-btn tall ok" data-act="hq:work:send:go">Send</button>
      <button class="hq-btn tall" data-act="hq:work:cancel:go">Cancel</button></div></div>`;
}

// The small status chips under the title: today's pool and what waits on the owner.
function hqChips(s) {
  const pool = s.pool || {}, waiting = (s.desk || []).length;
  // What waits on the owner first: a phone's chips scroll, and this one must always show.
  const chips = [`<button class="hq-chip hq-chip-go${waiting ? " has" : ""}" data-act="hq:place:map:decisions">${waiting ? `${waiting} waiting on you` : "Nothing waiting on you"}</button>`];
  if (pool.per_day != null) {
    const left = Math.max(0, pool.per_day - (pool.committed || 0));
    chips.push(`<span class="hq-chip" title="Today's money for the whole team"><span class="hq-wide">Pool </span><span class="hq-spent">${hqDollars(pool.spent)} spent · </span>${hqDollars(left)} left</span>`);
  }
  // Turns of judgment that wait for budget are kept, not dropped (ADR 0131).
  if (pool.waiting > 0) chips.push(`<span class="hq-chip" title="They start when the day turns, you top up the pool, or you give the member an allowance">${pool.waiting} ${pool.waiting === 1 ? "turn waits" : "turns wait"} for budget</span>`);
  // A member's day of turns on the house's own models is capped (ADR 0133): the rest wait.
  if (pool.capped > 0) chips.push(`<span class="hq-chip" title="A member takes at most a day's turns on the house's own models; the rest start when the day turns">${pool.capped} ${pool.capped === 1 ? "turn waits" : "turns wait"} for tomorrow</span>`);
  chips.push(...hqCreditChips());
  // What the week came to (ADR 0132): changes the owner approved and Endora pushed to the fork.
  const outcomes = s.scorecard && s.scorecard.outcomes;
  if (outcomes && outcomes.week) chips.push(`<span class="hq-chip" title="Changes you approved to push to your fork, and Endora pushed, in the last 7 days">${Number(outcomes.week.landed) || 0} pushed<span class="hq-wide"> this week</span><span class="hq-narrow">/7d</span></span>`);
  return `<div class="hq-chips">${chips.join("")}</div>`;
}

// The quiet row to the other places: a link each, the place showing marked, and nothing that
// competes with the map (no tab strip, no badges: what waits on you is a chip and the map's own).
function hqPlaces() {
  const here = hqPlace();
  return `<nav class="hq-places" aria-label="More of HQ"><span>More of HQ:</span>${HQ_PLACES.filter(([id]) => id !== "map").map(([id, label]) =>
    `<button class="hq-link" data-act="hq:place:${id}:go"${id === here ? ' aria-current="page"' : ""}>${label}</button>`).join("")}</nav>`;
}

// Health on a wide screen: the outcomes, What failed beside them, then the team's turns, so the two
// columns end near each other (round 6); a phone reads them in that order.
function hqHealthCards(s) {
  const card = hqScorecard(s), cut = card.indexOf("<h3>The team's turns</h3>");
  if (cut < 0) return card + hqFailures();
  // Named areas: on a wide screen What failed spans both rows beside the outcomes and the turns.
  return `<div class="hq-health">${card.slice(0, cut).replace('<section class="hq-card"', '<section class="hq-card hq-h-out"')}</section>${hqFailures().replace('<section class="hq-card"', '<section class="hq-card hq-h-fail"')}<section class="hq-card hq-h-turns"><h2>The team's turns</h2>${card.slice(cut + "<h3>The team's turns</h3>".length)}</div>`;
}
function hqPanel(id, html) {
  const label = id === "detail" ? "Looking at" : (HQ_PLACES.find(([p]) => p === id) || [id, id])[1];
  return `<div class="hq-place" id="hq-panel-${id}" role="region" aria-label="${esc(label)}"${hqPlace() === id ? "" : " hidden"}>${html}</div>`;
}

// Escape leaves a secondary place for the map, and Looking at for where it was opened, unless
// something is being typed or the paged history (its own Escape) is open.
function hqPlaceKey(ev) {
  if (ev.key !== "Escape" || HQ.feed || NAV.v !== "hq" || hqPlace() === "map" || hqTyping()) return;
  ev.preventDefault();
  hqGo(hqPlace() === "detail" ? HQ.from || "map" : "map"); render();
}
document.body.addEventListener("keydown", hqPlaceKey);

// What is typed in the give-work form is kept, so a live update never empties it.
document.body.addEventListener("input", (ev) => {
  const id = ev.target && ev.target.id;
  if (id === "hq-work-title") HQ.workDraft.title = ev.target.value;
  else if (id === "hq-work-done") HQ.workDraft.done = ev.target.value;
  else if (id === "hq-work-loop") HQ.workDraft.loop = ev.target.value;
});

// One card of the desk or of the digest: what it says and is about, what its choices do, and the
// choices. Every word goes through esc: they are a member's.
function hqNeed(s, d, help = true) {
  const choices = hqDeskChoices(d);
  // The task's own title leads, so a branch name or engine words are not the first thing read.
  const t = d.task ? ((s.tasks || []).find((x) => x.id === d.task) || null) : null;
  const title = t && t.title ? `<strong class="hq-need-title">${hqKey(t.id)} ${esc(t.title)}</strong>` : "";
  return `<div class="hq-need ${esc(d.kind)}">${title}${hqClamp(`need${d.id}`, d.text)}
      <p class="hq-who">${esc(String(d.kind).replace(/_/g, " "))}${d.task ? ` · ${hqKeyLink(d.task)}` : ""}${(d.also || []).length ? ` · also on ${d.also.map((t) => hqKeyLink(Number(t))).join(", ")}: one answer reaches each` : ""}${d.project ? ` · project ${esc(hqProjectTitle(d.project))}` : ""}</p>
      ${help && HQ_DESK_HELP[d.kind] ? `<p class="hq-help">${esc(HQ_DESK_HELP[d.kind])}</p>` : ""}
      ${HQ.deskNote[d.id] ? `<p class="hq-who hq-refused" role="alert">${esc(HQ.deskNote[d.id])}</p>` : ""}
      ${d.kind === "approval" ? hqApproval(d.approval, d.serves) : ""}
      ${d.kind === "push" && !hqCanPush(d) && hqBundleOf(d) ? `<div class="hq-row"><button class="hq-btn" data-act="hq:bundle:${d.task}:${esc(hqBundleOf(d))}">Download the bundle</button></div>` : ""}
      ${d.kind === "question" ? `<textarea id="hq-answer-${d.id}" rows="2" placeholder="Your answer, for the task's thread (optional)" aria-label="Your answer"></textarea>` : ""}
      ${d.kind === "project_cap" || d.kind === "project_budget" ? `<input id="hq-answer-${d.id}" inputmode="decimal" placeholder="${d.kind === "project_cap" ? "Dollars a month (empty: as asked)" : "The new cap, in dollars a month"}" aria-label="The cap, in dollars a month" />` : ""}
      ${d.kind === "out_for_day" ? `<input id="hq-answer-${d.id}" inputmode="decimal" placeholder="Dollars for today (empty: as offered)" aria-label="The allowance, in dollars for today" />` : ""}
      ${d.kind === "out_for_day" && hqPoolSpent(s) ? `<div class="hq-row"><button class="hq-btn" data-act="hq:pool:topup:go">Top up the pool first</button></div>` : ""}
      <div class="hq-row">${choices.map(([v, l, cls]) => `<button class="hq-btn ${cls}" data-act="hq:decide:${d.id}:${v}">${l}</button>`).join("")}</div></div>`;
}

// The desk: the first few cards whole, so the top of it is answered where it stands; the rest one
// line each, their kind and first words, opening to the whole card (the density audit,
// 2026-10-30: fifteen whole cards were three phone screens). Nothing is hidden from the count.
const HQ_DESK_WHOLE = 5;
function hqDesk(s) {
  const short = (text) => { const t = String(text || ""); return t.length > 80 ? t.slice(0, 80) + "…" : t; };
  // The team stopped by HQ itself is always whole: nothing goes on until the owner answers it.
  const isWhole = (d, i) => i < HQ_DESK_WHOLE || d.kind === "tripwire";
  const items = s.desk || [];
  // A kind's help is said on its first card only: the same lines on every card were noise.
  const told = new Set(), firstOfKind = (d) => !told.has(d.kind) && told.add(d.kind);
  const whole = items.filter(isWhole).map((d) => hqNeed(s, d, firstOfKind(d))).join("");
  const rows = items.filter((d, i) => !isWhole(d, i)).map((d) => hqOpenRow(`desk${d.id}`, esc(String(d.kind).replace(/_/g, " ")), esc(short(d.text)), hqNeed(s, d))).join("");
  return whole + (rows ? `<ul class="hq-team">${rows}</ul>` : "");
}

// "While you were away" (ADR 0132): stuck work, a guard's finding and a notice, newest first, in
// one card under the desk, on the Work tab. None needs a decision before the team can go on, so none
// interrupts; each is the owner's to act on when they like, with the choices it always had, and
// acting on one takes it out. Shown only when there is something in it.
function hqDigest(s) {
  const items = s.digest || [];
  if (!items.length) return "";
  // The same notice said again (the Mac still busy an hour later) is one card, counted: the
  // newest is acted on, and its repeats go with it when HQ clears them.
  const seen = new Map();
  for (const d of items) {
    const key = `${d.kind}|${d.task}|${d.text}`;
    if (seen.has(key)) seen.get(key).times += 1;
    else seen.set(key, { d, times: 1 });
  }
  // One line each until tapped (the density audit, 2026-10-30): what kind it is and its first
  // words; open, the card with the choices it always had.
  const short = (text) => { const t = String(text || ""); return t.length > 80 ? t.slice(0, 80) + "…" : t; };
  const rows = [...seen.values()].map(({ d, times }) => hqOpenRow(`digest${d.id}`, esc(String(d.kind).replace(/_/g, " ")),
    `${esc(short(d.text))}${times > 1 ? ` <span class="hq-count">×${times}</span>` : ""}`,
    `<div class="hq-needs">${hqNeed(s, d)}</div>`));
  const list = hqMore("digest", rows, 3);
  return `<section class="hq-card hq-digest" aria-live="polite"><h2>While you were away <span class="hq-count">${items.length}</span></h2>
    <p class="hq-note">Nothing here needs you before the team can go on: tap one to act on it when you like, and acting on one takes it out of this list.</p>
    <ul class="hq-team">${list.shown}</ul>${list.more}</section>`;
}

// Activity, a tab of its own (the owner, 31 Oct): the latest twenty, live, and "Show more" for
// a full-screen view of the past a page at a time, Newer and Older. Nothing loads as the page
// scrolls: every page is asked for by a button.
const HQ_LOG_SHOWN = 20, HQ_FEED_PAGE = 50;
// HQ serves the past before an id, at most 200 events at a time; a tick takes no line, so a page
// may ask again below the oldest it read, a few times at most.
const HQ_LOG_BATCH = 200, HQ_LOG_ASKS = 5;

// Up to `want` lines of the events just below the id `before`, newest first, and where the next
// page starts (0 when HQ has nothing older).
async function hqLogPage(before, want) {
  const lines = [];
  let next = before;
  for (let ask = 0; ask < HQ_LOG_ASKS && next > 1; ask++) {
    const got = await api("GET", `/v1/hq/events?before=${Number(next)}&limit=${HQ_LOG_BATCH}`);
    const events = (got.events || []).slice().reverse();
    for (const e of events) {
      next = e.id;
      const line = hqLogEntry(e);
      if (line) lines.push(line);
      if (lines.length >= want) return { lines, next: next > 1 ? next : 0 };
    }
    if (events.length < HQ_LOG_BATCH) return { lines, next: 0 };
  }
  return { lines, next: next > 1 ? next : 0 };
}

// The turns running now, at the top of Activity (the owner, 1 Nov): each with a Watch link to
// its replay, live while it runs. Nothing when none runs.
function hqRunningNow() {
  const runs = ((HQ.snap && HQ.snap.runs) || []).filter((r) => r.status === "running");
  if (!runs.length) return "";
  const line = (r) => `<li><span>R${Number(r.id)} ${esc(hqName(r.member))}${r.task ? ` on ${hqKey(r.task)}` : ""}</span> <button class="hq-link" data-act="hq:from:watch:${Number(r.id)}">Watch</button></li>`;
  return `<section class="hq-card hq-running hq-wide-card"><h2>Running now <span class="hq-count">${runs.length}</span></h2><ul class="hq-running-list">${runs.map(line).join("")}</ul></section>`;
}

// The Activity tab: the turns running now, then the latest twenty, newest first, each line whole.
function hqActivity() {
  const log = hqLogLines(HQ.log.slice(0, HQ_LOG_SHOWN));
  return `${hqRunningNow()}<section class="hq-card hq-wide-card"><h2>Activity</h2><ul class="hq-log hq-log-whole">${log || `<li class="hq-empty">Nothing yet.</li>`}</ul>
    <div class="hq-row"><button class="hq-btn" id="hq-feed-more" data-act="hq:feed:open:go">Show more</button></div></section>`;
}

// The past, full screen, a page at a time; Newer and Older step through it, as Replay steps
// through a task's turns, and Back (or Escape) returns to the Activity tab.
function hqFeedView() {
  const f = HQ.feed;
  const lines = hqLogLines(f.lines || []);
  const body = f.error ? `<p class="hq-empty">${esc(f.error)}</p>`
    : !f.lines ? `<p class="hq-empty">Loading…</p>`
    : `<ul class="hq-log hq-log-whole">${lines || `<li class="hq-empty">Nothing older.</li>`}</ul>`;
  const pager = `<div class="hq-replay-steps">
      <button class="hq-btn" data-act="hq:feed:newer:go"${!f.at || f.busy ? " disabled" : ""}>◀ Newer</button>
      <span class="hq-replay-progress">Page ${f.at + 1}</span>
      <button class="hq-btn" data-act="hq:feed:older:go"${!f.next || f.busy ? " disabled" : ""}>Older ▶</button></div>`;
  return `<div class="hq hq-feed"><div class="hq-bar"><h1 id="hq-feed-title" tabindex="-1">Activity</h1><span class="hq-grow"></span>
      <div class="hq-row"><button class="hq-btn" data-act="hq:feed:close:go">◀ Back to HQ</button></div></div>
    ${pager}<section class="hq-card" aria-live="polite">${body}</section>${pager}</div>`;
}

// Open the paged view on the newest page, step a page, or close it. Each page is read afresh.
async function hqFeedAct(what) {
  const focus = (id) => { const el = document.getElementById(id); if (el && el.focus) el.focus(); };
  if (what === "close") {
    HQ.feed = null; hqGo("activity"); render(); window.scrollTo(0, 0); focus("hq-feed-more");
    return true;
  }
  if (what === "open") HQ.feed = { at: 0, starts: [HQ.last + 1], lines: null, next: 0, busy: false, error: "" };
  const f = HQ.feed;
  if (!f || f.busy) return true;
  if (what === "older") { if (!f.next) return true; f.starts[f.at + 1] = f.next; f.at += 1; }
  else if (what === "newer") { if (!f.at) return true; f.at -= 1; }
  else if (what !== "open") return true;
  Object.assign(f, { busy: true, lines: null, next: 0, error: "" });
  render(); window.scrollTo(0, 0);
  try {
    const page = await hqLogPage(f.starts[f.at], HQ_FEED_PAGE);
    Object.assign(f, { lines: page.lines, next: page.next });
  } catch (e) { f.error = "Couldn't load the activity: " + e.message; }
  f.busy = false;
  if (HQ.feed !== f) return true;
  render(); focus("hq-feed-title");
  return true;
}

// Escape closes the paged view, as its Back button does.
function hqFeedKey(ev) {
  if (ev.key !== "Escape" || !HQ.feed) return;
  ev.preventDefault();
  dispatch("hq:feed:close:go");
}
document.body.addEventListener("keydown", hqFeedKey);

function viewHQ() {
  if (HQ.error) return `<div class="hq"><h1>Endora HQ</h1><section class="hq-card"><p class="hq-empty">${esc(HQ.error)}</p></section></div>`;
  const s = HQ.snap;
  if (!s) return `<div class="hq"><h1>Endora HQ</h1><p class="hq-empty">Loading…</p></div>`;
  if (HQ.feed) return hqFeedView();
  const place = hqPlace();
  // The studio draws and moves only while the Office shows it.
  setTimeout(() => {
    hqFonts();
    if (place !== "office") return;
    hqStudioWatch(); hqLoop();
    const cv = document.getElementById("hq-office");
    if (cv && !cv.dataset.tapped) { cv.dataset.tapped = "1"; cv.addEventListener("click", hqTap); }
  }, 0);
  const pool = s.pool || {}, c = s.counts || {};
  const retrying = (s.runs || []).filter((r) => r.status === "retry_waiting").length;
  const stats = [["Tick", c.ticks || 0], ["Running", c.running || 0], ["Queued", c.queued || 0], ["Retrying", retrying], ["Held", c.held || 0],
    ["Coalesced", c.folded || 0], ["Today", hqMoney(pool, `· ${hqDollars(pool.per_day)} a day`)], ["Open tasks", c.open_tasks || 0]]
    .map(([k, v]) => `<span class="hq-stat">${k} <b>${v}</b></span>`).join("");
  const desk = hqDesk(s);
  const title = place === "map" ? "Endora HQ" : place === "detail" ? "Looking at" : HQ_PLACES.find(([p]) => p === place)[1];
  const back = place === "map" ? "" : `<button class="hq-btn tall" data-act="hq:place:map:go">◀ Agents map</button>`;
  const lookBack = HQ.back ? hqBackLink()
    : `<div class="hq-row"><button class="hq-btn" data-act="hq:place:${esc(HQ.from || "map")}:go">◀ Back</button></div>`;
  return `<div class="hq" data-place="${esc(place)}">
    <div class="hq-bar">
      ${back}<h1 id="hq-title" tabindex="-1">${esc(title)}</h1>
      ${s.stopped && s.stopped_by === "tripwire" ? `<span class="hq-tag" role="status">Stopped by the runaway tripwire</span>` : ""}
      <span class="hq-grow"></span>
      <div class="hq-row">
        <button class="hq-btn tall" data-act="hq:work" aria-expanded="${HQ.working ? "true" : "false"}">Give work</button>
        <button class="hq-btn tall stop" aria-pressed="${s.stopped ? "true" : "false"}" data-act="${s.stopped ? "hq:resume" : "hq:stop"}">${s.stopped ? "Resume" : "Kill switch"}</button>
      </div>
    </div>
    ${hqWorkForm(s)}
    ${hqChips(s)}
    ${hqPlaces()}
    <div class="hq-main">
      ${hqPanel("map", `<div class="hq-mapgrid">${hqMap()}
        <div class="hq-decide" id="hq-decisions" tabindex="-1">
          <section class="hq-card"><h2>Your decisions <span class="hq-count">${(s.desk || []).length}</span></h2>
            <div class="hq-needs">${desk || `<p class="hq-empty">Nothing waits on you.</p>`}</div></section>
          ${hqDigest(s)}</div></div>`)}
      ${HQ.selected ? hqPanel("detail", `<section class="hq-card" id="hq-looking" aria-live="polite"><h2>Looking at</h2>${lookBack}${hqSelected()}</section>`) : ""}
      ${hqPanel("work", `
        ${hqInFlight(s)}
        ${hqHolodeck(s) ? hqFold("holodeck", "Holodeck", "chess matches and the model league", hqHolodeck(s)) : ""}`)}
      ${hqPanel("manage", `
        ${hqMoneyCard(s)}
        ${hqNotes(s)}
        ${hqProjects(s)}
        <div class="hq-foldstack">
        ${hqAccepted(s)}
        ${hqWorldCard(s) || hqHouse(s) ? hqFold("knows", "What the team knows", "the world's pages and the house", `${hqWorldCard(s)}${hqHouse(s)}`) : ""}
        <section class="hq-card"><h2>Team</h2>${hqRoster(s)}${hqFormFold("hire", "Hire a member", hqHireForm())}</section></div>
        ${hqContracts(s.contracts)}`)}
      ${hqPanel("health", `<div class="hq-stats hq-wide-card">${stats}</div>${hqHealthCards(s)}`)}
      ${hqPanel("activity", hqActivity())}
      ${hqPanel("chess", hqChessTab(s))}
      ${hqPanel("office", `
        <div class="hq-stage hq-wide-card">
          <div class="hq-canvas-wrap">${hqStudioCanvasTag()}</div>
          <div class="hq-legend"><span style="--c:#ffd27a">A lit lamp: a turn running at that desk</span><span style="--c:#ffb347">Hourglass: a turn waiting to retry</span><span style="--c:#ef5a5f">Red sign: stuck, or a failed check</span><span style="--c:#45c486">Green tick: recovered</span></div>
          <p class="hq-note">Studio art: Pixel Agents (MIT) and MetroCity by JIK-A-4 (CC0); <a href="/hq-art/CREDITS.txt">credits</a>. Tap a desk, the board, the shelves or the chess table to look closer.</p>
        </div>
        ${hqOffice(s)}`)}
    </div>
    <p class="hq-note">Decisions land on your desk, and the kill switch is yours.</p>
  </div>`;
}

// What a member thinks with, in a word for the roster line.
function hqBrainTag(brain) {
  const b = String(brain || "local");
  return b.includes("opus") ? "Opus" : b.includes("sonnet") ? "Sonnet" : b.includes("haiku") ? "Haiku" : b.includes("deepseek") ? "DeepSeek" : b === "mac" ? "Mac" : "Local";
}

// What a member's turn waits for, in the owner's words, if one waits (ADR 0131): for budget,
// or for the Mac.
function hqWaitingFor(m) {
  const turn = ((HQ.snap && HQ.snap.runs) || []).find((r) => r.member === m.id && r.status === "queued" && /^waiting for /.test(r.why || ""));
  return turn ? turn.why : "";
}

// One member on the team board, on one line (the owner's notes of 31 Oct): its name, the model
// doing its work, what it is doing now and today's money. Tapping it opens the rest of its card
// (its role, both brains, its day's allowance, the brain buttons, a new name, a look at it).
function hqRosterLine(m) {
  const on = (m.running || []).map((r) => r.task ? "#" + r.task : "a look round").join(", ");
  const waits = hqWaitingFor(m);
  const doing = m.furloughed ? "on furlough" : `${m.paused ? "paused · " : ""}${on ? "on " + esc(on) : waits ? esc(waits) : "between turns"}`;
  const money = m.brain === "local" || m.brain === "mac" ? "costs nothing" : `${hqDollars(m.spent)} today`;
  const open = !!HQ.roster[m.id];
  const controls = open ? `<div class="hq-controls">${hqBackLink()}
      <p class="hq-who"><span class="hq-tag">${esc(hqBrainTag(m.brain) + (m.brain === "mac" && HQ.mac ? ` (${hqMacWord(HQ.mac)})` : ""))}</span> ${hqMemberMoney(m)}</p>${hqBrains(m)}
      <div class="hq-row"><button class="hq-btn" data-act="hq:rename:${m.id}">Rename</button>
        <button class="hq-btn" data-act="hq:pick:member:${m.id}">Look at ${esc(m.name)}</button></div></div>` : "";
  return `<li class="hq-member"><button class="hq-line" id="hq-member-${Number(m.id)}" aria-expanded="${open ? "true" : "false"}" data-act="hq:roster:${m.id}:toggle"><b>${esc(m.name)}</b>
      <small>${esc(hqBrainTag(m.brain))} · ${doing}</small> <span>${money}</span></button>${controls}</li>`;
}

// The team board: a line a member, no groups (ADR 0142: ranked models, no roles or reporting
// lines; the owner's notes of 31 Oct).
function hqRoster(s) {
  return `<ul class="hq-team hq-board">${(s.members || []).map(hqRosterLine).join("")}</ul>`;
}

// The Office card (ADR 0119, 0142): the office's games. The floor became the studio (the owner,
// 4 Nov); the members work and, in their downtime, play chess at its table.
function hqOffice(s) {
  const o = s.office;
  if (!o) return "";
  return `<section class="hq-card hq-office-card"><h2>Office</h2>
    <p class="hq-note">Downtime costs nothing: it is work on the house's own model. The chess table stands in the studio's lounge.</p>
    ${hqGames(s)}
  </section>`;
}

// ---- games in the office (ADR 0121) -----------------------------------------------------
// Ping-pong and pool the engine plays for nothing, and chess the members play in their
// downtime. Every name and every result is the team's own words, so all of it is escaped.

// Each with U+FE0E, the text form: ♟ is also an emoji, which a phone draws black on either side.
const HQ_PIECES = { k: "♚\uFE0E", q: "♛\uFE0E", r: "♜\uFE0E", b: "♝\uFE0E", n: "♞\uFE0E", p: "♟\uFE0E" };

function hqStreak(n) { return n > 0 ? `W${n}` : n < 0 ? `L${-n}` : "–"; }

// One game's leaderboard as a table: most wins first.
function hqLeaderboard(game, chess) {
  const rows = (game && game.leaderboard) || [];
  if (!rows.length) return `<p class="hq-empty">Nobody has played yet.</p>`;
  const head = chess ? "<th>Won</th><th>Lost</th><th>Drawn</th>" : "<th>Won</th><th>Lost</th><th>Run</th>";
  const body = rows.map((r) => `<tr><td>${esc(r.name)}</td><td class="n">${Number(r.wins)}</td><td class="n">${Number(r.losses)}</td>
    <td class="n">${chess ? Number(r.draws) : esc(hqStreak(Number(r.streak)))}</td></tr>`).join("");
  return `<table class="hq-board"><thead><tr><th>Player</th>${head}</tr></thead><tbody>${body}</tbody></table>`;
}

// A chess board of 64 squares from the position's eight rows; capitals are white's pieces. The
// squares in `lit` (row * 8 + file) are the last move's.
function hqChessBoard(c) {
  const lit = new Set(c.lit || []);
  const squares = (c.rows || []).map((row, r) => [...String(row)].map((ch, f) => {
    const piece = ch === "." ? "" : HQ_PIECES[ch.toLowerCase()] || "", white = ch !== "." && ch === ch.toUpperCase();
    return `<span class="${(r + f) % 2 ? "dark" : ""}${white ? " white" : ""}${lit.has(r * 8 + f) ? " lit" : ""}">${piece}</span>`;
  }).join("")).join("");
  return `<div class="hq-chess" role="img" aria-label="${esc(`Chess board, ${c.white} against ${c.black}: ${c.fen}`)}">${squares}</div>`;
}

// One game of chess: who plays, the board, whose move it is or the result, and the moves.
function hqChessGame(c) {
  const state = c.status === "in_progress" ? `${esc(c.to_move)} to move${c.check ? " · in check" : ""}` : esc(c.result);
  return `<div class="hq-sel">${esc(c.white)} (white) against ${esc(c.black)} (black)</div>${hqChessBoard(c)}
    <p>${state}</p><p class="hq-note">${c.move_text ? esc(c.move_text) : "No moves yet."}</p>`;
}

// What the chess table shows when tapped: the games being played and the last finished ones.
function hqChessPanel() {
  const games = (((HQ.snap || {}).games || {}).chess || {}).games || [];
  if (!games.length) return `<div class="hq-sel">Chess</div><p class="hq-empty">No game of chess yet. A member challenges another in their downtime.</p>`;
  return games.map(hqChessGame).join("");
}

// The Games section of the Office card: the three leaderboards, the last five results, and the
// games of chess being played, each a tap away from its board.
function hqGames(s) {
  const g = s.games;
  if (!g) return "";
  // Ping-pong and pool are the office's animation alone (ADR 0142): only chess is kept.
  const recentList = hqMore("results", (g.recent || []).filter((r) => r.game === "chess").map((r) => `<li>${esc(r.text)}</li>`), 3);
  const recent = recentList.shown;
  const replay = HQ.holo && HQ.holo.what === "replay" ? hqHoloDetail() : "";
  const playing = (((g.chess || {}).games) || []).filter((c) => c.status === "in_progress")
    .map((c) => `<li>${esc(c.white)} against ${esc(c.black)}: ${esc(c.to_move)} to move, ${(c.moves || []).length} moves
      <button class="hq-btn" data-act="hq:pick:chess:0">Look at the board</button></li>`).join("");
  return `<h3>Games</h3>
    <p class="hq-note">Chess is played by the members, a move in each downtime turn, and the model league plays in the Holodeck.</p>
    <details><summary>Chess</summary>${hqLeaderboard(g.chess, true)}</details>
    ${playing ? `<p class="hq-who">Playing now</p><ul class="hq-team">${playing}</ul>` : ""}
    ${recent ? `<p class="hq-who">Latest results</p><ul class="hq-team">${recent}</ul>${recentList.more}` : `<p class="hq-empty">No results yet.</p>`}
    ${replay}`;
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

// The owner hires (ADR 0115, 0142): a name and the model it is. HQ refuses a model it does not
// have; this only asks.
function hqHireForm() {
  const brains = [["local", "Local"], ["mac", "Mac"], ["claude-haiku-5-5", "Claude Haiku"], ["claude-sonnet-5-5", "Claude Sonnet"], ["claude-opus-5-5", "Claude Opus"], ["deepseek-flash", "DeepSeek Flash"]]
    .map(([v, l]) => `<option value="${v}">${l}</option>`).join("");
  return `<h3>Hire</h3><div class="hq-row">
      <input id="hq-hire-name" placeholder="Name" aria-label="The new member's name" />
      <label>thinks with <select id="hq-hire-brain" aria-label="The model the member is">${brains}</select></label>
      <button class="hq-btn ok" data-act="hq:hire:new:add">Hire</button></div>`;
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

// What a member thinks with (ADR 0109, 0142): its one model, of those HQ lists. The one it has
// is pressed; each other is a tap away.
function hqBrains(m) {
  const brains = m.brains || [];
  if (brains.length < 2) return "";
  const btn = (b) => `<button class="hq-btn${b.name === m.brain ? " ok" : ""}" aria-pressed="${b.name === m.brain ? "true" : "false"}"
    data-act="hq:brain:${m.id}:${esc(b.name)}">${esc(b.label)}</button>`;
  return `<div class="hq-row" role="group" aria-label="What ${esc(m.name)} thinks with">Brain: ${brains.map(btn).join("")}</div>`;
}

// A project's title, by its id.
function hqProjectTitle(id) {
  const p = ((HQ.snap && HQ.snap.projects) || []).find((x) => x.id === id);
  return p ? p.title : "#" + id;
}

// Projects (ADR 0113), grouped by the goal each serves: its lead and status, and this month's
// spend against its cap, amber from 80% and red once spent. The owner, who is the board, makes
// them, caps them, hands them work, pauses, resumes, drops and reopens them. Every word a
// member wrote (a proposal's title, its reason) is escaped.
const HQ_PROJECTS_SHOWN = 4;
function hqProjects(s) {
  const projects = s.projects || [];
  const goals = (s.goals || []).filter((g) => g.status === "active" || projects.some((p) => p.goal === g.id));
  const status = (p) => p.pause === "budget" ? "paused: it spent its cap" : p.pause === "owner" ? "paused by you"
    : p.status === "proposed" ? `proposed by ${p.created_by}, waits for a decision` : p.status;
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
      open ? btn("tools", "Tools", "") : "",
      ["proposed", "dropped", "done"].includes(p.status) ? btn("reopen", p.status === "proposed" ? "Start it" : "Reopen", "ok") : "",
      open || p.status === "proposed" ? btn("drop", "Drop", "no") : "",
    ].join("");
  };
  // One line each, the money in a few words; tapped, the whole of it and its buttons.
  const spent = (p) => p.cap ? `${hqDollars(p.spent)} of ${hqDollars(p.cap)}` : `${hqDollars(p.spent)}, no cap`;
  const row = (p) => hqOpenRow(`proj${p.id}`, esc(p.title),
    `${esc(hqName(p.lead))} · ${esc(status(p))}${p.open_tasks ? ` · ${p.open_tasks} open` : ""} · ${spent(p)}`,
    `<span>${esc(hqName(p.lead))} leads · ${esc(status(p))}${p.open_tasks ? ` · ${p.open_tasks} open` : ""}${p.asked_cap ? ` · asks ${hqDollars(p.asked_cap)} a month` : ""}</span>
      ${money(p)}${p.tools ? `<span>Tools: ${esc(p.tools.join(", ") || "none of the node's")}</span>` : ""}${p.reason ? hqClamp(`reason${p.id}`, p.reason) : ""}${(p.evidence || []).length ? `<span>Evidence: ${esc(p.evidence.join(", "))}</span>` : ""}
      <div class="hq-row">${acts(p)}</div>`);
  // Live projects first; finished ones (done, dropped) wait behind "Show all".
  const finished = (p) => ["done", "dropped"].includes(p.status) ? 1 : 0;
  // Routines are a kind of project here (the owner's notes of 31 Oct), each after its goal's
  // projects; they stay their own record (ADR 0107), so this changes only how they show. The
  // ones that are off wait behind one "Show the N that are off" for the card.
  const routines = s.routines || [];
  const off = routines.filter((r) => !r.enabled).length;
  const showOff = !!HQ.more.routinesoff;
  const routinesOf = (goal) => routines.filter((r) => r.goal === goal && (r.enabled || showOff)).map(hqRoutineRow).join("");
  const groups = goals.map((g) => {
    // A few per goal at rest, those with open work first (tabs climb, round 0: 28 live rows read as
    // a wall); the rest behind "Show all".
    const mine = projects.filter((p) => p.goal === g.id).sort((a, b) => finished(a) - finished(b) || (b.open_tasks || 0) - (a.open_tasks || 0));
    const list = hqMore(`projects${g.id}`, mine.map(row), HQ_PROJECTS_SHOWN);
    const rows = list.shown + routinesOf(g.id);
    return `<li><b>${esc(g.title)}</b><ul class="hq-team hq-projects">${rows || `<li class="hq-empty">No project yet.</li>`}</ul>${list.more}</li>`;
  }).join("");
  // A routine whose goal is not listed (dropped, or none) still shows, after the goals.
  const listed = new Set(goals.map((g) => g.id));
  const loose = routines.filter((r) => !listed.has(r.goal) && (r.enabled || showOff)).map(hqRoutineRow).join("");
  const offFold = off ? `<div class="hq-row"><button class="hq-btn hq-more" aria-expanded="${showOff}" data-act="hq:more:routinesoff:toggle">${showOff ? "Hide" : "Show"} the ${off} that ${off === 1 ? "is" : "are"} off</button></div>` : "";
  const goalOpts = (s.goals || []).filter((g) => g.status === "active").map((g) => `<option value="${g.id}">${esc(g.title)}</option>`).join("");
  // Any member at work may lead a project (ADR 0142): there are no titles to pick by.
  const leadOpts = (s.members || []).filter((m) => !m.furloughed)
    .map((m) => `<option value="${m.id}">${esc(m.name)}</option>`).join("");
  const who = (s.members || []).map((m) => `<option value="${m.id}">${esc(m.name)}</option>`).join("");
  return `<section class="hq-card"><h2>Projects</h2>
    <ul class="hq-team">${groups || `<li class="hq-empty">No goal yet: set the team a goal first.</li>`}</ul>
    ${loose ? `<ul class="hq-team hq-projects">${loose}</ul>` : ""}${offFold}
    ${hqFormFold("newproject", "New project", `<h3>New project</h3><div class="hq-row">
      <select id="hq-project-goal" aria-label="The goal it serves">${goalOpts}</select>
      <input id="hq-project-title" placeholder="e.g. Media stack upkeep" aria-label="What it is" />
      <select id="hq-project-lead" aria-label="Who leads it">${leadOpts}</select>
      <label>cap $<input id="hq-project-cap" inputmode="decimal" placeholder="none" style="width:5em" aria-label="Monthly cap in dollars, or none" /> a month</label>
      <button class="hq-btn ok" data-act="hq:project:new:add">Make it</button></div>`)}
    ${hqFormFold("newroutine", "Add a routine", `<h3>Add a routine</h3><div class="hq-row">
      <input id="hq-routine-title" placeholder="e.g. Check the home lab" aria-label="What it does" />
      <label>every <input id="hq-routine-hours" type="number" min="1" max="720" value="6" style="width:4em" aria-label="Every how many hours" /> hours</label>
      <select id="hq-routine-who" aria-label="Who does it">${who}</select>
      <button class="hq-btn ok" data-act="hq:routine:new:add">Add</button></div>`)}</section>`;
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

// Whether today's pool (the base day and the owner's top-up) has nothing left (ADR 0113 rule 6).
function hqPoolSpent(s) { const p = s.pool; return !!p && p.per_day != null && (p.per_day - (p.committed || 0)) <= 0; }

// What the work came to (ADR 0132), over the last day and the last week: the changes that
// landed, what one cost, talk against work, how often the desk interrupted, how much the memory
// is used, and what waits now. HQ computed every number; this only renders them. A week that
// landed nothing says "none landed", never a cost of nought.
function hqOutcomes(o) {
  if (!o || !o.day || !o.week) return "";
  const landed = (w) => `${w.landed} landed · ${w.submitted} submitted · ${w.checks_passed} checks passed`;
  const cost = (w) => (w.per_landed == null ? "none landed" : `${hqDollars(w.per_landed)} each`);
  const talk = (w) => (w.talk ? `${w.talk.coordination} : ${w.talk.product} turns · ${w.talk.posts} posts : ${w.talk.acts} acts` : "not measured yet");
  const desk = (w) => `${w.interrupts} interrupts (${w.interrupts_a_day} a day) · ${w.digest} in the digest`;
  const memory = (w) => {
    const m = w.memory, c = m.consults;
    return `${m.reinvented} reinvented · ${m.record_asked} asks of the record · ${c.asked} consults (${c.answered} answered, ${c.from_record} from the record)`;
  };
  const rows = hqScoreRows("outcome", [["Changes", landed], ["Cost of one", cost], ["Talk : work", talk], ["Desk", desk], ["Memory", memory]]
    .map(([k, f]) => [k, f(o.day), f(o.week)]), "1 day", "7 days");
  const w = o.waiting || {};
  // Turns that wait for the day to turn (ADR 0133) are said when there are any, so a calm team's
  // line stays short, and a day of turns alone is never "nothing waits".
  const waits = w.budget || w.mac || w.cap
    ? `${w.budget || 0} waiting for budget · ${w.mac || 0} for the Mac${w.cap ? ` · ${w.cap} for tomorrow` : ""} · the oldest has waited ${hqSpan(w.oldest_ms)}`
    : "nothing waits for budget or the Mac";
  return `<h3>Outcomes</h3>
    ${rows}
    <p class="hq-note">${esc(waits)}</p>`;
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
  const done = (w) => (w.finished.per_done == null ? "none done" : `${hqDollars(w.finished.per_done)} each · ${w.finished.done} done${w.finished.unverified ? `, ${w.finished.unverified} not verified` : ""}`);
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
  const measures = [["Turns", turns], ["Sent back", review], ["Per done task", done], ["Waste", dropped], ["Alarms", alarms], ["Desk cards", desk], ["Cache", cache]]
    .map(([k, f]) => [k, f(card.today), f(card.week)]);
  const members = (w) => (w.turns.members || []).map((m) => `<li>${esc(hqName(m.member))}: ${m.turns} turns · ${m.nothing} showed nothing${m.unlabelled ? ` · ${m.unlabelled} unlabelled` : ""} · ${hqDollars(m.spent)}</li>`).join("")
    || `<li class="hq-empty">No turns.</li>`;
  const when = (at) => new Date(at).toLocaleString([], { weekday: "short", hour: "2-digit", minute: "2-digit", hour12: false });
  const since = [["today", card.today.since], ["the 7 days", card.week.since]].filter(([, at]) => at != null)
    .map(([w, at]) => `the record for ${w} starts ${esc(when(at))}`).join("; ");
  // Today's pool and the owner's one-day top-up (ADR 0113 rule 6).
  const pl = card.pool;
  if (pl) measures.push(["Pool", `${hqDollars(pl.base)} base${pl.topped_up ? ` + ${hqDollars(pl.topped_up)} top-up` : ""}`, null]);
  return `<section class="hq-card"><h2>How the team is doing</h2>
    ${hqOutcomes(card.outcomes)}
    <h3>The team's turns</h3>
    ${hqScoreRows("measure", measures, "Today", "7 days")}
    <details><summary>Per member</summary><div class="hq-score-members">
      <div><b>Today</b><ul class="hq-team">${members(card.today)}</ul></div>
      <div><b>7 days</b><ul class="hq-team">${members(card.week)}</ul></div></div></details>
    ${since ? `<p class="hq-note">${since}: older turns and events are no longer kept.</p>` : ""}</section>`;
}

// The old house log (ADR 0111): read-only, as the house page is where the house is
// kept now (ADR 0122, 0142).
function hqHouse(s) {
  const log = s.house_log || "";
  if (!log.trim()) return "";
  return `<section class="hq-card"><h2>The old house log (read-only)</h2>
      <p class="hq-note">Its lines are on the house page's timeline, in the World card.</p>
      <pre class="hq-doc">${esc(log)}</pre></section>`;
}

// ---- The owner's Mac as a brain (ADR 0130) ---------------------------------------------------
// A member may think with the owner's Mac. Its helper tells the node every thirty seconds
// whether it is plugged in, how long the owner has been idle and whether its model is up; the
// node decides whether the Mac is free and tells HQ. The "Mac busy" switch is the owner's, and
// works from the phone: on, the model is unloaded and the team's turns on the Mac wait.

async function hqLoadMac() {
  try { HQ.mac = await api("GET", "/v1/mac"); } catch (_) { HQ.mac = null; }
}

// What is left with each paid model provider (the owner, 2026-10-30): DeepSeek's balance as it
// says it, Claude's as the owner's last figure less what Endora has paid since.
async function hqLoadCredits() {
  try { HQ.credits = await api("GET", "/v1/credits"); } catch (_) { HQ.credits = null; }
}

// The two credit chips under the title: a balance in red when it is below its threshold, and
// Claude's asking to be set until the owner has entered it.
function hqCreditChips() {
  const c = HQ.credits;
  if (!c) return [];
  const chips = [];
  if (c.deepseek) {
    const d = c.deepseek, cur = d.currency === "USD" ? "" : ` ${esc(d.currency)}`;
    chips.push(`<button class="hq-chip${d.low ? " low" : ""}" data-act="go:models" title="DeepSeek's balance as DeepSeek says it${d.low ? ": below your threshold, top it up on DeepSeek's platform" : ""}">DeepSeek ${hqDollars(d.left)}${cur} left</button>`);
  }
  if (c.claude) {
    const renews = c.claude_grant ? `. Your plan's ${hqDollars(c.claude_grant)} grant renews on the ${hqOrdinal(c.claude_renews_on)}` : "";
    chips.push(`<button class="hq-chip${c.claude.low ? " low" : ""}" data-act="go:models" title="The balance you entered from Anthropic's Console, less ${hqDollars(c.claude.spent_since)} Endora has paid since${renews}${c.claude.low ? ": below your threshold, top it up" : ""}">Claude ≈ ${hqDollars(c.claude.left)} left</button>`);
  } else {
    chips.push(`<button class="hq-chip" data-act="go:models" title="Anthropic does not say a balance: enter the one its Console shows, in Settings → AI">Claude credit: set it</button>`);
  }
  return chips;
}

// 1st, 2nd, 3rd, 4th … 11th, 12th, 13th … 21st: the day a plan renews.
function hqOrdinal(n) {
  const tens = n % 100, ones = n % 10;
  if (tens >= 11 && tens <= 13) return `${n}th`;
  return `${n}${ones === 1 ? "st" : ones === 2 ? "nd" : ones === 3 ? "rd" : "th"}`;
}

// The credits on the Settings screen: Claude's balance as the Console shows it, a plan's
// monthly grant (Claude Max's $100) with its renewal day and the credit bought on top, and the
// two thresholds below which the owner is told. The grant is spent first; on the renewal day
// the estimate becomes the whole grant plus what is left of the bought credit.
function hqCreditsSetup() {
  const c = HQ.credits;
  if (!c) return `<div class="card"><p class="sub">The node could not say what is left.</p></div>`;
  const d = c.deepseek, cl = c.claude;
  const deepseekNow = d ? `DeepSeek: ${hqDollars(d.left)}${d.currency === "USD" ? "" : " " + esc(d.currency)} left, as DeepSeek says it.`
    : c.has_deepseek ? "DeepSeek: its balance could not be read." : "DeepSeek: no DeepSeek provider with a key.";
  const pots = cl && c.claude_grant ? ` Of it, ${hqDollars(cl.grant_left)} is this month's grant and ${hqDollars(cl.purchased_left)} credit you bought.` : "";
  const claudeNow = cl ? `Claude: about ${hqDollars(cl.left)} left (your ${hqDollars(cl.set)}, less ${hqDollars(cl.spent_since)} paid since).${pots}`
    : "Claude: Anthropic does not say its balance; enter the one its Console's billing page shows.";
  const plan = c.claude_grant
    ? `Your plan's ${hqDollars(c.claude_grant)} grant renews on the ${hqOrdinal(c.claude_renews_on)} (next ${esc(c.claude_next_renewal || "")}), by your clock.`
    : "A plan such as Claude Max grants API credit each month: enter it and the day the plan renews.";
  return `<div class="card hq-credits">
      <div class="model-role">Credits left</div>
      <p class="sub">${deepseekNow}<br>${claudeNow}</p>
      <div class="row" style="flex-wrap:wrap;margin-top:8px;">
        <label>Claude's balance in the Console now $<input id="hq-credit-claude" type="number" min="0" step="0.01" placeholder="${cl ? (cl.left / 1e6).toFixed(2) : "e.g. 20.00"}" aria-label="Claude's balance in dollars" /></label>
        <label>tell me under $<input id="hq-credit-claude-low" type="number" min="0" step="0.5" value="${(c.claude_low / 1e6).toFixed(2)}" aria-label="Claude threshold in dollars" /></label>
        <label>DeepSeek under $<input id="hq-credit-deepseek-low" type="number" min="0" step="0.5" value="${(c.deepseek_low / 1e6).toFixed(2)}" aria-label="DeepSeek threshold in dollars" /></label></div>
      <p class="sub" style="margin-top:8px;">${plan}</p>
      <div class="row" style="flex-wrap:wrap;margin-top:8px;">
        <label>Monthly grant $<input id="hq-credit-claude-grant" type="number" min="0" step="1" placeholder="100.00"${c.claude_grant ? ` value="${(c.claude_grant / 1e6).toFixed(2)}"` : ""} aria-label="The plan's monthly grant in dollars, 0 for none" /></label>
        <label>renews on day <input id="hq-credit-claude-renews" type="number" min="1" max="31" step="1" placeholder="1–31"${c.claude_renews_on ? ` value="${c.claude_renews_on}"` : ""} aria-label="The day of the month the plan renews" /></label>
        <label>bought credit left $<input id="hq-credit-claude-purchased" type="number" min="0" step="0.01" placeholder="${cl && c.claude_grant ? (cl.purchased_left / 1e6).toFixed(2) : "0.00"}" aria-label="Credit you bought that is left, in dollars" /></label>
        <button class="primary" data-act="hq:credits:save:go">Save</button></div>
      <p class="sub" style="margin-top:8px;">The grant is spent before bought credit; on the renewal day the estimate becomes the whole grant plus the bought credit left, and the warning is armed again. Enter Claude's balance, or the credit you bought, again after each top-up. Below a threshold you are told once on the desk.</p></div>`;
}

async function hqCreditsAct() {
  const num = (id) => { const v = val(id); return v === "" ? null : Number(v); };
  const body = {};
  const claude = num("hq-credit-claude"), cl = num("hq-credit-claude-low"), dl = num("hq-credit-deepseek-low");
  if (claude != null) body.claude_balance = claude;
  if (cl != null) body.claude_low = cl;
  if (dl != null) body.deepseek_low = dl;
  const grant = num("hq-credit-claude-grant"), renews = num("hq-credit-claude-renews"), bought = num("hq-credit-claude-purchased");
  if (grant != null) body.claude_grant = grant;
  if (renews != null) body.claude_renews_on = renews;
  if (bought != null) body.claude_purchased = bought;
  try { HQ.credits = await api("POST", "/v1/credits", body); flash("Saved.", "ok"); }
  catch (e) { flash("Couldn't save them: " + e.message, "err"); }
  render();
  return true;
}

// What the node's few words mean to the owner.
function hqMacWhy(why) {
  return { ready: "Free: the team may use it", "owner active": "You are using it: the team waits until you have been idle a while", "on battery": "On battery: the model is unloaded",
    busy: "Mac busy is on: the model is unloaded", "model loading": "Plugged in; the model is loading", "not heard from": "Not heard from lately" }[why] || String(why || "");
}

// The Mac's state in a word or two, by its reason (the owner, 2026-10-25: "Mac away" when it was
// plugged in and only waiting for them to be idle read as broken).
function hqMacWord(m) {
  if (m.busy) return "busy";
  if (m.ready) return "free";
  return { "owner active": "in use", "on battery": "on battery", "model loading": "loading", "not heard from": "away" }[m.why] || "away";
}

// The Mac's card, in Settings → AI beside the house's model (the owner, 31 Oct: it had its own
// card on HQ): its state and why, the busy switch, the helper's token, the model's address, key
// and name, and how long the owner must be idle. The buttons are the same ones the node always had.
function hqMacSetup() {
  const m = HQ.mac;
  if (!m) return `<div class="card"><p class="sub">The node could not say about the Mac.</p></div>`;
  const hq = (HQ.snap && HQ.snap.mac) || {};
  const heard = m.heard_s_ago == null ? "never" : m.heard_s_ago < 90 ? `${m.heard_s_ago} s ago` : `${Math.round(m.heard_s_ago / 60)} min ago`;
  const facts = m.plugged == null ? "" : `${m.plugged ? "plugged in" : "on battery"} · idle ${Math.floor((m.idle_s || 0) / 60)} min · model ${m.model_up ? "up" : "down"} · `;
  const waiting = hq.waiting ? ` ${hq.waiting} ${hq.waiting === 1 ? "turn waits" : "turns wait"} for it.` : "";
  const running = hq.running ? " A turn is running on it." : "";
  const token = HQ.macToken ? `<p class="hq-who" role="status">The helper's token, shown this once: <code>${esc(HQ.macToken)}</code>
      Save it in the file the install names (ENDORA_MAC_TOKEN_FILE).</p>` : "";
  return `<div class="card model-card hq-macsetup" aria-live="polite">
      <div class="model-role">The Mac <span class="pill">${hqMacWord(m)}</span></div>
      <div class="sub" style="margin:-4px 0 0;">${esc(hqMacWhy(m.why))}.${esc(waiting)}${esc(running)}<br>${esc(facts)}heard ${esc(heard)}. ${Number(m.idle_minutes) > 0
        ? `A turn runs on it only when it is plugged in and you have been idle for ${Number(m.idle_minutes)} min`
        : "A turn runs on it whenever it is plugged in, whether or not you are using it"}; one at a time.</div>
      <div class="row" style="align-items:flex-start;gap:10px;">
        <div class="grow"><div class="title" style="font-weight:500;">Mac busy</div><div class="sub">On, the model is unloaded and the team's turns on the Mac wait.</div></div>
        <button class="${m.busy ? "primary" : "ghost"}" aria-pressed="${m.busy ? "true" : "false"}" data-act="hq:mac:busy:${m.busy ? "off" : "on"}">${m.busy ? "On" : "Off"}</button></div>
      <div class="row" style="flex-wrap:wrap;">
        <input id="hq-mac-url" value="${esc(m.url || "")}" placeholder="http://192.168.1.10:8080/v1" aria-label="The Mac's model address" />
        <input id="hq-mac-model" value="${esc(m.model || "")}" placeholder="Model name" aria-label="The model's name" />
        <input id="hq-mac-key" type="password" autocomplete="off" placeholder="${m.has_key ? "Key kept; type to replace it" : "Key (the server's --api-key)"}" aria-label="The server's key" />
        <label>wait until I am idle for <input id="hq-mac-idle" type="number" min="0" max="1440" value="${Number(m.idle_minutes) || 0}" aria-label="Minutes the owner must be idle (0: no wait)" /> min (0: no wait)</label>
        <button class="primary" data-act="hq:mac:save:go">Save</button></div>
      <div class="row"><button class="ghost" data-act="hq:mac:token:make">${m.has_token ? "Make a new token for the helper" : "Make a token for the helper"}</button></div>${token}
      <div class="sub">Teams on the Mac brain wait for the Mac and never fall back to another model.</div></div>`;
}

async function hqMacAct(what, arg) {
  if (what !== "token") HQ.macToken = "";
  if (what === "busy") {
    try { await api("POST", "/v1/mac/settings", { busy: arg === "on" }); flash(arg === "on" ? "Mac busy is on: the model will unload." : "Mac busy is off.", "ok"); }
    catch (e) { flash("Couldn't change it: " + e.message, "err"); }
  } else if (what === "save") {
    const body = { model: val("hq-mac-model"), idle_minutes: Math.max(0, Number(val("hq-mac-idle")) || 0) };
    const url = val("hq-mac-url");
    if (url) body.url = url;
    const key = val("hq-mac-key");
    if (key) body.api_key = key;
    try { await api("POST", "/v1/mac/settings", body); flash("The Mac's settings are saved.", "ok"); }
    catch (e) { flash("Couldn't save them: " + e.message, "err"); }
  } else if (what === "token") {
    if (HQ.mac && HQ.mac.has_token && !confirm("This replaces the helper's token: the Mac's helper stops until you save the new one on it. Go ahead?")) return true;
    try { HQ.macToken = (await api("POST", "/v1/mac/token", {})).token || ""; }
    catch (e) { flash("Couldn't make it: " + e.message, "err"); }
  }
  await hqLoadMac(); render();
  return true;
}

// What an approval will run, from what the node stored: the tool, its arguments one a line,
// which criterion of its task it serves, as HQ kept it when it was asked (ADR 0124), and the
// member's reason. Every word is the member's or a tool's, so all of it is escaped.
function hqApproval(a, serves) {
  if (!a) return "";
  const args = (a.args || []).map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("");
  const served = serves && serves.text ? `<dt>Serves</dt><dd>${serves.n ? `criterion ${Number(serves.n)}: ` : ""}${esc(serves.text)}</dd>` : "";
  return `<dl class="hq-kv"><dt>Runs</dt><dd>${esc(a.tool)}</dd>${args}${served}</dl>
    <p class="hq-who">${esc(a.member || "A member")} says why: ${esc(a.why)}</p>`;
}

// Accepted, waiting to merge (ADR 0115, amended 2026-10-31): each accepted branch HQ has not yet
// merged, one line each (task, branch, when, where it stands), folded at rest; and the owner's two
// switches: the merge cards back on the desk, and HQ merging develop itself.
function hqAccepted(s) {
  const rows = s.accepted || [];
  const d = s.delivery || {};
  const btn = (key, on, label) => `<button class="hq-btn ${on ? "ok" : "no"}" aria-pressed="${on ? "true" : "false"}"
    data-act="hq:delivery:${key}:${on ? "off" : "on"}">${label}: ${on ? "on" : "off"}</button>`;
  const when = (at) => at ? `${new Date(at).toLocaleDateString([], { month: "short", day: "numeric" })} ${hqClock(at)}` : "";
  // A reason most rows share is said once in the card, not on every row (tabs climb, round 1).
  const whys = rows.map((r) => r.why).filter(Boolean), tally = new Map();
  for (const w of whys) tally.set(w, (tally.get(w) || 0) + 1);
  const common = [...tally].sort((a, b) => b[1] - a[1])[0];
  const shared = common && common[1] > 2 ? common[0] : "";
  const lines = rows.map((r) => {
    const slug = String(r.branch || "").replace(/^hq\//, "");
    // A blocked item's kept submission (D22) is never pushed: its bundle is how it is reached.
    const bundle = (r.status === "accepted" && !r.can_push) || r.status === "kept_blocked"
      ? ` <button class="hq-link" data-act="hq:bundle:${Number(r.task)}:${esc(slug)}">bundle</button>` : "";
    // The branch is a tooltip: its name repeated the title in engine words.
    return `<li title="${esc(`${hqKey(r.task)} ${r.title} · ${r.branch}`)}"><b>${hqKey(r.task)} ${esc(r.title)}</b> <small>${esc(when(r.at))} · ${esc(r.said || r.status)}${r.why && r.why !== shared ? `: ${esc(r.why)}` : ""}</small>${bundle}</li>`;
  });
  const list = hqMore("acceptedlist", lines, 8);
  const body = `<section class="hq-card"><h2>Accepted, waiting to merge <span class="hq-count">${rows.length}</span></h2>
    ${shared ? `<p class="hq-note">Where a row says only "not pushed": ${esc(shared)}.</p>` : ""}
    ${lines.length ? `<ul class="hq-lines hq-accepted">${list.shown}</ul>${list.more}` : `<p class="hq-empty">Nothing waiting to merge.</p>`}
    <p class="hq-note">HQ pushes each accepted branch to your fork once its review, its check and a privacy scan pass; nothing personal leaves without a card on your desk.
      Merging develop stays off while the experiment's rounds run.</p>
    <div class="hq-row">${btn("merge_develop", !!d.merge_develop, "Merge into develop")}${btn("cards", !!d.cards, "Cards on my desk")}</div></section>`;
  const meta = rows.length ? `${rows.length} branch${rows.length === 1 ? "" : "es"}` : "none";
  return hqFold("accepted", "Accepted, waiting to merge", meta, body);
}

// What the team may do on the home lab: Endora's contracts (ADR 0106), and the owner's two
// switches. Each button offers the other way.
function hqContracts(c) {
  if (!c) return "";
  const btn = (key, on, label) => `<button class="hq-btn ${on ? "ok" : "no"}" aria-pressed="${on ? "true" : "false"}"
    data-act="hq:contracts:${key}:${on ? "off" : "on"}">${label}: ${on ? "on" : "off"}</button>`;
  return `<section class="hq-card"><h2>What the team may do on the home lab</h2>
    <p class="hq-note">Look at the home lab and its logs (secrets replaced), and restart a container: one an hour each, three in all.
      You are told of every restart. Endora's other skills are the team's too, as each skill's own setting says:
      on runs (three an hour, and you are told), ask puts it on your desk for approval, off is never offered.</p>
    <div class="hq-row">${btn("all", c.all, "Contracts")}${c.all ? btn("restart_container", c.restart_container, "Restarts") : ""}</div></section>`;
}

// Each active goal's notes from the owner: standing facts every member working towards it reads
// near the top of each briefing. Escaped like everything a person typed.

function hqNotes(s) {
  const goals = (s.goals || []).filter((g) => g.status === "active");
  const set = `<div class="hq-row"><input id="hq-goal" placeholder="e.g. Keep the home lab healthy" aria-label="A new goal for the team" /><button class="hq-btn ok" data-act="hq:goal">Set it</button></div>`;
  if (!goals.length) return `<section class="hq-card"><h2>Goals</h2>${set}</section>`;
  // Each goal is one line until tapped (the owner, 2026-10-25); open, it shows its notes as
  // they read, with Edit to change them in place.
  const goalRows = goals.map((g) => {
    const editing = !!HQ.more[`edit${g.id}`];
    const edit = `<div class="hq-row"><button class="hq-btn" aria-expanded="${editing}" data-act="hq:more:edit${g.id}:toggle">${editing ? "Done" : "Edit"}</button></div>`;
    const meta = g.note ? "notes" : "no notes";
    const body = editing
      ? `${edit}
      <textarea id="hq-note-${g.id}" rows="3" maxlength="2000" aria-label="Notes for the team on ${esc(g.title)}"
        placeholder="What every member working on this should know">${esc(g.note || "")}</textarea>
      <div class="hq-row"><button class="hq-btn ok" data-act="hq:note:${g.id}:save">Save notes</button></div>`
      : `${edit}
      <p class="hq-who">Notes for the team</p>${g.note ? hqClamp(`note${g.id}`, g.note) : `<p class="hq-empty">None.</p>`}`;
    return hqOpenRow(`goal${g.id}`, esc(g.title), meta, body);
  });
  const listed = hqMore("goals", goalRows, 5);
  return `<section class="hq-card"><h2>Goals</h2>${set}<p class="hq-note">Notes for the team: every member working on a goal reads them in its briefing.</p><ul class="hq-team">${listed.shown}</ul>${listed.more}</section>`;
}

// A routine (ADR 0107) as a row of the Projects card: work that comes round again, each a task
// for one member, marked as a routine. Shut, its rhythm, who and on or off; open, when it last
// ran and whether it takes the home lab's alarms, with on/off and "Run now". The form that adds
// one is at the foot of the Projects card.
function hqRoutineRow(r) {
  const when = (at) => new Date(at).toLocaleString([], { weekday: "short", hour: "2-digit", minute: "2-digit", hour12: false });
  return hqOpenRow(`routine${r.id}`, esc(r.title),
    `routine · ${esc(r.rhythm)} · ${esc(hqName(r.assignee))} · ${r.enabled ? "on" : "off"}`,
    `<span>${esc(r.rhythm)} · ${esc(hqName(r.assignee))} · ${r.last_fired_at ? "last ran " + esc(when(r.last_fired_at)) : "not run yet"}${r.last_task_open ? ` · ${hqKeyLink(r.last_task)} open` : ""}${r.on_lab_trouble ? " · takes home-lab alarms" : ""}${r.project ? ` · in ${esc(hqProjectTitle(r.project))}` : ""}</span>
      <div class="hq-row"><button class="hq-btn ${r.enabled ? "ok" : "no"}" aria-pressed="${r.enabled ? "true" : "false"}" data-act="hq:routine:${r.id}:${r.enabled ? "off" : "on"}">${r.enabled ? "On" : "Off"}</button>
        <button class="hq-btn" data-act="hq:routine:${r.id}:fire">Run now</button></div>`);
}

// What was tapped, in detail: a member's turn and budget, a task and its chain of why, your corner,
// the week's shelves, what the team knows (47f-hq-studio.js), or the chess table.

// What a working member is doing, in a line: its latest word on the task, else the task.
function hqDoing(m, t) {
  const last = (t.thread || []).filter((p) => p.author === m.name).pop();
  return last ? last.text : `on ${hqKey(t.id)}: ${t.title}`;
}
function hqSelected() {
  const sel = HQ.selected;
  if (!sel) return `<p class="hq-empty">Tap a worker, your corner, the shelves or the chess table.</p>`;
  if (sel.type === "chess") return hqChessPanel();
  if (sel.type === "shelf") return hqShelfPanel();
  if (sel.type === "corner") return hqCornerPanel();
  if (sel.type === "knowing") return hqKnowingPanel();
  if (sel.type === "goal") return hqGoalPanel();
  const why = (t) => `<ul class="hq-why">${(t.why || []).map((w) => `<li>${esc(w)}</li>`).join("")}</ul>`;
  if (sel.type === "member") {
    const m = hqMember(sel.id); if (!m) return "";
    const run = (m.running || [])[0], t = run && run.task ? hqTask(run.task) : null;
    return `<div class="hq-sel">${esc(m.name)}</div><dl class="hq-kv">
      <dt>Doing</dt><dd>${m.paused ? "Paused" : t ? esc(hqDoing(m, t)) : hqWaitingRun(m.id) ? "Its turn was interrupted; it waits to retry" : "Between turns"}</dd>
      ${hqTroubleSaid(m.id) ? `<dt>${(hqTrouble(m.id) || {}).kind === "recovered" ? "Recovered" : "Problem"}</dt><dd>${hqTroubleSaid(m.id)}</dd>` : ""}
      <dt>Run</dt><dd>${run ? "R" + run.run + (t ? " · " + hqKey(t.id) : "") : "none"}</dd>
      <dt>Brain</dt><dd>${esc(((m.brains || []).find((b) => b.name === m.brain) || { label: m.brain || "Local" }).label)}</dd>
      <dt>Budget</dt><dd>${hqMemberMoney(m)} · ${m.slots} at once</dd></dl>${t ? why(t) : ""}${hqTurns((r) => r.member === m.id, "member-" + m.id)}`;
  }
  const t = hqTask(sel.id); if (!t) return "";
  if (t.brief) hqAskWhole(t.id);
  const runner =((HQ.snap && HQ.snap.runs) || []).find((r) => r.task === t.id && r.status === "running");
  const thread = (t.thread || []).slice(-3).map((p) => `<li>${esc(p.author)}${p.unsupported ? ` <span class="hq-unsupported">unsupported</span>` : ""}: ${esc(p.text)}</li>`).join("");
  return `<div class="hq-sel">${hqKey(t.id)} · ${esc(t.title)}</div><dl class="hq-kv">
    <dt>Status</dt><dd>${t.closure && t.closure.why === "completed_unverified" ? "Completed · Not verified" : esc(String(t.status).replace("_", " "))}${t.gate === true ? " · gate passed" : t.gate === false ? " · gate failed" : ""}</dd>
    <dt>Owner</dt><dd>${t.owner ? esc(hqName(t.owner)) : "nobody"}</dd>
    <dt>Project</dt><dd>${hqTaskProject(t)}</dd>
    <dt>Lock</dt><dd>${runner ? "held by R" + runner.id : "free"}</dd>
    <dt>Blocked by</dt><dd>${(t.waits_on || []).length ? t.waits_on.map(hqKey).join(", ") : "nothing"}</dd>
    <dt>Depth</dt><dd>${t.depth || 0}</dd></dl>${why(t)}${hqEvidence(t)}${hqDocuments(t)}${hqTurns((r) => r.task === t.id, "task-" + t.id)}${thread ? `<ul class="hq-thread">${thread}</ul>` : ""}
    ${replayTaskLink(t.id)}<div class="hq-row">${workMapLink(t.id)}</div>
    <textarea id="hq-reply-${t.id}" rows="2" placeholder="Reply in the thread: whoever has it reads it now" aria-label="Your reply"></textarea>
    <div class="hq-row"><button class="hq-btn ok" data-act="hq:reply:${t.id}:send">Send</button></div>`;
}

// A task's project, and for open work a way to move it into another active one (ADR 0139):
// its lead, tools and money are then the task's.
function hqTaskProject(t) {
  const projects = (HQ.snap && HQ.snap.projects) || [];
  const now = projects.find((p) => p.id === t.project);
  const name = now ? esc(now.title) : "none";
  if (["done", "dropped"].includes(t.status)) return name;
  const into = projects.filter((p) => p.status === "active" && p.id !== t.project);
  if (!into.length) return name;
  const options = into.map((p) => `<option value="${p.id}">${esc(p.title)}</option>`).join("");
  return `${name} <select id="hq-move-${t.id}" aria-label="Move into project">${options}</select> <button class="hq-btn" data-act="hq:move:${t.id}:go">Move</button>`;
}

// A link in Activity (the owner, 1 Nov) opens what it names where it already lives: an item
// under Work's "Looking at", a member's line on the team board (Manage), a turn's replay (with
// what it was given; Watch opens it as it is). Where Activity was is kept, for the way back.
function hqFrom(kind, n) {
  if (!n || !["task", "member", "run", "watch"].includes(kind)) return true;
  HQ.back = { y: window.scrollY || 0, feed: HQ.feed };
  if (kind === "run" || kind === "watch") { REPLAY.given = kind === "run"; go("replay", `run-${n}`); return true; }
  HQ.feed = null;
  if (kind === "task") { HQ.selected = { type: "task", id: n }; HQ.from = "activity"; HQ.place = "detail"; }
  else { HQ.roster[n] = true; hqGo("manage"); }
  render();
  const el = document.getElementById(kind === "task" ? "hq-looking" : `hq-member-${n}`);
  if (el && el.scrollIntoView) el.scrollIntoView({ block: "start" });
  return true;
}

// Back to Activity, where it was: the tab or the paged view, scrolled as it was.
function hqBackToActivity() {
  const back = HQ.back || { y: 0, feed: null };
  HQ.back = null; HQ.feed = back.feed; hqGo("activity");
  if (NAV.v === "hq") render(); else go("hq");
  // A screen newly opened scrolls to its top first; this comes after.
  requestAnimationFrame(() => requestAnimationFrame(() => window.scrollTo(0, back.y)));
  return true;
}

// The way back to Activity, on what one of its links opened.
function hqBackLink() {
  return HQ.back ? `<div class="hq-row"><button class="hq-btn" data-act="hq:back">◀ Back to Activity</button></div>` : "";
}

// The verbs this screen uses. Returns true when it handled one.
async function hqDispatch(verb, noun, id, arg) {
  if (!(verb === "hq")) return false;
  if (noun === "pick") { hqLook({ type: id, id: Number(arg) }); render(); hqFocusTitle(); return true; }
  // A line of Activity opens what it names, and back returns to where Activity was.
  if (noun === "from") return hqFrom(id, Number(arg));
  if (noun === "back") return hqBackToActivity();
  // Find a task by its number or words of its title, and open it.
  if (noun === "find") {
    const asked = (document.getElementById("hq-find") || {}).value || "";
    const found = hqFindTask(asked);
    if (!found) { flash(asked.trim() ? `No task matches "${asked.trim()}".` : "Type a task's number or a few words of its title.", "warn"); return true; }
    hqLook({ type: "task", id: found }); render(); hqFocusTitle(); return true;
  }
  // The owner's Mac as a brain (ADR 0130): the busy switch, its settings, the helper's token.
  if (noun === "mac") return hqMacAct(id, arg);
  // What is left with each paid provider (the owner, 2026-10-30).
  if (noun === "credits") return hqCreditsAct();
  // The Holodeck and the office's replays (ADR 0135, 0121).
  if (noun === "holo") return hqHoloAct(id, arg);
  if (noun === "roster") {
    HQ.roster[id] = !HQ.roster[id]; render();
    return true;
  }
  if (noun === "more") { HQ.more[id] = !HQ.more[id]; render(); return true; }
  if (noun === "turns") { HQ.allTurns[id] = !HQ.allTurns[id]; render(); return true; }
  if (noun === "logmore") {
    HQ.logOpen[id] = !HQ.logOpen[id];
    // A post HQ cut in its event is read whole from its task's thread, asked of HQ once.
    const l = hqLogFind(id);
    if (HQ.logOpen[id] && l && l.whole && hqLogText(l) === l.text) hqAskWhole(l.whole.task);
    render(); return true;
  }
  // Activity's paged view (the owner, 31 Oct).
  if (noun === "feed") return hqFeedAct(id);
  if (noun === "decide") {
    // A question's answer, if the owner wrote one, goes in the task's thread before the member
    // wakes.
    const box = document.getElementById(`hq-answer-${id}`);
    const item = ((HQ.snap && HQ.snap.desk) || []).find((d) => d.id === Number(id));
    const amount = item && (item.kind === "project_cap" || item.kind === "project_budget" || item.kind === "out_for_day");
    const takesWords = arg === "seen" || amount;
    const answer = takesWords && box ? box.value.trim() : "";
    if (arg === "answer" && !answer) { flash("Write your answer first.", "warn"); return true; }
    if (arg === "raise" && !answer) { flash("Write the new cap first, in dollars a month.", "warn"); return true; }
    // A refused decision is said on the card itself, and stays there until the next try: a
    // toast that fades is easy to miss on a phone, and the card would look as if nothing
    // had been pressed.
    delete HQ.deskNote[id];
    try { await api("POST", `/v1/hq/desk/${id}`, { decision: arg, answer }); flash(answer ? "Answered." : "Decided.", "ok"); }
    catch (e) { HQ.deskNote[id] = "Couldn't decide: " + e.message; flash(HQ.deskNote[id], "err"); }
    await loadHQ(); render(); return true;
  }
  // The owner moves an open task into an active project.
  if (noun === "move") {
    const project = Number(val(`hq-move-${id}`)); if (!project) return true;
    try { await api("POST", `/v1/hq/tasks/${id}/project`, { project }); flash("Moved.", "ok"); }
    catch (e) { flash("Couldn't move it: " + e.message, "err"); }
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
    if (arg !== "local" && arg !== "mac" && !confirm(`${m.name} will think with ${b.label}: each turn is paid from the team's pool, and what ${m.name} reads goes to Anthropic (Confidential results included; Restricted ones stay home). Go ahead?`)) return true;
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
  // The owner hires (ADR 0115, 0142): HQ refuses a model it does not have.
  if (noun === "hire") {
    const name = val("hq-hire-name");
    const brain = (document.getElementById("hq-hire-brain") || {}).value || "local";
    if (!name) { flash("Give the new member a name.", "warn"); return true; }
    try { await api("POST", "/v1/hq/members", { name, brain }); flash(`${name} joined the team.`, "ok"); }
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
  // The owner adds to today's pool, for today only (ADR 0113 rule 6). HQ holds the limit.
  if (noun === "pool") {
    // Prefilled with one floor day, the unit HQ counts top-ups in (at most three).
    const base = ((((HQ.snap || {}).scorecard || {}).pool || {}).floor) || 0;
    const said = prompt("Dollars to add to today's pool (gone tomorrow; at most three pool days in all):", (base / 1e6).toFixed(2));
    if (said === null) return true;
    const micros = Math.round(parseFloat(said) * 1e6);
    if (!Number.isFinite(micros) || micros <= 0) { flash("That is not an amount in dollars.", "err"); return true; }
    if (!confirm(`Add ${hqDollars(micros)} to today's pool? It is gone tomorrow.`)) return true;
    try { await api("POST", "/v1/hq/pool/top-up", { amount_micros: micros }); flash(`Today's pool is up by ${hqDollars(micros)}.`, "ok"); }
    catch (e) { flash("Couldn't top the pool up: " + e.message, "err"); }
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
      } else if (arg === "tools") {
        // What its work is given, commas between: "team" keeps each member's own HQ tools and
        // narrows only the node's; empty takes the list away, giving its work everything.
        const said = prompt(`The tools ${p.title}'s work is given, commas between (team keeps the members' own; empty: everything):`, (p.tools || []).join(", "));
        if (said === null) return true;
        const tools = said.split(",").map((t) => t.trim()).filter(Boolean);
        await api("POST", `/v1/hq/projects/${id}/tools`, { tools: tools.length ? tools : null }); flash(tools.length ? "Tools set." : "Its work is given everything again.", "ok");
      } else if (arg === "work") {
        const title = (prompt(`What should the team do in ${p.title}?`) || "").trim(); if (!title) return true;
        // The team starts from what done means (ADR 0124), here as in the Give work form: one line
        // is enough, and "live:" before it marks it as about the lab now.
        const done = (prompt(`What does done mean for it? One line (start with live: if it is about the lab as it is now).`) || "").trim();
        if (!done) { flash("Say what done means: at least one line, so the team knows when it is finished.", "warn"); return true; }
        await api("POST", "/v1/hq/tasks", { goal: p.goal, project: p.id, title, criteria: [done] }); flash("On the loop's plan.", "ok");
      } else {
        if (arg === "drop" && !confirm(`Drop ${p.title}? Its open tasks are dropped with it.`)) return true;
        const done = await api("POST", `/v1/hq/projects/${id}`, { action: arg }); flash(`${p.title} is ${done.status}.`, "ok");
      }
    } catch (e) { flash("Couldn't do it: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  // The team's world (ADR 0122): make a piece canon, demote canon, or download it all.
  if (noun === "world") return hqWorldAct(id, arg);

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
  // The owner's switches for accepted work (ADR 0115, amended 2026-10-31): the cards back on
  // the desk, and merging develop.
  if (noun === "delivery") {
    if (!["cards", "merge_develop"].includes(id)) return true;
    try { await api("POST", "/v1/hq/delivery", { [id]: arg === "on" }); flash(arg === "on" ? "Switched on." : "Switched off.", "ok"); }
    catch (e) { flash("Couldn't switch it: " + e.message, "err"); }
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
  // A place (M6), and a link kept from the old tabs, mapped to where that tab's things now live.
  // "decisions" goes to the map and brings your decisions into view.
  if (noun === "place" || noun === "tab") {
    const to = noun === "tab" ? HQ_TAB_MOVED[id] || id : id;
    if (!HQ_PLACES.some(([p]) => p === to)) return true;
    HQ.back = null; hqGo(to); render();
    if (arg === "decisions") { const el = document.getElementById("hq-decisions"); if (el && el.scrollIntoView) { el.scrollIntoView({ block: "start" }); if (el.focus) el.focus(); } }
    else hqFocusTitle();
    return true;
  }
  // Hand the team a piece of work, under the first goal it is working towards, to the lead the
  // owner picks: a form on the screen, not pop-ups.
  if (noun === "work") {
    if (id === "cancel") { HQ.working = false; HQ.workDraft = {}; render(); return true; }
    if (id !== "send") { HQ.working = !HQ.working; render(); return true; }
    const field = (name) => { const el = document.getElementById(name); return el && el.value ? String(el.value).trim() : ""; };
    const title = field("hq-work-title");
    if (!title) { flash("Say what the team should do.", "warn"); return true; }
    const s = HQ.snap || {};
    const loop = (s.projects || []).find((p) => p.id === Number(field("hq-work-loop")) && p.status === "active");
    if (!loop) { flash("Pick a loop for it.", "warn"); return true; }
    // What done means, at least one line (ADR 0114, 0124): one a line or separated by semicolons,
    // "live:" before one about the lab as it is now. The team starts from it, so nothing is sent
    // without it, and the form stays open with what was typed.
    const criteria = field("hq-work-done").split(/[;\n]/).map((c) => c.trim()).filter(Boolean);
    if (!criteria.length) { flash("Say what done means: at least one line, so the team knows when it is finished.", "warn"); return true; }
    try { await api("POST", "/v1/hq/tasks", { goal: loop.goal, project: loop.id, title, criteria }); flash("On the loop's plan.", "ok"); }
    catch (e) { flash("Couldn't hand it over: " + e.message, "err"); return true; }
    HQ.working = false; HQ.workDraft = {};
    await loadHQ(); render(); return true;
  }
  return false;
}
