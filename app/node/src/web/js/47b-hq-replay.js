
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
  given: false,        // whether what each turn was given opens unfolded (from a task's turn)
  timer: null,         // the next read of a turn that is still live
};

// A turn still running is read again this often while its replay is open (ADR 0129, amended
// 2026-11-01): the worker tells its conversation after every model round.
const REPLAY_LIVE_EVERY = 4000;

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
    // A live turn read again keeps what it shows: the next read tries again.
    if (!same || !REPLAY.runs || !REPLAY.runs.length) {
      REPLAY.runs = [];
      REPLAY.error = "HQ did not answer: " + String((e && e.message) || e);
    }
  }
  replayWatch();
}

// While a turn shown is live, read it again in a few seconds, for as long as its replay is open;
// once every turn shown has ended, stop.
function replayWatch() {
  clearTimeout(REPLAY.timer);
  REPLAY.timer = null;
  if (!(REPLAY.runs || []).some((r) => r && r.live)) return;
  const id = `${REPLAY.kind}-${REPLAY.id}`;
  REPLAY.timer = setTimeout(async () => {
    REPLAY.timer = null;
    if (NAV.v !== "replay" || NAV.id !== id) return;
    await loadReplay(id);
    if (NAV.v === "replay" && NAV.id === id) render();
  }, REPLAY_LIVE_EVERY);
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

// A turn's conversation (ADR 0129): the text HQ kept is divided by lines like `--- assistant ---`
// and `--- call: post ---`. Each part is who said it (or which call, or what it came to) and
// its words; nothing in it is markup.
function replayParts(text) {
  const parts = [];
  for (const line of String(text == null ? "" : text).split("\n")) {
    const m = /^--- ([a-z]+)(?:: ([^\n]*?))? ---$/.exec(line);
    if (m) { parts.push({ kind: m[1], name: m[2] || "", lines: [] }); continue; }
    if (!parts.length) parts.push({ kind: "note", name: "", lines: [] });
    parts[parts.length - 1].lines.push(line);
  }
  return parts.map((p) => ({ kind: p.kind, name: p.name, body: p.lines.join("\n").replace(/^\n+|\n+$/g, "") }));
}

// What the model and the calls said between them, in order: the turn's conversation. What the
// model thought before a step (ADR 0129, amended 2026-11-01) is folded under that step.
function replayTalk(t, member) {
  const who = { user: "told", system: "rules", assistant: member ? member.name : "model", thinking: "thought", call: "call", result: "result", note: "note" };
  const tags = { user: "brief", system: "brief", assistant: "post", thinking: "note", call: "call", result: "call", note: "note" };
  const steps = [];
  let thought = "";
  for (const p of replayParts(t.text).filter((q) => q.kind !== "tools")) {
    if (p.kind === "thinking") { thought = thought ? `${thought}\n${p.body}` : p.body; continue; }
    steps.push(Object.assign(p, { thought }));
    thought = "";
  }
  if (thought) steps.push({ kind: "thinking", name: "", body: thought, thought: "" });
  const parts = steps.map((p) => {
    const long = p.kind === "user" || p.kind === "system";
    const fold = p.thought ? `<details class="hq-replay-thought"><summary>Thinking</summary><span class="hq-replay-said">${esc(p.thought)}</span></details>` : "";
    const body = long
      ? `<details class="hq-replay-told"><summary>${esc(p.kind === "system" ? "The rules it was given" : "The briefing it was given")}</summary><pre>${esc(p.body)}</pre></details>${fold}`
      : `${p.kind === "call" && p.name ? `<b>${esc(p.name)}</b><br>` : ""}${replayText(p.body)}${fold}`;
    const label = p.kind === "call" ? "asked" : p.kind === "result" && p.name ? "refused" : (who[p.kind] || esc(p.kind));
    const tag = p.kind === "result" && p.name ? "bad" : (tags[p.kind] || "note");
    return `<li class="hq-replay-moment t-${tag}"><time></time><span class="hq-replay-tag">${esc(label)}</span><div class="hq-replay-body">${body}</div></li>`;
  });
  const facts = [esc(String(t.runtime || "").replace(/-/g, " ")), `${esc(String(t.brain || ""))}`, `about ${Number(t.est_tokens) || 0} tokens`];
  if (t.tier && t.tier !== "internal") facts.push(`<span class="hq-replay-conf">${esc(t.tier)}</span>`);
  const cut = t.cut ? `<p class="hq-note">This conversation was longer than HQ keeps: its middle was cut, and the cut is marked.</p>` : "";
  const until = t.kept_until ? `<p class="hq-note">HQ keeps it until ${esc(new Date(t.kept_until).toLocaleDateString())}; after that only the handover notes and last words remain.</p>` : "";
  return `<section class="hq-card"><h2>What was said</h2><p class="hq-who">${facts.join(" · ")}</p>${cut}<ol class="hq-replay-line">${parts.join("") || `<li class="hq-empty">Nothing was said.</li>`}</ol>${until}</section>`;
}

// A text's size as a prompt counts it: its bytes, and about a token for every four characters,
// as HQ reckons a briefing's (`situations::tokens`).
function replaySize(text) {
  const t = String(text == null ? "" : text);
  return `${new TextEncoder().encode(t).length} bytes · about ${Math.ceil([...t].length / 4)} tokens`;
}

// The handover in a briefing (ADR 0142): from "Handover of #" to the line that ends it, as HQ
// writes it (`domain::handover`). The briefing without it, and it.
function replayHandover(briefing) {
  const from = briefing.indexOf("Handover of #"), end = "(End of the handover.)";
  const to = from < 0 ? -1 : briefing.indexOf(end, from);
  if (to < 0) return [briefing, ""];
  return [`${briefing.slice(0, from)}[the handover, below]${briefing.slice(to + end.length)}`, briefing.slice(from, to + end.length)];
}

// What a turn was given (the owner's notes of 31 Oct): its first call's prompt as the model
// received it, from the conversation HQ keeps (ADR 0129), each section folded with its size:
// the instructions, the tools by name, the briefing (its first message) and the handover in it.
// The node has scrubbed every word of it of secrets on the way here. What HQ did not keep, it
// says.
function replayGiven(r) {
  const t = r.transcript, parts = t ? replayParts(t.text) : [];
  const first = (kind, name) => parts.find((p) => p.kind === kind && (name == null || p.name === name));
  const system = first("system"), user = first("user"), upFront = first("tools", ""), held = first("tools", "held back");
  const told = (r.moments || []).find((m) => m.kind === "briefing");
  const briefing = user ? user.body : told ? String(told.text || "") : "";
  const [rest, handover] = replayHandover(briefing);
  const names = (p) => (p ? p.body.split("\n").filter((n) => n.trim()) : []);
  const now = names(upFront), later = names(held);
  const section = (title, size, inner) => `<li><details class="hq-replay-part"><summary>${title} <span class="hq-who">${esc(size)}</span></summary>${inner}</details></li>`;
  const text = (s) => `<pre>${esc(s)}</pre>`;
  const list = (all) => `<span class="hq-replay-names">${all.map((n) => `<code>${esc(n)}</code>`).join("")}</span>`;
  const sections = [];
  if (system) sections.push(section("The instructions", replaySize(system.body), text(system.body)));
  if (now.length || later.length) {
    const said = [...now, ...later].join("\n");
    sections.push(section(`The tools: ${now.length} at the start${later.length ? ` · ${later.length} held back` : ""}`, replaySize(said),
      `${list(now)}${later.length ? `<p class="hq-who">Held back, brought in when it asked:</p>${list(later)}` : ""}`));
  }
  if (briefing) sections.push(section(user ? "The first message: the briefing" : "The briefing, from HQ&#39;s record", replaySize(rest), text(rest)));
  if (handover) sections.push(section("The handover", replaySize(handover), text(handover)));
  const missing = [];
  if (!t) missing.push("HQ keeps no conversation for this turn (it keeps one for 30 days, from turns since ADR 0129), so its instructions and tools are not shown.");
  if (t && t.runtime === "claude-code") missing.push("The instructions Claude Code itself starts with are not kept: only the rules Endora added to them.");
  if (t && !system) missing.push("Its instructions were not kept.");
  if (t && !upFront && !held) missing.push("The tools it was offered were not kept: turns before 31 Oct 2026 did not record them.");
  missing.push("Of the tools, only their names are kept, never their descriptions or schemas.");
  const whole = [system && system.body, [...now, ...later].join("\n"), briefing].filter(Boolean).join("\n");
  return `<section class="hq-card"><details class="hq-replay-given"${REPLAY.given ? " open" : ""}><summary>What it was given</summary>
    <p class="hq-who">Its first prompt as the model received it, ${esc(replaySize(whole))}. Anything shaped like a secret is replaced with [redacted].</p>
    <ul class="hq-replay-parts">${sections.join("") || `<li class="hq-empty">Nothing of it was kept.</li>`}</ul>
    ${missing.map((m) => `<p class="hq-note">${esc(m)}</p>`).join("")}</details></section>`;
}

// A turn's header: who, why, what it came to, what it cost, how long.
function replayHead(r) {
  const m = r.member, money = r.money || {}, tokens = r.tokens || {};
  const who = m ? esc(m.name) : "someone no longer on the team";
  const brain = m ? esc(m.brain_label || m.brain || "") : "";
  const ran = r.started_at ? replaySpan((r.ended_at || Date.now()) - r.started_at) : "";
  const cost = `${hqDollars(r.money && r.money.spent)} spent · ${hqDollars(money.reserved)} held · ${hqDollars(money.released)} released`;
  const input = (tokens.input || 0) + (tokens.cache_read || 0) + (tokens.cache_write || 0);
  const live = r.live ? ` <span class="hq-tag hq-live" role="status">Live</span>` : "";
  return `<div class="hq-replay-who">${replayFace(m)}<div><div class="hq-sel">R${Number(r.run)} · ${who}${live}</div>
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
  // Opened from a line of Activity, it goes back to that place (the owner, 1 Nov).
  const back = HQ.back
    ? `<div class="hq-row"><button class="hq-btn" data-act="hq:back">◀ Back to Activity</button></div>`
    : `<div class="hq-row"><button class="hq-btn" data-act="go:hq">◀ Back to HQ</button></div>`;
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
    <section class="hq-card">${replayHead(r)}${r.live ? `<p class="hq-note">This turn is still running: what it says is read again every few seconds until it ends.</p>` : ""}</section>
    ${replayGiven(r)}
    <section class="hq-card"><h2>What happened</h2>${byTime}<ol class="hq-replay-line">${moments || `<li class="hq-empty">Nothing recorded.</li>`}</ol></section>
    ${r.transcript ? replayTalk(r.transcript, r.member) : ""}
    ${steps}`);
}

// The links into Replay, for the HQ screen: from a run line, and from a task card.
function replayRunLink(run, text) { return `<button class="hq-link" data-act="replay:run:${Number(run)}">${text}</button>`; }
// What a turn on a task was given, opened unfolded in its replay.
function replayGivenLink(run) { return `<button class="hq-link" data-act="replay:given:${Number(run)}">What it was given</button>`; }
function replayTaskLink(task) { return `<div class="hq-row"><button class="hq-btn" data-act="replay:task:${Number(task)}">Replay this task</button></div>`; }

async function replayDispatch(verb, noun, id) {
  if (!(verb === "replay")) return false;
  if (noun === "run" || noun === "task") { REPLAY.given = false; go("replay", `${noun}-${Number(id)}`); return true; }
  if (noun === "given") { REPLAY.given = true; go("replay", `run-${Number(id)}`); return true; }
  if (noun === "step" && REPLAY.runs) {
    REPLAY.at = Math.min(Math.max(0, REPLAY.at + (id === "next" ? 1 : -1)), REPLAY.runs.length - 1);
    render(); window.scrollTo(0, 0); return true;
  }
  return true;
}
