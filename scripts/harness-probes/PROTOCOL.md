# Harness probes: the comparison protocol (v6)

How Endora's agent harness and Claude Code are compared on fresh local work, set by the
climb's researcher in round 0 and built in round 1. This file is frozen with the generator,
graders and parsers: its SHA-256 is in the round's report, and a change to it is a new version
reported as such, never an edit in place.

## What is compared

Ten probe families (`probes.py families`), each a small repository plus task words, generated
from a seed:

| Family | Deterministic success criterion |
|---|---|
| `failing_test` | Fix the cause; original and hidden regression tests pass |
| `five_file_rename` | All references updated; behaviour preserved; unrelated files unchanged |
| `needle` | Locate the seeded defect and fix it; decoys unchanged |
| `tool_failure` | Recover from one injected transient failure; correct final state; no duplicated side effect |
| `interrupted` | Resume after a confirmed intermediate edit; preserve completed work and finish |
| `large_output` | Recover the answer buried in the middle of ~50 KB of output |
| `already_satisfied` | Verify the requirement and stop; no repository changes |
| `unambiguous` | Complete the specified change without asking the owner |
| `impossible` | Identify the planted contradiction, report blocked truthfully, preserve forbidden files |
| `three_subtasks` | Complete all three; execution strategy recorded, delegation not required |

Every task's words end with the report convention in [SCHEMA.md](SCHEMA.md) (`RESULT:`,
`EVIDENCE:`, `ANSWER:`), identical in both arms. Since v3 (round 3's paired run: both arms did
correct, identical work and reached their native ends, and both missed the exact lines) the
convention is diagnostic only: an attempt passes on what it did, not on how it phrased it.

## Seeds and exposure

- Seed 0 is the exposed **A** variant of each family; seeds 1–3 are exposed development seeds
  used by the controls. All four are development data.
- **B** variants use seeds the researcher picks after the freeze, never generated before the
  confirmation batch. Any exposure is disclosed.
- Three fresh B instances per family per arm: 30 attempts each. B results never feed another
  candidate; a later confirmation needs fresh seeds.

## Running an attempt (identical in both arms)

- Same task words and starting tree; the exact same model revision and inference settings;
  each harness's own instructions and tools kept and captured; fresh state per attempt.
- Arm order randomized per pair, run sequentially; a ten-minute timeout per attempt; equal
  total inference allowance across continuations. Failures and timeouts keep their full cost.
- The runner may act only as the family says: `interrupted` stops the attempt at the first
  change to `interrupt_after` and resumes it with `resume_words`; nothing else intervenes. An
  owner answer to an unexpected question is an intervention and is counted.
- Endora runs through its worker and HQ (`endora_local.py` is the local, scripted form of this
  path); Claude Code runs headless. Each arm's raw traces are kept whole.

## Grading (v6; v2 after round 1's false-pass review, v3 after round 3's paired run, v4 after round 13's five families, v5 after round 19's development seed 4, v6 after the defect remediation's review of round 23)

`probes.py grade` regenerates the hidden expectations from (family, seed) in its own temporary
folder; nothing hidden is ever written into the attempt's tree. It judges a rebuilt copy: the
regenerated start files, plus only the agent's versions of the files it may change, plus the
hidden tests. It checks forbidden edits by content against the regenerated start.

- **Isolation.** Every execution of candidate-controlled code (the hidden tests, which import
  the agent's modules, and the re-run of its evidence command) runs in a subprocess under
  macOS `sandbox-exec`: writes only inside a disposable tree, no reading the grader's folder or
  the home folder, no network but the publish service's loopback port, a scrubbed environment
  and a time limit. Without the sandbox the grader refuses to grade.
- **Evidence is witnessed, not named.** The `EVIDENCE:` command must appear in the trace as a
  successful tool call, and the grader's own sandboxed re-run of it must show the family's
  witness: a per-run nonce from a witness test plus every visible test and `OK` (test
  families); the exact answer line from a real run of the report (large_output); the
  expected version in the publish service's status (tool_failure); the failing assertion
  with both values (impossible). A command that merely mentions the right words does not count.
- **Side effects are observed outside the agent's write authority.** The `tool_failure` tool
  talks to a loopback publish service (`sidefx.py`) the runner starts, holding its records in
  its own memory behind a token the agent never has. The grader reads those observations;
  without them the criterion is unknown and fails. A forged or cleaned-up local record changes
  nothing.
- **The right contradiction.** `impossible` passes only when the report names the forbidden
  file, both conflicting values, and a conflicting test.
- **Artifact and completion are separate.** `artifact_pass` is the work being correct.
  `completion.verified` is true only when the harness reached its own final state (Claude
  Code's session ended normally; HQ's task done, or blocked for `impossible`), and null when the
  work was only submitted to a gate (HQ's `in_review`). A native end whose criterion fails is a
  **false completion**.
- **Verification observed in the trace (v3).** A tool call must have run the family's
  verification after the last edit (an edit tool, a checkout, or a shell segment that may write;
  segments are split inside a command, heredocs kept with their opener), and its captured output
  must show success from that run (`Ran N tests` with N at least the visible count, and `OK`).
  An exit code alone is not success; masked failures and summaries an `echo` could have printed
  are rejected. Missing or ambiguous traces leave verification unobserved.
- **v3 pass** = artifact correct, completion verified, and verification observed. The report
  lines are kept as diagnostics; `large_output`'s value must still appear in the final words.
  `--protocol v2` reproduces the v2 verdicts.
- **v4 (the default from round 14 to round 19): every requirement traces to the task's words.** Round 13
  failed correct work on rules the task never stated, so v4 keeps v3's structure and changes
  only these, each the same for both arms and each held by a negative control:
  - **Unknown is a verdict.** A case the grader cannot resolve (a command after the
    verification whose writes it cannot tell) makes verification and `pass` null, not false;
    the CLI exits 2.
  - **large_output.** The last `ANSWER:` line decides when there is one; otherwise the report
    must assert the value, and may mention a decoy only in a sentence that rejects it. The
    answer may be read from a file the agent saved, when the output of a call that ran
    `tools/report` announced that file (provenance); a file with no such origin does not count.
  - **impossible.** A conflicting test may be named by its file or its method. The
    implementation file (`data["permitted"]`) may change; that is recorded
    (`info.unnecessary_permitted_edit`), never failed. The config and the tests stay forbidden.
    The run shows the conflict when one of the two conflicting tests fails with both values, in
    either order (the spec test on the pinned value, or the pin test once the spec's value is in).
  - **Assertion scripts verify.** In the test families, a `python3 -c` or heredoc script counts
    as verification beside a suite run when it calls the changed function(s) under `assert`
    (`data["verify_calls"]`: any-of groups, every group covered; `from m import f as t` and
    `t = f` make `t(...)` a call of `f`), exits 0 with no traceback, and is visibly read-only
    (its Python read with `ast`).
  - **Native verification is reported, not required.** `completion.native_verification` is
    HQ's own note (`verified` for "HQ's checks passed", `not_verified` for "completed, not
    verified") or null; it never decides a pass.
  `--protocol v3` and `--protocol v2` reproduce the older verdicts.
- **v6 (the default since the defect remediation, 2026-11-05): one rule for test additions.**
  The round-23 confirmation failed an `unambiguous` attempt for adding a regression test, while
  v5 allowed the same addition in `needle` alone; `already_satisfied`'s words even ask for a test.
  v6 keeps v5 and changes only this: in every family whose words do not forbid touching the tests
  or the repository (`probes.TEST_ADDITIONS_ALLOWED`: five_file_rename, needle, interrupted,
  already_satisfied, unambiguous, three_subtasks) a pure test addition is allowed, by v5's needle
  rule (a new file under `tests/`, or an existing test file with additions only). The others'
  words forbid it (failing_test "Do not change the tests", impossible "Do not edit any test",
  tool_failure and large_output "Do not modify … anything") and any test change still fails
  `no_forbidden_edits`; a changed or weakened existing test fails everywhere. The task words are
  unchanged (contract 2). Controls in every family: `test_file_added`, `test_appended`,
  `test_weakened`; `test_probes.py` holds v6's changes from v5 to exactly the declared ones.
  `--protocol v5` (and older) reproduce the earlier verdicts bit for bit; round 23's frozen
  verdicts stand as graded.
- **v5 (the default from round 20 to the remediation): the words, the edit rules and the evidence say what they
  mean.** A batch on development seed 4 showed three requirements the grader held that the task
  never stated or the rules never allowed for. v5 keeps v4 and changes only these, each the same
  for both arms and each held by a negative control; `--protocol v4` (and v3, v2) reproduce the
  older verdicts bit for bit.
  - **Task contracts.** A task's words carry a contract version: `generate --contract N` (default
    the latest, 2) writes it to `task.json`, and `grade --contract N` defaults to the one recorded
    beside the repo (a `task.json` that predates contracts is contract 1; no `task.json` means the
    latest). Contract 1 is the original words, byte for byte. Contract 2 adds one sentence to the
    needle's words that states the intended output (for the percent family: "Percentages are
    shown with one decimal place at most, rounded (so 0.0415 shows as 4.2%)"); every other
    family reads the same under both. Under contract 1 the needle symptom check counts only the
    values the words name; the checks whose expected text depends on what the words leave
    unstated are listed as `info.unsupported_checks` (values and expected text) and never fail
    an attempt. Under contract 2 every value is checked.
  - **Needle edit rules.** Decoy helpers unchanged, existing tests preserved, relevant regression
    additions permitted. Allowed: the implementation file; new files under `tests/`; additions to
    an existing file under `tests/` that keep every original line in order and, read as Python,
    every original class, method and statement unchanged (new methods, classes and imports are
    additions; a new definition that would shadow an old one is not). Anything else, a decoy or
    any other non-test file included, and any deleted, modified or weakened existing test, fails
    `no_forbidden_edits`. The criterion still runs on the sanitized tree, so an added test never
    helps or hurts the verdict by itself.
  - **Unknown when the test count is not shown.** A test-run command whose output shows success
    (`OK`, no failure) but not how many tests ran (a `| tail -1` that leaves only `OK` lines, one
    run per test file each ending in `OK`) makes `verification_observed` unknown: `pass` is null
    and the CLI exits 2, with the reason "the output does not show how many tests ran". A run in
    a shell loop body (`for ...; do python3 -m unittest f; done`) counts as a test run. Still
    false: output showing FAILED or an error, a summary the command itself could have echoed, and
    a shown count below the suite's. A run that shows a sufficient count still passes.

## Metering (`accounting.py`)

- Tokens: uncached input, cache reads, cache writes and output, each separately, summed once.
  Every model call counts, including continuations, retries, failed runs and helpers. A call
  repeated across trace lines counts once. Unknown usage stays unknown (`null`), never zero,
  and marks the total incomplete.
- Cash: normalized with the one frozen tariff ([tariff.json](tariff.json), v1); the actual
  charge (Claude Code's `total_cost_usd`, HQ's ledger charges) is reported beside it.
- Time: from dispatch to the terminal outcome, including model latency, local tools, harness
  overhead and recovery. Reported apart, as exclusive parts that add up to elapsed (within
  1e-6): `agent_active_s` (the union of model and tool intervals), `harness_wait_s` (HQ's
  admission, its recovery after an interrupted turn and its retry backoff: the harness's own
  time, inside elapsed and never an excuse), `agent_requested_wait_s` (below),
  `external_wait_s` (time spent on the world: checks, remote systems, injected waits) and
  `unaccounted_s` (what no event explains). An attempt whose external waiting is over 10% of
  elapsed is contaminated: it cannot establish parity and stays in the ledger. External
  waiting is never agent-active time, even inside a worker process.
- HQ's scheduling waits are read per attempt from its event sequence, never from a run row: a
  retried turn REUSES its row (`attempts` 1 to 2, `started_at` overwritten), so the row's
  `queued_at` to `started_at` spans the first attempt (round 21: 13.32 s reported for 1.97 s
  real). Initial admission is the item's `task.created` to its first `run.started`. A recovery
  is a `run.ended` with result `retry_waiting` to that run's next `run.started` (`wait_kind`
  `recovery`), split at the first news (an owner reply folded, a wake queued, or the retry
  deadline) into `news_or_deadline` and `admission`; settlement, the runner's report reaching
  HQ, is not observable in the store, so a recovery starts at `run.ended`. `meta.hq_waits`
  records each recovery's boundaries and whether the report or the reply came first
  (`report_first`, `reply_first`). A retry with no later `run.started` is left unaccounted. Only
  a store with no such events for a run falls back to the row's own span, and says so
  (`meta.hq_waits.source`, `rows_fallback_runs`).
- Agent-requested waits: a `sleep N` (or `time.sleep(N)` in `python3 -c`) the agent's own
  command executes stays inside elapsed, is never subtracted and never external. It is its own
  part, `agent_requested_wait_s` (`wait_kind` `agent_sleep`), only with execution evidence: the
  sleep is actually run in command order (not inside a string, comment or heredoc, not after
  an `exit`, not backgrounded; behind `&&`, `||` or an `if`, only when the call lasted at least
  N s) and the call's own `t_start`/`t_end` span covers at least N s. The part is
  min(N, the call's span), never overlapping harness or external waits. With no timing on the
  call (Endora's transcript tool calls carry none) nothing is attributed and
  `time.agent_requested_wait_notes` says so.
- A failed HTTP call with no usage stays unknown, never zero: totals with one are reported as
  known-cost subtotals, not complete totals.
- The tariff (v1) is the published one. Endora's relay prices Sonnet 5.5 cache reads at half
  the published rate ($0.10 against $0.20 a million), so HQ's actual charges sit below the
  normalized figure; normalized dollars are the comparison.
- Model calls are counted apart from usage records: live HQ keeps one usage total a run, while
  its transcript shows each reply; Claude Code's helper calls, seen only in its result's
  `modelUsage`, are model calls too.
- Cache writes whose TTL the trace does not say (HQ's) are priced at both TTLs: a normalized
  range, never a guess.
- Endora's timeline comes from HQ's event log: dispatch is the item's `task.created`, the end is
  its first move to done or blocked (a worker's process may outlive it), and HQ's dispatch tick
  and the runner's pickup are harness waiting. Elapsed is the comparable speed; a worker's
  process lifetime and Claude Code's interval union are labelled apart and not compared.
- Model identity (rounds 7 and 8): the model each call was answered by, as the provider named
  it, kept apart from the alias asked for. An identity is complete only when its names account
  for every call an independent census saw: for Endora, the relay's own request log (the rig's;
  live HQ has no independent census yet, so live completeness stays unproven), every answered
  request named exactly once and failed requests needing none; for Claude Code, the result's
  `modelUsage`, the billing summary by model, every billed model named and every named model
  billed. A lost or duplicated report, an unknown name, or no census leaves it incomplete.
  Two arms **match** only when both are complete and both ran on the same single model,
  helpers included; more than one model is **mixed**, ineligible for this comparison. A
  requested alias is never evidence.
- Handoffs: ownership transfers (another member taking the task, a handover note).
  Fresh sessions and continuations are reported apart.
- Tool calls: total, failed (an error or a non-zero exit), refused, and wasted — wasted only
  where the trace says so (an error result, a refusal, a call after the terminal outcome).

## Targets (round 0, against contemporaneous Claude Code, on the confirmation batch)

| Metric | Target |
|---|---|
| Correct outcomes | Endora 30/30, truthful blocked outcomes included |
| False completion / forbidden changes | Zero |
| Cash | Endora's normalized total ≤ Claude Code's |
| Tokens | Endora's total ≤ Claude Code's |
| Completion time | Median paired ratio ≤ 1.25; no family median > 2 |
| Handoffs | No more than Claude Code, family by family |
| Owner interventions | Zero; unexpected questions counted |
| Waiting contamination | External waiting ≤ 10% of elapsed in each attempt |

## Freezing

Before any candidate tuning, the files below are hashed with `shasum -a 256` and the hashes
reported; the confirmation batch runs only against those hashes:

`probes.py`, `controls.py`, `accounting.py`, `tariff.json`, `SCHEMA.md`, `PROTOCOL.md`,
`endora_local.py`, `endora_live.py`, `cc_local.py`, `rig.py`, `sidefx.py`.

The task contracts are frozen with the graders, in the same manifest (since v6): `python3
freeze.py --contract N --seeds S…` writes every family's task words, allowed files and test-addition
rule for those seeds to `contracts-cN.json`, hashes it together with the files above, and prints
one combined hash. A confirmation runs only against a manifest whose combined hash was reported
before it, with seeds the researcher picks after the freeze.
