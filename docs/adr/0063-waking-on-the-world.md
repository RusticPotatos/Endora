# 0063 — Waking on the world

## Status

Accepted (2026-08-03). The gap [0057](0057-thinking-between-turns.md) named and left —
*"nothing in this system is yet triggered by the world changing"* — closed, under the
widened autonomy grant.

## Context

Everything proactive in this system happens on a clock. The brief has an hour, the nightly
pass has an hour, and the check-in has a cadence: a timer fires, the butler looks around,
and it decides whether it has something to say ([0056](0056-how-it-behaves-toward-you.md)).

Meanwhile the watch loop records the world changing every two minutes. The transition log
knows the moment a door first opens, a device first appears, a sensor does something it has
never done — and nothing *wakes* on any of it. A butler that notices the unusual thing at
14:02 and mentions it at 18:00, because 18:00 is when its clock next fired, was paying
attention and acting like it wasn't.

This was the last named North-Star gap: the decision to reach out was still a clock.

## Decision

### A rare change spends the check-in budget early

When a watch pass records a transition whose key has **rarely changed in the whole
fortnight the log holds**, the check-in's `next_at` is pulled forward to now. The next
heartbeat tick runs the same `consider_reaching_out` that a scheduled check-in runs —
same agentic turn, same reversible-only clamp, same rule that **silence is the default**
and the budget is spent whether or not it speaks.

That reuse is the design. The wake adds no new way to speak, no new prompt, and no new
permission — it moves the *when* of a conversation the person already allowed. The
transition itself reaches the turn through the fact stream
([0059](0059-one-fact-source-many-consumers.md)), so the butler wakes already knowing what
woke it.

### Rarity is arithmetic, and it is the whole trigger

A transition is worth waking for when its key has changed **at most `RARELY` times** in
the fortnight, counting itself. Everything about that is derived from the log this system
already keeps:

- The hallway light has two hundred transitions: never a wake.
- A person coming home has dozens: never a wake.
- A sensor that has said nothing all fortnight and just spoke: a wake.

No keyword list, no per-integration salience table, no model judgement about what is
"important" — a name-based rule is the per-skill patch [0054](0054-other-peoples-services.md)
forbids, and a model deciding when to interrupt is the enforcement boundary
[0051](0051-where-the-boundary-is.md) forbids. Rarity is universal: it works for an
integration nobody has written yet, in a house nobody has seen.

### The person's dial still governs

No check-in schedule, no waking — the wake advances a budget that must exist. Turning
check-ins off turns the waking off with them, in the same switch, with nothing new to
learn. The dwell filter (five minutes, [0059]) still smooths flapping before anything
reaches the log, and first sight of a new key is a *noting*, not a transition — so a new
server's whole catalogue arriving at once wakes nothing.

### Bounded twice more

- **One wake consideration per pass**: the first qualifying transition wins; the rest are
  in the fact stream anyway.
- **A cooldown between wakes** (`WAKE_COOLDOWN_MS`, one hour), held in memory. A restart
  forgets it, which is at worst one extra consideration per restart — accepted, because
  the alternative is a stored timestamp whose only job is to be one.

## Consequences

- **The butler reaches out because something happened**, minutes after it happened, and
  can say which thing — the activity trail records the wake and its reason.
- The most this adds is one model consideration per cooldown window, each of which may
  conclude in silence. The person's experience of a false wake is *nothing*.
- Rarity misses the unusual **timing** of a common thing — the front door at 3am has a
  chatty key and will not wake. That is the honest v1 boundary; time-conditioned rarity is
  a later record if it earns one.
- A genuinely new sensor's first few changes will wake once. That is the desired
  behaviour, not a false positive: *new thing in the house* is exactly worth a word.

## Rejected

- **Letting the model watch the stream and decide when to interrupt.** The interruption
  decision is consequential, and the model is never the enforcement boundary.
- **A per-key or per-kind importance table.** The per-integration patch, again. It would
  be stale by the second integration.
- **A separate wake pipeline with its own way of speaking.** Everything the check-in
  already guarantees — silence default, budget spend, reversible-only, activity trail —
  would need a second copy, and second copies drift.
- **Pushing a notification instead of considering a check-in.** A push is louder than the
  evidence: the right first reaction to "something unusual moved" is the butler *looking*,
  and only then speaking if there is something to say.
- **Persisting the cooldown.** A stored timestamp whose only job is to exist, protecting
  against one extra look per restart.
