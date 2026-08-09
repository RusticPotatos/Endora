# 0022 — The autonomy envelope and self-authored capabilities

## Status

Accepted (2026). Builds on
[ADR 0005](0005-models-propose-policy-authorizes.md) (models propose; policy
authorizes), [ADR 0010](0010-autonomy-model.md) (the act/ask loop),
[ADR 0019](0019-proactive-self-improving-butler.md) (the proactive, self-improving
butler), [ADR 0020](0020-intent-first-understanding-loop.md) (intent-first), and
[ADR 0021](0021-capability-catalog-and-mcp-host.md) (the capability catalog).

## Context

Endora's north star is a butler that runs its **own** learning loop — has an idea,
iterates and learns, and acts — not one that asks permission for every step. We do
**not** want to restrict it from creating things or acting independently.

But two things are in tension on the surface:

1. **"Models propose; deterministic policy authorizes"** (ADR 0005) — the model is
   never the enforcement boundary; a consequential action is never routed straight
   from model output.
2. Today Endora defaults **timid**: the conservative posture is Observe / confirm
   most things (ADR 0010's default). That is safe but it is the opposite of an
   autonomous butler.

The resolution is that (1) is not a limit on **creativity** — it is a limit on
**unchecked consequential action**. The fix is not to move the enforcement boundary
onto the model (OpenClaw's bet: the model holds the keys, safety is trust +
locality). The fix is to make the **envelope** the deterministic layer enforces
*wide and user-defined*, so the butler is fully independent inside it and only
surfaces at its edges. Independence is something the person **grants by shape**, and
the policy layer guarantees the model cannot exceed that shape even if it "wants" to.

## Decision

Introduce a **user-defined autonomy envelope** the deterministic policy layer
enforces, and let the butler **author its own capabilities** within it.

### The envelope

A declarative, user-owned policy expressed along axes that are **deterministically
classifiable** — never a matter of the model's own say-so:

- **Reversibility** — reversible (has an undo / no lasting effect) vs. irreversible.
- **Cost / stakes** — free & local vs. spends money vs. acts as the person (sends,
  posts, books).
- **Reach** — stays on device vs. leaves the device.
- **Domain** — the capability or category it touches.

Each capability action carries these as **declared metadata** (extending
`CapabilityInfo`), or is derived by a deterministic classifier — the model does not
get to assert "this is safe/reversible." The envelope maps combinations to an
outcome: **act autonomously**, or **surface as a proposal** for the person to
confirm (the existing suggestions/confirm flow). The person widens or narrows it
("you may browse, draft, schedule, and run reversible experiments on your own; ask
before anything that spends money, sends on my behalf, or can't be undone").

The default posture shifts from "ask about most things" to **"act on the reversible,
local, no-cost, read/draft space; confirm the rest,"** and is configurable up or
down. This generalises ADR 0010's per-capability `AutonomyLevel` into a policy the
person sets once and the layer applies everywhere.

### The learning loop runs *inside* the envelope

- **Idea** → the butler forms a hypothesis (existing `Assumption` / `Experiment` /
  beliefs).
- **Iterate & learn** → it runs **reversible, in-envelope** steps on its own —
  drafts, reads, cheap experiments — and records what it learned (`Reflection`,
  `ProcessChange`). This is "apply the learning loop to the butler itself."
- **Return at the boundary** → only steps that fall *outside* the envelope
  (consequential, irreversible, spends, leaves the device) surface as proposals.
  The person reviews the edges, not the whole activity. Everything done
  autonomously is on the activity feed and in the audit log.

### Self-authored capabilities

The butler may **propose and author a new tool** when one it needs doesn't exist.
It enters the catalog (ADR 0021) as:

- **sandboxed** (resource / network / filesystem limits),
- **reversible-by-default**, `source: "self-authored"`,
- **`ConfirmEachAction`, and NOT autonomous until the person grants it** per ADR
  0021's enablement.

So authoring a tool is a *proposal*; running it autonomously is a *human grant*. The
model never both writes **and** autonomously executes arbitrary code — which is
exactly the OpenClaw capability without the OpenClaw risk, and keeps ADR 0005 intact
(in fact strengthens it for code the model wrote itself).

### Invariants that do not move

- **Classification is deterministic**, from declared metadata — never model
  self-report. Unknown/unclassifiable ⇒ treated as consequential ⇒ confirm
  (deny-by-default at the risky axes).
- **The model still only proposes.** The envelope is what *authorizes*; a wide
  envelope is still a deterministic gate, not the model pulling its own trigger.
- **Everything autonomous is auditable and, where possible, carries an undo.**

## Consequences

- The butler becomes genuinely independent and self-improving **inside a boundary
  the person owns** and can reshape — the autonomous-butler vision, without handing
  the model the keys.
- ADR 0005 is preserved and, for self-authored tools, strengthened: creation is
  decoupled from autonomous execution.
- New surface to build (in slices): an **envelope config** store + UI, an **action
  classifier** over declared metadata, a **sandbox** for self-authored tools, and
  **reversibility/undo** tracking on autonomous actions. Suggested order: envelope
  policy + classifier first (turns the existing confirm flow into a configurable
  boundary), then self-authored-tool sandboxing.
- New risk: **mis-classification** (an irreversible action treated as reversible).
  Mitigated by deny-by-default on unknown classification and by keeping the risky
  axes (spends, sends, irreversible, leaves-device) confirm-by-default unless the
  person explicitly widens them.

## Alternatives considered

- **OpenClaw-style unbounded execution** — the model has direct computer control and
  runs whatever it writes. Maximal capability, but the enforcement boundary becomes
  the model plus user trust; it breaks ADR 0005 ("never route a consequential action
  directly from model output"). Rejected — we take the other bet at this fork.
- **Keep the timid ask-everything default.** Safe, but it defeats the autonomous,
  self-improving butler (ADRs 0019/0020). Rejected as the long-term posture.
- **Let the model judge what is safe to auto-run.** Rejected outright: the model is
  never the enforcement boundary. Safety classification is deterministic.
