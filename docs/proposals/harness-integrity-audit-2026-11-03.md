# Harness integrity audit (3 November 2026)

**The question.** Which mechanisms on the minimal coding worker's path still drop, cut, distort,
suppress or misclassify information or actions that legitimate work needs?

**The code read.** Branch `milestone1` at `efceca76`: Milestone 1's three fixes, not released.
The audit traced the worker's path from task creation to the turn's end, through five boundaries:
- the worker: `app/worker` (`turn.rs`, `repo.rs`, `check.rs`, `shell.rs`);
- HQ's protocol and engine: `domains/team`, `app/hq`;
- the node's contracts, bridge and relay: `app/node`;
- the runner's keepers: `app/runner`.

**Out of scope.** Claude Code's runtime (`claude_code.rs`).

**The time.** 2 h inspection and 2 h correction were allowed. Inspection began 02:54 UTC; the
corrections were committed by 03:17 UTC; the gate and this report followed.

**How it was done.**
- One read-only tracer (Sonnet) per boundary, each told the class to look for.
- Every site that is fixed or carries a number in this report was then re-read in the code by
  the lead (Opus), and checked against HQ's database where a number is given. HQ was read with
  `mode=ro` only.
- Sites read only by a tracer are marked *(tracer)*.

**What was changed.** Seven corrections, committed locally on `claude/integrity-audit-ceb3c2`
(on top of `efceca76`), each test-first. Nothing was pushed, merged, released or touched live.
The full gate passes on the branch's head (`354409a9`):
- fmt and clippy (`-D warnings`);
- 2,218 unit and integration tests;
- 223 scenarios with 2,181 steps: team 179/1,948, and two other suites 24/160 and 20/73;
- the console, nothing-personal, parity and diff checks.

## Findings

Decision key:
- **fixed**: a confirmed defect, corrected here.
- **retained**: a justified limit or an intentional restriction.
- **open**: a confirmed defect left for a decision or a redesign.
- **concern**: possible, not shown to happen.

The commit named in each fixed row holds the test and the correction.

### 1. Assignment and prompt

| Mechanism | Existing behavior | Evidence | Consequence | Decision | Test or correction |
|---|---|---|---|---|---|
| Owner's criteria, `Criterion::from_lines` (`domain/evidence.rs:238`) | `.take(8)` kept the first 8 lines; the hand-out's own refusal past 8 never saw a 9th | code; HQ's route test made the task (200, 8 stored) | the task is made with its last requirements gone; the member is told 8 are "what done means" | **fixed** | `d2a9d08a`: every line read, the list refused whole (400); seeded scenario `04c61e38` |
| Count refusals (`Refusal::TooLong` for `MOST_CRITERIA`, `MOST_REFS`) | a count past the most was said as "longer than 8 characters" | code; test | the model is told something false, and cannot fix it from that | **fixed** | `d2a9d08a`: `Refusal::TooMany`, "more than the 8 criteria / 30 references" |
| Worker instructions (`turn.rs:916`, `:940`, `:986`, `:1055`) | told every turn a tester re-checks, the lead accepts, a lab question goes to the reviewer first, to hand sub-tasks to members below, "whatever your role" | ADR 0142 (2 Nov); all 28 `loop_solo` settings live | a model told someone will re-check may lean on a check that is not there; a hand-off is refused (`NotBelow`) | **fixed** | `f4a325ca` |
| HQ tool descriptions (`protocol.rs:228`, `:246`, `:252`, `:258`) | "another member re-checks it"; `create_task` "for another member"; "goes a rank up on its own"; "your role" | the same | the same | **fixed** | `f4a325ca` |
| Reviewer's read-only checkout paragraph (`turn.rs:1006`) | shown when `repo_checkout` is offered without `repo_submit` | code | true for a reviewer on a team loop; none is live | retained | — |
| Task title and description | stored and briefed whole, with no cap | code *(tracer)* | none found; a huge description against the prompt budget was not measured | concern | — |
| Briefing world budget (`domain/situations.rs`) | drops optional pieces, and names them | code *(tracer)*; audit: 0 drops in 3,374 briefings | stated; the task, criteria and lessons are outside it | retained | — |
| `create_task`'s `criteria_arg` (`protocol.rs:134`) | a criterion object without `text` is skipped | code *(tracer)* | one criterion of a member's own sub-task lost | concern | — |
| Review-briefing cuts (`engine::one_line`: goal note 300, owner posts 300 × 3) | cut with no marker | code *(tracer)* | review turns only; the reviewer is dormant | open, low | — |

### 2. Context and continuity

| Mechanism | Existing behavior | Evidence | Consequence | Decision | Test or correction |
|---|---|---|---|---|---|
| `my_turn`'s thread (`running.rs` `thread_for_turn`) | the newest 12 posts; older ones dropped with no sign under "The thread, newest last:" | code; the test pinned the absence | an owner's early word falls out of what the member reads, while the thread looks whole (posts since the last turn are still in the briefing, *tracer*) | **fixed** | `b91129e9`: "Left out here … the thread's N older posts (`ask_record` with task T finds them)"; seeded scenario |
| A rounds-out last word (`turn.rs:432`, `last_word`) | no words said on the way meant nothing posted, so the "rounds ran out" line was lost | run 5266 (#1050): its last round was a call, it ended "out of rounds", and no post says so; 4 of 7 kept worker transcripts ending out of rounds have none | the thread never says the turn was cut | **fixed** | `e1bb5e16`: the line goes alone, as out-of-room's already did |
| `beside` (`turn.rs:2384`): last word in 4,000, `ask_owner` in 1,200 | the model's words cut from the end, with no marker | code | a conclusion or a question that just stops, read as whole | **fixed** | `9d3b2fc9`: ends "…[cut]" |
| Work kept only at the prompt-budget cut (`keep_work`) | not on rounds-out, a failure or the 30-minute limit | code *(tracer)* | files survive in the kept sandbox; a lost sandbox loses them | open | — |
| Item sandbox lost (keeper stopped, runner restart, 3-day sweep) | the next turn finds it empty, with no word | code comments *(tracer)* | the turn starts over; the kept node branch still holds submitted work | retained, unstated | — |
| Checkout of another branch over a kept sandbox (`turn.rs:1331` → `repo.rs` `emptied`) | uncommitted work wiped silently | code (lead) | lost local work | concern, not observed | — |
| Handover `compose` (`domain/handover.rs:411`) | items past its budget dropped, without "n more" | code *(tracer)* | per-line cuts are marked; whole lines are not | open, low | — |
| Budget-cut line "goes on in a fresh turn" | HQ continues only when the relay metered at least half the budget | code *(tracer)* | can say "goes on" when HQ will not; prompt-budget cuts have been 0 since 1 Nov | concern | — |
| 6 KB result cut; shell 16 KB head and tail; transcript 2 MiB | cut, and said | code | stated | retained | — |

### 3. Tool discovery and execution

| Mechanism | Existing behavior | Evidence | Consequence | Decision | Test or correction |
|---|---|---|---|---|---|
| Node contract listing (`turn.rs:2585` `Node::list`) | any failure but 401/402 printed "no contracts this turn" on stderr; the turn ran without the node's tools | code | the model meets its task with no `repo_checkout` and no lab tools, and no reason (the #1051 shape) | **fixed** | `a7d42364`: the briefing says what is missing and why; the turn goes on |
| Bridged result (`node/api/bridge.rs:353` `held`) | `chars().take(16K)`, no marker | code; 0 work-turn calls of the 16 bridged tools (audit) | a part read as the whole | **fixed** | `354409a9`: "…[cut: N more characters]" |
| `Node::call` errors (`turn.rs:2639`) | any transport error, including the 60 s timeout, reads "the node could not be reached" | code *(tracer)* | a slow `repo_submit` or restart may have acted; the model may retry | open | — |
| Receipt not kept (`contracts.rs:182` `receipt`) | HQ failing to keep one leaves only a log line; the model gets no `(receipt rN)` and no reason | code (lead) | it cannot cite the call, and the evidence rule refuses it without saying why | open, small; not observed | — |
| Settings unreadable (`contracts.rs:47` `switches_of`) | fails closed: lists nothing; a call says "the owner has switched X off" | code *(tracer)* | false wording on a safe default | retained (security); wording open | — |
| HQ store error on a token (`hq/team.rs:118`) | read as 401, so the worker ends the turn "over" | code (lead) | a transient DB error is read as a dead token | open, low | — |
| `homelab_map` 100 containers; `find_tools` 8 names and 5 hits; `container_logs` cloud cut without a note | silent | code *(tracer)* | partial lists; the lab has fewer than 100 containers | concern | — |
| Restricted default, bridge policy, `only`/`never`, Endora's own containers refused, restart limits | refuse, and say so | code; ADR 0142 amendments | intended | retained (security) | — |
| Roles | nothing in the read paths still restricts by a retired role | code; ADR 0142 2 Nov | — | none left | — |

### 4. Evidence and verification

| Mechanism | Existing behavior | Evidence | Consequence | Decision | Test or correction |
|---|---|---|---|---|---|
| A box failure counts as the work's failure (`check.rs` `ok`; `wing.rs:648` `gate_from`) | "could not run" (bundle 404, a tool missing) sends `ok:false`; HQ counts a loop failure and sends work in review back | HQ: 6 of 16 failed checks, all bundle 404s (tasks 914, 948, 1017, 1019, and 1048 twice, which Milestone 1 fixed) | counts toward the 9-failure stop; an `ok:false` receipt is refused as evidence | **open** | not changed: the 9-failure stop also bounds spend, so exempting box failures needs a decision on what bounds a box that always fails |
| A lost check (`wing.rs:471`) | after `CHECK_LOST_AFTER_MS`, recorded failed: counted, and the item sent back | HQ: 3 of 16 failed checks (949, 1042, 1045); a late real result is then refused as `Finished` | a pass can be lost; an outage counts against the work | **open** | the same decision |
| Evidence rule on non-live criteria | any OK in-work receipt meets one; a citation, not verification | ADR 0114; audit replay | by design | retained | — |
| An `ok:false` receipt among those cited | refuses the whole claim | code *(tracer)* | stated; the member can drop it | retained | — |
| Outside-repo gate opens at submission | stated in the thread | Milestone 1 | — | retained | — |
| Receipt excerpt 1,000 characters; check tail 4 KiB | cut without a marker in the receipt | code *(tracer)* | the full result reached the model | retained | — |

### 5. Completion and recovery

| Mechanism | Existing behavior | Evidence | Consequence | Decision | Test or correction |
|---|---|---|---|---|---|
| A turn's final failure, outside `doing` (`guards.rs:368` → `Indeterminate`; `engine.rs:2827` skips it) | no recovery path wakes it | HQ now: #673 blocked since 30 Oct, #796 in review since 27 Oct, newest run failed, nothing since | stranded; a stranded loop item also holds its project's next hand-out | **open** | recovery design, not a small fix |
| HQ 502 / unreachable is the worker's fault (`turn.rs:2471`; `runs.rs:128` `fault_of`) | exit 1 means `Fault::Worker`; 2 retries 60 s apart; `Hq::send` has no retry | HQ: 32 runs failed for good on 502, unreachable or timeout (96 attempts); 7 of #1048's 13 | a release longer than about 3 min ends the turn for good, and its last word is lost | **open** | retry and fault policy; the review called it operational |
| Reply at its token limit (`turn.rs:334`) | the turn fails; the reply and its calls are dropped | HQ: 12 runs | lost turns; the cause may be the relay lowering `max_tokens` | open (the 8,192 cap is judged on its logs) | — |
| A failed or infra turn feeds `empty_turns` but not `shown_nothing` | two readers of one record disagree | code *(tracer)* | stale wakes are backed off longer | concern | — |
| Setting a status it already has | refused | audit: 26 never recovered | already decided (make it a no-op) | open (decided 2 Nov) | — |
| Solo item, from `in_review` to `done` | `ready_alone`, then `done_alone`; no reviewer needed | code *(tracer)*; live loops solo | sound | — | — |
| Drain, never-started re-wake, dropped projects | sound in the paths read | code *(tracer)* | — | — | — |

## Conclusions

**What was actually broken.** The same class as Milestone 1: a limit or a failure that said
nothing, or said something false.
- A count cap dropped criteria.
- Count refusals were worded as character limits.
- Four worker instructions and four tool descriptions promised a check or a member that does not
  exist: no tester, no lead, no reviewer on a solo loop, no hand-off.
- Silent cuts: the thread to 12 posts, the last word and the question to their caps, bridged
  results to 16K.
- A rounds-out turn could post nothing (seen live in run 5266).
- A node listing failure was invisible to the model.

None of these is shown to have decided a verdict. Run 5266's task was still verified, so each is
a correction to what the model is told, not a measured gain.

**What was fixed.** The seven commits above.

**What is intentionally retained.**
- Every security boundary in the tables. Nothing about authorization was changed.
- The stated cuts: 6 KB, the shell, the transcript, the world budget.
- The evidence rule.
- The 9-failure stop. Its spend bound is the reason the box-failure counting is left open.

**What remains unresolved.** Ranked by evidence:
1. **Box and lost-check failures counted as the work's.** 9 of 16 failed checks on record, each
   one a loop failure and a refused receipt. *Needs:* a decision on what bounds a box that always
   fails before it stops counting. Then a `box` mark on the check report (runner → HQ), and no
   count or send-back for it, while the gate stays shut.
2. **Infra failures are the worker's.** 32 runs lost, and stranded `Indeterminate` items (#673,
   #796 today). *Needs:* a retry and recovery design: an infra exit distinct from exit 1, retries
   that outlast a release, and a wake for a failed last turn outside `doing`.
3. **Small, confirmed by reading, never observed.**
   - No receipt, and no reason given.
   - The 60 s timeout reported as "could not be reached".
   - Work kept only at the budget cut.
   - A different-branch checkout wiping uncommitted work.
   - A store error read as 401.

**Coverage and limits.**
- *Read:* the worker's whole loop; HQ's protocol, briefing, thread, evidence, gate, check and
  recovery code; the node's contract listing, call, receipt and bridge; the runner's keeper.
- *Read by tracers only:* the rows marked *(tracer)*.
- *Not read:* the world pages' internals, the `gist` side call, `lessons_briefed`, sqlite
  atomicity of multi-step status changes, the console and owner paths that reopen tasks, and
  Claude Code's runtime.
- *How it was searched:* each site was found by searching for its pattern, so a loss in a shape
  not searched for can remain. This audit does not show the harness is free of defects.

**Is another engineering investment justified?**
- *Item 1:* yes, small. It has the most losses on record of anything open, once its spend bound is
  decided.
- *Item 2:* only with the release-drain work it belongs to.
- *Beyond these:* the research review's recommendation holds. Release Milestone 1 and these
  corrections, then run the held-out comparison. Do more harness work only where that comparison
  shows a loss whose trace diverges from Claude Code's at a harness cause.

*AI-assisted (Claude). HQ was read in read-only mode. Nothing was pushed, merged, released or
changed live.*
