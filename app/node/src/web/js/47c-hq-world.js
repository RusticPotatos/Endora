
// ---- Endora HQ: the team's world (ADR 0122) -------------------------------------------------
// What the team knows: the house page's sections, its dated timeline, and the lab and team
// pages HQ derives. Every word is a member's or the record's, so
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

// The owner's act on a section: edit it.
function hqWorldActs(kind, i) {
  return kind === "section" ? `<button class="hq-btn tall" data-act="hq:world:edit:open-${Number(i)}">Edit</button>` : "";
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
  if (!(doc.md || doc.open)) return "";
  return `<div id="hq-paste-box"><h3>The paste box</h3>
      <p class="hq-note">Paste a whole document. A section starts with <code>## house / key \u2014 Title</code> (a <code>-</code> or <code>:</code> works for the dash) and runs to the next <code>##</code>. Under <code>## timeline</code>, each bullet <code>- 17 Oct 2026: what happened</code> is a timeline entry. A <code># Title</code> line is ignored. Same key as a section that exists replaces it; the old text stays in its history.</p>
      <textarea id="hq-wd-md" class="hq-doc" rows="14" autocapitalize="off" autocomplete="off" autocorrect="off" spellcheck="false" aria-label="Markdown to paste">${esc(doc.md)}</textarea>
      <div class="hq-row"><button class="hq-btn tall ok" data-act="hq:world:paste:preview">Preview</button></div>
      ${preview}</div>`;
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

// Which page of the world the owner has open, and whether the owner's form is open: kept across
// HQ's redraws, so nothing folds shut under the owner (2026-10-25).
const HQ_WORLD_VIEW = { page: null, writing: false };

// The world's pages as a list (ADR 0122, 0126): the house, the pages from the record, and one
// for every project and goal, each with its steward and its sections.
// Everything in them is a member's, so all of it is escaped.
function hqWorldPageList(w) {
  const mapped = ((w.map || {}).pages || []);
  const pages = mapped.length ? mapped : [{ slug: "house", address: "world:house", title: "The house", steward: "the CEO", sections: (w.sections || []).length }];
  const rows = pages.map((pg) => {
    const count = Number(pg.sections) || 0;
    const what = pg.derived ? "from the record" : `kept by ${esc(pg.steward)} · ${count} section${count === 1 ? "" : "s"}`;
    return `<li><button class="hq-link" data-act="hq:world:page:${esc(pg.slug)}">${esc(pg.title)}</button> <small>${esc(pg.address || "")} · ${what}</small></li>`;
  });
  const listed = hqMore("pages", rows, 6);
  return `<h3>Pages</h3><p class="hq-note">One for the house, the lab and every project and goal. Open one to read and edit it.</p>
    <ul class="hq-team hq-world">${listed.shown}</ul>${listed.more}`;
}

// One page of the world, open on its own, with the way back to the list.
function hqWorldPageBody(w, slug, sections, sectionRow) {
  const back = `<div class="hq-row"><button class="hq-btn tall" data-act="hq:world:page:all">\u2190 All pages</button></div>`;
  if (slug === "house") {
    const rows = sections.filter(({ p }) => (p.page || "house") === "house").map(sectionRow).join("");
    const timeline = ((w.pages || [])[0] || {}).timeline || [];
    const lines = timeline.slice(0, 20).map((l) => `<li><b>${esc(l.day)}</b>, ${esc(l.by)}: ${esc(l.line)}</li>`).join("");
    return `${back}<h3>The house page</h3>
      <ul class="hq-team hq-world">${rows || `<li class="hq-empty">No sections yet: the CEO writes them from the timeline.</li>`}</ul>
      <h4>Its timeline (${timeline.length})</h4><ul class="hq-thread hq-timeline">${lines || `<li class="hq-empty">Nothing yet.</li>`}</ul>`;
  }
  const derived = (w.pages || []).find((p) => p.derived && p.slug === slug);
  if (derived) {
    return `${back}<h3>${esc(derived.title)} <small>(world:${esc(derived.slug)}, from the record)</small></h3>
      <ul>${(derived.lines || []).map((l) => `<li>${esc(l)}</li>`).join("") || `<li class="hq-empty">Nothing yet.</li>`}</ul>`;
  }
  const pg = ((w.map || {}).pages || []).find((p) => p.slug === slug) || { title: slug, address: `world:${slug}`, steward: "" };
  const mine = sections.filter(({ p }) => p.page === slug);
  return `${back}<h3>${esc(pg.title)}</h3><p class="hq-note">${esc(pg.address || "")}${pg.steward ? ` · kept by ${esc(pg.steward)}` : ""}</p>
    <ul class="hq-team hq-world">${mine.map(sectionRow).join("") || `<li class="hq-empty">No sections yet.</li>`}</ul>`;
}

// The written pages, for the owner's "Write as the owner" form.
function hqWorldPageOptions(w) {
  const pages = ((w.map || {}).pages || []).filter((pg) => !pg.derived);
  const list = pages.length ? pages : [{ slug: "house", title: "The house" }];
  return list.map((pg) => `<option value="${esc(pg.slug)}">${esc(pg.title)}</option>`).join("");
}

// The map (ADR 0142): one line each where a member found something, the most confirmed first;
// what was found this week is marked new, and nobody's confirming for 30 days lets a line go.
function hqMapLines(s) {
  const lines = s.map_lines || [];
  if (!lines.length) return "";
  const week = Date.now() - 7 * 86400000;
  const row = (l) => `<li><span>${esc(l.line)}</span>${Number(l.at) > week ? ` <span class="hq-tag">new</span>` : ""}
      <small> ${esc(l.by || "")} · confirmed ${Number(l.uses) || 1} time${Number(l.uses) === 1 ? "" : "s"}</small></li>`;
  return `<h3>The map</h3><p class="hq-note">Where things are, as the team found them: the most confirmed first.</p>
    <ul class="hq-lines">${lines.map(row).join("")}</ul>`;
}

function hqWorldCard(s) {
  const w = s.world;
  if (!w) return "";
  const sections = (w.sections || []).map((p, i) => ({ p, i }))
    .sort((a, b) => (HQ_STAGE_ORDER[a.p.unconfirmed_since ? "observation" : a.p.stage] ?? 9) - (HQ_STAGE_ORDER[b.p.unconfirmed_since ? "observation" : b.p.stage] ?? 9));
  const sectionRow = ({ p, i }) => `<li><b>${esc(p.heading)}</b> ${hqStageTag(p)}
      <span>${esc(p.headline)}</span>
      <small> ${esc(p.address)} · revision ${Number(p.revision) || 1}, ${esc(p.written_by || "")}${p.warrant ? ` · warrant: ${esc(p.warrant)}` : ""}${p.why ? ` · ${esc(p.why)}` : ""}</small>
      <div class="hq-row">${hqWorldActs("section", i)}</div></li>`;
  const open = HQ_WORLD_VIEW.page;
  return `<section class="hq-card"><h2>World</h2>
    ${hqClamp("worldintro", "What the team knows: the house page's sections and timeline, and the lab and team pages HQ works out from the record. Edit rewrites a section.", "hq-note")}
    <div class="hq-row"><button class="hq-btn tall ok" aria-expanded="${hqWorldDoc().md || hqWorldDoc().open ? "true" : "false"}" data-act="hq:world:paste:show">Paste Markdown</button>
      <button class="hq-btn tall" data-act="hq:world:export:md">Download the world (Markdown)</button></div>
    ${hqWorldPaste()}
    ${hqMapLines(s)}
    ${open ? hqWorldPageBody(w, open, sections, sectionRow) : hqWorldPageList(w)}
    <div class="hq-row"><button class="hq-btn tall" aria-expanded="${HQ_WORLD_VIEW.writing ? "true" : "false"}" data-act="hq:world:writing:toggle">Write a section</button></div>
    ${HQ_WORLD_VIEW.writing ? `<h3>Write as the owner</h3>
      <p class="hq-note">Your words stand on the page you pick (the house, or an area, project, goal or role) as an insight; on the house page they show first in every briefing. Writing under a key that exists replaces that section; its old text stays in its history.</p>
      <div class="hq-row"><label>Page <select id="hq-w-page">${hqWorldPageOptions(w)}</select></label>
        <label>Key <input id="hq-w-key" maxlength="40" placeholder="nas"></label>
        <label>Title <input id="hq-w-title" maxlength="80" placeholder="The NAS"></label></div>
      <textarea id="hq-w-text" rows="4" autocapitalize="off" maxlength="1200" aria-label="Section text (first line is its headline)"></textarea>
      <div class="hq-row"><button class="hq-btn tall ok" data-act="hq:world:write:section">Write the section</button>
        <button class="hq-btn tall" data-act="hq:world:write:timeline">Add to the timeline</button></div>` : ""}
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
  if (kind === "page") { HQ_WORLD_VIEW.page = what === "all" ? null : what; render(); return true; }
  if (kind === "writing") { HQ_WORLD_VIEW.writing = !HQ_WORLD_VIEW.writing; render(); return true; }
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
        const page = val("hq-w-page") || "house";
        await api("POST", "/v1/hq/world/section", { page, key: val("hq-w-key"), title, text });
        flash(page === "house" ? "It is on the house page." : `It is on world:${page}.`, "ok");
      } else {
        await api("POST", "/v1/hq/world/timeline", { page: val("hq-w-page") || "house", text });
        flash("It is on the timeline.", "ok");
      }
    } catch (e) { flash("Couldn't write it: " + e.message, "err"); return true; }
    await loadHQ(); render(); return true;
  }
  return true;
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
  // The button at the top of the World card opens (or closes) the paste box and goes to it.
  if (op === "show") {
    doc.open = !doc.open; render();
    const again = document.getElementById("hq-wd-md");
    if (doc.open && again && again.focus) again.focus();
    return true;
  }
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
