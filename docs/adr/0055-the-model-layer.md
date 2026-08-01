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

### A second model beats a bigger one, and the trigger is code

The constraint this section exists for is arithmetic, not a property of any model.
**Reliability compounds:** a model that obeys a procedural instruction about **one run in
three** — the measured figure this architecture rests on — turns an n-step task into (1/3)ⁿ.
Nothing about a longer chain is safe unless the steps stop being probabilistic.

That is why every guarantee here has been moved into code: a deterministic gate is *p = 1* and
does not compound downward. But some steps have to be a model, and for those there is a second
lever. Two independent attempts fail together only at (1-p₁)(1-p₂), so **a different model is
worth more than a bigger one** — and, unlike a bigger one, it exists today.

So when the local model fails a deterministic check, Endora asks a stronger one **once**:

- the check is the same `not_an_answer` used everywhere — empty, model unreachable, named a
  tool without calling it, protocol or frame-break prose — **plus repeating its own last
  answer**, which was observed live with the day's real record sitting in context;
- the trigger is **code**, never the model's opinion of how it did. A model that could tell it
  had failed would not have failed;
- the local model is tried, and retried, first. Nothing leaves the box on a first stumble;
- it is expressed on the **port** — "this butler knows a better one" — rather than threaded
  through nine call sites that mostly do not care.

**It is off by default, and that is not a reliability decision.** The deep model is usually
somebody else's API, and until this existed it was reached only when the person pressed a
button, so every use of it was them choosing to send that conversation off the box. Making the
fallback automatic without asking would quietly convert a local butler into one that phones out
whenever the small model stumbles — which it does often. Reliability is worth a great deal; it
is not worth deciding this on somebody's behalf.

**The daily brief is worded by it too, when the switch is on.** Measured rather than assumed:
given identical facts, the local model wrote *"the lights in your bedroom are off"* and the
deep one wrote a brief using every one of them. Six placements had already established that
the facts were reaching the local model and being ignored, which makes this a difference in
capability rather than a prompt left to tune.

**Gathering stays local, and only the facts leave.** The deep model is handed a context with
nothing else in it — no beliefs, no names for things, no track record — because writing prose
is no reason for any of that to travel. What does travel is disguised
([0051](0051-where-the-boundary-is.md)), and a brief never depends on somebody else's service
being up: if it says nothing usable, the local wording stands and the facts are appended
either way.

**Every escalated reply says so**, rather than relying on the person to remember a setting they
changed once. Which model answered is not a detail — it is where their words went.

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

### Bundling is a packaging decision, not an architectural one

The rejection above is about **code**. Nothing in Endora may know what serves the model — not
its format, not its lifecycle, not how to fetch one. That line stands.

It does not follow that a deployment cannot ship one. A compose profile that starts a
runtime alongside Endora and points it at the URL leaves Endora's code untouched: delete the
service and everything works exactly as before against somebody else's endpoint. So the
bundled runtime is **off unless asked for**, and exists for the person who wants a working
butler before they want to configure anything.

**The hardware detection belongs in the runtime, not here** — and that is settled by fact
rather than preference. Endora's container has no GPU access and cannot see a card even when
one is present; a container granted it reports the card immediately. Putting the choice in
the sidecar is therefore both the only place it works and the place that keeps the boundary.

**The ladder is not "the biggest that fits".** This project's own battery measured a 14B
failing to beat the 7B, and every model tested scoring 0/3 on following an explicit
instruction about verification. Size buys capability, not obedience — so the bundled choice
is the biggest that runs *comfortably*, and it is a starting point. Improving on it is the
adoption loop's job, gated on the battery rather than on specifications.

## Rejected

- **Hosting or bundling the model *in Endora*.** It would become a model runtime with a
  butler attached, and its lifetime would be tied to a vendor's. Still rejected — see the
  distinction below, which is about packaging rather than code.
- **A mixture of models** (a router plus a synthesiser). Measured on this hardware: it
  thrashes, and a 14B model is roughly 4× too slow. One 7B model, with guarantees in code.
- **Judging models by feel.** Three confident hypotheses died to the battery.
- **Adopting a model because it scored well on a public leaderboard.** The fitness function is
  *this* system's turn machinery, not a benchmark.
- **A confidence threshold** as an authorization mechanism, at any point.
- **Escalating on the model's own judgement** that it needs help. A model able to tell it had
  failed would not have failed.
- **Escalating by default.** It is a change to where the person's words go, not a tuning knob.
- **Waiting for a better local model** to fix procedural compliance. Measured: a 14B did not
  beat the 7B on the battery, and every model tested scored 0/3 on verification. Scale buys
  capability faster than it buys obedience.
