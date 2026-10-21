// ---- what every screen shares: the Settings list, the trails, Activity ----------------------
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
// What Endora has done is not here: it is a record, not a setting, and has its own place in the
// menu, Activity.
// What HQ and the house are set up with (ADR 0136).
const UNDER_SETTINGS = [
  { view: "models", icon: "sparkle", label: "AI", note: "every model Endora and its team use, where it runs (the NAS, your Mac, a cloud), and what it sees" },
  { view: "connections", icon: "gear", label: "Connections", note: "Home Assistant, your home lab's services, and tool servers" },
  { view: "skills", icon: "skills", label: "Tools", note: "what the team may use from the house, and which ask you first" },
  { view: "signin", icon: "lock", label: "Your sign-in", note: "password, authenticator, and tokens for scripts" },
  { view: "vault", icon: "lock", label: "Vault", note: "the house's secrets, and the passkeys that unlock them" },
  { view: "data", icon: "export", label: "Your data", note: "take a copy of what Endora knows, or delete it" },
];

// The trail for a screen under Settings. Anything not under Settings gets the short trail, so
// a top-level screen cannot accidentally claim to be nested.
function settingsCrumbs(view) {
  const page = UNDER_SETTINGS.find((p) => p.view === view);
  const here = page ? (page.short || page.label) : view;
  return crumbs([
    { label: "HQ", act: "go:hq" },
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
  const more = ACTIVITY_MORE
    ? `<div class="row" style="margin-top:6px;"><button class="ghost" data-act="activity:more"${ACTIVITY_BUSY ? " disabled" : ""}>${ACTIVITY_BUSY ? "Loading…" : "Load more"}</button></div>`
    : `<div class="sub">That is everything.</div>`;
  return ACTIVITY.map((a) => `
    <div class="card"><div class="row">
      <div class="grow"><div class="title">${icon(ACTIVITY_ICON[a.kind] || "note", 15)} ${esc(a.summary)}</div>
      <div class="sub">${esc(a.kind)}</div></div>
      <span class="sub">${esc(new Date(a.at_ms).toLocaleString())}</span>
    </div></div>`).join("") + more;
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
  return `
    ${crumbs([{ label: "HQ", act: "go:hq" }, { label: "Activity" }])}
    <h2>What Endora has done</h2>
    <div class="note">Its own account of its work — what it tried, whether it checked, and what it changed. Nothing here needs anything from you.</div>
    ${viewConfigWrites()}
    <h3 style="margin-top:22px;">Everything, newest first</h3>
    <div class="note">What it did, in its own words, and every decision policy made about it.</div>
    ${activityFeed()}`;
}

// Provider presets for the models (AI). Each fills the endpoint + example
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

