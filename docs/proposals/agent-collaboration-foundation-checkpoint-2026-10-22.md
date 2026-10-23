# Agent collaboration: foundation integration checkpoint

2026-10-22. AI-assisted work on `codex/agent-collaboration`, following
[ADR 0137](../adr/0137-addressed-conversations-and-bounded-assistance.md) and the
[reviewed plan](agent-collaboration-plan-reviewed-2026-10-21.md).

Current branch scope: ordinary consultation is integrated with authorized
taskless assistance; task-thread consultation now has durable delivery,
idempotent admission, shared allowances, trusted expert project attribution and
atomic answer effects. These close the implementation gaps listed in the
earlier checkpoints below. The first authorized live diagnostic then failed:
its initial prompt exhausted the frozen context before any consultation was
accepted. The [result](agent-collaboration-pilot-result-2026-10-22.md) records one
local call, zero paid charges and the subsequent worker token-limit correction.
The later shared-runtime execution pilot passed its bounded flow criteria with
an answer-quality caveat; see the current result below. Operational completion
still requires quality/initiation evaluation, matched comparison and
release/device gates. The latest changes are not deployed.

The authorized 16,384-token follow-up avoided truncation but was blocked by the
private experiment guard's exact-question and discovery restrictions before
application admission. The [follow-up record](agent-collaboration-pilot-followup-2026-10-22.md)
distinguishes this harness defect from application behavior: an offline replay
through the actual protocol accepted the captured question and reused its
request on retry, without a carrier task or model start. The owner then
authorized the corrected-guard rerun. It ended after two local model calls with
no consultation proposed: the requester treated the owner notes as already
served and followed the downtime briefing's option to stop. There was no
truncation or guard refusal. All services stopped and conversations were disabled;
no production change occurred. The three-attempt series stops for offline review
of the initiating wake, owner objective and briefing before another live attempt.
A live assistance result is still unverified.

The next offline slice reproduced a concrete briefing inconsistency: the final
downtime tool summary omitted conversation tools already advertised to the same
authorized turn. It now lists them only for an enabled participant with a scope,
and downtime says no task is due rather than nothing is due. The regression also
checks disabled/absent scopes and continued refusal of task creation. This is a
briefing correction, not proof that a model will initiate the diagnostic. The
pilot still supplied its objective through retained notebook notes during an
optional downtime turn; it did not create a current owner assignment. No new
wake or scheduling contract was added to make the experiment pass.

The next [execution-only check](agent-collaboration-execution-pilot-result-2026-10-23.md)
used operator admission to separate model execution from model initiation. It
stopped before a worker or inference call: the expert was incorrectly marked
shell-eligible. A regression confirmed the application policy gap. Explicit
experts and their verification-result turns now have no shell or node tools;
verification also has read-only handler enforcement and a six-read-round limit.
The cancelled attempt is preserved. No guard relaxation or replacement request
was used. Live execution and autonomous initiation are still unproven.

The fixed-policy follow-up then passed an actual HTTP assignment/source-reading
rehearsal with no model or worker. Its live expert started with the correct
restrictions, but the first model call failed with a GPU insufficient-memory
error before a response. The failed call and cancelled request are retained;
production and other model processes were untouched. Runtime GPU computation
readiness is now the immediate gate before another request, ahead of validating
the expert/result model exchange. Host memory readings alone did not predict
the allocation failure. The full application gate for `64a4253a` remains passed.

Runtime diagnosis reproduced the limit after a successful four-token control;
the normal expert call still failed. Smaller physical batches did not establish
an improvement. Further duplicate-model attempts have stopped. The execution
record proposed reusing the already resident, identically pinned model with
isolated application/data and an explicit shared-runtime boundary. The owner
accepted shared model capacity and weaker model-process isolation. No resident
service was changed or stopped.

The [shared-runtime result](agent-collaboration-execution-pilot-result-2026-10-23.md#shared-runtime-execution-result)
now records a bounded-execution pass on `07eedb8d`: two real workers each read
the original, one answer and result were retained, project reservations matched,
and cleanup completed. Eight application calls plus one runtime control took
177.26 seconds with zero paid calls or external actions. The worker now sends
an explicit 4,096-token reply limit on both provider paths; the full gate passed.
The requester nevertheless made an unsupported inference excluding code defects
and tried advertised read tools outside the private fixture scope. Answer quality
and autonomous initiation remain unproven. This is not a production deployment.

## This slice's acceptance criteria

1. A normal taskless member can use the familiar `consult` tool to obtain one
   attributed answer through existing authorized assistance, without a carrier
   task, duplicate request on retry, new scheduler or new service.
2. Routing never invents a participant/project grant. Multiple matching scopes
   require selection; original sources remain granted and revocable; a turn
   with Confidential inputs cannot send an Internal question. Existing task
   thread consultation keeps its behavior.
3. Pair discovery does bounded database work as unrelated conversations grow;
   normal eligible briefings explain how to use and discover the path without
   loading history into model context.

## Implementation

The existing `consult` protocol now adapts a taskless call without a task
argument to `request_assistance_in`. A single authorized pair scope routes
automatically. The optional `conversation`, `key` and `evidence` fields select
an existing scope and granted originals. Existing task-owned calls retain their
thread contract and refuse those taskless-only options.

Acceptance, expert scheduling, read-only tools, answer/result notification,
project charging, cancellation and restart recovery use the same assistance
records and use cases as explicit requests. The compatibility reply contains
only request/scope/status metadata and fixed guidance. Legacy task-thread
replies retain their conservative Confidential lineage handling.

Normal member briefings with an enabled conversation explain `consult`,
`read_messages` and explicit source selection. They append fixed guidance,
never conversation bodies. Quiz and restricted assistance briefings do not
receive that extra guidance. The owner still controls conversation grants;
agents cannot create permissions by naming another member.

A partial composite index on `(member1, member2, id)` supports the two possible
pair orientations. Each query arm and the combined result stop at two scopes,
enough to distinguish no match, unique and ambiguous. The regression test runs
the actual lookup against 1,000 and 100,000 unrelated conversations, requires
equal bounded SQLite VM work, checks reversed participants and removes the
index as a negative control. This proves this lookup's bound, not universal
latency as the entire database grows.

## Verification

The new behavior tests were observed failing before the adapter was added.
Coverage includes the ordinary protocol call through answer delivery, retry,
scope ambiguity, absent/foreign scopes, disabled policy, original receipt
admission and revocation, Confidential inputs and unchanged task-thread answers.
An executable owner scenario uses ordinary `consult` through one expert answer.
The complete repository gate passed: formatting, workspace Clippy with denied
lint warnings, workspace all-feature tests, console checks, personal-value
checks, collection parity and whitespace checks. The team suite passed 908
tests; its behavior harness passed 158 scenarios and 1,991 steps. Existing
explicitly ignored tests remained ignored. Live smoke, model inference and
device checks were not run for this branch change, which is not deployed.
No terminal-client source changed.

## Task-thread hardening follow-up

The next branch slice addresses two concrete legacy gaps before lifecycle
consolidation. Its acceptance criteria are bounded routine consultation reads,
one durable expert claim, and agreement between an answer's thread post and its
terminal status even when storage fails.

- Routine consultation admission now reads counts saturated at the existing
  two-per-turn and six-per-day limits. The application supplies the owner's day
  boundaries. Assigned-run lookup, expert claim and unread answers use dedicated
  indexed queries instead of loading every retained consultation. An unread
  batch is at most fifty answers; remaining answers stay pending for later
  requester turn completion. The outcomes snapshot's legacy archive aggregation
  remains outside this routine-query change.
- Expert claims now commit the oldest queued question's run association in one
  transaction. A retry reuses that association. Two SQLite connections racing
  for the same question cannot both claim it.
- Expert answers and labelled record fallback use the same conditional
  completion operation: terminal status and task-thread post commit together.
  Stale/repeated completions write nothing. Existing post-processing, feed
  events and requester wake policy still run after that transaction. Their
  crash recovery is not established by this change.

The original engine test reproduced a post left behind after a forced status
write failure. It now passes through failure, retry and reopening the actual
database. The reverse failure test refuses a post and proves the status rolls
back, for both expert and record-fallback completion. A two-connection race
proves one claim and one completion survive reopening. The actual routine SQL
is exercised with 1,000 and 100,000 retained consultations, with equal bounded
VM work and a missing-index negative control. A new owner scenario forces the
storage failure, recovers, retries and restarts HQ with exactly one answer.

Focused checks passed: 23 consultation-related tests and the new 17-step
storage-recovery scenario. The complete repository gate then passed:
formatting, workspace Clippy with denied lint warnings, workspace all-feature
tests, console checks, personal-value checks, collection parity and whitespace
checks. The team suite passed 913 tests; its behavior harness passed 159
scenarios and 2,008 steps. Existing explicitly ignored tests remained ignored.
There was no deployment, live model call or new device validation in this slice.

## Dispatch and notification recovery follow-up

Acceptance criteria: interruption must not leave a wake without its associated
run; an accepted answer must still reach its requester after restart; a replay
must not duplicate a delivery or restart an expert that already answered.

Expert and answer delivery now persist their wake/run association and wake audit
event atomically. New task-thread answer completions also save an exact post
association in the same transaction as the answer and terminal status. Legacy
consultation and conversation assistance share the transactional queue writer,
while existing deterministic application policy still supplies authorization,
fold/hold/refusal decisions and the different fallback contracts.

The tick reads batches of at most fifty pending dispatches, ended expert turns
still needing record fallback, and pending answer notices. These are indexed
queries, included in the 1,000/100,000 retained-consultation regression. It does
not retry a folded notice while its associated run is live. Once that turn ends,
delivery is decided again through the existing policy. Held and refused wakes
keep their previous meanings. The conversation switch does not disable legacy
task consultation; the global stop and drain still control scheduling.

The forced dispatch and notification failures first reproduced orphan wakes.
Recovery tests now reopen the real database, deliver the original request/result
once, and advance through platform retry without restarting an expert whose
answer is already accepted. A two-connection delivery race keeps one result run.
Separate checks cover replay, folded-notice deferral and adoption of an existing
unclaimed legacy expert turn without creating another wake. Three executable
owner scenarios cover status-write, run-write and notification-write failures,
storage recovery and HQ restart.

The restart test also reproduced a timestamp tie that hid the saved answer from
the next briefing. Result turns now retrieve their exact associated post; newer
posts continue through normal thread projection. This fix does not broaden task
or evidence access, fabricate a receipt, or treat the answer as task acceptance.

Existing stored consultations are preserved. Recovery adopts only an unstarted,
correctly addressed legacy expert run not already bound to either origin,
following the original oldest-question rule. New post associations apply to new
completions. Historical answers with no causal post association remain on their
existing thread-history path; matching text is not used to guess a binding.
Replay guarantees apply to the same persisted consultation ID. Deduplicating a
repeated task-owned API request after a lost acceptance response remains open.

The complete repository gate passed: formatting, workspace Clippy with denied
lint warnings, workspace all-feature tests, console checks, personal-value
checks, collection parity and whitespace checks. The team suite passed 917
tests; its behavior harness passed 161 scenarios and 2,041 steps. Existing
explicitly ignored tests remained ignored. No deployment, inference or new
device validation was performed for this branch slice.

## Remaining foundation work, in order

1. Review the completed offline foundation against the frozen compatibility
   cases. The ordinary taskless entry point,
   task-thread lifecycle, shared admission/queue mechanisms and answer effects
   are implemented, and the operator-started live expert/result flow has passed.
   Tighten observed-fact versus inference guidance and align the private fixture's
   advertised/enforced read scope before freezing the next quality evaluation.
2. Review the initiating wake and briefing offline after the three unsuccessful
   diagnostics. The corrected-guard run performed inference but proposed no
   consultation. The missing tool summary and overbroad wording now have a
   regression fix. Decide the next experiment's initiation contract explicitly:
   optional spontaneous consultation and an assigned diagnostic are different
   tests. The successful operator-started execution does not settle this. Do not
   count an operator-initiated request as model initiation, or add
   a taskless assignment API solely to pass a pilot. Renew candidate fingerprints
   and source grants before any further authorized isolated live attempt.
3. Complete the matched baseline/candidate evaluation and relevant device
   checks, then review the release and enablement evidence. Deploy through the
   existing NAS release workflow only with the deployment decision recorded.

The foundation and full plan remain incomplete. The NAS remains on the prior
deployed candidate with conversations disabled; this checkpoint is branch work.

## Admission, accounting and completion effects follow-up

Task-thread admission now checks exact replay before consuming an allowance,
then atomically counts both origins, accepts the question and snapshots its
thread's optional project. The canonical payload binds originating run, expert,
thread, topic and question. Same-turn retries retain one request and do not add
wakes or repeat an accepted answer. Different questions remain new admissions;
the third-distinct-question compatibility test still checks the existing limit.
Existing rank, task reach and live-token checks precede replay.

The shared allowance is two new requests per turn and six per requester/day,
using the configured owner-clock day. Conversation usage counters survive
cancellation/deletion, and legacy retained rows remain counted. A real-use-case
test combines two cancelled assistance requests with four task consultations in
subsequent turns and refuses the seventh. Its negative control omits legacy
daily usage and correctly detects the seventh being admitted. A two-connection
race between origins proves they cannot both spend the final turn allowance.
The Internal/Confidential guard remains unchanged; tests do not clear lineage to
force a turn across the two entry points.

The expert project comes from the requesting thread at acceptance, never the
question topic or a model-supplied payer. Tests move the thread to another
project after admission and verify reservation, use, charge, release and late
charge rows retain the admitted project. New receipt grants can recognize that
trusted legacy-expert project, with the existing original-source and tier checks.
Unscoped tasks and older requests with no snapshot retain their previous
behavior. Requester result turns retain ordinary task-thread budgeting.

Answer completion now commits the task post, terminal status, exact post link,
thread feed event and selected knowledge-use markers together. The application
resolves the answering member and cited knowledge IDs. Updates are idempotent
and use monotonic timestamps. Feed/status failures and a knowledge-update fault
roll back the answer; recovery and retry retain one answer and one feed event.
The feed-failure negative control restores the former event-after-commit order
and correctly detects a terminal answer left behind. The first trigger used an
incorrect event name; it was corrected to the actual `thread.posted` event before
the meaningful negative control and final verification.

The executable owner scenario retries the same question before and after its
answer and observes one request and one answer. The complete repository gate
passed: formatting, workspace Clippy with denied warnings, workspace all-feature
tests, console checks, personal-value checks, collection parity and whitespace
checks. The team suite passed 923 tests; its behavior harness passed 162
scenarios and 2,055 steps. Existing explicitly ignored tests remained ignored.
The first full gate caught a post index in the history-only schema fixture;
moving it to the schema that owns posts corrected the failure before this pass.
No model inference, deployment or new device validation is included.
