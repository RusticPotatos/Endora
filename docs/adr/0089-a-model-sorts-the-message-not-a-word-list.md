# 0089 — The local model sorts the message, not a word list

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Claude). Not deployed when written.

**Superseded 2026-10-20 by [0136](0136-endora-is-hq.md).** The local model's sorting of chat messages is retired. The reasoning below is kept; none of its mechanism runs.

## Context

ADR 0087 retries a turn with the deep model choosing the skills when the local answer says it
found nothing, or when a question about the world was answered with no skill run. "A question
about the world" was a list of words (when, next, latest, score, schedule, price…). It missed
"what specific games did they win/loose" live, and the model invented the scores.

## Decision

`Butler::classify_need` sorts a message into LOOKUP (needs current or outside facts), PERSONAL
(the person's own things) or CHAT (small talk, writing, arithmetic). It is one cheap call to the
**local** model at temperature zero; nothing leaves the device. The retry trigger uses it, and
only when it could matter: no skill ran this turn, and a deep model is available to retry with.
When the model cannot say (unreachable, or an answer that is not one of the three words), the
old word list decides, so the router can only add coverage.

## Measured

`tests/router_eval.rs` (ignored, live) runs 40 phrasings through the model and the word list
against fixed labels: 20 about the world, 10 about the person's own things (several containing a
world word), 10 small talk.

| | right |
|---|---|
| Word list, lookup vs not | 25 / 40 |
| `qwen2.5:7b` router, exact class | 40 / 40 |

About 0.4 s a call on the node's GPU. The set was written by the author from the live misses and
obvious controls, so it is not independent: it shows the router handles those phrasings, not that
it is perfect on others. Re-run it with `ENDORA_MODEL_URL` and `ENDORA_EVAL_MODELS` when the
model changes.

## Consequences and limits

- One extra local call on a turn that ran no skill, when a deep model is configured. Not on turns
  that used a skill, and not when the retry could not happen anyway.
- A wrong LOOKUP costs one deep-model call; a wrong not-LOOKUP is the status quo before this.
- The word list stays as the fallback and for `asks_what_it_did`, which decides whether the
  "what I did" notes are sent; that is a separate judgement.
