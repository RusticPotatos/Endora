# 0145 - The Core ↔ Console contract, and Console as its own deployable

## Status

Accepted, 2026-11-07, by the owner (P1 step 2, and CS1 = (b) in the product reset). AI-assisted
(Claude). The contract is [`docs/api/console-v1.md`](../api/console-v1.md) and its route list
[`docs/api/console-v1.json`](../api/console-v1.json). The sign-in question it raised is
[0146](0146-console-forwards-cores-session.md).

## Context

The product reset (7 Nov) makes Endora Core headless: the node, HQ's engine, the runner and the
worker, with no screens of their own once the cutover comes. Its screens become **Endora
Console**. The reviewers recommended keeping Console bundled behind a seam; the owner chose a
separate repository, image and release now (CS1 = (b)), at the same browser origin and with no
authority of its own. The step's wording, the Director's: *prove Console uses only documented
Core APIs and sessions, and prove Core operates with Console disabled.*

An inventory of the built-in console (at 38ad111b) found 111 distinct requests, every one to a
node route and none to HQ directly. Three things were missing for a separate client:

- an answer to a desk item could be pressed twice; the second was a bare `409`, so a client that
  lost the first reply could not tell "done" from "refused", and nothing recorded who answered,
  through what, or whether what it did succeeded;
- the live feed was a bare "changed" ping signed with a token in the query string, and HQ's
  events had a cursor but nothing to say when events after it had been pruned;
- a session could not be ended: there was no sign-out, and a password change left every other
  session open.

## Decision

1. **One documented, additive `/v1`.** Console may call the 103 routes in `console-v1.json`,
   grouped as session, work, decisions, runs and evidence, events, money and models, control,
   home and settings, and nothing else. They are the node's existing routes plus the few below. Routes and
   fields are only ever added within v1. Core says the contract's version at `/health` as `api`.
   No OpenAPI document, no second version served and no contract test in two repositories
   (principle 4); add them when a second consumer needs them.
2. **Sessions are Core's.** Signing in stays Core's password and authenticator code. New:
   `GET /v1/session/current` (when this session ends), `POST /v1/session/sign-out`,
   `POST /v1/session/sign-out-everywhere`, and a password change ends every other console
   session. Sessions are read on every request, so an ended one is refused at once.
3. **An answer is bound, idempotent and staged** (`/v1/decisions`). The node reserves the item
   for the answer in its own store before asking HQ, records the session's reference (never the
   token), the channel and the client's request key, and keeps four stages: requested, accepted,
   executed, verified. HQ remains the authority for whether a choice is allowed and decides each
   item once. The same choice again answers the stored record (`duplicate: true`) and applies
   nothing; another choice is `409`; an answer HQ never received stays requested and finishes,
   once, when sent again; an answer HQ applied but whose reply was lost is recognised by reading
   the item back and is not applied again. HQ gains one read, `GET /v1/desk/{id}`, for that.
4. **One event stream.** `GET /v1/events` pages HQ's events after a cursor (the snapshot's
   `last_event`) and says `gap` when some were pruned and `more` when there are more;
   `GET /v1/events/stream` is server-sent events signed with the header only, each event carrying
   its id for `Last-Event-ID`, plus `changed`, `gap` and `unavailable`. The query-token
   exception stays on the old feed alone.
5. **Console is its own deployable** (`RusticPotatos/endora-console`): the console's files as of
   this commit, its own build, image and release, served at the one browser origin in front of
   Core. Its proxy passes only the contract's routes (method and path pattern), checks the
   origin of every write, forwards only the request headers it needs, and keeps no credential
   and no session store. It never calls HQ. Audio appears in it as status and a link, from
   Audio's own health check.
6. **No owner function is lost without a destination** (the owner's M6 rule). Each screen of
   the built-in console goes somewhere, listed below; one has no destination yet and waits for
   the owner.
7. **Core does not change shape for it.** The rig's HQ routes (port 8790, `/v1/runner/*`,
   `/v1/team/*`, round, projects, tasks, goals, members, pool) keep path, shape and port. The
   built-in screens stay until the owner's cutover; until then HQ screen changes land only in
   Endora, and Console copies them.

## Where each owner function goes

Every screen and action of the built-in console (at 38ad111b), and where it is in Console v1:

| Screen | Owner actions | Destination |
|---|---|---|
| HQ: the agents map, your decisions, open work, projects and team, health, activity, Chess league, Office, Looking at, replay, Live Work, the world record | every control listed in the M6 inventory | **Console** (work, decisions, runs, control, money areas) |
| Settings: AI and model lists, connections, tools, sign-in, data (export, purge) | as today | **Console** (settings, money, session areas) |
| Home lab | the lab's figures and alerts, refresh, a service's logs | **Console** (home area) |
| Containers | list; start, stop, restart (Endora's own never offered) | **Console** (home area, `POST /v1/terminal/action`) |
| Logs | read one container's | **Console** (home area) |
| Jobs | list the allowlisted Ansible jobs; run one; open its last run | **Console** (home area) |
| Media | search; request what is not requested yet; recent requests | **Console** (home area) |
| Smart home | favorites on, off, scene; add or remove a favorite; search the catalogue | **Console** (home area) |
| Speakers | sources, speakers, wires and levels, setups, listening here, live sound | **Endora Audio's own page** (decided; Console shows Audio's status and a link) |
| Music | now playing; play, pause, next, previous; like; volume; choose a device; play on the Endora receiver; search; play a find or a playlist; discover | **Needs owner decision.** The Spotify player token moves to Audio (decided), but Audio's page has no Spotify player today. Console's Music shows Audio's status and link and says so. The options: (a) Audio's page gains the player, with its own token; (b) the Spotify app itself, which already sees Endora's receiver as a Spotify Connect device; (c) keep the token route in Core's contract for Console v1 until (a) |

The `/v1/terminal/*` paths keep their name in the contract: renaming them would add a second
set of routes for the same thing, against additive v1.

## Proof

- **Core without Console:** Core's full gate runs with no Console anywhere, and
  `every_route_the_contract_lists_is_served_by_the_node_alone` sends each listed route to the
  node's own router and fails on any it does not serve. The contract test also fails if a listed
  route is an HQ or rig route.
- **Console within the contract:** Console's proxy is built from a vendored copy of the JSON
  (pinned by its SHA-256), its screen check records every request the screens make and fails on
  one outside the list, and a check fails on any reference to Core's code or HQ's routes.

## Consequences

- A second client (the future chat connector, section 15 of the reset) answers through the same
  `/v1/decisions` and gets the same guarantees.
- The node keeps one more small table, `decision_answers`, one row per answered item.
- The Spotify account routes are in the contract under settings until Audio's cutover (A2).
- Two releases now move separately; the version at `/health` and Console refusing a major it
  does not know cover skew. Only the current pair is tested (principle 7).

## Alternatives considered

- **Bundle Console behind a seam** (the reviewers' (a)): fewer moving parts. Declined by the
  owner (CS1 = (b)).
- **Rename the routes for Console** (`/v1/work/*` for `/v1/hq/*`): no gain for the cost of a
  second set of names; additive v1 keeps the existing ones.
- **Make HQ's desk answer idempotent itself:** would change an HQ route's shape, which the rig
  holds frozen; the node is where answers are bound and audited already.
