// ---- lifecycle (North Stars & Targets) ------------------------------------
// Everything that lives under Settings, in one place.
//
// The Settings list and each screen's breadcrumb are rendered from **this** rather than
// written out twice, because written out twice is how they drifted: four screens said
// "Home › Skills" when they were two levels down, one had no breadcrumb at all, and two
// different screens both called themselves "Preferences" in the trail — the one actually
// named Preferences, and the one the menu calls "Things Endora remembers about you".
//
// `short` is what the breadcrumb says when the full label is too long to sit in a trail. Two
// renderings of one entry, side by side, so they cannot quietly disagree.
//
// What Endora has done is not here: it is a record, not a setting, and sits beside the Inbox
// in the menu. What you told Endora and what it has learned are one screen, "About you":
// they were two, side by side, and nothing said which was which.
const UNDER_SETTINGS = [
  { view: "display", icon: "speakerOn", label: "Notifications & voice", short: "Notifications", note: "your daily brief, check-ins, reading aloud and vibration" },
  { view: "models", icon: "sparkle", label: "AI", note: "every model Endora uses, where it runs, and what it sees" },
  { view: "connections", icon: "gear", label: "Connections", note: "Home Assistant, your home lab's services, and tool servers" },
  { view: "skills", icon: "skills", label: "Skills", note: "what Endora can do in chat, and skills you teach it" },
  { view: "permissions", icon: "scale", label: "Permissions", note: "how far Endora may act on its own" },
  { view: "watching", icon: "clock", label: "What Endora watches", short: "Watching", note: "what it does about each kind of thing, and what you told it to leave alone" },
  { view: "understanding", icon: "person", label: "About you", note: "what you've told it, and the pages of what it knows" },
  { view: "signin", icon: "lock", label: "Your sign-in", note: "password, authenticator, and tokens for scripts" },
  { view: "data", icon: "export", label: "Your data", note: "take a copy of what Endora knows, or delete it" },
];

// The trail for a screen under Settings. Anything not under Settings gets the short trail, so
// a top-level screen cannot accidentally claim to be nested.
function settingsCrumbs(view) {
  const page = UNDER_SETTINGS.find((p) => p.view === view);
  const here = page ? (page.short || page.label) : view;
  return crumbs([
    { label: "Home", act: "go:chat" },
    { label: "Settings", act: "go:settings" },
    { label: here },
  ]);
}

function crumbs(parts) {
  return `<div class="crumbs">` +
    parts.map((p, i) => i < parts.length - 1
      ? `<a data-act="${p.act}">${esc(p.label)}</a> › `
      : `<span>${esc(p.label)}</span>`).join("") +
    `</div>`;
}

// Takes rows as an array OR as already-joined html, because callers do both and the
// difference is invisible until it throws: `"".length` is 0, so a joined-string caller looks
// fine while it has nothing to show and breaks the moment it has something. Found exactly
// that way — the Models screen threw as soon as a model search returned a result.
function listOr(items, emptyText) {
  const html = Array.isArray(items) ? items.join("") : String(items ?? "");
  return html ? html : `<div class="empty">${esc(emptyText)}</div>`;
}

// A North Star card, reused across the value groups on the home view.
const ACTIVITY_ICON = { observation: "note", decision: "scale", action: "sparkle" };
function activityFeed() {
  if (!ACTIVITY.length) return `<div class="empty">No activity yet.</div>`;
  return ACTIVITY.map((a) => `
    <div class="card"><div class="row">
      <div class="grow"><div class="title">${icon(ACTIVITY_ICON[a.kind] || "note", 15)} ${esc(a.summary)}</div>
      <div class="sub">${esc(a.kind)}</div></div>
      <span class="sub">${esc(new Date(a.at_ms).toLocaleString())}</span>
    </div></div>`).join("");
}

// Everything Endora has actually done, in one place.
//
// These were spread over two screens for no reason a person would recognise: how its actions
// landed, what it changed inside somebody else's service, and what it claimed against what it
// saw all sat under "What Endora understands" — beside beliefs about the person, which they
// have nothing to do with. Understanding is a model of you; this is a receipt.
//
// Read-only, apart from saying how an outcome landed. Nothing here is a task and nothing
// accumulates that must be cleared: the record grows because things happened.
function viewAudit() {
  const rows = (DB.audit || []).map((a) => `
    <div class="card"><div class="sub mono">${new Date(a.at_ms).toLocaleString()}</div>
      <div>${esc(a.summary)}</div></div>`);
  return `
    ${crumbs([{ label: "Home", act: "go:chat" }, { label: "Activity" }])}
    <h2>What Endora has done</h2>
    <div class="note">Its own account of its work — what it tried, whether it checked, and what it changed. Nothing here needs anything from you.</div>
    ${viewHowItLands()}
    ${viewOutcomes()}
    ${viewConfigWrites()}
    <h3 style="margin-top:22px;">Everything, newest first</h3>
    <div class="note">What it did, in its own words, and every decision policy made about it.</div>
    ${activityFeed()}
    ${listOr(rows, "No decisions recorded yet.")}`;
}

// ---- voice (browser Web Speech API; STT may use a cloud service) ---
let SPEAK = localStorage.getItem("endora.speak") === "1";      // read replies aloud (default OFF)
let DEEP_MODE = localStorage.getItem("endora.deepmode") === "1"; // route sends to the deep model (default OFF)
// Sends marked 🔒 (ADR 0105): Restricted, answered only on the house's own network, never
// sent out even disguised, and never noted into the brain. Sticky until turned off.
let PRIVATE_MODE = localStorage.getItem("endora.privatemode") === "1";
const PRIVATE_MARK = "[private] ";
// A message as it is shown: a private one without its mark, and with a lock.
function shownText(text) {
  const t = String(text || "");
  return t.startsWith(PRIVATE_MARK) ? { text: t.slice(PRIVATE_MARK.length), private: true } : { text: t, private: false };
}
const TTS = window.speechSynthesis;
const STT = window.SpeechRecognition || window.webkitSpeechRecognition;
// iOS Safari only lets speech start after a user gesture unlocks it; speaking a
// silent utterance the moment the person taps "Speak" unlocks it for the later,
// async replies. (This is best-effort — a fully reliable mobile voice is the
// local Kokoro TTS path.)
function unlockSpeech() {
  if (!TTS) return;
  try {
    const u = new SpeechSynthesisUtterance(" ");
    u.volume = 0;
    TTS.speak(u);
  } catch (_) {}
}
function speak(text) {
  // In a hands-free conversation, resume listening once the reply is spoken — or
  // right away if there's nothing to speak, so the loop never stalls.
  if (!SPEAK || !TTS || !text) return;
  TTS.cancel();
  const u = new SpeechSynthesisUtterance(text);
  TTS.speak(u);
  // iOS sometimes pauses the queue; a nudge keeps it going.
  if (TTS.paused) TTS.resume();
}
function readAloud(text) {
  if (!TTS || !text) { flash("This browser can't read text aloud.", "err"); return; }
  TTS.cancel();
  TTS.speak(new SpeechSynthesisUtterance(text));
  if (TTS.paused) TTS.resume();
}

function listen() {
  if (!STT) { flash("Speech recognition isn't available in this browser (try Chrome/Edge).", "err"); return; }
  if (!window.isSecureContext) {
    flash("Voice input needs a secure page (HTTPS or localhost). Over plain http the browser blocks the mic — the butler can still speak its replies.", "err");
    return;
  }
  const rec = new STT();
  rec.lang = "en-US"; rec.interimResults = false; rec.maxAlternatives = 1;
  rec.onresult = (e) => {
    const input = document.getElementById("chat-input");
    if (input) { input.value = e.results[0][0].transcript; input.focus(); }
  };
  rec.onerror = (e) => {
    const err = e && e.error;
    if (err === "not-allowed" || err === "service-not-allowed") {
      flash("The browser blocked the mic. Allow microphone access, and use an HTTPS/localhost page.", "err");
    } else {
      flash("Couldn't capture speech" + (err ? " (" + err + ")" : "") + ".", "err");
    }
  };
  rec.onstart = () => buzz(15); // the mic is live — worth knowing without looking
  try { rec.start(); } catch (_) { flash("Couldn't start voice input.", "err"); }
}


// The butler chat: the conversation and an input. Anything the butler does in
// the world runs through the policy boundary as it happens — there is no queue
// of proposals for you to approve afterwards.
// Which local day a moment falls in, as YYYY-MM-DD.
//
// Computed on the CLIENT because the server has no idea what timezone you are in — it
// stores a moment, and only the browser knows which day that was where you are.
function dayOf(at) {
  const d = new Date(at);
  const local = new Date(d.getTime() - d.getTimezoneOffset() * 60000);
  return local.toISOString().slice(0, 10);
}

// The days that actually have conversation, oldest first.
//
// Derived, never stored: a message already carries the moment it happened, so a "day" is
// a filter and not a thing to archive. No job to run at midnight, nothing to migrate,
// nothing to go wrong while you sleep — and no day can be lost, because none was ever
// moved.
function chatDays() {
  return (CHAT_DAYS || []).map((d) => d.day);
}

// The moment a local day starts, and the one after it — the window the server is asked
// for. The browser knows where its own midnight falls; the server never has to.
function dayWindow(day) {
  const from = new Date(`${day}T00:00:00`).getTime();
  return { from, to: from + 86400000 };
}

// Load one day's conversation, and which days exist. Everything the console shows comes
// from these two, so a five-year-old install costs exactly as much as a fresh one.
async function loadChatDay(day) {
  const showing = day || dayOf(Date.now());
  const { from, to } = dayWindow(showing);
  try {
    CHAT_MSGS = await api("GET", `/v1/chat?from=${from}&to=${to}`);
  } catch (_) { CHAT_MSGS = []; }
  try {
    CHAT_DAYS = await api("GET", `/v1/chat/days?offset_minutes=${-new Date().getTimezoneOffset()}`);
  } catch (_) { CHAT_DAYS = []; }
}

// A day in words, for the header.
function dayInWords(day) {
  if (day === dayOf(Date.now())) return "Today";
  const yesterday = dayOf(Date.now() - 86400000);
  if (day === yesterday) return "Yesterday";
  return new Date(`${day}T12:00:00`).toLocaleDateString(undefined, {
    weekday: "long", month: "short", day: "numeric",
  });
}

function viewChat() {
  const days = chatDays();
  const today = dayOf(Date.now());
  // Default to today even before it has anything in it, so a new day starts clean.
  const showing = CHAT_DAY || today;
  const list = CHAT_MSGS || [];
  // "New topic": earlier messages stay readable, but the model is no longer sent them.
  const topicFrom = CTX && CTX.context_from_ms > 0 ? CTX.context_from_ms : 0;
  const firstAfter = topicFrom ? list.findIndex((m) => m.at_ms >= topicFrom) : -1;
  const divider = `<div class="topic-divider"><span>New topic · earlier messages are not sent to the model</span></div>`;
  const msgs = list.map((m, index) => {
    const mine = m.role === "user";
    const marker = topicFrom && index === firstAfter && index > 0 ? divider : "";
    const said = shownText(m.text);
    const lock = said.private ? `<span class="private-lock" title="Private: kept on your network">${icon("lock", 12)}</span>` : "";
    const bubble = `<div class="row msg-row${mine ? " mine" : ""}">
      <div class="bubble ${mine ? "me" : "butler"}">${lock}${mine ? esc(said.text) : rich(said.text)}</div></div>`;
    // A butler reply carries its persisted action trail + sources (if any), so
    // you can expand a PAST answer to see what it did and where it came from.
    if (!mine && m.actions) {
      const newest = m.id === (list[list.length - 1] || {}).id;
      // What it did sits ABOVE what it said, as a quiet line that opens in place.
      return marker + stepsHtml(m.actions.steps) + bubble + activityHtml(m.actions.activity)
        + actionsTakenHtml(m.actions.actions_taken, newest)
        + sourcesHtml(m.actions.sources);
    }
    return marker + bubble;
  }).join("") + (topicFrom && list.length && firstAfter === -1 ? divider : "");
  // Derived from persisted state, so it survives a reload: if the newest message is
  // yours, the butler still owes a reply — show the thinking indicator.
  //
  // Unless the person STOPPED it. Then no reply is coming, and the dots would sit
  // there forever waiting for something that was cancelled. Say what happened.
  // Only today can be awaiting a reply; an older day is finished by definition.
  const awaiting = showing === today && list.length > 0 && list[list.length - 1].role === "user";
  // Three different things, and they used to render as one animated ellipsis: stopped on
  // purpose, still being worked on, and never answered at all. Only the first two are worth
  // waiting for, and the third is the one that stranded somebody — a message sent, a reply
  // that flashed and vanished, and dots that survived every reload because the tab that had
  // been streaming was gone. `NODE_BUSY` is the node's own answer, so it is the same on
  // every device and outlives the tab.
  const stillWorking = CHAT_STREAMING || NODE_BUSY;
  const pending = awaiting
    ? (CHAT_STOPPED
        ? `<div class="row msg-row">
             <div class="bubble butler"><span class="sub">You stopped this one. Send again if you'd like an answer.</span></div></div>`
        : stillWorking
          ? `<div class="row msg-row" id="chat-pending">
             <div class="bubble butler thinking"><span class="dots"><i></i><i></i><i></i></span></div></div>`
          : `<div class="row msg-row">
             <div class="bubble butler"><span class="sub">That one didn't get an answer — nothing was saved.</span>
             <div class="row" style="margin-top:6px;"><button class="ghost" data-act="chat:retry">Ask it again</button></div></div></div>`)
    : "";
  // While a turn is streaming, DB.messages doesn't include it yet. Rebuild the
  // in-flight exchange from live state — the just-sent message(s) and the reply
  // growing in (id="chat-live" so the token loop keeps writing into it) — so a
  // re-render mid-stream (e.g. toggling Speak) can't wipe the current turn.
  let liveTurn = "";
  if (CHAT_STREAMING) {
    const users = [CHAT_INFLIGHT, ...CHAT_QUEUE].filter(Boolean).map((t) =>
      `<div class="row msg-row mine"><div class="bubble me">${typeof t === "object" && t.private ? `<span class="private-lock">${icon("lock", 12)}</span>` : ""}${esc(typeof t === "string" ? t : t.message)}</div></div>`).join("");
    const replyInner = LIVE_REPLY ? rich(LIVE_REPLY) : `<span class="dots"><i></i><i></i><i></i></span>`;
    liveTurn = users +
      `<div class="row msg-row" id="chat-live"><div class="bubble butler">${replyInner}</div></div>`;
  }
  // The in-flight turn only. Every finished turn renders its own note from the stored
  // record above, which is what makes it survive coming back to the chat; this is for the
  // moment between the stream ending and the reply appearing in the persisted history.
  const streamingActivity = SHOW_ACTIVITY && CHAT_STREAMING && shownActivity(LAST_ACTIVITY).length
    ? `<div class="activity">${icon("sparkle", 13)} ${shownActivity(LAST_ACTIVITY).map(esc).join(" · ")}</div>`
    : "";
  const speakBtn = TTS
    ? `<button class="ghost" data-act="toggle:speak" title="read replies aloud">${icon(SPEAK ? "speakerOn" : "speakerOff")}<span>${SPEAK ? "Speaking" : "Speak"}</span></button>`
    : "";
  // Dictation puts your words in the box and leaves them there — you read them and
  // hit send. Browser-native recognition, so there is no upload and no wait for a
  // transcription server; press-and-hold-to-send and the hands-free loop were slower
  // and less predictable than the thing they replaced.
  const micBtn = STT
    ? (window.isSecureContext
        ? `<button class="ghost" data-act="chat:mic" title="dictate into the box — you still press send">${icon("mic")}<span>Dictate</span></button>`
        : `<button data-act="chat:mic" title="voice input needs HTTPS or localhost">${icon("mic")}<span>needs HTTPS</span></button>`)
    : "";
  // A way back through the days that have conversation. Only shown once there IS a
  // yesterday — on a fresh install it would be a control that does nothing.
  const older = days.filter((d) => d < showing).pop();
  const newer = days.filter((d) => d > showing).shift();
  const dayBar = days.length > 1 || showing !== today
    ? `<div class="row" style="gap:8px;align-items:center;justify-content:center;padding:4px 0;">
         ${older ? `<button class="ghost" data-act="chat:day:${older}" title="${esc(dayInWords(older))}"><span style="display:inline-block;transform:rotate(180deg);">${icon("chevron", 14)}</span></button>` : ""}
         <span class="sub">${esc(dayInWords(showing))}${showing === today ? "" : ` · ${list.length} message${list.length === 1 ? "" : "s"}`}</span>
         ${newer ? `<button class="ghost" data-act="chat:day:${newer}">${icon("chevron", 14)}</button>` : ""}
         ${showing === today ? "" : `<button class="ghost" data-act="chat:day:${today}">Today</button>`}
       </div>`
    : "";
  const research = viewResearch();
  const emptyToday = `<div class="empty">${showing === today && days.length > 1
      ? "A new day. Yesterday's conversation is a tap back."
      : "Say anything — Endora is listening."}</div>`;
  return `
    <div class="chat">
      ${AGENDA && AGENDA.now ? `<div class="chat-top"><div class="chat-col">${viewNowLine()}${viewPinned()}</div></div>` : ""}
      <div class="chat-scroll" id="chat-scroll"><div class="chat-col">
        ${dayBar}
        ${research}
        <div id="chat-thread" class="chat-thread">${(msgs || (CHAT_STREAMING ? "" : emptyToday)) + (CHAT_STREAMING ? liveTurn : pending) + streamingActivity}</div>
        ${CHAT_STREAMING ? "" : viewNeedsYou()}
      </div></div>
      <div class="composer-dock"><div class="chat-col">
        <div class="composer">
          <div class="composer-chips" id="composer-chips">${composerChipsHtml()}</div>
          <textarea id="chat-input" rows="1" placeholder="${researching() ? "What should Endora research?" : "Talk to your butler…"}"></textarea>
          ${researching() ? `<textarea id="research-urls" rows="2" placeholder="Optional starting URLs, one per line"></textarea>` : ""}
          <div class="composer-bar">
            <div class="tools">
              <button class="icon-btn" data-act="toggle:tools" title="More" aria-label="More options" aria-haspopup="menu">${icon("plus", 18)}</button>
              <div class="tools-menu" id="tools-menu" role="menu" hidden>${toolsMenuHtml()}</div>
            </div>
            <div class="composer-right">
              <button class="ctx-ring" id="ctx-ring" data-act="ctx:toggle" title="${esc(ctxRingTitle())}" aria-label="What is sent to the model">${ctxRingSvg()}</button>
              <button class="send-round" id="send-btn" data-act="${CHAT_STREAMING ? "chat:stop" : researching() ? "research:start" : "chat:send"}" title="${CHAT_STREAMING ? "Stop" : researching() ? "Start research" : "Send"}" aria-label="${CHAT_STREAMING ? "Stop" : researching() ? "Start research" : "Send"}">${icon(CHAT_STREAMING ? "stop" : "up", 18)}</button>
            </div>
          </div>
          <div class="ctx-panel" id="ctx-panel" ${CTX_OPEN ? "" : "hidden"}>${ctxPanelHtml()}</div>
        </div>
      </div></div>
    </div>`;
}

// Whether send commissions research. Research goes out to web search and the models, so a
// private message never does (ADR 0105): with Private on, send sends a private message.
function researching() {
  return RESEARCH_MODE && !PRIVATE_MODE;
}

// What is switched on, shown inside the composer where it applies, each one a tap away
// from off. Modes live here rather than as four labelled buttons beside the box.
function composerChipsHtml() {
  const chip = (act, ic, label, title) =>
    `<button class="chip" data-act="${act}" title="${esc(title)}">${icon(ic, 13)}<span>${label}</span>${icon("close", 11)}</button>`;
  return [
    PRIVATE_MODE ? chip("toggle:private", "lock", "Private", "Answered on your network only, never sent out, never remembered — tap to turn off") : "",
    DEEP_MODEL.configured && DEEP_MODE && !PRIVATE_MODE ? chip("toggle:deep", "sparkle", "Deep", "Your messages go to the bigger model — tap to turn off") : "",
    researching() ? chip("toggle:research", "note", "Research", "Commissioning durable research — tap to turn off") : "",
    SPEAK ? chip("toggle:speak", "speakerOn", "Speaking", "Replies are read aloud — tap to turn off") : "",
  ].join("");
}

// The "+" menu: everything that is not typing or sending.
function toolsMenuHtml() {
  const item = (act, ic, label, on, title) =>
    `<button role="menuitem" data-act="${act}" title="${esc(title)}" class="${on ? "on" : ""}">${icon(ic, 16)}<span>${label}</span>${on ? icon("check", 14) : ""}</button>`;
  const dictate = STT
    ? (window.isSecureContext
        ? item("chat:mic", "mic", "Dictate", false, "dictate into the box — you still press send")
        : `<button role="menuitem" data-act="chat:mic" title="voice input needs HTTPS or localhost">${icon("mic", 16)}<span>Dictate (needs HTTPS)</span></button>`)
    : "";
  return [
    TTS ? item("toggle:speak", SPEAK ? "speakerOn" : "speakerOff", "Read replies aloud", SPEAK, "read replies aloud") : "",
    dictate,
    item("toggle:private", "lock", "Private", PRIVATE_MODE, "answered on your own network only: never sent out, even disguised, and never remembered"),
    item("toggle:research", "note", "Research", RESEARCH_MODE, "commission durable, bounded research"),
    item("ctx:reset", "plus", "New topic", false, "stop sending earlier messages to the model; they stay in the chat"),
    DEEP_MODEL.configured ? item("toggle:deep", "sparkle", "Ask the bigger model", DEEP_MODE, "when on, your messages go to the bigger model") : "",
  ].join("");
}

// Refresh the chips and menu in place. A full render() rebuilds the thread from persisted
// state and would wipe a reply that is still streaming in.
// ---- what is sent to the model (ADR 0088) ---------------------------------
const ctxFraction = () => (CTX && CTX.window_tokens && CTX.total_tokens ? Math.min(1, CTX.total_tokens / CTX.window_tokens) : 0);
const kTokens = (n) => (n >= 1000 ? (n / 1000).toFixed(n >= 10000 ? 0 : 1) + "k" : String(n));
function ctxRingTitle() {
  if (!CTX || !CTX.window_tokens) return "What is sent to the model";
  return `~${kTokens(CTX.total_tokens)} of ${kTokens(CTX.window_tokens)} tokens in the window (estimate). Tap for what is in it.`;
}
// A ring that fills as the window does: calm until it is most of the way, then amber, then red.
function ctxRingSvg() {
  const f = ctxFraction();
  const r = 9, c = 2 * Math.PI * r;
  const tone = f >= 0.9 ? "bad" : f >= 0.7 ? "warn" : "ok";
  return `<svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="${r}" fill="none" stroke="currentColor" stroke-opacity=".18" stroke-width="3"/>` +
    `<circle class="ring-${tone}" cx="12" cy="12" r="${r}" fill="none" stroke-width="3" stroke-linecap="round" stroke-dasharray="${(c * f).toFixed(1)} ${c.toFixed(1)}" transform="rotate(-90 12 12)"/></svg>`;
}
function ctxPanelHtml() {
  if (!CTX || !Array.isArray(CTX.sections)) return `<div class="sub">Counting what is sent…</div>`;
  const total = CTX.total_tokens || 1;
  const pct = Math.round(ctxFraction() * 100);
  const bar = CTX.sections.filter((s) => s.tokens > 0).map((s, i) =>
    `<i class="seg c${i % 8}" style="width:${Math.max(0.5, (s.tokens / Math.max(total, CTX.window_tokens)) * 100).toFixed(2)}%" title="${esc(s.label)}"></i>`).join("");
  const rows = CTX.sections.map((s, i) => {
    const items = s.items.map((it) => `<li><span>${esc(it.text)}</span><em>${kTokens(it.tokens)}</em></li>`).join("");
    const more = s.count > s.items.length ? `<li class="sub">…and ${s.count - s.items.length} more</li>` : "";
    return `<details class="ctx-row"><summary><i class="dot c${i % 8}"></i><span class="lbl">${esc(s.label)}${s.to_deep ? ' <b class="deep-tag" title="This is also what Ask deep is shown">Ask deep sees this</b>' : ""}</span><span class="n">${s.key === "held_back" ? "not sent" : "~" + kTokens(s.tokens)}</span></summary>` +
      `<div class="ctx-body">${s.note ? `<p class="sub">${esc(s.note)}</p>` : ""}<ul>${items}${more}</ul></div></details>`;
  }).join("");
  return `<div class="ctx-head"><strong>What is sent with each message</strong><span class="sub">~${kTokens(CTX.total_tokens)} of ${kTokens(CTX.window_tokens)} tokens · ${pct}% · estimate</span></div>` +
    `<div class="ctx-bar">${bar}</div><div class="ctx-rows">${rows}</div>` +
    `<p class="sub ctx-foot">Ask deep and the automatic re-check send the other model only your question and the skill definitions, never what a skill returned.</p>` +
    (DEEP_MODEL.configured ? `<div class="ctx-spend" id="ctx-spend">${deepSpendHtml()}</div>` : "") +
    `<div class="ctx-actions"><button class="ghost" data-act="ctx:reset" title="Stop sending earlier messages. They stay in the chat; what Endora has learned about you is kept.">${icon("plus", 14)}<span>New topic</span></button></div>`;
}
// ---- what the deep model costs (ADR 0091) --------------------------------
async function loadDeepSpend() {
  try { DEEP_USAGE = await api("GET", "/v1/deep-model/usage"); } catch (_) { DEEP_USAGE = null; }
  try { DEEP_BALANCE = await api("GET", "/v1/deep-model/balance"); } catch (_) { DEEP_BALANCE = null; }
}
const money = (n) => (n == null ? "" : "$" + (n > 0 && n < 0.01 ? n.toFixed(4) : n.toFixed(2)));
// The provider's own balance when it has one, then what Endora's calls to the deep model have
// used (and, once prices are set, what that comes to). Empty when there is no deep model.
function deepSpendHtml() {
  if (!DEEP_MODEL.configured) return "";
  const rows = [];
  const b = DEEP_BALANCE;
  if (b && b.supported) {
    if (b.error) {
      rows.push(`<div class="spend-row warn">${esc(b.error)}</div>`);
    } else {
      const list = Array.isArray(b.balances) ? b.balances : [];
      const bal = list.find((x) => x.currency === "USD") || list[0];
      if (bal) {
        const total = parseFloat(bal.total);
        const low = b.available === false || (bal.currency === "USD" && total < 1);
        const sym = bal.currency === "USD" ? "$" : bal.currency + " ";
        rows.push(`<div class="spend-row${low ? " warn" : ""}"><span>Balance</span><b>${sym}${esc(String(bal.total))}</b>` +
          `<em>${low ? "running low · " : ""}granted ${sym}${esc(String(bal.granted))}, topped up ${sym}${esc(String(bal.topped_up))}</em></div>`);
      }
    }
  }
  const u = DEEP_USAGE;
  if (u && u.month) {
    const tokens = kTokens(u.month.prompt_tokens + u.month.completion_tokens);
    if (u.month.cost_usd != null) {
      rows.push(`<div class="spend-row"><span>Spent this month</span><b>~${money(u.month.cost_usd)}</b><em>${tokens} tokens</em></div>`);
    } else {
      rows.push(`<div class="spend-row"><span>This month</span><b>${tokens} tokens</b><em>set prices under Settings → Models → Deep to see dollars</em></div>`);
    }
  }
  return rows.join("");
}

function refreshContextRing() {
  const ring = document.getElementById("ctx-ring");
  if (ring) { ring.innerHTML = ctxRingSvg(); ring.title = ctxRingTitle(); }
  const panel = document.getElementById("ctx-panel");
  if (panel) panel.innerHTML = ctxPanelHtml();
}

function refreshComposerModes() {
  const chips = document.getElementById("composer-chips");
  if (chips) chips.innerHTML = composerChipsHtml();
  const menu = document.getElementById("tools-menu");
  if (menu) menu.innerHTML = toolsMenuHtml();
}

function viewResearch() {
  const items = (RESEARCH && RESEARCH.items) || [];
  if (!items.length && !RESEARCH_MODE) return "";
  const cards = items.map((item) => {
    const terminal = ["delivered", "blocked", "budget_exhausted", "cancelled", "failed"].includes(item.status);
    const detail = item.status === "delivered"
      ? (item.partial ? "Partial report saved" : "Saved report ready")
      : (item.reason || `${item.phase} · ${item.budget.model_calls_used}/${item.budget.model_calls_limit} model calls`);
    return `<div class="card" style="margin:8px 0;">
      <div class="row" style="justify-content:space-between;gap:8px;"><b>${esc(item.assignment)}</b><span class="sub">${esc(item.status.replaceAll("_", " "))}</span></div>
      <div class="sub" style="margin-top:6px;">${esc(detail)}</div>
      ${item.report_markdown ? `<details style="margin-top:8px;"><summary>View report</summary><div class="bubble butler" style="margin-top:8px;">${rich(item.report_markdown)}</div></details>` : ""}
      <div class="row" style="gap:8px;margin-top:8px;">
        ${item.report_available ? `<button class="ghost" data-act="research:download:${item.id}">Download report</button>` : ""}
        ${!terminal ? `<button class="ghost" data-act="research:cancel:${item.id}">Cancel</button>` : ""}
      </div></div>`;
  }).join("");
  const setup = RESEARCH_MODE
    ? `<div class="card"><div class="sub">${RESEARCH_READY.ready
        ? "Endora will keep working after you close this page and save a bounded Markdown report citing pages it read. It can search only through an approved Brave Search connection; otherwise add starting URLs."
        : "Research needs the built-in web reader. Connect Brave Search for discovery, or add starting URLs after the reader is available."}</div></div>` : "";
  return `<div aria-label="Research assignments">${setup}${cards}</div>`;
}

// Provider presets for the butler models. Each fills the endpoint + example
// model names + author-recommended sampling (router cold for reliable skill
// selection, synthesizer warmer for natural prose). top_k / repeat_penalty are
// Ollama-only extensions, so cloud presets leave them blank (strict OpenAI
// endpoints reject them). Anthropic runs via OpenRouter — Endora speaks OpenAI.
const MODEL_PRESETS = {
  ollama: { label: "Ollama (local)", base_url: "http://host.docker.internal:11434/v1", key: false,
    single: { model: "qwen2.5:7b",  temperature: 0.5, top_p: 0.8, top_k: 20, repeat_penalty: 1.05 },
    router: { model: "hermes3:8b",  temperature: 0.1, top_p: 0.9, top_k: 20, repeat_penalty: 1.05 },
    synth:  { model: "qwen2.5:7b",  temperature: 0.6, top_p: 0.8, top_k: 20, repeat_penalty: 1.05 } },
  openai: { label: "OpenAI", base_url: "https://api.openai.com/v1", key: true,
    single: { model: "gpt-4o-mini", temperature: 0.5 },
    router: { model: "gpt-4o-mini", temperature: 0.1 },
    synth:  { model: "gpt-4o",      temperature: 0.6 } },
  openrouter: { label: "OpenRouter", base_url: "https://openrouter.ai/api/v1", key: true,
    single: { model: "anthropic/claude-sonnet-5",   temperature: 0.5 },
    router: { model: "anthropic/claude-3.5-haiku",  temperature: 0.1 },
    synth:  { model: "anthropic/claude-sonnet-5",   temperature: 0.6 } },
  // Claude directly, with the owner's own Anthropic API key (its OpenAI-compatible endpoint):
  // what the models plan calls for, paid from the owner's API credits.
  claude: { label: "Anthropic (direct, your API key)", base_url: "https://api.anthropic.com/v1", key: true,
    single: { model: "claude-sonnet-5-5", temperature: 0.5 },
    router: { model: "claude-haiku-5-5",  temperature: 0.1 },
    synth:  { model: "claude-sonnet-5-5", temperature: 0.6 } },
  anthropic: { label: "Anthropic (via OpenRouter)", base_url: "https://openrouter.ai/api/v1", key: true,
    single: { model: "anthropic/claude-sonnet-5",   temperature: 0.5 },
    router: { model: "anthropic/claude-3.5-haiku",  temperature: 0.1 },
    synth:  { model: "anthropic/claude-sonnet-5",   temperature: 0.6 } },
  groq: { label: "Groq", base_url: "https://api.groq.com/openai/v1", key: true,
    single: { model: "llama-3.3-70b-versatile", temperature: 0.5 },
    router: { model: "llama-3.3-70b-versatile", temperature: 0.1 },
    synth:  { model: "llama-3.3-70b-versatile", temperature: 0.6 } },
  // DeepSeek is OpenAI-compatible. deepseek-chat (V3) is the general model;
  // deepseek-reasoner (R1) is the heavier reasoning one — a good "deep" pick you
  // can type into the Model field.
  deepseek: { label: "DeepSeek", base_url: "https://api.deepseek.com/v1", key: true,
    single: { model: "deepseek-chat",     temperature: 0.5 },
    router: { model: "deepseek-chat",     temperature: 0.1 },
    synth:  { model: "deepseek-chat",     temperature: 0.6 } },
};

