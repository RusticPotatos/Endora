// The research map as a checkpoint page (the owner's decision M5b, 7 Nov 2026): who is working on
// what, drawn from org.json (the roles and who sends what to whom) and a snapshot the steward
// writes at a checkpoint (threads, handoffs, what waits on the owner, verified evidence). The page
// says when its snapshot was taken and nothing on it moves: it is a still picture of that moment,
// published as an artifact, never a live view. No dependency; the data stays outside the repo.
//
//   node scripts/research-map/render.mjs <snapshot.json> [out.html]   (org.json beside this file)

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const hhmm = (d) => `${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}`;
const stamp = (d) => `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}, ${hhmm(d)} UTC`;

/** How a thread's status reads at the snapshot: a picture of that moment, never of now. */
function status(t, asOf) {
  if (t.status === "running") return `running at ${hhmm(asOf)}`;
  if (t.status === "completed") return "between turns";
  return t.status;
}

function threadCard(t, asOf) {
  const seen = t.last_seen ? ` · last seen ${hhmm(new Date(t.last_seen))}` : "";
  const cls = `rm-thread rm-${t.status === "running" ? "run" : "still"}${t.blocked ? " rm-blocked" : ""}`;
  return `<li class="${cls}"><b>${esc(t.title)}</b><span>${esc(status(t, asOf))}${seen}</span>${t.blocked ? `<em>Blocked: ${esc(t.blocked)}</em>` : ""}${t.note ? `<small>${esc(t.note)}</small>` : ""}</li>`;
}

/**
 * The page. `org`: { roles: [{ id, name, tier, col, group?, does }], relations: [{ from, to, label }] }.
 * `snap`: { as_of, threads: [{ title, role, status, last_seen, blocked?, note? }],
 *           handoffs: [{ t, from, to, kind, approx }], owner_asks: [..], evidence: [..] }.
 */
export function render(org, snap) {
  if (!snap || !snap.as_of) throw new Error("the snapshot has no as_of: a checkpoint page must say when it was taken");
  const asOf = new Date(snap.as_of);
  if (Number.isNaN(asOf.getTime())) throw new Error(`as_of is not a time: ${snap.as_of}`);
  const ids = new Set(org.roles.map((r) => r.id));
  const threads = snap.threads || [];
  // A handoff whose written time is after the snapshot cannot have happened by then: said apart.
  const handoffs = snap.handoffs || [];
  const drawn = handoffs.filter((h) => new Date(h.t) <= asOf);
  const later = handoffs.filter((h) => new Date(h.t) > asOf);
  const when = (h) => `${h.approx ? "~" : ""}${hhmm(new Date(h.t))}`;
  const cards = org.roles.map((r) => {
    const mine = threads.filter((t) => t.role === r.id);
    const blocked = mine.some((t) => t.blocked);
    const extra = r.id === "owner" && (snap.owner_asks || []).length
      ? `<ul class="rm-asks">${snap.owner_asks.map((a) => `<li>${esc(a)}</li>`).join("")}</ul>`
      : r.id === "evidence" && (snap.evidence || []).length
        ? `<ul class="rm-asks">${snap.evidence.map((e) => `<li>${esc(e)}</li>`).join("")}</ul>` : "";
    return `<section class="rm-role${r.group ? " rm-group" : ""}${blocked ? " rm-blocked" : ""}" data-role="${esc(r.id)}" style="grid-row:${Number(r.tier)};grid-column:${Number(r.col)}">
      <h2>${esc(r.name)}</h2><p class="rm-does">${esc(r.does || "")}</p>${extra}
      ${mine.length ? `<ul class="rm-threads">${mine.map((t) => threadCard(t, asOf)).join("")}</ul>` : ""}</section>`;
  }).join("");
  const unplaced = threads.filter((t) => !ids.has(t.role));
  const lines = org.relations.map((l) => {
    const h = drawn.filter((x) => x.from === l.from && x.to === l.to).map((x) => `${when(x)} ${x.kind}`);
    return { from: l.from, to: l.to, label: l.label, said: h.join("; ") };
  });
  // Recorded handoffs the org does not name still happened: drawn dotted, never dropped.
  for (const x of drawn) {
    if (!org.relations.some((l) => l.from === x.from && l.to === x.to) && ids.has(x.from) && ids.has(x.to)) {
      lines.push({ from: x.from, to: x.to, label: "", said: `${when(x)} ${x.kind}`, extra: true });
    }
  }
  const linkList = lines.map((l) => `<li data-handoff="${esc(l.from)}>${esc(l.to)}"${l.said ? ' class="rm-said"' : ""}>${esc(l.from)} → ${esc(l.to)}: ${esc(l.label || "recorded handoff")}${l.said ? ` · ${esc(l.said)}` : ""}</li>`).join("");
  const running = threads.filter((t) => t.status === "running").length;
  const blockedN = threads.filter((t) => t.blocked).length;
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Research map, as of ${esc(stamp(asOf))}</title>
<style>
:root{--bg:#f4f2ee;--panel:#fff;--line:#ddd6ca;--fg:#1f1d29;--muted:#6b6577;--green:#1f8a55;--red:#d8434a;--amber:#b86a0f;--blue:#3f74d6}
@media (prefers-color-scheme:dark){:root{--bg:#14130f;--panel:#1d1b17;--line:#34302a;--fg:#ece8e0;--muted:#a39d92;--green:#5fc68f;--red:#f07a7f;--amber:#e3a85a;--blue:#86a8ee}}
body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.45 ui-monospace,Menlo,monospace;padding:16px}
.rm{max-width:1240px;margin:0 auto;display:grid;gap:14px}
h1{font-size:18px;margin:0}.rm-asof{font-weight:700;color:var(--amber)}
.rm-tiles{display:flex;flex-wrap:wrap;gap:8px}.rm-tiles span{border:1px solid var(--line);background:var(--panel);border-radius:8px;padding:6px 10px}
.rm-chart{position:relative;display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:56px 22px}
.rm-role{position:relative;z-index:1;background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:10px 12px;min-width:0}
.rm-group{border-style:dashed}.rm-blocked{border-color:var(--red)}
.rm-role h2{font-size:14px;margin:0}.rm-does{margin:2px 0 6px;color:var(--muted);font-size:12px}
.rm-threads,.rm-asks{list-style:none;margin:0;padding:0;display:grid;gap:6px}
.rm-asks li{border-left:3px solid var(--amber);padding-left:6px}
.rm-thread{border:1px solid var(--line);border-radius:8px;padding:6px 8px;display:grid;gap:2px}
.rm-thread span,.rm-thread small{color:var(--muted);font-size:12px}.rm-thread.rm-run{border-color:var(--green)}
.rm-thread.rm-run span{color:var(--green)}.rm-thread em{color:var(--red);font-style:normal;font-size:12px}
svg.rm-wires{position:absolute;inset:0;width:100%;height:100%;z-index:0;overflow:visible}
.rm-wires path{fill:none;stroke:var(--line);stroke-width:2}.rm-wires path.rm-said{stroke:var(--blue)}.rm-wires path.rm-extra{stroke-dasharray:3 4}
.rm-wires text{fill:var(--muted);font:11px ui-monospace,Menlo,monospace}.rm-wires text.rm-said{fill:var(--blue)}
.rm-list{font-size:12px;color:var(--muted)}.rm-list li.rm-said{color:var(--blue)}
@media (max-width:760px){.rm-chart{grid-template-columns:minmax(0,1fr);gap:12px}.rm-role{grid-row:auto!important;grid-column:auto!important}svg.rm-wires{display:none}}
</style></head><body><main class="rm">
<h1>Research team: who is working on what</h1>
<p class="rm-asof">As of ${esc(stamp(asOf))}. A still picture of that moment, not live: nothing on this page moves.</p>
<div class="rm-tiles"><span>${running} thread${running === 1 ? "" : "s"} running at ${hhmm(asOf)}</span><span>${blockedN} blocked</span><span>${(snap.owner_asks || []).length} waiting on you</span></div>
<div class="rm-chart">${cards}<svg class="rm-wires" aria-hidden="true"></svg></div>
${unplaced.length ? `<section class="rm-role"><h2>Unplaced</h2><p class="rm-does">Threads whose role is not in the org</p><ul class="rm-threads">${unplaced.map((t) => threadCard(t, asOf)).join("")}</ul></section>` : ""}
<section class="rm-list"><h2>Lines and recorded handoffs</h2><ul>${linkList}</ul>
${later.length ? `<p>Handoffs written later than the snapshot (hand-written times; not drawn): ${later.map((h) => esc(`${h.from} → ${h.to} ${when(h)} ${h.kind}`)).join("; ")}</p>` : ""}
<p>Handoff times marked ~ are hand-written and approximate.</p></section>
</main>
<script>
// Draws each line once, between the cards as they were laid out; it never redraws or moves.
(function(){var L=${JSON.stringify(lines).replace(/</g, "\\u003c")};var c=document.querySelector(".rm-chart"),s=c.querySelector("svg");
function draw(){if(getComputedStyle(s).display==="none")return;var b=c.getBoundingClientRect(),h="";
L.forEach(function(l){var a=c.querySelector('[data-role="'+l.from+'"]'),z=c.querySelector('[data-role="'+l.to+'"]');if(!a||!z)return;
var p=a.getBoundingClientRect(),q=z.getBoundingClientRect(),down=q.top>p.bottom-1,up=p.top>q.bottom-1;
var two=L.some(function(o){return o.from===l.to&&o.to===l.from}),off=two?(l.from<l.to?-16:16):0;
var x1=p.left+p.width/2-b.left+off,y1=(down?p.bottom:up?p.top:p.top+p.height/2)-b.top,x2=q.left+q.width/2-b.left+off,y2=(down?q.top:up?q.bottom:q.top+q.height/2)-b.top;
if(!down&&!up){x1=(q.left>p.left?p.right:p.left)-b.left;x2=(q.left>p.left?q.left:q.right)-b.left;}
var my=(y1+y2)/2,d=down||up?"M"+x1+","+y1+" V"+my+" H"+x2+" V"+y2:"M"+x1+","+y1+" H"+x2,cls=(l.said?"rm-said ":"")+(l.extra?"rm-extra":"");
h+='<path class="'+cls+'" d="'+d+'"/><text class="'+cls+'" x="'+((x1+x2)/2+4)+'" y="'+(my+(off>0?14:-4))+'">'+(l.label+(l.said?" · "+l.said:"")).replace(/[&<>]/g,function(ch){return{"&":"&amp;","<":"&lt;",">":"&gt;"}[ch]})+'</text>';});
s.innerHTML=h;}draw();addEventListener("resize",draw);})();
</script></body></html>`;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const [snapPath, out] = process.argv.slice(2);
  if (!snapPath) { console.error("usage: node scripts/research-map/render.mjs <snapshot.json> [out.html]"); process.exit(2); }
  const org = JSON.parse(readFileSync(new URL("./org.json", import.meta.url), "utf8"));
  const html = render(org, JSON.parse(readFileSync(snapPath, "utf8")));
  if (out) writeFileSync(out, html); else process.stdout.write(html);
}
