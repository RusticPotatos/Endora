// ---- the home lab, media, smart home and music (plan phase 4) --------------
// What the handheld can do, on the web too: the web console is the full set (ADR 0099). These
// screens read the same typed endpoints the handheld does (`/v1/terminal/*`), and act through
// the same one action route, whose vocabulary the node allowlists. Music borrows Spotify's
// access token from the node, which keeps the sign-in.

// Why a page could not load, in words. The node answers "Home console: <category>".
function labTrouble(e) {
  const category = String((e && e.message) || e).replace(/^Home console:\s*/, "");
  if (category === "unconfigured" || category === "configuration") {
    return "Not set up yet. Add the service under Settings › Connections.";
  }
  if (category === "unavailable" || category === "upstream") return "The service did not answer. Try again in a moment.";
  return "Could not load this: " + category + ".";
}

// One page from the node, kept by name; a failure is kept too, so the screen can say why.
async function labLoad(name, path) {
  try { LAB[name] = await api("GET", path); } catch (e) { LAB[name] = { error: labTrouble(e) }; }
}

// Everything these screens fetch, by screen. Called when one opens and on a reload while it shows.
async function loadLab(v, id) {
  if (v === "lab") await labLoad("dashboard", "/v1/terminal/dashboard");
  if (v === "containers") await labLoad("containers", "/v1/terminal/containers");
  if (v === "logs" && id) await labLoad("logs", `/v1/terminal/logs/${encodeURIComponent(id)}`);
  if (v === "jobs") await labLoad("jobs", "/v1/terminal/jobs");
  if (v === "job" && id) await labLoad("job", `/v1/terminal/jobs/${encodeURIComponent(id)}`);
  if (v === "media") {
    await labLoad("requests", "/v1/terminal/media/requests");
    if (LAB.mediaQuery) await labLoad("found", `/v1/terminal/media?query=${encodeURIComponent(LAB.mediaQuery)}`);
  }
  if (v === "house") {
    await labLoad("favorites", "/v1/terminal/home");
    if (LAB.catalogQuery !== undefined) await labLoad("catalog", `/v1/terminal/home/catalog?query=${encodeURIComponent(LAB.catalogQuery)}`);
  }
  if (v === "music") await loadMusic();
}

// One action through the node, which checks it against its own list before doing anything.
// The id is new for each tap, so a retry of the same tap is never done twice.
async function labAct(kind, action, target) {
  const request_id = `web-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  const op = await api("POST", "/v1/terminal/action", { request_id, kind, action, target, confirmed: true });
  flash(op.summary || op.state, op.state === "uncertain" ? "err" : "ok");
  return op;
}

const labItems = (page) => (page && Array.isArray(page.items) ? page.items : []);
const labError = (page) => (page && page.error ? `<div class="note">${esc(page.error)}</div>` : "");
const labCrumbs = (...rest) => crumbs([{ label: "Home", act: "go:chat" }, ...rest]);
const stateClass = (state) =>
  state === "running" ? "concluded" : state === "unhealthy" ? "rejected" : state === "unknown" ? "" : "pending";

// ---- Home lab ----------------------------------------------------------------
function viewLab() {
  const d = LAB.dashboard;
  const head = `${labCrumbs({ label: "Home lab" })}<h2>Home lab</h2>`;
  const nav = `
    <div class="card nav-list">
      <button class="ghost" data-act="go:containers">${icon("box")}<span class="grow" style="text-align:left;">Containers<span class="sub" style="display:block;font-weight:400;">start, stop, restart and read logs</span></span>${icon("chevron", 15)}</button>
      <button class="ghost" data-act="go:jobs">${icon("clock")}<span class="grow" style="text-align:left;">Jobs<span class="sub" style="display:block;font-weight:400;">the Ansible runs you allowed</span></span>${icon("chevron", 15)}</button>
    </div>`;
  if (!d) return `${head}${nav}<div class="note">Asking the home lab…</div>`;
  if (d.error) return `${head}${nav}${labError(d)}`;
  const stats = (d.stats || []).map((s) => `<span class="pill">${esc(s)}</span>`).join(" ");
  const groups = (d.groups || []).map((g) => {
    const services = g.services || [];
    const up = services.filter((s) => s.state === "running").length;
    const trouble = services.some((s) => s.state === "unhealthy" || s.state === "stopped");
    const rows = services.map((s) => `
      <div class="row" style="gap:8px;padding:6px 0;">
        <div class="grow"><div>${esc(s.name)}</div>${s.detail ? `<div class="sub">${esc(s.detail)}</div>` : ""}</div>
        <span class="pill ${stateClass(s.state)}">${esc(s.state)}</span>
        ${s.id ? `<button class="ghost" data-act="go:logs:${esc(s.id)}">Logs</button>` : ""}
      </div>`).join("");
    return `
      <details class="card"${trouble ? " open" : ""}>
        <summary><span class="title">${esc(g.name)}</span> <span class="pill${trouble ? " rejected" : ""}">${up} of ${services.length} up</span></summary>
        ${rows}
      </details>`;
  }).join("");
  return `${head}
    ${stats ? `<div class="row" style="gap:6px;flex-wrap:wrap;margin-bottom:10px;">${stats}</div>` : ""}
    ${nav}
    <h3>Services</h3>
    ${listOr(groups, "Homepage lists no services.")}`;
}

function viewContainers() {
  const page = LAB.containers;
  const head = `${labCrumbs({ label: "Home lab", act: "go:lab" }, { label: "Containers" })}<h2>Containers</h2>`;
  if (!page) return `${head}<div class="note">Asking Portainer…</div>`;
  if (page.error) return `${head}${labError(page)}`;
  // The ones that need you first.
  const items = labItems(page).slice().sort((a, b) =>
    Number(a.detail.startsWith("running")) - Number(b.detail.startsWith("running")));
  const rows = items.map((c) => {
    const running = c.detail.startsWith("running");
    const controls = c.protected
      ? `<span class="pill" title="Stopping it would cut the path to Endora">keeps Endora reachable</span>`
      : running
        ? `<button class="ghost" data-act="lab:container:restart:${esc(c.id)}">Restart</button>
           <button class="ghost danger" data-act="lab:container:stop:${esc(c.id)}">Stop</button>`
        : `<button class="ghost" data-act="lab:container:start:${esc(c.id)}">Start</button>`;
    return `
      <div class="card">
        <div class="row" style="gap:8px;"><div class="grow"><div class="title">${esc(c.label)}</div><div class="sub">${esc(c.detail)}</div></div></div>
        <div class="row" style="gap:6px;flex-wrap:wrap;justify-content:flex-end;margin-top:6px;">
          <button class="ghost" data-act="go:logs:${esc(c.id)}">Logs</button>${controls}
        </div>
      </div>`;
  });
  return `${head}
    ${page.summary ? `<div class="note" style="margin-bottom:10px;">${esc(page.summary)}</div>` : ""}
    ${listOr(rows, "Portainer lists no containers.")}`;
}

function viewLogs() {
  const page = LAB.logs;
  const name = (labItems(LAB.containers).find((c) => c.id === NAV.id) || {}).label || "Logs";
  const head = `${labCrumbs({ label: "Home lab", act: "go:lab" }, { label: "Containers", act: "go:containers" }, { label: name })}
    <h2>${esc(name)}</h2>
    <div class="row" style="justify-content:flex-end;"><button class="ghost" data-act="lab:refresh">Refresh</button></div>`;
  if (!page) return `${head}<div class="note">Reading the last hundred lines…</div>`;
  if (page.error) return `${head}${labError(page)}`;
  return `${head}<pre class="step-out" style="max-height:none;margin:0;">${esc(page.summary || "(nothing logged)")}</pre>`;
}

function viewJobs() {
  const page = LAB.jobs;
  const head = `${labCrumbs({ label: "Home lab", act: "go:lab" }, { label: "Jobs" })}<h2>Jobs</h2>
    <div class="note" style="margin-bottom:10px;">Only the runs listed in the job agent's own configuration on the NAS. Each one is asked before it starts.</div>`;
  if (!page) return `${head}<div class="note">Asking the job agent…</div>`;
  if (page.error) return `${head}${labError(page)}`;
  const rows = labItems(page).map((j) => `
    <div class="card"><div class="row" style="gap:8px;">
      <div class="grow"><div class="title">${esc(j.label)}</div><div class="sub">${esc(j.detail)}</div></div>
      ${j.latest_id ? `<button class="ghost" data-act="go:job:${esc(j.latest_id)}">Last run</button>` : ""}
      <button class="ghost" data-act="lab:job:run:${esc(j.id)}">Run</button>
    </div></div>`);
  return `${head}${listOr(rows, "No jobs are allowed yet.")}`;
}

function viewJob() {
  const page = LAB.job;
  const head = `${labCrumbs({ label: "Home lab", act: "go:lab" }, { label: "Jobs", act: "go:jobs" }, { label: "Run" })}<h2>Job run</h2>
    <div class="row" style="justify-content:flex-end;"><button class="ghost" data-act="lab:refresh">Refresh</button></div>`;
  if (!page) return `${head}<div class="note">Reading its output…</div>`;
  if (page.error) return `${head}${labError(page)}`;
  return `${head}<div class="note">${esc(page.state || "")}</div>
    <pre class="step-out" style="max-height:none;margin:0;">${esc(page.summary || "(no output yet)")}</pre>`;
}

// ---- Media -------------------------------------------------------------------
function viewMedia() {
  const head = `${labCrumbs({ label: "Media" })}<h2>Media</h2>`;
  const search = `
    <div class="form" style="margin-bottom:10px;">
      <input id="media-q" type="search" placeholder="a film or a show" autocomplete="off" value="${esc(LAB.mediaQuery || "")}" />
      <button class="primary" data-act="media:search">Search</button>
    </div>`;
  const found = LAB.found;
  const results = !LAB.mediaQuery ? "" : !found ? `<div class="note">Searching…</div>` : found.error ? labError(found)
    : listOr(labItems(found).map((m) => {
      const open = /not requested/i.test(m.detail);
      return `<div class="card"><div class="row" style="gap:8px;">
        <div class="grow"><div class="title">${esc(m.label)}</div><div class="sub">${esc(m.detail)}</div></div>
        ${open && m.media_type ? `<button class="ghost" data-act="media:request:${esc(m.media_type)}:${esc(m.id)}">Request</button>` : ""}
      </div></div>`;
    }), "Nothing by that name.");
  const recent = LAB.requests;
  const requests = !recent ? `<div class="note">Asking Overseerr…</div>` : recent.error ? labError(recent)
    : listOr(labItems(recent).map((m) => `<div class="card"><div class="title">${esc(m.label)}</div><div class="sub">${esc(m.detail)}</div></div>`),
      "No requests yet.");
  return `${head}${search}${results}<h3>Recent requests</h3>${requests}`;
}

// ---- Smart home --------------------------------------------------------------
// Favorites are the only things this screen switches: the node refuses an entity that is not
// one, so choosing favorites is choosing what may be switched from a phone.
function houseControls(item) {
  const [domain] = String(item.id || "").split(".");
  const now = String(item.detail || "").trim();
  if (domain === "scene") return `<button class="ghost" data-act="house:home:scene:${esc(item.id)}">Set</button>`;
  if (!["light", "switch", "input_boolean"].includes(domain)) return "";
  return `<button class="${now === "on" ? "primary" : "ghost"}" data-act="house:home:on:${esc(item.id)}">On</button>
    <button class="${now === "off" ? "primary" : "ghost"}" data-act="house:home:off:${esc(item.id)}">Off</button>`;
}

function viewHouse() {
  const head = `${labCrumbs({ label: "Smart home" })}<h2>Smart home</h2>`;
  const page = LAB.favorites;
  const favorites = !page ? `<div class="note">Asking Home Assistant…</div>` : page.error ? labError(page)
    : listOr(labItems(page).map((f) => `<div class="card"><div class="row" style="gap:8px;">
        <div class="grow"><div class="title">${esc(f.label)}</div><div class="sub">${esc(f.detail)}</div></div>
        ${houseControls(f)}
      </div></div>`), "No favorites yet. Choose some below.");
  const catalog = LAB.catalog;
  const choices = LAB.catalogQuery === undefined ? "" : !catalog ? `<div class="note">Asking Home Assistant…</div>` : catalog.error ? labError(catalog)
    : listOr(labItems(catalog).map((c) => `<div class="row" style="gap:8px;padding:6px 0;">
        <div class="grow"><div>${esc(c.label)}</div><div class="sub">${esc(c.id)} · ${esc(c.detail)}</div></div>
        <button class="${c.favorite ? "primary" : "ghost"}" data-act="house:favorite:${c.favorite ? "remove" : "add"}:${esc(c.id)}">${c.favorite ? "Favorite" : "Add"}</button>
      </div>`), "Nothing by that name.");
  return `${head}${favorites}
    <details class="card" style="margin-top:12px;"${LAB.catalogQuery !== undefined ? " open" : ""}>
      <summary class="title">Choose favorites</summary>
      <div class="form" style="margin:8px 0;">
        <input id="house-q" type="search" placeholder="a light, a switch, a scene (blank for all)" autocomplete="off" value="${esc(LAB.catalogQuery || "")}" />
        <button class="ghost" data-act="house:search">Show</button>
      </div>
      ${choices}
    </details>`;
}

// ---- Music -------------------------------------------------------------------
// Spotify, played from here, with the node's sign-in: the browser borrows an access token for
// an hour and asks again when it runs out. The refresh token never leaves Endora.
let MUSIC_TOKEN = { access: "", until: 0 };

async function spotifyToken(fresh) {
  if (!fresh && MUSIC_TOKEN.access && Date.now() < MUSIC_TOKEN.until) return MUSIC_TOKEN.access;
  const lent = await api("POST", "/v1/connections/spotify/token", {});
  MUSIC_TOKEN = { access: lent.access_token, until: Date.now() + Math.max(0, lent.expires_in - 60) * 1000 };
  return MUSIC_TOKEN.access;
}

// One Spotify Web API call. A token Spotify no longer accepts is borrowed again, once.
async function spotify(method, path, body, retried) {
  const res = await fetch("https://api.spotify.com/v1" + path, {
    method,
    headers: { authorization: "Bearer " + (await spotifyToken(retried)), ...(body ? { "content-type": "application/json" } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  if (res.status === 401 && !retried) return spotify(method, path, body, true);
  if (res.status === 202 || res.status === 204) return null;
  if (res.status === 403) throw new Error("Spotify needs Premium for that");
  if (res.status === 404) throw new Error("No device is playing. Pick one under Play on");
  if (res.status === 429) throw new Error("Spotify is busy; try again in a moment");
  if (!res.ok) throw new Error("Spotify could not do that");
  const text = await res.text();
  return text ? JSON.parse(text) : null;
}

// Search results, playlists and liked songs in the one shape the screen lists.
function musicItems(list, kind) {
  return (list || []).filter(Boolean).map((x) => {
    const track = x.track && x.track.uri ? x.track : null; // a liked song wraps its track
    const it = track || x;
    const who = (it.artists || []).map((a) => a.name).join(", ") || (it.owner && it.owner.display_name) || "";
    return { uri: it.uri, name: it.name, kind: track ? "track" : (it.type || kind), who };
  }).filter((x) => /^spotify:[a-z]+:[A-Za-z0-9]+$/.test(x.uri || ""));
}

async function loadMusic() {
  MUSIC.error = "";
  try { SPOTIFY = await api("GET", "/v1/connections/spotify"); } catch (_) { SPOTIFY = null; }
  if (!SPOTIFY || SPOTIFY.state !== "connected") return;
  try {
    const [now, devices] = await Promise.all([spotify("GET", "/me/player"), spotify("GET", "/me/player/devices")]);
    MUSIC.now = now;
    MUSIC.devices = ((devices && devices.devices) || []).filter((d) => d.id);
    if (!MUSIC.lists.length) MUSIC.lists = musicItems(((await spotify("GET", "/me/playlists?limit=30")) || {}).items, "playlist");
  } catch (e) { MUSIC.error = e.message; }
}

// Where playback goes: the device playing now, else the one last picked here, else the first.
function musicDevice() {
  const active = MUSIC.devices.find((d) => d.is_active);
  if (active) return active.id;
  let picked = "";
  try { picked = localStorage.getItem("endora.musicDevice") || ""; } catch (_) {}
  const remembered = MUSIC.devices.find((d) => d.id === picked);
  return remembered ? remembered.id : (MUSIC.devices[0] || {}).id;
}

function viewMusic() {
  const head = `${labCrumbs({ label: "Music" })}<h2>Music</h2>`;
  if (!SPOTIFY) return `${head}<div class="note">Asking Endora…</div>`;
  if (SPOTIFY.state !== "connected") {
    return `${head}<div class="card"><div class="title">Spotify is not connected</div>
      <div class="sub">Connect it once and every device plays with it: this one, the handheld and chat.</div>
      <div class="row" style="justify-content:flex-end;margin-top:8px;"><button class="primary" data-act="go:spotify">Connect Spotify</button></div></div>`;
  }
  const n = MUSIC.now;
  const item = n && n.item;
  const art = item && item.album && (item.album.images || []).slice(-2)[0];
  const artUrl = art && /^https:\/\//.test(art.url) ? art.url : "";
  const playing = !!(n && n.is_playing);
  const volume = n && n.device ? n.device.volume_percent : null;
  const nowCard = item
    ? `<div class="card"><div class="row" style="gap:12px;align-items:center;">
        ${artUrl ? `<img src="${esc(artUrl)}" alt="" width="64" height="64" style="border-radius:8px;flex:none;" />` : ""}
        <div class="grow"><div class="title">${esc(item.name)}</div>
          <div class="sub">${esc((item.artists || []).map((a) => a.name).join(", "))}${n.device ? ` · on ${esc(n.device.name)}` : ""}</div></div>
      </div>
      <div class="row" style="gap:6px;justify-content:center;margin-top:10px;flex-wrap:wrap;">
        <button class="ghost" data-act="music:prev" aria-label="Previous">⏮</button>
        <button class="primary" data-act="music:toggle">${playing ? "Pause" : "Play"}</button>
        <button class="ghost" data-act="music:next" aria-label="Next">⏭</button>
        ${volume === null || volume === undefined ? "" : `<button class="ghost" data-act="music:volume:-10">−</button><span class="sub">${esc(volume)}%</span><button class="ghost" data-act="music:volume:10">+</button>`}
      </div></div>`
    : `<div class="card"><div class="sub">Nothing is playing.</div></div>`;
  const devices = MUSIC.devices.map((d) => `
    <button class="pill pick${d.is_active ? " active" : ""}" data-act="music:device:${esc(d.id)}">${esc(d.name)}</button>`).join("");
  const row = (x, key) => `<div class="row" style="gap:8px;padding:6px 0;">
      <div class="grow"><div>${esc(x.name)}</div><div class="sub">${esc(x.kind)}${x.who ? ` · ${esc(x.who)}` : ""}</div></div>
      <button class="ghost" data-act="music:play:${key}">Play</button></div>`;
  const found = MUSIC.found.map((x, i) => row(x, "f" + i)).join("");
  const lists = MUSIC.lists.map((x, i) => row(x, "l" + i)).join("");
  return `${head}
    ${MUSIC.error ? `<div class="note">${esc(MUSIC.error)}</div>` : ""}
    ${nowCard}
    <h3>Play on</h3>
    <div class="model-picker">${devices || `<span class="sub">No device is open. Open Spotify on a phone, a speaker or the handheld.</span>`}</div>
    <div class="form" style="margin:10px 0;">
      <input id="music-q" type="search" placeholder="a song, an album, an artist" autocomplete="off" value="${esc(MUSIC.query)}" />
      <button class="primary" data-act="music:search">Search</button>
    </div>
    ${found ? `<div class="card">${found}</div>` : ""}
    <details class="card" style="margin-top:12px;"><summary class="title">Your playlists</summary>${listOr(lists, "No playlists.")}</details>`;
}

// ---- actions -----------------------------------------------------------------
// The verbs these screens use. Returns true when it handled one.
async function labDispatch(verb, noun, id, arg) {
  if (verb === "lab" && noun === "refresh") { await loadLab(NAV.v, NAV.id); render(); return true; }
  if (verb === "lab" && noun === "container") {
    const name = (labItems(LAB.containers).find((c) => c.id === arg) || {}).label || "this container";
    if (id !== "start" && !confirm(`${id === "stop" ? "Stop" : "Restart"} ${name}?`)) return true;
    try { await labAct("container", id, arg); } catch (e) { flash(labTrouble(e), "err"); }
    await loadLab("containers"); render(); return true;
  }
  if (verb === "lab" && noun === "job") {
    const job = (labItems(LAB.jobs).find((j) => j.id === arg) || {}).label || arg;
    if (!confirm(`Run "${job}" on the NAS now?`)) return true;
    try { await labAct("job", "run", arg); } catch (e) { flash(labTrouble(e), "err"); }
    await loadLab("jobs"); render(); return true;
  }
  if (verb === "media" && noun === "search") {
    const field = document.getElementById("media-q");
    LAB.mediaQuery = field ? field.value.trim() : "";
    LAB.found = null; render();
    await loadLab("media"); render(); return true;
  }
  if (verb === "media" && noun === "request") {
    const title = (labItems(LAB.found).find((m) => m.id === arg) || {}).label || "it";
    if (!confirm(`Request ${title}?`)) return true;
    try { await labAct("media", id, arg); } catch (e) { flash(labTrouble(e), "err"); }
    await loadLab("media"); render(); return true;
  }
  if (verb === "house" && noun === "search") {
    const field = document.getElementById("house-q");
    LAB.catalogQuery = field ? field.value.trim() : "";
    LAB.catalog = null; render();
    await loadLab("house"); render(); return true;
  }
  if (verb === "house" && (noun === "home" || noun === "favorite")) {
    try { await labAct(noun, id, arg); } catch (e) { flash(labTrouble(e), "err"); }
    await loadLab("house"); render(); return true;
  }
  if (verb === "music") { await musicDispatch(noun, id); return true; }
  return false;
}

// Spotify's controls, through the borrowed token.
async function musicDispatch(noun, id) {
  try {
    if (noun === "toggle") {
      const playing = MUSIC.now && MUSIC.now.is_playing;
      const device = musicDevice();
      await spotify("PUT", playing ? "/me/player/pause" : `/me/player/play${device ? `?device_id=${encodeURIComponent(device)}` : ""}`);
    } else if (noun === "next" || noun === "prev") {
      await spotify("POST", noun === "next" ? "/me/player/next" : "/me/player/previous");
    } else if (noun === "volume") {
      const now = (MUSIC.now && MUSIC.now.device && MUSIC.now.device.volume_percent) || 0;
      const to = Math.max(0, Math.min(100, now + Number(id)));
      await spotify("PUT", `/me/player/volume?volume_percent=${to}`);
    } else if (noun === "device") {
      try { localStorage.setItem("endora.musicDevice", id); } catch (_) {}
      await spotify("PUT", "/me/player", { device_ids: [id], play: true });
    } else if (noun === "search") {
      const field = document.getElementById("music-q");
      MUSIC.query = field ? field.value.trim() : "";
      if (!MUSIC.query) { MUSIC.found = []; render(); return; }
      const r = await spotify("GET", `/search?q=${encodeURIComponent(MUSIC.query)}&type=track,album,artist,playlist&limit=5`) || {};
      MUSIC.found = [
        ...musicItems((r.tracks || {}).items, "track"), ...musicItems((r.albums || {}).items, "album"),
        ...musicItems((r.artists || {}).items, "artist"), ...musicItems((r.playlists || {}).items, "playlist"),
      ];
      render();
      return;
    } else if (noun === "play") {
      const x = (id || "").startsWith("f") ? MUSIC.found[Number(id.slice(1))] : MUSIC.lists[Number((id || "").slice(1))];
      if (!x) return;
      const device = musicDevice();
      if (!device) throw new Error("No device is open. Open Spotify on a phone, a speaker or the handheld");
      await spotify("PUT", `/me/player/play?device_id=${encodeURIComponent(device)}`,
        x.kind === "track" ? { uris: [x.uri] } : { context_uri: x.uri });
    }
    // Spotify takes a moment to say what it is doing now.
    await new Promise((r) => setTimeout(r, 400));
  } catch (e) { flash(e.message, "err"); }
  await loadMusic(); render();
}
