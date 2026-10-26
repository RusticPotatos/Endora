# 0138 - One conversation and assistance lifecycle

## Status

Accepted target and definition of done, 2026-10-23. AI-assisted planning.
The owner explicitly selected full replacement of the old conversation framework,
retaining readable UI history, and allowed dropping legacy behavior that does not
support the new direction. This supersedes ADR 0137's requirement to preserve both
execution contracts as the final state. It does not claim replacement has shipped
or authorize destructive history deletion, paid experiments or deployment.

## Context

[ADR 0137](0137-addressed-conversations-and-bounded-assistance.md) introduced
authorized conversations and bounded assistance while preserving task-thread
consultation. Both origins now share quotas, transactional queue writes and
recovery infrastructure. They still have separate request records, application
completion paths and protocol answers. Sharing utilities is not full replacement.

The current seams are `application/engine/consult.rs`, `history_ports.rs` and
`infrastructure/sqlite/history.rs` for task consultation, alongside
`application/engine/assistance.rs` and the conversation store for explicit
assistance. `protocol_history.rs` routes the familiar `consult` tool according to
context. Work renders task threads and conversations through `47-hq.js` and
`47e-hq-conversations.js`. Historical storage and UI presentation must not require
keeping the old execution machinery alive.

The [source review](../proposals/agent-collaboration-source-review-2026-10-21.md)
supports bounded specialist assistance with requester responsibility, not a new
framework or mandatory management relay. The [pilot evidence](../proposals/agent-collaboration-execution-pilot-result-2026-10-23.md)
shows real expert/result execution, but also failures to initiate during optional
downtime and incomplete final findings. Preserving every former interaction or
making agents talk spontaneously is not the owner's desired outcome.

## Decision

Use the conversation/assistance model as the canonical peer communication path
in the team context. Information messages schedule nothing. Explicit assistance
has one admission, dispatch, answer, verification, cancellation and recovery
lifecycle. Task context is trusted metadata and a UI link, not a second execution
engine. No second scheduler, broker, service or generic agent framework is added.

Keep deterministic authority, original-source grants, data-tier enforcement,
project charging, rate/deadline/retry bounds and independent verification. Keep
the task system for actual commitments, artifacts, ownership and acceptance.
Keep owner escalation for actual decisions. Asking a colleague must not require
a fabricated task, a task-thread detour or a management relay just to communicate.

New assistance requires an explicitly authorized conversation and trusted project
scope. A task link or migrated transcript cannot create participant permissions,
evidence grants or a payer. Unscoped or ambiguous legacy records remain readable;
new execution fails closed until the required scope exists. Do not guess missing
authority from prose, names or historical participation.

### Keep, replace and retire

| Surface | Final treatment |
|---|---|
| Messages and bounded assistance | Canonical peer communication and execution lifecycle. |
| Task-thread consultation execution | Replace with the canonical lifecycle; delete the old admission, claim, answer/fallback, delivery and recovery branches after cutover. |
| Familiar tool/API names | May remain as thin adapters to the canonical use case, with no private queue, persistence lifecycle or legacy fallback execution. |
| Legacy task posts, consultation answers and retained run transcripts | Read-only historical sources and task/history projections, preserving original attribution and links. Their storage is not a live workflow. |
| Task ownership, tester acceptance and owner approvals | Retain their own authoritative responsibilities; conversation completion cannot satisfy them. |
| Task status posts | May remain part of task reporting and linked UI history; they do not dispatch a second peer-help workflow. Audit `post` and `ask_owner` callers as well as `consult` before cutover. |
| Record search | Retain as explicitly labelled historical testimony. Remove automatic substitution of a record answer for a live peer response in the new lifecycle. |
| Management escalation | Retain for authority decisions; drop compulsory routing through management for ordinary scoped peer help. |
| Optional chatter and downtime initiation | Not a completion criterion. Do not retain behavior solely to demonstrate spontaneous agent conversation. |

### History visible to people watching

Use the existing HQ Work home. People can inspect who asked whom, the objective,
answer, source references, verification finding and terminal outcome, following
task/run links where present. Show waiting, failed, cancelled and unavailable
outcomes as well as successes. Preserve existing readable task-thread history and
retained run transcripts; do not manufacture missing messages, causes or receipts.

Retain owner-facing messages, questions, answers and final findings until explicit
owner deletion, including retained legacy conversation content made available by
the migration. Preserve provenance; do not restore already deleted material or
invent missing history. Raw worker transcripts and underlying evidence retain
their separate retention/access contracts; an expired source stays unavailable
even when a conversation still cites it.
Use bounded indexed pages and bounded model context. Growing archives may grow
storage but must not introduce whole-history reads into routine scheduling or UI
refresh. Historical body text stays out of the general activity feed.

### Retained requester finding

The associated requester-result turn records its final finding through
`record_assistance_finding`, within the same assistance lifecycle. Retain one
immutable finding, its authenticated run, original-source locators and time with
the request until owner deletion; do not rely on the expiring raw transcript.
Only the live associated requester-result turn may write it. Identical retries
reuse the record; conflicting replacements refuse. Current conversation/source
permissions and input tiers still apply. The finding is an attributed claim,
including an inconclusive finding when originals are unavailable; recording it
does not prove truth, complete a task, authorize an action or grant evidence.
Expose it in the existing bounded Work request page and its export. Legacy
requests without a retained finding remain explicitly unavailable; never invent
one from an answer, a run status or a transcript fragment.

### Compatibility and migration

Preserve public contracts within their major version. Inventory callers and
version the cutover before retiring any incompatible API/tool behavior. A thin
adapter is acceptable only if it can honor that published contract using the new
lifecycle. Otherwise deprecate and remove/change it at an explicit version
boundary; do not keep a second permanent engine to avoid that decision.

Inventory old queued/running requests and delivery associations. Drain or
explicitly cancel them before final cutover, preserving terminal outcomes. Map
historical records idempotently with stable source IDs and provenance; use
read-only views/adapters where copying would invent identity or authority.
Backups, migration rehearsal, count/link checks and restart tests precede any
live migration. Do not drop populated history tables merely because their old
execution code is retired. Rollback restores the prior release and its compatible
database backup; it must not run both engines against the same work.

## Definition of done

1. All supported new peer-help entry points use the canonical application and
   persistence lifecycle, for both task-linked and taskless work. No legacy
   execution, automatic record-answer fallback or alternate scheduler remains.
2. Old execution code, lifecycle storage write paths, tool advertising, configuration and
   tests that require retired behavior are removed or replaced. Any surviving
   compatibility adapter only calls the canonical use case. A repository/caller
   audit proves this, not merely a passing direct-path pilot.
3. Retained history is still readable in the actual UI with correct attribution,
   ordering and task/run/source links. Fresh and migrated histories, empty pages,
   terminal failures, pagination, export and explicit deletion are verified.
4. Real work can trigger bounded peer help and return an independently checked,
   complete finding without an operator submitting the consultation for the
   agent. Deterministic work triggers are acceptable; spontaneous downtime
   chatter is not required. Observe actual model/tool traces, not scripted claims.
5. Restart, retries, interruption, cancellation, revocation and unavailable peers
   produce bounded visible outcomes without duplicate answers or lost accepted
   work. Authority, privacy, source access and trusted accounting still hold.
6. Archive-growth checks prove bounded routine reads, and comparable workload
   checks report quality, starts, latency and all attempts honestly. No performance
   win or precise percentage is required or claimed without evidence.
7. Required repository checks, meaningful migration/cutover tests, actual UI and
   relevant device checks pass against the final candidate. Earlier dual-path
   tests alone do not establish completion of the replacement.
8. The final candidate is deployed through the existing NAS/Ansible release
   workflow with backup, drain, versioned cutover, live checks and a recorded
   enablement/rollback decision. No obsolete engine remains active in production.

## Amendment (2026-10-25): what the first live requests taught

The first three production requests were not answered: #1 expired (a tester's answer turn
pinned to the Mac, fixed by answering on any free house model), and #2 and #3, Cy asking Dee on
#677, were declined. Two causes, both fixed:

- **The answer tool was hidden.** A local turn with more than twelve tools starts with its core
  and a tool search; an expert's turn has the node's read tools and the shell beside it, so
  `answer_assistance` itself was behind the search and the model ended in plain text. The tool a
  special turn answers with (`answer_assistance`, `read_assistance_source`,
  `record_assistance_finding`, `answer_quiz`) is now always loaded.
- **Sources named, not passed.** Cy's question named `r877` but passed no evidence, and prose
  grants nothing, so Dee had nothing to read. A consult whose question names a receipt or
  document while passing none is refused, in the asker's turn, with the exact `evidence` to
  pass. Prose still grants nothing.

## Consequences

This expands the remaining work beyond the earlier four validation gates: actual
retirement and history/caller migration are now required. Earlier fixes and live
results remain useful evidence, but do not satisfy the new final-state criteria.
The [checkpoint](../proposals/agent-collaboration-foundation-checkpoint-2026-10-22.md)
owns the ordered implementation steps. Prefer the smaller observable workflow
over preserving clever but unnecessary legacy behavior. This decision changes
the target, not the claim about what is currently implemented or deployed.
