# Agent collaboration: foundation integration checkpoint

2026-10-22. AI-assisted work on `codex/agent-collaboration`, following
[ADR 0137](../adr/0137-addressed-conversations-and-bounded-assistance.md) and the
[reviewed plan](agent-collaboration-plan-reviewed-2026-10-21.md).

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

## Remaining foundation work, in order

1. Consolidate task-thread `consult` and `answer_consult` onto the common durable
   lifecycle. Routine indexed queries and answer-post/status atomicity are now
   covered. Durable request-to-dispatch and result-to-notification associations,
   recovery of post-commit effects, and common origin-aware accounting remain.
   Keep thread posting, labelled fallback, CEO restrictions, optional task
   project, quotas and feature-off behavior under compatibility tests. Do not
   silently relabel legacy records or broaden authority.
2. Verify the revised path with the real worker and local model in the stopped,
   isolated pilot. Re-pin the candidate, renew the source grant and repeat
   preflight before activation. The prepared pilot has performed no inference.
3. Complete the matched baseline/candidate evaluation and relevant device
   checks, then review the release and enablement evidence. Deploy through the
   existing NAS release workflow only with the deployment decision recorded.

The foundation and full plan remain incomplete. The NAS remains on the prior
deployed candidate with conversations disabled; this checkpoint is branch work.
