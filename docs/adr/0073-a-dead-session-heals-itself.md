# 0073 — A dead session heals itself

## Status

Accepted (2026-08-06). Narrows how [0054](0054-other-peoples-services.md)'s MCP
connections are kept alive, after a live failure that made every action against the
house fail silently until the node was restarted.

## Context

The person connected a light in Home Assistant and told Endora so. Every action that
followed failed:

```text
home-assistant.HassTurnOn — reported error: MCP SSE post failed: http status: 404
```

The HTTP+SSE transport works by the server handing out a **session-specific POST URL**
at connect time; every later call posts there. Installing a device reloads Home
Assistant, and the reload takes the session with it. Endora went on posting to a URL
that had ceased to exist.

Nothing noticed, and that is the part worth recording. The node already reconnects
servers on a heartbeat — but its health check is *"does this server expose any
tools?"*, and `McpConnection` holds `tools: Vec<McpToolInfo>` **cached from the last
successful connect**. A server whose session is dead therefore lists all twenty of its
tools, looks entirely healthy, and never qualifies for reconnection. Verified on the
live node while it was broken: the registry still listed `HassTurnOn`, `GetLiveContext`
and the rest.

**The health check asked whether the server had tools; the failure was that the session
behind those tools was gone.** Two working parts — a tool cache and a reconnect trigger
— joined by a predicate that could not see the actual failure. The state persisted
until a restart, and would have recurred on every Home Assistant reload.

## Decision

### The transport heals its own session

`HttpMcpClient` holds its SSE connection behind a lock so it can be replaced. When a
call fails in a way that means *the session is gone*, the client opens a fresh session,
re-runs the `initialize` handshake on it, and retries the call **once**.

Healing belongs here because this is the only layer that can see a dead session. The
heartbeat sees a healthy-looking server; the runner sees an error string; the transport
sees the 404 come back from the URL it was given.

### Once, never a loop

One retry. If the fresh session fails too, the server is genuinely unavailable and
saying so is the honest answer — the failure reaches the person as it does today,
rather than being buried under retries that also fail.

### The predicate is about shape, not status alone

A dead session is `MCP SSE post failed` **plus** a `404` or an explicit "session"
complaint. A timeout, a 500, a refused connection are a server having a bad day, and
reopening on those would be a reconnect storm against something already struggling. The
test pins both directions, including the near-miss (`MCP HTTP request failed: … 404` —
the streamable transport, a different failure, deliberately not matched).

### Every transport, because the class is the transport's not the server's

The audit that followed the fix found the same shape twice more, so all three heal:

- **HTTP+SSE** — the observed failure. Reopen the session, re-handshake, retry once.
  A dead *stream* counts too: `recv_timeout` distinguishes a closed channel (the
  reader thread ended, so the stream is gone) from a timeout (the server is thinking),
  and only the first heals. The code previously collapsed both into "timed out",
  which would have left stream-side deaths uncovered.
- **Streamable HTTP** — the specification is explicit that an unknown session id
  answers `404` and the client starts a new one. Nothing on this install speaks this
  transport, so it is the same class fixed on the strength of the spec rather than a
  screenshot. The stale id is cleared *before* the handshake, or the server is being
  asked to honour the session it just disowned.
- **stdio** — a subprocess is a session and dies like one. A closed channel or a broken
  pipe means it is gone; the client keeps what it takes to start the server again and
  does so once. A timeout does not restart: that would kill work in progress.

### The weak health check stays, and says why

*"Exposing no tools"* is kept as the coarse net for the coarse case it was written for:
a server that was not up when the node started. Its comment now records what it cannot
catch and where that is handled instead, so the next person to read it does not mistake
it for complete.

## Consequences

- **A Home Assistant restart no longer breaks the house until Endora restarts.** The
  next action reopens the session and goes through.
- **The first call after a reload pays one extra round trip** — a reopen and a
  handshake. Once, and only on the call that discovers the death.
- **A genuinely dead server fails the same way it does now**, one retry later.
- **All three transports recover; none of them retries twice.** The shared discipline
  is that a fresh session failing means the server is genuinely down, and saying so is
  the answer.
- **A restart costs the work in flight on that one call**, which is why only
  unambiguous death — a closed channel, a broken pipe, a 404 on a session — triggers
  it, and never a timeout.

## Rejected

- **Reconnecting on every MCP error.** A reconnect storm against a struggling server,
  and it would hide real outages behind retry latency.
- **Making the heartbeat's health check stronger** — pinging each server periodically.
  It moves the fix further from the evidence, adds traffic on a schedule, and still
  leaves a window between ping and call. The transport already learns the truth for
  free, at exactly the moment it matters.
- **Dropping the tool cache so a dead session empties the list.** It would make the
  weak check work by making every server's catalogue depend on a live round trip —
  slower turns, and a flap would empty the catalogue mid-conversation.
- **Restarting the node on MCP failure.** A sledgehammer that loses in-flight state to
  fix one connection.
