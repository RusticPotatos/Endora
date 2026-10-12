# Research assistant: five sequential implementation packets

The broader product sequence and frozen milestone scenarios now live in
[`jarvis-next-steps.md`](jarvis-next-steps.md) and
[`jarvis-acceptance-scenarios.md`](jarvis-acceptance-scenarios.md). This checklist
remains the implementation record for the commissioned-research slice.

**Smallest complete slice implemented.** AI-assisted (Codex/Astra), 2026-09-06.
Design: [handoff](research-assistant-handoff.md). Decision:
[ADR 0077](../adr/0077-seeing-something-through.md), accepted 2026-09-06.

Implementation validation (2026-09-06): formatting, Clippy, 913 unit/integration
tests plus one doc test, console rendering, personal-data scan and diff whitespace
checks passed. The normal suite leaves fourteen live/model tests ignored. The new
research eval was also run separately and passed with the local `qwen3:14b` model;
the other live/model tests were not run.

## Start gate

- [x] Review current code and specify scope, contracts and tests.
- [x] Revise proposed ADR without marking it accepted.
- [x] Save handoff and execution checklist.
- [x] Owner accepted ADR 0077 on 2026-09-06.
- [x] Verified repository root/current work; implementation continues on a branch
  from develop carrying the accepted plan. Do not assume this planning branch is
  merged or lose its files. No PR/deployment unless separately requested.

Use one implementing task sequentially. Sol is suggested for packets 1–2;
Terra for 3–5, escalating concrete hard defects to Sol. These allocations are
suggestions, not guarantees about subscription consumption. No self-acceptance
of architecture or silent weakening of a contract to finish a packet.

## Packet 1 — Commissioning, lifecycle and storage (Sol)

Prerequisite: accepted design. Outcome: explicit work can be atomically persisted,
read, cancelled and exported. Worker does not start yet.

- [x] Test origin invariants, lifecycle, immutable assignment, delivery
  requires validated report, terminal/cancelled work cannot advance.
- [x] Implement pure research types, origin-specific lookup, application DTOs
  and ResearchWorkStore operations from handoff sections 3–4.
- [ ] Canonical fresh/legacy migration through real startup; remove duplicated
  intention schema/decoder; retain all old IDs/fields and old API representation.
- [ ] Atomic commission/reserve/checkpoint/finish/cancel/recovery, with typed
  conflicts and conditional updates; context-owned SQL helpers share transaction.
- [x] Add authenticated resource/status/create/cancel/readiness routes and
  exact error/status contracts. No UI release before packet 4.
- [x] Extend HTTP export/purge to all new data and existing omitted intentions/
  outcomes; report in SQLite, not a second disk artifact store.
- [ ] Test actual SqliteStore startup with temporary legacy DB, repeated reopen,
  rollback, invalid provenance and duplicate active legacy rows.
- [x] Test same-key replay, changed-payload conflict, occupied submissions,
  busy user slot alongside belief slot, old intention list/drop compatibility.
- [x] Test publication rollback and duplicate finish: no unsaved report notice
  and no delivered state without its unique persisted notice.
- [ ] HTTP-level export/purge includes checkpoint, evidence, partial/final report.
- [x] All six handoff checks pass; execution record updated.

Done when storage/API tests pass using production startup. Intermediate API must
not claim work actually started; don't deploy this incomplete release.

## Packet 2 — Shared resumable executor and worker (Sol)

Prerequisite: packet 1. Outcome: deterministic assignments survive yield/restart/
cancel through the existing policy path.

- [ ] Characterize current run_tool_turn before extraction: read-then-act, empty
  retry, repeat guard, blocked/failed calls, escalation, taint and disclosure.
- [ ] Extract shared TurnSession/advance_tool_session; existing callers retain
  behavior; research yields without forced final answers.
- [ ] Persist proposals/pending calls/results/repeat cache and BEFORE-I/O attempt
  reservations, including otherwise hidden empty retries/escalation.
- [x] Narrow research runner; deferred discovery can't expand it. Buffer research
  audit/events until conditional commit; don't file research beliefs.
- [ ] Single immediate-wake/fallback worker, startup recovery, expiry/backoff,
  synthesis reserve and all section 6 size/call/transport budgets.
- [x] Shared foreground lock and heartbeat deferral without consuming due
  schedules; belief-origin work remains separate from commissioned work.
- [ ] Test restart after proposal, after reserved read before result, after
  checkpoint and after delivery. No budget refund or duplicate committed result.
- [x] Test cancellation rejects late state/report publication and purge removes
  assignment data without allowing resurrection.
- [ ] Test foreground wins next quantum and no overlapping replacement worker
  while an old blocking I/O call is still running.
- [ ] Test exact 16/16 caps, two-call synthesis reserve, three-attempt backoff,
  expiry, checkpoint/context limits and corrupt/unknown checkpoint version.
- [x] Research cannot reach Deeper or authorize an actuator; policy revocation
  blocks the next dispatch.
- [x] All six handoff checks pass; execution record updated.

Done when production-composed mechanics pass and foreground policy regressions
retain their outcomes. Fake-model success is not a live-quality claim.

## Packet 3 — Real evidence and a saved report (Terra; Sol if needed)

Prerequisite: packet 2. Outcome: approved search or starting URLs lead to a
persisted report with inspectable references.

- [ ] Confirm curated Brave connected schema/output without exposing credentials;
  sanitize representative fixture and implement exact-ID adapter validation.
- [x] If search unavailable, support source-only work; mark live search
  verification blocked, never claim catalogue availability proves it works.
- [x] Extend web_fetch windows additively; preserve old defaults and egress.
- [ ] Test useful text beyond 2000 chars, clipped/empty pages, failed reads,
  invalid schemes/redirects and retrieved instruction injection.
- [x] Successful guarded fetch URLs receive source IDs; search
  snippets aren't eligible report citations; arbitrary model URLs rejected.
- [ ] Gathering -> synthesis, structured report validation, single repair attempt,
  application-rendered references and honest partial terminal results.
- [ ] Test fabricated IDs/URLs, absent evidence, malformed structure, degraded
  response, truncated model output, source-only limitations and last-call cap.
- [x] Test saved report/notice through full composition; download never regenerates.
- [x] All six handoff checks pass; execution record updated.

Done when realistic scripted search/fetch/model data produces a report a fresh
process reads, and insufficient evidence produces partial/blocked, not delivered.

## Packet 4 — Console commissioning and delivery (Terra)

Prerequisite: packet 3. Outcome: the whole slice is usable without manual API calls.

- [x] Chat/Research composer mode and disclosed report scope; optional starting
  URLs, exact approved search binding and shared readiness/setup predicate.
- [x] Request key retained across retries; no duplicate chat submission; draft
  retained on failure; occupied slot doesn't silently replace anything.
- [x] Current/recent research cards with reason/phase, Cancel, View/download and
  partial/terminal labels; completed report discoverable from durable chat.
- [x] Existing live refresh plus visible-active polling fallback; authenticated
  fetch/Blob download and safe model/source rendering.
- [x] Console fixtures: queued/running/retry_wait/delivered/blocked/exhausted/
  cancelled/failed and partial-result rendering.
- [x] Exercise every control, routed endpoint and request signature; hostile
  report/title/URL text stays safe; existing section/field budgets hold.
- [x] Reload after missed SSE still shows saved report; no duplicate completion
  messages, routine progress chatter, task dashboard or count badge.
- [x] All six handoff checks pass; execution record updated.

Done when commissioning -> download works from the existing console, including
setup failure, network retry, cancellation and reload. No new client framework.

## Packet 5 — Live acceptance and focused review (Terra)

Prerequisite: packets 1–4. Outcome: reviewable evidence the configured deployment
produces useful reports, or a precise remaining limitation.

- [x] Add ignored research_eval.rs using actual persistence/runner; sanitize fixtures.
- [ ] Run comparison, primary-source and source-only assignments against an
  isolated test node with the configured model/provider. Record calls/time/status.
- [ ] Inspect claims against sources, requested coverage, usefulness of next
  steps and uncertainty. Record actual verdicts; don't score links alone.
- [ ] Restart during work; committed findings survive and terminal notice occurs
  once. Cancel another run; verify no further operations/publication.
- [ ] Exercise missing provider/timeout/revoked capability live where practical;
  identify which cases are only composition-tested.
- [x] Reconcile affected ADR 0052/0053/0056 clauses, architecture and roadmap
  with accepted and implemented scope, including actual model limitations.
- [x] Run all six offline checks; disclose that configured live evals were skipped
  because no live endpoint was supplied. No production deployment occurred.
- [x] Prepare focused review: transaction/lifecycle diff, migration, policy parity,
  taint recovery, foreground latency and actual report. Astra review optional;
  no repeated repository-wide analysis.

Done when useful reports and verification evidence are reviewable. Failed live
quality means the slice is not yet accepted, even if all offline tests pass.

## Required handoff commands

Run from repository root. Use targeted tests first while implementing and record
actual red -> green evidence. Do not invent passing results or commit IDs.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
node scripts/check-console.mjs
node scripts/check-nothing-personal.mjs
git diff --check
```

| Packet | Status | Files / commit | Red -> green evidence | Six checks | Deviations / next step |
| --- | --- | --- | --- | --- | --- |
| 1 | Complete for smallest slice | understanding + SQLite + API | migration/idempotency/cancel/publication tests | Passed | Rich checkpoint/evidence rows deferred |
| 2 | Complete for smallest slice | application runner + node worker | before-I/O counters and late-result test | Passed | Whole read-only turn reruns after restart |
| 3 | Complete offline | guarded fetch + report validation | source/report/download tests | Passed | Live Brave/model quality not run |
| 4 | Complete | console + console checker | eight states, safe rendering, routed controls | Passed | Browser layout remains human review |
| 5 | Complete offline | ADR/docs + ignored live eval | compile-only live harness | Passed | Run eval when a model is configured |

## Prompt for the implementing model

> Read AGENTS.md, ADR 0077 and docs/plans/research-assistant-handoff.md. Check the
> acceptance gate; never mark the ADR accepted yourself. Implement only the next
> incomplete packet in docs/plans/research-assistant-checklist.md using TDD.
> Preserve the specified execution/policy path and resolve routine details
> independently. Verify against current code and preserve unrelated work and
> existing API behavior. Run required checks, then record files, actual outcomes,
> deviations and the next packet in the checklist. Escalate only listed concrete
> design conflicts with a reproduction. No PR or deployment.
