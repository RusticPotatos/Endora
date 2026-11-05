---
name: hq-round
description: Run, judge and close a round of the baseline experiment that measures Endora HQ against Claude Code (docs/proposals/baseline-experiment-2026-10-28.md). Use when the owner says to start, check, judge, close or plan the next round, or asks how HQ is doing against Claude Code.
---

# A round of the baseline experiment

The experiment: the same ten tasks, done by Claude Code (the bar, 10 of 10) and by HQ, one round
after another, with one evidence-backed set of changes between rounds. This skill is the playbook
that rounds 0 to 3 taught (28 Oct – 1 Nov 2026). Follow it in order; every step leaves a written
record, so rounds compare.

The record is one document: `docs/proposals/baseline-experiment-2026-10-28.md`.
- `### Round N` holds the round's table: one row a task with the five fields.
- `## Open issues and next steps (kept current)` holds the defects (D1, D2, …), next steps, the
  code size, and the owner's own list.
Keep it current as you go, never in a scratch file alone.

Arguments: `start`, `tick`, `judge <task>`, `close`, `next`. With none, find where the current
round stands (step 2's status) and do the next due step.

**Run it from a session started in this repository**, so this skill, its agents
(`round-judge`, `duplication-reviewer`), the `end-of-round` workflow and the guard hooks in
`.claude/settings.json` load. The round's checks run as `/loop /hq-round tick`: the loop's prompt
is the skill's name, so every tick reads the rules as they are now, never a pasted copy that goes
stale. Start a fresh session for each round; the record, not the conversation, carries it over.

**Fresh sessions, safely.** A new session in this repository briefs itself: the SessionStart
hook (`.claude/hooks/round-brief.py`, read-only) gives it the checkout, the round branches
being built, the latest round's table and the open defects, from the record. So handing over
is three steps, and nothing is pasted:
1. The old session brings the record up to date (the round's rows, new defects, what waits on
   the owner) and pushes it. That is the handoff; a conversation's memory is not.
2. Start a new session in this repository (the desktop app, or `claude` in the folder) and say
   `/hq-round` (or `/loop /hq-round tick` for a round in flight).
3. The old session stops its loop.
Start one at each round's start, and whenever a session has been summarised or runs long. A
round branch's builder works in its own worktree; the driver never edits it.

## The cycle (a round in 8 hours)

| When | What |
|---|---|
| Round N runs, **8 hours** at most (owner, 1 Nov) | Ticks (step 2); each task judged as it reaches done or blocked (step 3). Meanwhile round N+1's fixes are built on one branch, `round<N+1>`, in its own worktree and session: a failing test and a scenario first, the gate on each (step 5) |
| Hour 8, or all judged | Close round N (step 4): a task not finished is not accepted |
| Before the next round | Every change for round N+1 tried first on the replay bench (`scripts/replay-bench/`, real decision points from round N, minutes not hours); score round N with `experiment.py metrics`, three ways (the fixed ten, homelab fresh, outside the homelab) |
| Then | Merge `round<N+1>`, gate, release, start round N+1 (step 1) |

Speed comes from removing waits, not from more agents: what held round 3 was members stopped
with money in the pool (D21, D15), verdicts that could not land (D19) and tasks never started.
Settings that change speed (items in hand at once, limits) change only at a round's start.

## Standing rules (from the owner; never relaxed by a round)

- **The purpose of harness engineering is not to make the harness more sophisticated. It is to
  make valuable work more reliable and economical.** Fix the machinery around the reasoning
  before adding to it; add no agent, orchestration layer or scoring framework to run a decision
  the round's report can hold.
- **Claude Code is the bar.** Improve HQ by measured changes; never propose scrapping it.
- **Nothing personal in a tracked file**: the repo and the fork are public. Review every outgoing
  diff and commit message before a push.
- **Never touch** the fork's `group_vars`, `host_vars`, inventories, vaults, or MongoDB's
  password. The lab is read-only to you; the owner runs playbooks.
- **Money and keys are the owner's.** Approve a member's "out for today" only within the day's
  pool; never top up past the owner's standing word; never enter a password.
- **No release in the middle of a round.** A round measures one build.
- **One pattern per job.** Search before adding a helper; after parallel work, a duplication
  review; consolidate with one agent, not several.
- **A cucumber scenario in the owner's words with every behaviour change** (`docs/standards.md`).
- **Decide from evidence.** Ask the owner only for money, keys, accepted risk, irreversible
  steps or taste; otherwise decide, and cite the evidence (the round's record, the ADRs,
  published guidance).
- **Let the agents be agents** (owner, 1 Nov). Prefer removing a hand-off, check or question to
  adding one; a step of HQ's pipeline stays only where the record shows it changed an outcome.
- **Investigate without asking, and never idle.** Read-only checks (the store, logs, metrics,
  the lab's state) need no permission. While a round waits on anything, prepare the next one on
  its branch (fixes, arms, tasks), gated and pushed; releasing still waits for the close.
- **No wait longer than it earns.** A task's window or wait is as short as shows what it must
  (task 8's window: a few hours, unless a round shows the longer one finds more).

## 1. Before a round (`start`)

1. **The build is ready**: every branch for the round merged into one branch; then, in order:
   - `sh scripts/prune-builds.sh 15 ..` (keep the disk from filling; a full disk stopped every
     agent on 31 Oct);
   - the full gate, `scripts/gate.sh` on a detached copy (its header says how), which prints the
     unit/scenario/step totals and `sh scripts/code-size.sh`; record the size
     in "Code size" if it is a release;
   - a duplication review of the round's diff with the `duplication-reviewer` agent (or the
     `end-of-round` workflow); act on what it finds before release, or list it for after;
   - the privacy scan: `git log --format=%B <released>..<branch>` and the added lines of
     `git diff <released>..<branch>` hold no local path, real name, home address or key.
2. **The setup is right** (round 2 lost four hours to these):
   - every member's model can run on its runtime (Claude Code runs Claude alone; D1);
   - every released tool is in the loops' tool lists (D4);
   - the day's pool: the credit-sized pool or the owner's top-up rule covers the round's
     reviews; check it before the owner sleeps, not after it runs out.
3. **Release** only with no turn running (HQ's `runs.status = 'running'` is empty), through the
   established release, `zsh scripts/release.sh` (push, snapshots, rollback tags, drain, NAS and
   Pi, health), and check the build after.
4. **Start**: set the previous round aside and make the new one, the ten tasks' words unchanged:
   `docker --context nas exec -i endora-hq python3 - start --round N --from-tasks <ids>
   --previous <last round's ids> --yes < scripts/experiment.py` (dry-run first, without
   `--yes`, and read the plan: the right sources, nothing else). Record in `### Round N` what
   was released, what changed and why, the task ids and the start time.
   - From round 4, the fresh set: 2–3 homelab tasks HQ has not seen and one outside the
     homelab (F4: a small task in a public repository new to HQ, cloned into the sandbox, its
     branch kept local), each with criteria written before either arm runs; Claude Code does
     them too. Retire each once run.
   - A task that can no longer be met (round 2's task 8: its window had left the logs) is not
     silently kept: say so, and fix the task's conditions before the round counts it.
   - Round 4's solo arm (item E): the same tasks again, with `start --round N --from-tasks
     <ids> --solo <name>` (dry-run first), in a project that member works alone (ADR 0142,
     amended 2026-11-01); the member is a Haiku member hired for the arm, not one of the team's.
5. **The judge is tested** (from round 4, before its first judging) on its known-good and known-bad branches, on both
   counts: it accepts and refuses rightly, and its verdict is recorded and acted on (D19 was
   the second, with the reasoning right). The cases are `domains/team/tests/fixtures/judge.json`,
   each with the verdict a right judge gives and why. **Delivery** (and what HQ's rules find) is
   `features/the-judge-is-tested.feature` with the worker's
   `a_claude_turn_loads_what_hq_marks_for_its_kind`, both in the gate. **Correctness**: give the
   `round-judge` agent each case (its criteria and files, not its verdict or why) and compare
   its verdict with the case's; one wrong is a finding to fix before judging.

## 2. During the round (`tick`)

Check every 30 minutes (an hour when nothing can move): `status` and the desk. A member
stopped while the pool has money and work waits is a setup fault: get it going through HQ's own
card and log it (the owner, 1 Nov: "I don't want phase 3 to stop for the 80 something cents").
- `docker --context nas exec -i endora-hq python3 - status --tasks <ids> < scripts/experiment.py`
- **A question on the desk**: answer only by restating the task's own criteria; count it as an
  intervention. When the item's tools cannot meet them, add "If your tools cannot meet these,
  record that and end it as blocked." Never give a hint or an answer.
- **A question on an item already judged**: close it ("the item is blocked and judged"); not
  counted. If a judged item keeps waking members, set it aside (a dropped project) so it stops
  spending.
- **A setup fault you fix** (a runtime, a tool list, a stuck item that needs a nudge) is logged
  as setup, not as an intervention, and becomes a numbered defect.
- **The pool**: within it you may approve a member's day; past it, the owner's standing word or
  ask.
- **Every new defect** gets the next number (D14, D15, …) in the open-issues list, with the
  evidence (task, time, numbers) and the fix proposed.

## 3. Judging (`judge <task>`)

Judge when a task reaches done or blocked, against its written criteria only, with **every claim
re-checked by you, read-only** — use the `round-judge` agent for this. What re-checking meant:
- **Docs**: every default in the README against the role's `defaults/main.yml`, value for value.
- **Tags or versions**: each one exists on its registry (Docker Hub's tags API; ghcr with an
  anonymous token).
- **Lint**: `ansible-lint nas.yml` and `yamllint .` on the branch against develop — no new
  findings, not "no findings".
- **Tests**: run them at the tip and at any deliberately broken commit the criteria ask for.
- **Logs**: the quoted lines are in the live logs (redact keys from your own output).
- **CI**: a run on the branch exists (a workflow that runs only on main and develop gives none).
- **"Nothing changed on the lab"**: container start times, Docker's events, and HQ's record of
  restarts.

Then, for an accepted branch, review its diff and commit messages for anything personal and
approve the push (or, with automatic delivery, check the scan's result). Record the row:
**Accepted** (yes/no, with the evidence in brief), **Cost** (HQ's record), **Elapsed** (start to
first ready; note waits on the pool), **Interventions**, and **Rework** (filled seven days on).

## 4. Closing a round (`close`)

1. **The result**: accepted of ten, **reported** (what HQ recorded) and **verified** (what the
   judge finds the work meets, whatever HQ recorded) apart, so a fixed recording defect is not
   read as better work, and each of these for the fixed ten, the homelab fresh tasks and the
   task outside the homelab apart (a gain that does not show outside the homelab did not carry
   over); interventions, spend; the side-by-side table of every round
   and Claude Code; what held the round back, by the evidence.
2. **Cost by model** from HQ's scorecard (`by_model`): per turn and share. It drove the review
   cap.
3. **The code**: `sh scripts/code-size.sh` against the last release; the `end-of-round`
   workflow (or `duplication-reviewer`) on the round's diff; consolidate its findings with one
   agent, in order of what each removes, a secret or safety gap first.
4. **The investment gate** (from round 4; the experiment doc's "Round 4, planned"), a short
   section of the report answered from its numbers: acceptance (fixed ten and the fresh set
   apart), cost per accepted task, interventions, whether the gain carried over to the fresh
   tasks, and what the round cost to build (sessions and mechanisms added), with break-even
   tasks for a change that saves per task. Keep the development and unattended-operations
   scorecards apart: they are two products. If none improves across two rounds, put the three
   choices to the owner: continue, narrow the development harness, or redirect to unattended
   operations.
5. **The cheat sheet** (the owner's "Agent teams cheat sheet" Claude Doc, linked from the global
   instructions): the round's lessons, with their numbers, go into its "Field notes (full)" tab
   as a dated section, and its current principles change where the evidence does; its "One page"
   tab stays one page (replace a line, don't add one). Public references only.
5. **Push the record** after the privacy scan, and summarise for the owner in plain words.

## 5. The next round (`next`)

Pick the changes from the round's defects, ranked by cost and effect, each labelled **general**
(helps any task) or **homelab-specific** (helps only the lab's work, and needs a stated reason),
each with its evidence and,
for a design choice, published guidance (as the review cap cited Anthropic's and OpenAI's
evaluation advice). Put each on its own branch from one round branch; build them in parallel
only when they touch different files, and give each a failing test and a scenario first. Merge
each as it passes its gate; then step 1 again.

## What the rounds taught (keep)

- A turn's prompt must stay small: stubs of old results, a turn budget with a fresh
  continuation, a handover that carries the work (and the newest handover only), work in
  progress kept on a branch.
- Paid reviews were most of the cost and caught less than code checks did: reviews capped at
  Haiku, HQ's mechanical checks (forbidden paths, CI) do the rest.
- Setup faults cost more than model mistakes: check runtimes, tool lists and the pool first.
- Parallel agents make duplicates: one consolidation pass before release, every time.
