# Endora foundation reset and OpenClaw benchmark

Proposed plan, 2026-09-07. Prepared with AI assistance. This is the controlling
implementation sequence for dependable delegation, superseding completion claims
and sequencing in `jarvis-next-steps.md` and `durable-goal-loop.md`. It does not
claim runtime parity or approve a replacement architecture. No production changes
are part of this planning pass.

## Decision in plain terms

Stop expanding the current execution layer. Preserve useful product assets, but
require the runtime to earn its place in a build-versus-adopt evaluation. The
default preference is to adopt an existing runtime if it meets the requirements
with less maintenance. Choose a custom Endora runtime only for demonstrated needs
that a supported integration cannot meet. Do not maintain two autonomous brains.

Rust, a modular monolith, SQLite, HTTP/SSE, and MCP are reasonable foundations.
The central problem is fragmented execution and weak behavioral acceptance, not
the programming language. Neither a complete rewrite nor more endpoint patches
is justified before the comparison.

The product promise is: ask in ordinary language, watch useful progress, let the
assistant change approach within your permissions, interrupt it, leave and return,
and receive a supported answer or an honest explanation of what remains missing.

## What the reference actually establishes

OpenClaw documents a session runtime integrating intake, context, tools, streaming,
persistence and serialized execution. Its current documentation describes writer
ownership checks that reject stale transcript writes. These are useful reference
mechanisms; documentation alone is not evidence that a particular deployment
completes the user's assignments. [Agent loop](https://docs.openclaw.ai/concepts/agent-loop).

Its goal feature holds a durable session objective; goals are distinct from
background tasks and recurring automation. Endora must therefore test both
reasoning within a run and continuation across runs instead of assuming a goal
record itself supplies either. [Goals](https://docs.openclaw.ai/tools/goal).

The reference separates assistant/tool events and startup progress. Delivery
channels have different streaming behavior, so compare the browser experience
directly rather than promising token streaming on every channel.
[Streaming](https://docs.openclaw.ai/concepts/streaming).

Search, lightweight fetch and browser access are separate tools. Endora needs a
usable general retrieval path, with source-specific adapters as optional fast
paths. [Web tools](https://docs.openclaw.ai/tools/web).

Pin the exact reference release, model, configuration and tool set in the benchmark.
This plan targets the behaviors below, not the entirety of OpenClaw's plugin catalog.

## Audit findings that reopen the foundation gate

These are code-review findings, except the ESPN 403, which was reproduced on the
deployed container during the preceding investigation. Fault-injection tests must
reproduce the concurrency findings before changes are implemented.

| Current code | Consequence | Required replacement or repair |
| --- | --- | --- |
| `Goal::run_plan` makes every executor error terminal | A source outage ends the objective | Typed errors and bounded replanning |
| Sports route provides a single step and hardcodes count three | Alternative tools and other requested counts are not honored generally | Extract constraints once; optional schedule adapter within the general loop |
| `sports_schedule_satisfied` checks numbered prose and `Source:` | Formatting can masquerade as verification | Validate structured events, identity, timestamps and source evidence |
| Foreground sports saves before and after the entire lookup | Intermediate reads are not durable checkpoints | Commit each completed operation before the next |
| Sports request keys are generated on each invocation | A client retry can create another goal | Stable client request key with payload conflict checks |
| Workers reserve before obtaining the turn lock | Busy chat consumes attempts without doing work | Separate scheduling claims from external-attempt budgets |
| Lease lasts 30 seconds, has no renewal, and only fences release | Slow work can outlive ownership; old workers can still write | Owner/generation checked on every mutation, renewal and publication |
| Goal checkpoints use an insert-or-update save | A late save after purge can recreate the deleted goal | Separate create from update; updates require existing ID and generation |
| Exhausted lease reservations return false without terminalizing work | A goal can remain active but never run | Explicit budget-exhausted transition and visible reason |
| Research and sports have separate worker lifecycles | Sharing a lease table has not unified execution | One lifecycle owner with domain-specific evidence adapters |
| Research holds the lock across a multi-round turn | Foreground chat may wait through the whole run | Yield scheduling at bounded operation boundaries |
| Research appends sources to generated prose before delivery | Retrieved links do not prove claim support or task coverage | Source IDs, claim mapping, coverage checks and partial results |
| Goal notice points to “open the goal”; listing returns active goals only | Completed evidence is difficult to discover in the console | Linked persisted result and terminal history in chat |
| Browser stream abort is separate from durable cancellation | Stop can disconnect the display while work continues | Explicit server cancellation contract |
| Streaming fallback can use a one-shot response | Streaming support does not guarantee visible generation | Trace every selected provider/wrapper path through the actual browser |

Ownership: orchestration changes belong in `crates/endora-application`; storage in
`crates/endora-infrastructure/src/sqlite.rs`; capability behavior in its context;
HTTP composition in `app/node/src/api.rs`; display in `app/node/src/web/app.js`.
Move worker decisions out of the HTTP module as the slice is replaced, without
turning this into a repository-wide formatting or directory refactor.

## Keep, replace, freeze, retire

| Treatment | Components | Condition |
| --- | --- | --- |
| Keep and characterize | Authentication, policy-gated capabilities, MCP connections, settings, guarded egress, audit, memory rights, console shell | Preserve contracts; do not assume every implementation is correct |
| Replace or substantially rework | Goal orchestration, two worker loops, lease ownership, completion evaluation, event delivery | Must pass the shared runtime tests |
| Reuse as adapters | Sports parser, research source handling and saved reports, model adapters | No independent scheduling or completion authority |
| Freeze expansion | Nightly prose/reflection, automatic model adoption, recipe authoring, more configuration screens, native clients, new integrations | Resume only for a demonstrated acceptance scenario |
| Retire after cutover | Superseded workers, duplicate publication paths, keyword routes that bypass planning, obsolete completion claims | Zero callers, data migration verified, rollback window closed |
| Later, gated | Scoped browser/file execution, standing tasks, voice interruption, delegation, more channels | Each adds a useful workflow on the same proven runtime |

Do not delete memories, integrations or reports merely to reduce code. Freeze is
a development decision in this plan; live feature settings have not been changed.

## Phase 0 — Choose the foundation (timebox: 3–5 engineering days)

1. Preserve current dirty work and identify the exact deployable snapshot. Start
   implementation from develop carrying required changes deliberately. Record a
   content/image digest: the current git SHA does not distinguish dirty builds.
2. Freeze a common benchmark and reproduce baseline failures with synthetic data.
3. Run a pinned OpenClaw instance in isolation with synthetic accounts/tools and
   the same model and permitted capabilities. No production secrets are required.
4. Compare three paths: adopt OpenClaw with supported Endora integrations; replace
   only Endora's orchestration through a supported runtime interface; retain and
   repair the custom runtime. Verify extension APIs/license/upgrade costs before
   selecting an integration design. A runtime embedded inside another full runtime
   is not an acceptable default.
5. Compare model quality separately: run the current model and a stronger allowed
   candidate on the same tasks. A weak model must not be hidden behind more
   keyword rules. Cloud use remains opt-in and within existing data-egress rules.

Hard requirements: cancellation, privacy boundaries, streaming, persistent state,
observable failures, useful results and acceptable operation on the target host.
Then score task success, latency, operating cost, migration work and maintenance.
Proposed weights: 40% usefulness, 25% reliability, 15% maintenance, 10% latency,
10% cost. Hard requirements cannot be traded away by a weighted score.

Exit: one ADR proposes the chosen runtime, costs, concrete migration map and
rejected alternatives with evidence. If OpenClaw passes and integration is simpler,
adopt it. If custom work wins, name the measured differentiator. If neither passes,
resolve the model/tool limitation before expanding either. Missing reference access
means the comparison is incomplete, never that Endora wins by default.

## Phase 1 — One observable, durable execution path (5–10 days after selection)

Build or adapt these contracts once:

- Request: session ID, stable request key, original message, extracted constraints,
  objective revision, authority snapshot reference and input attachments.
- Run: ID, lifecycle, owner generation, expiry, budget, next operation and failure
  history. Clarifications become explicit revisions authorized by the user.
- Step: typed tool/model input, dispatch reservation, result, timestamps, error
  category and evidence IDs. Tool results retain untrusted provenance.
- Result: supported answer or artifact, unmet constraints, terminal reason and
  transactional completion outbox linked to the originating chat message.
- Events: monotonic sequence, run ID, accepted/queued/preparing/tool-start/tool-end/
  retry/waiting-user/verifying/answer-delta/complete/cancelled/error.

One scheduler advances a bounded operation and yields. Separate model capacity
from read I/O so a slow fetch does not own the model lock. Reads may repeat after
a crash; consequential operations require their own idempotency/reconciliation.
Every checkpoint and completion verifies ownership and cancellation atomically.
An expired lease is not proof its old operation stopped. Cancel/purge invalidate
that generation; no late result can publish or recreate work.

The console shows acceptance immediately, current operation, useful approach
changes and answer deltas. Persist lifecycle/tool events and bounded draft
snapshots; resume after reconnect using an event cursor. Show waiting honestly
when a provider offers no text yet. Do not fabricate a reasoning transcript.
Stop calls the server; closing the browser merely detaches. Distinguish both.

Exit: interruption, contention, long-call, reconnect, cancel, purge and duplicate
delivery tests pass through the production composition and browser.

## Phase 2 — Adaptive solving with general tools (5–10 days)

Ordinary chat enters the same runtime. The model interprets the request and
proposes the next useful operation; deterministic code validates arguments,
permissions, budgets and lifecycle. Simple conversational replies can finish
without creating an enduring background assignment.

Provide normalized search and fetch through configured integrations. A specialist
API is a fast path. On source refusal, choose another independently permitted
source; never evade access controls. Timeouts/429 use bounded backoff. Invalid
arguments may be repaired. Missing credentials require setup. Policy denial does
not authorize a substitute that defeats the same restriction. Record why each
approach failed and prevent identical retries without new information.

Define completion by task: structured schedule checks; claim/source and coverage
checks for research; actual postcondition reads for actions. Model judgment may
assess semantic coverage but is not proof by itself. Permit useful partial results
with gaps; never call an appended source list verification.

Demonstration: next-three-games request → first source returns 403 → search another
source → fetch evidence → validate future dates/opponents/count → stream a supported
answer. Reuse the same loop for a comparison report and a public event lookup,
including unfamiliar phrasings. No new routing regex to make the benchmark pass.

Exit: all three task families work, including forced first-source failure and
all-sources-unavailable cases. Generalization is tested with held-out prompts.

## Phase 3 — Migrate and remove competing paths (3–7 days)

Map existing research and sports records into one lifecycle, referencing existing
report/evidence records rather than copying their authoritative content. Preserve
legacy IDs and v1 routes through adapters. Backfill transactionally with schema
versions and repeated-open tests. Quarantine unsupported checkpoints visibly.

Make the old workers drain or disable them before the new worker takes ownership;
never let both advance the same assignment. Validate read-only shadow comparisons
before switching writes. Route ordinary chat research through this contract and
update ADR 0077's explicit-composer restriction as part of the accepted change.
Preserve ordinary questions and follow-ups without accidental new commissions.

After successful cutover, remove dead workers and independent completion paths.
Keep historical artifacts readable. Rollback uses a compatible image or a verified
pre-migration backup, not an old writer against an incompatible schema.

Exit: no duplicate lifecycle owner, old URLs still work, report/history counts and
checksums reconcile, and a rollback rehearsal succeeds on an isolated copy.

## Phase 4 — Release gate and daily-use trial (3–5 days plus 7 calendar days)

Extend the existing 20 acceptance scenarios to 40: 10 task usefulness/interpretation,
10 recovery/ownership, 8 interaction/streaming, 8 authority/provenance, 4 migration/
deployment. Include counterexamples, midnight/timezone and season transitions,
ambiguity, user correction, provider outage and exhausted budgets. Freeze fixtures
and held-out prompts before implementing the behaviors they test.

Run each live quality prompt three times on each compared configuration. Record
model/version, actual tool calls, evidence, tokens/cost, latency, errors and result.
Inspect claims against retrieved sources. An LLM judge may assist but is not the
sole reviewer. Reference runtime results remain separately labeled.

Proposed release thresholds:

- All mechanical/policy scenarios pass; zero false success, unauthorized action,
  fabricated reference or duplicate completion in evaluated runs.
- At least 90% useful outcomes on answerable tasks; unanswerable tasks must explain
  the actual gap. Do not remove hard cases to reach the threshold.
- Browser acceptance visible within one second on the local network; received
  model text displayed within 500 ms; reconnect restores progress within two seconds.
- No unexplained silence over ten seconds: show the actual waiting phase and
  elapsed time. Measure model generation latency separately from UI latency.
- Cancellation acknowledgement within one second; no new dispatch afterward.
  Already dispatched operations remain bounded by transport deadlines.
- No budget spent solely waiting for capacity; foreground gets the next eligible
  slot after the current nonpreemptible operation.

Run all six repository checks plus behavior smoke against the isolated deployed
image. Existing ten read-only smoke checks remain required but cannot establish
assistant competence. Deploy a content-identified candidate, wait for health, then
smoke sequentially. Observe seven days of real use before declaring this milestone
dependable. A severe failure reopens its gate rather than generating another feature.

## Phase 5 — Expand only along proven workflows

In order: retrieval of relevant past work with freshness/provenance; one revocable
standing task; scoped browser and file work; voice interruption; then optional
delegation and extra channels. Each capability needs a real task, a before/after
success measure, an owner, a bounded authority contract and a retirement decision
for what it replaces. Delegation is a later product capability, not a prerequisite
to solving the first question. Do not add shell access to compensate for missing
search or broken scheduling.

## Rules that should change versus rules that should remain

Keep deterministic authorization, privacy, cancellation and memory rights. These
do not require hand-scripted thinking. Let the model choose permitted approaches,
tools and queries; let evidence and policy constrain what it claims and does.
Treat tone/personality instructions as presentation, never routing rules.

Reconcile the reflection-oriented constitution introduction with the desired
assistant purpose through an explicit proposed amendment, not a silent edit.
Review the current model-escalation/taint policy against the selected runtime:
if evidence synthesis needs a stronger model, define the permitted data path and
user choice before implementing it. Do not bypass restrictions in a retry.

## Effort, scope control and next action

Planning ranges above assume one experienced implementer with reliable model and
tool access. They total roughly 4–8 engineering weeks plus observation for a custom
replacement; adopting a runtime may shorten this, while migration blockers may
extend it. These are estimates to revise after Phase 0, not delivery promises.

No additional milestone document is needed to start. Execute Phase 0, maintain
evidence and gate status in this file, and issue the runtime-choice ADR. A checkpoint,
trait, test count or deployment is never reported as a completed user capability.
Every shipped slice must include intake, execution, visible progress, result,
failure behavior, acceptance evidence and retirement of its predecessor.
