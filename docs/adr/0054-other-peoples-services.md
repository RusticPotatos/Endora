# 0054 — Other people's services

## Status

Accepted (2026-07-26). **Consolidates 0021, 0038, 0039, 0040, 0041, 0042, 0043 and 0045**
(and, through 0041, the archived 0046–0048). The largest cluster, because this is where
almost every real failure has come from.

## Context

Endora is only as useful as the things it can reach, and everything it reaches belongs to
somebody else: a smart home, a calendar, a filesystem. Those services expose tools designed
for a different consumer — usually a voice assistant — and the gap between that surface and
what a butler needs is where the work is.

The evidence, from one house over one week: nine attempts to switch a light that failed
because the model kept inventing names; fourteen consecutive attempts at a light called
`Kitchen Table` with `names: Kitchen Table` in the model's context every single time, and
not once did it send that string; a call that switched on **every light in the house**; a
tool that can only set brightness being chosen repeatedly to switch a light on.

The wrong response to all of that is to patch each one. [0038] counted the result of doing
so: six hardcoded quirks and one integration's response format parsed inside the generic
protocol client.

## Decision

### A catalogue, with configuration and enablement

Capabilities — built in, or discovered from **MCP servers** Endora hosts — live in one
catalogue with per-capability configuration, enablement and settings. MCP tools are
namespaced `server.tool`, deny-by-default, and the person opens them individually.
"Off" means **not offered to the model at all**, for every kind of capability, from every
source.

### Facts about a tool are data, ranked by trust

Endora keeps a profile per capability, built the way it builds understanding of a person —
facts with provenance, correctable — from three sources, **ranked**:

1. **Declared** — what the server says about itself. A proposal, never authoritative: a
   careless or hostile server says exactly the same words as a good one.
2. **Observed** — what Endora saw happen, accumulated from outcomes
   ([0053](0053-honesty-about-what-it-did.md)).
3. **Confirmed** — what the person said. Authoritative.

Deterministic policy consumes confirmed facts. Declared facts are surfaced, never obeyed —
[models propose, policy authorizes](0051-where-the-boundary-is.md) applied to a third party.

The facts worth holding are deliberately few, and they come from **one question**: *which
tool reads this server's state?* That nomination makes the reader, and makes every other
tool on that server verifiable through it.

### Findings are derived, never stored

From outcome history alone, Endora derives what is wrong with its own tooling. A proposal is
**computed on read** — there is no table, no row to dismiss, nothing to groom. When the
outcomes age out or the world changes, the finding stops being derived. This is
[0052](0052-what-it-knows-about-you.md)'s anti-queue guarantee made structural rather than
promised, and **answering is the dismissal**.

Two findings, told apart by how *wide* a failure is — arithmetic that names no service:

| history | means | remedy |
| --- | --- | --- |
| fails on one target, works on others | the name is wrong | *what is it really called?* |
| **refuses** on several, has **never** worked | the tool is wrong | *stop offering it* |

Only outright refusals support a withdrawal, and **any** success disqualifies it — including
an unverified one. The first version counted "changed nothing" too, and immediately proposed
withdrawing the most useful tool in the house on five successes misread as failures.

The withdrawal is applied by policy without asking
([0051](0051-where-the-boundary-is.md)); the name question is the person's and always will be.

### Finding the thing the person meant

When a call fails, Endora searches the service's own reading for what the call was aiming at.
It always shows what it found; it acts only when one candidate is clearly the answer. Four
signals, each from a specific failure:

| signal | asks |
| --- | --- |
| matched words | how much of the request does this name contain? |
| leftover words | how much of *itself* does the request not account for? |
| operability | could the tool actually act on this? |
| uniqueness | does exactly one candidate survive? |

The first two rank, the third can only remove a candidate, the fourth is the gate. A tie acts
on nothing. Everything is **pure text** — a reading is fragments, a value is what follows a
colon, a candidate shares whole words — so a calendar is searched by the same code as a house.

A filter value the service has never used as a **category** is treated as part of what was
named: "kitchen table" arriving as `{area:"kitchen", device_class:["table"]}` has no name in
it at all, and acting on what remained switched on two lights. The service supplies the
vocabulary; where there is none, nothing changes.

Recovery **never widens a call**: kind filters are kept, and only scalars the real name
already contains are dropped. Written after argument hygiene turned an aimed-at-nothing call
into every light in the house.

### Direct reach, where it is given

A tool surface is a product decision, not the service. Where Endora holds the service's own
interface it uses it — to see **everything** (including what the tool surface hides) and to
act on exactly one thing **by id**, which cannot be mis-matched. All service-specific
knowledge lives in one named adapter; everything above it speaks to a generic port and never
learns which service it is.

Deliberately not "run arbitrary API calls": that is a far larger grant than any observed
failure needed.

### It can change the service's configuration, narrowly

Endora writes the names **the person confirmed** into the service that owns them, so the fix
works for every client rather than only for Endora. Strictly additive, never inventing a
name it merely inferred, off until deliberately enabled — and **every change is logged with
the prior value**, individually reversible. A prior value nobody stores is not a
reversibility story but a claim about one.

That log survives "forget everything": it is not knowledge about the person, it is a receipt
for a change that still exists inside somebody else's service, and deleting the receipt does
not undo the change.

### Quirks are allowed, behind a boundary

Some knowledge genuinely cannot generalise. It stays code — in that integration's **named
adapter**, never in the shared runner and never in the protocol client. The test: *could
another server reasonably need the opposite behaviour?* If yes, it is a quirk.

## Consequences

- Any MCP server can get read-back, evidence, findings and repair without Endora shipping a
  line of code about it.
- Onboarding an integration is a person answering one question, not a patch release.
- The name-matching failure class **ends** for any service Endora has reach into.
- **Four rules decide which thing a person meant.** Each came from an observed failure and
  each has a test against a real house. A fifth should be read as evidence that ranking names
  is the wrong instrument.
- Endora now holds credentials to a service and edits its configuration. The gates are:
  confirmed facts only, additive only, undo stored, off by default, disclosed.

## Rejected

- **Patching each integration.** Produced six hardcodes and a leak into the transport.
- **Trusting MCP annotations.** Taking an unvetted third party's word for what is safe.
- **Inferring tool behaviour from names** (`Get*`, `List*`). A heuristic that is usually right
  is the worst kind at a policy boundary.
- **Asking the model which of its tools are useless.** It is the component whose bad choice
  created the finding.
- **Storing findings so they can be dismissed.** A dismissible record is a queue with extra
  steps.
- **Writing names Endora merely inferred.** A recovery proves what worked for one call, not
  what a word means in a house.
- **Renaming a person's entities** instead of adding aliases. Destructive, and it presumes
  Endora's opinion beats theirs.
- **Requiring every word to match**, **preferring the shortest name**, **acting on the best of
  several tied candidates**, **dropping kind filters on retry**, **filtering non-controls out
  of the shortlist** — each tried, each wrong, each recorded in the archived originals.
