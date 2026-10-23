# 0137 - Addressed conversations and bounded assistance

## Status

Accepted for isolated implementation, 2026-10-21. Prepared with AI assistance.
The owner authorized starting
the [reviewed plan](../proposals/agent-collaboration-plan-reviewed-2026-10-21.md)
on a separate branch. Phase 0 has an
[operational baseline](../proposals/agent-collaboration-baseline-2026-10-21.md).
The owner's follow-up selected retaining all conversations for historical
research and required that archive growth not slow routine system operation.
This record fixes those contracts before behavioral implementation. It does not
authorize a production pilot, paid model calls, merge or deployment.

Proposed amendments: 0102/0111 (communication outside task threads), 0113
(trusted project attribution for taskless assistance), 0114/0115 (read-only
evidence access separate from ownership and acceptance), 0129 (shared consultation
application path), and 0124/0131/0133/0134 (new wake, waiting and recovery rules).
Earlier authority rules remain in force except for the explicitly scoped
communication additions below. No constitution or standards amendment is proposed.

## Context and outcomes

Endora already has stable member IDs, bounded direct consultation, queued runs,
deterministic placement, receipts, task criteria, independent testing and budget
caps. Consult 1 and consult 2 in the baseline both received answers from taskless
expert turns, but still needed a task's thread and their turns were labelled
empty. Twelve question-initiated runs accounted for 39.82% of the declared
cohort's spending. The record does not establish which of that cost was avoidable.

Current `read_document` scopes a non-lead's access to its turn's task tree; a
consult's topic does not grant its taskless expert that reach. Project attribution
in `reserve` is derived from `runs.task -> tasks.project`, so taskless assistance
also needs a trusted charging context. An in-process engine lock does not make
separate saves atomic across a crash. New messaging cannot inherit those gaps.

Three outcomes define this work:

1. Two explicitly authorized members exchange persistent information without
   creating a task, waking a model or involving management merely to send it.
2. An explicit bounded read-only assistance request returns an attributable
   answer with authorized original evidence, with project caps and independent
   task verification preserved.
3. Restart, cancellation, duplicate requests and failures leave visible bounded
   outcomes rather than lost accepted work or unbounded wakes.

## Decision

### One responsibility, two execution contracts

The team context owns conversation and assistance rules. Add narrow domain
types, application use cases/store ports and a SQLite adapter. HQ wires the
existing engine and exposes tools; the node fronts the owner's API. No service,
crate, message broker, SDK, independent worker endpoint or external A2A adapter.

- **Information message:** persistent text and references. It schedules nothing.
- **Assistance request:** explicit objective, input, expected result and bounded
  execution. Its requester retains responsibility. First version is read-only.
- **Task:** existing commitment, artifact/system change, ownership and criteria.
  Keep `create_task`, rank/area rules, tester and lead acceptance.
- **Approval:** existing deterministic authority boundary and owner desk. Neither
  message text, request completion nor a peer's claimed consent grants it.

Adapt existing `consult`/`answer_consult` through the shared assistance use case
when that slice is implemented, preserving existing thread answers and explicitly
labelled record fallback. Do not replace legacy behavior in the information-only
slice or introduce a second scheduler. Existing `post` and `ask_owner` retain
their meanings. No new notification is inferred from prose or an `@mention`.

The shared path must distinguish two trusted origins. A new conversation-origin
request requires the owner-created pilot scope and mandatory project below. A
legacy consult-origin request retains its existing task thread, optional project
inherited from that task, CEO-to-lead exception, limits and record fallback. Its
origin is selected by the authenticated entry point, never a caller-controlled
flag. Feature disable closes new conversation-origin operations without disabling
legacy consultation. Consolidation ships only when tests show both origins keep
their contracts; until then legacy consultation remains on its existing path.

Integration clarification, 2026-10-22: ordinary `consult` with no task argument
from a taskless turn now enters the conversation-origin assistance use case.
The live caller and named colleague must match an existing owner-created pair.
A unique matching conversation supplies the project; ambiguity requires an
explicit conversation ID. This adapter creates neither permissions nor tasks.
Optional evidence uses the same typed, owner-granted original-source checks.
Retries without an explicit key derive a stable label from the run and question;
the persisted full payload must still match, so the label grants no authority.
Normal eligible member briefings describe this path and paged discovery.

Briefing clarification, 2026-10-22: an eligible downtime turn's final tool
summary also names the authorized conversation tools, including ordinary
`consult`. Downtime says no task is due, rather than claiming nothing is due.
This aligns the briefing with existing permissions; it adds no assignment,
wake, capability or requirement to initiate a conversation. Notebook content is
not an execution receipt. A model choosing to stop during downtime does not prove
that an outstanding diagnostic was performed.

This is partial consolidation: task-owned calls, or taskless calls with an
explicit task argument, still use legacy thread answers and record fallback.
Their lifecycle and accounting still need the compatibility work described
above. The adapter does not complete that work or combine the
two origins' existing admission counters. Feature disable continues to leave
legacy task-thread consultation available.

Task-thread hardening, 2026-10-22: routine admission, assigned-run lookup,
expert claim and unread-answer lookup now use indexed intent-specific queries.
Claims are transactional and reuse their stored run association. Both expert
answers and labelled record fallback conditionally commit the task-thread post
and terminal consultation status together. Neither write survives a failed
transaction, and a terminal result cannot be overwritten by a stale completion.
Existing thread effects and requester wakes still follow that commit through
their existing application behavior. Moving those effects and both origins'
notification/dispatch intents into the common recoverable lifecycle remains
required; this hardening does not claim that consolidation is complete.

Delivery recovery follow-up, 2026-10-22: task-thread consultations now persist
expert and answer delivery associations with their wake, queued run and wake
audit event in one transaction. Both assistance origins use the same queue
write primitive; their authenticated application entry points still authorize
their different scopes and fallback policies. The existing tick reconciles
bounded pending dispatches, interrupted fallback and unanswered delivery intents.
A folded answer notice waits for that run to end before another decision;
replaying a committed delivery never creates another run. Legacy consultation
continues independently of the conversation switch and respects the existing
kill switch, drain, pause, project and wake rules.

New answer completions also bind the exact immutable task-thread post. A result
turn includes that answer even when its timestamp equals the previous turn's;
it does not infer identity from matching prose. Older queued expert runs may be
adopted only by the old oldest-question rule, when unstarted, correctly addressed
and unassociated with either assistance origin. Historical answer posts are not
retroactively guessed or relabelled. A terminal consultation prevents its expert
from starting again after platform interruption. Admission retry deduplication,
shared origin-aware accounting and recovery of other post-commit thread effects
remain separate compatibility work.

Admission and completion follow-up, 2026-10-22: task-thread admission is now a
transactional replay-or-accept operation. The stable identity is the authenticated
originating turn, expert, answer thread, topic and canonical question. Repeating
that payload in one turn returns the original request, including after an answer
or at the allowance limit. A changed payload is a new request. Rank, live-turn
and task-reach checks still run before replay. Historical duplicates are kept;
they are not rewritten to manufacture a unique history.

Both origins now enforce one allowance of two new requests per turn and six per
requester per owner-calendar day, in the same transaction as admission. Legacy
retained rows and durable conversation usage counters supply indexed accounting.
Cancellation, disable and conversation deletion do not reset that usage. This
supersedes the earlier rollout's separate-origin counters.

New task-thread admissions snapshot the requesting answer thread's optional
project from the database. Its taskless expert uses that trusted context for
placement and ledger reservation; later use, settlement and late charges inherit
the reservation. Moving the task later cannot change that expert's payer.
Unscoped tasks remain supported. Older requests without an admission snapshot
retain their previous unscoped expert context; no historical payer is guessed.
Requester result turns keep their existing task-thread budgeting semantics.

Task-thread completion now atomically saves the answer, status, exact post link,
thread feed event, posted-turn marker and application-selected story/section use
timestamps. A failure rolls them all back. Member identity and referenced IDs
come from the application, not post text. Cleaned peer testimony does not create
mention wakes. No approval, task acceptance or external act is granted by an
answer. The previously listed offline lifecycle implementation gaps are closed
and the full repository gate passed; live evaluation and release gates remain.

Live diagnostic follow-up, 2026-10-22: the authorized isolated run failed before
request acceptance when its frozen context was exhausted. The worker had treated
the empty response as a successful turn. A subsequent regression fix rejects
provider token-limit responses before executing their proposed calls or
finishing the turn; the existing transcript path still attempts retention.
This does not undo prior completed calls or authorize another run. The
[pilot result](../proposals/agent-collaboration-pilot-result-2026-10-22.md) records
one local inference call, zero paid charges, stopped/disabled scope and the
separate larger-context proposal. No live assistance result is established.

### Pilot scope, identity and authority

The owner enables the pilot, selects one existing project and creates a
conversation with exactly two existing non-CEO members. The owner action,
authenticated by the node, is the explicit communication grant. This is not a
claim that the current project model already has participant ACLs. Workers
cannot create conversations, add participants, change the project, enable the
pilot or grant evidence access. Membership/project are immutable in this slice;
changing scope requires a new conversation. The CEO policy is unchanged.

Sender member and run IDs come from the authenticated live turn. Display names
are resolved for presentation, never routing or historical authorship. Owner
and Endora system actors remain distinct from members. Reject a body-supplied
sender rather than allowing caller-selected attribution. Reads recheck the
current live turn, participation, project state, deletion state and execution data tier.
An expired token remains useless even if it previously sent the message.

| Operation | Principal and scope | Enforcement and audit |
|---|---|---|
| Enable/disable pilot | Owner via node; global feature remains off by default | Audited owner API; model tools cannot call it. Disable stops new requests/wakes, not owner inspection. |
| Create conversation | Owner; active project and exactly two eligible members | Team application validates records and fixed scope; node records owner action. |
| Send information | Live turn; member is a participant; active enabled conversation | Application derives identity, validates limits/tier and atomically saves message/idempotency record. No wake/task/desk side effect. |
| Read messages | Live participant, current model tier and permitted conversation | Application checks each page and returns only that conversation. Owner has an authenticated inspection path. |
| Ask assistance | Live participant; approved expert; trusted conversation project | Application checks request limits, stores request/delivery intent atomically; existing tick places the run. |
| Answer assistance | Only its claimed expert run, before terminal/deadline | Conditional application transition; one logical result; owner may inspect. |
| Inspect original evidence | Existing task reach, or an explicit conversation grant for that assistance run, plus current tier/expiry checks | The grant authorizes only its named immutable resource/revision; a link alone grants nothing. Node witnesses tool observations where required. |
| Cancel request | Original requester or owner | Terminal conditional transition; records winner of answer/cancel race. |
| Export/delete conversation | Owner | Export includes own conversation records; delete cancels pending work and removes new conversation data. Existing immutable node receipts are unaffected. |
| Claim/submit/test/accept/merge/deploy | Existing task, role, area and owner rules | Existing handlers remain authoritative. Conversation membership grants none of these operations. |

Record a messaging operation's admitted/refused outcome with actor/run, operation,
conversation/request ID if authorized, reason code and timestamp. Do not put
message bodies or unauthorized resource details in the general event feed or
refusal text. The baseline demonstrates that node tool receipts alone do not
cover all HQ protocol refusals. Keep this audit distinct from a node-issued
consequential-action receipt.

### Evidence and data tiers

Reuse Internal/Confidential/Restricted semantics from 0105/0129. Derive tier from
runtime-observed input, never a member's requested label. Propagate the highest
input tier through message reads and subsequent sends. A read must record its
provenance/tier before returning content, so a later message cannot discard it.
Unknown stored tiers fail closed. Do not let a switch to a cloud model expose
previously permitted local-only content. Preserve quoting/scrubbing and escaping
on every model/UI rendering; these do not substitute for access checks.

For the initial information-only slice, support Internal messages only. Refuse
creation from a turn marked Confidential/Restricted by runtime observations;
do not silently downgrade it. Evidence attachments are locators whose existence
is checked and whose dereference remains subject to existing policy. There is
no automatic cross-task evidence access in that slice.

For read-only assistance, start with owner-granted immutable repository/check
documents needed by B2/B3 and typed receipt references. Grants name concrete
resource IDs and revisions; at most five per conversation/request. No arbitrary
filesystem paths or shared writable checkout. Recheck existence, revision and
tier at use time; unavailable/expired is not a successful verification. Granting
read access requires the owner's explicit conversation setup in the pilot.

For the associated expert run and the one assigned requester result run, an active grant is a narrow alternative to
the current task-tree read predicate in `read_document`; requiring that predicate
as well would still deny every taskless expert. It authorizes reading only the
named document or receipt at its recorded revision. Grants last 24 hours and
the owner can revoke them immediately. The result run needs this same narrow
read so B2's requester can inspect the original rather than trust the expert's
summary. An expert's request deadline still applies to its execution; a result
run checks the current grant and accepted answer, without reopening that request.
This does not broaden `reach`, branch
checkout, submission, acceptance or any mutating operation. Ordinary conversation
reads do not acquire this assistance-specific resource grant.

Taskless assistance receipts can be inspected as source material. They do not
automatically satisfy `judge_work` scope/freshness or the tester's own-reading
requirement. The task's verifier takes its own authorized observation. Summaries,
peer answers and historical record fallback remain testimony. No global
relaxation of evidence rules is part of this decision.

### Concrete initial bounds

Existing limits are retained. New limits are conservative engineering defaults
to evaluate under the frozen cases; they are not claimed empirical optima.

| Bound | Initial value and rationale |
|---|---|
| Participants | Exactly 2; no broadcast, rooms or membership changes. |
| Information message / answer | 1,200 Unicode characters, matching existing consult answers. |
| Transport / argument bytes | New dedicated JSON endpoints: 16 KiB before parsing. New worker tool arguments: 8 KiB at the protocol boundary after the existing MCP parser; do not change the legacy MCP ingress contract. Reject over-bound input rather than truncate it. |
| Assistance question | 400 characters, matching existing record question bound. |
| References | At most 5 typed IDs; no inline files or arbitrary URLs as capability grants. |
| Idempotency key | 8-64 ASCII letters/digits, `_` or `-`, unique per sender and conversation. Same key/different canonical payload is a conflict. |
| Informational rate | 12 newly accepted sends per turn and 60 per member per owner-calendar day. Idempotent retries do not consume another slot. |
| Assistance rate | Existing 2 per turn, 6 per requester per owner-calendar day; no self-request. |
| Open assistance | At most 1 per requester/expert/conversation; no nested assistance. |
| Expert read rounds | Existing 6; then answer or report uncertainty. Mutators remain denied at handler and tool-list boundaries. |
| Request deadline | 15 minutes from durable acceptance. A valid wait still expires; do not extend its deadline on retry. |
| Delivery association | Request transition, wake, queued run and immutable association commit in one SQLite transaction. There is no separately persisted claim that can expire or orphan a run. An executing expert remains owned by its stored run ID. |
| Delivery attempts | At most 3 total attempts before visible failure, with existing bounded retry/backoff and request deadline also applying. |
| Read pages | At most 50 messages after a stable increasing message ID; authoritative pagination, no feed replay required. |
| New conversation retention | No automatic expiry or deletion. Keep the complete conversation history for research until the owner deletes it. Request deadlines still bound execution. |

Keep existing 60 free turns/member/day, 40 team starts/10 minutes and all paid
caps. Task coordination guard settings retain their current shadow/enforcement
mode; new request/rate bounds are enforced independently, including taskless work.
The numbers above must be constants in the domain and exercised at boundaries.

### Charging and scheduling

The fixed conversation project is the authority for taskless assistance's
charging scope. If a request also names a parent task, it must belong to that
project and be within the caller's existing reach. Reject mismatch before saving
or waking. A sender cannot choose a different payer, transfer a budget or use
the owner's-request exception. Persist charging project on request, wake/run and
reservation. Settlement and every retry keep that attribution, even after the
parent task changes status. Existing task-linked runs keep their old semantics.

Reuse `Consulted` placement for the expert's restricted local work where possible;
do not let a model select its own runtime/tier. Add an explicit answered-request
wake only where existing reasons cannot represent it without task assumptions;
classify it in every exhaustive placement/guard table. The stored request/run
association is authoritative; it must not depend on parsing message text.

Ordinary messages create no model invocation. Answering atomically moves the
request to `answered` and records one pending result notification. It is eligible
only while the request is answered-but-unnotified and its requester still has
authorized actionable work; it is not conditioned on a request still being open.
Cancelled, expired, declined or failed requests never satisfy that answer-wake
predicate. Fold an eligible notification into an existing turn where permitted,
or deliver once after that turn ends. Ack,
read receipt, duplicate send, viewer reconnect and unchanged waiting state never
wake a model. Paused/furloughed members are never resumed implicitly.

### Persistence, recovery and cancellation

Use the existing HQ SQLite database and additive, repeatable schema-open
upgrades. Add conversation/messages and operation records in the first slice;
add assistance/delivery records only when the executing slice needs them.
Declare ports by domain intent and implement transactions inside the adapter.
IDs are never reused, including after expiry/deletion. A unique persisted key
binds idempotency to canonical payload and sender. Test independent store opens,
not just engine locks.

Information acceptance and its idempotency/audit record commit together. The
general pruned event feed is only a change hint, never the primary message
store. Request acceptance and delivery intent commit together. Claiming a
delivery, associating one queued run, accepting one answer and recording one
pending result notification each have conditional transactional boundaries.
Delivery is at least once; logical acceptance/results are idempotent.

Request states: queued -> processing -> answered, declined, cancelled, expired
or failed. Cancellation/expiry may win from queued or processing. Terminal states
never reopen. Compare status and clock inside the state-changing transaction:
an answer committed before cancellation remains answered; a cancellation that
commits first rejects a later answer as late and produces no new result wake.
Store late-disposition metadata without treating its content as a second result.

Reconcile request/run associations on tick/startup. Closing a run before its
request disposition is written is recovered as declined or failed; an existing
live run is never claimed a second time. Drain keeps
delivery intent queued, starts nothing and preserves the existing handover
path. Kill switch/feature disable prevents new starts. Valid waiting requests
do not enter the task stuck ladder; on deadline they expire visibly. Retry
exhaustion is a terminal failed disposition in Work, not a paid polling loop.
No mutating assistance means message retries cannot repeat an external act.

### Retained history, bounded routine work and owner control

Keep all conversation messages, original authorship and result/disposition records
until the owner explicitly deletes them. Closing a conversation or reaching an
assistance deadline stops execution, not historical inspection. A closed or old
conversation never reopens merely because its history is searched or read.
This policy applies to the new explicit communication record, not existing
short-lived raw model transcripts. Do not promise that a message locator preserves
an unrelated source artifact after that artifact's own retention period.

Archive size must not determine the amount of conversation history loaded by
normal HQ snapshots, model briefings, participant inboxes or scheduler ticks:

- Indexed keyset pagination by stable ID; never `OFFSET` or loading all messages
  into a vector before limiting. Read at most 51 rows to return a 50-row page and
  `has_more`. Use `(conversation_id, id)` and explicit participant lookup indexes.
- Keep conversation summary fields (latest message ID/time and message count)
  atomically with new acceptance. An idempotent retry changes none of them. List
  summaries with a bounded cursor query, without joining every historical body.
- Index idempotency by `(conversation_id, sender_member, client_key)`, turn quota
  by `(sender_run, id)` and daily quota by `(sender_member, accepted_day, id)`.
  Quota checks stop at limit+1 rows, rather than counting the lifetime archive.
- Schedule from indexed active delivery/request states and due time. Completed
  conversations and answered requests are not part of normal pending-work scans.
  Phase 3 must replace hot-path `consults()` full-history loads with intent-specific
  indexed queries when consolidating old and new consultation. Do not append all
  new records to an existing unbounded `Vec` API and call that reuse.
- Model context contains only the bounded selected page/summary, never an archive.
  Historical research is an explicit authorized paginated operation. No background
  model call, embedding pass or summarization is triggered by archive growth.
- For later full-text search, reuse bundled SQLite FTS5 and apply scope/tier checks
  to returned records. If FTS is unavailable, report unavailable rather than
  silently scan every message. A broad research query is separate from routine
  inbox/scheduler work and has explicit page/time limits before it is exposed.
- Keep transactions short. Do not hold a live read transaction while a researcher
  reads pages or a model reasons. Large exports stream bounded cursor batches.
  Backups and historical exports necessarily do more work as stored data grows;
  schedule and measure those operations rather than putting them on the tick.

Use the real production SQL in a scale regression: identical current inbox,
pending work and message page, first with 1,000 and then 100,000 older messages
in other conversations and in the same conversation. Verify page bounds, query
plans and SQLite VM work for indexed seek/limited reads. Deliberately drop the
index or replace the bounded query with a full scan to prove the checker fails.
Record wall-clock distributions separately; do not assert universal constant
latency from one timing run. Add larger optional benchmarks before widening the
pilot. B-tree maintenance, disk usage and backups still grow; this decision
requires bounded routine reads, not a claim that infinite history is free.

Owner deletion immediately makes the conversation unavailable and cancels pending
requests, then purges its message/grant/delivery data in bounded batches so a
large archive cannot hold the main writer lock indefinitely. Only minimal
deletion-progress state persists until the purge finishes. IDs are never reused;
replayed keys for a removed conversation fail, rather than create it again.
Existing immutable node receipts are unaffected. Feed events contain IDs only;
views show deleted references as unavailable. Do not promise forensic erasure
of backups or SQLite free pages. Existing backup/privacy policy still applies.

Provide export/delete for new data before real use. This does not claim to close
the pre-existing HQ-wide memory-rights gap in ADR 0136. Pilot activation remains
a separately scoped risk/budget decision. Keep current database journaling in
the initial slice; any global WAL change needs separate evidence and backup/
concurrency testing rather than being smuggled into message retention.

Feature defaults off. Owner inspection/export/delete remains available when off;
member reads/sends and new assistance starts stop. On disable, pending requests
become visibly cancelled; existing running workers lose the new operation's
authority and their later answer cannot revive the exchange. Re-enabling never
resumes cancelled work. Do not automatically fall back into paid escalation.
Prefer forward disable over deploying an old binary against an unproven schema.

Implementation refinement, 2026-10-22: the initial 60-second claim lease is
superseded by atomic local association. The source-grounded integration map
identified an existing SQLite queue rather than an external delivery worker;
there is no crash boundary between claiming and associating the queued run.
Independent-connection/reopen tests in the conversation adapter exercise this
boundary and the answer/cancel race. The existing run retries provide at most
three attempts, bounded by the request deadline, without a second retry engine.
`Consulted` and `QuestionAnswered` keep their established placement meanings;
unique persisted request/run associations distinguish these new turns.
Source locators are a separate `Document`/`Receipt` type, since the existing task
citation type has no document variant. This assistance slice retains Internal-only
sharing: Confidential or unclassified sources are refused rather than exposed
through a grant or model-tier change. Full history retention is unchanged.

Phase 4 recovery clarification, 2026-10-22: an already accepted answer closes its
expert execution before any worker recovery or stale queued retry. Retrying a
worker cannot reopen the logical result. New assistance and result deliveries
count at most three total attempts, including platform interruptions and Mac
unavailability; the legacy consultation/task interruption allowance remains
unchanged. The offline failure matrix exposed and regression-tested both seams.

## Delivery gates and verification

Implementation checkpoint, 2026-10-22: the separate branch contains the information-only
slice, including owner scope controls, paged JSON inspection/export, deletion and content-free
operation history. A read outcome records that a page was returned, not that a recipient
processed its claims. The Work view saves one bounded page; a one-click whole-archive streaming
download is not implemented. An owner may traverse/export the retained pages without a live
archive-wide read transaction. No live installation or pilot has occurred.
Content-free run and member/day admission counters are independent of conversation
deletion and updated atomically with acceptance. They retain counts, without message
text, evidence or conversation links, so explicit deletion does not reset a rate limit.

History reads now persist runtime-observed Confidential input before returning passages.
That taint survives restart, blocks information sends and is inherited by later transcript/
record searches; it is not a member-supplied label. Protocol refusals omit caller-supplied
conversation IDs and message text. Worker directory discovery checks current member eligibility
and shows only active projects within its bounded cursor page. Owner inspection remains available
while disabled.
Worker access is also tied to the enabled switch revision at turn briefing time;
an owner switch during a turn cannot admit a turn started under an older policy.
The feature-off path leaves legacy briefing classification unchanged.
Legacy text with no reliable source-run lineage fails closed for the Internal pilot.
In particular, task-thread posts do not yet carry an immutable author run, so task
turns cannot send pilot messages until that lineage is added. This is a deliberate
limit of the first slice rather than a claim that free-text parsing proves a tier.

Phase 2 delivers information-only messaging, persistence, owner inspection and
export/delete together. No executing assistance is offered yet. Phase 3 adds the
request lifecycle, evidence grants, charging and recovery together. Phase 4
hardens failure coverage before an explicitly authorized live pilot. Office
visualization and comparative rollout follow the reviewed plan.

[B1-B8](../proposals/agent-collaboration-baseline-cases.md) are the frozen acceptance
specifications. Add red-first tests at domain, real SQLite, application, protocol,
node/HQ composition and UI levels as appropriate. Assert zero new task/wake/model
starts for informational send/read, denied forged sender/outsider/tier/token,
rename-safe identity, idempotency conflict, concurrent/duplicate acceptance,
restart, deadlines, cancellation, feature disable and purge. A filtered test
run executing zero tests cannot pass the gate. Run the complete root checks and
inspect served visible changes; no deployment is authorized by passing tests.

## Consequences and alternatives

- Communication becomes independent of task administration, but task ownership
  and consequential authority remain unchanged. New communication grants are
  explicit and revocable by the owner.
- Existing consultation remains usable throughout the rollout. The execution
  slice consolidates it through one application path rather than shipping a
  parallel task/delegation system.
- Fixed participants simplify privacy and idempotency. The owner selected
  permanent history with explicit deletion and bounded indexed routine access.
  Larger rooms and dynamic membership are deferred.
- A broker, A2A conversion or hosted framework adds no demonstrated value to
  this local two-member pilot. See the
  [source review](../proposals/agent-collaboration-source-review-2026-10-21.md).
- Treating every message as a wake repeats measured coordination costs; treating
  every request as speech leaves no accountability. The explicit request boundary
  separates those concerns with deterministic code.
- Granting reviewers broad write/checkout privileges would hide document-reach
  defects. Concrete read-only resources preserve least authority instead.
