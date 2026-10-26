// ---- the Holodeck on the HQ screen (ADR 0135), and replays of the office's matches (0121) ----
// The Work tab lists the scenarios and the trials and opens one step by step; the Office
// replays a match of pool or ping-pong shot by shot or rally by rally. All of it is fetched
// when it is opened, and all of it is the team's or the world's words, so all of it is escaped.

const HQ_HOLO_KINDS = { lighthouse: "Lighthouse", silent_station: "Silent Station", chess: "Chess" };
const HQ_HOLO_UNITS = { lighthouse: "minutes", silent_station: "ticks", chess: "half-moves" };

function hqHoloKind(kind) { return HQ_HOLO_KINDS[kind] || String(kind || ""); }

// The Holodeck card, in the Work tab: nothing until there is something in it.
function hqHolodeck(s) {
  const h = s && s.holodeck;
  const scenarios = (h && h.scenarios) || [], trials = (h && h.trials) || [];
  if (!scenarios.length && !trials.length) return "";
  const scen = scenarios.map((x) => hqLine(`hq:holo:open:${Number(x.task)}`, `${esc(hqHoloKind(x.kind))}: ${esc(x.title)}`,
    `${esc(x.end || x.status)}, ${Number(x.elapsed)} ${esc(HQ_HOLO_UNITS[x.kind] || "")}${x.round ? ` · ${esc(x.round)}` : ""}`));
  const scenList = hqMore("scenarios", scen, 5);
  const queued = Number((h && h.queued) || 0);
  const again = queued
    ? `<p class="hq-note">${queued} scenario${queued === 1 ? "" : "s"} queued to run again, one at a time.</p>
      <div class="hq-row"><button class="hq-btn tall" data-act="hq:holo:restart:all">Start over on today's engine</button></div>`
    : `<div class="hq-row"><button class="hq-btn tall" data-act="hq:holo:rerun:all">Run every scenario again</button></div>`;
  const tried = trials.map((t) => hqLine(`hq:holo:trial:${esc(t.name)}`, esc(t.name), `${esc(hqHoloKind(t.kind))}, ${Number(t.runs)} runs · compare`)).join("");
  const open = HQ.holo && HQ.holo.what !== "replay" ? hqHoloDetail() : "";
  return `<section class="hq-card"><h2>Holodeck</h2>
    <p class="hq-note">Scenarios the team plays in worlds that close themselves, and trials that compare who plays them. Nothing here touches the house.</p>
    ${again}
    ${scen.length ? `<p class="hq-who">Scenarios</p><ul class="hq-lines">${scenList.shown}</ul>${scenList.more}` : ""}
    ${tried ? `<p class="hq-who">Trials</p><ul class="hq-lines">${tried}</ul>` : ""}
    ${open}</section>`;
}

// Whatever is open: a scenario, a trial or a replayed match, with a way to close it.
function hqHoloDetail() {
  const h = HQ.holo;
  if (!h || !h.data) return "";
  const close = `<div class="hq-row"><button class="hq-btn" data-act="hq:holo:close:0">Close</button></div>`;
  if (h.what === "trial") return hqTrialView(h.data) + close;
  if (h.what === "replay") return hqReplayView(h.data) + close;
  return hqScenarioView(h.data) + close;
}

// A scenario step by step: where it stands, and every action tried, refused ones too.
function hqScenarioView(d) {
  const head = `<div class="hq-sel">${esc(hqHoloKind(d.kind))} · task ${Number(d.task)}${d.end ? ` · ${esc(d.end)}` : ""}</div>`;
  let body = "";
  if (d.kind === "chess") {
    const sides = d.sides || {};
    const board = d.rows ? hqChessBoard({ rows: d.rows, white: sides.white || "", black: sides.black || "", fen: d.fen || "" }) : "";
    const state = d.result ? `${esc(d.result)}${d.reason ? `, ${esc(d.reason)}` : ""}` : `${esc(d.to_move || "")} to move`;
    body = `${board}<p>${state}</p><p class="hq-note">${d.moves ? esc(d.moves) : "No moves yet."}</p>`;
  } else if (d.kind === "silent_station") {
    const s = d.state || {};
    body = `<p>Life support ${Number(s.life_support)} ticks · ship energy ${Number(s.ship_energy)} · station power ${Number(s.station_power)}% ·
      hazard ${Number(s.hazard)} of 3${s.crew_safe === false ? " · someone was hurt" : ""}${s.power_routed ? " · power routed" : ""}${s.probe_out ? " · probe out" : ""}${s.contact ? " · contact" : ""}</p>
      <p>Score ${Number(d.score)}${d.anomaly ? ` · the anomaly was a ${esc(d.anomaly)}` : ""}</p>
      ${d.brief === "plan_first" ? `<p class="hq-note">Briefed plan first: nothing is done before the captain's plan.</p>` : ""}
      ${d.plan ? `<p>The captain's plan: ${esc(d.plan)}</p>` : ""}`;
  } else {
    body = `<p>${Number(d.minutes)} of ${Number(d.limit)} minutes${d.cause ? ` · the cause was the ${esc(d.cause)}` : ""}</p>`;
  }
  const legal = d.kind === "chess" ? ["legal", "illegal"] : ["accepted", "refused"];
  const roles = (d.by_role || []).map((r) => `<li>${esc(r.role)}: ${esc(r.by)}, ${Number(r.accepted)} ${legal[0]}, ${Number(r.refused)} ${legal[1]}</li>`).join("");
  const steps = (d.log || []).map((a) => `<li${a.ok ? "" : ` class="hq-refused"`}>${a.ok ? "" : "refused: "}${esc(a.by)}${a.role ? ` (${esc(a.role)})` : ""}: ${esc(a.action)} → ${esc(a.said)}</li>`).join("");
  return `${head}${body}${roles ? `<ul class="hq-team">${roles}</ul>` : ""}
    ${steps ? `<p class="hq-who">Step by step</p><ol class="hq-log">${steps}</ol>` : `<p class="hq-empty">Nothing done yet.</p>`}`;
}

// A trial's conditions side by side, summed from the record.
function hqTrialView(d) {
  const rows = (d.conditions || []).map((c) => `<tr><td>${esc(c.label)}</td><td class="n">${Number(c.runs)}</td><td class="n">${Number(c.done)}</td>
    <td class="n">${Number(c.not_done)}</td><td class="n">${Number(c.unfinished)}</td><td class="n">${Number(c.mean_score)}</td><td class="n">${Number(c.mean_elapsed)}</td>
    <td class="n">${Number(c.refused)}</td><td class="n">${Number(c.unsafe_runs)}</td><td class="n">${Number(c.turns)}</td><td class="n">${Number(c.idle_turns)}</td>
    <td class="n">$${(Number(c.spent) / 1e6).toFixed(2)}</td><td>${(c.brains || []).map((b) => esc(b)).join(", ")}</td></tr>`).join("");
  return `<div class="hq-sel">Trial ${esc(d.name)} · ${esc(hqHoloKind(d.kind))}, version ${Number(d.version)}, seeds ${(d.seeds || []).map(Number).join(", ")}</div>
    <table class="hq-board"><thead><tr><th>Condition</th><th>Runs</th><th>Done</th><th>Not done</th><th>Unfinished</th><th>Score</th><th>Time</th>
      <th>Refused</th><th>Unsafe</th><th>Turns</th><th>Idle</th><th>Cost</th><th>Brains</th></tr></thead><tbody>${rows}</tbody></table>
    <p class="hq-note">A few runs a condition show a difference, not a reason: open each run's log before believing an explanation.</p>`;
}

// A match from the office played again: every shot of pool, every rally of ping-pong.
function hqReplayView(d) {
  const players = d.players || [], score = d.score || [];
  const head = `<div class="hq-sel">${d.kind === "pool" ? "Pool" : "Ping-pong"}: ${esc(players[0])} against ${esc(players[1])}, ${score.map(Number).join("–")}, ${esc(d.winner)} won</div>`;
  const steps = (d.steps || []).map((s) => {
    if (s.step === "rally") return `<li>${(s.score || []).map(Number).join("–")} ${esc(s.won_by)}</li>`;
    if (s.step === "scratch") return `<li>${esc(s.by)} scratched on the 8</li>`;
    return `<li>${esc(s.by)} ${s.potted ? "potted" : "missed"} (${(s.balls || []).map(Number).join("–")})</li>`;
  }).join("");
  const check = d.matches === false ? `<p class="hq-note">This replay does not end as the record says.</p>` : "";
  return `${head}${check}<ol class="hq-log">${steps}</ol>`;
}

// The taps: open a scenario, compare a trial, replay a match, close what is open.
async function hqHoloAct(what, arg) {
  if (what === "close") { HQ.holo = null; render(); return true; }
  // Every scenario again, one at a time: the earlier round is kept, labelled with today.
  if (what === "rerun" || what === "restart") {
    const restart = what === "restart";
    const round = "round of " + new Date().toISOString().slice(0, 16).replace("T", " ");
    if (!confirm(restart
      ? `Start the re-run over? The scenario being played is dropped (kept), and every scenario is queued again. Results so far are kept, as "${round}".`
      : `Run every scenario again, one at a time? The earlier results are kept, as "${round}".`)) return true;
    try { const r = await api("POST", "/v1/hq/scenarios/rerun", { round, restart }); flash(`${Number(r.queued)} queued to run again.`, "ok"); }
    catch (e) { flash("Couldn't run them again: " + e.message, "err"); }
    await loadHQ(); render(); return true;
  }
  const path = what === "open" ? `/v1/hq/scenarios/${Number(arg)}`
    : what === "trial" ? `/v1/hq/trials/${encodeURIComponent(String(arg || ""))}`
    : what === "replay" ? `/v1/hq/games/${Number(arg)}/replay` : "";
  if (!path) return true;
  try { HQ.holo = { what: what === "open" ? "scenario" : what, data: await api("GET", path) }; }
  catch (e) { flash("Couldn't open it: " + e.message, "err"); }
  render(); return true;
}
