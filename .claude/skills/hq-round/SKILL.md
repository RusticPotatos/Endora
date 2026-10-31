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
round stands (step 3's status) and do the next due step.

## Standing rules (from the owner; never relaxed by a round)

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

## 1. Before a round (`start`)

1. **The build is ready**: every branch for the round merged into one branch; then, in order:
   - `sh scripts/prune-builds.sh 15 ..` (keep the disk from filling; a full disk stopped every
     agent on 31 Oct);
   - the full gate (AGENTS.md "Before handoff"), and `sh scripts/code-size.sh`; record the size
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
   established release (snapshots, rollback tags, drain), and check health and the build after.
4. **Start**: set the previous round aside and make the new one, the ten tasks' words unchanged:
   `docker --context nas exec -i endora-hq python3 - start --round N --from-tasks <ids>
   --previous <last round's ids> --yes < scripts/experiment.py` (dry-run first, without
   `--yes`, and read the plan: the right sources, nothing else). Record in `### Round N` what
   was released, what changed and why, the task ids and the start time.
   - A task that can no longer be met (round 2's task 8: its window had left the logs) is not
     silently kept: say so, and fix the task's conditions before the round counts it.

## 2. During the round (`tick`)

Check every 30 minutes (an hour when nothing can move): `status` and the desk.
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

1. **The result**: accepted of ten, interventions, spend; the side-by-side table of every round
   and Claude Code; what held the round back, by the evidence.
2. **Cost by model** from HQ's scorecard (`by_model`): per turn and share. It drove the review
   cap.
3. **The code**: `sh scripts/code-size.sh` against the last release; the `end-of-round`
   workflow (or `duplication-reviewer`) on the round's diff; consolidate its findings with one
   agent, in order of what each removes, a secret or safety gap first.
4. **Push the record** after the privacy scan, and summarise for the owner in plain words.

## 5. The next round (`next`)

Pick the changes from the round's defects, ranked by cost and effect, each with its evidence and,
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
