# Agent collaboration: isolated diagnostic result

Later status: the owner authorized the 16,384-token follow-up. It avoided
truncation but was blocked by an overly strict experiment guard before
application admission. See the [follow-up result and correction](agent-collaboration-pilot-followup-2026-10-22.md).
The 8,192-token run described below remains a separate failed result.

2026-10-22. AI-assisted work on `codex/agent-collaboration`. The owner explicitly
authorized the single diagnostic in the [pilot packet](agent-collaboration-live-pilot-2026-10-22.md).
Application candidate: `6ff6341a`; fixed local model/runtime and 8,192-token
context as recorded in that packet. The later worker correction below was not
part of this run. No production deployment or enablement occurred.

## Verdict: failed before request acceptance

| Measured item | Observed result |
|---|---|
| Physical workers started | 1, initial requester only |
| Model inference calls | 1, through the actual isolated node's local relay |
| Paid charges | 0 microdollars; no paid provider configured |
| Node-recorded prompt/completion tokens | 8,017 / 173 |
| Model server's stop | 8,191 retained tokens, `truncated = 1` |
| Accepted assistance requests / answers | 0 / 0 |
| Expert / requester-result model turns | 0 / 0 |
| Elapsed session, including launch checks | 349.9 seconds |
| Final scope | Conversations disabled, global stop persisted, envelope unarmed |
| Final services | All recorded pilot processes terminated; pilot listeners closed |

The frozen context left only 175 tokens beyond the prompt. The model server
reported truncation; the worker's retained transcript contains an empty visible
assistant response and no tool call. Node usage records reconcile with the
single processed model task. No consultation was accepted, so there was no
expert answer, original-source read by an expert, independent requester check
or expert/result project charge to evaluate. These predicates did not pass.
Zero paid charges do not establish project attribution for turns that never ran.

The original synthetic receipt was rechecked against the exact fixture; it was
not modified or replaced. The transcript was not cut by the history store. The
raw completion JSON was not retained, so the exact finish-reason field of this
live response cannot be independently inspected; runtime truncation and the
node's token counts are the direct evidence for context exhaustion.

The owner notes and ordinary consultation guidance were present. The chosen
taskless downtime briefing also contains office instructions. This is a further
compatibility concern, not an established explanation of what the model would
have done with adequate context. No office action or unrelated tool was run.

## Launch attempts and containment

There were three model-process loads, not three inference calls. Two launch
checks stopped before any worker or inference call:

1. The private controller expected a `status` health field, while actual HQ uses
   `ok` and `ticking`. Cleanup disabled conversations, persisted stop and
   terminated the recorded services. Three readiness tests now distinguish the
   native HQ/model shapes from loading or a failed tick.
2. HQ advertised shell eligibility for the initial unscoped taskless turn. The
   original supervisor refused it. Inspection of `may_use_a_shell` and the
   pinned worker established that this is eligibility for a workbench, not an
   allocated shell: the worker adds a shell only with `ENDORA_WORKBENCH=1`.
   The supervisor now checks the exact cleared launch environment. It accepts
   that initial eligibility only for the pinned standard worker with no
   workbench variables; linked expert/result turns must still advertise no
   shell. Four launch-boundary tests and the actual worker's existing
   workbench-only test passed. Global application guards were unchanged.

Both attempts and their stopped stores/logs were retained. The original
20-minute watchdog was not reset. The third launch reached the single inference
call described above, then stopped on the missing accepted request.

Actual HQ also scheduled an ordinary downtime turn for the expert after the
requester finished. It was stopped without being dispatched to a worker; it was
not a linked assistance expert. Failed launch/requester records and this
undispatched turn remain in the store, with zero-amount reservations. Physical
worker/model counts must not be inferred from the number of run rows.

The network/profile checks and their permissive negative control passed before
activation. Workers could reach only the private protocol fence and isolated
node, never HQ/model directly or an external destination. No workbench, shell,
external contract or paid credential was supplied. The protocol fence remained
in place. After cleanup, read-only checks confirmed SQLite integrity, no
assistance requests, no nonzero ledger rows, disabled conversations, global stop
and closed pilot listeners. Production and the unrelated baseline collector
edit were untouched.

Private evidence remains in `.local/agent-collaboration-pilot/`: the immutable
application/model fingerprints, both launch-failure archives, pre-live store
copies, worker/runtime/controller logs, retained transcript, usage/ledger
reconciliation and `evaluated-result.json`. Raw credentials and machine paths
are not included in this public record.

## Worker correction after the failed diagnostic

The live requester exited zero and was recorded as succeeded despite producing
no usable response. The worker did not examine provider token-limit stop
metadata. This was reproduced with process tests before changing behavior:
an empty local `length` reply, a local `length` reply containing a complete tool
call, and a Messages `max_tokens` reply containing a tool call all previously
returned success. The provider tests use local fake relays, not paid services.

The worker now retains the proposed reply through its existing transcript path,
then fails with an explicit token-limit reason before posting a final answer or
executing any call from that incomplete reply. Transcript submission can still
fail and is reported by the existing path; retention is not promised during a
node outage. Calls already completed in earlier rounds are not undone. Normal
complete responses, missing optional stop metadata and round limits retain
their existing behavior. The failed pilot's original status/evidence was not
rewritten, and the corrected worker has not been deployed or run in a new pilot.

The complete repository gate passed: formatting, workspace Clippy with denied
warnings, workspace all-feature tests, console checks, personal-value checks,
collection parity and whitespace checks. The worker process suite passed 33
tests, including the three new regressions and transcript inspection. The team
suite passed 923 tests and its behavior harness passed 162 scenarios and 2,055
steps. Existing explicitly ignored tests remained ignored. No terminal-client
source changed and no new physical-device validation was performed.

## Next controlled run

A separate stopped packet is now prepared privately at
`.local/agent-collaboration-pilot-next/`, retaining this failed run in its
original directory. It pins corrected application candidate `4735fdc1`, rebuilt
with the locked three-binary build command. It uses a consistent copy of the
original pre-live HQ preparation store, not a rewritten failed-run database;
the original receipt is carried forward with its operator attribution. Node
storage and credentials are fresh. The actual services reopened that scope,
renewed only the same original-source grant and then stopped. No model was
loaded for this proposed packet.

The activation-store check, seven protocol-fence tests, four shell-boundary
tests, three readiness tests and connection/fingerprint checks passed. A test
listener initially encountered socket teardown state after service verification;
after proving no listener was active, the test listener used address reuse and
the full connection matrix passed. The actual controller also refused the
unapproved packet with a nonzero exit before starting any service, model or
worker. Final checks found zero requests, live turns, model usage or nonzero
ledger rows. The packet remains stopped and unarmed, with approval unset.

The proposed runtime change is a context increase from 8,192 to 16,384 tokens
on the same model/runtime, with the same source, pair, objective,
network/protocol fence, zero paid calls, one request, eight worker-start maximum,
48-call maximum and 20-minute watchdog. Before inference, check the actual
rendered prompt and output headroom; do not trim the tool list or change
authorization to manufacture a pass. Check memory headroom again after loading.

The larger window is a proposed remedy supported by this measurement, not a
claim that the model will complete the diagnostic. A new run needs explicit
authorization because the one authorized diagnostic is finished and its frozen
runtime setting would change. Matched comparison, relevant devices and production
release gates remain unrun. The foundation's live validation is incomplete.
