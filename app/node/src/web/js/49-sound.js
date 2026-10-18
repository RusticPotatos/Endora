// ---- Speakers: endora-sound's remote (ADR 0104) ---------------------------------------------
// Sources on one side, speakers on the other, and wires between them, each with its own
// level. Tap a speaker under a source to wire it; drag the wire's slider to set its share of
// that speaker's mix. Speakers with the same wires play as one group, in sync. This device
// can be a speaker too: it listens to the hub's stream.
//
// The console talks to the node only. Listening uses a ticket the node hands out, good for a
// minute for this speaker, because an <audio> element cannot sign its request. The same ticket
// plays the live stream (49b-sound-live.js): Listening is the MP3 in an <audio> element, which
// plays on with the screen locked; Live is raw sound in a worklet, a fraction of a second behind.

const SOUND = {
  state: null,        // what the hub says: sources, speakers, wires, groups, setups, listening
  error: "",
  found: [],          // AirPlay speakers the senders can see, by name
  ticket: null,       // { url, at } a fresh ticket for this device's stream, ready for a tap
  listening: false,   // whether this device is playing its stream
  mode: null,         // how it plays now: "live" or "listening" (null when it does not)
  liveBlocked: false, // live would not start without a tap: listening until the next one
  stat: null,         // the live buffer's last report: { depthMs, target, under, ... }
  measure: null,      // the last delay measurement: { live, listening }
  measuring: false,
  gen: 0,             // counts the streams started, so a slow start cannot outlive a stop
};

// The <audio> element lives outside the screen, so a re-render never stops the sound.
function soundAudio() {
  let a = document.getElementById("sound-player");
  if (a) return a;
  a = document.createElement("audio");
  a.id = "sound-player";
  a.preload = "none";
  a.hidden = true;
  // A stream that drops comes back on its own, with a new ticket, while this device listens.
  a.addEventListener("error", () => { if (SOUND.listening) setTimeout(soundReconnect, 1500); });
  a.addEventListener("ended", () => { if (SOUND.listening) setTimeout(soundReconnect, 1500); });
  a.addEventListener("stalled", () => {
    if (!SOUND.listening) return;
    const at = a.currentTime;
    setTimeout(() => { if (SOUND.listening && a.currentTime === at) soundReconnect(); }, 8000);
  });
  document.body.appendChild(a);
  return a;
}

// Which stream speaker this device is: kept in this browser only.
function soundMine() {
  let id = null;
  try { id = Number(localStorage.getItem("endora.soundSpeaker") || "") || null; } catch (_) {}
  const s = SOUND.state && (SOUND.state.speakers || []).find((x) => x.id === id && x.kind === "stream");
  return s || null;
}

// What this device would call itself the first time.
function soundGuessName() {
  const ua = (navigator && navigator.userAgent) || "";
  return /iPad/.test(ua) ? "iPad" : /iPhone/.test(ua) ? "iPhone" : /Macintosh/.test(ua) ? "Mac" : /Android/.test(ua) ? "Phone" : "This browser";
}

async function soundTicket() {
  const mine = soundMine();
  if (!mine) { SOUND.ticket = null; return; }
  try {
    const t = await api("POST", `/v1/sound/listen/${mine.id}/ticket`, {});
    SOUND.ticket = { url: t.url, live: t.live, at: Date.now() };
  } catch (_) { SOUND.ticket = null; }
}

// Starts this device's listening, if `speaker` is this device and it is not listening yet.
// Called straight from a tap (Safari starts sound only so), with a ticket fetched before it.
// Whether it started.
function soundStartHere(speaker) {
  const mine = soundMine();
  if (!mine || mine.id !== speaker || SOUND.listening || !SOUND.ticket) return false;
  const ticket = SOUND.ticket; SOUND.ticket = null;
  SOUND.listening = true; SOUND.liveBlocked = false;
  soundRemember(true); soundBanner("");
  soundPlay(ticket).catch(() => { SOUND.listening = false; });
  return true;
}

// How this device is asked to hear: the owner's choice, kept in this browser only.
function soundPref() {
  try { const v = localStorage.getItem("endora.soundMode"); if (v === "live" || v === "listening") return v; } catch (_) {}
  return "auto";
}

// The way to hear right now: the owner's choice, or what the wires say; listening where live
// cannot play (no worklet, no tap since the screen was off) and in the background.
function soundWantedMode() {
  const mine = soundMine(), st = SOUND.state || {};
  let mode = mine ? liveModeFor(soundPref(), st.sources, st.wires, mine.id) : "listening";
  if (SOUND.liveBlocked || !liveSupported()) mode = "listening";
  return liveEffectiveMode(mode, typeof document !== "undefined" && document.hidden === true);
}

// Starts hearing `ticket`'s stream, in the way wanted. Called straight from a tap: whatever has
// to be started by a tap is started here, before anything is waited for. The audio context is
// made (so later switches need no tap), and in live mode the <audio> element plays a moment of
// silence, so it may start the MP3 by itself later, when the screen locks.
function soundPlay(ticket) {
  const a = soundAudio();
  const mode = soundWantedMode();
  if (liveSupported()) {
    const ctx = liveContext();
    if (ctx && ctx.state !== "running") ctx.resume().catch(() => {});
  }
  SOUND.mode = mode;
  if (mode === "live") {
    a.src = liveSilence();
    a.play().catch(() => {});
    soundLiveStart(ticket.live);
    return Promise.resolve();
  }
  a.src = ticket.url;
  return a.play();
}

// Opens the live stream and plays it. When live sound is coming, the MP3 (if it was playing)
// is let go. If it cannot start, the MP3 plays instead and live waits for the next tap.
async function soundLiveStart(url) {
  const gen = ++SOUND.gen;
  liveClose(LIVE.session); LIVE.session = null;
  try {
    const s = await liveOpen(url, false, {
      stat: soundLiveStat,
      playing: (on) => { if (on && gen === SOUND.gen) soundAudioQuiet(); },
      ended: () => { if (gen === SOUND.gen && SOUND.listening && SOUND.mode === "live") setTimeout(soundReconnect, 1500); },
    }, (soundMine() || {}).sync_ms);
    if (gen !== SOUND.gen) { liveClose(s); return; }
    LIVE.session = s;
  } catch (_) {
    if (gen !== SOUND.gen || !SOUND.listening) return;
    SOUND.liveBlocked = true; SOUND.mode = "listening";
    soundBanner(`<div class="grow">Live sound needs a tap to start again.</div><button class="primary" data-act="sound:golive">Go live</button>`);
    await soundReconnect();
  }
}

// The MP3 stops once the live stream has taken over.
function soundAudioQuiet() {
  if (SOUND.mode !== "live") return;
  const a = soundAudio();
  a.pause(); a.removeAttribute("src"); a.load();
}

// The live buffer's report: the delay shown on the card follows it.
function soundLiveStat(m) {
  SOUND.stat = m;
  const el = document.getElementById("sound-live-label");
  if (el) el.textContent = soundModeSaid();
}

// What the card says about how this device hears.
function soundModeSaid() {
  const pref = soundPref();
  if (!SOUND.listening) {
    return pref === "live" ? "Live: a fraction of a second behind the hub. Stops if the screen locks."
      : pref === "listening" ? "Listening: a few seconds behind, plays on with the screen locked."
      : "Auto: live when something is cast in from an AirPlay door, listening for music and radio.";
  }
  if (SOUND.mode === "live") {
    const m = SOUND.stat;
    const out = (LIVE.ctx && LIVE.ctx.outputLatency) || 0;
    if (!m || !m.playing) return "live · filling its buffer…";
    return `live · ${liveDelayLabel(m.depthMs + out * 1000)}${m.under ? " · the network was rough, so it keeps more in hand" : ""}`;
  }
  return SOUND.liveBlocked ? "listening · a few seconds behind; live needs a tap"
    : "listening · a few seconds behind; plays on with the screen locked";
}

// Lip-sync on the speaker card, in live mode: "Lip-sync  −  +0.0 s  +", a tenth of a second a
// tap. Positive plays this speaker later. Earlier than the buffer allows says what to do.
function soundSyncControl(mine) {
  const ms = Number(mine.sync_ms) || 0;
  const short = liveSyncPlan(ms, LIVE_MIN).short;
  return `<div class="row" style="gap:8px;align-items:center;margin-top:6px;">
      <span class="sub">Lip-sync</span>
      <button class="ghost" data-act="sound:sync:-100" aria-label="Play 0.1 s earlier"${ms <= -500 ? " disabled" : ""}>−</button>
      <b id="sound-sync-label">${esc(soundSyncLabel(ms))}</b>
      <button class="ghost" data-act="sound:sync:100" aria-label="Play 0.1 s later"${ms >= 1500 ? " disabled" : ""}>+</button></div>
    ${short ? `<div class="sub" id="sound-sync-note">Can't go earlier on this speaker; lower the door's early hand-over.</div>` : ""}`;
}

// Moves this speaker's lip-sync by `by` ms: the playing stream follows at once, and the hub
// keeps it. A hub that refuses puts it back.
async function soundSyncBy(by) {
  const mine = soundMine();
  if (!mine) return;
  const was = Number(mine.sync_ms) || 0;
  const ms = Math.max(-500, Math.min(1500, was + by));
  if (ms === was) return;
  mine.sync_ms = ms;
  if (LIVE.session) liveSetSync(LIVE.session, ms);
  render();
  try { await api("PUT", `/v1/sound/speakers/${mine.id}/sync`, { ms }); }
  catch (e) {
    mine.sync_ms = was;
    if (LIVE.session) liveSetSync(LIVE.session, was);
    flash("Couldn't save the lip-sync: " + e.message, "err");
    render();
  }
}

// The way to hear changed (the owner picked one, the wires changed, the page went to the
// background or came back): switches the stream that plays, if it must.
async function soundApplyMode() {
  if (!SOUND.listening) return;
  const want = soundWantedMode();
  if (want === SOUND.mode) return;
  if (SOUND.switching) { SOUND.again = true; return; }
  SOUND.switching = true;
  try {
    // A ticket kept ready, so a page going to the background need not ask for one first.
    let t = SOUND.ticket && Date.now() - SOUND.ticket.at < 50000 ? SOUND.ticket : null;
    if (!t) { await soundTicket(); t = SOUND.ticket; }
    if (!t) return;
    if (want === "listening") {
      SOUND.gen++;
      SOUND.mode = "listening";
      liveClose(LIVE.session); LIVE.session = null;
      const a = soundAudio();
      a.src = t.url;
      a.play().catch(() => {});
    } else {
      SOUND.mode = "live";
      await soundLiveStart(t.live);
    }
    render();
  } finally {
    SOUND.switching = false;
    if (SOUND.again) { SOUND.again = false; soundApplyMode(); }
  }
}

if (typeof document !== "undefined" && typeof document.addEventListener === "function") {
  document.addEventListener("visibilitychange", () => { soundApplyMode(); });
}

async function loadSound() {
  SOUND.error = "";
  try { SOUND.state = await api("GET", "/v1/sound"); }
  catch (e) {
    SOUND.state = null;
    SOUND.error = /not set up/i.test(e.message) ? "The sound hub is not set up on this node yet." : "The sound hub did not answer: " + e.message;
    return;
  }
  // What the senders can see, to add by name; nothing when there are no senders yet.
  try { SOUND.found = ((await api("GET", "/v1/sound/found")) || {}).speakers || []; } catch (_) { SOUND.found = []; }
  // A ticket ready before the tap: Safari only starts sound straight from the tap itself.
  if (!SOUND.ticket || Date.now() - SOUND.ticket.at > 45000) await soundTicket();
}

async function soundReconnect() {
  if (!SOUND.listening) return;
  await soundTicket();
  if (!SOUND.ticket) return;
  const t = SOUND.ticket; SOUND.ticket = null;
  if (SOUND.mode === "live") { await soundLiveStart(t.live); return; }
  const a = soundAudio();
  a.src = t.url;
  try { await a.play(); } catch (_) {}
}

function soundName(kind, id) {
  const list = (SOUND.state && SOUND.state[kind === "source" ? "sources" : "speakers"]) || [];
  const x = list.find((y) => y.id === id);
  return x ? x.name : "?";
}

function soundWire(source, speaker) {
  return ((SOUND.state && SOUND.state.wires) || []).find((w) => w.source === source && w.speaker === speaker) || null;
}

// What a source's card says it is. The general AirPlay door takes anything cast into it and
// can be wired to any speaker; an old device door is for that device to cast into.
function soundSourceSaid(src) {
  if (src.kind !== "airplay") return SOUND_KIND[src.kind] || src.kind;
  if (src.device) return `AirPlay door for ${src.device} to cast into; can't play back on ${src.device}`;
  return `Cast anything here (Apple TV, phone…) and wire it to where you listen: pick “${src.name}” in the AirPlay menu. A device wired here hears what it casts in.`;
}

const SOUND_KIND = { airplay: "AirPlay", spotify: "Spotify", radio: "radio", voice: "Endora's voice", stream: "stream" };

function viewSound() {
  const head = `${labCrumbs({ label: "Music", act: "go:music" }, { label: "Speakers" })}<h2>Speakers</h2>`;
  if (SOUND.error) return `${head}<div class="card"><div class="sub">${esc(SOUND.error)}</div></div>`;
  const s = SOUND.state;
  if (!s) return `${head}<div class="note">Asking the hub…</div>`;
  const mine = soundMine();
  const listening = new Set(s.listening || []);
  // Drawn by the console, not the browser: Safari's own slider stopped its knob short of the
  // end at 100. The fill and the knob follow the value exactly, and follow a drag.
  const range = (act, value, label) => {
    const v = Number(value) || 0;
    return `<input type="range" class="level" min="0" max="100" step="5" value="${v}" data-change="${act}" aria-label="${esc(label)}" style="--v:${v / 100};" />`;
  };

  // This device, as a speaker.
  // Listening with nothing wired is silence; it says so, and where to wire something.
  const wiredHere = mine && (s.wires || []).some((w) => w.speaker === mine.id);
  const hearing = !SOUND.listening ? "Not listening · a stream, behind the speakers"
    : wiredHere ? "Listening to its wires · a stream, behind the speakers"
    : `Listening, but nothing is wired to this ${mine ? mine.name : "device"} yet: tap ${mine ? mine.name : "it"} under a source below`;
  const pref = soundPref();
  const modeButton = (m, label) => `<button class="pill pick${pref === m ? " active" : ""}" data-act="sound:mode:${m}" aria-pressed="${pref === m ? "true" : "false"}">${label}</button>`;
  const measured = SOUND.measuring ? "Measuring: you will hear a short beep, and a moment of quiet." : soundMeasured(SOUND.measure);
  const here = mine
    ? `<div class="card"><div class="row" style="align-items:center;gap:10px;">
        <div class="grow"><div class="title">This ${esc(mine.name)}</div>
          <div class="sub">${esc(hearing)}</div></div>
        ${SOUND.listening
          ? `<button class="ghost" data-act="sound:stop">Stop</button>`
          : `<button class="primary" data-act="sound:listen">Listen here</button>`}
      </div>
      <div class="model-picker" style="margin-top:8px;">${modeButton("auto", "Auto")}${modeButton("live", "Live")}${modeButton("listening", "Listening")}</div>
      <div class="sub" id="sound-live-label" style="margin-top:4px;">${esc(soundModeSaid())}</div>
      <div class="row" style="gap:8px;align-items:center;margin-top:6px;">
        <button class="ghost" data-act="sound:measure"${SOUND.measuring || !SOUND.listening ? " disabled" : ""}>Measure delay</button>
        <span class="sub grow">${esc(measured || (SOUND.listening ? "" : "Listen first, then measure."))}</span></div>
      ${SOUND.listening && SOUND.mode === "live" ? soundSyncControl(mine) : ""}</div>`
    : `<div class="card"><div class="title">Listen on this device</div>
        <div class="sub">It becomes a speaker you can wire sound to.</div>
        <div class="form" style="margin-top:8px;"><input id="sound-mine" value="${esc(soundGuessName())}" maxlength="40" autocomplete="off" />
          <button class="primary" data-act="sound:mine">Add it</button></div></div>`;

  // Each source, with its speakers to tap.
  const speakers = s.speakers || [];
  const sources = (s.sources || []).map((src) => {
    const chips = speakers.map((sp) => {
      const own = src.device && sp.device && src.device === sp.device;
      if (own) return "";
      const w = soundWire(src.id, sp.id);
      // A borrowed speaker keeps its wires but plays none of them: said where it is tapped.
      return `<button class="pill pick${w ? " active" : ""}" data-act="sound:wire:${src.id}-${sp.id}" aria-pressed="${w ? "true" : "false"}">${esc(sp.name)}${w ? ` · ${w.level}` : ""}${sp.borrowed ? " · in use" : ""}</button>`;
    }).join("");
    const wires = speakers.map((sp) => {
      const w = soundWire(src.id, sp.id);
      if (!w) return "";
      return `<div class="row" style="gap:8px;align-items:center;margin-top:6px;">
        <span class="sub" style="min-width:84px;">${esc(sp.name)}</span>
        ${range(`sound:level:${src.id}-${sp.id}`, w.level, `${src.name} on ${sp.name}`)}
        <span class="sub level-n">${w.level}</span></div>`;
    }).join("");
    const its = (s.wires || []).filter((w) => w.source === src.id);
    const all = its.length > 1
      ? `<div class="row" style="gap:8px;align-items:center;margin-top:8px;"><span class="sub" style="min-width:84px;">All of them</span>
          ${range(`sound:all:${src.id}`, Math.max(...its.map((w) => w.level)), `${src.name} on all its speakers`)}</div>`
      : "";
    return `<div class="card">
      <div class="title">${esc(src.name)}</div>
      <div class="sub">${esc(soundSourceSaid(src))}</div>
      <div class="model-picker" style="margin-top:8px;">${chips || `<span class="sub">No speakers yet.</span>`}</div>
      ${wires}${all}</div>`;
  }).join("");

  // Each speaker: how loud, what it plays with, and whether something else has it.
  const groupOf = (id) => (s.groups || []).find((g) => (g.speakers || []).includes(id));
  const speakerRows = speakers.map((sp) => {
    const g = groupOf(sp.id);
    const mates = g ? g.speakers.filter((x) => x !== sp.id).map((x) => soundName("speaker", x)) : [];
    const said = [
      SOUND_KIND[sp.kind] || sp.kind,
      sp.borrowed ? "in use by another device" : null,
      sp.kind === "stream" ? (listening.has(sp.id) ? "listening" : "not listening") : null,
      !g ? "plays nothing" : mates.length ? `in sync with ${mates.join(", ")}` : null,
    ].filter(Boolean).join(" · ");
    return `<div class="card"><div class="title">${esc(sp.name)}</div><div class="sub">${esc(said)}</div>
      <div class="row" style="gap:8px;align-items:center;margin-top:6px;"><span class="sub" style="min-width:84px;">Volume</span>
        ${range(`sound:volume:${sp.id}`, sp.volume, `${sp.name} volume`)}<span class="sub level-n">${sp.volume}</span></div>
      <div class="row" style="justify-content:flex-end;margin-top:6px;"><button class="ghost" data-act="sound:removespeaker:${sp.id}">Delete</button></div></div>`;
  }).join("");

  // Setups: sets of wires, by name.
  const setups = (s.setups || []).map((x, i) => `<div class="row" style="gap:8px;padding:6px 0;align-items:center;">
      <div class="grow">${esc(x.name)}<div class="sub">${(x.wires || []).length} wire${(x.wires || []).length === 1 ? "" : "s"}</div></div>
      <button class="primary" data-act="sound:play:${i}">Play</button>
      <button class="ghost" data-act="sound:forget:${i}">Forget</button></div>`).join("");

  // Speakers another device is playing to: first, each with the way to take it back.
  const borrowedRows = speakers.filter((sp) => sp.borrowed).map((sp) =>
    `<div class="row" style="gap:8px;align-items:center;padding:4px 0;"><div class="grow"><b>${esc(sp.name)}</b>
      <div class="sub">In use by another device. Its wires wait; take it back when you want them again.</div></div>
      <button class="primary" data-act="sound:free:${sp.id}">Take it back</button></div>`).join("");
  // Adding a station: in the empty "what plays where" card while there is nothing, else under
  // "Add or remove". Never both, so its fields stay one of each.
  const radioForm = `<div class="sub" style="margin-top:8px;">A radio station</div>
      <div class="form"><input id="sound-radio-name" placeholder="Name, e.g. Classical" maxlength="40" autocomplete="off" />
        <input id="sound-radio-address" placeholder="https://… the station's stream" autocomplete="off" />
        <button class="primary" data-act="sound:addradio">Add</button></div>`;
  // AirPlay speakers the senders can see that are not speakers here yet.
  const have = new Set(speakers.map((x) => x.name));
  const foundRows = (SOUND.found || []).map((name, i) => have.has(name) ? "" :
    `<div class="row" style="gap:8px;padding:4px 0;"><span class="grow">${esc(name)}</span><button class="ghost" data-act="sound:addfound:${i}">Add</button></div>`).join("");
  const manage = [
    ...(s.sources || []).map((x) => `<div class="row" style="gap:8px;padding:4px 0;"><span class="grow">${esc(x.name)} <span class="sub">${esc(SOUND_KIND[x.kind] || x.kind)}</span></span><button class="ghost" data-act="sound:removesource:${x.id}">Remove</button></div>`),
    ...speakers.map((x) => `<div class="row" style="gap:8px;padding:4px 0;"><span class="grow">${esc(x.name)} <span class="sub">${esc(SOUND_KIND[x.kind] || x.kind)}</span></span><button class="ghost" data-act="sound:removespeaker:${x.id}">Remove</button></div>`),
  ].join("");

  return `${head}
    ${here}
    <h3>What plays where</h3>
    ${borrowedRows ? `<div class="card">${borrowedRows}</div>` : ""}
    ${sources || `<div class="card"><div class="title">Add something to play</div>
      <div class="sub">A radio station is the simplest start; your devices and Spotify come later.</div>${radioForm}</div>`}
    <h3>Speakers</h3>
    ${speakerRows || `<div class="card"><div class="sub">No speakers yet.</div></div>`}
    <h3>Setups</h3>
    <div class="card">${setups || `<div class="sub">None saved yet.</div>`}
      <div class="form" style="margin-top:8px;"><input id="sound-setup" placeholder="Name these wires, e.g. Kitchen mornings" maxlength="40" autocomplete="off" />
        <button class="ghost" data-act="sound:save">Save</button></div></div>
    <details class="card" style="margin-top:12px;"><summary class="title">Add or remove</summary>
      ${sources ? radioForm : ""}
      ${foundRows ? `<div class="sub" style="margin-top:8px;">Found on the network</div>${foundRows}` : ""}
      <div class="sub" style="margin-top:8px;">An AirPlay speaker, by the name it shows</div>
      <div class="form"><input id="sound-airplay-name" placeholder="e.g. Kitchen" maxlength="40" autocomplete="off" />
        <button class="ghost" data-act="sound:addairplay">Add</button></div>
      <div style="margin-top:8px;">${manage}</div></details>`;
}

// The verbs this screen uses. Returns true when it handled one.
async function soundDispatch(verb, noun, id, arg) {
  if (!(verb === "sound")) return false;
  const s = SOUND.state || {};
  // Playing starts straight from the tap, with the ticket already in hand.
  if (noun === "listen") {
    if (!SOUND.ticket) { await soundTicket(); }
    if (!SOUND.ticket) { flash("Couldn't start listening.", "err"); return true; }
    const ticket = SOUND.ticket; SOUND.ticket = null;
    SOUND.listening = true; SOUND.liveBlocked = false;
    soundRemember(true); soundBanner("");
    try { await soundPlay(ticket); } catch (e) { SOUND.listening = false; SOUND.mode = null; flash("Couldn't play: " + e.message, "err"); }
    render(); await soundTicket(); return true;
  }
  if (noun === "mode") {
    try { localStorage.setItem("endora.soundMode", id === "live" || id === "listening" ? id : "auto"); } catch (_) {}
    await soundApplyMode();
    render(); return true;
  }
  // The tap that lets live sound start again after the screen was off.
  if (noun === "golive") {
    const ctx = liveContext();
    if (ctx) ctx.resume().catch(() => {});
    SOUND.liveBlocked = false; soundBanner("");
    await soundApplyMode();
    render(); return true;
  }
  if (noun === "measure") { await soundMeasure(); return true; }
  if (noun === "sync") { await soundSyncBy(Number(id) || 0); return true; }
  if (noun === "forgetlisten") { soundRemember(false); soundBanner(""); return true; }
  if (noun === "stop") {
    SOUND.listening = false; SOUND.mode = null; SOUND.gen++; SOUND.stat = null;
    soundRemember(false);
    liveClose(LIVE.session); LIVE.session = null;
    const a = soundAudio(); a.pause(); a.removeAttribute("src"); a.load();
    render(); return true;
  }
  try {
    if (noun === "mine") {
      const name = val("sound-mine") || soundGuessName();
      const made = await api("POST", "/v1/sound/speakers", { name, kind: "stream", device: name.toLowerCase() });
      try { localStorage.setItem("endora.soundSpeaker", String(made.id)); } catch (_) {}
      flash("This device is a speaker now. Cast into “Endora Mixer” from any device to play through it.", "ok");
    } else if (noun === "wire") {
      const [source, speaker] = String(id).split("-").map(Number);
      if (soundWire(source, speaker)) await api("DELETE", `/v1/sound/wires/${source}/${speaker}`);
      else await api("PUT", "/v1/sound/wires", { source, speaker, level: 80 });
    } else if (noun === "level") {
      const [source, speaker] = String(id).split("-").map(Number);
      await api("PUT", "/v1/sound/wires", { source, speaker, level: Number(arg) });
    } else if (noun === "all") {
      await api("PUT", `/v1/sound/sources/${Number(id)}/level`, { level: Number(arg) });
    } else if (noun === "volume") {
      await api("PUT", `/v1/sound/speakers/${Number(id)}/volume`, { level: Number(arg) });
      const mine = soundMine();
      if (mine && mine.id === Number(id)) soundAudio().volume = Number(arg) / 100;
    } else if (noun === "save") {
      const name = val("sound-setup"); if (!name) return true;
      await api("POST", "/v1/sound/setups", { name });
      flash("Saved.", "ok");
    } else if (noun === "play" || noun === "forget") {
      const x = (s.setups || [])[Number(id)]; if (!x) return true;
      if (noun === "play") await api("POST", `/v1/sound/setups/${encodeURIComponent(x.name)}/apply`, {});
      else { if (!confirm(`Forget the setup "${x.name}"?`)) return true; await api("DELETE", `/v1/sound/setups/${encodeURIComponent(x.name)}`); }
    } else if (noun === "addradio") {
      const name = val("sound-radio-name"), address = val("sound-radio-address");
      if (!name || !address) { flash("A station needs a name and its address.", "warn"); return true; }
      await api("POST", "/v1/sound/sources", { name, kind: "radio", address });
    } else if (noun === "addfound") {
      const name = (SOUND.found || [])[Number(id)]; if (!name) return true;
      await api("POST", "/v1/sound/speakers", { name, kind: "airplay" });
    } else if (noun === "free") {
      await api("POST", `/v1/sound/speakers/${Number(id)}/free`, {});
    } else if (noun === "addairplay") {
      const name = val("sound-airplay-name"); if (!name) return true;
      await api("POST", "/v1/sound/speakers", { name, kind: "airplay" });
    } else if (noun === "removesource" || noun === "removespeaker") {
      const what = noun === "removesource" ? "source" : "speaker";
      if (!confirm(`Remove ${soundName(what, Number(id))} and its wires?`)) return true;
      if (what === "source") await api("DELETE", `/v1/sound/sources/${Number(id)}`);
      else await api("DELETE", `/v1/sound/speakers/${Number(id)}`);
    }
  } catch (e) { flash(e.message, "err"); }
  // Music shows speakers' volumes too: it reloads its own way.
  if (NAV.v === "music") { await loadMusic(); render(); return true; }
  await loadSound(); render(); return true;
}

// While a level is dragged, its fill and its number follow; it is sent once let go.
document.body.addEventListener("input", (ev) => {
  const r = ev.target;
  if (!(r instanceof HTMLInputElement) || !r.classList.contains("level")) return;
  r.style.setProperty("--v", String(Number(r.value) / 100));
  const n = r.nextElementSibling;
  if (n && n.classList.contains("level-n")) n.textContent = r.value;
});

// ---- keeping up: a new version, a stream gone quiet, and a reload while listening ----------
// Every half minute: has a new build been deployed? Has this device's stream stopped moving?
// A stream that has is joined again with a fresh ticket. A new build reloads the page, unless
// that would cut off what is playing or what is being typed: then it asks. And a page reloaded
// while it was listening offers to listen again at once: an iPhone starts sound only from a
// tap, so it cannot simply carry on by itself.

function soundRemember(on) { try { localStorage.setItem("endora.soundListening", on ? "1" : "0"); } catch (_) {} }
function soundWasListening() { try { return localStorage.getItem("endora.soundListening") === "1"; } catch (_) { return false; } }

// One line pinned above everything, outside the screen, so a re-render never takes it away.
function soundBanner(html) {
  let b = document.getElementById("sound-banner");
  if (!html) { if (b) b.remove(); return; }
  if (!b) {
    b = document.createElement("div");
    b.id = "sound-banner";
    b.className = "card";
    b.style.cssText = "position:fixed;left:12px;right:12px;bottom:12px;z-index:50;display:flex;gap:10px;align-items:center;";
    document.body.appendChild(b);
  }
  b.innerHTML = html;
}

const SOUND_KEEP = { last: -1, still: 0 };

async function soundKeepUp() {
  // A new build: the page loaded one, the node now serves another.
  try {
    const h = await (await fetch("/health")).json();
    const pill = document.querySelector('[title^="build "]');
    const loaded = pill ? pill.getAttribute("title").slice(6) : "";
    if (h && h.build && h.build !== "dev" && loaded && loaded !== "dev" && h.build !== loaded) {
      const typing = typeof hqTyping === "function" && hqTyping();
      if (!SOUND.listening && !typing) { location.reload(); return; }
      soundBanner(`<div class="grow">A new version of Endora is ready.</div><button class="primary" data-act="reload">Update</button>`);
    }
  } catch (_) { /* the node is restarting: try again next time */ }
  // A stream that stopped moving while this device listens: join it again.
  if (SOUND.listening && SOUND.mode === "live") {
    // Live frames come every 20 ms: none for five seconds is a stream that is gone. And a
    // ticket is kept ready, for the hand-over to the MP3 when the page goes to the background.
    const s = LIVE.session;
    if (s && performance.now() - s.lastAt > 5000) soundReconnect();
    else soundTicket();
  } else if (SOUND.listening) {
    const a = soundAudio();
    const now = a.currentTime;
    SOUND_KEEP.still = now === SOUND_KEEP.last ? SOUND_KEEP.still + 1 : 0;
    SOUND_KEEP.last = now;
    if (SOUND_KEEP.still >= 1) { SOUND_KEEP.still = 0; soundReconnect(); }
    else soundTicket();
  }
}

// Reloaded while listening: a ticket ready, and one tap to listen again.
async function soundOfferResume() {
  if (!soundWasListening() || SOUND.listening) return;
  try { SOUND.state = SOUND.state || await api("GET", "/v1/sound"); } catch (_) { return; }
  if (!soundMine()) return;
  await soundTicket();
  if (SOUND.ticket) soundBanner(`<div class="grow">You were listening on this ${esc(soundMine().name)}.</div><button class="primary" data-act="sound:listen">Listen again</button><button class="ghost" data-act="sound:forgetlisten">No</button>`);
}

if (typeof window !== "undefined" && typeof setInterval === "function" && !window.__soundKeepUp) {
  window.__soundKeepUp = true;
  // In a browser these are numbers; under Node (the console check) they are timers, unref'd so
  // they never keep a finished check running.
  const every = setInterval(soundKeepUp, 30000);
  const once = setTimeout(soundOfferResume, 2500);
  for (const t of [every, once]) if (t && typeof t.unref === "function") t.unref();
}
