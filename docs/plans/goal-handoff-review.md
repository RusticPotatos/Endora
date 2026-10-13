# Goal runtime handoff review

2026-10-12. Reviewed `c220320` on `codex/foundation-gate`, including the six
planner/worker changes after checkpoint `ad41823`. AI-assisted review and tests.

The loop can continue between passes. It has not demonstrated dependable,
verified completion and delivery of a general goal. Keep the feature-expansion
freeze from `assistant-foundation-reset.md`.

## Implementation update — Phase 7 handoff (2026-10-12): bugs from real use

After the Phase 6 build was deployed, a real conversation on the node showed three
poor answers. Two were code defects, fixed in ADR 0085: a question about a city
was answered from a Home Assistant `event` entity because the standing-question
route had no notion of "another time or place", and the "what I did" record was
appended to an unrelated answer because it was keyed on words in the model's reply
(`activities`, in a pleasantry). The route now has an about-now-and-this-house gate
and the record is keyed on what the person asked. The third — the model calling a
ticketed-events skill three times instead of searching the web — was measured with a
new live harness (`tests/tool_choice_eval.rs`) and is not one cause. The node's Ollama
truncates every prompt to 2,050 tokens (46 truncations in eight hours, prompts up to
12,300 tokens); the first comparison made tool count look decisive, and a control with
truncation removed showed much of that was truncation. The real gap is the second step:
after a narrow skill returns nothing, four local models (7B to 14B) searched the web in
1 of 12 trials, and rewording the empty result did not help. The configured deep model
is offered the same tools but is not triggered by a confident wrong answer and is
refused after any third-party tool result. Web search is probably not in front at all
(only a nominated reader per MCP server is). Details and open decisions are in
ADR 0085; no code was changed for this part.

Handoff checks, run after formatting: all six required ordinary checks pass,
including **997 tests passed, 25 ignored** (the evaluation tests are ignored). The
foundation gate is unchanged at **8 passed, 0 failed, 32 unexecuted** and still blocks
release. The two code fixes are deployed as build `c220320+local-2` and are local and
uncommitted in the repository; they have not been exercised on the live node through a
signed-in chat. This corrects an earlier statement in this note: a chat model *is*
stored on the node (a single `qwen2.5:7b`).

## Implementation update — Phase 6 handoff (2026-10-12): first live evidence

This is the first live evaluation since Phases 1–5: the real pass loop against a
local `qwen2.5:14b` (five runs, two tiny objectives; not a reliability rate). It
found that the evaluation harness itself was stale — it never renewed its lease
or used the ownership-guarded runner, so any pass over 30 seconds failed at
checkpoint — and that the message people would receive was unhelpful. The harness
now mirrors the worker and prints the exact message and tool receipts. ADR 0084
records the runtime fix: a pass that retrieved nothing is not allowed to turn the
model's talk about a failure into a finding, and the notice now says the tool's
real error once, hides the loop's own bookkeeping, and words `Partial` as
"unconfirmed" rather than "failed".

Result on the live model: the refused-address objective ends `Blocked` with the
real refusal and nothing invented; the answerable objective shows its finding and
"I couldn't confirm this fully answers your request". The answerable objective
still ends `Partial`, so the evaluation's `Complete` assertion is deliberately
still red: that needs a general completion verifier, which does not exist.

Handoff checks, run after formatting: all six required ordinary checks pass,
including **992 tests passed, 25 ignored**. The foundation gate is unchanged at
**8 passed, 0 failed, 32 unexecuted** and still blocks release. Not covered by
the live run: web search, chat intake, the schedule verifier and the real worker.
Changes remain local and uncommitted; nothing was deployed.

Still open from "Next": a general completion verifier (the answerable case cannot
pass without one); catching a finding the model merely rewords; a persistent link
from a chat notice to its goal; push progress and proof that a stopped goal
dispatches nothing further; migrating the dedicated research worker. The 32
unmapped scenarios remain. Keep the feature freeze.

## Implementation update — Phase 5 handoff (2026-10-12)

ADR 0083 makes unattended goal work visible in the console and stoppable from it,
and gives finished goals a history. The chat view has a new strip: running goals
show their phase, finding count and next step with a Stop button (the existing
cancel route; the goal's key is URI-encoded because the dispatcher splits on
`:`), and the five newest ended goals expand to the same text the chat used, now
served as `outcome` in the goal JSON. `outcome_notice` covers every status, so a
cancelled goal is never described as a shortfall. The strip is fed by the data
the console already loads (`/v1/export`); there is no new endpoint, and running
work refreshes on the existing periodic poll.

Handoff checks, run after formatting: all six required ordinary checks pass,
including **984 tests passed, 25 ignored**. The foundation gate is unchanged at
**8 passed, 0 failed, 32 unexecuted** and still blocks release. The console
render check gained a goals block (every state, the Stop control, hostile text,
the recorded cancel request). It exercises the code in Node only: nothing here was
viewed in a real browser or on a phone. Changes remain local and uncommitted;
nothing was deployed.

Still open from "Next": a persistent link from a chat notice to its goal in the
console; push (not poll) progress and proof that a stopped goal dispatches nothing
further; a real-model recovery evaluation; a verifier and dedicated-worker
migration for general research. The 32 unmapped scenarios remain. Keep the
feature freeze.

## Implementation update — Phase 4 handoff (2026-10-12)

ADR 0082 closes the "partial results are never published" half of finding 2.
Partial, blocked and failed goals are now queued and delivered to chat exactly
once, with what was found, what is still missing and what did not work;
cancellation is deliberately not announced. `goal_worker_delivers_its_terminal_partial_result`
is no longer ignored and passes through the actual worker, including a check that
a later tick does not announce the outcome again. The foundation gate's R04
evidence now also requires an exhausted goal to be queued for delivery.

Handoff checks, run after formatting: all six required ordinary checks pass,
including **983 tests passed, 25 ignored** (975 / 26 before this slice). The
separately invoked foundation gate is unchanged at **8 passed, 0 failed, 32
unexecuted** and still blocks release. The delivery test lives in the node crate,
which the gate does not execute, so it does not move the gate count. Changes
remain local and uncommitted; nothing was deployed.

At that point still open: a persistent link from the notice to the stored
evidence, terminal history in the console, progress and cancellation in the
browser, a real-model recovery evaluation, and a verifier and dedicated-worker
migration for general research. Phase 5 above records what has changed since.

## Implementation update — Phase 1

Storage and background ownership repairs in ADR 0079 now address findings 3 and
4 below. Creation and checkpoint updates are separate, claims carry unique
tokens, checkpoints require current ownership, and expired budgets become
visibly blocked. The worker renews during long calls and checks ownership before
new tool dispatches. Competing connections, restart, migration, cancellation,
purge/key reuse and the actual worker heartbeat have executable tests.

The foundation gate now reports **7 passed, 1 failed, 32 unexecuted**. Its
remaining executed failure is unsupported completion from invented schedule
prose. The terminal partial-result publication test and the live general-goal
completion test also remain unresolved, as planned for subsequent phases.
The findings and measurements below record the reviewed baseline; they do not
claim these subsequent repairs were already present in `c220320`.

## Blocking findings

### Implementation update — Phase 3 handoff (2026-10-12)

ADR 0081 implements declared chat intake with immutable criteria and retry keys,
removes the sports keyword pre-route and speculative failed-answer goals, and
adds built-in Brave internet search through existing secret Skills settings.
The worker can verify supported fetched JSON-LD calendars after a primary-source
failure and delivers completed evidence directly to chat. The actual-worker
composition test demonstrates 403 → search → fetch → verified two-game result
→ one chat publication, using synthetic providers and a synthetic planner.
No live Brave request or new-model reliability evaluation was performed.

Handoff checks: all six required ordinary checks pass, including **975 tests
passed, 26 ignored**. The separately invoked foundation gate still blocks release:
**8 passed, 0 failed, 32 unexecuted**. Changes are local and uncommitted; no
deployment or pull request was created.

Next: deliver partial/blocked outcomes; expose durable progress and cancellation
in chat; add a real-model recovery evaluation; establish a verifier for general
research and migrate its dedicated worker. Keep the feature freeze. The 32
unmapped foundation scenarios remain open; this slice is not a release gate pass.

Implementation update, Phase 2: ADR 0080 replaces the formatting-based completion
check with actual tool observations and structured schedule verification. The
actual worker completes a source-backed synthetic schedule, and the invented
answer counterexample passes. All eight currently mapped foundation checks pass;
32 scenarios still lack executed coverage, so the release gate remains blocked.
General completion, alternative-source extraction, terminal delivery and browser
progress are still open. Baseline findings below are retained as review history.

1. **P1: model formatting can assert success without any source.**
   `resume_goal` passes model prose to `sports_schedule_satisfied`, which checks
   only `1.`, `2.`, `3.`, and `Source:`. A new counterexample supplies invented
   opponents, no dates, no tools and an invented source. The result is `Complete`.
   This is a deterministic orchestration defect, not an unlucky live-model score.
   Keep tool observations separate from generated conclusions; completion must
   validate the request's constraints and actual retrieved evidence.

2. **P1: general goals cannot reach verified completion, and partial results are
   never published.** `resume_goal` returns `Continue` for every non-sports
   objective until repetition or budget makes it `Partial`. The publication query
   selects only `complete`; the listing endpoint selects only active goals. The
   actual worker test settles an exhausted goal as `Partial` but receives no chat
   message on its next publication tick. This is why something can run and then
   appear to vanish. Define explicit completion criteria and deliver all terminal
   outcomes with findings, remaining gaps and a persistent result link.

3. **P1: persistence still accepts stale writes and resurrects purged work.**
   The existing foundation tests reproduce both failures. `save_goal` neither
   requires current lease ownership nor separates initial creation from updates.
   Timestamp ordering alone would not solve ownership. Require a current owner
   generation for checkpoints and publication; reject updates after deletion.

4. **P1: the reservation budget can strand active work.** The ninth reservation
   fails after eight attempts without recording a terminal reason. The model-pass
   budget does not cover this path: context failures or process interruptions can
   consume reservations without advancing the pass checkpoint. The existing
   budget-exhaustion acceptance test remains red.

5. **P2: the previous live acceptance test accepted an unfinished attempt.**
   The original two-case battery passed on the local `qwen2.5:14b` model. The
   answerable example-page request ended `Partial` after repeating its description;
   the unreachable-source case also ended `Partial`, with a question to the user
   stored as evidence. Passing this battery did not establish completion, source
   provenance, worker recovery, browser progress or chat delivery. The answerable
   case now requires `Complete`, and its scope is stated explicitly.

Background progress also remains invisible: `resume_goal` discards step and token
callbacks. This review did not measure browser timings or run a deployed-node
smoke test. The live model test calls the pass loop directly, using isolated
in-memory state and reversible capabilities, without personal accounts.

## What improved and should be preserved

- Recoverable failures now leave a resumable objective rather than ending the
  entire plan; the existing recovery test now passes.
- The goal worker obtains its turn lock before reserving an attempt.
- Background capabilities are restricted to reversible operations.
- Passes and accumulated attempts are bounded.
- Cancellation defeats late success in the tested storage path; restart preserves
  stored evidence and the existing lease; completion publication is idempotent in
  its tested transaction. These narrower checks pass.

## Reproduction

```sh
make foundation-gate
cargo test -p endora-node goal_worker_delivers_its_terminal_partial_result -- --ignored --nocapture
ENDORA_MODEL_URL=http://127.0.0.1:11434/v1 ENDORA_MODEL=qwen2.5:14b \
  cargo test -p endora-infrastructure --test goal_eval -- --ignored --nocapture --test-threads=1
```

The foundation gate reports **4 passed, 4 failed, 32 unexecuted**. Missing coverage
remains blocking; it is not included in a success percentage. The separate actual
worker delivery check fails. The new counterexample is an explicitly invoked
acceptance test, consistent with the existing foundation suite; it is not run by
the default regression command.

With the tightened success assertion, the live answerable case failed in 31.90
seconds: four passes, three descriptions of the same page, one repeated-result
dead end, final status `Partial`. Model: local `qwen2.5:14b`, digest prefix
`7cdf5a0187d5` (full runtime metadata belongs in ignored benchmark artifacts).
This is one live trial, not a reliability percentage. Source-reading assertions
in the old test inspect generated prose; a future provenance gate must also
inspect actual successful tool receipts.

All six ordinary repository checks passed after the test changes: formatting,
Clippy with warnings denied, workspace tests (**947 passed, 25 ignored**), console
rendering, personal-data scan, and diff whitespace. The deliberately invoked
acceptance failures above remain unresolved; the green ordinary suite does not
override them.

The review changes tests, the acceptance manifest and documentation only. It does
not fix these production defects or deploy a candidate. The next implementation
should be one complete slice: request → persisted objective → retrieved evidence
→ verified outcome → delivered chat result, including partial/blocked outcomes.
Prove it through the actual worker, then repeat with a failed first source, process
restart and cancellation. Do not add further capabilities until that path passes.

## Phase 8: Ask deep, the router, and what is sent (2026-10-12)

Not part of the goal runtime, recorded here because it grew out of the same live review.

- Ask deep plans with skills while the local model reads (ADR 0086); a local answer that found
  nothing, or a world question answered with no lookup, is retried once the same way (ADR 0087).
- The local model sorts a message into lookup / personal / chat instead of a word list (ADR 0089).
- The console shows what is sent and how full the window is, and a topic can end (ADR 0088);
  skills are no longer sent twice (ADR 0090).
- Deployed in steps on the NAS as build `dafd821`; the later commits on `cleanup` (dependency
  bumps, router, single skill list, input restore, docs) are not deployed.
