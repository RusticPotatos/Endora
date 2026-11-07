// ---- Endora HQ: the agents map ---------------------------------------------------------------
// The Core team at a glance (the owner's decision M5, 7 Nov 2026): what runs now and what waits
// on you first; then you and your desk by project, and below, each member with the open items it
// holds, grouped by project. Read-only: it reads `/v1/hq/team-map`, HQ's projection of its own
// records, refreshed with HQ's 3-second poll. A line moves only while a turn runs and was heard
// from recently; a silent one stands still and says since when. An item opens its Live Work map.

const AGENTS = { map: null, error: "" };

async function loadAgents() {
  AGENTS.error = "";
  try { AGENTS.map = await api("GET", "/v1/hq/team-map"); }
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

function agItem(i) {
  const cls = `ag-item ag-${i.status === "blocked" ? "bad" : i.status === "in_review" ? "review" : "calm"}${i.asks_owner ? " ag-asks" : ""}`;
  return `<button class="${cls}" data-act="live:task:${Number(i.task)}" title="${esc(i.title)}"><b>#${Number(i.task)}</b>${i.asks_owner ? " ?" : ""} ${esc(i.title)}</button>`;
}

function agMember(m) {
  const live = (m.live || []).map((l) => `<div class="ag-live${l.moving ? " ag-moving" : ""}"><span class="ag-wire"></span>
    <button class="hq-link" data-act="replay:run:${Number(l.run)}">R${Number(l.run)}</button>${l.task ? ` on <button class="hq-link" data-act="live:task:${Number(l.task)}">#${Number(l.task)}</button>` : ""}
    ${l.moving ? "" : `<small>no signal since ${esc(agWhen(l.heard))}</small>`}</div>`).join("");
  const seen = m.last_seen ? `last seen ${esc(agWhen(m.last_seen))}` : "not seen yet";
  const work = m.last_work ? ` · last work ${esc(agWhen(m.last_work.at))}${m.last_work.task ? ` on #${Number(m.last_work.task)}` : ""}` : "";
  const groups = (m.groups || []).map((g) => `<p class="ag-proj">${esc(g.project)}</p>${g.items.map(agItem).join("")}`).join("");
  return `<article class="ag-card ag-${esc(m.state)}"><span class="ag-stem"></span>
    <h3><span class="ag-dot"></span>${esc(m.name)}</h3><p class="ag-sub">${esc(m.brain)} · ${esc(AG_STATE[m.state] || m.state)}</p>
    <p class="ag-sub">${seen}${work}</p>${live}${groups ? `<div class="ag-items">${groups}</div>` : ""}</article>`;
}

function viewAgents() {
  const back = `<button class="hq-btn" data-act="go:hq">◀ Back to HQ</button>`;
  const shell = (inner) => `<div class="hq hq-agents"><div class="hq-bar"><h1>Agents map</h1><span class="hq-grow"></span><div class="hq-row">${back}</div></div>${inner}</div>`;
  if (AGENTS.error) return shell(`<section class="hq-card"><p class="hq-empty">${esc(AGENTS.error)}</p></section>`);
  const m = AGENTS.map;
  if (!m) return shell(`<p class="hq-empty">Reading the record…</p>`);
  const members = m.members || [];
  const away = members.filter((x) => ["furloughed", "paused", "out_for_today"].includes(x.state));
  const here = members.filter((x) => !away.includes(x));
  const word = (k) => String(k).replace(/_/g, " ");
  // Each project is its own group, as HQ keeps them; past the four biggest, the rest is one chip.
  const chip = (g) => `<span class="ag-chip">${g.count} ${esc(g.project)} (${g.kinds.map((k) => `${k.count} ${esc(word(k.kind))}`).join(", ")})</span>`;
  const groups = m.desk || [], rest = groups.slice(4);
  const desk = groups.slice(0, 4).map(chip).join("") + (rest.length ? `<span class="ag-chip" title="${esc(rest.map((g) => `${g.count} ${g.project}`).join("; "))}">+${rest.length} more project${rest.length === 1 ? "" : "s"} (${rest.reduce((n, g) => n + g.count, 0)} cards)</span>` : "");
  const unheld = (m.unheld || []).length ? `<article class="ag-card ag-idle"><span class="ag-stem"></span><h3>Held by nobody</h3><div class="ag-items">${m.unheld.map(agItem).join("")}</div></article>` : "";
  const awayRow = away.length ? `<p class="ag-away"><b>Away:</b> ${away.map((x) => `${esc(x.name)} (${esc(x.state === "furloughed" ? "Furloughed" : AG_STATE[x.state])})`).join(" · ")}</p>` : "";
  return shell(`<section class="hq-card ag">
    <div class="ag-hero${m.running ? " ag-busy" : ""}"><b>${esc(m.said)}</b>
      <small>${m.running ? `${m.running} running, ${m.queued} waiting to start.` : `No turn is running${m.queued ? `; ${m.queued} waiting to start` : " or waiting"}, so no line moves.`} ${m.blocked} open items blocked. Downtime turns are not work and are not drawn.</small></div>
    <div class="ag-owner"><h3><span class="ag-dot"></span>You</h3><p class="ag-sub">${m.desk_total} card${m.desk_total === 1 ? "" : "s"} on your desk${m.desk_oldest ? `, oldest ${esc(agWhen(m.desk_oldest))}` : ""}</p><div>${desk}</div></div>
    <div class="ag-team">${here.map(agMember).join("")}${unheld}</div>${awayRow}
    <p class="hq-note">Read-only, from HQ's record; it updates with HQ every few seconds. A moving line is a turn running now; still means idle; red is blocked, a "?" item asks you. Tap an item for its Live Work map.</p></section>`);
}

async function agentsDispatch(verb, noun) {
  if (!(verb === "agents")) return false;
  if (noun === "open") go("agents");
  return true;
}
