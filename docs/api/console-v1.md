# Core ↔ Console contract, v1

What Endora Console may ask of Endora Core, and how Core answers. The machine-readable list is
[`console-v1.json`](console-v1.json): every route Console may call, by method and path, and
nothing else. Core's tests hold every listed route to being served by the node alone; Console's
proxy passes exactly this list and its checks hold the screens to it
([ADR 0145](../adr/0145-core-console-contract.md)).

## Rules

- **One version, additive.** `/v1` gains routes and fields; none is removed, renamed or given a
  new meaning, and an event kind is never reused. Console ignores fields it does not know.
  Core says its contract version at `/health` as `api` (`"1.0"`; the minor number grows with
  each addition). Console refuses a Core whose major version it does not know, with a plain
  message, and tests only the current pair.
- **Core authorizes every write.** Console holds no credential and no session store. The
  person's session token (from signing in to Core) travels in `Authorization: Bearer` from the
  browser, through Console's proxy, to Core, which checks it and its scope on every request.
- **Console never calls HQ.** The engine's own routes (port 8790, `/v1/runner/*`, `/v1/team/*`,
  `/v1/round`, `/v1/projects`, `/v1/tasks`, `/v1/goals`, `/v1/members`, `/v1/pool`) are the
  research rig's and the workers'; they are not in this contract and do not change. The
  `/v1/hq/*` routes below are the node's, which forwards to HQ and audits.
- **Errors** are JSON `{"error": "<plain words>"}` with the status: `400` bad request, `401` no
  usable session, `404` not found, `409` conflicts with what is recorded, `502`/`503` HQ not
  reachable or not set up.

## Areas

| Area | Routes (see the JSON for the full list) |
|---|---|
| Session | `GET /health`; `GET`/`POST /v1/session` (whether sign-in exists; sign in with password and authenticator code); `/v1/session/setup` (claim); `GET /v1/session/current`; `POST /v1/session/sign-out`; `POST /v1/session/sign-out-everywhere`; password, authenticator, checking credential, bootstrap token |
| Work | `GET /v1/hq` (the snapshot), tasks, one item's Live Work, the team map, scenarios and the league, the world record |
| Decisions | `GET /v1/decisions`, `GET`/`POST /v1/decisions/{id}` |
| Runs and evidence | replays of a run and a task, a delivered bundle, Activity, configuration writes |
| Events | `GET /v1/events`, `GET /v1/events/stream` |
| Money and models | credits and caps, the pool's top-up, the Mac's settings, AI, model lists |
| Control | the kill switch and resume, giving work, goals, members, projects, routines, delivery, contracts |
| Home | the web console's home lab, media and smart home: the lab's figures, containers and their logs, the allowlisted jobs, media requests, the house's favorites and catalogue (`GET /v1/terminal/*`), and their one action route `POST /v1/terminal/action`, which Core checks against its own allowlist and runs once per request id. The paths keep their old name; the handheld that first used them is retired |
| Settings | capabilities, tool servers, connections (the Spotify account until Audio's cutover), configuration undo, export and purge |

## Sessions

Signing in is Core's: `POST /v1/session` with `{"password", "code"}` (an authenticator code)
answers `{"token"}`, throttled and locked against guessing. A console session lasts thirty days
from sign-in and ends after seven days unused (ADR 0125).

- `GET /v1/session/current` → `{"scope", "bootstrap", "issued_ms", "expires_ms",
  "idle_expires_ms"}`: when this session ends. The bootstrap token has no end (`null`s).
- `POST /v1/session/sign-out` ends the session that asks. `POST /v1/session/sign-out-everywhere`
  ends every console session, this one included, and answers `{"ended": n}`. Changing the
  password ends every other console session too.
- An ended or expired session is refused with `401` on its next request, at once: sessions are
  read on every request, never cached.

## Decisions

A decision is an item on the owner's desk, by its stable id. An answer binds the signed-in
person to that exact pending item and a choice the item allows.

- `GET /v1/decisions` → `{"pending": [...], "answered": [...]}`: what waits (HQ's desk and
  *While you were away*, as the snapshot shows them) and the latest answers with their stages.
- `GET /v1/decisions/{id}` → `{"decision": <record or null>, "desk": <the item, with its status>}`.
- `POST /v1/decisions/{id}` with `{"choice", "words"?, "request"?, "channel"?}` answers it.
  `request` is a key the client makes once per press; `channel` names the client
  (`console` by default).

The record:

```json
{
  "id": 42, "kind": "approval", "choice": "approve", "words": "",
  "by": { "person": "owner", "session": "s-1a2b3c4d" }, "channel": "console", "request": "k-…",
  "stages": {
    "requested": { "at": 1730000000000 },
    "accepted":  { "at": 1730000000050 },
    "executed":  { "at": 1730000000400, "ok": true, "said": "…" },
    "verified":  { "at": 1730000000450, "ok": true, "how": "…" }
  }
}
```

- **requested**: Core has the answer and has reserved the item for it.
- **accepted**: HQ recorded the decision on its desk.
- **executed**: what the choice does has run. For most kinds that is the same moment as
  accepted (HQ applies it as it records it); an approval runs the stored tool, a push pushes the
  branch, and each says what came of it, `ok: false` with the reason when it failed.
- **verified**: Core read the desk back afterwards and found the item recorded with that choice
  (and, for a push or an approval, that what ran succeeded). `ok: false` says what did not match.

Idempotent: the same choice again (a double press, a retry after a dropped connection) changes
nothing and answers the stored record with `"duplicate": true`. A different choice for an item
already answered, here or in another client, is `409` with what is recorded. A choice the item
does not allow is refused by HQ (`400`), and nothing is recorded. If HQ cannot be reached the
record stays at **requested** and the answer is `502`; sending the same answer again finishes
it, and never applies it twice.

## Events

One stream for Console. The snapshot's `last_event` (`GET /v1/hq`) is the cursor.

- `GET /v1/events?after=<cursor>` → `{"events": [{"id", "at", "kind", "data"}], "cursor",
  "gap", "more"}`, oldest first, at most 500. `more` says to ask again from the new cursor.
  `gap: true` means events after the cursor were pruned: take a new snapshot.
- `GET /v1/events?before=<id>&limit=<n>` → older events, for paging back through Activity.
- `GET /v1/events/stream?after=<cursor>` is server-sent events, signed with the header (a
  `fetch` stream; the query-token exception stays only on the old `/v1/activity/stream`). It
  sends `hello` with the cursor, then each event as `event: hq` with `id:` set to its id, from
  the cursor on, so a reconnect with `Last-Event-ID` (or `after`) catches up exactly; `changed`
  when something outside HQ's record changed (settings, connections); and `gap` when the cursor
  is older than what HQ keeps. The client drops any id at or below the last it applied, so a
  reconnect never applies an event twice.

## What is not in the contract

Sound (`/v1/sound/*`, Audio's own API after its cutover), the Spotify player token (Audio's), the old desk route `/v1/hq/desk/{id}`
(Console answers through `/v1/decisions/{id}`), the query-signed `/v1/activity/stream`, the
workers' relay and gateway, and every HQ route. The built-in console keeps using its routes until
the cutover.
