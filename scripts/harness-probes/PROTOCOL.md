# Harness probes: the comparison protocol (v1)

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
`EVIDENCE:`, `ANSWER:`), identical in both arms.

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

## Grading

`probes.py grade` regenerates the hidden expectations from (family, seed) in its own temporary
folder (nothing hidden is ever written into the attempt's tree), runs hidden tests on a copy
of the final repository, checks the forbidden set against the `start` commit, and checks the
evidence: the `EVIDENCE:` command must appear in the attempt's trace as a tool call, succeed
there (for `done`), succeed again when the grader re-runs it on the final tree, and be relevant
to the family. A `done` whose criterion fails is a **false completion**.

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
`endora_local.py`, `cc_local.py`.
