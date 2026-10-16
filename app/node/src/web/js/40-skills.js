// The butler's skills: what chat can do. A short list in four groups, each skill opening to
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
  const recipes = (RECIPES || []).length;
  const nav = (act, ic, label, note) => `
    <button class="ghost" data-act="${act}">${icon(ic)}
      <span class="grow" style="text-align:left;">${label}<span class="sub" style="display:block;font-weight:400;">${note}</span></span>
      ${icon("chevron", 15)}</button>`;
  return `
    ${settingsCrumbs("skills")}
    <h2>Skills</h2>
    <div class="note">What Endora can do in chat. Tap one to turn it on or off, choose whether it asks first, or set it up.</div>
    ${groups || `<div class="empty">No skills registered.</div>`}
    <div class="group-title">Skills you teach it</div>
    <div class="card nav-list">
      ${nav("go:recipes", "plus", "Recipes", recipes ? `${recipes} taught so far` : "read one web address and say one sentence about it — no code")}
    </div>
    <div class="group-title">Also</div>
    <div class="card nav-list">
      ${nav("go:connections", "gear", "Connections", `where skills get their data — ${ready} of ${connections.length} set up`)}
      ${nav("go:permissions", "scale", "Permissions", "how far Endora may act on its own")}
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
    { label: "Home", act: "go:chat" },
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
        <span class="grow" style="text-align:left;">Spotify<span class="sub one-line">one sign-in for the web console, the handheld and chat</span></span>
        <span class="pill ${SPOTIFY && SPOTIFY.state === "connected" ? "concluded" : ""}">${esc(SPOTIFY ? ({ connected: "Connected", waiting: "Waiting" }[SPOTIFY.state] || "Not connected") : "…")}</span>${icon("chevron", 15)}</button>
    </div>
    <h3>Home lab</h3>
    <div class="note">Services on your network. The handheld uses them, and chat reads from the ones its skills name.</div>
    ${lab.length ? `<div class="card nav-list">${lab.map(labRow).join("")}</div>` : `<div class="empty">No home-lab services are offered by this node.</div>`}
    ${mcpSection()}`;
}

// How far Endora may go on its own. Per-skill "asks first" stays on each skill's screen, and a
// tool server's tools on Connections; this is the boundary around all of them.
function viewPermissions() {
  const env = AUTONOMY || {};
  const envelope = `
    <div class="card">
      <div class="title">How independently should Endora act?</div>
      <div class="sub" style="margin:4px 0 2px;">Endora acts on its own inside the boundary you set here, and asks you at its edges. You can widen or narrow it any time.</div>
      ${onOffRow(env.auto_external !== false, "autonomy:external", "Use read-only skills on its own", "Check weather, news, safety alerts and web pages without asking. (Recommended)")}
      ${onOffRow(env.auto_consequential === true, "autonomy:consequential", "Take consequential actions on its own", "Let it carry out actions that normally need your confirmation — spending, sending, or things that can't be undone. Off is safer; turn on only if you're sure.")}
    </div>`;
  return `
    ${settingsCrumbs("permissions")}
    <h2>Permissions</h2>
    <div class="note">How far Endora may go on its own. Each skill can also be set to ask first, on its own screen under Skills; a tool server's tools stay blocked until you allow each one, under Connections.</div>
    ${envelope}`;
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
            ? "Turned off — the butler isn't offered this tool at all."
            : t.opened
              ? (AUTONOMY.auto_consequential
                  ? "Allowed — and because you let consequential skills act on their own, it runs without asking."
                  : "Allowed — asks before every use.")
              : "Blocked — the butler can see it but cannot use it."}</div>
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
    { label: "Home", act: "go:chat" },
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

// One recipe: what it does, what it needs, and — folded, since it's the
// mechanism rather than the point — exactly what it fetches and says.
function recipeCard(r) {
  const needs = (r.inputs || []).map((i) => `${i.name} (${i.type})`).join(", ") || "nothing extra";
  return `
    <div class="card">
      <div class="row">
        <div class="grow">
          <div class="title">${esc(r.description)}</div>
          <div class="sub">${esc(r.id)} · needs: ${esc(needs)}</div>
        </div>
        <span class="pill ${r.enabled ? "concluded" : ""}">${r.enabled ? "On" : "Off"}</span>
      </div>
      <details class="steps" style="margin-top:6px;">
        <summary>How it works</summary>
        <div class="sub" style="word-break:break-all;margin-top:4px;">Fetches: ${esc(r.get)}</div>
        <div class="sub" style="margin-top:4px;">Says: "${esc(r.say)}"</div>
      </details>
      <div class="row" style="gap:6px;flex-wrap:wrap;margin-top:8px;">
        <button class="ghost" data-act="recipe:enable:${esc(r.id)}:${r.enabled ? "0" : "1"}">${r.enabled ? "Turn off" : "Turn on"}</button>
        <button class="ghost danger" data-act="recipe:delete:${esc(r.id)}">Delete</button>
      </div>
    </div>`;
}

// The `name:type` lines of the "what it needs" box, as objects. One parser, used
// by the sample-value fields, by Try, and by Save — three readings of the same
// text would drift.
// Ready-made recipes, to answer the blank page. Every one of these was run
// against its real API before shipping — a starter that does not work is worse
// than no starter, because it teaches the person the feature is broken.
//
// **None of them declare inputs**, and that is deliberate rather than lazy: an
// input has to be supplied by the model at ask time, and asked "what's the air
// quality?" a small local model does not know anybody's coordinates and will
// invent some. A constant in the address is a number the person edits once and
// is then simply right, forever. Inputs remain for the case they are actually
// for — something that genuinely varies per question.
//
// The coordinates below are a public example (New York), not anybody's home;
// the person swaps them for theirs. Same rule as every other fixture here.
const RECIPE_STARTERS = [
  {
    id: "air_quality",
    description: "Today's air quality.",
    get: "https://air-quality-api.open-meteo.com/v1/air-quality?latitude=40.71&longitude=-74.01&current=us_aqi",
    say: "The air quality index is {current.us_aqi} right now.",
    note: "swap the latitude and longitude for yours",
  },
  {
    id: "uv_index",
    description: "How strong the sun is right now.",
    get: "https://api.open-meteo.com/v1/forecast?latitude=40.71&longitude=-74.01&current=uv_index",
    say: "The UV index is {current.uv_index} right now.",
    note: "swap the latitude and longitude for yours",
  },
  {
    id: "sunset",
    description: "When the sun sets today.",
    get: "https://api.sunrise-sunset.org/json?lat=40.71&lng=-74.01&formatted=0",
    say: "The sun sets at {results.sunset}.",
    note: "swap the lat and lng for yours",
  },
  {
    id: "dollar_in_euro",
    description: "What a dollar is worth in euro.",
    get: "https://api.frankfurter.app/latest?from=USD&to=EUR",
    say: "One US dollar is {rates.EUR} euro.",
    note: "change USD, EUR and the sentence together — the field name follows the currency",
  },
];

function recipeInputsFrom(text) {
  return (text || "").split("\n").map((l) => l.trim()).filter(Boolean).map((line) => {
    const [name, type] = line.split(":").map((s) => (s || "").trim());
    return { name, type };
  });
}

// Regenerates a sample-value box per declared input, so Try has something to fill
// the address with. Called from the textarea's own `oninput` — it rewrites one
// div and nothing else, because a re-render here would wipe the half-typed form
// around it.
function recipeSamplesChanged(text) {
  const box = document.getElementById("recipe-samples");
  if (!box) return;
  const inputs = recipeInputsFrom(text).filter((i) => i.name);
  box.innerHTML = inputs.length
    ? `<div class="sub" style="margin:8px 0 4px;">Sample values, so you can try it:</div>` +
      inputs.map((i) => `
        <div class="field"><label>${esc(i.name)} <span class="sub" style="font-weight:400;">· ${esc(i.type || "string")}</span></label>
          <input data-sample="${esc(i.name)}" data-sample-type="${esc(i.type || "string")}" placeholder="a value to test with" /></div>`).join("")
    : "";
}

// What a trial found: the address it really fetched, the fields it can read
// (tap one to use it), and the sentence as it reads. This is the whole point of
// the authoring loop — nobody should be guessing a JSON path against an API
// they cannot see.
function recipeTrialHtml(r) {
  if (!r) return "";
  if (!r.ok) {
    return `<div class="msg show err" style="margin-top:8px;">${esc(r.error || "that didn't work")}</div>`;
  }
  const chips = (r.fields || []).map((f) => `
    <button class="pill pick" data-act="recipe:pick:${esc(f.path)}" title="use this field"
      style="margin:2px 4px 2px 0;">${esc(f.path)} <span class="sub">${esc(String(f.sample).slice(0, 24))}</span></button>`).join("");
  const preview = r.preview
    ? `<div class="card" style="margin-top:8px;"><div class="sub">It would say:</div>
         <div class="title" style="font-weight:500;">${esc(r.preview)}</div></div>`
    : `<div class="msg show err" style="margin-top:8px;">${esc(r.preview_error || "the sentence didn't resolve")}</div>`;
  return `
    <div style="margin-top:10px;">
      <div class="sub" style="word-break:break-all;">Fetched: ${esc(r.url)}</div>
      ${chips ? `<div class="sub" style="margin:8px 0 4px;">Tap a field to put it in your sentence:</div><div>${chips}</div>` : ""}
      ${r.skipped_a_list ? `<div class="sub" style="margin-top:6px;">This answer also had a list in it. A recipe can only read plain fields, so those aren't offered.</div>` : ""}
      ${preview}
    </div>`;
}

// Recipes: capabilities the person teaches Endora themselves — five fields, no
// code (ADR 0071). The whole screen is a list plus one folded form, because the
// form is a rarer action than reading what already exists — the same call the
// MCP "add a server by hand" form makes.
function viewRecipes() {
  const recipes = RECIPES || [];
  const addForm = `
    <details class="adv">
      <summary>Teach it a new one</summary>
      <div class="card">
        <div class="sub" style="margin:4px 0 8px;">Five things: a short name, what it does, what it needs from you, a web address to fetch (with those things filled in), and the one sentence to read back from what comes back. That's the whole capability — it can only read, never act.</div>
        <div class="sub" style="margin:8px 0 4px;">Start from one of these, then edit it:</div>
        <div style="margin-bottom:10px;">
          ${RECIPE_STARTERS.map((s, n) => `
            <button class="pill pick" data-act="recipe:start:${n}" style="margin:2px 4px 2px 0;"
              title="${esc(s.get)}">${esc(s.description)}</button>`).join("")}
        </div>
        <div class="field"><label>Short name</label><input id="recipe-id" placeholder="e.g. air_quality — lowercase, no spaces" /></div>
        <div class="field"><label>What it does</label><input id="recipe-description" placeholder="e.g. Today's air quality where you are." /></div>
        <div class="field"><label>What it needs <span class="sub" style="font-weight:400;">· one per line, name:type — type is "string" or "number"</span></label>
          <textarea id="recipe-inputs" rows="2" placeholder="lat:number&#10;lon:number" oninput="recipeSamplesChanged(this.value)"></textarea></div>
        <div id="recipe-samples"></div>
        <div class="field"><label>Web address to fetch <span class="sub" style="font-weight:400;">· use {name} for each thing above</span></label>
          <input id="recipe-get" placeholder="https://example.com/api?lat={lat}&lon={lon}" /></div>
        <div class="row" style="justify-content:flex-start;">
          <button class="ghost" data-act="recipe:try">Try it →</button>
          <span class="sub">fetch it once and see what comes back</span>
        </div>
        <div id="recipe-trial"></div>
        <div class="field" style="margin-top:10px;"><label>What to say <span class="sub" style="font-weight:400;">· tap a field above, or type {path.into.the.answer}</span></label>
          <input id="recipe-say" placeholder="The air quality index is {current.us_aqi} right now." /></div>
        <div class="row" style="justify-content:flex-end;"><button class="primary" data-act="recipe:add">Save recipe</button></div>
      </div>
    </details>`;
  return `
    ${crumbs([
      { label: "Home", act: "go:chat" },
      { label: "Settings", act: "go:settings" },
      { label: "Skills", act: "go:skills" },
      { label: "Recipes" },
    ])}
    <h2>Recipes</h2>
    <div class="note">A recipe is a capability you teach Endora yourself — no code. It can only read one web address and say one sentence about it; it can't act on anything. A new one stays off until you turn it on.</div>
    ${listOr(recipes.map(recipeCard), "No recipes yet — teach it one below.")}
    ${addForm}`;
}

// The home surface: what Endora currently understands about you. Not a task list —
// beliefs it has formed (with the evidence and how sure it is), which you can
// affirm or correct. This is the point of the product (ADR 0052).
const BELIEF_KIND_LABEL = {
  intent: "What you're really after", value: "What you value", preference: "Preferences",
  pattern: "Patterns", motivation: "What drives you", frustration: "Frustrations",
  stressor: "Stressors", relationship: "People who matter", other: "Other",
};
const BELIEF_KIND_ORDER = ["intent","value","motivation","pattern","preference","frustration","stressor","relationship","other"];

// Spotify, held by the node (plan phase 4). The same sign-in for every device: one started on
// the handheld shows here as waiting and can be finished here, and the other way round.
function viewSpotify() {
  const trail = crumbs([
    { label: "Home", act: "go:chat" },
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
        <div class="sub">The web console, the handheld and chat all use this one sign-in; it stays on Endora.</div>
        <div class="row" style="justify-content:flex-end;margin-top:8px;"><button class="ghost danger" data-act="spotify:disconnect">Disconnect</button></div>
      </div>${setup}`;
  }
  if (s.state === "waiting") {
    const started = s.started_ms ? new Date(s.started_ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : "";
    const pasted = (s.redirect || "").startsWith("http://127.0.0.1");
    return `${trail}<h2>Spotify</h2>
      <div class="card"><div class="title">${esc(s.detail)} <span class="pill">Waiting</span></div>
        <div class="sub">Started ${esc(started)}, on this or another of your devices. Open the address, approve Endora, and ${pasted ? "paste the address Spotify sends you to (it will not load; that is expected) below" : "you will come back here by yourself"}.</div>
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
      <div class="sub">Connect once here or on the handheld; both use it, and chat can say what is playing and play things.</div>
      ${s.client_id_set ? "" : `<div class="form" style="margin-top:8px;"><input id="spotify-client" placeholder="your Spotify app's client id" autocomplete="off" /></div>`}
      <div class="row" style="gap:8px;flex-wrap:wrap;margin-top:8px;">
        <button class="primary" data-act="spotify:begin:endora">Approve on this device</button>
        <button class="ghost" data-act="spotify:begin:paste">Approve on another device</button>
      </div>
    </div>${setup}`;
}

