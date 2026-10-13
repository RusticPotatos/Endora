# 0087 — A local answer that found nothing is checked once

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Claude). Not yet deployed when written.

## Context

"when is the next panthers game?" got "I couldn't find any upcoming games in your area"
from the local 7B model, with no skill used at all. Measured earlier (ADR 0085): after an
empty first result, local models of 7B–14B try another source in about 1 of 12 trials, and
rewording the result does not change that. The automatic hand-off to the deep model does not
rescue it either: a confident wrong answer is not detected as a failure, and a turn that has
read a third party's words (even an empty result) is refused by the door (ADR 0064).

## Decision

When a chat turn did not act, the local model has not already escalated, and its reply says
it looked and found nothing, the turn is run once more with the deep model choosing the
skills and the local model reading, exactly the arrangement of the Ask deep button
(ADR 0086). The deep model never sees a result and sees nothing about the person; the
local model is given no skills while it reads; the same clearance gate runs every call.

- **Only with the person's consent.** It runs only when automatic escalation is on and a deep
  model is configured (`Butler::deeper`, ADR 0055). Otherwise the local answer stands.
- **Disclosed.** The reply ends with the `[deep]` line saying what went where, and the
  activity trail records that the local answer found nothing.
- **Fails safe.** If the retry errors or says nothing, the original reply is kept.
- **Once.** One extra attempt per turn, with the planner bounded to three calls.

**A second trigger, found live:** asked "when is the next panthers game?", the local model
answered with a date, a time and an opponent, stated plainly and with no skill run. All of it
was invented (checked against the published schedule). So a turn also retries when it ran no
skill at all and the question is about the world: words such as when, next, latest, score,
schedule, price. A question about the person's own things ("my", "our", "I") never counts, so
it stays local.

**And a leak, same turn:** the answer ended with a copy of the "what I did lately" notes, which
the model had quoted from its own instructions. Those notes are now given to the model only
when the person asks what Endora did, and any `[did]` block in a reply to a different question
is cut. The record is still appended by code when it is asked for.

The trigger is a short list of phrases in the reply ("couldn't find", "no upcoming",
"no results", ...), not judgement of the question. A false positive costs one deep-model
call; it cannot change what the person is told except by replacing it with an answer that
had the skills.

## Limits

- The world-question words are a list. A question without one of them, answered confidently
  from memory, is still not caught.
- A model that answers wrongly without saying it found nothing is not caught.
- The question, redacted, and the skill list leave the box on these turns (as in ADR 0086).
- The first reply may show in the console for a moment before the retry files it under
  "Thinking" and empties the bubble; the stored message is the final one. Holding the draft back
  would stop replies streaming for everyone with a deep model, so this is accepted.
- Whether it helps depends on a search skill being in front; without one the deep model has
  nothing better to call.

## Validation

Turn-level tests: a give-up is retried and the answer is the reader's, with the `[deep]` line,
the skill run, and nothing private or stranger-written in what the deep model saw; an answer
that found something never asks; with escalation off the local answer stands. Not run against
live models.
