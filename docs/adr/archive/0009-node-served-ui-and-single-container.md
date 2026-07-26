# 0009 — Node-served web UI and single-container packaging

## Status

Accepted (2026).

## Context

v0.1.0 ships a CLI against a local HTTP node. External review is blunt that the
gap to a usable product is not features but a **GUI** and a **one-command
install** — "nobody logs reflections via `curl`." The architecture already treats
clients as thin and replaceable over the versioned protocol
([ADR 0003](0003-http-json-openapi-protocol.md),
[architecture.md](../../architecture.md)), so a browser UI is just another client.

The question is *how* to add a UI and *how* to package the whole thing so it
installs in one step, without contradicting local-first or the thin-client model.

## Decision

**The node serves a self-contained web console at `/`.** Static assets
(HTML/CSS/JS) are **embedded in the `endora-node` binary** (via `include_str!` /
an embed crate) and served same-origin from the existing Axum app.

**The whole system is packaged as a single container** — the image already built
by the `Dockerfile` (node + the embedded UI + the CLI binary) — with an optional
local model as a *separate* Compose service. `docker compose up` then brings up
the entire product in one command.

Consequences of "same-origin, embedded, web-first":

- No CORS: the page and the API share an origin, so no cross-origin config and no
  new middleware.
- No separate front-end build/deploy for v0.2: a hand-written console (no
  npm/bundler) keeps the build simple and the container self-contained. A build
  step can be adopted later *behind the same route* without changing this
  decision.
- The UI holds no authority — it is a client that makes the same API calls the
  CLI does; nothing privileged is exposed to the browser.

## Consequences

- One-command install and "the container is the product": closes two of the
  review's four gaps (GUI, one-command install) at once.
- The binary stays self-contained (assets compiled in), so the image needs no
  extra files and the page can't drift from the server that serves it.
- A plain HTML/JS console is less rich than a framework SPA. Acceptable for 0.2;
  richer interactions can come with a build step later.
- The API remains **unauthenticated** (see [SECURITY.md](../../../SECURITY.md)); a
  browser UI does not change that, and the deployment note (keep it on
  loopback/private) still applies. Authentication stays pre-1.0 work.

## Alternatives considered

- **A separate front-end project (React/Vite, its own dev server)** — rejected
  for 0.2: adds a JS toolchain, a second deployable, and CORS, for no benefit
  while the UI is simple. Revisit if the console outgrows hand-written HTML.
- **Native (Swift/SwiftUI) client first** — rejected as the *first* GUI: slower
  to usable and platform-locked. It becomes a follow-up client, not a
  prerequisite.
- **No UI (CLI only)** — rejected: the review is right that a CLI does not retain
  users for this kind of tool.
- **Bundling the model into the app image** — rejected: multi-GB weights do not
  belong in the app image; the model stays an optional, replaceable Compose
  service ([ADR 0008](0008-local-model-adapter.md)).
