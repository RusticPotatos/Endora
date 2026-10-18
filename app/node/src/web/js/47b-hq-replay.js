
// ---- Endora HQ: Replay ---------------------------------------------------------------------
// One turn read back in full, from HQ's record: why it was woken, what it was told (folded),
// each call it made with what came back, each post with what it cited, what it changed, and
// what it cost. Or a task's turns, one at a time, stepped through in order. Everything a model
// or a tool wrote is shown as text. Opened from a run line ("R12 Kit · EL-7 · advanced") or a
// task card on the HQ screen; it is the owner's screen, so Confidential excerpts are shown.

const REPLAY = {
  kind: "",            // "run" or "task"
  id: 0,               // the run's id, or the task's
  runs: null,          // the turns read back (one for a run), oldest first; null while loading
  at: 0,               // which of them is shown
  error: "",
};

// A long excerpt or post is folded past this many characters: its start shows, the rest opens.
const REPLAY_FOLD = 280, REPLAY_START = 160;

async function loadReplay(id) {
  const [kind, n] = String(id || "").split("-");
  if ((kind !== "run" && kind !== "task") || !Number(n)) { REPLAY.error = "Nothing to replay."; return; }
  const same = REPLAY.kind === kind && REPLAY.id === Number(n);
  Object.assign(REPLAY, { kind, id: Number(n), error: "" });
  if (!same) { REPLAY.runs = null; REPLAY.at = 0; }
  try {
    if (!HQ.snap) { try { HQ.snap = await api("GET", "/v1/hq"); } catch (_) { /* names only */ } }
    if (kind === "run") {
      const got = await api("GET", `/v1/hq/runs/${Number(n)}/replay`);
      REPLAY.runs = [got.replay];
    } else {
      const got = await api("GET", `/v1/hq/tasks/${Number(n)}/replay`);
      REPLAY.runs = got.runs || [];
      // A task opens on its latest turn; stepping goes back from there.
      if (!same) REPLAY.at = Math.max(0, REPLAY.runs.length - 1);
    }
  } catch (e) {
    REPLAY.runs = [];
    REPLAY.error = "HQ did not answer: " + String((e && e.message) || e);
  }
}

// Seconds as a turn's length reads: 45s, 3m 05s, 1h 02m.
function replaySpan(ms) {
  if (ms == null || ms < 0) return "";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}

// A moment's time, to the second: a turn's calls are seconds apart.
function replayClock(at) { return new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false }); }

// Words a model or a tool wrote: escaped, and folded when long.
function replayText(text) {
  const t = String(text == null ? "" : text);
  if (t.length <= REPLAY_FOLD) return `<span class="hq-replay-said">${esc(t)}</span>`;
  return `<details class="hq-replay-fold"><summary>${esc(t.slice(0, REPLAY_START))}…</summary><span class="hq-replay-said">${esc(t)}</span></details>`;
}

// The member's office character, cut from its sprite sheet: the first frame, facing the reader.
function replayFace(m) {
  if (!m) return "";
  const look = Math.abs(Number(m.look) || 0) % 6;
  return `<span class="hq-face" aria-hidden="true" style="background-image:url('/hq-art/characters/char_${look}.png')"></span>`;
}

// A name for a member id, from the office's snapshot when it has one.
function replayWho(id) { return typeof hqName === "function" ? hqName(id) : "someone"; }

// What an event of the feed says, in words. Unknown kinds say their name and what they carry.
function replayEvent(m) {
  const d = m.data || {}, key = hqKey(d.task), on = key ? ` ${key}` : "";
  const words = (s) => String(s || "").replace(/_/g, " ");
  switch (m.name) {
    case "run.started": return ["start", `Started${on}${d.reserved ? `, ${hqDollars(d.reserved)} held` : ""}`];
    case "run.ended": return [d.result === "succeeded" ? "done" : "warn", `Ended: ${words(d.result)}${d.why ? ` (${d.why})` : ""}${d.label ? ` · ${d.label}` : ""}`];
    case "task.claimed": return ["move", `Checked out${on}`];
    case "task.released": return ["warn", `Handed${on} back`];
    case "task.changed": return ["move", `${key || "The task"} is ${words(d.status)}`];
    case "task.created": return ["move", `Made ${key}${d.title ? `: ${d.title}` : ""}`];
    case "desk.added": return ["desk", `On your desk: ${words(d.kind)}${on}`];
    case "wake.queued": return ["wake", `Woke ${replayWho(d.member)}: ${words(d.reason)}${on}`];
    case "wake.folded": return ["wake", `${replayWho(d.member)} told in their live turn: ${words(d.reason)}`];
    case "wake.held": return ["wake", `${replayWho(d.member)} waits: ${words(d.reason)}`];
    case "wake.refused": return ["warn", `Wake for ${replayWho(d.member)} skipped: ${words(d.why)}`];
    case "checkback.set": return ["wake", `Will check back in ${d.minutes} min: ${d.why || ""}`];
    case "lesson.noted": return ["note", `Noted a lesson: ${d.lesson || ""}`];
    case "budget.crossed": return ["bad", `Crossed its limit at ${hqDollars(d.spent)}`];
    case "gate.result": return [d.passed ? "done" : "bad", `The gate ${d.passed ? "passed" : "failed"}${on}`];
    // The open office (ADR 0119): a proposal, a vote, a change, a proposal let go or vetoed.
    case "office.proposed": return ["note", d.text || "Proposed a change to the office"];
    case "office.voted": return ["note", d.text || "Voted on a proposal for the office"];
    case "office.changed": return ["done", d.text || "The office changed"];
    case "office.dropped": return ["warn", d.text || "A proposal for the office was dropped"];
    case "office.vetoed": return ["warn", d.text || "A proposal for the office was vetoed"];
    default: {
      const said = Object.entries(d).filter(([k, v]) => k !== "run" && v !== null && typeof v !== "object")
        .map(([k, v]) => `${words(k)}: ${v}`).join(" · ");
      return ["note", `${m.name}${said ? ` · ${said}` : ""}`];
    }
  }
}

// One moment of a turn: its time, a tag for its kind, and what it was.
function replayMoment(m, member) {
  const name = member ? member.name : "The member";
  let tag = "note", label = "", body = "";
  if (m.kind === "woken") { tag = "wake"; label = "woken"; body = esc(String(m.reason || "").replace(/_/g, " ")); }
  else if (m.kind === "joined") { tag = "wake"; label = "told"; body = `Woken again while it ran: ${esc(String(m.reason || "").replace(/_/g, " "))}`; }
  else if (m.kind === "briefing") {
    tag = "brief"; label = "briefing";
    body = `<details class="hq-replay-told"><summary>What ${esc(name)} was told</summary><pre>${esc(m.text)}</pre></details>`;
  } else if (m.kind === "call") {
    tag = m.ok ? "call" : "bad"; label = m.act ? "act" : "read";
    body = `<b>${esc(m.receipt)} ${esc(m.tool)}</b>${m.args ? ` <span class="hq-replay-args">${esc(m.args)}</span>` : ""} · ${m.ok ? "ok" : "refused"}${m.confidential ? ` · <span class="hq-replay-conf">Confidential</span>` : ""}<br>${replayText(m.excerpt)}`;
  } else if (m.kind === "post") {
    tag = "post"; label = "post";
    const cites = (m.evidence || []).length ? ` <span class="hq-replay-cites">cites ${(m.evidence || []).map(esc).join(", ")}</span>` : "";
    body = `<b>${esc(m.author)}</b>${m.task ? ` in ${hqKey(m.task)}` : ""}${m.unsupported ? ` <span class="hq-unsupported">unsupported</span>` : ""}${cites}<br>${replayText(m.text)}`;
  } else if (m.kind === "cited") {
    tag = "post"; label = "evidence";
    body = `${esc(String(m.stage || ""))}${m.criterion ? `, criterion ${Number(m.criterion)}` : ""}: ${(m.refs || []).map(esc).join(", ") || "nothing"}`;
  } else if (m.kind === "ended") {
    tag = m.status === "succeeded" ? "done" : "warn"; label = "ended";
    body = `${esc(String(m.status || "").replace(/_/g, " "))}${m.reason ? ` (${esc(m.reason)})` : ""}${m.label ? ` · ${esc(m.label)}` : ""}`;
  } else if (m.kind === "event") {
    const [k, said] = replayEvent(m); tag = k; label = String(m.name || "").split(".")[0]; body = esc(said);
  }
  const time = m.at ? `<time>${esc(replayClock(m.at))}</time>` : "<time></time>";
  return `<li class="hq-replay-moment t-${tag}">${time}<span class="hq-replay-tag">${esc(label)}</span><div class="hq-replay-body">${body}${m.by_time ? ` <span class="hq-replay-time">matched by time</span>` : ""}</div></li>`;
}

// A turn's header: who, why, what it came to, what it cost, how long.
function replayHead(r) {
  const m = r.member, money = r.money || {}, tokens = r.tokens || {};
  const who = m ? `${esc(m.name)} · ${esc(hqRole(m.role))}` : "someone no longer on the team";
  const brain = m ? esc(m.brain_label || m.brain || "") : "";
  const ran = r.started_at ? replaySpan((r.ended_at || Date.now()) - r.started_at) : "";
  const cost = `${hqDollars(r.money && r.money.spent)} spent · ${hqDollars(money.reserved)} held · ${hqDollars(money.released)} released`;
  const input = (tokens.input || 0) + (tokens.cache_read || 0) + (tokens.cache_write || 0);
  return `<div class="hq-replay-who">${replayFace(m)}<div><div class="hq-sel">R${Number(r.run)} · ${who}</div>
    <div class="hq-who">${r.task ? hqKey(r.task) + " · " : ""}${esc(String(r.status || "").replace(/_/g, " "))}${r.label ? ` · <b>${esc(r.label)}</b>` : ""}${r.attempts > 1 ? ` · ${Number(r.attempts)} attempts` : ""}</div></div></div>
    <dl class="hq-kv">
      <dt>Woken</dt><dd>${esc(String(r.reason || "?").replace(/_/g, " "))}</dd>
      ${r.briefed ? `<dt>Started from</dt><dd>${esc(String(r.briefed.situation || "").replace(/_/g, " "))} · about ${Number(r.briefed.tokens) || 0} of ${Number(r.briefed.budget) || 0} tokens of the world${r.briefed.over_budget ? " · over its budget" : ""}${(r.briefed.dropped || []).length ? ` · left out: ${(r.briefed.dropped || []).map((d) => esc(d)).join(", ")}` : ""}</dd>` : ""}
      <dt>Brain</dt><dd>${brain || "?"} <span class="hq-who">(as set now)</span></dd>
      <dt>Ran</dt><dd>${ran ? esc(ran) : "not started"}${r.started_at ? ` from ${esc(replayClock(r.started_at))}` : ""}</dd>
      <dt>Money</dt><dd>${cost}</dd>
      <dt>Tokens</dt><dd>in ${input} (${tokens.cache_read || 0} from the cache) · out ${tokens.output || 0}</dd>
      ${r.end_reason ? `<dt>Ended</dt><dd>${esc(r.end_reason)}</dd>` : ""}
    </dl>`;
}

function viewReplay() {
  const back = `<div class="hq-row"><button class="hq-btn" data-act="go:hq">◀ Back to HQ</button></div>`;
  const title = REPLAY.kind === "task" ? `Replay ${hqKey(REPLAY.id)}` : REPLAY.id ? `Replay R${REPLAY.id}` : "Replay";
  const head = `<div class="hq-bar"><h1>${esc(title)}</h1><span class="hq-grow"></span>${back}</div>`;
  const shell = (inner) => `<div class="hq hq-replay">${head}${inner}</div>`;
  if (REPLAY.error) return shell(`<section class="hq-card"><p class="hq-empty">${esc(REPLAY.error)}</p></section>`);
  if (!REPLAY.runs) return shell(`<p class="hq-empty">Reading the record…</p>`);
  if (!REPLAY.runs.length) return shell(`<section class="hq-card"><p class="hq-empty">No turns on this task yet.</p></section>`);
  const at = Math.min(Math.max(0, REPLAY.at), REPLAY.runs.length - 1), r = REPLAY.runs[at];
  const task = REPLAY.kind === "task" && typeof hqTask === "function" ? hqTask(REPLAY.id) : null;
  const steps = REPLAY.kind === "task" ? `<div class="hq-replay-steps">
      <button class="hq-btn" data-act="replay:step:prev"${at === 0 ? " disabled" : ""}>◀ previous run</button>
      <span class="hq-replay-progress">run ${at + 1} of ${REPLAY.runs.length}</span>
      <button class="hq-btn" data-act="replay:step:next"${at === REPLAY.runs.length - 1 ? " disabled" : ""}>next run ▶</button></div>` : "";
  const moments = (r.moments || []).map((m) => replayMoment(m, r.member)).join("");
  const byTime = r.by_time ? `<p class="hq-note">This turn ran before HQ named each event's turn: what is marked "matched by time" was matched by its member, its task and when it happened.</p>` : "";
  return shell(`${task ? `<p class="hq-who">${hqKey(task.id)} · ${esc(task.title)}</p>` : ""}${steps}
    <section class="hq-card">${replayHead(r)}</section>
    <section class="hq-card"><h2>What happened</h2>${byTime}<ol class="hq-replay-line">${moments || `<li class="hq-empty">Nothing recorded.</li>`}</ol></section>
    ${steps}`);
}

// The links into Replay, for the HQ screen: from a run line, and from a task card.
function replayRunLink(run, text) { return `<button class="hq-link" data-act="replay:run:${Number(run)}">${text}</button>`; }
function replayTaskLink(task) { return `<div class="hq-row"><button class="hq-btn" data-act="replay:task:${Number(task)}">Replay this task</button></div>`; }

async function replayDispatch(verb, noun, id) {
  if (!(verb === "replay")) return false;
  if (noun === "run" || noun === "task") { go("replay", `${noun}-${Number(id)}`); return true; }
  if (noun === "step" && REPLAY.runs) {
    REPLAY.at = Math.min(Math.max(0, REPLAY.at + (id === "next" ? 1 : -1)), REPLAY.runs.length - 1);
    render(); window.scrollTo(0, 0); return true;
  }
  return true;
}
