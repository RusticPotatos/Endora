// ---- The agenda loop (ADR 0103): the Now line, pinned cards, and what Endora watches -----------
// The node decides; this only shows what it decided and sends the owner's answers back. Items are
// drawn here, never written into the chat: chat is history a model may later read, and what the
// agenda says about mail stays in the house.

let AGENDA = null;        // /v1/agenda: the Now line's numbers and the open items
let AGENDA_RULES = null;  // /v1/agenda/rules: what each kind does, and what the owner taught it

async function loadAgenda() {
  try { AGENDA = await api("GET", "/v1/agenda"); } catch (_) { AGENDA = null; }
}

// What each source is called on the Now line.
const AGENDA_SOURCES = { docker: "the NAS", sonarr: "Sonarr", radarr: "Radarr", mail: "the mail",
  calendar: "the calendar", monitoring: "the alerts", endora: "Endora's copy" };

// "just now", "4 min ago", "2 h ago", "3 days ago".
function agendaAgo(at) {
  const s = Math.max(0, (Date.now() - at) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  const d = Math.floor(s / 86400);
  return `${d} day${d === 1 ? "" : "s"} ago`;
}

// "Watching 4 things · checked the NAS 1 min ago · 1 needs you". It opens Activity, the record of
// everything the loop did and why.
function viewNowLine() {
  if (!AGENDA || !AGENDA.now) return "";
  const n = AGENDA.now;
  // The newest read, the NAS first among equals; Endora's own copy is checked every look, so
  // it is never what the line names.
  const order = ["docker", "monitoring", "mail", "calendar", "sonarr", "radarr"];
  const rank = (s) => (order.includes(s) ? order.indexOf(s) : order.length);
  const latest = (n.checked || []).filter((c) => c.source !== "endora")
    .sort((a, b) => (b.at - a.at) || (rank(a.source) - rank(b.source)))[0];
  const parts = [`Watching ${n.watching} thing${n.watching === 1 ? "" : "s"}`];
  if (latest) parts.push(`checked ${AGENDA_SOURCES[latest.source] || esc(latest.source)} ${agendaAgo(latest.at)}`);
  if (n.needs_you) parts.push(`<b>${n.needs_you} need${n.needs_you === 1 ? "s" : ""} you</b>`);
  return `<button class="now-line" data-act="go:audit" title="Open Activity"><span class="now-dot"></span>${parts.join(" · ")}</button>`;
}

// What needs the owner, pinned above the conversation until answered.
function viewPinned() {
  const items = ((AGENDA && AGENDA.items) || []).filter((i) => i.level === "needs_you" || i.level === "urgent");
  return items.map((i) => `
    <div class="card pinned ${i.level === "urgent" ? "urgent" : ""}">
      <div class="sub">${esc(i.level_said)} · ${esc(i.kind_said)} · ${agendaAgo(i.opened_at)}</div>
      <div class="title" style="font-weight:500;">${esc(i.sentence)}</div>
      ${i.why ? `<div class="sub">${esc(i.why)}</div>` : ""}
      <div class="row pinned-acts">
        <button class="ghost" data-act="agenda:done:${i.id}">Done</button>
        <button class="ghost" data-act="agenda:dismiss:${i.id}">Dismiss</button>
        <button class="ghost danger" data-act="agenda:stop:${i.id}" title="${esc(i.stop_said)}">Stop</button>
      </div>
    </div>`).join("");
}

// Settings: what Endora does about each kind of thing, and the rules taught with Stop.
function viewWatching() {
  const head = `${settingsCrumbs("watching")}<h2>What Endora watches</h2>
    <div class="note">Endora reads your containers, downloads, mail and calendar, and decides what each thing deserves. It only ever restarts a container on its own; everything else it tells you. Activity has every decision and why.</div>`;
  if (!AGENDA_RULES) return `${head}<div class="empty">Loading…</div>`;
  const kinds = (AGENDA_RULES.kinds || []).map((k) => `
    <div class="card">
      <div class="title" style="font-weight:500;">${esc(k.said)}</div>
      <div class="row" style="gap:6px;flex-wrap:wrap;margin-top:6px;">
        ${(k.allowed || []).map((r) => `<button class="${r.name === k.response ? "primary" : "ghost"}" data-act="agenda:set:${esc(k.kind)}:${esc(r.name)}"
            aria-pressed="${r.name === k.response}">${esc(r.said)}${r.name === k.default ? " (usual)" : ""}</button>`).join("")}
      </div>
    </div>`).join("");
  const rules = (AGENDA_RULES.rules || []).map((r) => `
    <div class="card row" style="align-items:center;gap:10px;">
      <div class="grow"><div class="title" style="font-weight:500;">${esc(r.said)}</div><div class="sub">Since ${agendaAgo(r.at)}</div></div>
      <button class="ghost" data-act="agenda:forget:${r.id}">Undo</button>
    </div>`).join("");
  return `${head}
    <h3>What you've told it to leave alone</h3>
    ${rules || `<div class="empty">Nothing yet. Stop on a pinned card teaches it here.</div>`}
    <h3>What it does about each kind of thing</h3>
    ${kinds}`;
}

// The verbs this part uses. Returns true when it handled one.
async function agendaDispatch(verb, noun, id, arg) {
  if (!(verb === "agenda")) return false;
  try {
    if (noun === "done" || noun === "dismiss" || noun === "stop") {
      // Stop teaches a rule, so it says which before it does.
      const item = ((AGENDA && AGENDA.items) || []).find((i) => String(i.id) === String(id));
      if (noun === "stop" && !confirm(`${item ? item.stop_said : "Stop this"}? You can undo it under Settings, What Endora watches.`)) return true;
      const r = await api("POST", `/v1/agenda/${id}/${noun}`, {});
      if (r && r.rule) flash(`${r.rule}. Undo it under Settings.`, "ok");
    } else if (noun === "forget") {
      await api("POST", `/v1/agenda/rules/${id}/forget`, {});
    } else if (noun === "set") {
      await api("POST", `/v1/agenda/kinds/${id}`, { response: arg });
    } else {
      return false;
    }
  } catch (e) {
    flash("Couldn't do that: " + e.message, "err");
  }
  await loadAgenda();
  if (NAV.v === "watching") { try { AGENDA_RULES = await api("GET", "/v1/agenda/rules"); } catch (_) {} }
  render();
  return true;
}
