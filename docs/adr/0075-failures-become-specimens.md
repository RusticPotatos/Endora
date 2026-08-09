# 0075 — Failures become specimens

## Status

Accepted (2026-08-08). Extends the loop spine (ADR 0052/0053's "memory learns")
to the butler's own failures; changes nothing about how a turn runs.

## Context

A turn that ends at the honesty valve is a measurement thrown away. The person
asked something real; every deterministic check the machinery has — not-an-answer,
repeats-itself, gave-up-after-a-failure — rejected what the model produced; the
deep model did no better; and the only trace was a log line and an apology.

Meanwhile the eval battery — the instrument that measures whether any of this
improves — grows only by hand. Every case added this month was a live failure a
person noticed, diagnosed, and carved into `eval.rs`. The battery's own policy
(correctly) forbids the shortcut: harvesting a live database into a checked-in
fixture would put someone's private conversation in git, which the constitution
forbids. So the fleet-wide battery cannot learn from this house's failures — and
nothing else did either.

The result: the butler got better all week, and the only way anyone knew was the
person re-asking questions that had failed before. The system never noticed
itself improving, because it kept no record of what it had failed at.

## Decision

**A failed turn files a specimen.** When a chat turn's reply is rejected by the
same deterministic verdicts that gate retries and escalation (`degraded`, or
`not_an_answer`), the ask and the verdict are filed as a `Specimen` — in the
house's own database, alongside beliefs and outcomes, and **never** in a
checked-in fixture. Only answer-shaped failures file: a turn that acted has an
outcome record already, and replaying an action unattended is a different
mechanism this ADR does not build. The model's opinion of itself files nothing.

**The nightly loop replays one.** Each night, the oldest open specimen is
re-asked against the current machinery — under the same reversible-only runner as
the rest of the night, so a replay can gather and never actuate — and judged by
the same verdict that filed it. Passing retires it, and the morning trail says
so: *"A question that once stumped me answers now."* Failing is recorded too,
and enough failures retire it unresolved — re-asking past two weeks stops being
information.

**The shelf is bounded.** At most `MOST_SPECIMENS_OPEN` (12) open at once, one
replay per night, duplicates of an open ask not refiled. A backlog deeper than
the shelf is a signal to fix the machinery, not to queue more evidence.

## What this is not

- **Not a second battery.** The checked-in battery measures *model fitness* with
  synthetic fixtures and stays hand-authored, per the constitution. Specimens
  measure *this house's* regressions with this house's own asks, privately. The
  two share their judging vocabulary (the deterministic verdicts) and nothing
  else; no `EvalCase` is generated, and nothing here writes to the repository.
- **Not model self-assessment.** Filing, replaying, and retiring are all decided
  by code-applied predicates. The pattern budget's inventory is unchanged — this
  composes Repository and the existing loop; no new pattern, and nothing to
  retire because no prior mechanism did any part of this.

## Consequences

- The scorecard's neighborhood gains a second reward signal: read-back confirms
  what acting achieved; specimens confirm what answering achieved. Both derived,
  neither stored as a verdict.
- Replays cost at most one bounded tool-turn per night, and can reach external
  read skills (news, weather) exactly as the nightly loop already may.
- A replayed action-shaped ask ("turn off the lights") that ends in a
  permission-shaped answer counts as passing — under an unattended runner that
  *is* the correct answer. Accepted imprecision, noted here deliberately.
- The natural next consumers — a console shelf view, and specimen-informed
  focus-picking for the nightly intention — are left for the need to arrive.
