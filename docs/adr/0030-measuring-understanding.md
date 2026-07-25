# 0030 — Measuring understanding: the L3 eval tier and the adoption floor

## Status

Accepted (2026-07-25). **Amends [0027](0027-self-improving-model-layer.md)** — extends
its fitness function with a third tier and adds one rule to its adoption policy.
Follows directly from [0029](0029-delete-the-goal-tracker.md), which made
understanding load-bearing.

## Context

[ADR 0029](0029-delete-the-goal-tracker.md) deleted the goal tracker and stated the
risk plainly: *"understanding now has no structured fallback. If belief-forming is
weak on a given model, Endora has less to go on than before, where an explicit North
Star could carry it."*

That risk is currently unmeasured, which makes it unmanaged. Worse, it interacts badly
with the model layer. [ADR 0027](0027-self-improving-model-layer.md) has Endora
**auto-adopt** a better local model, ranked by `Scorecard::total()` — and that total
only counted skill routing (L1) and the "Jarvis" behaviours (L2). A model that routes
tools brilliantly and is blind to the person would score *higher* than the incumbent
and be adopted automatically, silently degrading the one thing there is no fallback
for. ADR 0027's own rejected alternative — *"a regression or a subtly worse model
would silently degrade the butler"* — describes the hole its scoring now leaves open.

ADR 0027 anticipated this: *"the agentic eval becomes load-bearing; it must stay
representative, so it grows with the skills the butler gains."* The butler's job
changed; the battery has to follow.

## Decision

### 1. An L3 "understanding" tier

`Scorecard` gains `l3` / `l3_max`, and `evaluate` gains eight cases that score what
Endora now depends on:

| Case | What it catches |
| --- | --- |
| `forms-understanding` | Notices anything at all from a turn that plainly reveals something. |
| `evidence-grounded` | Cites words actually present in the conversation, not an invented quote. |
| `declines-on-nothing` | Stays silent on "what's 2 + 2?" — a belief every turn is noise the person must correct. |
| `no-duplicate` | Doesn't re-file what the context already says it understands. |
| `kind-accuracy` | Files a plainly-stated aim as intent, not a passing preference. |
| `confidence-calibrated` | Doesn't claim high confidence from one hedged remark (constitution §4). |
| `second-person` | Writes "you …", the stored form the view and the next turn's context assume. |
| `no-jargon` | Keeps "belief"/"confidence" out of the reply (ADR 0017). |

**The negative cases are gated on the model having formed something.** Otherwise a
butler that forms *nothing* passes `no-duplicate` and `confidence-calibrated` for
free — silence would score as good judgement. A unit test pins this by scoring the
offline `ScriptedButler` and asserting it cannot exceed 2/8.

**Scoring is lexical, not model-judged.** A model grading its own understanding makes
the fitness function circular; an LLM judge makes the score non-deterministic and
unauditable — the same objection [0028](0028-native-tool-calling-turn.md) raises to
trusting a model's self-report. The heuristics are coarse and are themselves unit
tested. They catch invented evidence, duplicate filing, and overconfidence. They
**cannot** judge whether a belief is insightful, and nothing here should be read as
claiming they do.

### 2. The adoption floor

`decide_adoption` takes the incumbent's full `Scorecard` rather than a bare total,
and applies one new rule:

> A candidate that wins on total but scores **lower on L3** than the incumbent is
> never auto-adopted. It is **proposed** instead.

Adoption still requires strictly beating the incumbent overall, and local is still
preferred over cloud. Understanding is a **veto on automatic adoption, not a way to
win** — a candidate that scores worse overall stays out however well it understands.
The person decides whether tool-routing gains are worth an understanding loss; that
is exactly the class of judgement ADR 0027 reserves for a human.

## Consequences

- The model layer can no longer trade away understanding on its own. The failure mode
  it protects against — "adopted a better router, quietly stopped knowing me" — would
  have been nearly invisible, since nothing else in the system reports on belief
  quality.
- `Scorecard::total()` and `max()` change (15 → 23). Scores are **not comparable
  across this change**; any recorded baseline predates the tier and should be re-run.
- `decide_adoption`'s signature changes from `usize` to `&Scorecard`. Internal, and
  the extra information is the point of the change.
- The `agentic_eval` harness asserts `l3 >= 3` alongside the existing `l1 >= 3`: a
  model that cannot understand the person is not a viable butler brain, however well
  it routes tools.
- The battery grows from 15 to 23 live model calls, so a run is slower. It is a
  scheduled job, not a request path.

## Alternatives considered

- **Use an LLM judge for understanding quality.** It would measure far more than
  lexical overlap can — genuine insight, not just grounding. Rejected for now: it
  makes the fitness function non-deterministic and unauditable, and if the judge is
  the model under test it is circular. Worth revisiting with a *fixed, pinned* judge
  model held separate from the candidate pool.
- **Weight L3 higher than L1/L2 in the total.** Rejected as arbitrary — the numbers
  would encode a trade-off nobody has justified. A hard floor on regression states the
  actual intent ("do not lose this without asking") without inventing an exchange rate.
- **Block adoption outright on any L3 regression, with no Propose path.** Rejected:
  it would freeze the model layer whenever the eval is noisy, and it takes a real
  decision away from the person rather than surfacing it.
- **Leave adoption alone and only report L3.** Rejected — the report would be
  advisory while the layer kept auto-adopting on a total that ignored it. Measuring a
  risk and then not acting on the measurement is the same as not measuring it.
