// ---- Endora HQ: the agents map ---------------------------------------------------------------
// The Core team at a glance (the owner's decision M5, 7 Nov 2026), and HQ's first screen (M6):
// what runs now and what waits on you first; then you and your desk by project, and below, each
// member with the open items it holds, grouped by project. It reads `/v1/hq/team-map`, HQ's
// projection of its own records, with HQ's snapshot and its poll, and changes nothing: the only
// controls beside it are the decisions and the bar's, each the node's own route. A line moves only
// while a turn runs and was heard from recently; a silent one stands still and says since when.
// An item opens its Live Work map, a member its Looking at page.

const AGENTS = { map: null, error: "" };

async function loadAgents() {
  AGENTS.error = "";
  try {
    const got = await api("GET", "/v1/hq/team-map");
    // An older HQ answers this path with something else: say so rather than draw it.
    if (!got || !Array.isArray(got.members)) throw new Error("its map is not one this console reads");
    AGENTS.map = got;
  }
  catch (e) { AGENTS.map = null; AGENTS.error = "HQ did not answer: " + String((e && e.message) || e); }
}

// A moment as the map says it: today's as 16:42, older ones with the day.
function agWhen(at) {
  const d = new Date(at), today = new Date().toDateString() === d.toDateString();
  return d.toLocaleString([], today ? { hour: "2-digit", minute: "2-digit", hour12: false }
    : { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit", hour12: false });
}

const AG_STATE = { working: "working", unknown: "running, silent", queued: "waiting to start", idle: "idle",
  out_for_today: "out for today", paused: "paused", furloughed: "furloughed" };

// How many of a member's items show before "All N items".
const AG_ITEMS_SHOWN = 6;

function agItem(i) {
  const cls = `ag-item ag-${i.status === "blocked" ? "bad" : i.status === "in_review" ? "review" : "calm"}${i.asks_owner ? " ag-asks" : ""}`;
  return `<button class="${cls}" data-act="live:task:${Number(i.task)}" title="${esc(i.title)}"><b>#${Number(i.task)}</b>${i.asks_owner ? " ?" : ""} ${esc(i.title)}</button>`;
}

// A member's name opens its Looking at page when HQ's snapshot knows it by that name.
function agName(name) {
  const m = ((HQ.snap && HQ.snap.members) || []).find((x) => x.name === name);
  return m ? `<button class="hq-link" data-act="hq:pick:member:${Number(m.id)}">${esc(name)}</button>` : esc(name);
}

function agMember(m) {
  const live = (m.live || []).map((l) => `<div class="ag-live${l.moving ? " ag-moving" : ""}"><span class="ag-wire"></span>
    <button class="hq-link" data-act="replay:run:${Number(l.run)}">R${Number(l.run)}</button>${l.task ? ` on <button class="hq-link" data-act="live:task:${Number(l.task)}">#${Number(l.task)}</button>` : ""}
    ${l.moving ? "" : `<small>no signal since ${esc(agWhen(l.heard))}</small>`}</div>`).join("");
  const seen = m.last_seen ? `last seen ${esc(agWhen(m.last_seen))}` : "not seen yet";
  const work = m.last_work ? ` · last work ${esc(agWhen(m.last_work.at))}${m.last_work.task ? ` on #${Number(m.last_work.task)}` : ""}` : "";
  // A member holding many items shows its first few and the rest on a tap, so one long list does
  // not push the rest of the team off the screen; the count is never hidden.
  const key = "ag-" + String(m.name).replace(/[^A-Za-z0-9]/g, "_"), open = !!HQ.more[key];
  let left = open ? Infinity : AG_ITEMS_SHOWN, held = 0;
  const groups = (m.groups || []).map((g) => {
    held += g.items.length;
    const shown = g.items.slice(0, Math.max(0, left)); left -= shown.length;
    return shown.length ? `<p class="ag-proj">${esc(g.project)}</p>${shown.map(agItem).join("")}` : "";
  }).join("") + (held > AG_ITEMS_SHOWN ? `<button class="hq-link ag-more" aria-expanded="${open}" data-act="hq:more:${esc(key)}:toggle">${open ? "Show fewer" : `All ${held} items`}</button>` : "");
  return `<article class="ag-card ag-${esc(m.state)}"><span class="ag-stem"></span>
    <h3><span class="ag-dot"></span>${agName(m.name)}</h3><p class="ag-sub">${esc(m.brain)} · ${esc(AG_STATE[m.state] || m.state)}</p>
    <p class="ag-sub">${seen}${work}</p>${live}${groups ? `<div class="ag-items">${groups}</div>` : ""}</article>`;
}

// The map, as the first thing on HQ's map place: never blank while it is read or when HQ did
// not answer, since your decisions stand beside it either way.
function hqMap() {
  const shell = (inner) => `<section class="hq-card hq-agents ag" aria-label="The team at work">${inner}</section>`;
  if (AGENTS.error) return shell(`<p class="hq-empty">${esc(AGENTS.error)}</p>`);
  const m = AGENTS.map;
  if (!m) return shell(`<p class="hq-empty">Reading the record…</p>`);
  const members = m.members || [];
  const away = members.filter((x) => ["furloughed", "paused", "out_for_today"].includes(x.state));
  const here = members.filter((x) => !away.includes(x));
  const word = (k) => String(k).replace(/_/g, " ");
  // Each project is its own group, as HQ keeps them; past the four biggest, the rest is one chip.
  const chip = (g) => `<span class="ag-chip">${g.count} ${esc(g.project)} (${(g.kinds || []).map((k) => `${k.count} ${esc(word(k.kind))}`).join(", ")})</span>`;
  const groups = m.desk || [], rest = groups.slice(4);
  const desk = groups.slice(0, 4).map(chip).join("") + (rest.length ? `<span class="ag-chip" title="${esc(rest.map((g) => `${g.count} ${g.project}`).join("; "))}">+${rest.length} more project${rest.length === 1 ? "" : "s"} (${rest.reduce((n, g) => n + g.count, 0)} cards)</span>` : "");
  const answer = m.desk_total ? `<button class="hq-link" data-act="hq:place:map:decisions">Answer them</button>` : "";
  const unheld = (m.unheld || []).length ? `<article class="ag-card ag-idle"><span class="ag-stem"></span><h3>Held by nobody</h3><div class="ag-items">${m.unheld.map(agItem).join("")}</div></article>` : "";
  const awayRow = away.length ? `<p class="ag-away"><b>Away:</b> ${away.map((x) => `${esc(x.name)} (${esc(x.state === "furloughed" ? "Furloughed" : AG_STATE[x.state])})`).join(" · ")}</p>` : "";
  return shell(`
    <div class="ag-hero${m.running ? " ag-busy" : ""}"><b>${esc(m.said)}</b>
      <small>${m.running ? `${m.running} running, ${m.queued} waiting to start.` : `No turn is running${m.queued ? `; ${m.queued} waiting to start` : " or waiting"}, so no line moves.`} ${m.blocked} open items blocked. Downtime turns are not work and are not drawn.</small></div>
    <div class="ag-owner"><h3><span class="ag-dot"></span>You</h3><p class="ag-sub">${m.desk_total} card${m.desk_total === 1 ? "" : "s"} on your desk${m.desk_oldest ? `, oldest ${esc(agWhen(m.desk_oldest))}` : ""} ${answer}</p><div>${desk}</div></div>
    <div class="ag-team">${here.map(agMember).join("")}${unheld}</div>${awayRow}
    <p class="hq-note">From HQ's record, every few seconds. A moving line is a turn running now; still means idle; red is blocked, a "?" item asks you. Tap an item for its Live Work map, a name for the member.</p>`);
}

// The agents map's old way in (M5's bar button) opens HQ's map.
async function agentsDispatch(verb, noun) {
  if (!(verb === "agents")) return false;
  hqGo("map");
  go("hq");
  return true;
}
