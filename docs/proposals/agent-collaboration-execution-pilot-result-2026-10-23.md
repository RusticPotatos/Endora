# Agent collaboration: execution-only pilot and capability correction

2026-10-23, owner's local date. AI-assisted work on
`codex/agent-collaboration`. Initial application candidate: `7337ecde`.
Latest candidate: `07eedb8d`; bounded shared-runtime execution passed with an
answer-quality caveat. Earlier failed attempts below remain part of the record.

## Outcome: stopped before any worker or inference call

After the three unsuccessful end-to-end diagnostics and the offline briefing
correction, the owner's continuation authorized the next staged foundation
check. This deliberately separated execution from model initiation: an operator
used the authenticated ordinary consultation protocol on one actual taskless
turn, then the experiment would run the real expert and requester-result
workers. Operator admission could not satisfy the model-initiation criterion.
The dispatcher explicitly refused any initial requester worker.

The frozen scope retained the same project, pair, immutable `r1` source, model,
runtime and 16,384-token context. Limits were one diagnostic request, eight
physical starts, 48 local model calls, a 20-minute watchdog and zero paid calls,
external actions or production changes. Both workers had to read the original
and explain its expected 200, observed 503 and unavailable `fixture-cache`.

Fresh application binaries, an offline operator-admission replay, 20 harness
tests, dispatcher checks, actual stopped-service configuration, refreshed source
grant, network isolation and fingerprint checks passed before activation.
Unapproved controller entry was shown to refuse before services or inference.
The fresh stores came from the original pre-live synthetic preparation snapshot;
earlier attempts were retained separately. Admission reused the real public
application/protocol path, not SQL-created lineage or a fabricated source.

The operator's request was accepted; its exact retry returned the same ID.
The live controller then stopped in 3.94 seconds because the expert assignment
advertised shell eligibility. The private dispatcher required that read-only
expert to be ineligible and refused before starting it. The model server had
loaded, but there were zero physical workers and zero inference calls. The one
accepted operator request was cancelled during cleanup. No answer, source read
or requester-result turn occurred. All ledger amounts were zero. Conversations
were disabled, global stop persisted, the envelope was disarmed, recorded
processes terminated and all pilot listeners closed. No replacement request
was created and the guard was not weakened.

## Confirmed application defect and correction

The application regression reproduced the observed eligibility defect:
`may_use_a_shell` returned true for the explicit read-only expert. The shell
policy only considered deploy checks and task-owned project restrictions.
These taskless assistance runs bypassed those restrictions. The node-capability
policy similarly returned unrestricted role defaults instead of an empty list.
The isolated pilot's cleared worker environment and disabled contracts prevented
those capabilities being exercised; production policy still needed correction.

Explicit conversation-assistance experts and their associated result-verification
turns now receive no shell and an empty node-capability list. They cannot inherit
execution rights from the conversation's charging project. Result turns also
receive only the existing read tools plus granted-source reading. The expert
alone receives `answer_assistance`. Handler checks refuse direct mutation,
delegation and recursive-assistance calls, independently of tool advertising.
Verification reads stop after six protocol read rounds, with a finish instruction
rather than an instruction to submit another answer. Original-source grant and
revocation checks are unchanged.

The correction uses persisted expert/result associations. It adds no capability,
scheduler, service, schema or dependency. Ordinary turns retain their existing
eligibility; legacy task-thread consultation retains its existing contract.
The application test covers the complete ordinary-consult/answer/result path,
including direct forbidden calls and the verification read bound. An HTTP
regression checks the actual runner's `shell: false` and node's `only: []` for
both stages, even when the conversation project lists a shell and lab tools.
It also checks that the original ordinary turn remains shell-eligible.
The owner scenario now checks the expert's capability restriction.

Private evidence is retained in `.local/agent-collaboration-pilot-execution/`:
operator admission, rejected assignment, stopped stores, controller result,
fingerprints, red/green regression logs and separate evaluated result. Machine
paths, credentials and raw records remain ignored. The unrelated baseline script
edit is preserved and excluded.

## Remaining gates

This launch failure is not a successful live assistance result. The correction
passed the complete repository gate: formatting, workspace Clippy with denied
warnings, workspace all-feature tests, console checks, personal-value checks,
collection parity and whitespace checks. The gate includes the updated owner
scenario, handler/read-bound regression and actual HTTP policy test. Existing
ignored tests remained ignored. No terminal-client source changed or new device
validation ran. A newly pinned isolated candidate must still establish the
expert/result model behavior. Autonomous initiation remains
a separate unproven criterion. Matched evaluation, relevant device checks and
the production release decision remain outstanding. No new deployment occurred.

Before another model load, extend preflight to inspect the actual expert and
verification assignments and their node-capability payloads on a separate
offline/service replay. An operator-authored preflight answer must remain labelled
as such. Check `shell: false`, `only: []`, the granted-source tool and direct
mutation refusal for both stages. Mocked assignment shapes alone did not expose
this missing application boundary. Keep the live guard strict.

## Fixed-policy follow-up: GPU compute failure

The owner's next continuation authorized one execution-only follow-up with
candidate `64a4253a` and the same scope, model, runtime, context and budgets.
A fresh packet preserved the prior failure. Before model loading, an actual HQ
HTTP rehearsal on a separate store verified both linked assignments:
`shell: false`, node policy `only: []`, the granted-source tool, and direct
mutation refusals. Operator calls in that rehearsal read the same original from
both stages and retained project-scoped zero-amount reservations. Three broken
policy controls per stage were correctly rejected. No worker or model performed
the rehearsal; its operator-authored answer is explicitly labelled preflight.
The primary preparation store remained stopped with no requests.

The 20 harness tests, network isolation, source-grant refresh, binary/supervisor
fingerprints and unapproved-entry refusal also passed. The ensuing live test
accepted one operator-started request and started its actual expert worker with
no shell. The model was offered eight permitted read/answer tools and no node
contracts. The capability correction therefore held at the actual launch.

The first model-call attempt failed with HTTP 500 `Compute error.` The pinned
runtime logged Metal `Insufficient Memory` during prompt computation. The
controller stopped after 5.07 seconds. There were no successful model responses,
answers, original reads by a model, or result workers. The node recorded no
successful usage row; that does not erase the failed call attempt or expert
physical start. Paid charges remained zero. The one request was cancelled during
cleanup, all pilot processes and listeners were verified stopped, conversations
were disabled and the execution envelope was disarmed. No replacement request
or changed-setting retry was made.

The measured rendered prompt was 1,866 tokens, with 10,390 tokens beyond the
response reserve and margin. This was not a context-exhaustion result. Reported
system memory availability was 25 percent after model load and 27 percent at the
worker sample, exceeding the existing 10-percent threshold. Those host readings
did not establish that the GPU could allocate its compute buffers. Another
`llama-server` process remained after pilot cleanup; its model, service identity
and causal contribution were not established, and it was not changed or stopped.

This follow-up verifies the actual permission boundary and its operator-driven
HTTP sequence, not live model cooperation. Before another request is admitted,
runtime readiness must exercise GPU computation in a controlled resource window;
health/template/tokenizer responses and host free-memory percentages alone are
insufficient. Any runtime control inference must be labelled and counted, with
its settings and budget frozen before the next experiment. Do not pause another
service without identifying it and establishing authorization to interrupt it.
Expert/result model behavior, autonomous initiation, matched evaluation, relevant
device checks and production release remain outstanding.

Private records are retained separately in
`.local/agent-collaboration-pilot-execution-v2/`, including the actual HTTP
rehearsal, negative controls, runtime error, audited failed response, stopped
stores, prompt measurement and evaluated result. Application source did not
change in this follow-up. The complete passing gate recorded for `64a4253a` is
reused; prose changes receive fresh privacy, local-link and whitespace checks.

## Runtime diagnosis and compute-gated follow-up

Read-only inspection found a resident local server using the same pinned runtime
binary, model file and sampling settings, with a 65,536-token context rather
than the isolated pilot's 16,384. Its slot was idle when inspected. It was not
stopped, restarted, reconfigured or used for control inference. Two copies of
the model were therefore resident during an isolated model load, but this alone
does not prove the exact GPU allocation that failed.

The failed pilot's log also showed GPU insufficient-memory errors during warmup,
before its health endpoint reported ready. A readiness regression using that
actual captured startup log failed with the former health-only predicate and
passed with explicit startup/compute-error rejection. The earlier successful
runtime capture and loading-state checks passed too: three focused tests total.

A bounded resource check then compared the original profile with one change:
physical batch size 64. It allowed two model starts, two direct local control
calls at most, four output tokens per call and a five-minute original watchdog.
The original profile completed one control call. The smaller physical batch
logged a warmup memory error, so readiness rejected it before a call was sent.
The check made two isolated model starts and one inference call, no agent
requests and no paid calls; both test processes stopped. Elapsed time was 173.43
seconds, including setup. The first slots-status check lacked authentication and
failed before any model start; its record was retained, the scoped read was
corrected, and the original watchdog was not reset. The mixed results do not
establish that smaller batches improve this host's allocation behavior, so that
variant was not adopted.

A fresh compute-gated diagnostic retained the original profile and unchanged
application candidate `64a4253a`. Actual HTTP assignment/source rehearsal,
network/fingerprint checks and all 23 harness/readiness tests passed. Its new
gate required clean startup plus one direct four-token control on the same model
process before operator admission. That control used the captured synthetic
expert input and tool schemas, with no tool execution or HQ turn. Its 1,866
input and four output tokens are runtime-control usage, not application usage.
Its deliberate `length` finish cannot count as a successful agent answer.

The control passed, the operator request was admitted, and one correctly
restricted expert worker started. Its first normal model call still failed with
HTTP 500 and GPU insufficient memory. The controller stopped after 21.58 seconds.
There were two total model-call attempts: one successful short runtime control
and one failed application call. There were no successful application responses,
answers, model source reads or result workers. The node's successful-usage table
remained empty; all ledger amounts were zero. The request was cancelled and the
store stopped/disabled through the public application use cases, including an
offline cleanup path. All recorded processes, pilot listeners and live turns
were verified stopped. No other model process was changed.

This reproduces the GPU limit after a short successful control. A four-token
probe is not a sufficient readiness criterion for the normal response workload.
Further duplicate-model GPU attempts stop here. Captures are kept separately in
`.local/agent-collaboration-runtime-preflight/` and
`.local/agent-collaboration-pilot-execution-v3/`. No application source changed;
the complete passing gate for `64a4253a` is reused with fresh document checks.

## Approved next experiment: reuse the resident runtime

The evidence supports avoiding another full model load. The owner accepted the
changed experiment boundary before execution:

- Keep the candidate HQ, node, SQLite stores, credentials, conversation, grants
  and worker permissions isolated. Continue using synthetic fixture data only.
- Reuse the already running model server without restarting or reconfiguring
  it. Freeze its verified binary/model hashes, sampling settings and actual
  65,536-token context. Do not present it as the former 16,384-token condition;
  any matched comparison must use the same recorded runtime condition.
- Place a temporary authenticated loopback gateway between the isolated node
  and the existing runtime. Allow only required metadata/template/tokenizer and
  completion routes. Keep the resident server's credential outside worker
  access. Deny model-management routes and tool execution; record the connection
  allowlist and negative controls before any inference.
- Check that the resident slot is idle before admission. First replay the normal
  captured expert payload with its normal response budget, as a labelled runtime
  control that executes no proposed tool. A short four-token response is
  insufficient. Count that control inside the total model-call limit.
- Retain one operator-started diagnostic, at most eight physical workers and
  48 total calls, a 20-minute watchdog, zero paid calls and zero house actions.
  Abort on competing workload, failed runtime control or scope/policy drift.
  Never terminate or change the resident runtime during cleanup.
- Label the result accurately: application/data/capability isolation remains;
  model-process and compute-resource isolation do not. The resident runtime
  retains its current networking and shares capacity with ordinary work.
  Operator admission still cannot prove autonomous initiation.

This trades full model-process isolation for avoiding duplicate GPU allocation.
An exclusive-resource window with an identified, authorized stop/restore plan
is the alternative. At approval, no shared-runtime gateway or new inference had
started under this proposal, and no existing service had been interrupted.

Preparing this experiment exposed a separate reply-budget omission: the OpenAI
worker path sent no output-token limit, while the Messages path already sent
4,096. The worker now sends the same explicit 4,096-token limit on both paths.
A process-level regression first failed on the missing field, then passed after
the correction; the existing exact request-shape assertion was updated too.
The captured earlier expert request was uncapped. The shared-runtime control
uses that synthetic payload with the newly explicit limit, rather than claiming
the earlier request already had it. This bounds new output but does not prove
that a particular model will complete useful work within the limit.

The corrected worker passed formatting, workspace Clippy and all-feature tests,
the 30-screen console check, configured personal-value check, collection parity
and whitespace checks. The private shared-runtime harness passed 24 tests;
a permissive-gateway negative control failed the real HTTP boundary test as
expected. These are preparation results, not live collaboration evidence.

## Shared-runtime execution result

The approved diagnostic ran against `07eedb8d`, using the resident runtime's
verified 65,536-token context and unchanged process/settings. The sandboxed
gateway initially could not launch the operating system's privileged process
inspection utility. Before inference, its identity check was changed to read
the current user's process arguments directly through the operating system API.
The exact pinned argument hash and authenticated idle-slot metadata then passed
inside the existing network profile. No network permission was broadened.

Fresh source grants, actual HTTP expert/result assignment rehearsals, direct
mutation refusals, wrong-policy negative controls, exact-retry admission,
unapproved-entry refusal, connection restrictions and fingerprints passed. The
rehearsal used a separate stopped copy and an operator answer, with zero model
calls; it is distinct from the live evidence below.

The live run met the frozen bounded-execution criteria:

- One operator-started ordinary consultation and its exact retry produced one
  accepted request and no new carrier task.
- Two real workers completed successfully. Each independently read the same
  granted original `r1`; their returned original-content hashes matched.
- The expert persisted one answer citing `r1`: expected 200, observed 503,
  dependency `fixture-cache` unavailable, no repair performed. The requester
  received one result and independently confirmed these facts after its read.
- Both linked worker runs reserved against the admitted project. All amounts
  were zero. Eight successful application calls were recorded by the node:
  16,270 prompt tokens and 1,726 completion tokens. One separate runtime control
  used 1,866 prompt tokens and 66 completion tokens, proposed a tool and executed
  none. The gateway's nine successful forwards reconcile with eight application
  calls plus that control; there were no paid calls or external actions.
- The run took 177.26 seconds and two physical worker starts, within the frozen
  20-minute, eight-worker and 48-call limits. Cleanup disabled conversations,
  stopped the isolated store and disarmed execution. An additional scheduled
  downtime run was stopped without a physical worker start. No live runs, owned
  processes or pilot listeners remained. The resident process identity was
  unchanged and it remained running.

This is a bounded-execution pass with a quality caveat, not a full evaluation
pass. The requester tried `read_notebook` and `read_world`, which the application
advertised as read-only tools but the private fixture fence refused. It still
finished within four model calls. Its final response also asserted that no code
defect was implicated and predicted restoring the dependency would make the
check pass. The single diagnostic receipt does not establish either conclusion.
No repair, recovery or deployment was actually claimed as completed or executed.

Next, tighten the verification briefing's distinction between observed facts and
inference, and align the fixture's advertised read scope with its enforced scope
before freezing a quality evaluation. Preserve this result rather than rerun
until wording improves. Autonomous initiation, matched baseline comparison,
device checks and release review remain outstanding. Shared runtime capacity
also prevents treating this timing as an exclusive-resource benchmark. Production
remains on the previously deployed candidate with conversations disabled.

Private traces, final responses, accounting checks and cleanup verification are
retained in `.local/agent-collaboration-pilot-shared-runtime/`. The full passing
application gate for `07eedb8d` is reused for this prose-only result update, with
fresh personal-value, local-link and whitespace checks.

## Quality follow-up contract

The owner's continuation authorizes the next bounded follow-up under the same
accepted shared-runtime boundary. Before another inference, both actual expert
and result briefings now require separating original observations, explicitly
labelled inference and remaining unknowns. They prohibit ruling out other causes
or predicting successful repair without evidence. A rendered-handoff regression
failed before this guidance and passed afterward. This is prompt guidance, not
a deterministic guarantee of factual model output; capability policy still
authorizes every action independently.

The next private fixture gateway intersects the real application's advertised
tool list with its existing stage allowlist, preserving schemas and call-time
argument/source checks. It adds no tool and grants no access. Real HTTP tests
first exposed the advertised/refused mismatch at all three stages, then passed
after filtering. A permissive-discovery negative control failed at every stage.
Replay of the completed pilot's actual tool lists leaves only `my_turn`,
`read_assistance_source` and, for the expert, `answer_assistance`. The three
previously refused notebook/world calls are now unadvertised. The completed
pilot packet is preserved unchanged; preparation uses separate private packets.

Freeze one new operator-started quality diagnostic: same original source bytes,
pair/project, model/runtime/context, explicit 4,096-token output limit, six
rounds per worker and prior 20-minute/eight-worker/48-call envelope. Fresh stores,
grants, credentials, binaries and fingerprint/network/assignment checks precede
activation. No paid calls, external actions or production changes are permitted.
Count the runtime control separately within the total call limit.

All prior bounded-execution criteria still apply. Additionally, inspect every
claim in both final responses against the original. Required facts are the
expected 200, observed 503 and unavailable fixture dependency. An inference must
be labelled and grounded; unsupported exclusion of code defects or predicted
repair success fails quality even if the required facts are present. Missing
evidence must remain unknown. Any call to a tool absent from the intersected
stage list fails the tool-alignment criterion. Keyword matching alone cannot
award a quality pass. Retain a failed outcome without rerunning for better prose.
This follow-up cannot establish autonomous initiation, a matched baseline or
general model reliability.

Before activation, the corrected guidance passed the complete repository gate:
formatting, workspace Clippy, all-feature tests, console, configured personal-value,
collection parity and whitespace checks. The separate quality pilot harness
passed 25 tests. No inference is included in these preparation results.

## Quality activation stopped: resident runtime busy

Candidate `f59e1ecc` passed the fresh admission, HTTP assignment, scope refusal,
network/fingerprint and unapproved-entry checks. The one quality activation then
stopped after 0.36 seconds: the authenticated resident slot reported processing
other work. The gateway's readiness check refused before the runtime control or
application admission. There were zero inference attempts, zero physical workers
and zero accepted requests. This is no result on model quality, not a failed
answer and not evidence that the new guidance works.

The controller terminated only its temporary gateway, stopped/disabled the
isolated store through the offline application cleanup and disarmed execution.
There were no remaining pilot processes, listeners or live runs; the resident
runtime remained alive with unchanged identity. No immediate retry or service
preemption was attempted. Evidence is in
`.local/agent-collaboration-quality-pilot/`; offline scope replay and negative
controls are in `.local/agent-collaboration-quality-preflight/`.

The next live gate is this same frozen quality diagnostic when the resident slot
is idle, using fresh stopped stores/grants and preflight checks. Do not reuse the
stopped packet as a resumed live session. The earlier bounded-execution pass is
unchanged. Quality, autonomous initiation, matched comparison and device/release
checks remain open. This prose-only outcome update reuses the full passing gate
for `f59e1ecc`, with fresh personal-value, local-link and whitespace checks.

## Fresh quality trial: grounded answers, incomplete final finding

After the owner's continuation, the unchanged resident runtime was idle. A fresh
packet retained application candidate `f59e1ecc`, the frozen source and quality
rubric, new stores/credentials/grants and all earlier scope/stop limits. The 25
harness tests, actual HTTP assignment/admission rehearsals, network/fingerprint
checks and unapproved-entry refusal passed before activation. The stopped busy
attempt was preserved separately.

Two real workers completed in 166.13 seconds. Both independently read the same
original. One answer and one result were retained, with no new carrier task;
both linked runs reserved against the admitted project and all amounts were
zero. The node recorded eight successful application calls, 11,683 prompt tokens
and 1,631 completion tokens. One separate runtime control used 1,866 prompt and
75 completion tokens and executed no proposed tool. Nine gateway forwards
reconcile with those eight calls plus the control. There were no paid calls or
external actions. Every model-proposed tool was in its intersected stage list.

Claim-by-claim review found the expert's persisted answer accurately reported
expected 200, observed 503 and dependency unavailability. It labelled the causal
connection as an inference from co-occurrence and kept alternative causes
unknown. The requester independently read `r1` and correctly retained uncertainty
about causation, other causes and repair outcomes. Neither final response made
the earlier unsupported causal exclusion or repair prediction.

The frozen quality criterion nevertheless remains incomplete: the requester's
final finding omitted expected 200 and did not restate the check by name. Saying
the colleague's summary is accurate does not explicitly restate all required
facts. Record this as an execution pass and incomplete quality result, not a full
quality pass. Do not infer general model reliability or a causal token/time
improvement from these two differing, shared-runtime trials.

Cleanup stopped/disabled the isolated store, disarmed execution and left no live
runs, owned processes or pilot listeners. The resident runtime remained alive
with unchanged identity. Full traces and the explicit semantic review are in
`.local/agent-collaboration-quality-pilot-v2/`. No immediate model retry followed.

Offline review then confirmed that the result briefing omitted the original
question entirely. A rendered-handoff regression failed on that omission. The
briefing now includes the bounded original question using the existing untrusted
quotation wrapper. Both assistance stages also require explicitly restating key
evidence with original locators in the final finding; agreement with a peer
summary is insufficient. These are context/guidance corrections, not proof that
the next model response will be complete. The revised candidate needs fresh
live quality validation before that gate can close.

The revised handoff passed the focused regression and complete repository gate:
formatting, workspace Clippy, all-feature tests, console, configured personal-value,
collection parity and whitespace checks. Changed documents also passed ASCII and
local-link checks. There was no further inference or deployment for this offline
correction. Four validation gates remain: quality completion, autonomous
initiation, matched comparison and device/release validation.
