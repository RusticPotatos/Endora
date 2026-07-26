# 0018 — Streaming chat responses

## Status

Accepted (2026). Builds on [ADR 0014](0014-the-butler-conversation-values-attention.md)
(the butler) and [ADR 0007](0007-async-web-stack.md) (the async web stack).

## Context

The butler answered in one shot: the console POSTed to `/v1/chat` and waited for
the whole reply before showing anything. On a local model — especially inference
on CPU — that is a multi-second stare at a spinner. Modern chat agents stream the
reply token-by-token, which both *feels* far faster and makes a slow model legible
(you can see it working). We want that, without breaking the two things that make
the butler the butler:

1. **Models propose; deterministic policy authorizes.** The reply carries not just
   prose but a closed set of structured **proposals** the person confirms. Streaming
   must not turn the model into an actor.
2. **Local-first, minimal dependencies.** The node already uses a synchronous HTTP
   client (`ureq`) for the model; we would rather not add an async HTTP stack.

## Decision

Add a streaming path alongside — not replacing — the one-shot path.

- **Port.** `Butler` gains `respond_streaming(history, prefs, context, on_token)`
  with a **default** implementation that computes the whole reply and emits it in
  one chunk. So every butler (including the scripted fallback) works with a
  streaming caller; only `LlmButler` overrides it to stream for real.
- **Model envelope stays the same.** The model still returns the tuned JSON
  envelope `{"reply":"…","proposals":[…]}` (unchanged prompt, so the anti-jargon
  and anti-sycophancy behaviour is untouched). `LlmButler` requests `stream: true`,
  reads the server-sent delta chunks, and **incrementally extracts the growing
  `reply` string** from the partial JSON, emitting only the new prose to `on_token`
  — the JSON envelope itself is never shown. The authoritative `reply` + proposals
  come from a full parse of the accumulated envelope once the stream ends. A helper
  (`extract_reply_preview`) unescapes as it goes and holds back any incomplete
  trailing escape, so the preview only ever grows (unit-tested).
- **Endpoint.** `POST /v1/chat/stream` returns Server-Sent Events. Each event's
  `data` is a JSON object tagged by `type`: `token` (next prose piece), `done`
  (the persisted reply + proposals), or `error`. The blocking model call runs on a
  worker thread (`spawn_blocking`) and feeds tokens through a channel into the async
  SSE response — so **no async HTTP client is added** (`ureq`'s streaming body is
  read synchronously and bridged). The one-shot `POST /v1/chat` remains for the CLI
  and as a fallback.
- **Persistence & recovery (unchanged invariant).** The person's message is
  persisted *before* the model is called; the reply is persisted when the stream
  completes. So an interrupted stream never loses the turn — the last stored message
  is then still the person's, and the console shows a "still thinking" indicator
  derived from that state.
- **Confirm loop unchanged.** Proposals arrive only in the terminal `done` event and
  are still confirmed by the person; the model never executes anything.

## Consequences

- The console grows the butler's bubble live as prose arrives, then finalises to the
  persisted reply with its confirmable proposals. On CPU the tokens trickle (but
  visibly); on GPU it is smooth.
- One extra endpoint and a streaming path in the model adapter; no new dependency,
  no prompt/format change, and the non-streaming path is preserved.
- The console suppresses change-stream reloads while a reply is streaming, so a
  concurrent write can't wipe the in-progress bubble.
- Bounded by the model timeout (ADR-adjacent, in `LlmButler`): a stuck stream still
  falls back to the scripted reply rather than hanging.
