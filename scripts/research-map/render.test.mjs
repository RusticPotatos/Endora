// The research map's checkpoint renderer (the owner's decision M5b): a still page stamped with the
// snapshot's own time, drawn from org.json and a snapshot the steward writes. Run: node --test scripts/research-map
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { render } from "./render.mjs";

const org = JSON.parse(readFileSync(new URL("./org.json", import.meta.url), "utf8"));
const snap = () => ({
  as_of: "2026-11-07T17:06:00Z",
  owner_asks: ["MR7: which apps to keep on the rebuilt Mac", "Seven product-split decisions (deferred)"],
  threads: [
    { title: "Steward: climb 3", role: "steward", status: "completed", last_seen: "2026-11-07T17:04:00Z" },
    { title: "Worker: example task", role: "workers", status: "running", last_seen: "2026-11-07T17:04:00Z" },
    { title: "Worker: Constraint <challenge>", role: "workers", status: "completed", last_seen: "2026-11-07T16:53:00Z", blocked: "its paid run was stopped ~16:42" },
    { title: "Side project", role: "product", status: "running", last_seen: "2026-11-07T17:04:00Z" },
    { title: "A thread with no role", role: "nobody", status: "idle", last_seen: "2026-11-07T15:00:00Z" },
  ],
  handoffs: [
    { t: "2026-11-07T14:38:00Z", from: "director", to: "steward", kind: "recommendation accepted", approx: false },
    { t: "2026-11-07T17:00:00Z", from: "product", to: "steward", kind: "decisions MR1-MR7", approx: true },
    { t: "2026-11-07T17:50:00Z", from: "steward", to: "workers", kind: "task MR9", approx: true },
  ],
  evidence: ["close-interlock fix released b64f6caa"],
});

test("the page says when its snapshot was taken, in the title and at the top", () => {
  const html = render(org, snap());
  assert.match(html, /As of 7 Nov 2026, 17:06 UTC/);
  assert.match(html, /<title>Research map, as of 7 Nov 2026, 17:06 UTC<\/title>/);
});

test("a snapshot without its time is refused, never stamped with today's", () => {
  const s = snap(); delete s.as_of;
  assert.throws(() => render(org, s), /as_of/);
});

test("nothing on the page moves: no animation of any kind", () => {
  const html = render(org, snap());
  assert.doesNotMatch(html, /animation|@keyframes|requestAnimationFrame|setInterval/);
});

test("every role and every relationship in org.json is drawn, so the map follows the org", () => {
  const html = render(org, snap());
  for (const r of org.roles) assert.ok(html.includes(`data-role="${r.id}"`), r.id);
  for (const l of org.relations) assert.ok(html.includes(l.label), l.label);
  const moved = structuredClone(org);
  moved.roles.push({ id: "reviewer", name: "Second Reviewer", tier: 2, col: 1, does: "reads results" });
  moved.relations.push({ from: "workers", to: "reviewer", label: "For review" });
  const changed = render(moved, snap());
  assert.ok(changed.includes('data-role="reviewer"') && changed.includes("For review"));
});

test("a running thread says it was running at the snapshot time, not that it runs now", () => {
  const html = render(org, snap());
  assert.match(html, /running at 17:06/);
  assert.doesNotMatch(html, /running now/i);
});

test("the blocker is shown with its cause, and text stays text", () => {
  const html = render(org, snap());
  assert.ok(html.includes("Worker: Constraint &lt;challenge&gt;"));
  assert.match(html, /class="[^"]*rm-blocked/);
  assert.ok(html.includes("its paid run was stopped ~16:42"));
});

test("a handoff written later than the snapshot is not drawn, and is said apart", () => {
  const html = render(org, snap());
  assert.ok(!html.includes("task MR9\"") && !/Assignments · [^<]*task MR9/.test(html));
  assert.match(html, /written later than the snapshot/);
  assert.ok(html.includes("task MR9"));
});

test("hand-written handoff times are marked approximate on the line they label", () => {
  const html = render(org, snap());
  assert.ok(html.includes("~17:00 decisions MR1-MR7"));
  assert.ok(html.includes("14:38 recommendation accepted"));
});

test("a thread whose role is not in org.json is shown as unplaced, never guessed into the chart", () => {
  const html = render(org, snap());
  assert.match(html, /Unplaced[\s\S]*A thread with no role/);
});

test("what waits on the owner is on the owner's card", () => {
  const html = render(org, snap());
  assert.match(html, /data-role="owner"[\s\S]*MR7: which apps to keep/);
});
