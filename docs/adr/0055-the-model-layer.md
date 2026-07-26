# 0055 — The model layer

## Status

Accepted (2026-07-26). **Consolidates 0008, 0027 and 0030**, which are archived.

## Context

Endora is meant to outlive any particular model. It also runs on one person's hardware, where
the model is a ~7B local one with measurable, unglamorous limits — and where "just use a
bigger model" is a real constraint, not a preference.

Two things follow: the model must be replaceable without touching anything that matters, and
"is this model good enough?" has to be a **measurement**, not an opinion.

## Decision

### Replaceable, behind a port, model-agnostic

The model is reached through an **application-defined port** implemented by an adapter
speaking to a local **OpenAI-compatible** HTTP endpoint. The domain never references a model.
Endora does not host, manage, or bundle the model: it ships guidance and tested setups, and
connects to whatever is at the configured URL.

That line is deliberate. Hosting a model would make Endora a model runtime with a butler
attached, and would tie the project's lifetime to a vendor's.

### The eval battery is the fitness function

Model quality is measured by a **battery of cases** run against the real turn machinery, not
by vibes:

- tiered (**L1/L2/L3**) from basic instruction-following up to understanding;
- probes that cover the situations that actually break — with and without skills, after a
  tool result, after a **failure**, with a crowded catalogue, across a conversation;
- data-driven, so a case is a row rather than a code change.

**The battery earns its keep by refuting things.** Three separate hypotheses about why a
light kept failing were held confidently and killed by it. It also produced the numbers this
architecture rests on — roughly **1-in-3** compliance with an explicit instruction about
verification — which is why guarantees live in code
([0051](0051-where-the-boundary-is.md)).

### Adoption is gated, deterministic and reversible

A candidate model is discovered, filtered to what the host can actually run, evaluated
against the battery, and adopted **only if it clears the floor** — a deterministic threshold,
applied by policy. The stored configuration wins over environment defaults, and the active
model is visible.

## Consequences

- The model can be swapped, upgraded or downgraded without touching the domain.
- Claims about model behaviour are checkable, and several confident ones have been wrong.
- Measurement is what made "the model cannot be the enforcement boundary" a fact rather than
  a posture.
- **The battery costs real time to run** against a local model. Worth it, and the reason it is
  not run on every change.
- A weak local model shapes the whole architecture — deterministic recovery, disclosure,
  policy-side guarantees. That is a feature: it forces honesty that a stronger model would let
  us skip until it mattered.

## Rejected

- **Hosting or bundling the model.** Endora would become a model runtime with a butler
  attached.
- **A mixture of models** (a router plus a synthesiser). Measured on this hardware: it
  thrashes, and a 14B model is roughly 4× too slow. One 7B model, with guarantees in code.
- **Judging models by feel.** Three confident hypotheses died to the battery.
- **Adopting a model because it scored well on a public leaderboard.** The fitness function is
  *this* system's turn machinery, not a benchmark.
- **A confidence threshold** as an authorization mechanism, at any point.
