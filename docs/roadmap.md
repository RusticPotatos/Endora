# Endora Roadmap

> Status: living document. This roadmap states **intent and sequencing**; it is
> not authoritative for decisions. Architectural decisions are recorded in
> [ADRs](adr/README.md), and execution is tracked in GitHub issues/milestones.
> When this roadmap and an ADR disagree, the ADR wins.
>
> Rewritten 2026-07-25 after [ADR 0029](adr/0029-delete-the-goal-tracker.md). The
> previous version planned releases 0.5→1.0 around the goal tracker (North Stars,
> targets, experiments, reflections). That machinery is gone; so is the plan built
> on it. The canonical statement of direction is
> [docs/direction-reset.md](direction-reset.md).

## What Endora is

An **autonomous personal intelligence**, running on your hardware. Its job is to
understand a person and act usefully in their interest — not to give them somewhere
to file their goals. See [direction-reset.md](direction-reset.md) and
[ADR 0020](adr/0020-intent-first-understanding-loop.md).

The guiding test for every feature, unchanged: *if this disappeared tomorrow, would
Endora still feel like an autonomous personal intelligence?* If yes, it is probably
just tooling.

## Where we are

Built and working:

- **Understanding** — beliefs about the person with evidence, confidence, and the
  ability to be affirmed, corrected, or to expire. The home surface, and the only
  model Endora keeps of a person (ADRs 0020, 0029).
- **One tool-calling turn** — the butler runs its skills through the policy layer and
  answers grounded in the real results, success or failure, with no deterministic
  narration papering over either (ADR 0028).
- **Skills and the MCP host** — built-in skills plus any MCP server, each behind
  per-skill configuration and enablement (ADR 0021).
- **The safety machinery** — reversibility bands with deny-by-default, the autonomy
  envelope, the SSRF egress guard and outbound secret tripwire, and an audit trail
  (ADRs 0005, 0022, 0023, 0024).
- **Proactivity** — optional check-ins, a daily brief, and a nightly loop that
  researches whatever Endora is most sure the person is reaching for, reflects, and
  leaves a note (ADRs 0019, 0024).
- **A model layer that can improve itself** — a fitness battery and a deterministic
  adoption policy: auto-adopt a better local model, only *propose* a cloud one
  (ADR 0027).
- **Memory rights** — everything visible, correctable, exportable, deletable.

## The near arc

Sequenced by what unblocks what, not by release number. Tagging is a human decision.

### 1. Measure understanding

Belief quality is now the load-bearing behaviour and the thing with no fallback
(ADR 0029's stated risk). Extend the [0027](adr/0027-self-improving-model-layer.md)
battery from routing and no-fabrication to **understanding**: does the butler form
beliefs with real evidence, avoid restating what it already believes, and correctly
decline to form one when the conversation supports nothing? Until this is measured,
every other improvement here is guesswork.

### 2. Beliefs that behave like a model, not a list

Expiry and decay are modelled but barely exercised. Beliefs should weaken without
reinforcement, contradict each other visibly, and be consolidated when the butler
learns something that subsumes several. This is what makes understanding a *living*
model (direction-reset, "Understanding is a living model").

### 3. Interventions, properly

The reset promised interventions **proportional to confidence** — higher uncertainty
means a smaller action or just a question. Today the butler acts when asked and
researches overnight; it does not yet size an unprompted action to how sure it is.
This needs its own ADR, and it must not reintroduce a queue of records to approve.

### 4. Agentic proactivity

Check-ins and briefs are on the single tool-calling turn now, but the *decision to
reach out at all* is still a clock. It should come from what Endora understands and
has noticed — "it comes to you," but for a reason it can state.

### Later

- A sandbox in which the butler can author and run its own capabilities
  (ADR 0022's self-authored capabilities, deliberately still ahead).
- The capability ladder: exhaust local before ranking up, and manage its own
  infrastructure as it goes.
- Native clients, if the web console ever genuinely stops being enough.

## What 1.0 would mean

Not a feature count. **1.0 is when Endora understands a person well enough, and
acts well enough on that understanding, that a non-author would keep it running** —
with the constitutional guarantees enforced in code, not merely documented:
deterministic policy authorizes every consequential action, memory stays visible /
correctable / exportable / deletable, and the model is never the enforcement
boundary.

The `0.x` protocol is unstable by design until then; tagging `1.0.0` commits to
compatibility within the major version, so it is a deliberate human decision.

## Cross-cutting, in every release

- **Models propose; deterministic policy authorizes.** Never routed around.
- **Reversibility first.** Endora acts alone only within reversible bounds.
- **Memory rights** hold for everything stored, including audit records.
- **Sycophancy and fabrication are defects measured by evals**, never left to the
  model's discretion — and never patched over with a canned string (ADR 0028).
