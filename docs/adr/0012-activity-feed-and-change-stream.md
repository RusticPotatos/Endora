# 0012 — Activity feed and a server-sent change stream

## Status

Accepted (2026).

## Context

Retention is 0.3's unproven risk: the learning loop only closes if a person keeps
coming back to observe their experiments. Review scheduling
([ADR 0011](0011-review-scheduling-reminders.md)) brings due experiments back to
attention; the complementary need is a sense of *momentum* — a visible record of
what has happened — and for the console to reflect changes **live** rather than
only on manual reload.

Two questions: what is "the activity feed", and how does the console stay current
without polling?

Most of the learning loop does not yet store a creation time; only **observations**
and **audit records** carry a durable timestamp. So a "what happened" timeline can
be *derived* from those today, and widen later as more of the loop gains
timestamps — without inventing a new persisted event log now (which would mean
threading an append into every use case, a large change for little immediate gain).

## Decision

**The activity feed is a read projection, not a new persisted entity.** A
`recent_activity` use case merges the already-persisted, already-timestamped facts
— recorded observations and audited decisions — into one newest-first timeline
(`ActivityItem { at, kind, summary }`), served at `GET /v1/activity`. It stores
nothing new and needs no schema of its own. As more of the loop gains durable
timestamps, the same endpoint widens without a protocol change.

**Live updates use server-sent events carrying a nudge, not data.** The node holds
a broadcast channel; a small middleware sends a signal after any **successful
write** (`GET`s never signal). `GET /v1/activity/stream` is an SSE endpoint that
emits a `changed` event per signal. Clients **re-read** the authoritative state on
each event (the console refreshes its snapshot and feed); the stream never carries
the changed data itself.

Why this shape:

- **Derived feed over a new event log.** No new table, no append threaded through
  every use case, no second source of truth to keep consistent. The feed cannot
  drift from the data because it *is* the data, re-projected.
- **A change *signal*, not change *data*.** The stream says only "something
  changed"; the client re-reads. This keeps the node the single authority (a
  client can never act on stale pushed state), makes the stream trivially correct
  under coalescing and lag, and avoids leaking a per-event payload contract.
- **Middleware, not per-handler calls.** One layer over the router notifies on any
  successful `POST`, so new write endpoints are covered automatically and no
  handler can forget to signal.

## Consequences

- The console shows a live feed and reflects new activity — including due reviews —
  without polling; a write from the CLI or another tab appears immediately.
- The feed is intentionally partial at first (observations and decisions). This is
  honest and widens for free as creations and lifecycle transitions gain durable
  timestamps; no protocol change is needed when they do.
- SSE is one-way and reconnects natively in the browser's `EventSource`, so no new
  client library and no WebSocket upgrade path. A lagged subscriber still receives
  a single `changed` signal and re-reads — it can miss intermediate signals but
  never ends up stale.
- The stream requires a long-lived connection per client. Acceptable for a
  local-first, single-user node; it is not a fan-out broadcast system.
- One additional dependency direct-declared (`futures-util`, already in the tree)
  to adapt the broadcast channel into the SSE stream.

## Alternatives considered

- **A persisted activity/event log written by every use case.** Rejected for now:
  a large, cross-cutting change (an append threaded through the whole loop, a new
  table, and its own memory-rights handling) for a feed we can already derive.
  Revisit if activity needs to record things that carry no other durable trace.
- **Streaming the activity items themselves over SSE.** Rejected: it makes the
  node push state a client could act on while stale, and commits us to a per-event
  payload contract. A re-read on a signal keeps the node authoritative and the
  stream dumb.
- **Client polling (`GET /v1/activity` on a timer).** Rejected as the primary
  mechanism: wasteful and laggy compared with a push signal. (A client may still
  poll as a fallback where SSE is unavailable; the endpoint supports it.)
- **WebSockets.** Rejected as overkill: updates are one-way (server → client), for
  which SSE is simpler, proxy-friendly, and needs no upgrade handshake.
