# 0005 — Models propose; policy authorizes

## Status

Accepted (2026).

## Context

Endora uses AI models as reasoning components. Models are probabilistic and can
be wrong or manipulated (including via prompt injection). The constitution
requires that human autonomy stay final, that consequential actions require
explicit authority, and that the language model is **never** the final
enforcement boundary. We need a structural rule — not just a convention — that
keeps model output from directly causing privileged effects.

## Decision

Establish a hard architectural boundary: **models propose, deterministic policy
authorizes, capabilities execute.**

- Model output is treated as a **proposal**, never as an authorization.
- A **deterministic policy layer** (the Policy & Consent context) decides whether
  a proposal is permitted, given the current autonomy level, consent, and
  reversibility/proportionality checks.
- **Capabilities** execute only what policy authorized, under least authority.
- No privileged capability is ever exposed directly to a model.
- Autonomy levels are modeled in the domain (`endora_domain::AutonomyLevel`) and
  default to the most conservative posture (observe only).

## Consequences

- A compromised, hallucinating, or prompt-injected model cannot by itself cause a
  consequential effect; it can only produce a proposal that deterministic code
  will reject or escalate.
- The security-critical decision path is deterministic and therefore testable,
  auditable, and reviewable.
- Every consequential path must be routed through policy — convenience shortcuts
  that let a model act directly are prohibited and are a review/CI concern.
- Prompt injection that attempts to cross the deterministic boundary is an
  in-scope security issue (see [SECURITY.md](../../../SECURITY.md)).

## Alternatives considered

- **Trust the model within a system prompt / guardrail prompt** — rejected: a
  prompt is not an enforcement boundary; it can be bypassed.
- **Let models call tools directly with post-hoc checks** — rejected: makes the
  probabilistic component the de facto authority; violates the constitution.
- **Human-in-the-loop for everything** — rejected as the only mechanism: does not
  scale and is unnecessary for reversible, pre-authorized actions; humans are
  reserved for what actually warrants their authority.
