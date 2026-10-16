// The bullet lines of one "## " section of a page, as the brain writes them.
function mdSection(text, heading) {
  const lines = String(text || "").split("\n");
  const at = lines.findIndex((l) => l.trim() === `## ${heading}`);
  if (at < 0) return [];
  const out = [];
  for (const line of lines.slice(at + 1)) {
    if (line.startsWith("## ")) break;
    const t = line.trim();
    if (t.startsWith("- ")) out.push(t.slice(2).trim());
  }
  return out;
}

// About you is the brain (ADR 0101): what you told it, what it knows best, and the pages of
// everything else, each one tap away. Nothing is confirmed card by card: a page is read, and
// edited as text when it is wrong.
function viewUnderstanding() {
  const knowsLocation = true;
  const knowsAddress = (DB.preferences || []).some((p) => /^\s*address me as\b/i.test(p.text));
  const locationSetup = knowsLocation ? "" : `
    <div class="card" style="border-color: color-mix(in srgb, var(--accent) 40%, var(--line));">
      <div class="title" style="margin-bottom:8px;">${icon("target", 15)} Where are you based?</div>
      <div class="form">
        <input id="setup-location" placeholder="a city, e.g. Boston, MA" />
        <button class="primary" data-act="setlocation">Save</button>
      </div>
    </div>`;
  const addressSetup = knowsAddress ? "" : `
    <div class="card" style="border-color: color-mix(in srgb, var(--accent) 40%, var(--line));">
      <div class="title" style="margin-bottom:8px;">${icon("sparkle", 15)} What should Endora call you?</div>
      <div class="form">
        <input id="setup-address" placeholder="a name, or sir / ma'am" />
        <button class="primary" data-act="setaddress">Save</button>
      </div>
    </div>`;
  const brain = BRAIN || { memory: "", pages: [] };
  const knows = mdSection(brain.memory, "What I know best");
  const knowsRows = knows.map((line) => `<div class="card"><div class="title" style="font-weight:400;">${esc(line)}</div></div>`);
  const folders = {};
  for (const page of brain.pages || []) {
    const folder = page.path.includes("/") ? page.path.slice(0, page.path.lastIndexOf("/")) : "";
    (folders[folder] = folders[folder] || []).push(page);
  }
  const pageList = Object.keys(folders).sort().map((folder) => `
    ${folder ? `<div class="sub" style="margin:12px 0 4px;text-transform:capitalize;">${esc(folder.replace(/-/g, " "))}</div>` : ""}
    ${folders[folder].map((p) => `
      <div class="card" data-act="go:brainpage:${esc(p.path)}" style="cursor:pointer;"><div class="row">
        <div class="grow"><div class="title">${esc(p.title)}</div>
          ${p.line ? `<div class="sub">${esc(p.line)}</div>` : ""}</div>
        ${icon("chevron", 14)}
      </div></div>`).join("")}`).join("");
  const anything = knows.length || (brain.pages || []).length;
  const forget = anything ? `
    <div class="card"><div class="row" style="align-items:center;gap:10px;">
      <div class="grow"><div class="title" style="font-weight:500;">Start fresh</div>
        <div class="sub">Forget every page and its history. What you told it yourself stays.</div></div>
      <button class="ghost danger" data-act="forgetlearned">Forget</button>
    </div></div>` : "";
  return `
    ${settingsCrumbs("understanding")}
    <h2>About you</h2>

    ${locationSetup}${addressSetup}
    ${rememberedSection()}
    <h3 style="margin-top:16px;">What it knows best</h3>
    ${listOr(knowsRows, "Nothing yet. It keeps the few things that matter most here, from its pages.")}
    <div class="row" style="margin-top:6px;"><button class="ghost" data-act="go:brainpage:memory">Open memory</button></div>
    <h3 style="margin-top:16px;">Its pages</h3>
    ${listOr(pageList, "No pages yet. As you talk, it notes what lasts onto pages it organises itself.")}
    ${forget}`;
}

// One page of the brain: read it, edit it as text, see what happened to it, or delete it.
function viewBrainPage() {
  const id = NAV.id || "";
  const page = BRAIN_PAGE;
  const back = crumbs([
    { label: "Settings", act: "go:settings" },
    { label: "About you", act: "go:understanding" },
    { label: id },
  ]);
  if (!page) return `${back}<div class="empty">Loading…</div>`;
  const history = (page.history || []).map((h) => `
    <div class="row" style="gap:8px;"><span class="sub" style="min-width:92px;">${esc(String(h.at).slice(0, 10))}</span><span>${esc(h.why)}</span></div>`).join("");
  const body = BRAIN_EDITING ? `
    <textarea id="brain-text" rows="18" style="width:100%;font-family:var(--mono, monospace);">${esc(page.text)}</textarea>
    <div class="row" style="gap:8px;margin-top:8px;">
      <button class="primary" data-act="brainsave:${esc(id)}">Save</button>
      <button class="ghost" data-act="brainedit:off">Cancel</button>
    </div>` : `
    <div class="card">${rich(page.text)}</div>
    <div class="row" style="gap:8px;margin-top:8px;">
      <button class="ghost" data-act="brainedit:on">Edit</button>
      ${id === "memory" ? "" : `<button class="ghost danger" data-act="braindelete:${esc(id)}">Delete</button>`}
    </div>`;
  return `
    ${back}
    ${body}
    <h3 style="margin-top:16px;">What happened to it</h3>
    ${listOr(history, "No history yet.")}`;
}

// How Endora's own actions have actually landed (ADR 0053).
//
// Deliberately four numbers rather than one percentage. A claim of success that changed
// nothing is not the same kind of miss as an outright error, and "couldn't be checked" is
// genuinely unknown — counting an unknown as a success is how a system starts lying to
// itself about how well it works.
//
// It names the worst offender because a number nobody can act on is decoration. That is also
// where a tool Endora has no way to know is read-only shows up: it looks like an actuator
// that never changes anything, which is exactly what it is from Endora's side.
function viewHowItLands() {
  if (!LANDING || !LANDING.considered) return "";
  const worst = LANDING.worst_offender;
  return `
    <h3 style="margin-top:22px;">How its actions have landed</h3>
    <div class="card">
      <div class="title" style="font-weight:500;">${LANDING.changed} of ${LANDING.considered} verified as doing what was asked</div>
      <div class="sub" style="margin-top:4px;">${esc(LANDING.in_words)}</div>
      ${worst ? `<div class="sub" style="margin-top:6px;">Most often claims success and changes nothing: <b>${esc(worst.capability)}</b> (${worst.times}×). If that one only ever reads, tell Endora it's this server's reader in <a class="link" data-act="go:skills">Skills</a> — then it stops being treated as an action.</div>` : ""}
    </div>`;
}

// Connect something new to a service Endora already reaches (ADR 0054).
//
// Endora does not know what a calendar or a mail account needs — the service does, and it
// says so. The form below is rendered from whatever came back, so a kind of thing nobody
// here has heard of works exactly like one that ships today.
//
// The suggestions are a convenience, not a list of what is supported: anything the service
// can set up can be typed in. They are what a butler is most likely to be asked for.
const WORTH_CONNECTING = [
  ["caldav", "Calendar", "iCloud, Fastmail, Nextcloud — anything CalDAV"],
  ["local_calendar", "A calendar kept here", "no account needed"],
  ["imap", "Email (read only)", "so it can tell you what arrived"],
  ["mqtt", "Sensors over MQTT", "doors, motion, whatever you add"],
];

// Endpoints Endora knows, so it stops asking for what it could have said.
//
// The form renders exactly what the service declares — and Home Assistant's CalDAV form
// declares no default for `url`, so it asked for an address that Endora could name perfectly
// well. Naming iCloud, Fastmail and Nextcloud in the description and then not offering their
// endpoints is the gap.
//
// These are **suggestions**, not knowledge about the service: choosing one fills the fields
// and every one stays editable, which is the same shape the model settings already use for
// provider presets. Field names come from the live flows, not from memory.
const KNOWN_PROVIDERS = {
  caldav: [
    ["iCloud", { url: "https://caldav.icloud.com" }],
    ["Fastmail", { url: "https://caldav.fastmail.com" }],
    ["Nextcloud", { url: "https://your-server/remote.php/dav" }],
  ],
  imap: [
    ["iCloud", { server: "imap.mail.me.com", port: "993" }],
    ["Gmail", { server: "imap.gmail.com", port: "993" }],
    ["Fastmail", { server: "imap.fastmail.com", port: "993" }],
    ["Outlook", { server: "outlook.office365.com", port: "993" }],
  ],
};

// Fills the form from a chosen provider. Nothing is submitted and nothing is locked — this
// is typing on the person's behalf, which they can undo by typing over it.
function useProvider(kind, index) {
  const chosen = (KNOWN_PROVIDERS[kind] || [])[index];
  if (!chosen) return;
  const [label, values] = chosen;
  for (const [field, value] of Object.entries(values)) {
    const el = document.getElementById(`connect-${field}`);
    if (el) el.value = value;
  }
  flash(`Filled in what ${label} uses — check it and sign in.`, "ok");
}

function viewConnect() {
  if (CONNECT && CONNECT.fields) {
    const fields = CONNECT.fields.map((f) => `
      <div class="field">
        <label>${esc(f.name)}${f.required ? "" : " <span class=\"sub\">(optional)</span>"}</label>
        <input id="connect-${esc(f.name)}" type="${f.secret ? "password" : (f.kind === "boolean" ? "text" : "text")}"
               autocomplete="off" value="${esc(f.default == null ? "" : String(f.default))}"
               placeholder="${f.secret ? "never stored by Endora" : ""}" />
      </div>`).join("");
    const providers = KNOWN_PROVIDERS[CONNECT.kind] || [];
    const presets = providers.length
      ? `<div class="field"><label>Who is it with?</label>
           <select onchange="useProvider('${esc(CONNECT.kind)}', this.value)">
             <option value="">Choose to fill in the address…</option>
             ${providers.map(([label], i) => `<option value="${i}">${esc(label)}</option>`).join("")}
           </select></div>`
      : "";
    return `
      <div class="title" style="margin:14px 0 6px;">Connecting ${esc(CONNECT.kind || "something")}</div>
      <div class="card">
        <div class="note" style="margin-bottom:8px;">These are the questions <b>your own service</b> asked — Endora is passing them on. Anything you type here goes straight to it and is never written down here.</div>
        ${presets}
        ${fields}
        <div class="row" style="gap:8px;justify-content:flex-end;">
          <button class="ghost" data-act="connect:cancel">Cancel</button>
          <button class="primary" data-act="connect:submit">Connect</button>
        </div>
      </div>`;
  }
  // Offering to set up something already working reads as though the last attempt failed, and
  // the screen had no other way to answer "is my calendar connected?" (ADR 0058). A service
  // already there says so and offers nothing; adding a second account is still possible from
  // the box below, by name.
  const isConnected = (kind) => (CONNECTED || []).includes(kind);
  const buttons = WORTH_CONNECTING.map(([kind, label, why]) => `
    <div class="row" style="align-items:center;gap:10px;margin-bottom:8px;">
      <div class="grow"><div class="title" style="font-weight:500;">${esc(label)}</div><div class="sub">${isConnected(kind) ? "Already set up — Endora can use it." : esc(why)}</div></div>
      ${isConnected(kind)
        ? `<span class="pill active">connected</span>`
        : `<button class="ghost" data-act="connect:start:${esc(kind)}">Connect</button>`}
    </div>`).join("");
  // Everything else the person has set up themselves. Without this the screen could only ever
  // talk about the four things Endora happens to suggest, and stayed silent about the rest.
  const offered = WORTH_CONNECTING.map(([kind]) => kind);
  const alsoThere = (CONNECTED || []).filter((k) => !offered.includes(k));
  const alsoHtml = alsoThere.length
    ? `<div class="sub" style="margin-top:10px;">Also set up: ${alsoThere.map(esc).join(", ")}</div>`
    : "";
  // Folded: most visits are to see what is connected, not to add something. A connection in
  // progress (above) is shown open.
  return `
    <details class="adv"><summary>Connect something new through Home Assistant</summary>
    <div class="note" style="margin:8px 0 10px;">Endora sets it up in Home Assistant for you — you only sign in. Whatever you add here, it can use straight away.</div>
    <div class="card">
      ${buttons}
      <div class="form" style="margin-top:6px;">
        <input id="connect-other" placeholder="or something else, by its Home Assistant name" />
        <button class="ghost" data-act="connect:other">Start</button>
      </div>
      ${alsoHtml}
    </div>
    </details>`;
}

// The one thing Endora would like an answer about, shown where you already are.
//
// It used to live on a screen reached by menu → Settings → Understanding → scroll: three
// taps away from the conversation, on a page holding six unrelated things. A question you
// have to go and find is a question that does not get answered — 109 outcomes carried zero
// reactions for exactly that reason.
//
// **One at a time, never a list.** Seven problem statements rendered together is the pile of
// chores ADR 0052 exists to prevent, and putting the pile in the chat would only make it
// harder to ignore. This is the same "a cursor, not a queue" rule that governs intentions:
// one question, answering it is the dismissal, and the next appears when it appears.
//
// No badge and no count anywhere. Both are how a queue announces itself, and a screen with
// "3 waiting" on it is a chore list whatever the cards say.
function viewNeedsYou() {
  // Before anything else: where the person is.
  //
  // One line of text, and four skills are silent without it — weather, the news, safety
  // alerts and what the city is doing are all "near you", and near nowhere is nothing. They
  // report themselves as configured the whole time, so the failure looks like working.
  //
  // It was already being asked, three taps away on Settings > Understanding, which is exactly
  // the placement the comment above this function was written about. A question you have to
  // go and find is a question that does not get answered — and this one is the difference
  // between a butler that knows where it lives and one that does not.
  const knowsWhere = (DB.preferences || []).some((p) => /^\s*based in\b/i.test(p.text));
  if (!knowsWhere) {
    return `
      <div class="card" style="margin:6px 0;border-color:color-mix(in srgb, var(--accent) 30%, var(--line));">
        <div class="title" style="font-weight:500;">Where are you based?</div>
        <div class="sub" style="margin-bottom:6px;">The weather, the news, safety alerts and what's on locally all need this. A town or city is enough.</div>
        <div class="form">
          <input id="setup-location" placeholder="a city, e.g. New York, NY" />
          <button class="primary" data-act="setlocation">Save</button>
        </div>
      </div>`;
  }

  // Devices that stopped answering are not asked about: the owner did not want it
  // (2026-10-16). A misfiring tool is Endora's own, and still is.
  const repair = (REPAIRS || [])[0];
  if (!repair) return "";
  if (repair.remedy === "stop_offering_it") {
    return `
      <div class="card" style="margin:6px 0;">
        <div class="row" style="align-items:center;gap:10px;">
          <div class="grow">
            <div class="title" style="font-weight:500;">${esc(repair.capability)}</div>
            <div class="sub">${repair.attempts} attempts, and it has never once worked. Looks like the wrong tool rather than the wrong name.</div>
          </div>
          <button class="ghost danger" data-act="skill:enable:${esc(repair.capability)}:0">Stop offering it</button>
        </div>
      </div>`;
  }
  // "Leave it" is not decoration. Answering is meant to be the dismissal, but somebody who
  // does not know the real name has no answer to give — and with no way out the card sits
  // there for good. The trouble card has had "It's fine" for exactly this reason; this one
  // never did.
  return `
    <div class="card" style="margin:6px 0;">
      <div class="grow">
        <div class="title" style="font-weight:500;">${esc(repair.capability)}</div>
        <div class="sub">${repair.attempts} attempts aimed at &ldquo;${esc(repair.target)}&rdquo; didn't work. What is it actually called?</div>
        <div class="form" style="margin-top:6px;">
          <input id="alias-${esc(repair.capability)}-${esc(repair.target)}" placeholder="the real name, e.g. Kitchen Main" />
          <button class="primary" data-act="alias:${esc(repair.capability)}:${esc(repair.target)}">Remember</button>
        </div>
        <div class="row" style="justify-content:flex-end;margin-top:6px;">
          <button class="ghost" data-act="repair:leave:${esc(repair.capability)}:${esc(repair.target)}" title="Stop asking about this one. Nothing is turned off.">Leave it</button>
        </div>
      </div>
    </div>`;
}

// Changes Endora has made to your services' own settings (ADR 0054).
//
// The memory right to SEE what it changed about the world, next to the right to see what
// it believes and what it did — and, unlike those, a button to put each one back. Undone
// changes stay listed: what Endora changed about your house is not something it should be
// able to make disappear.
function viewConfigWrites() {
  if (!(CONFIG_WRITES || []).length) return "";
  const rows = CONFIG_WRITES.map((w) => `
    <div class="card"><div class="row">
      <div class="grow">
        <div class="title">${esc(w.what)}${w.undone ? ` <span class="pill">put back</span>` : ""}</div>
        <div class="sub">in ${esc(w.server)}${w.at_ms ? ` · ${new Date(w.at_ms).toLocaleString()}` : ""}</div>
      </div>
      ${w.undone ? "" : `<button class="ghost" data-act="unwrite::${esc(w.id)}" title="Put this back exactly as it was">Undo</button>`}
    </div></div>`).join("");
  return `
    <h3 style="margin-top:22px;">Changes Endora made to your services</h3>
    <div class="note" style="margin-bottom:10px;">Names it wrote into a connected service so the fix works everywhere, not just here. Each one can be put back.</div>
    ${rows}`;
}

// What Endora has actually DONE, and what it saw afterwards (ADR 0053).
//
// The tool's claim and Endora's observation are shown SEPARATELY, exactly as they
// are stored — the console must not merge them either, because a tool that reports
// success while nothing changed is the whole reason the record exists.
//
// Saying how it landed is optional and never solicited: no badge, no counter, no
// "N awaiting your feedback". An outcome nobody comments on is complete.
function viewOutcomes() {
  if (!(OUTCOMES || []).length) return "";
  const rows = OUTCOMES.map((o) => {
    const seen = o.observed
      ? `<div class="sub">Endora then looked: ${esc(o.observation)}</div>`
      : `<div class="sub">Endora couldn't check this one for itself.</div>`;
    const picked = (r) => o.reaction === r ? " active" : "";
    return `
      <div class="card"><div class="row">
        <div class="grow">
          <div class="title">${esc(o.capability)} <span class="pill${o.observed ? "" : " pending"}">${o.observed ? "checked" : "unconfirmed"}</span></div>
          <div class="sub">It reported: ${esc(o.claim)}</div>
          ${seen}
        </div>
        <button class="ghost${picked("helped")}" data-act="react:helped:${o.id}" title="that helped">Helped</button>
        <button class="ghost${picked("did_not_help")}" data-act="react:did_not_help:${o.id}" title="that didn't help">Didn't</button>
      </div></div>`;
  }).join("");
  return `
    <h3 style="margin-top:22px;">What Endora has done</h3>
    <div class="note" style="margin-bottom:10px;">Only what it changed — reading things isn't listed. Say how one landed if you like; there's no need to.</div>
    ${rows}`;
}

