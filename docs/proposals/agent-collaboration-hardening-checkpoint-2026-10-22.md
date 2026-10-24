# Agent collaboration: offline hardening checkpoint

2026-10-22. AI-assisted Phase 4 work on `codex/agent-collaboration`, after the
[Phase 3 checkpoint](agent-collaboration-assistance-checkpoint-2026-10-22.md).
This records local deterministic tests; it is not a live pilot or deployment.

## Failure matrix

| Case | Check against the actual project |
|---|---|
| Restart before delivery | Reopen the real SQLite store and create a fresh engine/runner; one accepted intent creates one expert association. |
| Drain and restart | A drain holds accepted intent without starting an expert; a new engine lifts the old drain and delivers once. |
| Expert worker disappears | Retry uses the same request/run association and a newly issued token; the old worker token cannot answer. |
| Answer committed before worker loss | Fresh-engine recovery and a persisted queued retry both retain the answer and invoke no second expert; one result notification survives. |
| Worker/model host unavailable | Three failed starts terminate the request with the stored cause; recovery of the host does not reopen it. |
| Repeated platform interruptions | The new delivery bound is three total attempts, including interruptions; it cannot inherit a fourth attempt from legacy recovery allowances. |
| Paused or furloughed expert | No expert invocation or polling wake; the request expires at its original deadline and does not resume when the member returns. |
| Exhausted free-turn limit | Sixty real simulated engine turns consume the daily allowance; assistance waits without a sixty-first start or task escalation, then expires. |
| Project budget exhausted and late use | Synthetic metering pauses the trusted project, denies a new request and accounts for late usage; raising the cap does not reopen completed work. |
| Source tier changes | A previously readable granted original becomes unavailable after its source observes Confidential input; the grant cannot downgrade it. |
| Forged authority and injected instructions | Caller-supplied sender/expert/payer/approval fields, arbitrary URLs and ungranted originals are denied. An objective claiming owner approval cannot admit task creation, deployment, messaging or nested assistance. |
| Prior schema and repeated upgrade | Remove only Phase 3's empty schema additions from a populated Phase 2-shaped store, then upgrade/open twice; retained messages and identities survive. |
| Crash during forward disable | Persist the off revision before cancellation, reopen the engine and tick; accepted pending delivery is cancelled before any expert starts, while owner history stays readable. |

Two recovery defects were observed failing before their fixes: restarting an
expert after its logical answer, and allowing a fourth physical assistance
attempt through the legacy platform-interruption allowance. Fixes are scoped
to the new assistance/result associations. Existing consultation and task retry
rules retain their behavior.

Work now exposes the persisted worker cause, run status and next retry time.
Queued waits also explain current drain, member and project conditions. Failure
text is escaped, long diagnostic text wraps, and automatic retry remains bounded;
terminal requests require a fresh authorized request rather than reopening.
Only an active request whose worker is actually waiting for retry displays a
next-attempt time; stale timestamps on terminal runs are not advertised as work.
The free-turn wait produces no stuck, redundant-wake or coordination finding,
without changing the guards' configured modes.

The final full repository gate passed: formatting, Clippy across all targets and
features with warnings denied, all workspace tests, console checks, privacy
checks including the owner's ignored local value list, collection parity and
whitespace checks. All eleven new engine failure tests passed. The complete team
suite passed 903 unit tests and 157 executable owner-language scenarios (1,983
steps), including accepted-answer loss and unavailable-worker scenarios. Standard
ignored external integration tests were left unchanged.

The updated real renderer and stylesheet were opened in local synthetic desktop
and 390-pixel previews. Persisted failure causes, retry times and long unbroken
diagnostic text remained readable without horizontal clipping. A terminal result
displayed no stale next attempt. This remains synthetic preview evidence, not
production-served or physical-device validation.

## Rollback and pilot boundary

The schema test is a controlled representative upgrade, not a production backup
or restore rehearsal. No old binary was run against the upgraded store; backward
binary compatibility is unverified. The supported rollback action for this
candidate is forward disable with the current binary, which is exercised above.
Do not substitute an old binary speculatively.

The next live proposal is deliberately narrow: an isolated HQ store with two
temporary synthetic participants, one synthetic diagnostic report, the existing
house model pinned to its exact runtime version, no external capabilities or
production credentials, a $0 paid-call budget, and one request with the existing
fifteen-minute/six-read/three-attempt bounds. The requester must inspect the
original and the fixture's independent verifier must confirm the finding.
Any authorization leak, duplicate completed work, unscoped spend or limit breach
stops the pilot. This does not authorize the pilot or create its environment.

Real pilot activation remains a separately accepted risk/budget decision under
[ADR 0137](../adr/0137-addressed-conversations-and-bounded-assistance.md). Production
deployment additionally retains the reviewed plan's exact-candidate, backup/
restore, drain and `make deploy-check` gates. Phase 5's served-device evidence and
Phase 6's controlled adoption comparison remain outstanding; passing offline
tests is not an adoption decision or a demonstrated savings claim.
