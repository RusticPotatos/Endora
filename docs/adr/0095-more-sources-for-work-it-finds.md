# 0095 — More sources for the work it finds

## Status

Accepted, 2026-10-15, on the owner's instruction to start Phase 4 of
[the always-on plan](../plans/always-on-work.md), after Phase 3 (standing trouble) was
built. It extends [0094](0094-work-it-finds-for-itself.md), whose rules all still hold:
derived work only, the source is the verifier, two lanes with the person's first, quiet
before starting, quiet in chat, bounded per item, idle when there is nothing. Nothing here
adds an unattended action. Prepared with AI assistance (Claude).

**Superseded 2026-10-16 by [0100](0100-the-clean-slate.md)**, with
[0094](0094-work-it-finds-for-itself.md).

## Context

Phase 3 made one thing Endora can look into on its own: a device that has stood in
trouble. Two more lists of real problems are derived by code and not acted on by anything
in the lane, and the owner decided on 2026-10-15 (recorded in 0094) that the nightly loop's
own long-running intention is folded into the lane rather than kept beside it.

Reading the code for each, they are different in kind, and the difference decides the design.

- **A specimen** is an ask the butler failed. "Working on it" is not an investigation: it is
  asking the same thing again against today's machinery and judging the answer by the
  deterministic verdict that filed it. The nightly loop does this for one specimen a night.
- **A repair** is a skill that claims success and changes nothing. It already ends in one of
  two places: a policy action (`withdraw_what_never_works` turns off a skill that has never
  once worked), or a question only the person can answer (which name did they mean).
  There is nothing left for a goal to gather.
- **The nightly intention** is a belief Endora is most sure of, pursued over up to seven
  nights as a prose note, with the person's one verb being to drop it.

## Decision

**Specimens become a source, and their work is a replay, not a goal pass.** A specimen
that has waited a day since it was filed or last replayed, and is still open, becomes a
record-lane goal whose completion criterion is the specimen itself. The worker does not run
the open-ended goal loop on it: it asks the person's question again, once, through the same
tool turn the nightly loop used, and judges the answer by the same verdict. Passing
completes the goal and retires the specimen; failing ends the goal `Partial` and records the
failure on the specimen, which gives up after the same number of failed replays as before.
No model says it passed. The specimen, not the goal, is the record of how many attempts there
have been, so an ask is never replayed more often than the nightly loop did.

A replay is allowed the same skills the nightly replay had (the reversible, read-only
runner), because it is the person's own ask and a replay denied the skills the ask needs
would always fail and falsely prove "nothing I have can answer it", which is the signal
[0071](0071-capabilities-it-writes-itself.md) draws recipe proposals from. It is held to the
local model, like everything in the lane. That changes what "gave up" means by a small
amount: it now means the local model could not answer, which is the question being asked.

**Repairs do not become a source.** They are derived and already acted on or put to the
person; a goal would add a model call and nothing to learn. Revisit only if a repair ever
arises that has something to read.

**The nightly intention becomes the last source: a belief.** After standing trouble and
specimens, and only when neither has anything, the belief Endora is most sure of becomes a
goal, chosen by the arithmetic the nightly loop already used. It runs as an ordinary lane
goal, so it is bounded by the goal's own pass budget (replacing seven nights), by the lane's
single slot (replacing "one active"), and by the per-item attempts and cooldown (replacing
the churn 0077 noted, where a retired intention could be formed again from the same belief
with no memory of what it found). It records which belief it came from. It cannot be
completed by its source, so it ends `Partial` and a model never declares it done.

This is the one source that is not purely a record: the belief was formed by a model from
conversation. 0052 accepted pursuing one on those terms and this keeps that and no more.
It is also the one source that reaches outside the house. The nightly intention used the
web and the encyclopedia to look into a topic the person cares about, and folding it in
without that would leave it nothing to do. So a belief goal is offered the same reversible,
read-only skills the intention had, on the local model only. What leaves the house is a query
built from a belief about the person, exactly as before, and it is the reason this source is
last and bounded. It also keeps as a finding only what a read supports.

Standing trouble keeps the narrower rule from 0094: the house only.

**The old paths retire behind the switch, not before it.** The nightly loop stops replaying
a stumper and stops forming and continuing an intention **only when the lane is on**
(`ENDORA_OWN_WORK=1`). With the switch off, the nightly loop behaves exactly as it did, so
deploying this changes nothing for a node that has not turned the lane on, and there is no
interval in which neither mechanism runs. Once the lane has been on and observed, the old
paths and the condition are deleted. An intention already active when the lane is turned on
is left to expire by the existing rules (it no longer advances); it is not carried across.

## What this amends and retires

- **[0075](0075-failures-become-specimens.md)** — the nightly replay moves to the lane when
  the lane is on. The shelf, the verdict, the give-up count and the trail wording are
  unchanged.
- **[0052](0052-what-it-knows-about-you.md)** and **[0077](0077-seeing-something-through.md)**
  — the nightly intention is replaced as described, when the lane is on.
- **[0094](0094-work-it-finds-for-itself.md)** — "gather from the house only" was written
  for standing trouble. Specimens and beliefs have the skills above; the rest of 0094's
  rules apply to every source.

## Not decided here

- **Repairs as a source,** see above.
- **The nightly note's thread.** The overnight note used to carry the intention's notes from
  night to night. With the intention in the lane, the note no longer knows what was found
  there. Putting the lane's results into the brief and the note is Phase 5.
- **Mail,** and any unattended action, as in 0094.

## Consequences

- With the switch off nothing changes. With it on, a stumper is replayed in idle time
  rather than at a fixed night hour, at most once a day each, and a belief is looked into
  only when nothing is wrong in the house and no ask is waiting to be replayed.
- A node with no Home Assistant has no standing troubles, so on such a node the lane is
  specimens and one belief at a time.
- A belief goal's queries leave the house. It is bounded, last and local-model-only, and it
  shows in the strip with a Stop, but it is the least private thing the lane does.
