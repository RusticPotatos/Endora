// ---- Speakers: endora-sound's remote (ADR 0104) ---------------------------------------------
// Sources on one side, speakers on the other, and wires between them, each with its own
// level. Tap a speaker under a source to wire it; drag the wire's slider to set its share of
// that speaker's mix. Speakers with the same wires play as one group, in sync. This device
// can be a speaker too: it listens to the hub's stream.
//
// The console talks to the node only. Listening uses a ticket the node hands out, good for a
// minute for this speaker, because an <audio> element cannot sign its request.

const SOUND = {
  state: null,        // what the hub says: sources, speakers, wires, groups, setups, listening
  error: "",
  found: [],          // AirPlay speakers the senders can see, by name
  ticket: null,       // { url, at } a fresh ticket for this device's stream, ready for a tap
  listening: false,   // whether this device is playing its stream
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
    SOUND.ticket = { url: t.url, at: Date.now() };
  } catch (_) { SOUND.ticket = null; }
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
  const a = soundAudio();
  a.src = SOUND.ticket.url; SOUND.ticket = null;
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

const SOUND_KIND = { airplay: "AirPlay", spotify: "Spotify", radio: "radio", voice: "Endora's voice", stream: "stream" };

function viewSound() {
  const head = `${labCrumbs({ label: "Music", act: "go:music" }, { label: "Speakers" })}<h2>Speakers</h2>`;
  if (SOUND.error) return `${head}<div class="card"><div class="sub">${esc(SOUND.error)}</div></div>`;
  const s = SOUND.state;
  if (!s) return `${head}<div class="note">Asking the hub…</div>`;
  const mine = soundMine();
  const listening = new Set(s.listening || []);
  const range = (act, value, label) =>
    `<input type="range" min="0" max="100" step="5" value="${Number(value) || 0}" data-change="${act}" aria-label="${esc(label)}" style="flex:1;min-width:90px;" />`;

  // This device, as a speaker.
  const here = mine
    ? `<div class="card"><div class="row" style="align-items:center;gap:10px;">
        <div class="grow"><div class="title">This ${esc(mine.name)}</div>
          <div class="sub">${SOUND.listening ? "Listening to its wires" : "Not listening"} · a stream, a few seconds behind the speakers</div></div>
        ${SOUND.listening
          ? `<button class="ghost" data-act="sound:stop">Stop</button>`
          : `<button class="primary" data-act="sound:listen">Listen here</button>`}
      </div></div>`
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
      return `<button class="pill pick${w ? " active" : ""}" data-act="sound:wire:${src.id}-${sp.id}" aria-pressed="${w ? "true" : "false"}">${esc(sp.name)}${w ? ` · ${w.level}` : ""}</button>`;
    }).join("");
    const wires = speakers.map((sp) => {
      const w = soundWire(src.id, sp.id);
      if (!w) return "";
      return `<div class="row" style="gap:8px;align-items:center;margin-top:6px;">
        <span class="sub" style="min-width:84px;">${esc(sp.name)}</span>
        ${range(`sound:level:${src.id}-${sp.id}`, w.level, `${src.name} on ${sp.name}`)}
        <span class="sub" style="min-width:28px;text-align:right;">${w.level}</span></div>`;
    }).join("");
    const its = (s.wires || []).filter((w) => w.source === src.id);
    const all = its.length > 1
      ? `<div class="row" style="gap:8px;align-items:center;margin-top:8px;"><span class="sub" style="min-width:84px;">All of them</span>
          ${range(`sound:all:${src.id}`, Math.max(...its.map((w) => w.level)), `${src.name} on all its speakers`)}</div>`
      : "";
    return `<div class="card">
      <div class="title">${esc(src.name)}</div>
      <div class="sub">${esc(SOUND_KIND[src.kind] || src.kind)}</div>
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
    const back = sp.borrowed
      ? `<div class="row" style="justify-content:flex-end;margin-top:6px;"><button class="ghost" data-act="sound:free:${sp.id}">Take it back</button></div>`
      : "";
    return `<div class="card"><div class="title">${esc(sp.name)}</div><div class="sub">${esc(said)}</div>${back}
      <div class="row" style="gap:8px;align-items:center;margin-top:6px;"><span class="sub" style="min-width:84px;">Volume</span>
        ${range(`sound:volume:${sp.id}`, sp.volume, `${sp.name} volume`)}<span class="sub" style="min-width:28px;text-align:right;">${sp.volume}</span></div></div>`;
  }).join("");

  // Setups: sets of wires, by name.
  const setups = (s.setups || []).map((x, i) => `<div class="row" style="gap:8px;padding:6px 0;align-items:center;">
      <div class="grow">${esc(x.name)}<div class="sub">${(x.wires || []).length} wire${(x.wires || []).length === 1 ? "" : "s"}</div></div>
      <button class="primary" data-act="sound:play:${i}">Play</button>
      <button class="ghost" data-act="sound:forget:${i}">Forget</button></div>`).join("");

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
    ${sources || `<div class="card"><div class="sub">No sources yet. Add a radio station below.</div></div>`}
    <h3>Speakers</h3>
    ${speakerRows || `<div class="card"><div class="sub">No speakers yet.</div></div>`}
    <h3>Setups</h3>
    <div class="card">${setups || `<div class="sub">None saved yet.</div>`}
      <div class="form" style="margin-top:8px;"><input id="sound-setup" placeholder="Name these wires, e.g. Kitchen mornings" maxlength="40" autocomplete="off" />
        <button class="ghost" data-act="sound:save">Save</button></div></div>
    <details class="card" style="margin-top:12px;"><summary class="title">Add or remove</summary>
      <div class="sub" style="margin-top:8px;">A radio station</div>
      <div class="form"><input id="sound-radio-name" placeholder="Name, e.g. Classical" maxlength="40" autocomplete="off" />
        <input id="sound-radio-address" placeholder="https://… the station's stream" autocomplete="off" />
        <button class="ghost" data-act="sound:addradio">Add</button></div>
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
    const a = soundAudio();
    if (!SOUND.ticket) { await soundTicket(); }
    if (!SOUND.ticket) { flash("Couldn't start listening.", "err"); return true; }
    a.src = SOUND.ticket.url; SOUND.ticket = null;
    SOUND.listening = true;
    try { await a.play(); } catch (e) { SOUND.listening = false; flash("Couldn't play: " + e.message, "err"); }
    render(); await soundTicket(); return true;
  }
  if (noun === "stop") {
    SOUND.listening = false;
    const a = soundAudio(); a.pause(); a.removeAttribute("src"); a.load();
    render(); return true;
  }
  try {
    if (noun === "mine") {
      const name = val("sound-mine") || soundGuessName();
      const made = await api("POST", "/v1/sound/speakers", { name, kind: "stream", device: name.toLowerCase() });
      try { localStorage.setItem("endora.soundSpeaker", String(made.id)); } catch (_) {}
      flash("This device is a speaker now.", "ok");
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
  await loadSound(); render(); return true;
}
