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

### 1. Measure understanding — *delivered*

Belief quality is the load-bearing behaviour and the thing with no fallback
(ADR 0029's stated risk). [ADR 0030](adr/0030-measuring-understanding.md) adds an
**L3 understanding tier** to the fitness battery — does the butler form beliefs from
real evidence, stay quiet when a turn reveals nothing, avoid re-filing what it
already knows, and refrain from overclaiming confidence — and an **adoption floor**,
so the model layer can no longer trade understanding away for tool-routing points on
its own.

The battery is now data-driven (37 cases) and runs repeatedly, reporting the spread
rather than hiding it. **Measured baseline for `qwen2.5:7b`, 3 runs: mean 29.7/34,
range 27–31, spread 4** — so any model comparison closer than 4 points is noise. That
number is the resolution of the instrument, and it is the prerequisite for any
fine-tuning or distillation work: without it there is no way to tell a real gain from
a lucky run.

> That baseline predates the three `verify:*` cases added for
> [ADR 0034](adr/0034-evidence-verifies.md), so it is out of 34, not 37. It needs
> re-measuring against the current battery before it is compared to anything.

What the first trustworthy run found:

- **`relay:failure-is-honest` fails 1 run in 3.** With a tool error in its immediate
  context, the model still sometimes narrates success. This is precisely the risk
  [ADR 0028](adr/0028-native-tool-calling-turn.md) accepted when it deleted the
  deterministic honesty nets — now quantified rather than assumed. Per that ADR the
  answer is a better model or a better prompt, **never** a canned string; whatever is
  tried can now be measured against this case.
- **`select:knowledge` and `select:web_search` fail 0/3** — consistent, not noise.
  Routing remains the axis that scales, exactly as the earlier evals suggested.
- **`no-duplicate` passes only 1 run in 3** — the model re-states what it already
  knows; the deterministic backstop (ADR 0033) is what stops it reaching storage.

Still ahead: growing the battery further (now cheap), and a *pinned* LLM judge to
measure whether a belief is genuinely insightful, which lexical scoring cannot see
(ADR 0030 alternatives).

### 2. Beliefs that behave like a model, not a list — *decay delivered*

[ADR 0032](adr/0032-beliefs-decay-and-expire.md): beliefs now **weaken without
reinforcement and fade out entirely**. `BeliefStatus::Expired` existed and was never
set by any code path — the "nothing is assumed permanently" promise was fiction.
Half-lives are per kind, encoding ADR 0020's own claim that intent changes slowly
(365 days) and a stressor does not (45); affirming resets the clock; the nightly loop
persists the forgetting so it is visible.

Still ahead here: **contradiction** (two beliefs that cannot both be true should be
surfaced, not silently coexist) and **consolidation** (several specific beliefs
subsumed by one the butler has since learned).

### 3. Interventions, properly — *the destination; sequenced below*

The reset promised interventions **proportional to confidence** — higher uncertainty
means a smaller action or just a question. Today the butler acts when asked and
researches overnight; it does not yet size an unprompted action to how sure it is.
This needs its own ADR, and it must not reintroduce a queue of records to approve.

Stated plainly: **nothing Endora does unprompted changes anything.** Every unattended
path ends in a message or a reversible read. That is the gap between a butler that
notices and one that acts, and it is the last big one. It is deliberately *last* in the
sequence below — it is the only step where being wrong has consequences, and it needs
both a track record to calibrate against and work that outlives a single turn.

### 4. Agentic proactivity — *delivered for check-ins*

[ADR 0031](adr/0031-agentic-proactivity.md): the check-in schedule is now a **budget,
not a trigger**. Deterministic code owns *how often* the butler may speak (minimum
interval, and never on top of someone who just spoke); the butler owns *whether* it
has anything worth saying, and the reason lands in the activity trail. The budget is
spent whether or not it speaks, so a "nothing to say" cannot become a retry loop.

The brief and the nightly loop stay time-anchored on purpose — both are legitimately
"at this hour" things.

Still ahead: letting a genuine event (a new high-confidence belief, a due reminder)
*open the budget early*, as an additional gate rather than as the decision itself.

## The sequence to unprompted action

Each step closes one clause of the architecture principle, and each exists because the
one after it cannot stand up without it:

> **Models propose. Policy authorizes. Capabilities execute. Evidence verifies.
> Memory learns.**

Clauses one to four are built. **Memory learns** holds for *beliefs* and not for
*actions* — nothing records whether something Endora did actually helped. That is the
first step, because "proportional to confidence" is otherwise uncalibratable.

### A. Unattended means reversible — *delivered*

The person's levers answer "may Endora do this **when I am here**". Opening an
irreversible tool and widening the envelope together cleared it to act inside a chat
turn — and the heartbeat shared that runner, so the nightly loop's documented guarantee
("nothing it could do that it couldn't undo") held only while the envelope happened to
be closed. `ReversibleOnlyRunner` clamps unattended turns to the `Observe` and
`Reversible` bands, so the claim is enforced rather than asserted. Not a wall in front of
step D — it is the baseline D deliberately opens a narrow, audited door through.

### B. Outcomes: what happened after Endora acted

The reward signal, and cheaper than it looks — [ADR 0034](adr/0034-evidence-verifies.md)
already produces the observation half. An outcome is that read-back plus the person's
lightweight reaction (helped / didn't / didn't notice), linked to the belief that
motivated it. `run_tool_turn` records one per non-`Observe` run; `butler_context` carries
a short track record so the butler can see how its own actions have landed; memory rights
apply as they do to beliefs. Needs an ADR.

### C. Durable intentions

Turns are capped at three to six rounds and reseed from chat history; no work survives a
turn, let alone a restart. An `Intention` — statement, originating belief, next step,
state, step budget — gives the nightly loop something to *continue* rather than restart,
and decays like a belief so it cannot accumulate.

The ADR must open by naming what this is **not**. [ADR 0029](adr/0029-delete-the-goal-tracker.md)
deleted a queue of records the person had to groom; this is Endora's own working memory,
reviewed the way understanding is reviewed and never managed. If the console grows an
"add intention" button, the ADR failed. Lives in `understanding` until a second consumer
earns it a context of its own.

### D. Proportional interventions

The payload. Confidence is a *model output*, so the sizing must be deterministic policy
that treats it as data — a pure function in `shared/kernel` beside `Reversibility`, never
an instruction in a prompt. The same lesson [ADR 0028](adr/0028-native-tool-calling-turn.md)
learned about honesty and [ADR 0033](adr/0033-what-understanding-admits.md) about
de-duplication.

Roughly: low confidence buys a question, medium buys `Observe`/`Reversible` work, high
buys `OutwardReversible` *only* where the person opened that capability and widened the
envelope — and `Irreversible` is never available unprompted, at any confidence, through
any gate. That last line is what keeps the door narrow.

**Blocked on a measured number, not on an opinion.** `relay:failure-is-honest` fails
**1 run in 3** ([ADR 0030](adr/0030-measuring-understanding.md)). Unattended action on a
model that misreports failure a third of the time is not shippable. Either the read-back
becomes the load-bearing honesty guarantee for actuations, or the model rungs up first —
and the battery says which, rather than a hunch.

### E. Then

- Event-driven wake: a genuine event (a new high-confidence belief, a due reminder)
  *opens the check-in budget early*, as an extra gate and never as the decision itself
  — the remainder [ADR 0031](adr/0031-agentic-proactivity.md) named.
- Belief **contradiction** and **consolidation**, the remainder
  [ADR 0032](adr/0032-beliefs-decay-and-expire.md) named.
- A sandbox in which the butler can author and run its own capabilities
  (ADR 0022's self-authored capabilities). Correctly last: the only step where the butler
  writes code that then runs, and it wants every prior step's machinery.
- The capability ladder: exhaust local before ranking up, and manage its own
  infrastructure as it goes.
- Native clients, if the web console ever genuinely stops being enough.

Throughout: **grow the battery with each step** — it is data-driven now, so cases are
cheap — and re-baseline afterwards reporting the *spread*, not the best run. The
instrument resolves to 4 points; a 3-point gain is noise.

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
