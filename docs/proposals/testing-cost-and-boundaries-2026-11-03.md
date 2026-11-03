# Testing: what covers the boundaries, what it costs, and a cadence (3 November 2026)

**The question.** Do the current tests cover the critical boundaries between node, worker,
engine, tools and persistence? What does testing cost? This is a bounded review.

**Method.**
- Read the tests, the gate script and CI.
- Measured times from logs that already exist.
- Added no test, framework or tooling.

The release `3fcee523` was live and verified before this began.

## 1. What exists

| Layer | What runs | What is real, what is faked |
|---|---|---|
| Unit tests | 2,222 across the workspace (`cargo test`) | One function or module |
| Scenarios | 224 (2,201 steps) in 50 `.feature` files (43 in `domains/team/features`) | **Real** engine and **real** SQLite store (in memory); the worker is a `SimulatedRunner`. No node, HQ API or worker process. |
| Worker tests | `app/worker/tests/turn.rs` runs the **real worker binary** | HQ, the model and the node are local fake HTTP servers. |
| Node contract tests | `app/node/src/api/contracts.rs` | The **real** node router, called in-process, with real git and real bundles. The worker is not there. |
| HQ API tests | `app/hq/src/api.rs` | The **real** owner API router and the **real** engine and store. |
| Script and deploy tests | `test-hq-drain.sh`, `test-mac-brain.sh`, `check-console.mjs` (every console screen), `experiment.py` tests | Against fakes |
| Release checks | `release.sh`: snapshots, drain, deploy, `make smoke` against the deployed node, health | Live |

- **The gate** (`scripts/gate.sh`) runs, in order: fmt, clippy `-D warnings`, the workspace tests,
  the console check, nothing-personal, collection parity and diff check.
- **CI** (`.github/workflows/ci.yml`) runs the same checks, plus the drain and brain script tests
  and the Ansible collection check.
- There is no coverage report, and none is needed for this decision.

## 2. Boundaries against the defects found

| Boundary | Is a test run with both real sides? | Defects found there | Verdict |
|---|---|---|---|
| Engine ↔ store | **Yes**: every scenario | Solo close as `met_criteria`; `set_status` saying "in review" on a closed item | Covered. Both defects lived inside this boundary, and a scenario now pins each. |
| Owner API → engine → store | **Yes**: HQ API tests | Criteria cut at 200 characters, or past the 8th | Covered at intake (refusals, live-checked after the release). |
| Engine briefing → worker | Partly. The worker's dev-dependency on `endora-team` reads a real briefing in its unit tests. The worker binary's tests use a hand-written HQ. | Criteria lost before the worker read them | **Gap, small.** No single test goes from owner API to the briefing the worker reads. Each half is tested. |
| Worker ↔ node (repo transfer) | **No.** Each side tests against a hand-copied version of the other. | Node took 16 MiB, worker read 10 MB (#1051–1053) | **Gap: the main one** (see 3). |
| HQ check → node (branch check) | **No.** The scenario fakes the node's answer. | A branch outside the lab got a 404, counted as failed work | Gap, but the fix is a refusal decided on the HQ side, before the node is asked. Low risk now. |
| Node → Portainer (logs) | No. Portainer is external; the adapter is tested against a fake. | `tail=100` cut without saying so | Covered as far as it can be: the node now says when it cut, and a unit test pins the limit. |
| Store ↔ older build | **One test** (`a_completion_nothing_checked_is_kept_where_an_older_build_never_misreads_it`) | `completed_unverified` would have broken a rollback | Covered for this change. No general mechanism, deliberately. |

**The pattern.**
- Where both sides are in one crate (engine, store, API), the tests are real, and the defects
  found there are now pinned.
- Every gap is a **process boundary**: worker ↔ node and worker ↔ HQ. The worker is a binary
  with no library, and so is the node. Neither can call the other in a test, so each side's tests
  fake the other by hand.

## 3. The repository transfer: is it tested through the real path?

**No.** Milestone 1 (`ab0fb8fb`) tested the transfer from both ends, against the same limit
(`CONTRACT_ANSWER_MOST`, 48 MiB, in `shared/protocol`).

| End | Test | Real | Faked |
|---|---|---|---|
| Node | `contracts.rs` (the >13 MiB pyparsing-shaped bundle) | Router, `repo_checkout`, git, the answer limit (`with_receipt`) | The worker |
| Worker | `a_checkout_the_size_of_pyparsings_is_read_whole_and_unpacked` (15 MiB) and `a_node_answer_too_big_to_read_is_said_to_be_so` | Worker binary, HTTP read, unbundling, the clear error over the limit | The node's answer, written by hand as `{"text": "<json> (receipt rN)"}` |

**What this already guards.**
- Both limits now come from one constant.
- The worker side tests the full size and the error past it, so a limit mismatch like #1051's
  cannot come back unnoticed.

**What it does not guard.**
- The **answer's shape** is copied by hand into the worker test. If the node changed how it puts
  the receipt or the bundle into `text`, both suites would still pass while checkouts broke.

**Why no joined test was added.** Both `endora-node` and `endora-worker` are binary-only crates.
A test that runs the real node handler and the real worker transport together needs one of these:
- **(a)** The worker's node client becomes a library the node's tests can call. That is a small
  restructuring of `app/worker`.
- **(b)** A test that starts the node binary with a lab, a fake HQ and a fake model, then runs the
  worker binary against it. That is new test orchestration.

The brief ruled out both: "existing infrastructure", and no custom test orchestration. Done
without them, the result would be one more hand copy, which adds nothing.

**Decision (owner, 3 November 2026): accepted risk.** Neither (a) nor (b) is built.

- **The risk:** the node's checkout answer format, `{"text": "<json> (receipt rN)"}` with a
  base64 bundle, can drift from what the worker parses without any test failing.
- **What bounds it:**
  - The format has not changed since Milestone 1.
  - A drift would show on the first live checkout, as an explicit "could not be read" said to the
    model, not as a silent loss.
  - The size limit, the cause of the real incident, is pinned on both sides.
- **Unchanged:** the focused tests, the boundary tests and the release gate.

**A possible later step, documented and not built:** one shared, real serialized fixture.
- A small file of test data, under `shared/protocol` or a test-data folder: one checkout answer
  with a tiny bundle and a receipt, exactly as the node writes it.
- Each side checks it against its own real code:
  - The node's contract test asserts that the real router's answer for a small repository has
    the fixture's shape. The bundle and receipt id are compared structurally, since they vary.
  - The worker's `node_handing` test serves the fixture's bytes and checks that they unpack to
    the expected head.
- No production code changes and no new orchestration: two existing test files read one file.
  It catches format drift, not size limits; the size limit is already covered.
- If the format is ever touched, do this first, in the same change.

## 4. What testing costs

Timings come from logs that already exist: the gate logs' creation and last-write times, and
the per-stage logs under `.local/gate`.

| Item | Time |
|---|---|
| Full gate, warm build | **90–160 s**, across 15 logged runs |
| Full gate, cold worktree (first build) | 552 s, once |
| Gate stages, warm (`3fcee523`) | fmt about 1 s, clippy about 6 s, tests about 70 s (compile 9 s; slowest test binaries 8 s and 6 s), console 14 s, the rest about 2 s |
| All logged gates, 30 Oct – 3 Nov | 16 runs, about **37 minutes** in total |
| Release (snapshots, drains, NAS and Pi deploy, smoke, health) | about 6 minutes |
| One focused test while iterating | seconds (for example, 0.02 s for the closure tests after compiling) |

**Where the 12+ hours went.**
- Not test execution: under 40 minutes of the period.
- It went on model-driven work, in turns that ended and waited on each other:
  - the Ralph rounds: audit about 60 minutes, goal run about 25, #796 follow-up about 65;
  - reviewing between rounds;
  - the comparison arms;
  - the earlier 12-task rounds, which ran for days.
- Each round also re-read the same evidence from scratch.

**Waste in the testing itself.**
1. **The same commit gated twice.** `9b0e7ce2` was gated by the follow-up session and again by
   the main session: about 2 minutes, and a full turn of waiting. The second run added
   independence, not information.
2. **Cold builds in new worktrees.** Each new worktree builds from scratch: about 9 minutes the
   first time, and 9–12 GB of `target/` each. 18 `target/` folders exist now. Gating in the
   shared `../Endora-gate` tree, as `gate.sh` already suggests, avoids it.
3. **Ralph sessions waiting on a gate.** Each session's turn ended on "waiting for the gate",
   then had to be woken. The gate is two minutes; running it in the foreground (it fits under the
   tool's 10-minute limit) removes a round-trip.

## 5. A minimal testing cadence

1. **While implementing:** a failing test first, then only the focused tests for what changed
   (`cargo test -p <crate> <name>`). Seconds.
2. **When a change touches a boundary** (a limit, an answer shape, a status or closure value, a
   stored column, anything another process or an older build reads): also run the tests on both
   sides of that boundary.
   - The scenarios for engine and store;
   - `app/worker/tests` and the node's contract tests for worker and node;
   - an older-build reading check when the store changes shape.

   Minutes.
3. **Full gate:** once, on the integrated release candidate, in the shared gate tree. Again only
   when a change after it invalidates the result. A prose-only follow-up reuses the earlier gate,
   as AGENTS.md already allows.
4. **Release:** unchanged.
   - Privacy check of the outgoing diff;
   - check that no turn is running;
   - snapshots and drain;
   - smoke and health;
   - a live check of the corrected behaviour where it can be done without changing anything.

**What not to repeat:**
- a second gate on an unchanged commit;
- a Ralph iteration that only waits for a gate;
- a new worktree for work that can share the gate tree.

## 6. What we deliberately did not build

- No joined node–worker test (see 3).
- No coverage dashboard or mutation testing.
- No new test framework or BDD layer: Gherkin stays for the engine's behaviour, where it is
  already real.
- No custom test orchestration, no agents or reviewers for testing, no scoring system.
- No new benchmark or model comparison.
- No general version-compatibility harness: one store test per shape change is enough at this
  rate of change.

**Not changed:** the gate, CI, the security checks and the release procedure.
