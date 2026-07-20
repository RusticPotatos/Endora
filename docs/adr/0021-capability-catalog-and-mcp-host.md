# 0021 — The capability catalog: configuration, enablement, and an MCP host

## Status

Accepted (2026). Extends
[ADR 0019](0019-proactive-self-improving-butler.md) (capabilities as the butler's
skills) and upholds [ADR 0005](0005-models-propose-policy-authorizes.md)
(models propose; policy authorizes). Related:
[ADR 0004](0004-sqlite-first.md) (persistence),
[ADR 0001](0001-modular-monolith.md) (layering).

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
