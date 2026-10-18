
// ---- Endora HQ: the team's world (ADR 0122) -------------------------------------------------
// What the team knows, by stage: the house page's sections (canon first), its dated timeline,
// the lab and team pages HQ derives, and the stories. Only the owner makes something canon or
// demotes it, here or from a proposal on the desk. Every word is a member's or the record's, so
// all of it is escaped. The whole world downloads as one Markdown file.

const HQ_STAGE_ORDER = { canon: 0, insight: 1, observation: 2, field_note: 3, superseded: 4, retired: 5 };

// What the owner pastes, as changes (the "Paste Markdown" editor). A section starts with
// `## <page> / <key> — <Title>` (a hyphen or a colon will do for the dash, and the spaces
// round the slash are optional); its text runs to the next `##`. A `## timeline` heading (or
// `## timeline <page>`) holds bullets `- <date>: <text>`, each a timeline entry on that page.
// A `# ` title line and anything before the first `##` are ignored. Pure: no screen, no network.
const HQ_WORLD_PAGES = ["house", "lab", "team"];
const HQ_MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

// A date as a person writes it (17 Oct 2026, Oct 17 2026, 2026-10-17) as noon UTC, or null.
function hqWorldDate(said) {
  const t = String(said || "").trim().replace(/[.,]/g, " ").replace(/\s+/g, " ");
  let y, m, d, hit;
  if ((hit = /^(\d{4})-(\d{2})-(\d{2})$/.exec(t))) { y = +hit[1]; m = +hit[2] - 1; d = +hit[3]; }
  else if ((hit = /^(\d{1,2}) ([A-Za-z]{3,9}) (\d{4})$/.exec(t))) { d = +hit[1]; m = HQ_MONTHS.indexOf(hit[2].slice(0, 3).toLowerCase()); y = +hit[3]; }
  else if ((hit = /^([A-Za-z]{3,9}) (\d{1,2}) (\d{4})$/.exec(t))) { m = HQ_MONTHS.indexOf(hit[1].slice(0, 3).toLowerCase()); d = +hit[2]; y = +hit[3]; }
  else return null;
  if (m < 0 || d < 1 || d > 31) return null;
  const at = Date.UTC(y, m, d, 12);
  return new Date(at).getUTCDate() === d ? at : null;
}

// A key as a title when none was given: how-we-work -> How we work.
function hqWorldTitleOf(key) {
  const words = String(key).replace(/[-_]+/g, " ").trim();
  return words ? words[0].toUpperCase() + words.slice(1) : "";
}

// -> { changes: [{ kind: "section", page, key, title, text } | { kind: "timeline", page, at, when, text }],
//      notes: [what was set aside and why] }
function hqWorldParse(markdown) {
  const changes = [], notes = [];
  const blocks = [];
  let block = null;
  for (const raw of String(markdown || "").split(/\r?\n/)) {
    const head = /^##(?!#)\s*(.*?)\s*$/.exec(raw);
    if (head) { block = { head: head[1], lines: [] }; blocks.push(block); continue; }
    if (block) block.lines.push(raw);
  }
  for (const b of blocks) {
    const text = b.lines.join("\n").trim();
    const timeline = /^timeline\b\s*([a-z][a-z0-9_-]*)?/i.exec(b.head);
    if (timeline) {
      const word = (timeline[1] || "").toLowerCase();
      const page = HQ_WORLD_PAGES.includes(word) ? word : "house";
      for (const line of b.lines) {
        const bullet = /^\s*[-*]\s+(.*\S)\s*$/.exec(line);
        if (!bullet) continue;
        const dated = /^(\d{4}-\d{2}-\d{2}|\d{1,2} [A-Za-z]{3,9}\.?,? \d{4}|[A-Za-z]{3,9}\.? \d{1,2},? \d{4})\s*[:—–-]\s*(\S.*)$/.exec(bullet[1]);
        const at = dated ? hqWorldDate(dated[1]) : null;
        changes.push(at === null
          ? { kind: "timeline", page, at: null, when: "", text: bullet[1] }
          : { kind: "timeline", page, at, when: dated[1].replace(/\s+/g, " "), text: dated[2].trim() });
      }
      continue;
    }
    const sect = /^([a-z][a-z0-9_-]*)\s*\/\s*([a-z0-9][a-z0-9_-]*)\s*(?:[—–:-]\s*(.*))?$/i.exec(b.head);
    if (!sect) { notes.push(`Set aside "${b.head}": a section heading reads "## page / key — Title".`); continue; }
    const key = sect[2].toLowerCase();
    if (!text) { notes.push(`Set aside "${b.head}": it has no text.`); continue; }
    changes.push({ kind: "section", page: sect[1].toLowerCase(), key, title: (sect[3] || "").trim() || hqWorldTitleOf(key), text });
  }
  return { changes, notes };
}

// The pasted document's state: the text, the changes read from it, and how each went.
function hqWorldDoc() { return HQ.worldDoc || (HQ.worldDoc = { md: "", items: null, notes: [] }); }

// What one change would do, in a line.
function hqWorldSaid(c) {
  if (c.kind === "timeline") return `timeline${c.page === "house" ? "" : " " + c.page}: ${c.when ? c.when + " " : ""}${c.text}`;
  const sections = ((HQ.snap && HQ.snap.world && HQ.snap.world.sections) || []);
  const old = sections.find((p) => p.page === c.page && p.anchor === c.key);
  const doing = old ? `replaces the current text${old.written_by && old.written_by !== "the owner" ? ` (${old.written_by}'s stays in its history)` : ""}` : "new section";
  return `${c.page} / ${c.key} — ${c.title}: ${doing}`;
}

// A piece's stage as shown: its own, or "unconfirmed since <date>" once its evidence is old.
function hqStageTag(p) {
  const shown = p.unconfirmed_since ? "observation" : String(p.stage || "observation");
  return `<span class="hq-tag stage-${esc(shown)}">${esc(p.said || p.stage || "")}</span>`;
}

// The owner's two acts on a piece: make it canon, or demote canon saying why.
function hqWorldActs(kind, i, p) {
  const edit = kind === "section" ? `<button class="hq-btn tall" data-act="hq:world:edit:open-${Number(i)}">Edit</button>` : "";
  if (p.stage === "canon") return edit + `<button class="hq-btn tall no" data-act="hq:world:${kind}:demote-${Number(i)}">Demote\u2026</button>`;
  if (p.stage === "retired" || p.stage === "superseded") return edit;
  return edit + `<button class="hq-btn tall ok" data-act="hq:world:${kind}:promote-${Number(i)}">Make canon</button>`;
}

// The paste editor and its preview: tick what to apply, then one Apply.
function hqWorldPaste() {
  const doc = hqWorldDoc();
  const items = (doc.items || []).map((it, i) => {
    const mark = it.result === "ok" ? `<span class="hq-tag stage-canon">done</span>` : it.result ? `<span class="hq-tag stage-retired">${esc(it.result)}</span>` : "";
    return `<li class="hq-pick"><label><input type="checkbox" data-act="hq:world:paste:toggle-${i}"${it.on ? " checked" : ""}${it.result === "ok" ? " disabled" : ""}>
      <span>${esc(hqWorldSaid(it.change))}</span></label> ${mark}${it.warn ? `<small class="hq-note"> ${esc(it.warn)}</small>` : ""}</li>`;
  }).join("");
  const waiting = (doc.items || []).filter((it) => it.on && it.result !== "ok").length;
  const preview = doc.items === null ? "" : `<h3>Preview</h3>
      ${(doc.notes || []).map((n) => `<p class="hq-note">${esc(n)}</p>`).join("")}
      <ul class="hq-world hq-changes">${items || `<li class="hq-empty">Nothing to apply: no \u201c## page / key \u2014 Title\u201d section or \u201c## timeline\u201d bullet was found.</li>`}</ul>
      <div class="hq-row"><button class="hq-btn tall ok" data-act="hq:world:paste:apply"${waiting ? "" : " disabled"}>Apply ${waiting} change${waiting === 1 ? "" : "s"}</button>
        <button class="hq-btn tall" data-act="hq:world:paste:clear">Clear</button></div>`;
  return `<details${doc.md ? " open" : ""}><summary>Paste Markdown</summary>
      <p class="hq-note">Paste a whole document. A section starts with <code>## house / key \u2014 Title</code> (a <code>-</code> or <code>:</code> works for the dash) and runs to the next <code>##</code>. Under <code>## timeline</code>, each bullet <code>- 17 Oct 2026: what happened</code> is a timeline entry. A <code># Title</code> line is ignored. Same key as a section that exists replaces it; the old text stays in its history.</p>
      <textarea id="hq-wd-md" class="hq-doc" rows="14" autocapitalize="off" autocomplete="off" autocorrect="off" spellcheck="false" aria-label="Markdown to paste">${esc(doc.md)}</textarea>
      <div class="hq-row"><button class="hq-btn tall ok" data-act="hq:world:paste:preview">Preview</button></div>
      ${preview}</details>`;
}

// The full-screen editor for one section: Save and Cancel stay at the top.
function hqWorldEditor() {
  const e = HQ.worldEdit;
  if (!e) return "";
  return `<div class="hq-editor" role="dialog" aria-modal="true" aria-label="Edit ${esc(e.title)}">
    <div class="hq-editor-bar"><button class="hq-btn tall" data-act="hq:world:edit:cancel">Cancel</button>
      <b class="hq-editor-name">${esc(e.page)} / ${esc(e.key)}</b>
      <button class="hq-btn tall ok" data-act="hq:world:edit:save"${e.busy ? " disabled" : ""}>Save</button></div>
    <input id="hq-we-title" class="hq-editor-title" maxlength="80" autocapitalize="sentences" value="${esc(e.title)}" aria-label="Title">
    <textarea id="hq-we-text" class="hq-editor-text" maxlength="1200" autocapitalize="off" aria-label="Section text (first line is its headline)">${esc(e.text)}</textarea>
    ${e.note ? `<p class="hq-note">${esc(e.note)}</p>` : ""}</div>`;
}

function hqWorldCard(s) {
  const w = s.world;
  if (!w) return "";
  const sections = (w.sections || []).map((p, i) => ({ p, i }))
    .sort((a, b) => (HQ_STAGE_ORDER[a.p.unconfirmed_since ? "observation" : a.p.stage] ?? 9) - (HQ_STAGE_ORDER[b.p.unconfirmed_since ? "observation" : b.p.stage] ?? 9));
  const sectionRows = sections.map(({ p, i }) => `<li><b>${esc(p.heading)}</b> ${hqStageTag(p)}
      <span>${esc(p.headline)}</span>
      <small> ${esc(p.address)} · revision ${Number(p.revision) || 1}, ${esc(p.written_by || "")}${p.warrant ? ` · warrant: ${esc(p.warrant)}` : ""}${p.why ? ` · ${esc(p.why)}` : ""}</small>
      <div class="hq-row">${hqWorldActs("section", i, p)}</div></li>`).join("");
  const timeline = ((w.pages || [])[0] || {}).timeline || [];
  const timelineRows = timeline.slice(0, 12).map((l) => `<li><b>${esc(l.day)}</b>, ${esc(l.by)}: ${esc(l.line)}</li>`).join("");
  const derived = (w.pages || []).filter((p) => p.derived).map((p) => `<details><summary>${esc(p.title)} <small>(world:${esc(p.slug)}, from the record)</small></summary>
      <ul>${(p.lines || []).map((l) => `<li>${esc(l)}</li>`).join("")}</ul></details>`).join("");
  const stories = (w.stories || []).map((p, i) => `<li><b>${esc(p.handle)}</b> ${hqStageTag(p)}${p.to_retell ? ` <span class="hq-tag">to retell</span>` : ""}
      <span>${p.moment ? `When ${esc(p.moment)} ` : ""}${p.reflex ? `Tempting: ${esc(p.reflex)} ` : ""}Instead: ${esc(p.instead)}${p.because ? ` Because: ${esc(p.because)}` : ""}</span>
      <small> ${esc(p.whose || p.scope || "")} · told by ${esc(p.author || "")}${p.why ? ` · ${esc(p.why)}` : ""}</small>
      <div class="hq-row">${hqWorldActs("story", i, p)}</div></li>`).join("");
  return `<section class="hq-card"><h2>World</h2>
    <p class="hq-note">What the team knows, by stage. Only you make something canon; an insight not confirmed in time reads as unconfirmed.</p>
    ${hqWorldPaste()}
    <h3>The house page</h3>
    <ul class="hq-team hq-world">${sectionRows || `<li class="hq-empty">No sections yet: the CEO writes them from the timeline.</li>`}</ul>
    <details><summary>Its timeline (${timeline.length})</summary><ul class="hq-thread hq-timeline">${timelineRows || `<li class="hq-empty">Nothing yet.</li>`}</ul></details>
    ${derived}
    <details><summary>Write as the owner</summary>
      <p class="hq-note">Your words stand on the house page as an insight and show first in every briefing. Writing under a key that exists replaces that section; its old text stays in its history.</p>
      <div class="hq-row"><label>Key <input id="hq-w-key" maxlength="40" placeholder="nas"></label>
        <label>Title <input id="hq-w-title" maxlength="80" placeholder="The NAS"></label></div>
      <textarea id="hq-w-text" rows="4" autocapitalize="off" maxlength="1200" aria-label="Section text (first line is its headline)"></textarea>
      <div class="hq-row"><button class="hq-btn tall ok" data-act="hq:world:write:section">Write the section</button>
        <button class="hq-btn tall" data-act="hq:world:write:timeline">Add to the timeline</button></div></details>
    <h3>Stories</h3>
    <ul class="hq-team hq-world">${stories || `<li class="hq-empty">No stories yet.</li>`}</ul>
    <div class="hq-row"><button class="hq-btn tall" data-act="hq:world:export:md">Download the world (Markdown)</button></div>
    ${hqWorldEditor()}</section>`;
}

// The owner's acts on the world: make a piece canon, demote canon with a reason, or download it.
async function hqWorldAct(kind, what) {
  const w = (HQ.snap && HQ.snap.world) || {};
  if (kind === "export") {
    try {
      const res = await fetch("/v1/hq/world/export.md", { headers: signed({}) });
      if (!res.ok) throw new Error("HTTP " + res.status);
      const url = URL.createObjectURL(await res.blob());
      const a = document.createElement("a"); a.href = url; a.download = "team-world.md"; a.click();
      setTimeout(() => URL.revokeObjectURL(url), 10000);
      flash("The world is saved as team-world.md.", "ok");
    } catch (e) { flash("Couldn't download it: " + e.message, "err"); }
    return true;
  }
  if (kind === "paste") return hqWorldPasteAct(what);
  if (kind === "edit") return hqWorldEditAct(what);
  if (kind === "write") {
    const val = (id) => { const el = document.getElementById(id); return el ? el.value.trim() : ""; };
    const text = val("hq-w-text");
    if (!text) { flash("Write something first.", "err"); return true; }
    try {
      if (what === "section") {
        const title = val("hq-w-title");
        if (!title) { flash("A section needs a title.", "err"); return true; }
        await api("POST", "/v1/hq/world/section", { page: "house", key: val("hq-w-key"), title, text });
        flash("It is on the house page.", "ok");
      } else {
        await api("POST", "/v1/hq/world/timeline", { page: "house", text });
        flash("It is on the timeline.", "ok");
      }
    } catch (e) { flash("Couldn't write it: " + e.message, "err"); return true; }
    await loadHQ(); render(); return true;
  }
  const [op, n] = String(what || "").split("-");
  const list = kind === "story" ? (w.stories || []) : (w.sections || []);
  const p = list[Number(n)];
  if (!p) return true;
  try {
    if (op === "promote") {
      if (!confirm(`Make "${p.headline || p.handle}" canon? The team will treat it as established.`)) return true;
      await api("POST", "/v1/hq/world/promote", { address: p.address, why: "" });
      flash("It is canon.", "ok");
    } else if (op === "demote") {
      const why = (prompt("Why is it no longer canon?") || "").trim(); if (!why) return true;
      await api("POST", "/v1/hq/world/demote", { address: p.address, to: "insight", why });
      flash("Demoted to an insight; the feed and the page say why.", "ok");
    }
  } catch (e) { flash("Couldn't change it: " + e.message, "err"); }
  await loadHQ(); render(); return true;
}

// Keeps what is typed in the paste box and the editor across a redraw.
document.body.addEventListener("input", (ev) => {
  const id = ev.target && ev.target.id;
  if (id === "hq-wd-md") hqWorldDoc().md = ev.target.value;
  if (HQ.worldEdit && id === "hq-we-title") HQ.worldEdit.title = ev.target.value;
  if (HQ.worldEdit && id === "hq-we-text") HQ.worldEdit.text = ev.target.value;
});

// The paste editor: read it into changes, tick or untick them, apply the ticked ones in order.
async function hqWorldPasteAct(what) {
  const doc = hqWorldDoc();
  const box = document.getElementById("hq-wd-md");
  if (box) doc.md = box.value;
  const [op, n] = String(what || "").split("-");
  if (op === "preview") {
    const read = hqWorldParse(doc.md);
    doc.notes = read.notes;
    doc.items = read.changes.map((change) => {
      const long = change.kind === "section" ? change.text.length > 1200 : change.text.length > 500;
      return { change, on: !long, result: "", warn: long ? "Too long for the world; shorten it or untick." : "" };
    });
    render(); return true;
  }
  if (op === "clear") { HQ.worldDoc = null; render(); return true; }
  if (op === "toggle") {
    const it = (doc.items || [])[Number(n)];
    if (it) it.on = !it.on;
    render(); return true;
  }
  if (op !== "apply") return true;
  // Sections first, then the timeline, each in the order written.
  const todo = (doc.items || []).filter((it) => it.on && it.result !== "ok");
  const ordered = todo.filter((it) => it.change.kind === "section").concat(todo.filter((it) => it.change.kind === "timeline"));
  let done = 0, failed = 0;
  for (const it of ordered) {
    const c = it.change;
    try {
      if (c.kind === "section") await api("POST", "/v1/hq/world/section", { page: c.page, key: c.key, title: c.title, text: c.text });
      else await api("POST", "/v1/hq/world/timeline", c.at === null ? { page: c.page, text: c.text } : { page: c.page, at: c.at, text: c.text });
      it.result = "ok"; it.on = false; it.warn = ""; done++;
    } catch (e) { it.result = "failed"; it.warn = String(e.message || e); failed++; }
  }
  flash(failed ? `${done} applied, ${failed} failed: the failed ones are still ticked.` : `${done} applied.`, failed ? "warn" : "ok");
  await loadHQ(); render(); return true;
}

// The section editor: open one with its current text, save it back under the same page and key.
async function hqWorldEditAct(what) {
  const [op, n] = String(what || "").split("-");
  if (op === "open") {
    const p = (((HQ.snap && HQ.snap.world) || {}).sections || [])[Number(n)];
    if (!p) return true;
    const body = String(p.body || "");
    // A headline cut short is repeated at the start of the body; do not say it twice.
    const text = body.startsWith(p.headline) ? body : (body ? p.headline + "\n" + body : p.headline);
    HQ.worldEdit = { page: p.page, key: p.anchor, title: p.heading, text, note: p.written_by && p.written_by !== "the owner" ? `Written by ${p.written_by}. Your version replaces it; theirs stays in its history.` : "" };
    render(); return true;
  }
  if (op === "cancel") { HQ.worldEdit = null; render(); return true; }
  const e = HQ.worldEdit;
  if (op !== "save" || !e) return true;
  const title = document.getElementById("hq-we-title"), text = document.getElementById("hq-we-text");
  if (title) e.title = title.value;
  if (text) e.text = text.value;
  if (!e.title.trim() || !e.text.trim()) { flash("A section needs a title and some text.", "err"); return true; }
  e.busy = true;
  try {
    await api("POST", "/v1/hq/world/section", { page: e.page, key: e.key, title: e.title.trim(), text: e.text.trim() });
    HQ.worldEdit = null;
    flash("Saved.", "ok");
  } catch (err) { e.busy = false; flash("Couldn't save it: " + err.message, "err"); render(); return true; }
  await loadHQ(); render(); return true;
}
