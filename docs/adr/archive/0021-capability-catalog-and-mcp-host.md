# 0021 — The capability catalog: configuration, enablement, and an MCP host

## Status

Accepted (2026). Extends
[ADR 0019](0019-proactive-self-improving-butler.md) (capabilities as the butler's
skills) and upholds [ADR 0005](0005-models-propose-policy-authorizes.md)
(models propose; policy authorizes). Related:
[ADR 0004](0004-sqlite-first.md) (persistence),
[ADR 0001](0001-modular-monolith.md) (layering).

**Amended (2026).** The [Supplement](#supplement-2026-trust-model-transport-and-modular--agentic-extension)
below reconciles the MCP host with the reversibility bands shipped in
[ADR 0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md) and the
autonomy envelope + self-authored capabilities of
[ADR 0022](0022-autonomy-envelope-and-self-authored-capabilities.md), and settles
the three questions the original Decision left open: the **transport** boundary, how
the host is **modular** (others add servers), and how the butler may **extend itself
agentically**. Where the two conflict, the Supplement's band-based rule **supersedes**
Decision §2's "`ConfirmEachAction` by default."

## Context

The butler already has a **capability registry**: `default_capabilities()` returns
a `Vec<Arc<dyn Capability>>`, each carrying a `CapabilityInfo` manifest (id, name,
description, category, `reaches_external`, `autonomy`, `configured`, `needs`).
`GET /v1/capabilities` lists it and the console's **Skills** view renders it. This
is, in effect, a small catalog — comparable to an AI platform's model/tool catalog,
and more principled in one respect: every entry declares its **autonomy** and
whether it **leaves the device**, which the policy layer enforces.

Three gaps keep it from being a stable, long-term foundation:

1. **Configuration and enablement are baked in.** `configured` is derived from
   compile-time facts or the environment (e.g. `image_review` reads
   `ENDORA_VISION_MODEL`). A person cannot enable/disable a skill or supply its
   settings (an API key, a data source, an opt-in) from the UI. Turning a scaffold
   into a working skill means editing code or env and redeploying.
2. **Every skill is hand-written Rust.** Adding a capability means writing a struct.
   There is no way to bring in third-party or remote capabilities, and no standard
   protocol for doing so — even though ADR 0019 explicitly names the registry as
   "the substrate a future MCP host adapter plugs into."
3. **Status is a static bool.** The catalog shows `configured` but not liveness —
   whether a skill actually reached its source, when it last ran, whether it failed.

## Decision

Grow the registry into a **catalog** along three additive lines. None weakens the
policy boundary: the model still only *proposes* a capability; deterministic policy
still authorizes; the capability still executes behind that boundary.

### 1. A configuration + enablement store

- New application port `CapabilityConfigRepository` and a SQLite table
  `capability_config` (`id TEXT PRIMARY KEY, enabled INTEGER, settings TEXT` — the
  settings a small JSON object). Forward-created at open, like the other tables.
- `CapabilityInfo.configured` is **computed**, not baked: a skill is usable when it
  is *enabled* and its required settings are present. Each capability declares its
  settings schema (a small list of `{key, label, secret}`) via a new
  `CapabilityInfo.settings` field; the Skills view renders a form and an enable
  toggle, and `POST /v1/capabilities/{id}/config` persists it.
- Secrets (API keys) live only in this store, never in `CapabilityInfo` responses:
  the list endpoint returns *whether* a setting is set, never its value.

### 2. An MCP host adapter as a catalog *source*

- A new infrastructure module connects to configured **MCP servers**, lists their
  tools, and wraps each as a `Capability` in the same registry — so remote/third-
  party capabilities appear in the catalog next to the built-ins.
- `CapabilityInfo` gains a `source` field (`"built-in"` or `"mcp:<server>"`), shown
  in the catalog so provenance is legible.
- MCP-sourced capabilities are **`ConfirmEachAction` by default** (they leave the
  device and are not ours to vouch for); promoting one to autonomous is an explicit,
  per-capability human decision recorded in the config store. This keeps ADR 0005
  intact for capabilities we did not write.
- MCP server connections are themselves entries in the config store (URL/command +
  enable), managed from the Skills view.

### 3. Liveness in the catalog entry

- The registry tracks, per capability, `last_used_at` and `last_outcome`
  (ok/failed + a short note), updated when the interventions loop runs a skill.
- An optional cheap **health probe** (`Capability::health()`, default "unknown")
  lets a skill report reachability without being invoked. The Skills view shows a
  status dot, like a deployment's health — not just a static "configured".

### Where it plugs in

The `Capability` trait + `default_capabilities()` remain the single extension seam.
#1 and #3 are additive (a new port, a table, extra manifest fields, UI). #2 is a new
infrastructure module implementing `Capability` by proxying MCP — no change to the
application layer, which still speaks only to `CapabilityRunner`/the registry.

## Consequences

- The Skills view becomes a real catalog: per-skill status, source, autonomy,
  enable toggle, and a settings form — scaffolds like `image_review`, `flights`,
  and `local_events` become enable-and-configure, not edit-and-redeploy.
- Endora gains a standard, open way to acquire capabilities (MCP) instead of a
  struct per skill — the strategic unlock, and the industry-aligned analog to a
  platform capability catalog.
- More surface area: a config store (with secrets), an MCP client, and health
  plumbing. Mitigated by shipping in slices — **#1 first** (immediately useful,
  self-contained), then **#2** (the MCP host), with **#3** folded into whichever
  touches the entry rendering first.
- The policy boundary is unchanged and, for MCP, strengthened: unvetted remote
  capabilities default to confirm-each-action, and autonomy is granted per
  capability by a human, recorded and auditable.

## Alternatives considered

- **Keep hand-writing every capability.** Simple, fully in our control, but it does
  not scale and gives the person no way to extend Endora. Rejected as the long-term
  shape (built-ins remain first-class alongside MCP).
- **A bespoke plugin protocol instead of MCP.** More control, but reinvents a
  standard the ecosystem is converging on and isolates Endora from existing tool
  servers. Rejected.
- **Config via env/files only.** Zero new storage, but no UI-driven enable/config
  and no per-person settings; a redeploy to turn on a skill. Rejected as the stable
  answer, though env remains a valid override for headless setups.

## Supplement (2026): trust model, transport, and modular / agentic extension

The original Decision predates the **reversibility bands** ([ADR 0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md))
and the **autonomy envelope + self-authored capabilities** ([ADR 0022](0022-autonomy-envelope-and-self-authored-capabilities.md)).
It also deferred three questions that determine whether the host is maintainable and
safe long-term: where the **transport** boundary sits, how **others** add servers,
and whether the **butler** may add them itself. This supplement settles them. The
through-line: MCP does not get a second code path or a second policy — it enters
through the **one seam** (`CapabilityRunner`) and the **one gate** (the reversibility
classifier) every other capability already uses. If a design here needs to touch
`gather_with_skills` or the proactive flows, it is the wrong design.

### 1. Band, not vouching — supersedes Decision §2's default

Decision §2 said MCP tools are `ConfirmEachAction` by default. Replace that with the
band model (ADR 0024): an MCP tool's autonomy is **not read from its manifest** — the
deterministic classifier assigns a band, and **unknown / unclassifiable ⇒
`Irreversible` ⇒ blocked** until the person deliberately opens that capability's
irreversible band (`CapabilityConfigRepository::set_open_irreversible`, the existing
per-capability opener). This is stricter than "confirm each action," reuses machinery
already shipped, and — critically — a server's self-description is treated as
**untrusted input** (the same class as web content under the instruction boundary):
the band comes from what the tool *can do*, never from what it *claims*.

### 2. Registration is itself a gated capability

Connecting a server is not neutral configuration — it grants a whole **surface** of
new tools at once. So "register an MCP server" is modeled as an **action with its own
band** (`OutwardReversible` / `Irreversible` ⇒ deny-by-default). The server list is
data — an `McpServerRegistry` repository of rows (`{ name, transport, command|url,
enabled }`, sitting beside `capability_config`) — but **writing a row is gated**.
This is the recursion that makes both modularity and self-extension safe with one
rule: the act of extending the butler is governed by the same envelope as any other
consequential act.

### 3. Who may add a server — one mechanism, four trust tiers

Modularity ("others can add servers") and agentic self-extension ("the agent adds
them") are the **same registry write** across different trust boundaries. Only the
*registration gate* differs; everything downstream (classifier, audit, the four
proactive flows) is identical.

| Source | How a server is registered | Gate on registration |
| --- | --- | --- |
| **Built-in** | compiled (`default_capabilities()`) | trusted; still band-classified |
| **User** | Skills view → add server | user vouches; tools still band-gated |
| **Agent — proposes** | butler suggests a known server it judges useful | **confirmation required** (surfaces as an opener/proposal; ADR 0005) |
| **Agent — authors** | writes a server in its **sandbox** (ADR 0022) | sandbox isolation **+** every tool defaults `Irreversible` ⇒ blocked until opened |

Two independent walls protect the frontier tier: the **sandbox** contains the
*authoring*; the **classifier** contains the *invoking*. A tool the butler wrote for
itself last night is exactly as gated as one a stranger published — both arrive as
`CapabilityInfo` and both hit the same `classify()`. At **no** tier does
self-extension get a bypass around the gate. The "propose" tier ships without a
sandbox and is the near-term, high-value step; the "author" tier is the WALL-E north
star and waits on the sandbox.

### 4. Transport behind a port; ship stdio first

Transport is an infrastructure detail, not an architectural commitment. Define a port

```
trait McpTransport { fn list_tools(&self) -> …; fn call(&self, tool, args) -> …; }
```

and build the **stdio** implementation first: it matches the model-agnostic boundary
(Endora connects to tools it does not host), adds no network surface to the
unauthenticated 0.x API, and covers the bulk of the current MCP ecosystem. **HTTP/SSE**
becomes a *second `McpTransport` impl* later — not a rewrite; the adapter, classifier,
and flows do not move.

### 5. Modular-source hygiene (supportability)

Opening the host to servers we did not write demands three cheap disciplines:

- **Namespacing** — tools are prefixed by server (`calendar.create_event`) so two
  servers cannot collide on a name.
- **Isolation + health** — each server is an external process that can crash, hang, or
  emit garbage. A per-server timeout and an "unhealthy ⇒ `configured: false`" state
  drop a bad server out of the skill list (reusing the existing *unavailable* path)
  instead of failing a turn. One third-party server cannot take down the host.
- **Provenance** — the catalog carries `source` (`built-in` / `mcp:<server>`) and the
  **audit log** records which server a tool came from and *who* registered it
  (built-in / user / agent). That trail is the first thing support needs.

### Where it plugs in

A `CompositeRunner` merges the built-in registry and each MCP source behind the single
`CapabilityRunner` trait; `gather_with_skills` and the chat / brief / nightly /
check-in flows are **unchanged**. The MCP adapter, the transport port, the server
registry, and the classifier are all **infrastructure**; the application layer keeps
speaking only to `CapabilityRunner`. That invariant — no MCP knowledge above
infrastructure — is the maintainability test for every slice of the build.
