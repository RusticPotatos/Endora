# Agent collaboration: bounded assistance implementation checkpoint

2026-10-22. AI-assisted work on `codex/agent-collaboration`, in the isolated
worktree branched from `develop`. This follows the validated information-only
slice and the [Phase 3 integration map](agent-collaboration-assistance-integration-2026-10-22.md),
under [ADR 0137](../adr/0137-addressed-conversations-and-bounded-assistance.md).

## Implemented scope

- Explicit taskless requests between the two fixed authorized participants.
  Identity and charging project come from the live turn and conversation, never
  model-supplied sender, expert, payer or permissions. Task-owned investigations
  keep the existing consult/task path. No task is created to carry an exchange.
- Durable idempotent acceptance, two requests per turn and six per requester/day,
  one open request per directed pair/conversation, a 400-character objective,
  fifteen-minute deadline and at most five typed original sources. Cancellation
  and owner deletion do not reset admission accounting.
- A single SQLite transaction queues the expert wake/run and associates it with
  the request. Existing placement, pool/member/project limits, six expert read
  rounds, run retry/backoff and drain behavior remain the execution path. There
  is no second worker, scheduler, lease or paid escalation path.
- Handler-enforced read-only expert tools, separate from tool advertisement.
  Nested assistance and mutators are refused. An answer commits once with one
  durable notification intent. Identical retries return the original result;
  conflicting answers cannot replace it.
- Result delivery waits until the requester has no live turn. One queued result
  run gets the attributed answer and original source locators. The requester can
  inspect granted originals in that assigned run; ordinary turns do not acquire
  cross-task reach. Peer testimony cannot accept, test, merge or deploy work.
- Owner-granted immutable documents and receipts are scoped to the fixed project
  and conversation, limited to five live grants, expire after 24 hours and can be
  revoked immediately. Existence, recorded revision, active grant, source input
  classification and assigned run are rechecked on access. The initial slice
  refuses Confidential or unclassified source material. No writable checkout is
  shared and the existing task verifier still needs its own authorized reading.
- Expert and result-run reservations, use, settlement and late charges retain
  the trusted project. The immutable reservation remains the payer fallback
  after owner deletion purges the conversation association.
- Owner/requester cancellation, feature disable, kill switch and deletion stop
  pending/live assistance. Re-enabling does not resume cancelled work. A committed
  answer stays retained when its notification is cancelled. Deadline sweeps and
  terminal-run reconciliation run in bounded batches, including during stop/drain.
- HQ Work exposes paged request state, answer/source attribution, attempts,
  project, recorded spend, notification status, request-page export, cancellation
  and source grant/revocation. Message and request/result history stays until owner deletion; deletion
  purges at most 1,000 new conversation rows per tick and retains existing receipts
  and accounting. Existing model transcripts, receipts and backups retain their
  existing policies; conversation deletion is not a promise to erase every copied
  observation or backup. Owner inspection remains available with the feature off.

## Verification record

Focused tests include durable intent before model start, retry conflicts,
conditional answers, deadlines, one-time notification after the requester ends,
owner/requester cancellation, kill switch, disable/re-enable, immediate deletion,
grant scope/revocation/expiry, original-document immutability and quota boundaries.
Regression tests were observed failing before the corresponding behavior fixes.

Real on-disk SQLite reopen and independent connections exercise a concurrent
delivery claim, a lost answer acknowledgement, answer/cancel competition,
notification replay and charging of expert/result runs including late use.
The payer remains discoverable after owner deletion removes the request record.

The assistance archive regression uses the actual delivery, notification and
request-page SQL against 1,000 and 100,000 retained requests in the same and other
conversations. It exposed delivery and result-notification scans caused by index
selection; the indexes now cover the actual predicates. The test also removes
the scheduler indexes to prove its cost check detects an unbounded scan. This
measures SQLite work, not a universal latency guarantee or a benchmark of every
pre-existing scheduler/history path.

Observed SQLite VM steps were unchanged at both archive sizes: delivery 36,
result-notification 33, and a request history page 1,084. Both missing-index
negative checks correctly detected work expanding beyond the bound.

The real conversation renderer and stylesheet were opened in local synthetic
desktop and 390-pixel iframe previews. Request text, answers, sources and owner
controls fit without horizontal clipping. These previews do not establish
production-served behavior or replace Phase 5's relevant-device gate.

The final repository gate passed: formatting, Clippy across all targets/features
with warnings denied, all workspace tests, console rendering/control checks,
privacy checks including the owner's ignored local value list, collection parity
and whitespace checks. The team suite passed 155 executable scenarios and 1,968
steps, including the two new assistance scenarios. Standard ignored external
integration tests remained ignored; no live model or service was needed.

The final regression pass also proved that disabling stops an expert which has
already answered but has not exited, retains its accepted answer, and prevents
new reads after a terminal result. Request retries report the actual saved state
rather than claiming a cancelled request is queued again.

## Remaining rollout gate

No live service, production database, paid model call, pilot, push, merge or
deployment was exercised by this checkpoint. Phase 4's expanded interruption,
budget/placement and hostile-input matrix remains required before the explicitly
authorized named two-member pilot. Wider collaboration, background full-text
indexing, external protocols and mutating assistance remain outside this slice.
