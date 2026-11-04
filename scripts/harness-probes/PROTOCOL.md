# Harness probes: the comparison protocol (v3)

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

## Grading (v3; v2 after round 1's false-pass review, v3 after round 3's paired run)

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

## Metering (`accounting.py`)

- Tokens: uncached input, cache reads, cache writes and output, each separately, summed once.
  Every model call counts, including continuations, retries, failed runs and helpers. A call
  repeated across trace lines counts once. Unknown usage stays unknown (`null`), never zero,
  and marks the total incomplete.
- Cash: normalized with the one frozen tariff ([tariff.json](tariff.json), v1); the actual
  charge (Claude Code's `total_cost_usd`, HQ's ledger charges) is reported beside it.
- Time: from dispatch to the terminal outcome, including model latency, local tools, harness
  overhead and recovery. Reported apart: agent-active time (the union of model and tool
  intervals), **harness waiting** (HQ's queue between ticks and its retry backoff: the
  harness's own time, inside elapsed and never an excuse) and **external waiting** (time spent
  on the world: checks, remote systems, injected waits). An attempt whose external waiting is
  over 10% of elapsed is contaminated: it cannot establish parity and stays in the ledger.
  External waiting is never agent-active time, even inside a worker process.
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
