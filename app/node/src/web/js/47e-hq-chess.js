// ---- the Chess tab (the owner's notes of 31 Oct; ADR 0135) ----
// The model league's own tab: the games in play, each with its board, whose move it is and the
// last move; this round's bracket, brains against brains, and the standings; and every finished
// game, newest first, a page at a time, each with its PGN. The live part reads the snapshot, so
// the console's own refresh moves it; the list is asked of the node a page at a time, and again
// only when a game finishes. The league is a round robin, one game a pair, and a round runs
// from one reset to the next: it is not monthly, so the tab says the round, never a month. The
// board, the sparkline, the results in words and the PGN download are the Holodeck's helpers
// (47d) and the office's board (47-hq.js). Every word is a brain's label or the world's, so
// all of it is escaped.

// A brain's label, by its name, from the league's table, as text (the board escapes its own)
// and escaped, for markup.
function hqChessLabelRaw(l, name) {
  const row = ((l && l.table) || []).find((r) => r.brain === name);
  return String(row ? row.label : name);
}
function hqChessLabel(l, name) { return esc(hqChessLabelRaw(l, name)); }

// Points as a scoresheet writes them: 1½ pts, ½ pt, 1 pt, 0 pts.
function hqChessPoints(r) {
  const p = Number.isFinite(Number(r.points)) ? Number(r.points) : Number(r.won || 0) + Number(r.drawn || 0) / 2;
  const whole = Math.floor(p), half = p - whole >= 0.5;
  const said = whole ? `${whole}${half ? "½" : ""}` : half ? "½" : "0";
  return `${said} ${p === 1 || p === 0.5 ? "pt" : "pts"}`;
}

// A move as numbered on a scoresheet, by how many half-moves have been played: 1. e4, 1... e5.
function hqChessNumbered(halfMoves, san) {
  const n = Number(halfMoves) || 0;
  return `${Math.ceil(n / 2)}.${n % 2 ? "" : ".."} ${esc(san)}`;
}

// The tab, whole: the league's, or a line saying there is none yet.
function hqChessTab(s) {
  const l = s && s.holodeck && s.holodeck.league;
  if (!l || !(l.table || []).length) {
    return `<section class="hq-card"><h2>Chess</h2><p class="hq-empty">No model league yet: start one and its games, bracket and standings show here.</p></section>`;
  }
  hqChessKeepUp(l);
  const open = HQ.holo && HQ.holo.where === "chess" ? `<section class="hq-card" aria-live="polite"><h2>Game</h2>${hqHoloDetail()}</section>` : "";
  return `${open}${hqChessLive(l)}${hqChessRound(l)}${hqChessAll(l)}`;
}

// Live: each league game in play, its board with the last move lit, whose move it is and the
// last move, and a way to watch it move by move.
function hqChessLive(l) {
  const playing = (l.games || []).filter((g) => g.live);
  const next = Number(l.pairs_left || 0)
    ? "The next pair sits down when two members are free and nothing else is being played."
    : "Every pair has played this round; resetting the league starts the next.";
  if (!playing.length) return `<section class="hq-card hq-wide-card"><h2>Live</h2><p class="hq-empty">No league game is being played now.</p><p class="hq-note">${next}</p></section>`;
  const boards = playing.map((g) => {
    const white = hqChessLabel(l, g.white), black = hqChessLabel(l, g.black);
    const side = g.to_move === "black" ? `Black, ${black}` : `White, ${white}`;
    const lit = g.before ? hqChessMoved(g.before, g.rows) : [];
    const board = g.rows ? hqChessBoard({ rows: g.rows, white: hqChessLabelRaw(l, g.white), black: hqChessLabelRaw(l, g.black), fen: g.fen || "", lit }) : "";
    const last = g.last ? `Last move: ${hqChessNumbered(g.half_moves, g.last)}` : "No moves yet";
    return `<p class="hq-who">White: ${white} · Black: ${black}</p>${board}
      <p>${side}, to move. ${last}.</p>
      <div class="hq-row"><button class="hq-btn" data-act="hq:holo:watch:${Number(g.task)}">Watch move by move</button></div>`;
  }).join("");
  return `<section class="hq-card hq-wide-card"><h2>Live</h2>${boards}</section>`;
}

// This round: what a round is, the bracket (each row's result against each column) in a box
// that scrolls on its own, and the standings, points then rating, as HQ ranks them.
function hqChessRound(l) {
  const round = l.round || {};
  const number = Number(round.number) || 1;
  const since = round.since ? `since the reset on ${esc(round.since)}` : "since the league started";
  const brains = (l.table || []).map((r) => r.brain);
  const games = l.games || [];
  const result = (a, b) => {
    if (a === b) return "—";
    const g = games.filter((x) => (x.white === a && x.black === b) || (x.white === b && x.black === a)).pop();
    if (!g) return "not yet";
    if (g.live) return "playing";
    if (g.end === "draw") return "drawn";
    if (g.end === "white_won" || g.end === "black_won") return (g.end === "white_won") === (g.white === a) ? "won" : "lost";
    return "no result";
  };
  const head = brains.map((b) => `<th scope="col">${hqChessLabel(l, b)}</th>`).join("");
  const rows = brains.map((a) => `<tr><th scope="row">${hqChessLabel(l, a)}</th>${brains.map((b) => {
    const said = result(a, b);
    return `<td class="${a === b ? "self" : said.replace(/[^a-z]/g, "")}">${said}</td>`;
  }).join("")}</tr>`).join("");
  const standings = (l.table || []).map((r, i) => `<li>${i + 1}. ${esc(r.label)} <small>${hqChessPoints(r)} · ${Number(r.rating)} · ${Number(r.won)} won, ${Number(r.drawn)} drawn, ${Number(r.lost)} lost</small> ${hqSpark(r.history, r.label)}</li>`).join("");
  const left = Number(l.pairs_left || 0);
  return `<section class="hq-card hq-wide-card"><h2>This round</h2>
    <p>Round ${number}, ${since}.</p>
    <p class="hq-note">The league is a round robin: every pair of brains plays once a round, and a round runs until the league is reset. ${left ? `${left} pair${left === 1 ? "" : "s"} still to meet.` : "Every pair has met."}</p>
    <div class="hq-scroll" role="region" aria-label="Who has played whom this round: each row's result against each column" tabindex="0"><table class="hq-cross"><thead><tr><th scope="col"></th>${head}</tr></thead><tbody>${rows}</tbody></table></div>
    <p class="hq-who">Standings</p><ul class="hq-lines">${standings}</ul></section>`;
}

// All games: a page of every finished league game, newest first, each opening move by move with
// its PGN to download; Newer and Older between pages, never an endless scroll.
function hqChessAll() {
  const c = HQ.chessGames;
  const d = c && c.data;
  // A failed load says so in the card, with a way to try again, rather than "Loading…" for ever.
  if (!d && c && c.error) return `<section class="hq-card hq-wide-card"><h2>All games</h2><p class="hq-empty" title="${esc(c.error)}">Couldn't load the league's games.</p><div class="hq-row"><button class="hq-btn" data-act="hq:holo:page:1">Try again</button></div></section>`;
  if (!d) return `<section class="hq-card hq-wide-card"><h2>All games</h2><p class="hq-empty">Loading…</p></section>`;
  const games = d.games || [];
  if (!games.length) return `<section class="hq-card hq-wide-card"><h2>All games</h2><p class="hq-empty">No league game has finished yet.</p></section>`;
  const items = games.map((g) => {
    const white = esc(g.white_label || g.white), black = esc(g.black_label || g.black);
    const said = g.result === "abandoned" ? "stopped with no result" : hqChessEnd(g.result, white, black);
    const moves = Math.ceil((Number(g.half_moves) || 0) / 2);
    return `<li class="hq-game"><button class="hq-line" data-act="hq:holo:watch:${Number(g.task)}"><b>White: ${white} · Black: ${black}</b> <small>${said}, ${moves} move${moves === 1 ? "" : "s"} · round ${Number(g.round)}${g.day ? ` · ${esc(g.day)}` : ""}</small></button>
      <button class="hq-btn" data-act="hq:holo:save:${Number(g.task)}" aria-label="Download the PGN of game ${Number(g.task)}">PGN</button></li>`;
  }).join("");
  const page = Number(d.page) || 1, pages = Number(d.pages) || 1;
  const busy = c.busy ? " disabled" : "";
  const pager = pages > 1 ? `<div class="hq-row">${page > 1 ? `<button class="hq-btn" data-act="hq:holo:page:${page - 1}"${busy}>Newer</button>` : ""}
    <span class="hq-note">Page ${page} of ${pages}</span>${page < pages ? `<button class="hq-btn" data-act="hq:holo:page:${page + 1}"${busy}>Older</button>` : ""}</div>` : "";
  return `<section class="hq-card hq-wide-card"><h2>All games <span class="hq-count">${Number(d.total) || 0}</span></h2><ul class="hq-lines">${items}</ul>${pager}</section>`;
}

// While the tab is showing, the list is asked for once, and again (from its newest page) when the
// snapshot counts another finished game: the console's own refresh drives it, no timer of its own.
function hqChessKeepUp(l) {
  if (hqPlace() !== "chess") return;
  const finished = (l.games || []).filter((g) => !g.live).length;
  const c = HQ.chessGames;
  if (c && (c.busy || c.seen === finished)) return;
  setTimeout(() => hqChessLoad(1, finished), 0);
}

// A page of the finished games, from the node.
async function hqChessLoad(page, seen) {
  const c = HQ.chessGames = HQ.chessGames || { page: 1, data: null, seen: -1 };
  if (seen !== undefined) c.seen = seen;
  c.busy = true;
  try {
    c.data = await api("GET", `/v1/hq/league/games?page=${Math.max(1, Number(page) || 1)}`);
    c.page = Number(c.data.page) || 1; c.error = "";
  } catch (e) { c.error = String((e && e.message) || e); }
  c.busy = false;
  render();
}
