// Append a chat bubble to the thread and keep the newest in view.
function appendBubble(html, cls, id) {
  const thread = document.getElementById("chat-thread");
  if (!thread) return null;
  const mine = cls.split(" ").includes("me");
  const wrap = document.createElement("div");
  wrap.className = "row msg-row" + (mine ? " mine" : "");
  if (id) wrap.id = id;
  wrap.innerHTML = `<div class="bubble ${cls}">${html}</div>`;
  thread.appendChild(wrap);
  scrollBubbleIntoView(wrap);
  return wrap;
}

// Scroll a chat row fully into view ABOVE the sticky composer (which otherwise
// overlaps the newest message). Reserves the composer's real height.
function scrollBubbleIntoView(el) {
  if (!el) return;
  el.style.scrollMarginBottom = "16px";
  el.scrollIntoView({ block: "end", behavior: "smooth" });
}

// Render the live action trail into its panel. Collapsible via native <details>:
// open while working, collapsed to a one-line summary once the turn finishes.
const STEP_ICON = { running: '<span class="sspin"></span>', done: '<span class="sdone">✓</span>',
  failed: '<span class="sfail">✕</span>', blocked: '<span class="sblock">⊘</span>' };
function renderSteps(wrap, steps, running) {
  if (!wrap) return;
  if (!steps.length) { wrap.innerHTML = ""; wrap.style.display = "none"; return; }
  wrap.style.display = "";
  const rows = steps.map((s) => {
    const ic = STEP_ICON[s.status] || "";
    // A step with output is expandable — click to reveal what the skill returned.
    if (s.output) {
      const tag = s.skill === "thinking" ? "div" : "pre";
      return `<details class="step-item"><summary class="step-row">${ic}<span>${esc(s.label)}</span><span class="step-more">details</span></summary>` +
        `<${tag} class="step-out${s.skill === "thinking" ? " thought" : ""}">${esc(s.output)}</${tag}></details>`;
    }
    return `<div class="step-row">${ic}<span>${esc(s.label)}</span></div>`;
  }).join("");
  const n = steps.length;
  const head = running ? "Working…" : `${n} step${n > 1 ? "s" : ""}`;
  const lead = running ? '<span class="sspin"></span>' : icon("sparkle", 13);
  wrap.innerHTML =
    `<details class="steps" ${running ? "open" : ""}><summary>${lead}<span>${head}</span></summary>` +
    `<div class="step-list">${rows}</div></details>`;
}

// The http(s) URLs a skill returned — real sources, never guessed. Trailing
// punctuation is trimmed so a URL at the end of a sentence stays clean.
function extractUrls(text) {
  if (!text) return [];
  return (text.match(/https?:\/\/[^\s"'<>)]+/g) || []).map((u) => u.replace(/[.,;:]+$/, ""));
}

// Render a "Sources" chip row (deduped) beneath the reply, from the URLs the
// turn's skills actually returned. Nothing shows when no skill returned a link —
// so a source list only ever appears when there genuinely was one.
function renderSources(afterEl, steps) {
  const seen = new Set();
  const urls = [];
  for (const s of steps) for (const u of extractUrls(s.output)) if (!seen.has(u)) { seen.add(u); urls.push(u); }
  if (!urls.length || !afterEl) return;
  const host = (u) => { try { return new URL(u).hostname.replace(/^www\./, ""); } catch (_) { return u; } };
  const links = urls.slice(0, 8)
    .map((u) => `<a class="src-link" href="${esc(u)}" target="_blank" rel="noopener noreferrer" title="${esc(u)}">${esc(host(u))}</a>`)
    .join("");
  const box = document.createElement("div");
  box.className = "row";
  box.style.cssText = "justify-content:flex-start; margin:2px 0 10px;";
  box.innerHTML = `<div class="sources"><span class="src-label">Sources</span>${links}</div>`;
  afterEl.insertAdjacentElement("afterend", box);
}

// Trim a long tool result to something readable, keeping the start (where the useful
// part is) and saying how much was left out rather than trailing off silently.
function clip(text, max) {
  const s = String(text || "");
  if (s.length <= max) return s;
  return s.slice(0, max) + `… (+${s.length - max} more characters)`;
}

// Nominate the tool that reads a server's state (ADR 0054). Blank clears it.
async function setReader(el) {
  const server = el.getAttribute("data-reader-for");
  try {
    await api("POST", "/v1/mcp/servers/" + encodeURIComponent(server) + "/reader", { reader_tool: el.value });
    flash(el.value ? `Endora will check ${server} with ${el.value}.` : `No read-back for ${server}.`, "ok");
  } catch (e) {
    flash("Couldn't set that: " + e.message, "err");
  }
  return reload();
}

// Which of a server's tools READS its state (ADR 0054).
//
// One answer settles two things: that tool's result becomes an observation rather than
// a receipt, and everything else on the server is checked through it after it acts.
// Without it Endora can only repeat what a tool claimed about its own work — honest,
// but unverifiable. Endora never picks this itself: a server's own say-so about what
// it does is not evidence.
function readerRow(s) {
  // Tools arrive namespaced as `server.tool`; the nomination is the bare tool name.
  const prefix = s.name + ".";
  const tools = (s.tools || [])
    .map((t) => (t.id || "").startsWith(prefix) ? t.id.slice(prefix.length) : t.id)
    .filter(Boolean);
  if (!tools.length) return "";
  const opts = ['<option value="">— nobody has said —</option>']
    .concat(tools.map((t) => `<option value="${esc(t)}"${t === s.reader_tool ? " selected" : ""}>${esc(t)}</option>`))
    .join("");
  // Stacked, not side by side. A `<select>` sizes itself to its longest option, and
  // `HassMediaSearchAndPlay` is long enough to squeeze the label beside it down to one word
  // per line — which is what it did on a real screen. Label above, control below, full
  // width: the same shape as every other field, and nothing to squeeze.
  return `
    <div class="field" style="margin-top:8px;">
      <label>Which tool reads this server's state?</label>
      <div class="sub" style="margin:-2px 0 4px;">Endora uses it to check what an action actually did, instead of taking the tool's word for it.</div>
      <select data-reader-for="${esc(s.name)}" onchange="setReader(this)" style="width:100%;">${opts}</select>
    </div>`;
}

// HTML-string versions for rendering a PAST message's persisted actions in the
// chat history (collapsed; click to expand). Same look as the live panel.
// What Endora actually DID this turn, and whether it confirmed the effect (ADR 0053).
//
// Not collapsible and not buried under "details", unlike the step trail: the whole
// point is that it is visible without being asked for. The model ignores the read-back
// roughly two runs in three and asserts unverified success every time, so the reply
// above this may well claim the opposite of what it says here. Both are shown; the
// person judges. Nothing here edits the reply.
// A subtle note of what Endora did behind the scenes on THIS turn — what it looked up and
// what it learned. Rendered per message from the stored record, so it is still there when
// you come back to the chat (ADR 0056). It used to be drawn from a live stream event held
// in memory, which meant the note existed only while you stayed on the screen.
// What a reply's activity line shows in the chat. A fact Endora noted about the owner
// ("Noted in tech: You value reliability…") is left out: it read as running commentary on
// the person, line after line, and lives on in About you, where it can be seen and changed.
function shownActivity(activity) {
  return (Array.isArray(activity) ? activity : []).filter((line) => !/^Noted in /.test(String(line)));
}

function activityHtml(activity) {
  const shown = shownActivity(activity);
  if (!SHOW_ACTIVITY || !shown.length) return "";
  return `<div class="activity">${icon("sparkle", 13)} ${shown.map(esc).join(" · ")}</div>`;
}

// `latest` adds the one question worth asking: did that help?
//
// It is shown on the NEWEST turn only, and never anywhere else. The machinery for judging
// an outcome has existed for months and had never once been used, because the only place
// to say so was a section further down a screen nobody opens — which, by its own design,
// never asked. A loop with no input is not a loop.
//
// Still no badge, no counter, nothing that accumulates: the ask is gone by the next turn
// whether or not it was answered, so ignoring it stays free. That is the anti-queue rule
// (ADR 0052) kept, while actually asking once, where you already are.
function actionsTakenHtml(actions, latest) {
  if (!actions || !actions.length) return "";
  const rows = actions.map((a) => {
    const ask = (latest && a.outcome)
      ? `<span class="step-more" style="margin-left:8px;">
           <button class="ghost" data-act="react:helped:${a.outcome}" title="that helped" style="padding:1px 7px;font-size:12px;">Helped</button>
           <button class="ghost" data-act="react:did_not_help:${a.outcome}" title="that didn't help" style="padding:1px 7px;font-size:12px;">Didn't</button>
         </span>`
      : "";
    const what = `<span class="step-more">${esc(a.skill)}</span>`;
    if (a.confirmed) {
      // Bounded: a read-back can run to thousands of characters, and pasting the whole
      // thing under the reply buries it. The full text is still in the step trail.
      return `<div class="step-row">${icon("check", 13)}<span>${what} — Endora checked afterwards: ${esc(clip(a.observed, 240))}</span>${ask}</div>`;
    }
    return `<div class="step-row">${icon("target", 13)}<span>${what} — reported ${esc(clip(a.claimed, 240))}, <strong>not confirmed</strong>. Endora couldn't check this one for itself.</span>${ask}</div>`;
  }).join("");
  return `<div class="row" style="justify-content:flex-start;margin:2px 0;"><div class="steps" style="padding:8px 10px;"><div class="step-list">${rows}</div></div></div>`;
}
function stepsHtml(steps) {
  if (!steps || !steps.length) return "";
  const rows = steps.map((s) => {
    const ic = STEP_ICON[s.status] || "";
    if (s.output) {
      const tag = s.skill === "thinking" ? "div" : "pre";
      return `<details class="step-item"><summary class="step-row">${ic}<span>${esc(s.label)}</span><span class="step-more">details</span></summary><${tag} class="step-out${s.skill === "thinking" ? " thought" : ""}">${esc(s.output)}</${tag}></details>`;
    }
    return `<div class="step-row">${ic}<span>${esc(s.label)}</span></div>`;
  }).join("");
  const n = steps.length;
  return `<div class="row msg-row aux"><details class="steps"><summary>${icon("sparkle", 13)}<span>${n} step${n > 1 ? "s" : ""}</span></summary><div class="step-list">${rows}</div></details></div>`;
}
function sourcesHtml(urls) {
  if (!urls || !urls.length) return "";
  const host = (u) => { try { return new URL(u).hostname.replace(/^www\./, ""); } catch (_) { return u; } };
  const links = urls.slice(0, 8).map((u) => `<a class="src-link" href="${esc(u)}" target="_blank" rel="noopener noreferrer" title="${esc(u)}">${esc(host(u))}</a>`).join("");
  return `<div class="row" style="justify-content:flex-start;margin:2px 0 10px;"><div class="sources"><span class="src-label">Sources</span>${links}</div></div>`;
}

// Send a message to the butler and stream the reply token-by-token (SSE): the
// person's message shows at once, then the butler's bubble grows live as the
// model produces prose, finishing with the "done" event. On the
// server the message is persisted first and the reply when complete, so a reload
// always reflects the true state (that's why we just reload() at the end).
// Toggle the composer's primary button between Send and Stop to match state.
function updateComposerButton() {
  const btn = document.getElementById("send-btn");
  if (!btn) return;
  btn.dataset.act = CHAT_STREAMING ? "chat:stop" : researching() ? "research:start" : "chat:send";
  const label = CHAT_STREAMING ? "Stop" : researching() ? "Start research" : "Send";
  btn.title = label;
  btn.setAttribute("aria-label", label);
  btn.innerHTML = icon(CHAT_STREAMING ? "stop" : "up", 18);
}

// Update the Speak button in place (icon + label) without a full render, so
// toggling it never disturbs a streaming reply.
function updateSpeakButton() {
  refreshComposerModes();
}

// Update the Deep-mode toggle in place (same reasoning as the Speak button).
function updateDeepButton() {
  refreshComposerModes();
}

// Send a question to the deep (bigger) model. It persists both the question and the
// answer server-side, so a reload shows the exchange. Keeps the input until it goes
// through, so nothing is lost if the deep model is off or unreachable.
async function askDeep(q, input) {
  appendBubble(esc(q), "me");
  flash("Asking the deep model…", "ok");
  try {
    const r = await api("POST", "/v1/deep-ask", { question: q });
    if (r && r.answered === false) { flash(r.note || "No deep model configured.", "err"); return; }
    if (input) { input.value = ""; growInput(input); }
  } catch (e) { flash("Deep model: " + e.message, "err"); return; }
  return reload();
}

// Stop the in-flight turn and drop anything still queued.
// A short buzz for the moments you may not be looking at the screen: a reply
// landing, and the mic actually starting to listen. Android honours it; iOS ignores
// vibrate entirely, so this is a bonus rather than a signal anything depends on.
function buzz(ms) {
  if (HAPTIC && navigator.vibrate) navigator.vibrate(ms);
}

function stopChat() {
  CHAT_QUEUE = [];
  // Remember that THIS was deliberate. The reload in the stream's `finally` renders
  // from persisted state, which ends with the person's message and no reply — so the
  // thinking indicator comes back and never leaves, because the reply it is waiting
  // for was cancelled. Marking the stop lets the render say so instead.
  CHAT_STOPPED = true;
  if (CHAT_ABORT) CHAT_ABORT.abort();
}

// Enqueue a message. Turns are SERIALIZED: a single local GPU (especially the
// router+synth mixture) thrashes and turns cross-talk if two run at once, so we
// never fire a second /v1/chat/stream while one is in flight — we show the
// message, queue it, and drain in order.
function sendChat() {
  const input = document.getElementById("chat-input");
  const msg = input ? input.value.trim() : "";
  if (!msg) return;
  // A new turn: whatever was stopped before is history.
  CHAT_STOPPED = false;
  // Talking always happens today, so a reply can never land in a day you were reading.
  CHAT_DAY = null;
  const thread = document.getElementById("chat-thread");
  if (thread && thread.querySelector(".empty")) thread.innerHTML = "";
  // Deep mode on: route to the bigger model instead of the everyday butler. Keep the
  // text until it lands (askDeep clears it on success).
  // A private message never goes to the deep model.
  if (DEEP_MODE && !PRIVATE_MODE && DEEP_MODEL && DEEP_MODEL.configured) { askDeep(msg, input); return; }
  if (input) { input.value = ""; growInput(input); }
  appendBubble((PRIVATE_MODE ? `<span class="private-lock">${icon("lock", 12)}</span>` : "") + esc(msg), "me"); // show it immediately, even if it waits its turn
  CHAT_QUEUE.push(PRIVATE_MODE ? { message: msg, private: true } : msg);
  drainChat();
}

async function startResearch() {
  // Research goes out to web search and the models: never with Private on (ADR 0105), even
  // from a button drawn before Private was switched on.
  if (PRIVATE_MODE) {
    flash("Private is on, and research goes out to web search. Turn Private off to commission it.", "err");
    return;
  }
  const input = document.getElementById("chat-input");
  const assignment = input ? input.value.trim() : "";
  if (!assignment) return;
  const raw = (document.getElementById("research-urls") || {}).value || "";
  const source_urls = raw.split(/[\n,]+/).map((url) => url.trim()).filter(Boolean);
  const search = (RESEARCH_READY.search_tools || [])[0];
  const signature = JSON.stringify({ assignment, source_urls, search: search ? search.id : null });
  if (!RESEARCH_PENDING || RESEARCH_PENDING.signature !== signature) {
    const key = globalThis.crypto && crypto.randomUUID
      ? crypto.randomUUID() : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    RESEARCH_PENDING = { key, signature };
    localStorage.setItem("endora.researchPending", JSON.stringify(RESEARCH_PENDING));
  }
  await api("POST", "/v1/research", {
    request_key: RESEARCH_PENDING.key, assignment, source_urls,
    search_capability_id: search ? search.id : null,
  });
  RESEARCH_PENDING = null;
  localStorage.removeItem("endora.researchPending");
  if (input) input.value = "";
  flash("Research started. You can close this page; Endora will keep working.", "ok");
  return reload();
}

async function downloadResearch(id) {
  const res = await fetch(`/v1/research/${encodeURIComponent(id)}/report`, { headers: signed({}) });
  if (!res.ok) throw new Error("report is not ready");
  const blob = await res.blob();
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = "endora-research.md";
  link.click();
  URL.revokeObjectURL(url);
}

// Process the queue one turn at a time.
async function drainChat() {
  if (CHAT_STREAMING || !CHAT_QUEUE.length) return;
  const queued = CHAT_QUEUE.shift();
  const msg = typeof queued === "string" ? queued : queued.message;
  const requestId = queued.request_id || `chat-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  const isPrivate = typeof queued === "object" && !!queued.private;
  LAST_CHAT_REQUEST = { message: msg, request_id: requestId, private: isPrivate };
  // Remember the in-flight turn in module state so a re-render (e.g. toggling
  // Speak) can rebuild the just-sent message and the reply growing in, instead of
  // wiping them (they aren't in the persisted DB.messages until the turn ends).
  CHAT_INFLIGHT = isPrivate ? { message: msg, private: true } : msg;
  LIVE_REPLY = "";
  // A live action trail sits just above the reply bubble; it fills in as the
  // butler routes to skills, and collapses to a summary when the turn ends.
  STEP_LIST = [];
  const thread0 = document.getElementById("chat-thread");
  const stepsWrap = document.createElement("div");
  stepsWrap.className = "row";
  stepsWrap.style.cssText = "justify-content:flex-start; margin:6px 0; display:none;";
  if (thread0) thread0.appendChild(stepsWrap);
  // A butler bubble that starts as a "thinking" indicator and grows with tokens.
  const live = appendBubble('<span class="dots"><i></i><i></i><i></i></span>', "butler", "chat-live");
  let acc = "";
  CHAT_STREAMING = true;
  CHAT_ABORT = new AbortController();
  updateComposerButton();
  try {
    const res = await fetch("/v1/chat/stream", {
      method: "POST",
      headers: signed({ "content-type": "application/json" }),
      body: JSON.stringify({ message: msg, request_id: requestId, private: isPrivate }),
      signal: CHAT_ABORT.signal,
    });
    if (!res.ok || !res.body) throw new Error("stream unavailable");
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buf = "";
    let finished = false;
    while (!finished) {
      const { value, done } = await reader.read();
      if (done) break;
      buf += decoder.decode(value, { stream: true });
      // SSE frames are separated by a blank line; each carries a `data:` JSON.
      let sep;
      while ((sep = buf.indexOf("\n\n")) >= 0) {
        const frame = buf.slice(0, sep);
        buf = buf.slice(sep + 2);
        const dataLine = frame.split("\n").find((l) => l.startsWith("data:"));
        if (!dataLine) continue;
        let ev;
        try { ev = JSON.parse(dataLine.slice(5).trim()); } catch (_) { continue; }
        if (ev.type === "token") {
          acc += ev.text;
          LIVE_REPLY = acc;
          // Look the bubble up fresh each token: if a re-render replaced the thread
          // mid-stream, viewChat rebuilt a #chat-live from LIVE_REPLY and we keep
          // writing into that one rather than a now-detached node.
          const liveRow = document.getElementById("chat-live");
          const bod = liveRow && liveRow.querySelector(".bubble");
          // Rendered, not textContent, so a link arriving mid-stream reads as a
          // link and never as a wall of URL. `rich` escapes before it dresses, so
          // a streamed token can no more become markup than a stored one.
          if (bod) bod.innerHTML = rich(acc);
          if (liveRow) scrollBubbleIntoView(liveRow);
        } else if (ev.type === "step") {
          // Working notes are not the answer: file them in the trail and empty the bubble,
          // so the reply never shows words that are later taken back.
          if (ev.skill === "thinking") {
            acc = "";
            LIVE_REPLY = "";
            const liveRow = document.getElementById("chat-live");
            const bod = liveRow && liveRow.querySelector(".bubble");
            if (bod) bod.innerHTML = '<span class="dots"><i></i><i></i><i></i></span>';
          }
          if (ev.status === "running") {
            STEP_LIST.push({ skill: ev.skill, label: ev.label, status: "running", output: null });
          } else {
            // Terminal: finalize the last still-running step, else record it fresh
            // (a "blocked" step arrives with no prior "running").
            // Match on skill as well as state: a blocked call reports no "running"
            // at all, so finalising whatever happened to be in flight would overwrite
            // an unrelated call and hide the refusal.
            let i = STEP_LIST.length - 1;
            while (i >= 0 && !(STEP_LIST[i].status === "running" && STEP_LIST[i].skill === ev.skill)) i--;
            if (i >= 0) { STEP_LIST[i].status = ev.status; STEP_LIST[i].output = ev.output || null; }
            else STEP_LIST.push({ skill: ev.skill, label: ev.label, status: ev.status, output: ev.output || null });
          }
          renderSteps(stepsWrap, STEP_LIST, true);
          if (live) scrollBubbleIntoView(live);
        } else if (ev.type === "done") {
          // A reply landed, so nothing is outstanding.
          CHAT_STOPPED = false;
          buzz(25);
          LAST_ACTIVITY = ev.activity || [];
          LAST_ACTIVITY_MSG = ev.reply && ev.reply.id;
          renderSteps(stepsWrap, STEP_LIST, false); // collapse to a summary
          renderSources(live, STEP_LIST); // real source links beneath the reply
          // Read as it is shown: a private reply without its mark.
          speak(ev.reply && shownText(ev.reply.text).text);
          finished = true;
        } else if (ev.type === "error") {
          throw new Error(ev.message || "the butler couldn't reply");
        }
      }
    }
  } catch (e) {
    if (e && e.name === "AbortError") {
      // Stopped by the person: leave a note; the server keeps whatever it saved.
      const liveRow = document.getElementById("chat-live");
      const bod = liveRow && liveRow.querySelector(".bubble");
      if (bod) bod.textContent = "(stopped)";
      renderSteps(stepsWrap, STEP_LIST, false);
    } else {
      // Don't re-send (the server may have already saved the turn) — reload to the
      // true persisted state below.
      flash("The butler's reply was interrupted — your message was saved.", "err");
    }
  } finally {
    CHAT_STREAMING = false;
    CHAT_ABORT = null;
    CHAT_INFLIGHT = null;
    LIVE_REPLY = "";
    const liveRow = document.getElementById("chat-live");
    if (liveRow) liveRow.removeAttribute("id"); // free "chat-live" for the next turn
    updateComposerButton();
    if (CHAT_QUEUE.length) {
      // More queued: don't full-reload (it would wipe the not-yet-persisted queued
      // bubbles); settle this reply in place and process the next turn.
      drainChat();
    } else {
      try { await reload(); } catch (_) {}
      const again = document.getElementById("chat-input");
      if (again) again.focus();
    }
  }
}

