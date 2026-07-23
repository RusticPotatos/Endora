# 0027 — The self-improving model layer (model discovery, eval, gated adoption)

## Status

Accepted (2026). **Core implemented**: the fitness function and the deterministic
adoption policy ship in `crates/endora-infrastructure/src/model_layer.rs`
(`evaluate` → `Scorecard`, `decide_adoption`, `run_model_layer`), over the
runtime-swappable model config (`ButlerModelConfig`, ADR-linked). It is
**runnable end-to-end**, on demand: `POST /v1/model-layer/run` (a console
"Evaluate & tune" button) discovers the local endpoint's models, scores each, and
gate-adopts the best local one in the background, logging scores + the decision to
activity.

**The capability ladder is implemented** (automatic escalation): the person can
configure a deeper (bigger / cloud) model, exposed to the butler turn through a
`DeepAsker` port (`endora_infrastructure::DeepModelAsker`). The turn is
**local-first** — it escalates to the deeper rung **only when the local model comes
up empty**, a deterministic trigger (not the model's self-report), and only when the
person has opted in by configuring one. Escalation returns **prose only**, never an
action, so it stays a reasoning aid behind the deterministic policy boundary; and
the asker applies the **egress guard + PII minimization** (ADR 0023) before the
question leaves the device, logging the escalation to the action feed. **Pending**:
automating the local tune — a `ModelDiscoverySchedule` driving `run_model_layer`
from the heartbeat off-hours — and widening discovery beyond the local endpoint to
Hugging Face / leaderboards; and richer escalation triggers (e.g. low-confidence,
not only empty).

## Context

Endora's north star is a curious, self-improving agent (see ADR 0019). The
butler's quality is bounded by the model behind the policy boundary, and on
consumer hardware (the reference target is an RTX A2000, 12 GB) the choice of
model is the single biggest lever on how *agentic* the butler can be. Two facts
make that choice a moving target:

1. **The discriminating axis is measurable.** An agentic eval — the fitness
   function `endora_infrastructure::model_layer::evaluate` (run for a human by
   `tests/agentic_eval.rs`) — scores a model on skill selection, no-fabrication,
   relay accuracy, grounding, brief-intent, and the L2 "Jarvis" behaviours. Early
   runs show the safety axes (no-fabrication, relay) hold at every size — the
   deterministic guardrails work — while **skill selection / routing** is what
   scales with capability. That is exactly the axis a *function-calling
   specialist* can win cheaply, so "which model?" is really "which *specialist*?"
   and the answer changes as new models ship.
2. **Better models arrive constantly.** New small/tool-use models and leaderboard
   movements (e.g. the Berkeley Function-Calling Leaderboard on Hugging Face)
   appear weekly. Picking the model by hand, once, does not scale and is not
   self-improving.

The improvement loop (beliefs → experiments → observations → reflection → process
change) already exists for the person's goals. The insight of this ADR is to
point that same loop at the **butler's own brain**: a model swap is fully
reversible, objectively measurable, and low-stakes — the ideal first instance of
"the butler improves itself," proving the pattern before it is aimed at real
infrastructure (containers, routers) where mistakes are expensive.

## Decision

Add a **scheduled model-discovery loop** — the improvement loop applied to the
model layer, with the agentic eval as its fitness function:

```
scheduled (heartbeat; a ModelDiscoverySchedule, same shape as BriefSchedule)
  DISCOVER  query Hugging Face (leaderboard + trending tool-use / small models),
            filter to candidates that fit the host VRAM (≤ ~9 GB Q4) and are
            servable (GGUF available for the local runtime)
  VET       keep only candidates newer or higher-ranked than the incumbent
  TEST      pull → run agentic_eval (+ the candour eval) → a scorecard
  RANK      compare the candidate's score to the current model's
  PROPOSE   if a candidate wins, the butler PROPOSES adoption
  ADOPT     on authorization, swap the model, record to audit/activity, form a
            belief, and reflect — the eval score is the reward that closes the loop
```

This composes the existing bounded contexts, not new architecture:

- **capabilities** — a reversible *model-registry* skill (list / pull / evaluate /
  swap via the local runtime's OpenAI-compatible + management API, plus HF
  discovery). HF fetches go through the egress guard (ADR 0023) against a curated
  allowlist. Endora still never *hosts* the model (ADR 0008 / model-agnostic
  boundary) — it manages *which* endpoint the butler points at.
- **scheduling** — the `ModelDiscoverySchedule` fires the loop from the heartbeat.
- **direction** — each discovery is an experiment; the eval is its observation;
  adopting a model is a process change.
- **policy** — **models propose; deterministic policy authorizes** (ADR 0005).
  Adoption is split by class (`decide_adoption`): a better **local** (keyless,
  self-hosted) model is **auto-adopted** — it is reversible, already available, and
  no data leaves the device, so it fits the "exhaust local before ranking up"
  ladder and the layer writes its config directly. A better **cloud** (keyed)
  model — which leaves the device and costs money — is only **proposed** for the
  person to confirm, never auto-adopted. The policy prefers a winning local over a
  higher-scoring cloud (adopt the local, don't propose the cloud), and requires a
  candidate to *strictly* beat the incumbent (a tie keeps the incumbent).

The eval remains a **proxy** for real quality, so a cloud swap keeps a human in
the loop, and any adopted model can be rolled back to the previously recorded-best
in one step.

## Consequences

- The butler runs on the best brain the hardware can hold, and keeps up with new
  models automatically rather than by manual, one-time selection.
- The agentic eval becomes load-bearing (the fitness function); it must stay
  representative, so it grows with the skills the butler gains.
- First concrete "self-improvement" feature, deliberately chosen for being
  reversible and measurable — a safe rehearsal for higher-stakes autonomy.
- New surfaces to guard: egress for HF discovery (allowlist + guard) and the
  bandwidth/disk cost of pulling candidates (sparse schedule, candidate cap,
  cache reuse).
- Sets up the escalation ladder (ADR-to-come): the same registry/eval machinery
  picks not just one model but a *tier* — small resident specialists locally,
  escalating to a bigger local or cloud model only when a task needs it.

## Alternatives considered

- **Pick the model by hand, once** — rejected: does not scale, is not
  self-improving, and goes stale as better models ship.
- **Always run the biggest model that fits** — rejected: VRAM-bound, and the eval
  shows a tuned specialist can out-route a bigger generalist, so "biggest" is not
  "best."
- **Auto-adopt the top-scoring model with no gating** — rejected: violates
  models-propose/policy-authorizes; a regression or a subtly worse model would
  silently degrade the butler. Gating first, autonomy later.
- **A separate external "model ops" tool** — rejected: the improvement loop,
  scheduling, policy boundary, and audit already exist in Endora; reusing them
  keeps one coherent self-improving system instead of a bolted-on pipeline.
