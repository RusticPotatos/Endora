
// ---- Endora HQ: the team's world (ADR 0122) -------------------------------------------------
// What the team knows, by stage: the house page's sections (canon first), its dated timeline,
// the lab and team pages HQ derives, and the stories. Only the owner makes something canon or
// demotes it, here or from a proposal on the desk. Every word is a member's or the record's, so
// all of it is escaped. The whole world downloads as one Markdown file.

const HQ_STAGE_ORDER = { canon: 0, insight: 1, observation: 2, field_note: 3, superseded: 4, retired: 5 };

// A piece's stage as shown: its own, or "unconfirmed since <date>" once its evidence is old.
function hqStageTag(p) {
  const shown = p.unconfirmed_since ? "observation" : String(p.stage || "observation");
  return `<span class="hq-tag stage-${esc(shown)}">${esc(p.said || p.stage || "")}</span>`;
}

// The owner's two acts on a piece: make it canon, or demote canon saying why.
function hqWorldActs(kind, i, p) {
  if (p.stage === "canon") return `<button class="hq-btn no" data-act="hq:world:${kind}:demote-${Number(i)}">Demote…</button>`;
  if (p.stage === "retired" || p.stage === "superseded") return "";
  return `<button class="hq-btn ok" data-act="hq:world:${kind}:promote-${Number(i)}">Make canon</button>`;
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
    <h3>The house page</h3>
    <ul class="hq-team hq-world">${sectionRows || `<li class="hq-empty">No sections yet: the CEO writes them from the timeline.</li>`}</ul>
    <details><summary>Its timeline (${timeline.length})</summary><ul class="hq-thread hq-timeline">${timelineRows || `<li class="hq-empty">Nothing yet.</li>`}</ul></details>
    ${derived}
    <h3>Stories</h3>
    <ul class="hq-team hq-world">${stories || `<li class="hq-empty">No stories yet.</li>`}</ul>
    <div class="hq-row"><button class="hq-btn" data-act="hq:world:export:md">Download the world (Markdown)</button></div></section>`;
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
