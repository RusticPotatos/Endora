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

