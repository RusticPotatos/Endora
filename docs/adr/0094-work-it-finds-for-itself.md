# 0094 — Work it finds for itself

## Status

Accepted, 2026-10-15, for Phases 1–3 of [the always-on plan](../plans/always-on-work.md).
The owner answered the plan's five open questions on that date; each answer is recorded
below. Later phases — more sources, mail, any unattended action — are **not** accepted
here and need their own records. Prepared with AI assistance (Claude).

## Context

The owner's aim is an Endora that is always doing something: when nothing has been asked
of it, it looks into the house and the things it already knows are wrong, and makes
progress on them, rather than waiting.

Most of the machinery already exists. A worker advances goals a bounded pass at a time,
under a lease, clamped to the reversible bands, with a console strip to see and stop them
(ADR 0079–0083). And four lists of real problems are already derived by code from stored
records — standing trouble, specimens that gave up (ADR 0075), repairs (ADR 0054) and
notions — none of which anything acts on. `gave_up_specimens()` was written for exactly
this and has no caller.

What is missing is the connector, and it was removed on purpose. [ADR 0081](0081-declare-work-and-search-for-sources.md)
retired failed-answer goal creation because "failed answers could silently become
speculative goals", leaving a goal to start only when the person declares one. That
objection is right about *speculation*. It does not apply to work whose trigger is a fact
code derived from the record.

Three things make adding self-started work unsafe as the worker stands today, each checked
in the code:

- It claims goals oldest-updated first with no notion of whose they are, and the
  three-goal cap is one shared pool. Self-started work would queue ahead of, and could
  exhaust the slots of, the person's own requests.
- Nothing caps the paid model. ADR 0091 *meters* deep-model use; a turn escalates through
  one call (`butler.deeper()`), and goal passes run through that same turn. Observed live:
  a goal pass against a refused source escalated twice per pass.
- Nothing says where a goal came from, so nothing can treat the two differently.

## Decision

**Work may start without being asked, only from a record code already derived — never from
a model's guess.** The first source is standing trouble (a thing in the house that has
stopped answering, with a clock). Specimens that gave up come next, then repairs. No
classifier decides anything here (ADR 0076), and nothing is created from a failed answer
(0081 stands on that).

**The source is the verifier.** Such a goal is done when the condition that started it no
longer holds, checked by code re-reading the same source: the trouble clears, the
specimen's replay passes the verdict that filed it, the repair stops being derived. A
model never declares its own work finished (ADR 0053/0077). If the budget runs out first,
the goal ends `Partial` with what it found, which is a result, not a failure.

**Two lanes, the person's first.** A goal records its origin, `Person` or `Record`. The
person lane keeps its cap of three; the record lane holds one. The cap counts within a
lane, so neither can exhaust the other, and the worker claims person goals before record
goals. An origin that cannot be read is treated as `Record`: the lane that never outranks
a request.

**Only when quiet.** A self-started goal begins only when no person goal is claimable, the
turn lock is free, the record lane is empty, nobody has spoken for a quiet window, and a
cooldown has passed since the last one ended. Starting values, to be tuned after a week
watching the activity trail: quiet window 10 minutes, cooldown 30 minutes, gate checked
every 5 minutes.

*As built (Phase 2).* The quiet window is applied each time the worker would claim a
self-started goal, on the worker's own tick; the empty lane, cooldown and 5-minute
cadence apply when a goal is *started*, and arrive with the thing that starts them in
Phase 3.

**Local model only.** A record-lane pass never reaches the paid model. This is enforced
structurally — it is handed a butler that has no deep model to escalate to — and not by a
setting. A budget is not built; revisit it only if a month of use shows the local model is
the limit.

**Gather and reversible only.** The existing reversible-only runner is unchanged. This
decision adds no unattended action. Anything that would act needs its own record and must
meet the bar `withdraw_what_never_works` documents: only ever undoes, a high threshold,
one click to reverse, said out loud.

**Speak rarely.** Record-lane goals do not post a chat message when they end. They write
to the activity trail and are folded into the next brief; they reach chat only when they
need the person. ADR 0056's budget on speaking is unchanged.

**Bounded per item.** Attempts per item are capped, a cooldown follows each ending, and
an item the person marks "it's fine" or stops is not started again until it recurs as a
new situation. Otherwise a permanently broken device would be investigated forever.

**Nothing to do means doing nothing.** If no source yields work the node idles. It does
not invent an objective to look busy.

## Owner decisions, 2026-10-15

1. The expansion freeze (`assistant-foundation-reset.md`) is **lifted for Phases 1–3
   only**. `make goal-eval` must not regress across Phase 1, and the freeze returns after
   Phase 3 until scenarios S1–S9 pass on the live node.
2. First source: standing trouble.
3. Results appear in the activity trail and the next brief.
4. Local model only.
5. The starting numbers above.

## What this amends and retires

- **[0081](0081-declare-work-and-search-for-sources.md)** — a goal may now start without
  being declared, but only from a derived record, never from a failed answer. Its retirement
  of failed-answer creation stands.
- **[0082](0082-announce-every-way-a-goal-ends.md)** — "every goal that ends is announced in
  chat" now applies to the person lane. The record lane goes to the trail and the brief.
- **Retires, in Phase 4:** the nightly loop's "replay one stumper" step, which does the
  same job once per night for one item. It stays until specimens become a source, so one
  item is never worked twice. No new worker and no new store are introduced: the record
  lane runs on the existing worker, which is the answer to the reset plan's rule against
  maintaining two autonomous brains.

## Not decided here

- **Intentions.** ADR 0052/0077 give the nightly loop its own durable pursuit, a second
  mechanism beside goals and research. Whether to fold it into the record lane must be
  settled before Phase 3. Phases 1 and 2 do not depend on it.
- **Mail as a source**, and any unattended action. Each needs its own record. Mail also
  needs the egress disguise taught its line shapes first.
- **A defect found while writing this.** The escalation door disguises the person's
  messages but not what a skill returned, so a tool result carrying a real name reaches
  the deep model unchanged. Local-only self-started work does not depend on the door, but
  the defect is real for every skill and is recorded here so it is not lost.

## Consequences

- A self-started goal can waste local-model time on something that turns out not to
  matter. The bounds (one slot, per-item attempts and cooldown, the quiet gate) cap that
  at a small, visible amount, and every such goal shows in the console strip with a Stop.
- One GPU serves chat and background work. Whether the quiet gate and the single slot are
  enough is a Phase 3 measurement and not an assumption.
- Phase 1 is deliberately behaviour-free: it adds the origin, the lanes and the claim
  order, and nothing creates a record goal until Phase 3. Its evidence is that the person
  lane behaves exactly as before, and that a record goal, once one exists, can neither
  outrank nor crowd out the person's.
