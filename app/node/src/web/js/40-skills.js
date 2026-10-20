// The house's tools (skills): what the team's turns may use, through the bridge (ADR 0108). A short list in four groups, each skill opening to
// its own screen. Where skills get their data (Home Assistant, the home lab, tool servers) is
// under Connections, and how far Endora may act on its own is under Permissions: all three
// were one page, about twelve phone screens long, with every setup form open on it.

// The connection a skill rides, if it borrows one (`borrows_from`).
function lenderOf(c) {
  return c && c.borrows_from ? (CAPS || []).find((x) => x.id === c.borrows_from) || null : null;
}

// Whether a connection is ready: an entry that only holds settings is ready when they are
// filled; Home Assistant when it is configured.
function connectionReady(x) {
  return x.settings_only ? x.settings_complete === true : x.configured === true;
}

// Where a skill stands, in one word, and the pill class that colours it.
function skillStatus(c) {
  if (c.settings_only) return c.settings_complete ? ["Set up", "concluded"] : ["Needs setup", ""];
  if (c.usable) return ["On", "concluded"];
  if (c.enabled === false) return ["Off", ""];
  const lender = lenderOf(c);
  if (lender && !connectionReady(lender)) return ["Needs a connection", ""];
  return ["Needs setup", ""];
}

// The groups the Skills list is shown in, by what a skill is for rather than how it is built.
const SKILL_GROUPS = [
  ["home", "Your home"],
  ["lookup", "Looking things up"],
  ["homelab", "Home lab"],
  ["endora", "About Endora"],
];

function skillGroup(c) {
  const lender = lenderOf(c);
  if (lender && lender.settings_only) return "homelab";
  if (c.category === "endora") return "endora";
  if (lender || ["presence", "travel", "safety"].includes(c.category)) return "home";
  return "lookup";
}

// An on/off row: a label, a line under it, and the switch.
function onOffRow(on, act, label, sub) {
  return `
    <div class="row" style="align-items:flex-start;gap:10px;margin-top:8px;">
      <div class="grow"><div class="title" style="font-weight:500;">${label}</div><div class="sub">${sub}</div></div>
      <button class="${on ? "primary" : "ghost"}" data-act="${act}:${on ? "0" : "1"}">${on ? "On" : "Off"}</button>
    </div>`;
}

// One skill in full: what it does, its switch, whether it asks first, and its settings.
function skillCard(c) {
    const ext = c.reaches_external;
    const enabled = c.enabled !== false;
    // The irreversible band (ADR 0051): blocked deny-by-default until the person
    // opens it, and even then confirmed on every use — never autonomous.
    const irreversible = c.reversibility === "irreversible";
    const opened = c.open_irreversible === true;
    // An entry that only holds settings (for the handheld, and chat skills that borrow them)
    // is set up or not by its settings; it has no on/off and never runs on its own.
    const holdsSettings = c.settings_only === true;
    const status = holdsSettings
      ? (c.settings_complete ? `<span class="pill concluded">Set up</span>` : `<span class="pill">Needs setup</span>`)
      : c.usable
        ? `<span class="pill concluded">On</span>`
        : (!enabled ? `<span class="pill">Off</span>` : `<span class="pill">Needs setup</span>`);
    // A settings form for skills that need configuring (a model, a key, a URL).
    const settingsForm = (c.settings && c.settings.length) ? `
      <div style="margin-top:8px;border-top:1px solid var(--line);padding-top:8px;">
        ${c.settings.map((s) => `
          <div class="form" style="margin-bottom:6px;align-items:center;">
            <label class="sub" style="min-width:130px;">${esc(s.label)}${s.set ? ` <span class="pill concluded">set</span>` : ""}</label>
            <input id="setting-${c.id}-${s.key}" type="${s.secret ? "password" : "text"}" placeholder="${s.set ? "•••••• (unchanged)" : (s.secret ? "enter a value" : "")}" value="${!s.set && s.key === "open_on_tap" ? esc(location.origin) : ""}" autocomplete="off" />
          </div>`).join("")}
        <div class="row" style="gap:8px;">
          <button class="primary" data-act="skillcfg:${c.id}">Save settings</button>
          <button class="ghost" data-act="skilltest:${c.id}" title="check these settings actually work">${icon("check", 14)} Test</button>
        </div>
      </div>` : "";
    return `
      <div class="card">
        <div class="row">
          <div class="grow">
            <div class="title">${esc(c.name)} ${status}${ext ? ` <span class="pill">uses the internet</span>` : ""}${irreversible ? ` <span class="pill">can't be undone</span>` : ""}</div>
            <div class="sub">${esc(c.summary || c.description)}</div>
            ${(enabled && !c.configured && !holdsSettings && !lenderOf(c)) ? `<div class="sub" style="margin-top:4px;">Needs: ${esc(c.needs)}</div>` : ""}
          </div>
          ${holdsSettings ? "" : `<button class="ghost" data-act="skill:enable:${c.id}:${enabled ? "0" : "1"}">${enabled ? "Turn off" : "Turn on"}</button>`}
        </div>
        ${irreversible ? `
        <div class="row" style="align-items:flex-start;gap:10px;margin-top:8px;border-top:1px solid var(--line);padding-top:8px;">
          <div class="grow">
            <div class="title" style="font-weight:500;">Actions that can't be undone</div>
            <div class="sub">${opened
              ? "Allowed — Endora still asks before every use, never on its own."
              : "Spending, sending or deleting — blocked until you allow it, and it always asks first."}</div>
          </div>
          <button class="${opened ? "primary" : "ghost"}" data-act="skill:open:${c.id}:${opened ? "0" : "1"}">${opened ? "Block again" : "Allow (with confirmation)"}</button>
        </div>` : (enabled && !holdsSettings ? `
        <div class="row" style="gap:8px;margin-top:6px;">
          <button class="ghost" style="font-size:12px;padding:2px 8px;" data-act="skill:confirm:${c.id}:${c.confirm ? "0" : "1"}"
            title="${c.confirm ? "Endora asks before every use. Click to let it decide." : "Endora may use this on its own. Click to be asked each time."}">${c.confirm ? "Asks first" : "Runs without asking"}</button>
        </div>` : "")}
        ${settingsForm}
      </div>`;
  }

function viewSkills() {
  const skills = (CAPS || []).filter((c) => !c.settings_only);
  const row = (c) => {
    const [label, cls] = skillStatus(c);
    return `
      <button class="ghost" data-act="go:skill:${esc(c.id)}">
        <span class="grow" style="text-align:left;">${esc(c.name)}<span class="sub one-line">${esc(c.summary || c.description)}</span></span>
        <span class="pill ${cls}">${esc(label)}</span>${icon("chevron", 15)}</button>`;
  };
  const groups = SKILL_GROUPS.map(([key, title]) => {
    const mine = skills.filter((c) => skillGroup(c) === key);
    return mine.length
      ? `<div class="group-title">${esc(title)}</div><div class="card nav-list">${mine.map(row).join("")}</div>`
      : "";
  }).join("");
  const connections = (CAPS || []).filter((c) => c.settings_only || c.id === "home_assistant");
  const ready = connections.filter(connectionReady).length;
  const nav = (act, ic, label, note) => `
    <button class="ghost" data-act="${act}">${icon(ic)}
      <span class="grow" style="text-align:left;">${label}<span class="sub" style="display:block;font-weight:400;">${note}</span></span>
      ${icon("chevron", 15)}</button>`;
  return `
    ${settingsCrumbs("skills")}
    <h2>Tools</h2>
    <div class="note">What the team's turns may use from the house. Tap one to turn it on or off, choose whether it asks you first, or set it up.</div>
    ${groups || `<div class="empty">No tools registered.</div>`}
    <div class="group-title">Also</div>
    <div class="card nav-list">
      ${nav("go:connections", "gear", "Connections", `where the tools get their data — ${ready} of ${connections.length} set up`)}
    </div>`;
}

// One skill's own screen, or one connection's: a home-lab service opens here too, under
// Connections rather than Skills.
function viewSkill() {
  const c = (CAPS || []).find((x) => x.id === NAV.id);
  const parent = c && c.settings_only
    ? { label: "Connections", act: "go:connections" }
    : { label: "Skills", act: "go:skills" };
  const trail = crumbs([
    { label: "HQ", act: "go:hq" },
    { label: "Settings", act: "go:settings" },
    parent,
    { label: c ? c.name : "Skill" },
  ]);
  if (!c) return `${trail}<div class="empty">That skill isn't here any more.</div>`;
  const lender = lenderOf(c);
  const uses = lender ? `
    <div class="card nav-list" style="margin-top:14px;">
      <button class="ghost" data-act="go:connections">${icon("gear")}
        <span class="grow" style="text-align:left;">Uses your ${esc(lender.name)} connection<span class="sub" style="display:block;font-weight:400;">${connectionReady(lender) ? "set up" : "not set up yet — set it up under Connections"}</span></span>
        ${icon("chevron", 15)}</button>
    </div>` : "";
  return `
    ${trail}
    <h2>${esc(c.name)}</h2>
    ${skillCard(c)}
    ${uses}`;
}

// Where skills get their data, each entered once: Home Assistant, the home lab's services,
// and tool servers. A skill that uses one says so on its own screen.
function viewConnections() {
  const ha = (CAPS || []).find((c) => c.id === "home_assistant");
  const lab = (CAPS || []).filter((c) => c.settings_only);
  const labRow = (c) => {
    const [label, cls] = skillStatus(c);
    return `
      <button class="ghost" data-act="go:skill:${esc(c.id)}">
        <span class="grow" style="text-align:left;">${esc(c.name)}<span class="sub one-line">${esc(c.summary || c.description)}</span></span>
        <span class="pill ${cls}">${esc(label)}</span>${icon("chevron", 15)}</button>`;
  };
  const haRow = ha ? `
    <div class="card nav-list">
      <button class="ghost" data-act="go:skill:home_assistant">
        <span class="grow" style="text-align:left;">Home Assistant<span class="sub" style="display:block;font-weight:400;">its address and token, what Endora may write back, and how it reaches you</span></span>
        <span class="pill ${ha.configured ? "concluded" : ""}">${ha.configured ? "Connected" : "Not connected"}</span>${icon("chevron", 15)}</button>
    </div>` : "";
  return `
    ${settingsCrumbs("connections")}
    <h2>Connections</h2>
    <div class="note">Where Endora's skills get their data. Each is entered once; the skills that use it work as soon as it is set up.</div>
    <h3>Home Assistant</h3>
    ${haRow}
    ${viewConnect()}
    <h3>Accounts</h3>
    <div class="card nav-list">
      <button class="ghost" data-act="go:spotify">
        <span class="grow" style="text-align:left;">Spotify<span class="sub one-line">one sign-in for the web console and the handheld</span></span>
        <span class="pill ${SPOTIFY && SPOTIFY.state === "connected" ? "concluded" : ""}">${esc(SPOTIFY ? ({ connected: "Connected", waiting: "Waiting" }[SPOTIFY.state] || "Not connected") : "…")}</span>${icon("chevron", 15)}</button>
    </div>
    <h3>Home lab</h3>
    <div class="note">Services on your network. The handheld uses them, and the team reads the lab through them.</div>
    ${lab.length ? `<div class="card nav-list">${lab.map(labRow).join("")}</div>` : `<div class="empty">No home-lab services are offered by this node.</div>`}
    ${mcpSection()}`;
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
      <div class="sub" style="margin:-2px 0 4px;">Endora uses it to check what an action actually did, and the team's bridge treats it as a read.</div>
      <select data-reader-for="${esc(s.name)}" onchange="setReader(this)" style="width:100%;">${opts}</select>
    </div>`;
}

// Tool servers (ADR 0054), on Connections: one row each, opening to its own screen.

// One tool server in full: where it is, its tools and what each may do.
function toolServerCard(s) {
    const addr = s.transport === "http" ? s.url : `${s.command} ${(s.args || []).join(" ")}`.trim();
    const health = s.tools_live > 0
      ? `<span class="pill concluded">${s.tools_live} tool${s.tools_live === 1 ? "" : "s"}</span>`
      : `<span class="pill">not connected</span>`;
    // A withdrawn tool (ADR 0054) is not offered to the butler at all — a different
    // thing from blocked, which still shows it the tool and refuses each use. Shown
    // here so it is never a mystery why a tool stopped being used, with one click back.
    const toolRow = (t) => `
      <div class="row" style="align-items:flex-start;gap:10px;margin-top:6px;border-top:1px solid var(--line);padding-top:6px;${t.enabled === false ? "opacity:.6;" : ""}">
        <div class="grow">
          <div class="title" style="font-weight:500;">${esc(t.id)}${t.enabled === false ? ` <span class="pill">turned off</span>` : ""}</div>
          <div class="sub">${esc(t.description || "")}</div>
          <div class="sub">${t.enabled === false
            ? "Turned off — it is not offered at all."
            : t.opened
              ? "Allowed — asks before every use."
              : "Blocked — it is listed but cannot be used."}</div>
        </div>
        ${t.enabled === false
          ? `<button class="ghost" data-act="skill:enable:${t.id}:1">Offer it again</button>`
          : `<button class="${t.opened ? "primary" : "ghost"}" data-act="skill:open:${t.id}:${t.opened ? "0" : "1"}">${t.opened ? "Block" : "Allow"}</button>`}
      </div>`;
    // Stacked, not side by side. Four buttons beside a name is a row on a laptop and a
    // disaster on a phone: the actions took the width they needed and left the name a column
    // about eight characters wide, so "brave-search" broke across two lines, the pills came
    // out as "8" above "tools", and the launch command rendered one word per line. Adding a
    // fifth button made it worse.
    //
    // Second time for this exact shape — the reader dropdown did it to its own label, for the
    // same reason, and the fix was the same: full width, one thing under another. A `.row` is
    // only safe when nothing in it needs room to breathe.
    return `
      <div class="card">
        <div>
          <div>
            <div class="title">${esc(s.name)} ${health} <span class="pill">${esc(s.transport)}</span>${s.auth_set ? ` <span class="pill concluded">token set</span>` : ""}${(s.env_keys || []).length ? ` <span class="pill concluded">${(s.env_keys || []).length} env</span>` : ""}</div>
            <div class="sub" style="overflow-wrap:anywhere;">${esc(addr)}</div>
            ${s.tools_live === 0 ? `<div class="sub" style="margin-top:4px;color:var(--danger,#c33);"><strong>Connected to nothing — no tools.</strong> Endora can't use this server. Most often that's a missing or wrong environment variable rather than a broken server${(s.env_keys || []).length ? ` — it has ${(s.env_keys || []).map(esc).join(", ")} set` : " — it has none set"}. ${s.transport === "stdio" ? "Some servers also need a one-time setup with a browser before they can run headless; check their docs." : "Check the endpoint is reachable."} Retrying every couple of minutes.</div>` : ""}
          </div>
          <div class="row" style="gap:6px;flex-wrap:wrap;margin-top:8px;">
            <button class="ghost" data-act="mcp:edit:${esc(s.name)}" title="Load this server into the form below to change its URL or settings">Edit</button>
            <button class="ghost" data-act="mcp:test:${esc(s.name)}" title="Call its state-reading tool for real, so a wrong key says so instead of going quiet">Test</button>
            <button class="ghost" data-act="mcp:reconnect:${esc(s.name)}" title="Retry the connection using its saved settings">Reconnect</button>
            <button class="ghost danger" data-act="mcp:remove:${esc(s.name)}">Remove</button>
          </div>
        </div>
        ${onOffRow(s.trust_all, `mcp:trust:${esc(s.name)}`, "Allow all its tools", "Opens every tool this server exposes, so you don't allow them one by one — and if you have widened autonomy, an opened tool can act without asking. Turning this off closes them all again; reopen the ones you want.")}
        ${readerRow(s)}
        ${(s.tools || []).length
          ? `<details class="steps" style="margin-top:6px;"><summary>${(s.tools || []).length} tool${(s.tools || []).length === 1 ? "" : "s"}</summary>
               ${(s.tools || []).length > 1 ? `
                 <div class="row" style="gap:8px;align-items:center;margin:6px 0 2px;">
                   <div class="grow sub">${(s.tools || []).filter((t) => t.opened).length} of ${(s.tools || []).length} allowed</div>
                   <button class="ghost" data-act="mcp:tools:${esc(s.name)}:1">Allow all</button>
                   <button class="ghost" data-act="mcp:tools:${esc(s.name)}:0">Block all</button>
                 </div>` : ""}
               <div class="step-list">${(s.tools || []).map(toolRow).join("")}</div></details>`
          : ""}
      </div>`;
}

// Finding a server and adding one by hand, on Connections; on a server's own screen only the
// form, which its Edit fills.
function toolServerForms(withSearch = true) {
  // Browse/search the catalog: curated entries + (best effort) the community
  // registry. Picking one prefills the form below — everything stays editable, so a
  // stale launch command can be corrected before it's registered.
  const mcpBrowse = `
    <div class="card">
      <div class="title">Find a server</div>
      <div class="sub" style="margin:4px 0 8px;">Search well-known servers and the community registry. Choosing one fills in the form below — you can edit anything before adding it.</div>
      <div class="row" style="gap:8px;">
        <input id="mcp-search" placeholder="e.g. files, github, home assistant" style="flex:1;" />
        <button class="ghost" data-act="mcp:search">${icon("sparkle", 14)} Search</button>
      </div>
      <div id="mcp-catalog-results"></div>
    </div>`;
  // Folded, because most people arrive at this screen to see what Endora can do — not to
  // hand-configure a server. Browsing the catalogue above fills this in for you, so typing
  // it out is the fallback rather than the front door.
  const mcpAddForm = `
    <details class="adv">
      <summary>Add a server by hand</summary>
    <div class="card">
      <div class="sub" style="margin:4px 0 8px;">A local command (stdio) or a networked endpoint (e.g. a Docker MCP Gateway). Its tools appear above, blocked until you allow each one.</div>
      <div class="field"><label>Name</label><input id="mcp-name" placeholder="e.g. filesystem" /></div>
      <div class="field"><label>Connection</label>
        <select id="mcp-transport" onchange="mcpTransportChange(this.value)">
          <option value="stdio">Local command (stdio)</option>
          <option value="http">HTTP endpoint</option>
        </select></div>
      <div id="mcp-stdio-fields">
        <div class="field"><label>Command</label><input id="mcp-command" placeholder="e.g. npx" /></div>
        <div class="field"><label>Arguments <span class="sub" style="font-weight:400;">· one per line</span></label>
          <textarea id="mcp-args" rows="3" placeholder="-y&#10;@modelcontextprotocol/server-filesystem&#10;/data"></textarea></div>
        <div id="mcp-needs"></div>
        <details class="steps" style="margin-top:6px;">
          <summary>Advanced · other environment variables</summary>
          <div class="field" style="margin-top:6px;">
            <label>KEY=value, one per line</label>
            <textarea id="mcp-env" rows="2" placeholder="TOKEN=… (only if this server needs one)"></textarea></div>
        </details>
      </div>
      <div id="mcp-http-fields" style="display:none;">
        <div class="field"><label>Endpoint URL</label><input id="mcp-url" placeholder="http://mcp-gateway:8080/" /></div>
        <div class="field"><label>Access token <span class="sub" style="font-weight:400;">· optional, sent as a bearer token</span></label>
          <input id="mcp-auth" type="password" autocomplete="off" placeholder="stored securely, never shown" /></div>
      </div>
      <label class="row" style="gap:8px;align-items:center;margin-top:4px;">
        <input type="checkbox" id="mcp-trust" />
        <span class="sub">Allow all its tools at once. Off by default — each tool stays blocked until you allow it, and if you have widened autonomy an allowed tool can act without asking.</span>
      </label>
      <div class="row" style="justify-content:flex-end;"><button class="primary" data-act="mcp:add">Save server</button></div>
    </div>
    </details>`;
  return withSearch ? `${mcpBrowse}${mcpAddForm}` : mcpAddForm;
}

function mcpSection() {
  const servers = (MCP_SERVERS && MCP_SERVERS.servers) || [];
  const row = (s) => {
    const allowed = (s.tools || []).filter((t) => t.opened).length;
    const pill = s.tools_live > 0
      ? `<span class="pill concluded">${s.tools_live} tool${s.tools_live === 1 ? "" : "s"} · ${allowed} allowed</span>`
      : `<span class="pill">not connected</span>`;
    return `
      <button class="ghost" data-act="go:toolserver:${esc(s.name)}">
        <span class="grow" style="text-align:left;">${esc(s.name)}<span class="sub one-line">${esc(s.transport === "http" ? s.url : s.command || "")}</span></span>
        ${pill}${icon("chevron", 15)}</button>`;
  };
  return `
    <h3>Tool servers <span class="sub" style="font-weight:400;">· MCP</span></h3>
    <div class="note">Tools from a tool server are off-limits by default: Endora can see them, but each stays blocked until you allow it. Whether an allowed tool then asks first depends on Permissions.</div>
    ${servers.length ? `<div class="card nav-list">${servers.map(row).join("")}</div>` : ""}
    <details class="adv"><summary>Add a tool server</summary>${toolServerForms()}</details>`;
}

// One tool server's own screen.
function viewToolServer() {
  const s = ((MCP_SERVERS && MCP_SERVERS.servers) || []).find((x) => x.name === NAV.id);
  const trail = crumbs([
    { label: "HQ", act: "go:hq" },
    { label: "Settings", act: "go:settings" },
    { label: "Connections", act: "go:connections" },
    { label: s ? s.name : "Tool server" },
  ]);
  if (!s) return `${trail}<div class="empty">That tool server isn't here any more.</div>`;
  return `
    ${trail}
    <h2>${esc(s.name)}</h2>
    ${toolServerCard(s)}
    ${toolServerForms(false)}`;
}

// Spotify, held by the node (plan phase 4). The same sign-in for every device: one started on
// the handheld shows here as waiting and can be finished here, and the other way round.
function viewSpotify() {
  const trail = crumbs([
    { label: "HQ", act: "go:hq" },
    { label: "Settings", act: "go:settings" },
    { label: "Connections", act: "go:connections" },
    { label: "Spotify" },
  ]);
  const s = SPOTIFY;
  if (!s) return `${trail}<h2>Spotify</h2><div class="note">Asking Endora…</div>`;
  const callback = `${location.origin}/connect/spotify/callback`;
  const setup = `
    <details class="adv"><summary>Your Spotify app</summary>
      <div class="note" style="margin-top:8px;">Endora signs in with your own app from Spotify's developer dashboard. Add both of these as its redirect addresses, exactly:</div>
      <div class="card"><div class="sub mono" style="overflow-wrap:anywhere;">${esc(callback)}</div><div class="sub mono">http://127.0.0.1:8888/callback</div></div>
    </details>`;
  if (s.state === "connected") {
    return `${trail}<h2>Spotify</h2>
      <div class="card"><div class="title">${esc(s.detail)} <span class="pill concluded">Connected</span></div>
        <div class="sub">The web console and the handheld both use this one sign-in; it stays on Endora.</div>
        <div class="sub">Connect again when Endora says its sign-in doesn't allow something: you approve on Spotify and come back, and this sign-in keeps working until then.</div>
        <div class="row" style="justify-content:flex-end;gap:8px;margin-top:8px;"><button class="ghost danger" data-act="spotify:disconnect">Disconnect</button><button class="primary" data-act="spotify:begin:endora">Connect again</button></div>
      </div>${setup}`;
  }
  if (s.state === "waiting") {
    const started = s.started_ms ? new Date(s.started_ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : "";
    const pasted = (s.redirect || "").startsWith("http://127.0.0.1");
    return `${trail}<h2>Spotify</h2>
      <div class="card"><div class="title">${esc(s.detail)} <span class="pill">Waiting</span></div>
        <div class="sub">Started ${esc(started)}, on this or another of your devices. Open the address, approve Endora, and ${pasted ? "paste the address Spotify sends you to (it will not load; that is expected) below" : "you will come back here by yourself"}.${s.connected ? " Until then, the sign-in Endora has keeps working." : ""}</div>
        <div class="row" style="gap:8px;margin-top:8px;"><a class="ghost" href="${esc(s.sign_in_url || "#")}" rel="noreferrer">Open Spotify to approve</a>
          <button class="ghost" data-act="spotify:check">Check again</button></div>
        <div class="form" style="margin-top:8px;">
          <input id="spotify-returned" placeholder="http://127.0.0.1:8888/callback?code=…" autocomplete="off" />
          <button class="primary" data-act="spotify:finish">Finish</button>
        </div>
        <div class="row" style="justify-content:flex-end;"><button class="ghost" data-act="spotify:begin:paste">Start over</button></div>
      </div>${setup}`;
  }
  return `${trail}<h2>Spotify</h2>
    <div class="card"><div class="title">Not connected</div>
      <div class="sub">Connect once here or on the handheld; both use it.</div>
      ${s.client_id_set ? "" : `<div class="form" style="margin-top:8px;"><input id="spotify-client" placeholder="your Spotify app's client id" autocomplete="off" /></div>`}
      <div class="row" style="gap:8px;flex-wrap:wrap;margin-top:8px;">
        <button class="primary" data-act="spotify:begin:endora">Approve on this device</button>
        <button class="ghost" data-act="spotify:begin:paste">Approve on another device</button>
      </div>
    </div>${setup}`;
}

