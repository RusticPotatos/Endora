// ---- Endora HQ: the Live Work map ----------------------------------------------------------
// One item's path as HQ recorded it (the owner's decision M3, 7 Nov 2026; proposal v3 §14): what
// it is now, what waits on you first, then one strip of its steps in time order, left to right
// (top to bottom on a phone), with your decisions above it and a stall drawn as a hatched gap.
// Read-only: it reads `/v1/hq/tasks/{id}/live`, HQ's projection of its own records, and changes
// nothing. A step opens what it rests on: its records and the turns that led to it, in Replay.

const WORKMAP = {
  id: 0,        // the item's id
  map: null,    // HQ's map of it; null while loading
  sel: -1,      // the step opened; -1 picks the stall, else the first failure, else the last
  error: "",
};

async function loadWorkMap(id) {
  const [kind, n] = String(id || "").split("-");
  if (kind !== "task" || !Number(n)) { WORKMAP.error = "Nothing to map."; return; }
  if (WORKMAP.id !== Number(n)) Object.assign(WORKMAP, { id: Number(n), map: null, sel: -1 });
  WORKMAP.error = "";
  try { WORKMAP.map = await api("GET", `/v1/hq/tasks/${Number(n)}/live`); }
  catch (e) { WORKMAP.map = null; WORKMAP.error = "HQ did not answer: " + String((e && e.message) || e); }
}

function workMapLink(task) { return `<button class="hq-btn" data-act="live:task:${Number(task)}">Live work map</button>`; }

// A step's moment as the strip says it: 18 Oct 09:14.
function wmWhen(at) {
  return new Date(at).toLocaleString([], { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit", hour12: false });
}

// The step opened at rest: the stall, else the first failure, else the latest.
function wmOpened(steps) {
  if (WORKMAP.sel >= 0 && WORKMAP.sel < steps.length) return WORKMAP.sel;
  const stall = steps.findIndex((s) => s.kind === "stall");
  if (stall >= 0) return stall;
  const bad = steps.findIndex((s) => s.tone === "bad");
  return bad >= 0 ? bad : steps.length - 1;
}

function wmIds(ids) {
  return (ids || []).length ? `<div class="lw-ids">${ids.map((x) => `<span>${esc(x)}</span>`).join(" ")}</div>` : "";
}

function wmStep(s, i, prev, opened, firstAfter) {
  const mark = { ok: "✓", bad: "✕", open: "?", owner: "•", calm: "•", warn: "!" }[s.tone] || "•";
  const chip = s.owner ? `<span class="lw-chip lw-chip-${s.tone === "warn" ? "sys" : "owner"}">${esc(s.owner)}</span>` : "";
  const when = s.kind === "stall" ? `${wmWhen(s.at)} → ${wmWhen(s.until)}` : wmWhen(s.at);
  const cls = ["lw-st", `lw-${s.tone}`, s.kind === "stall" ? "lw-gap" : "", firstAfter ? "lw-after" : "", i === opened ? "lw-sel" : ""].filter(Boolean).join(" ");
  return `<li class="${cls}" style="--i:${i};--from:var(--lw-${prev ? prev.tone : s.tone})">
    <div class="lw-own">${chip}</div>
    <div class="lw-rail"><span class="lw-line"></span>${s.kind === "stall" ? "" : `<span class="lw-node">${mark}</span>`}</div>
    <button class="lw-txt" aria-expanded="${i === opened}" data-act="live:step:${i}">
      <span class="lw-when">${esc(when)}${s.gap && s.kind !== "stall" ? ` <span class="lw-cue">${esc(s.gap)}</span>` : ""}</span>
      <span class="lw-what">${esc(s.label)}</span>${s.sub ? `<span class="lw-sub">${esc(s.sub)}</span>` : ""}${wmIds(s.ids)}
    </button></li>`;
}

function wmDetail(s) {
  const turns = (s.runs || []).map((r) => `<button class="hq-link" data-act="replay:run:${Number(r)}">R${Number(r)}</button>`).join(" ");
  const when = s.kind === "stall" ? `${wmWhen(s.at)} to ${wmWhen(s.until)}` : wmWhen(s.at);
  return `<li class="lw-detail lw-${s.tone}" aria-live="polite"><h3>${esc(s.label)}</h3>
    <p class="hq-who">${esc(when)}${s.owner ? ` · you: ${esc(s.owner)}` : ""}</p>
    ${s.sub ? `<p>${esc(s.sub)}</p>` : ""}${wmIds(s.ids)}
    ${turns ? `<p class="hq-who">Turns since the step before: ${turns}</p>` : ""}</li>`;
}

function viewWorkMap() {
  const back = HQ.back
    ? `<button class="hq-btn" data-act="hq:back">◀ Back to Activity</button>`
    : `<button class="hq-btn" data-act="go:hq">◀ Back to HQ</button>`;
  const shell = (title, inner) => `<div class="hq hq-live"><div class="hq-bar"><h1>${title}</h1><span class="hq-grow"></span><div class="hq-row">${back}</div></div>${inner}</div>`;
  if (WORKMAP.error) return shell("Live work", `<section class="hq-card"><p class="hq-empty">${esc(WORKMAP.error)}</p></section>`);
  const m = WORKMAP.map;
  if (!m) return shell("Live work", `<p class="hq-empty">Reading the record…</p>`);
  const steps = m.steps || [];
  const tone = { completed: "ok", working: "ok", blocked: "bad", unknown: "open", waiting: "warn", done_unverified: "open", closed: "calm" }[m.state] || "calm";
  const silent = m.state === "unknown" && m.silent_since ? ` · no signal since ${esc(wmWhen(m.silent_since))}` : "";
  const waiting = (m.attention || []).map((d) => `<li><b>Waiting on you</b> · ${esc(d.text)} <span class="lw-ids"><span>#${Number(d.id)}</span></span></li>`).join("");
  const verdict = `<div class="lw-verdict"><span class="lw-v lw-${tone}">${esc(m.said)}${silent}</span>
    ${(m.open || []).map((o) => `<span class="lw-v lw-open">${esc(o)}</span>`).join("")}
    ${waiting ? "" : `<span class="lw-v lw-calm">Nothing waits on you</span>`}</div>`;
  const decided = steps.filter((s) => s.owner && s.owner !== "Waiting on you").length;
  const people = [decided ? `<b>Owner</b> answered ${decided} card${decided === 1 ? "" : "s"}` : ""]
    .concat((m.people || []).map((p) => `<b>${esc(p.name)}</b> ${p.turns} turn${p.turns === 1 ? "" : "s"}${p.failed ? ` (${p.failed} failed)` : ""}`))
    .filter(Boolean).join(" · ");
  const opened = wmOpened(steps);
  const closedAt = steps.findIndex((s) => s.kind === "closed");
  const items = steps.map((s, i) => wmStep(s, i, steps[i - 1], opened, closedAt >= 0 && i === closedAt + 1));
  if (opened >= 0 && steps[opened]) items.splice(opened + 1, 0, wmDetail(steps[opened]));
  const open = (m.open || []).length ? `<p class="lw-note"><b>Open:</b> ${m.open.map(esc).join("; ")}.</p>` : "";
  const t = m.task || {};
  return shell(`Live work ${hqKey(t.id)}`, `<section class="hq-card lw">
    <p class="hq-who">${esc(t.title || "")}</p>
    ${waiting ? `<ul class="lw-waiting">${waiting}</ul>` : ""}${verdict}
    ${people ? `<p class="lw-people">${people}</p>` : ""}
    <ol class="lw-strip">${items.join("")}</ol>${open}
    <p class="hq-note">Read-only, from HQ's record. Green went as it should, red failed, amber stalled or raised by HQ, blue is you, a hollow mark is unresolved.</p>${replayTaskLink(t.id)}</section>`);
}

async function workMapDispatch(verb, noun, id) {
  if (!(verb === "live")) return false;
  if (noun === "task") { go("live", `task-${Number(id)}`); return true; }
  if (noun === "step") { WORKMAP.sel = Number(id); render(); return true; }
  return true;
}
