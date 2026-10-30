// ---- the Holodeck on the HQ screen (ADR 0135) ----
// The Work tab lists the chess matches and the model league, and opens a match move by move.
// The lighthouse, Silent Station, trials and re-runs were retired (2026-10-30). All of it is
// fetched when it is opened, and all of it is the team's or the world's words, so all of it is
// escaped.

// How a world ended, in words: "black won", not the stored "black_won".
function hqHoloEnd(end) { return esc(String(end || "").replace(/_/g, " ")); }

// A chess result in words, given each side as already-escaped text: who won, or a draw.
function hqChessEnd(end, white, black) {
  if (end === "white_won") return `${white} won`;
  if (end === "black_won") return `${black} won`;
  if (end === "draw") return "drawn";
  return esc(end || "");
}

// The brain a league game's side played with, or "" for a game outside the league.
function hqLeagueBrain(task, side) {
  const l = ((HQ.snap || {}).holodeck || {}).league || {};
  const game = (l.games || []).find((g) => Number(g.task) === Number(task));
  if (!game) return "";
  const row = (l.table || []).find((r) => r.brain === game[side]);
  return row ? row.label : game[side];
}

// The Holodeck card, in the Work tab: nothing until there is something in it.
function hqHolodeck(s) {
  const h = s && s.holodeck;
  const scenarios = (h && h.scenarios) || [];
  const league = hqLeague(h && h.league);
  if (!scenarios.length && !league) return "";
  const scen = scenarios.map((x) => hqLine(`hq:holo:open:${Number(x.task)}`, `Chess: ${esc(x.title)}`,
    `${hqHoloEnd(x.end || x.status)}, ${Number(x.elapsed)} half-moves${x.round ? ` · ${esc(x.round)}` : ""}`));
  const scenList = hqMore("scenarios", scen, 3);
  const open = HQ.holo && HQ.holo.what !== "replay" ? hqHoloDetail() : "";
  return `<section class="hq-card"><h2>Holodeck</h2>
    <p class="hq-note">Chess the team plays in matches that close themselves, and the league that ranks the brains by it. Nothing here touches the house.</p>
    ${scen.length ? `<p class="hq-who">Matches</p><ul class="hq-lines">${scenList.shown}</ul>${scenList.more}` : ""}
    ${league}
    ${open}</section>`;
}

// The model league (ADR 0135, amended 2026-10-25): the brains ranked by their chess, one line
// each, and its games, latest first, each opening its board.
function hqLeague(l) {
  if (!l || !(l.table || []).length) return "";
  const label = (name) => { const row = (l.table || []).find((r) => r.brain === name); return esc(row ? row.label : name); };
  const rows = l.table.map((r, i) => `<li>${i + 1}. ${esc(r.label)} <small>${Number(r.rating)} · ${Number(r.won)} won, ${Number(r.drawn)} drawn, ${Number(r.lost)} lost</small></li>`).join("");
  const games = (l.games || []).slice().reverse().map((g) => hqLine(`hq:holo:open:${Number(g.task)}`,
    `White: ${label(g.white)} · Black: ${label(g.black)}`, g.end ? hqChessEnd(g.end, label(g.white), label(g.black)) : "playing"));
  const list = hqMore("league", games, 3);
  const left = Number(l.pairs_left || 0);
  return `<p class="hq-who">Model league</p><ul class="hq-lines">${rows}</ul>
    <p class="hq-note">${left ? `${left} pair${left === 1 ? "" : "s"} still to meet; then the rematches, colours swapped.` : "Every pair has met; the rematches go on, colours swapped."}</p>
    ${games.length ? `<ul class="hq-lines">${list.shown}</ul>${list.more}` : ""}`;
}

// Whatever is open: a match, with a way to close it.
function hqHoloDetail() {
  const h = HQ.holo;
  if (!h || !h.data) return "";
  const close = `<div class="hq-row"><button class="hq-btn" data-act="hq:holo:close:0">Close</button></div>`;
  return hqScenarioView(h.data) + close;
}

// A match move by move: where it stands, and every action tried, refused ones too.
function hqScenarioView(d) {
  const head = `<div class="hq-sel">Chess · task ${Number(d.task)}${d.end ? ` · ${hqHoloEnd(d.end)}` : ""}</div>`;
  const sides = d.sides || {};
  // Black's side above the board and white's below, as the board is drawn, each with its brain in a league game.
  const side = (colour) => { const brain = hqLeagueBrain(d.task, colour); return `${esc(sides[colour] || "")}${brain ? ` · ${esc(brain)}` : ""}`; };
  const who = (colour) => `${colour === "white" ? "White" : "Black"} (${esc(sides[colour] || "")})`;
  const seen = d.positions || [], last = seen.length - 1;
  const ply = hqHoloPly(last);
  const rows = seen.length ? seen[ply] : d.rows;
  const lit = ply > 0 ? hqChessMoved(seen[ply - 1], seen[ply]) : [];
  const board = rows ? hqChessBoard({ rows, white: sides.white || "", black: sides.black || "", fen: d.fen || "", lit }) : "";
  const state = d.result && d.result !== "in_progress" ? `${hqChessEnd(d.result, who("white"), who("black"))}${d.reason ? `, ${esc(d.reason)}` : ""}` : `${who(d.to_move === "black" ? "black" : "white")} to move`;
  const moves = seen.length > 1 ? hqChessSteps(d, ply, last) : `<p class="hq-note">${d.moves ? esc(d.moves) : "No moves yet."}</p>`;
  const body = `<p class="hq-side black">♚\uFE0E Black: ${side("black")}</p>${board}<p class="hq-side white">♔\uFE0E White: ${side("white")}</p>
    <p>${state}</p>${moves}`;
  const roles = (d.by_role || []).map((r) => `<li>${esc(r.role)}: ${esc(r.by)}, ${Number(r.accepted)} legal, ${Number(r.refused)} illegal</li>`).join("");
  const steps = (d.log || []).map((a) => `<li${a.ok ? "" : ` class="hq-refused"`}>${a.ok ? "" : "refused: "}${esc(a.by)}${a.role ? ` (${esc(a.role)})` : ""}: ${esc(a.action)} → ${esc(a.said)}</li>`).join("");
  return `${head}${body}${roles ? `<ul class="hq-team">${roles}</ul>` : ""}
    ${steps ? `<p class="hq-who">Step by step</p><ol class="hq-log hq-steps">${steps}</ol>` : `<p class="hq-empty">Nothing done yet.</p>`}`;
}

// The move the owner has stepped to, or the latest while they watch it live.
function hqHoloPly(last) {
  const ply = HQ.holo && HQ.holo.ply;
  return ply === undefined || ply === null ? last : Math.max(0, Math.min(last, Number(ply)));
}

// The squares a move changed, from the rows before it and after: two, or four for castling.
function hqChessMoved(before, after) {
  const lit = [];
  (after || []).forEach((row, r) => [...String(row)].forEach((ch, f) => {
    if (String((before || [])[r] || "")[f] !== ch) lit.push(r * 8 + f);
  }));
  return lit;
}

// A game to watch: step through it, or tap a move in the list; live until the owner steps back.
function hqChessSteps(d, ply, last) {
  const list = d.move_list || [];
  const following = !d.end && (HQ.holo.ply === undefined || HQ.holo.ply === null);
  const step = (to, label, name) => `<button class="hq-btn" data-act="hq:holo:ply:${to}" aria-label="${name}">${label}</button>`;
  const at = ply === 0 ? "Start" : `Move ${ply} of ${last}: ${ply % 2 ? "white" : "black"} played ${esc(list[ply - 1] || "")}`;
  const pairs = [];
  for (let i = 0; i < list.length; i += 2) {
    const move = (j) => list[j] === undefined ? "" : `<button class="hq-move${j + 1 === ply ? " on" : ""}" data-act="hq:holo:ply:${j + 1}">${esc(list[j])}</button>`;
    pairs.push(`<li>${i / 2 + 1}. ${move(i)} ${move(i + 1)}</li>`);
  }
  return `<div class="hq-row">${step(0, "⏮", "Start")}${step(Math.max(0, ply - 1), "◀", "Back a move")}${step(ply + 1 >= last ? "live" : ply + 1, "▶", "On a move")}${step("live", "⏭", "Latest")}</div>
    <p class="hq-note">${at}${following ? " · Live" : ""}</p><ol class="hq-moves">${pairs.join("")}</ol>`;
}

// While a game being played is open, fetch it again so the board moves on its own;
// where the owner stepped to is kept.
async function hqHoloRefresh() {
  const open = HQ.holo;
  if (!open || open.what !== "scenario" || !open.data || open.data.end) return false;
  const data = await api("GET", `/v1/hq/scenarios/${Number(open.data.task)}`);
  if (HQ.holo !== open) return false;
  const moved = JSON.stringify(data) !== JSON.stringify(open.data);
  open.data = data;
  return moved;
}

// The taps: open a match, step through it, close what is open.
async function hqHoloAct(what, arg) {
  if (what === "close") { HQ.holo = null; render(); return true; }
  // A step through the game: past the last move, or "live", follows it again.
  if (what === "ply") {
    const last = ((HQ.holo && HQ.holo.data && HQ.holo.data.positions) || []).length - 1;
    if (HQ.holo) HQ.holo.ply = arg === "live" || Number(arg) >= last ? null : Number(arg);
    render(); return true;
  }
  if (what !== "open") return true;
  try { HQ.holo = { what: "scenario", data: await api("GET", `/v1/hq/scenarios/${Number(arg)}`) }; }
  catch (e) { flash("Couldn't open it: " + e.message, "err"); }
  render(); return true;
}
