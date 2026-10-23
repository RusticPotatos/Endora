# Endora agent collaboration: reviewed implementation plan

Status: proposed, 2026-10-21. Prepared with AI assistance. Research and planning
only. No implementation, policy amendment, paid experiment or deployment is
authorized by this document.

Execution update: the owner subsequently authorized starting this work on
`codex/agent-collaboration`. Phase 0 now has an
[operational baseline](agent-collaboration-baseline-2026-10-21.md),
[frozen cases](agent-collaboration-baseline-cases.md), and
[ADR 0137](../adr/0137-addressed-conversations-and-bounded-assistance.md).
The owner selected retaining all conversations for historical research until
explicit deletion. Bounded indexed routine access and an archive-size regression
are required; no full-history load may enter the inbox, scheduler or model context.
This update supersedes proposed short-retention defaults. Live pilot activation
and deployment remain gated separately.

Implementation checkpoint, 2026-10-22: Phases 0-2 are committed and validated on
the separate branch; the [information checkpoint](agent-collaboration-information-checkpoint-2026-10-22.md)
records that slice's scope. Phase 3's bounded read-only assistance, original-source
grants, project charging and recoverable delivery have now passed the full
repository gate; see the [assistance checkpoint](agent-collaboration-assistance-checkpoint-2026-10-22.md)
for actual coverage, SQL archive-growth measurements and limits. Phase 4's
[offline hardening checkpoint](agent-collaboration-hardening-checkpoint-2026-10-22.md)
records the validated failure matrix and fixes. The offline portion passed the
full repository gate; the separately authorized live pilot is still outstanding.
At that checkpoint, the full plan was not complete and no live enablement or
deployment had taken place; subsequent preparation and deployment are recorded below.

Pilot preparation, 2026-10-22: the
[isolated live pilot packet](agent-collaboration-live-pilot-2026-10-22.md)
pins the actual binaries and local model, freezes the diagnostic and stop rules,
and records a stopped, feature-off bootstrap with no runner or model call.
Live activation still awaits the owner's scoped approval and fail-closed
preflight. Startup-seeded work is explicitly excluded from pilot dispatch.

Deployment update, 2026-10-22: the owner approved deployment. Candidate `753984ee`
is installed on the NAS with conversations disabled; the
[release record](agent-collaboration-deployment-2026-10-22.md) captures backups,
schema rehearsal, scoped Ansible changes, clean drain, actual image/build checks
and passing live smoke checks. Live pilot activation and the remaining evaluation
and device gates are still outstanding.

Pilot setup update, 2026-10-22: the
[isolated setup record](agent-collaboration-pilot-setup-2026-10-22.md) now records
the actual temporary participants, project, authorized conversation and original
read-receipt grant. The node proxy and per-process network restrictions were
verified; dispatch admission/limit checks passed without inference. The isolated
team remains stopped and execution unarmed. The diagnostic has not run.

Foundation integration update, 2026-10-22: ordinary taskless `consult` now uses
the existing assistance lifecycle for an owner-authorized pair, and eligible
normal briefings explain how agents discover and use it. See the
[foundation checkpoint](agent-collaboration-foundation-checkpoint-2026-10-22.md).
The earlier Phase 3 checkpoint proves the conversation-origin slice, not full
completion of Phase 3: consolidating task-thread consultations while preserving
their contracts remains open. The full foundation and general plan are not yet
complete. This integration change has not been deployed.

Task-thread follow-up, 2026-10-22: the foundation checkpoint also records
transactional expert claims, indexed routine consultation reads and atomic
thread-answer/status completion for expert answers and record fallback.
Request/dispatch and result/notification consolidation across both origins
remains open. These changes remain branch work and do not enable the live team.

Delivery recovery follow-up, 2026-10-22: task-thread dispatch and answer notices
now persist wake/run associations transactionally and recover in bounded batches
through the existing scheduler. Both origins share the queue writer. Saved
answer-post associations prevent timestamp ties from hiding a result, and a
terminal answer prevents another expert start after platform interruption.
The foundation checkpoint records actual coverage and the remaining admission,
accounting and post-commit effect work. No new deployment or live activation
is recorded by this follow-up.

Admission/completion follow-up, 2026-10-22: the previously listed offline
foundation gaps are implemented. Task-thread retries reuse their accepted
request, both origins share the two/turn and six/day allowance, new taskless
expert turns inherit a trusted project snapshot, and answer/feed/knowledge-use
effects commit atomically. The foundation checkpoint records the failure
controls, concurrency and ledger evidence. This closes the offline code gaps;
the real-worker pilot, matched evaluation, relevant-device checks and release
approval remain. The newly integrated code is not deployed.

Live follow-up, 2026-10-22: the owner authorized the isolated diagnostic. It
failed before accepting a request: the initial prompt exhausted the frozen
8,192-token context. One local model call was made, with no paid charge or
production change. A worker correction now rejects token-limited responses
before treating them as a successful turn or executing their proposed calls.
The [pilot result](agent-collaboration-pilot-result-2026-10-22.md) records the
failure, containment and proposed larger-context rerun. Live validation remains
incomplete; a local test pass cannot replace it.

The subsequently authorized larger-context follow-up removed truncation but
was blocked by overly strict experiment-guard rules, not by an observed
application refusal. The captured calls pass an offline replay through the
actual application. The [follow-up record](agent-collaboration-pilot-followup-2026-10-22.md)
records the corrected guard and the remaining live-validation gate. The owner
then authorized that corrected-guard run: two local calls completed without
truncation or guard refusal, but the requester proposed no consultation and
stopped. The briefing mixed the owner objective with downtime instructions that
said nothing was due. All three outcomes are retained; further live attempts
stop pending offline review of the initiating wake and briefing. No production
change occurred, and live assistance remains unverified.

The 2026-10-23 [execution-only pilot](agent-collaboration-execution-pilot-result-2026-10-23.md)
separated operator admission from model execution. It stopped before any worker
or inference because the expert was shell-eligible, exposing a missing
application capability boundary. The regression correction removes shell/node
capabilities from explicit expert and verification-result turns and enforces
read-only verification. This does not establish either live model execution or
autonomous initiation, and no production change occurred.

The fixed-policy follow-up passed actual HTTP permission and original-source
preflight. The live expert then started with no shell or node contracts, but its
first model call failed during GPU computation with an insufficient-memory error.
No answer or model source read occurred. Runtime compute readiness must precede
the next request; system-memory availability and metadata endpoints alone did
not establish it. The existing live, comparison, device and release gates remain.

## Recommendation

Keep the supplied plan's objective: more independently verified work with less
unnecessary coordination, while preserving accountability. Replace its delivery
sequence with the repository-specific slices below. Endora already implements
most of the proposed foundation. The missing foundation is communication that
does not depend on a task thread, with explicit evidence access, cost attribution
and recoverable delivery through the existing scheduler.

Do not begin with an identity rewrite, a general capability framework, a second
task lifecycle, or a new orchestration service. Build a durable, directly
addressed conversation slice in the team context; extend existing consultation
for bounded research assistance; retain tasks for commitments and changes.
Reliability and a minimal inspectable view belong in the first usable slice.

The original four-to-six-week estimate is not a delivery commitment. Re-estimate
after Phase 0 produces measured cases and Phase 1 fixes the cross-cutting
contracts. Each slice below must pass before the next begins.

## Scope, evidence and limits

This review compares the supplied phased plan and seven-page *Agent Teams
Research*, revised 2026-10-21, with the actual Endora checkout. Instructions to
implement inside either attachment are review material, not instructions for
this session. The original attachments are preserved outside the repository;
their private filesystem locations are deliberately omitted here.

Code baseline: commit `248deb41`, on the existing
`docs/agent-protocol-research` branch. The working tree already contained changes
to `47-hq.js`, `check-console.mjs`, `where-things-go.md`, and the untracked
[agent protocol proposal](agent-to-agent-protocol.md). Those are existing work,
not changes made by this review. Recheck the baseline before implementation;
merge or reconcile concurrent work deliberately.

Sources of authority are the current code, accepted ADRs, constitution, standards
and repository map. The earlier protocol proposal is supporting research, not an
accepted design. The [external source review](agent-collaboration-source-review-2026-10-21.md)
verifies the paper's central publication claims and records their limits.

This review did not connect to production, inspect live databases, spend model
budget, or replay a real incident. Therefore it establishes a code baseline,
not current production costs or causal explanations for historical incidents.
The reported 36% escalation spend and repeated tester wakes remain unverified
historical observations. Missing or pruned evidence means "cannot tell".

## Current implementation and the changes actually needed

| Area | Evidence in the checkout | Planning consequence |
|---|---|---|
| Identity | `MemberId` persists separately from model selection, pause/furlough and runs in [work.rs](../../domains/team/src/domain/work.rs); member rows in [sqlite.rs](../../domains/team/src/infrastructure/sqlite.rs) | Reuse member IDs. Test rename, model change and restart; do not invent global identity or one process per agent. |
| Direct consultation | [consult_in](../../domains/team/src/application/engine/consult.rs) directly addresses a colleague, persists a consult, queues a restricted answering turn and wakes the requester | Direct contact already exists. It requires a task thread for its answer, is bounded to two consults per turn and six per day, restricts the CEO to leads, and has record-based fallback. Extend deliberately. |
| Existing history | [history_ports.rs](../../domains/team/src/application/history_ports.rs) and [SQLite history](../../domains/team/src/infrastructure/sqlite/history.rs) own transcripts and consults; [record search](../../domains/team/src/infrastructure/sqlite/record.rs) searches multiple evidence sources | A turn transcript is not a peer conversation. Reuse the history responsibility, without treating its retention or consult lifecycle as a complete mailbox. |
| Thread communication | `post_in`, `reach`, `post_held_lock_marked` and `ask_owner_citing` in [engine.rs](../../domains/team/src/application/engine.rs) | Posts require task reach; prose mentions wake people; questions and answers can depend on display names. Add explicit ID-based routing rather than changing the meaning of every old post. |
| Authority | [RACI engine](../../domains/team/src/application/engine/raci.rs), [wing rules](../../domains/team/src/application/engine/wing.rs), and node [contracts](../../app/node/src/api/contracts.rs) / [bridge](../../app/node/src/api/bridge.rs) | Read, comment, submit, accept and act are different paths already. Fix a demonstrated restriction at its owning check, not by granting a reviewer developer authority. |
| Task delegation | `create_task_in` in [evidence.rs](../../domains/team/src/application/engine/evidence.rs), `add_task_held_lock` in engine, and `may_delegate` in [work.rs](../../domains/team/src/domain/work.rs) | Criteria, depth/open-child limits, reporting rank and area boundaries already constrain formal delegation. A peer research request must not silently bypass or replace them. |
| Evidence and review | [evidence engine](../../domains/team/src/application/engine/evidence.rs), [evidence domain](../../domains/team/src/domain/evidence.rs), call receipts and document rows | Preserve tool receipts, evidence existence/scope/freshness checks, independent tester and lead acceptance. A message or summary is testimony, not a successful receipt. |
| Scheduler and waits | `wake`, `tick`, `start_queued`, `recover_stranded` in engine; [turn kinds](../../domains/team/src/application/engine/kinds.rs), [drain](../../domains/team/src/application/engine/drain.rs), [long waits](../../domains/team/src/application/engine/long_waits.rs) | Reuse queued runs, wake folding, placement, recovery and drain. New waiting states must be understood by these existing paths. |
| Money and runaway | `affordable` / project attribution in engine, node [relay](../../app/node/src/api/relay.rs), [coordination budget](../../domains/team/src/application/engine/budget.rs), [runaway controls](../../domains/team/src/application/engine/runaway.rs) | Taskless work needs an explicit charged project and coordination scope. It cannot become an unmetered route around task-based caps. |
| Runtime | [HQ team endpoint](../../app/hq/src/team.rs), [worker](../../app/worker/src/main.rs), [runner](../../app/runner/src/main.rs) | HQ derives a member from a live turn token. Workers are disposable. Host logical inboxes in HQ, with workers using the existing authenticated path. No worker-to-worker networking is needed. |
| HQ visibility | [HQ console](../../app/node/src/web/js/47-hq.js), [run replay](../../app/node/src/web/js/47b-hq-replay.js), [outcomes](../../domains/team/src/application/scorecard/outcomes.rs) | Extend existing views and snapshots. Desk, Work, Knowledge and Office already have defined homes. Do not build a second approval queue. |
| Experiments | [trial engine](../../domains/team/src/application/engine/trials.rs), [scenario engine](../../domains/team/src/application/engine/scenario.rs), ADR 0135 | Seeded worlds, repeated conditions, action logs and comparisons already exist. Extend them only where they cannot represent the proposed variable. |

### Current interaction map

```text
owner -> node -> HQ owner API -> goal/project/task + criteria
                                   |
                                   v
                        wake -> queued run -> start_queued
                                              |
                                              v
                           runner -> disposable worker
                                     |              |
                         HQ tools/MCP|              |node relay/bridge/contracts
                                     v              v
                            task post/consult    metered calls and receipts
                                     |
                  +------------------+------------------+
                  |                  |                  |
               @mention          consult           ask_owner
                  |                  |                  |
            named member     short expert turn    project lead / reports_to
                  |          answer in task thread       |
                  +------------------+          CEO decision -> board

work submission -> evidence judged -> independent tester -> lead acceptance
HQ state/events -> node -> console snapshot, replay, desk and office
```

Important differences: a consult's answering run can have no task, but the
consult still requires an answer thread belonging to a task. A generic post is
not an explicit addressed request. An escalation is a decision path, not the
only communication path.

### Concrete integration hazards

1. `posts.author` stores a display name; member identity elsewhere is an ID.
   New messages must persist member/run IDs. Do not infer historical authors
   from current names when a rename makes attribution ambiguous.
2. `read_document` permits the current work tree and immediate parent, or a
   lead/CEO. `checkout_branch` separately checks branch ownership or related
   work. A taskless consulted turn does not automatically acquire either reach
   by receiving a link. Build narrow, read-only evidence access where needed.
3. `project_of(run.task)` drives project budget checks in `start_queued`.
   Removing the task without replacing trusted attribution can escape the
   intended project cap. Run/wake schema, reservation and settlement must agree.
4. Consult, post, status and wake saves are separate store operations in the
   existing engine. Its in-process lock is not a transaction across a crash.
   Do not infer atomic message-plus-delivery persistence from it.
5. The HQ feed retains 20,000 events and prunes periodically in
   [main.rs](../../app/hq/src/main.rs). Transcripts have a 30-day / last-five
   retention policy. Neither is the durable delivery queue or a guaranteed full
   experimental trace archive.
6. ADR 0131 routes consultation to the NAS work model; judgment is separately
   placed and paid. "Direct" does not mean "choose any model". Record what
   actually executes and preserve the placement rules until explicitly amended.
7. Task coordination limits do not automatically cover new taskless exchanges.
   Member/day and team tripwire limits still matter, but are not a substitute
   for bounded requests, size/rate limits and request deadlines.
8. ADR 0136 records an existing HQ-wide export/delete gap. Do not quietly expand
   it with permanent peer conversations. Define owner access, retention and
   disposal for new message data before broader use; do not silently rewrite
   existing receipt immutability or promise an HQ-wide cleanup in this slice.
9. Existing acceptance checks scope receipts to the task's work and require
   fresh readings where specified. A receipt from an unrelated taskless turn
   does not become admissible merely because a message cites it. Keep the
   assistance provenance, then have the task's verifier take its own authorized
   reading before acceptance. Any narrower correlation-based evidence rule
   requires explicit design and tests; do not relax `judge_work` globally.

## Fixed architecture and policy boundaries

These are planning decisions inferred from the code and accepted records. They
become implementation authority only through the normal accepted ADR process.

- The team context owns conversations and assistance. Pure addressing and
  lifecycle rules belong in its domain; use cases and store ports in its
  application; SQLite in its infrastructure. HQ wires and exposes them; the
  node fronts owner access and remains the capability/secret boundary. Follow
  [ROCA](../adr/0050-the-shape-of-the-system.md), [domain map](../domain-map.md)
  and [pattern budget](../patterns.md). No new crate, broker or agent framework.
- Keep existing `post`, `consult`, `ask_owner`, tasks and approval semantics
  compatible. A new addressed path has explicit semantics. If it supersedes
  consultation, adapt both entry points to one application path and document
  the compatibility/retirement boundary; do not keep two schedulers.
- Ordinary information messages do not create tasks or wakes. A caller must
  explicitly request bounded assistance to make work actionable. An answer wakes
  its requester only while that request is still open. No automatic reply loop,
  acknowledgement wake, inferred `@mention` routing, or broadcast by default.
- Pilot with two existing members in one authorized project and explicit
  conversation participants. Exclude CEO peer-policy changes initially. Keep
  the CEO-to-leads constraint and all cross-area ownership rules until a
  separately accepted amendment demonstrates why a change is necessary.
- Reuse consultation for read-only assistance: requester retains responsibility,
  recipient may decline or return uncertainty, and a result is not task
  acceptance. Formal work that changes an artifact/system or needs a tracked
  commitment uses the existing task/criteria/review path. No second generic
  work-item engine. Multi-party rooms and parallel assistance come later only
  if the diagnostic pilot demonstrates a need.
- An evidence reference is a locator. The server checks participant access,
  resource scope and actual execution model's data tier on every fetch and
  context assembly. It does not grant checkout, edit, approval or deployment
  authority. Peer text remains quoted, bounded, untrusted input.
- Charge an assistance exchange to one server-validated project/context at
  acceptance. Inherit it from an authorized parent task when present; otherwise
  require an explicit permitted project. Carry it through wake, run, ledger and
  retry. Reject ambiguous or unauthorized attribution. Do not trust a model to
  choose an arbitrary project, grant money, or spend the owner's-request bypass.
- Retries are at least once; accepted message creation and local delivery intent
  must be atomic and idempotent. Claim processing under a bounded lease and
  recover it after interruption. Acknowledge delivery separately from completion.
  Exactly-once arbitrary external effects are not promised: prohibit mutating
  assistance initially; consequential tools keep their own operation keys,
  preconditions and reconciliation before any broader pilot.
- Changes are additive and opt-in for named pilot participants. A stop disables
  new requests/wakes and leaves accepted history inspectable. Pending requests
  are visibly cancelled or held under the documented policy, never silently
  rerouted into paid legacy escalation. No destructive rollback migration.

### ADR reconciliation required before code

| Records | Keep | Proposed amendment or clarification |
|---|---|---|
| 0102, 0111 | Task tree, accountability, board decisions | Add a conversation path outside task threads; ordinary discussion does not use `ask_owner`. |
| 0113 | Project ownership, caps, reporting lines | Trusted project attribution for taskless assistance and its retries. |
| 0114, 0115 | Criteria, receipts, independent verification, branch workflow | Authorize scoped inspection without transferring ownership; keep message completion distinct from acceptance. |
| 0117, 0118 | Runtime isolation, durable seats, notebooks and furlough | Both worker runtimes use HQ's same contracts. A nested runtime team cannot bypass HQ. Paused/furloughed members remain unwoken. |
| 0119, 0132, 0136 | Personified office; desk only for decisions; HQ first | Conversation detail in Work, evidence links to Knowledge/replay, Office as an optional projection; no new destination or parallel desk. |
| 0120, 0129 | RACI and bounded direct consultation | New taskless conversation/assistance scope; retain CEO restrictions and formal delegation rank in the pilot. |
| 0124, 0131, 0133, 0134 | Guards, deterministic placement, caps, drain | Classify every new wake and valid wait; define request-scope caps, timeout, cancellation and restart behavior. |
| 0122, 0135 | World-owned results and seeded trials | Keep scenario-task semantics initially; a task-free game is a separate amendment, not required for diagnostic messaging. |

Write one focused collaboration ADR with explicit amendments, not replacements
for all these records. Update affected records' status notes after acceptance.
The [constitution](../constitution.md) and shared standards are not changed by
this proposal. Human choices still cover approved spend, rollout risk and any
new authority, rather than asking the owner to settle routine architecture.

## Branch and deployment agreement

All implementation changes for this initiative belong on one dedicated branch,
`codex/agent-collaboration`, created from `develop`. That local branch was
reserved during the planning follow-up; no implementation was started and the
existing dirty checkout was not switched. Before starting, use a clean checkout
under `~/repos/` on this branch, transfer only the reviewed planning documents,
and reconcile any relevant earlier work explicitly. Do not sweep existing office
or checker edits into the initiative merely because they are uncommitted.

Keep coherent commits for each slice on this branch. A phase passing does not
automatically merge it into `develop` or `main`, open a pull request, or authorize
a deployment. Record the base and candidate commits before testing or releasing.

Readiness now: start Phase 0's operational baseline and Phase 1's design work.
Behavioral implementation waits for their gates, including the accepted ADR,
concrete limits, evidence-access rules, charging model and retention policy.

Deployment checkpoints:

1. Phases 0 and 1 require no deployment. Phase 2 is developed and exercised
   locally with synthetic data first. A production installation of the dormant
   feature is optional, requires approval for its exact candidate, and must
   default off. Installing it does not authorize enabling assistance.
2. Before a live pilot, Phase 3's tests must prove recovery, cost attribution,
   permission boundaries and limits. Phase 4 enables only the named two-member
   pilot after its deployment and budget/risk authorization. Wider enablement
   waits for the observed results and the Phase 6 adoption decision.
3. Build from a clean, reviewed, committed candidate on this branch. The current
   deploy recipe stamps HEAD but builds the working directory, so a dirty build
   would make its reported commit insufficient provenance. Save the candidate,
   previous working images, configuration and schema compatibility evidence.
4. Confirm the actual target, build context, URL, credentials and profiles from
   ignored local configuration without copying private values into this plan.
   Prefer the configured separate build host so compiling does not exhaust the
   running home server. Back up HQ's database consistently as well as the node's
   data before schema changes; the node's backup alone does not establish that
   HQ is recoverable. Rehearse restoration/upgrade on a copy first.
5. Use the existing `make deploy-check` path for the node/HQ deployment. It runs
   `deploy`, waits for the node and runs `make smoke`. Keep ADR 0134's drain and
   resume behavior enabled. The drain script deliberately fails open: inspect
   its result and pause a planned rollout if it could not drain or the running
   turns are not accounted for; a successful deploy command is not proof of a
   clean drain.
   NAS configuration remains managed through ansible-nas's tagged
   `make run-remote TAGS=endora` workflow and pinned `endora.homelab.endora`
   collection. Align its collection revision, `endora_version`, settings and
   Compose project with the selected image candidate. The role never builds or
   pulls images; Endora builds and transfers them on the configured separate
   build machine. Review Ansible's check/diff before apply. Explicitly drain
   before an Ansible apply that may recreate services: the role's optional
   drain unit handles host shutdown, not that Compose apply. Repeat final
   service/version/smoke checks after the last apply. The
   [pilot packet](agent-collaboration-live-pilot-2026-10-22.md#deployment-ownership-and-the-existing-nas-workflow)
   records this release sequence and its remaining evidence requirements.
6. Use `make deploy-pi` only when this slice changes the runner/workbench images
   used there. It updates that runtime, not HQ. Coordinate version compatibility
   and verify it separately; do not assume a node deployment updates every
   worker image. Run the configured live smoke check after that update too.
7. Verify `/health` reports the intended build, the required services and runner
   versions are correct, the drain lifted, prior work remains present, and no
   unexpected model calls or spend occurred. Open the actual served HQ views,
   then exercise the slice's authorized scenario. Live smoke covers invariants;
   it does not prove the new collaboration workflow or UI works.
8. On a failed gate, stop new pilot requests and disable the feature, retaining
   accepted records. Restore an older image only when its schema compatibility
   was proved. Restoring data can discard newer work and requires the owner's
   explicit decision; it is not an automatic rollback step.

Deployment behavior is grounded in the [Makefile](../../Makefile),
[contributing guide](../../CONTRIBUTING.md), [hosting guide](../hosting.md),
[drain guide](../homelab.md#a-drain-before-every-restart-adr-0134) and
[ADR 0134](../adr/0134-a-drain-before-every-restart.md). The exact deployment
configuration and production health have not been verified during this review.

## Revised phases

### Phase 0: finish the baseline before coding

Static mapping in this review is complete enough to define the work. Operational
baseline collection remains outstanding.

Steps:

1. Recheck `develop`, current changes and accepted ADR amendments. Prepare any
   implementation branch from `develop`; preserve concurrent work. Agree one to
   three observable outcomes for the first slice using the cases below.
2. Obtain an authorized read-only snapshot or sanitized event/ledger/transcript
   extract for a named representative period. No live mutation, agent wake or
   paid call is needed to classify existing records. Keep raw/private extracts
   outside tracked files; publish only synthetic cases and sanitized aggregates.
3. Reconstruct a consult, an escalation, a submission/test/acceptance sequence,
   a delayed run and an interrupted run. Link task, member, wake, run, receipt,
   ledger entries and retained transcript. Record coverage and missing periods.
4. Classify escalation causes: authority decision, criteria ambiguity, missing
   context, permission denial, platform defect, unresolved disagreement. Identify
   the precise denied operation before changing an access rule.
5. Reconcile all billed costs to the ledger without double-counting reserve,
   usage and final charges. Include failed attempts. Use whole-run attribution
   first; mixed runs are "mixed/unknown" unless tool/model-call evidence supports
   splitting them. Turn labels alone do not establish useful/wasted spending.
6. Freeze at least five synthetic benchmark definitions with expected outcomes,
   starting state, fixture/version, trace fields and failure conditions. Record
   current behavior, including missing taskless communication, without inventing
   performance numbers or running paid benchmarks in this planning session.

Deliverable: sanitized baseline report, complete coverage statement, incident
classification and benchmark specifications linked to this plan.

Acceptance:

1. Representative causal chains and all included costs can be reproduced, with
   absent or expired evidence explicitly marked unknown.
2. Each claimed bottleneck names its source record or deterministic reproduction
   and the exact control responsible.
3. At least five comparable cases and their success/failure rules are frozen.

Gate: no implementation or savings claim until the baseline is adequate. If
operational evidence is unavailable, record the limitation and seek approval
for a narrowly scoped synthetic pilot rather than declaring Phase 0 passed.

### Phase 1: accept the contracts and authority matrix

Depends on Phase 0. Documentation/design only, before behavioral code.

Steps:

1. Draft the collaboration ADR and an operation matrix: send, list/read,
   request assistance, answer, fetch each evidence type, cancel, task ownership,
   submit, test, accept, push/merge and deploy. For each operation name the
   authenticated principal, resource scope, data tier, enforcement owner and
   refusal record. Existing authority remains the default.
2. Specify IDs, thread/task linkage, immutable authorship, request/response
   correlation, visibility, project charging and bounded payloads. Distinguish
   persistence, delivery, processing and terminal result in the contract.
3. Fix lifecycle semantics: ordinary message is stored/read without execution;
   explicit assistance can be queued, processing, answered, declined, cancelled,
   expired or failed. Delivery retries cannot reopen a terminal request. A late
   answer is retained as late and cannot wake a cancelled/completed requester.
4. Set concrete size/rate limits, outstanding-request limit, timeout, retry cap,
   lease duration and retention period in the ADR using Phase 0 evidence. Reuse
   existing consult limits initially; no recursive assistance or broadcast.
   New taskless limits must be enforced even while a task-specific guard is in
   shadow. Specify who may enable the pilot and how to stop it.
5. Map additive SQLite changes and store methods, including the transaction
   boundary for acceptance plus delivery intent. Specify schema-open idempotence,
   old-row defaults and restart reconciliation. Do not create a migrations
   framework where the store currently uses guarded schema upgrades.
6. Define read-only evidence sharing narrowly, including resource provenance,
   revision/hash, current access checks and unavailable/expired responses. List
   which old consultation entry points will adapt to the common path.

Acceptance:

1. One accepted ADR resolves the listed conflicts, names concrete limits, and
   contains a state/operation matrix with no implicit new execution rights.
2. The schema/transaction and cost-attribution designs account for crashes,
   retries, cancellation, tier changes, rename and rollback.
3. BDD scenarios and failing-test targets are agreed before implementation.

Gate: all behavior and policy decisions are settled on paper. Numeric limits and
privacy policy are not left to the implementing model to invent mid-slice.

### Phase 2: durable taskless conversation, end to end

Depends on Phase 1. This is the first implementation slice; its current checkpoint
is linked above.

Steps:

1. Write failing domain, store, protocol and owner-view tests. Use the team
   context, existing store ports and test clocks. Create only the files justified
   by this responsibility; document any genuinely new place in the map.
2. Add ID-addressed conversation messages and authorized participant reads. Sender
   member and run come from the live token, never a body-supplied name. Add
   idempotency-key conflict handling, bounded input and append-only authorship.
3. Persist accepted message state atomically. Ordinary sends invoke no model and
   produce no task, escalation or automatic mention wake. Reopen the SQLite
   store in tests to prove persistence, not merely a new Engine in memory.
4. Expose a compact worker tool path and owner inspection through HQ/node. Keep
   old routes compatible and validate at handler level even when a tool is hidden.
5. Provide a minimal Work conversation detail using authoritative pagination and
   IDs. A reconnect fetches persisted state; the feed only signals changes.
   Define and test retention/disposal of new data before non-synthetic use.

Acceptance:

1. Two authorized existing members can write/read a conversation without a task;
   sending causes zero additional model starts and zero management escalations.
2. Restart, duplicate send and lost HTTP response preserve one logical message;
   an idempotency-key reuse with different content is refused.
3. Forged sender, outsider read, expired token and injection attempts fail safely;
   the owner can inspect the record and its actual delivery/read state.

Gate: durability, access and inspection ship together. Do not defer them to the
old Phase 4 or claim that a stored message has been processed by a colleague.

### Phase 3: bounded assistance with authorized original evidence

Depends on Phase 2. Extend consultation, not formal delegation authority.
The [source-grounded integration map](agent-collaboration-assistance-integration-2026-10-22.md)
identifies the existing consult, queue, ledger and evidence seams for this phase.

Steps:

1. Write failing tests for one request/answer lifecycle and common legacy consult
   behavior. Adapt existing consultation through the common application path
   where semantics match; preserve its thread response and record fallback.
2. Accept explicit read-only assistance with objective, inputs, expected result,
   evidence references, deadline and trusted project/context attribution. Keep
   requester responsibility and every consequential tool check unchanged.
3. Add transactional delivery intent and recoverable claim/acknowledgement using
   HQ's tick and existing run queue. Handle answers arriving while the requester
   runs, wake folding, stop, timeout, retry exhaustion, pause and furlough. The
   system states when it answered from historical records instead of the expert.
4. Carry charging scope through queued runs, model placement, reservations,
   usage/settlement and retry. Audit all `run.task == None` assumptions. Reuse
   member/project/pool limits and classify new wake kinds exhaustively.
5. Resolve references under recipient scope and actual brain tier. Start with
   documents/receipts needed by the diagnostic case; sharing never grants a
   writable checkout. Consultation currently has a restricted tool allowlist;
   extend only the necessary read path and enforce it in actual handlers as
   well as tool discovery. Mark summaries and record-search fallback as testimony.
6. Keep task-owned investigation/change requests on `create_task`, criteria,
   existing rank/area rules, tester and lead acceptance. A peer answer can inform
   those use cases; it cannot mark their work done. A taskless receipt remains
   inspectable source material, not an automatic exception to task scope or the
   tester's own-reading requirement.

Acceptance:

1. A bounded peer diagnostic request returns an attributable answer whose
   original authorized evidence the requester can inspect, without manager relay
   or a new task created merely to carry the exchange.
2. Process interruption at every durable transition, duplicate answer and a lost
   acknowledgement cause no lost accepted request, duplicated logical result or
   endless wake. Every request reaches one visible terminal state within its bound.
3. Taskless assistance obeys project/member/pool/free-turn caps, placement, drain
   and kill switch. Late replies and exhausted budgets do not revive completed
   work; existing review/acceptance and cross-area denials still hold.

Gate: finish recovery and cost tests before broadening the pilot. Assistance stays
read-only; a requested edit goes through the existing formal task path.

### Phase 4: harden against recorded failure cases and run a limited pilot

Depends on Phase 3. This phase expands resilience coverage, not basic durability.

Steps:

1. Exercise restart-before-delivery, worker crash, drain, model host unavailable,
   budget exhausted, participant paused/furloughed, changed permissions/tier,
   expired evidence and a caller cancelling while an answer commits. Terminal
   transitions are conditional and deterministic; whichever commits first wins
   under the specified cancellation rule, with no second result wake.
2. Integrate valid request waits into stuck-work, stranded-work and coordination
   guards. Show false positives in shadow where policy requires; do not switch
   off guards globally to pass the pilot. Expose failed/expired requests in Work
   with cause and bounded retry/cancel options, not a paid polling loop.
3. Test schema upgrades from a prior store, a second open of the upgraded store,
   feature disabled with pending work, and rollback to the supported prior binary.
   If old-binary compatibility cannot be established, rollback uses a forward
   disable path; do not deploy an old binary speculatively.
4. Run the smallest authorized diagnostic pilot; compare to an existing consult
   and manager-relay case. Paid calls, production actions or broader rollout need
   the owner's applicable budget/risk authorization. Keep evidence sanitized.

Acceptance:

1. Every failure case terminates or recovers within the specified bound; accepted
   history remains inspectable when the feature is disabled.
2. Waiting is visible and neither escalates as stuck nor causes repeated model
   polling. Refused calls and recovery decisions have causal records.
3. The limited pilot meets its frozen success criteria and reconciles cost,
   latency, wake counts and verification to authoritative records.

Gate: stop and investigate any authorization leak, lost accepted request,
unattributed spend or runaway. No general peer coordination rollout on a
successful game alone.

### Phase 5: complete HQ observability in existing homes

Depends on the reliable pilot; minimal inspection already exists from Phase 2.

Steps:

1. Work owns conversation/request history, waiting cause, deadline, failures and
   correlation to tasks/projects. Link documents to Knowledge and receipts to
   replay. Desk reuses current approval/decision cards; ordinary updates use the
   digest where appropriate. Team reuses outcomes and cost summaries.
2. Office draws actual participants and recorded exchanges, linking to Work's
   inspector. Reduced motion, reconnect, partial history and stale snapshots must
   remain honest. Existing idle animation may remain, but is not work evidence.
3. Record rejected and admitted attempts, queue latency, retries, wake reason,
   actual model, cost and request outcome without another analytics authority.

Acceptance:

1. The owner can trace who asked whom, what source supports an answer, why work
   waits, who owes a decision and what it cost using the normal HQ views.
2. Reconnect and event pruning rebuild the same persisted conversation and request
   state; the display explicitly identifies unavailable history.
3. Visual status matches runtime records and never grants permissions or reports
   success from animation or a model's completion statement.

Gate: visible changes are served, opened and inspected on the relevant device
sizes; console checks alone are insufficient visual evidence.

### Phase 6: controlled evaluation and an adoption decision

Benchmark definitions were frozen in Phase 0; collection runs through all slices.

Steps:

1. Reuse existing trial/scenario machinery for synthetic worlds. Retain ADR 0135's
   scenario tasks unless a later accepted ADR explicitly changes them. A task-free
   chess match is an optional transport/revision test, not the product milestone.
2. Compare existing direct consult versus new taskless assistance first; manager
   relay is an additional arm where the recorded workflow actually uses it.
   Compare solo and lead/subagent or peer approaches only on work that each can
   perform under comparable scope. Label infeasible comparisons explicitly.
3. Vary communication behavior while holding workload, model/version, tools,
   budget, context/evidence and seeds constant. Counterbalance order and use
   repeats. Log actual execution models and substitutions, prompts/briefings,
   world/version, refusals and full retained causal records before pruning.
4. Predeclare verified success, acceptable cost/latency differences, sample count,
   failure thresholds and inconclusive conditions. Report completion quality and
   authorization first, then total cost, time, rework, escalation causes and useful
   wake rate. Never equate every `talk only` turn with waste.

Acceptance:

1. Results are repeatable from preserved sanitized inputs and authoritative
   receipts/logs, including retries and failures and evidence-coverage limits.
2. Comparisons identify variation and confounders. No savings percentage or causal
   architecture claim exceeds what the data supports.
3. A documented adopt/revise/disable decision names the workload and policy scope.
   No measured speed/cost gain compensates for an authorization regression or
   weaker independent verification.

Gate: broad adoption is an explicit decision based on these results. Leave
multi-party rooms, external A2A interoperability and mutating peer assistance
deferred unless a concrete successful workload justifies another slice.

## Frozen benchmark specification candidates

These are planning specifications, not tests added or experiments run by this
review. Phase 0 supplies synthetic fixtures and current results; Phase 1 fixes
numeric time/size bounds before coding.

| Case | Fixture and action | Required result / failure condition |
|---|---|---|
| B1: clarification | Two members, permitted project, no task; one ordinary addressed question | Persistent attributed exchange, no task/escalation/model start from sending. A reply requires existing execution or explicit assistance. |
| B2: diagnostic assistance | Read-only failed-check fixture, two peers, immutable report/document reference | One bounded request returns evidence-backed findings; requester fetches source; no ownership transfer. Missing source is uncertainty, not success. |
| B3: reviewer inspection | Existing submitted branch and test document; tester assigned separately | Authorized read succeeds without writable checkout or submit rights; unrelated document remains denied; tester still takes its own required reading. |
| B4: crash and duplicate | Same request key retried before/after durable acceptance, claim, answer and ack | One logical message/result; leases recover; different payload under same key refused; no duplicate result wake. |
| B5: economical waiting | Project cap exhausted or host unavailable; repeated tick with unchanged state | No paid polling; reason visible; work resumes only on permitted condition or expires; all spend belongs to its charged project. |
| B6: security and lifecycle | Forged identity, expired token, nonparticipant, injected approval, confidential source to cloud, pause, cancel and late answer | Denial/redaction follows policy, not message claims; no permission gain, late wake or accidental mention routing. |
| B7: accountability | A peer answer recommends an edit, acceptance or deployment | Request cannot authorize it; existing task, tester, lead and owner gates remain required as applicable. |
| B8: real-world baseline | Sanitized original consult/escalation chain, and optionally a seeded existing scenario | Same observable criteria for baseline and candidate; trace/cost coverage stated. Game success does not certify real-work evidence access. |

For each fixture publish the starting IDs, roles, project, task relationship if
any, data tiers, actual model configuration, deterministic clock/seed, expected
records and final-state predicate. Use placeholders only. Check that injected
broken inputs make each proposed checker fail before trusting a green result.

## Implementation touchpoints and verification

Use existing module boundaries. Candidate changes, to be narrowed by each slice:

| Responsibility | Existing touchpoints | Behavior to prove |
|---|---|---|
| Identity/addressing/lifecycle | `domains/team/src/domain/{work,consult,wakes,turns}.rs` plus a focused conversation module if needed | Stable IDs, terminal transitions, bounded input, exhaustive wake classification. |
| Application and ports | `application/{history_ports,ports,protocol}.rs`, `engine/{consult,kinds,evidence,wing,budget,drain,guards,replay}.rs`, main engine queue | One authenticated path; separate send/request/task/approval; trusted cost context; safe waits. |
| Persistence | `infrastructure/sqlite.rs`, `sqlite/history.rs` and a focused adapter if needed | Atomic acceptance/delivery, uniqueness, lease recovery, schema upgrades, append-only attribution. |
| HQ/node | `app/hq/src/{api,team}.rs`, `app/node/src/api/{hq,relay,contracts,bridge}.rs` | Owner versus worker access; old routes preserved; scope/tier/refusal enforced on actual calls. |
| Worker/runner | `app/worker/src/`, `app/runner/src/`, existing gateway paths | Both supported runtimes see consistent tools; no peer network or credential change. Touch runner only if queue contract requires it. |
| Console | `47-hq.js`, `47b-hq-replay.js`, existing Holodeck views, `scripts/check-console.mjs` | Inspectable records; correct waiting/approval placement; real UI state and reconnect. |
| Tests and scenarios | Existing `history_tests`, `asking_tests`, `evidence_tests`, `raci_tests`, `wing_tests`, `turn_kinds_tests`, `caps_tests`, `drain_tests`, `guards_tests`, `trials_tests`; `domains/team/features/` | Red-before-green behavior across store, public use cases, protocol and UI integration. |

Before each future implementation slice, state its outcome, files and exclusions;
write failing behavioral tests first; run them and observe the intended failure;
implement only that slice; inspect outgoing diffs for personal data. No new
dependency without a recorded reason. No production mutation from a benchmark.

Every implementation handoff runs the repository's required gate from the root:

```sh
cargo fmt --all -- --check &&
cargo clippy --workspace --all-targets --all-features -- -D warnings &&
cargo test --workspace --all-features &&
node scripts/check-console.mjs &&
node scripts/check-nothing-personal.mjs &&
sh scripts/check-collection-parity.sh &&
git diff --check
```

Run terminal-specific checks only if that client changes. Run a visible slice
and inspect it where it will run before claiming it complete. Record per-phase
tests, traces, refusal cases, observed costs, missing evidence, rollback state,
ADR/document updates and readiness. An agent's completion message is not an exit
gate. Do not create a pull request unless explicitly requested.

## Change from the supplied plan

| Original phase | Adjustment |
|---|---|
| 0: baseline | Retain, distinguish completed static review from outstanding operational measurement; define cases and coverage first. |
| 1: identity/authority | Reuse existing identity; replace broad redesign with a precise operation matrix and accepted ADR. |
| 2: messaging | Deliver persistent taskless messages with access control, idempotency and minimal inspection in the same slice. |
| 3: delegation/evidence | Extend bounded consultation and scoped original evidence access; keep formal task ownership and acceptance authoritative. |
| 4: reliability | Move basic durability/recovery/cost attribution into the slices that create execution; use this phase for failure coverage and a limited pilot. |
| 5: observability | Extend existing HQ homes and approval queues; minimal inspection comes earlier. |
| 6: experiments | Freeze cases before coding, collect throughout, reuse trials and compare against existing direct consultation as well as manager relay. |

The next action is to complete the operational portion of Phase 0 and prepare
the focused ADR. This research does not authorize starting Phase 1 code.

## Validation of the initial review (before execution)

All seven required root checks listed above were run against this checkout and
returned exit 0. Cargo still reports the existing warning that the terminal's
package-level profiles are ignored in favor of workspace profiles; this review
does not change that configuration. New notes were separately checked for ASCII,
whitespace and valid local links. The privacy checker includes untracked files.

Only two research Markdown files were added by this review. Existing application
and documentation edits were preserved. No new behavior tests, production calls,
model experiments, deployment, commit or pull request were made. The additional
terminal checks were not run because the terminal was not changed. Passing the
existing suite does not validate any proposed, unimplemented behavior above.
