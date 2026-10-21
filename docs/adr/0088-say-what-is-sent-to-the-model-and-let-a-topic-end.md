# 0088 — Say what is sent to the model, and let a topic end

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Claude). Not yet deployed when written.

**Superseded 2026-10-20 by [0136](0136-endora-is-hq.md).** The context inspector and the chat's topics are retired. The reasoning below is kept; none of its mechanism runs.

## Context

The prompt is assembled out of sight: fixed instructions, the date and place, a running
summary, preferences, what Endora understands about the person, the skill list (twice: as text
and as tool definitions), and the recent messages. The person could not see what was in it, how
much of the model's window it filled, or what the "Ask deep" path shows another company. The
window is also the one thing a long conversation uses up, with no way to start over short of
deleting history.

## Decision

**A window ring and an inspector.** The composer shows a ring that fills with the window and
turns amber and then red. Tapping it lists every section of the next message with its estimated
size, the biggest lines first, and marks the sections an Ask deep plan is also shown. It is
`GET /v1/chat/context`, built from the same context and history a turn uses
(`context_report`, `history_the_next_turn_would_send`), without running the summariser.

**Estimates, labelled.** Four characters to a token. The model server's own count is not
available to the application layer, and a precise-looking wrong number is worse than an honest
rough one. The window size is `ENDORA_CONTEXT_TOKENS`, defaulting to the 16,384 the Ollama
role configures.

**New topic.** `POST /v1/chat/context/reset` records the moment and clears the running summary.
From then on, the model is sent only messages after it. Nothing is deleted: the earlier chat
stays on screen under a divider, and what Endora has learned about the person is untouched.
It is in the "+" menu and the inspector.

## Consequences and limits

- The inspector shows an ordinary message. The "what I did lately" notes are left out, since
  they are only sent when the person asks about them (ADR 0087).
- The skill list used to be sent twice (as text, then as tool definitions); the inspector made
  that visible and ADR 0090 removed the text copy.
- The estimate does not see the model server's own chat template or tool formatting.
- A reset is one boundary, not a set of named conversations.

## Validation

Unit tests for the report (ordering, totals, held-back tools, Ask deep marking, clipping), a
turn-level test that old messages stay in the chat but are not sent after a boundary, an API
test for the report and the reset, and console tests for the ring, the inspector and the
divider, including that text inside them stays text. Looked at in a browser against a
throwaway node, not against the live model.
