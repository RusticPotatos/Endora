# 0100 — The clean slate

## Status

Accepted, 2026-10-16, by the owner through a design interview (step 1 of
[the always-on butler plan](../plans/always-on-butler.md), questions 3 to 6). Prepared with AI
assistance (Claude).

This supersedes the parts of earlier records that built what is removed here; each of them
carries a dated note saying so. Nothing in [the constitution](../constitution.md) changes. The
removal of standing trouble, earlier the same day, is recorded in
[0056](0056-how-it-behaves-toward-you.md).

## Context

Over three months Endora grew ways of working on its own: beliefs formed from conversation,
intentions taken up from them, a daily check-in, a nightly loop that thought about the day and
replayed what it had failed, notions it was still wondering about, a self-tune at 4 am, an
own-work lane that started goals for itself, and durable goals started from chat. Each was
recorded and reasoned about when it was built.

Used for real, the owner found the output to be noise: an About you screen of stale or wrong
beliefs ("appointment tomorrow" weeks later), the same intention four times over, an Inbox of
messages nobody asked for, and the own-work lane re-asking questions that had already failed.
The sources were wrong, a 7B model was judging what mattered, and nothing knew what the owner
actually cares about. The owner's answer was to "just start fresh": remove what produced the
noise, and build the always-working butler again on real signals (the plan's later steps).

## Decision

**Removed, code and storage:**

- The **daily check-in**, and waking early on an unusual change
  ([0063](0063-waking-on-the-world.md)).
- The **nightly loop**, **notions** and the replay of failed asks: **specimens**
  ([0057](0057-thinking-between-turns.md), [0075](0075-failures-become-specimens.md)).
- **Intentions** formed from beliefs and their nudges. Intentions remain only as the record
  of research the person asked for ([0077](0077-seeing-something-through.md)).
- The **own-work lane** and its `ENDORA_OWN_WORK` switch, with the record lane's house-only
  and local-only wrappers ([0094](0094-work-it-finds-for-itself.md),
  [0095](0095-more-sources-for-work-it-finds.md)).
- **Durable goals**: the chat's intake tool, the goal worker, checkpoints and their leases,
  the schedule verifier, the `/v1/goals` routes and the chat's goal strip
  ([0079](0079-goal-checkpoints-require-ownership.md) to
  [0083](0083-show-unattended-work-and-let-it-be-stopped.md)).
- The **rhythm finder**, whose only caller turned house patterns into beliefs
  ([0096](0096-patterns-it-notices.md)).
- The **4 am model tune**'s schedule. The comparison runs only when started by hand
  ([0055](0055-the-model-layer.md)).
- **Inbox**: messages Endora sends unasked stay in the chat, and Activity is the one record.
- **About you** as a list of beliefs by kind with confidence and "That's right / Not quite".
  It now says how much Endora has learned, offers to forget all of it, and keeps what the
  person stated themselves.

The heartbeat no longer writes device changes to Activity. Beliefs whose kind is an intent or
"other" are no longer formed, since they were the passing remarks that went stale.

**Kept:** the morning brief, research the person asks for (one card while it runs, the
result as a reply), mail reading in quiet time, stated preferences, and the outcome record of
what Endora did. The quiet-time rule that paces mail moves to its own module.

**Storage.** An existing database drops the removed tables when the node starts: the check-in,
nightly-loop and model-tune schedules, notions and their citations, standing trouble,
specimens, and goals with their leases (research keeps its own). Earlier passes had left these
tables in place, unread. That was wrong: a table nothing reads is data nothing can export or
delete, which is outside the memory rights in constitution §6. A fresh database never creates
them.

**Starting fresh.** `POST /v1/understanding/forget` (Settings › About you › Forget, which
needs confirming) deletes every learned belief and every intention Endora formed from one.
What the person told it, and research they asked for, stay.

**The API.** The routes for the removed features are gone (`/v1/checkin`,
`/v1/nightly-loop/schedule`, `/v1/model-tune/schedule`, `/v1/notions`, `/v1/intentions`,
`/v1/goals`), and the export no longer has a `goals` field. A chat's `request_id` is still checked and a malformed one is still refused;
nothing is keyed by it any more. These are breaking changes, marked as such in the commits and
the changelog.

## Consequences

- Endora no longer does anything unasked except the morning brief and reading the mail. That
  is deliberate and temporary: the agenda loop (plan step 3) brings back unattended work, from
  real signals, with Activity as its record and the owner deciding what it may handle.
- About 16,000 lines and 260 tests go. What remains is easier to reason about before the next
  steps add to it.
- Anything a removed feature knew is gone on the next start. The owner asked for exactly this.
- The ideas are not lost: each superseded record keeps its reasoning, and the agenda loop is
  to borrow what held up (bounded passes, completion from tool observations, quiet before
  starting, announcing every ending).

## Alternatives considered

- **Switching the features off and keeping the code.** Rejected by the owner: off-but-present
  code still has to be kept compiling and understood, and it had already produced the noise
  once.
- **Overhauling About you and goals in place.** Rejected for now: their sources were the
  problem, and the markdown brain (plan step 2) replaces what About you was trying to be.
- **Leaving old tables unread**, as the first passes did. Rejected for the memory-rights
  reason above.
