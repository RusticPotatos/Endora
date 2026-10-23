# Agent collaboration: larger-context pilot follow-up

2026-10-22, owner's local date. AI-assisted work on `codex/agent-collaboration`.
The owner authorized one follow-up diagnostic with 16,384 context tokens and the
corrected worker. Application candidate: `4735fdc1`. The previous
[8,192-token failure](agent-collaboration-pilot-result-2026-10-22.md) remains a
separate retained result. No production deployment or enablement occurred.

## Verdict: blocked by the experiment guard

The larger window avoided truncation. The experiment still did not establish a
live assistance result: its private guard rejected the question and conversation
discovery before either reached Endora. This is a pilot-harness defect, not
evidence that the application refused the consultation.

| Measured item | Observed result |
|---|---|
| Physical workers | 1, initial requester only |
| Local model calls | 5 |
| Node-recorded prompt/completion tokens | 46,291 / 3,782 |
| Provider-reported cached input tokens | 36,527; distinct from the node's cache counter |
| Truncated replies | 0 |
| Minimum space after the 4,096-token response reserve and margin | 2,508 tokens |
| Accepted requests / answers | 0 / 0 |
| Expert/result model turns and original-source reads | 0 |
| Paid charges | 0 microdollars |
| Session elapsed | 393.0 seconds |
| Final scope | Conversations disabled, global stop persisted, envelope unarmed |
| Final processes | Recorded pilot processes terminated; all pilot listeners closed |

Every model request was measured before forwarding through the actual node
relay. The existing private protocol fence also handled model forwarding, using
the pinned runtime's non-inference template/tokenizer endpoints, a 4,096-token
response reserve and a 32-token bookkeeping margin. It forwarded the original
request body, leaving the node's authentication, local-model selection and usage
recording in the path. The runtime documents these
[template and tokenizer endpoints](https://github.com/ggml-org/llama.cpp/blob/620d2225c/tools/server/README.md#post-apply-template-apply-chat-template-to-a-conversation).
Three budget tests and an HTTP forwarding/call-cap test passed before activation;
the permissive budget stub failed the initial tests. The headroom check is a
preflight reserve, not a guarantee that generation can never exhaust context.

All five measured requests fit. The node usage rows, private model-response
audit and five processed model tasks reconcile. Responses ended with four
`tool_calls` finishes and one `stop`, with no `length` finish or runtime
truncation. The first measured prompt was 8,016 tokens. During worker execution,
sampled memory availability ranged from 18 to 23 percent, above the pilot's
10-percent stop threshold. This is sampled availability, not a peak-memory claim.

## What the guard got wrong

The model reread its briefing, proposed an office change, tried `consult`, tried
`read_messages`, then ended with a report that the diagnostic was blocked.
The office proposal was correctly refused; no office change occurred.

The consultation proposed the selected expert, conversation, granted `r1`
evidence and stable `pilot_diag_once` key. Its question was:

```text
Explain why health-check-v1 fails.
```

The guard demanded that the question also copy the remaining sentences of the
fixture objective, including the citation and no-repair instructions. It
therefore refused the shorter question before application admission. The guard
also refused `read_messages` with empty arguments, although that is the documented
application operation for discovering authorized conversations. This isolated
fixture contains only the selected scope. These restrictions were tighter than
the intended permissions and prevented the model from exercising the integrated
application path. The active run's guard was not changed after seeing the output.

The model's final report was not used as the acceptance checker. The captured
model proposals, generated tool-refusal transcript, protocol audit, empty request
table and saved controller result supply the evidence. No expert answer,
independent requester reading or expert/result project attribution was exercised.
Those gates remain unverified. A completed requester turn is not a successful
diagnostic.

## Correction and replay against the application

A separate guard revision now permits ordinary discovery and two explicit
phrasings of this fixed question: the complete frozen objective or its first
sentence. It does not accept arbitrary topics or use a model to decide scope.
The selected expert, conversation, evidence, read-only stage and exact retry
binding remain enforced. After the first admission intent is reserved, changed
payloads still cannot create another request. The corrected fixture is checked
to contain exactly the one authorized conversation before model dispatch.

The two new guard tests were observed failing, then passed with the correction.
All nine guard tests pass, including different-topic, wrong-recipient/scope,
unrelated-source, changed-retry and recursive-request refusals.

The captured consultation and empty-argument discovery call were also replayed
through Endora's actual public application/protocol use cases on an offline copy
of the original stopped preparation store. No source, identity, classification
or receipt was fabricated with SQL. The application returned its single
authorized conversation, accepted the captured question as queued and returned
the same request on an exact retry. Only one request existed and no carrier task
was created. The admission-only copy held the expert paused. A further offline
replay then exercised the whole application sequence through the real runner
adapter: the initial turn ended, the expert was unpaused, the assigned expert read
the original receipt and an explicitly operator-authored answer was submitted.
The linked requester-result turn read the same original. Exactly one answer and
one notified result remained, and both turns' real zero-amount reservation rows
retained the selected project. No worker or model was started; the operator,
not an AI agent, drove these protocol calls. This verifies source access and
orchestration in the application, not independent model diagnosis, model usage
charging or live worker behavior. Both offline copies were stopped and disabled.
The replay checker uses the adapter's actual `state` reply field, after an
initial checker incorrectly expected `status`. The failed checker copy and the
passing replay were retained separately. This replay demonstrates admission
compatibility and the operator-driven application sequence, not live inference
or an independently checked model result.

Private evidence is retained in `.local/agent-collaboration-pilot-next/` and
`.local/agent-collaboration-pilot-guard-review/`: raw audited responses, the
transcript, prompt measurements, usage/ledger reconciliation, stopped stores,
red/green guard tests and the actual application replay. Credentials, machine
paths and raw records remain ignored. The unrelated baseline-collector edit was
preserved and not used as a checker.

## Corrected packet preparation

A separate packet is prepared at `.local/agent-collaboration-pilot-corrected/`.
It retains application/model/runtime identities and the 16,384-token setting;
the correction is confined to the experiment guard. Its store comes from the
original preparation snapshot, while both attempted runs remain intact. Node
storage and credentials are fresh. The actual services verified the same
project/pair, unique conversation and original-source grant, then stopped.

Nine guard tests, three model-budget tests, the HTTP forwarding test, four
shell-boundary tests, three readiness tests, dispatcher admission checks,
activation-store checks and network/fingerprint checks passed for the prepared
packet. The application source is unchanged. The full repository gate was rerun
and passed: formatting, workspace Clippy with denied warnings, workspace
all-feature tests, console checks, personal-value checks, collection parity and
whitespace checks. It included 923 team tests, 162 behavior scenarios with 2,055
steps and 33 worker process tests. Existing explicitly ignored tests remained
ignored. No new physical-device validation occurred.

At preparation, the corrected packet was stopped and unarmed, with zero new
requests, live turns, worker starts or model calls. No source access or capability
was broadened. The owner subsequently authorized its one execution with "Yes run
it", retaining one diagnostic request, eight worker starts at most, 48 model
calls at most, a 20-minute watchdog, zero paid calls and the same stop conditions.

## Corrected-guard execution: no consultation proposed

The authorized rerun completed in 147.3 seconds with one physical requester
worker and two local model calls. The controller exited nonzero because the
requester ended without an accepted consultation. The worker itself exited zero;
that is a completed turn, not a passing diagnostic. No expert or result worker
started, no original source was read and no answer was produced.

| Measured item | Observed result |
|---|---|
| Node-recorded prompt/completion tokens | 17,371 / 470 across two calls |
| Provider-reported cached input | 8,013 tokens; node cache counter remains zero |
| Response finishes | `tool_calls`, then `stop`; no truncation |
| Minimum space after response reserve and margin | 2,902 tokens |
| Sampled memory availability | 21 to 23 percent |
| Forwarded tool calls | `my_turn` twice |
| Guard refusals / consultation proposals | 0 / 0 |
| Accepted requests / answers / source reads | 0 / 0 / 0 |
| Paid charges | 0 microdollars |
| Final scope | Conversations disabled, global stop persisted, envelope unarmed |
| Final processes | All pilot listeners closed; no live runs remain |

The actual briefing included both owner-authored notebook instructions: submit
the initial consultation and independently read `r1` on the later result turn.
It also explicitly called this a downtime turn, said nothing was due, described
office recreation and said leaving the office unchanged was a good turn before
stopping. Its final tools summary omitted consultation even though the earlier
colleague guidance described it and the advertised tool schema included it.
The private activation helper had deliberately selected `WakeReason::Downtime`;
it did not queue an explicit owner assignment. The model reread the briefing,
then called the notebook instructions already served and stopped. That claim
is contradicted by the fresh packet's empty request table; it is not evidence
of prior execution.

This result establishes an unsuccessful initiating turn. Conflicting briefing
instructions are a plausible contributor, not a proven causal explanation of
the model's decision. Neither a guard rejection nor an application admission
failure occurred, because no consultation was proposed. The corrected guard's
live consultation path therefore remains unexercised. No prompt, guard or
objective was changed during execution, and no extra requester was started.

All three live diagnostic outcomes are retained separately. This series now
stops for an offline review under the standards' three-attempt rule. The next
review must reconcile the initiating wake's purpose, owner objective, actual
briefing and advertised tools before proposing any further live attempt. Any
behavioral correction requires a failing regression first; do not simply change
the model's question or repeat inference until it happens to consult. Live
end-to-end validation, matched comparison, device checks and production release
remain incomplete. Production was not changed.

The corrected packet retains raw protocol/model responses, measured prompts,
usage rows, the stopped stores, controller failure and a separate evaluated
result in `.local/agent-collaboration-pilot-corrected/`. Private records and
credentials remain ignored. No application source changed in this execution.

The 20 private harness tests and refreshed network/fingerprint preflight passed
before activation. After recording the result, the required repository gate
passed again: formatting, workspace Clippy with denied warnings, workspace
all-feature tests, console checks, personal-value checks, collection parity and
whitespace checks. No terminal-client source changed or new device check ran.

## Offline briefing correction after the three-attempt stop

The regression drives the actual engine, runner briefing and authenticated tool
interface for an ordinary taskless downtime turn. It first failed because the
final tool summary omitted authorized conversation operations. After adding the
summary, it failed on the remaining "nothing is due" wording. The correction
lists the existing tools for eligible turns and narrows the claim to "no task is
due". The test passes with the real advertised tools, preserves task-creation
refusal and checks that disabled or absent scopes do not gain the summary. The
existing owner scenario now checks this initial briefing before ordinary
consultation proceeds through its public use cases.

The evidence settles a presentation defect, not model causality. ADR 0119 makes
downtime optional activity; ADR 0118 defines retained notebook notes, not a new
current assignment. The pilot used that optional wake and put the initial and
later-result instructions into the same notebook. The model still has no
deterministic obligation to initiate the requested diagnostic. Do not replace
that gap with a new assignment API or infer completed work from note age.

The next evaluation should separate initiation from execution: first verify the
expert/result workers and independent original reads using a clearly labelled
operator-initiated request, then evaluate whether an ordinary member initiates
the same question from its real context. The first result cannot satisfy the
second criterion. This changes the experiment approach after the three-attempt
stop; it does not change the original end-to-end acceptance gate. A new live
packet must freeze that distinction, candidate and budgets before activation.
No fourth live attempt, deployment or source-grant expansion was performed in
this offline correction.

The focused regression passed after the two observed failures. The final
implementation passed formatting, workspace Clippy with denied warnings,
workspace all-feature tests (including the updated owner scenario), console
checks, personal-value checks, collection parity and whitespace checks. Existing
ignored tests remained ignored. No terminal-client source changed and no new
device validation ran. The unrelated baseline script edit remains excluded.
