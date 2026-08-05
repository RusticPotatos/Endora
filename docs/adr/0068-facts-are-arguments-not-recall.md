# 0068 — Facts are arguments, not recall

## Status

Accepted (2026-08-04). Names the principle that [0065](0065-a-place-is-not-the-models-to-remember.md),
[0066](0066-their-verdict-decides-too.md) and [0067](0067-one-way-to-the-deep-model.md) each
applied to one fact, and ships its next two applications.

## Context

Four records in four days, one shape:

| Failure | The fact | Where it was |
| --- | --- | --- |
| "Your daily brief for New York" ×4 | where they live | in the prompt, as prose |
| Unhelpful tool kept acting unasked | whether it helped | in the prompt, as prose |
| Generic template from the deep model | who is asking, from where | absent entirely |
| "Morning, sir" at half past one, ×4 | what time it is; what was already said | in the prompt, as prose |

In every case the system **held** the fact — in preferences, in outcomes, in the clock, in
the chat history — and handed it to a 7B model as text to weigh, alongside everything else it
weighs badly. The constitution already said *models propose; deterministic policy authorizes*.
What it did not say is that **proposing quietly included recalling**, and recall is where all
four failures lived.

The check-in that produced the fourth row is the clearest specimen. Its prompt says, in so
many words: *open with it plainly — not a greeting*. The model opened with "Morning, sir"
four times in one day, the last at 13:35, each message re-raising the same lights complaint
in slightly different words. Both violated facts — the hour, and what had already been said —
sat in code's hands the whole time. The instruction was in the prompt. **An instruction the
model must remember is not a rule.**

## Decision

### The principle

> A fact the system holds is supplied or enforced by code. The model composes prose around
> facts; it is never the memory for them.

This is [0053](0053-honesty-about-what-it-did.md)'s "the guarantee goes in code" applied to
data rather than behaviour. The division of labour it implies: code owns every load-bearing
fact in a message — the place, the hour, what changed, what was already said — and the model
owns only the connective prose. A confused model can then write awkwardly *around* correct
facts, which is an embarrassment; it can no longer state wrong ones the system knew better
about, which was the failure.

### Applied now: a check-in cannot repeat itself

What has been said since the person last spoke is a fact the chat history holds. Before a
check-in posts, it is compared — by meaning, via the same sameness test beliefs use, because
the four live repeats were each reworded — against every butler message since the person's
last reply. A repeat is not sent, and the activity trail says so.

The window resets when the person speaks: an unacknowledged concern raised again in a new
conversation is service; raised four times into silence it is nagging.

### Applied now: the salutation is removed, not forbidden

The check-in's prompt keeps its instruction, and code now enforces it: a leading
greeting-shaped clause — a known opener, an optional short address, terminal punctuation —
is stripped, so the message opens with the thing the butler actually noticed. "Morning
traffic is heavy" opens with a fact and is untouched; a message that was *only* a greeting
becomes empty and the empty check declines to send it.

Narrow and shape-based on purpose, like every deliberate heuristic in this repository: the
worst failure of an over-eager strip is a slightly blunter opening sentence, and the worst
failure of no strip was measured four times in one day.

### What follows later, under this name

Two remaining slices, each its own record when it ships:

- **One egress door** — the checks that exist (secret scan, pseudonyms, consent, taint)
  move into the constructor of a sealed request type, so [0067](0067-one-way-to-the-deep-model.md)'s
  bug class stops compiling rather than being caught by review.
- **Taint as a type** — `Turn<Tainted>` without `escalate()` or `act_unasked()`, so
  [0064](0064-what-a-stranger-said.md) stops being a flag the orchestration must remember at
  each of its exits.

## Consequences

- **The observed incident cannot recur**: the second, third and fourth messages fail the
  sameness gate, and a genuinely new finding at 13:35 opens with the finding.
- **A legitimate re-raise is delayed** until the person next speaks. Accepted: the person
  who has not answered the first message has not asked to hear it again.
- **The strip can blunt an opening** in rare shapes ("Morning briefing:" would survive, a
  comma-led address would not). Cosmetic, and bounded by the shape check.
- **Every future "make the model remember" fix is now suspect by policy.** The review
  question this record licenses: *which held fact is being narrated here, and why is code
  not supplying it?*

## Rejected

- **A stronger prompt instruction.** Tried, verbatim, in the check-in prompt that produced
  the failure. This is the fourth record in a row to reject it and it should be the last to
  have to.
- **Suppressing repeats by exact text.** The four live messages were each worded
  differently; exact matching is the duplicate detector this repository already learned is
  wrong ([the checker that compared strings against model output](0053-honesty-about-what-it-did.md)).
- **A cooldown clock on check-in topics.** A second timer to tune, and wrong in both
  directions — it re-raises into silence when it expires and suppresses a real change while
  it runs. The person speaking is the boundary that means something.
- **Post-editing the model's facts in general** (detecting a wrong city in prose, fixing a
  wrong number). Unsound, for the reason [0065](0065-a-place-is-not-the-models-to-remember.md)
  gave: it requires knowing which mentions are wrong in prose that may legitimately contain
  any of them. Supplying facts is exact; correcting recall is a guess. The salutation strip
  is not an instance of this — it removes a *format* violation by shape, it does not repair
  a fact.
