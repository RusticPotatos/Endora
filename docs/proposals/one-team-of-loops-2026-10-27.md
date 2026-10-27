# One team of loops: retiring the company structure

2026-10-27. The owner's decision: retire the company structure. One team with no titles,
like Valve's. Boring under the hood, fun to watch, like an ant farm. Every line of code is
another bug and another hour of work.

## Why, from the record

- **The structure produces most of the work.** The last 24 hours: 80 turns of talk, 46 of work.
  #796 went round the chain of command for a day (17 coordination turns for 1 change) and never
  reached the owner. The fix was more structure: a send-back rule and a briefing line (74eef5ae).
- **The conversation machinery goes unused.** 3 help requests ever, none answered. 5
  conversations, none with a message.
- **The owner's desk fills with process.** On 26 Oct, about 3 of some 20 desk decisions needed
  the owner (money, a risk, a change to the house). The rest were duplicate branches, merge and
  deploy bookkeeping, checks with no baseline, and stale notices.
- **The weight.** The team context is about 104,000 lines (38,000 of them tests). Some 60
  ADRs since 0100, most adding a role, a rule or a guard against what the last rule caused.

The model to copy is Ralph (Huntley; snarktank/ralph):
- a loop takes the most important unfinished item from a plan file;
- it does one thing, runs the checks, and commits or reverts;
- it writes down what it learned, and goes round again.

Checks push back on bad work; review by another agent does not. When a loop keeps going wrong,
the fix is a line in its plan or prompt (a "sign"), not a new role.

## The shape we want

**A member** is a worker: a name, a brain, an avatar in the office. There are no roles. Every
member may do any work its loop's tools allow.

**A loop** is one plan, one set of checks, one cap. A project becomes a loop.

| Loop | Plan | Checks (back-pressure) |
|---|---|---|
| ansible-nas | the fork's open work | syntax, ansible-lint, yamllint, the fork's tests, a dry-run diff |
| The lab | keep the house healthy | health and alarms against a baseline taken before |
| Endora | the checkpoint | the full gate |
| Holodeck and league | scenarios, games | the world's own rules |

**A turn**: a free member takes the top unfinished item of a loop's plan and works it. It runs
the checks and records the result. If the item passes, it is done, and anything for the house
goes to the owner as one change to approve. If it fails, the member writes why in the plan. A
loop that fails the same item 3 times takes it a model tier up (local → Haiku → Sonnet), then
puts it on the owner's desk.

**Memory** is files a loop reads at the start of every turn: its plan, its notes (`AGENTS.md`
style) and its history. No knowledge system beyond that.

**The owner's desk** has five kinds of card and nothing else:
1. money;
2. credentials and keys;
3. accepting a risk;
4. anything irreversible (deletion, a push or merge into the fork);
5. taste.

**The office** stays the window: members walk to their loop's corner, sit at its desk, play when
there is nothing to do. The activity feed tells the story.

## What stays, what shrinks, what goes

### Stays: the boring core

- **The tick and turn placement:** the scheduler, the Mac queue, the free-turn cap (0103, 0130,
  0131).
- **Caps, the runaway tripwire, the kill switch, the drain** (0133, 0134), and the pool, with
  one cap per loop instead of per project.
- **"Models propose, code authorizes":** contracts and the bridge (0106, 0108); restarts and lab
  tools stay gated.
- **Confidentiality tiers** (0140, 0141), **secrets** (0125, 0127).
- **Runs, receipts, transcripts, replay:** the record (0129), as a log.
- **Escalation by model tier:** the one part of 0139 worth keeping. It moves an item to a
  stronger brain, not to a boss.

### Shrinks

| Today | Becomes |
|---|---|
| Projects, budgets, reports (0113; projects 1,500 lines) | A loop: plan file, checks, cap |
| Routines (0107) | A loop's schedule, or an item a loop adds to its own plan |
| The world, map, quiz, gaps, notebooks, retraction (0118, 0122, 0126; about 7,000 lines) | Each loop's notes file plus the house page; the quiz and stages go |
| Evidence and acceptance criteria (0114; 2,500 lines) | The checks' results; receipts kept as the log |
| Guards (0124; 2,500 lines) | The caps, plus one rule: a loop failing an item 3 times escalates |
| The digest and desk kinds (0132) | The five owner kinds; notices clear themselves |
| Deploy cards (0115) | HQ sees the merge and the run itself; docs-only changes need no check |

### Goes

| Part | ADRs | About |
|---|---|---|
| CEO, chain of command, board decisions, planning | 0111 | 1,400 lines |
| Leads, areas, area reports, wings, staffing, furlough, seats | 0110, 0113 (part), 0115 (part), 0118 | 3,700 lines |
| Enforced RACI, roles, discipline | 0120 | 850 lines |
| The tester's review of every change | 0114 (part) | 800 lines |
| Stuck ladder, huddles, decide_stuck | 0124 (part) | 900 lines |
| Conversations, assistance, consult, peers | 0137, 0138, 0139 (part) | 1,900 lines |
| Office proposals and votes | 0119 (part) | 300 lines |

Moving to loops removes, or folds into files, roughly half of the team context's code and tests.

## Phases

Each phase is its own release, with the usual rollback tag and snapshots. Data is never
deleted outright: retired tables are archived, read-only, in the snapshot first.

1. **Freeze and decide.** Write one ADR, "One team of loops", superseding 0110, 0111, 0120,
   0137 and 0138, and amending 0113, 0114, 0115, 0118, 0124 and 0139 to point at it. Build no
   new structure from here on.
2. **One loop, side by side (a week).** The ansible-nas loop runs on the free models alongside
   the team, on the same budget. Compare changes landed, dollars per change, owner clicks, and
   how often the checks catch it. Go or no-go on the record.
3. **Flatten.** Every member becomes a plain member; the CEO, leads and the tester stop being
   seats. Projects become loops with plans. The desk keeps its five kinds; the rest stop being
   created. Nothing is deleted yet; the old code just stops being reached.
4. **Delete.** Remove what nothing reaches, one part per release in the "Goes" table's order,
   each with its tests. Archive first, then drop.
5. **Shrink knowledge.** Fold the world, the map, the quiz and the notebooks into each loop's
   notes file, once the loops show which notes they actually read.
6. **Make it an ant farm.** Each loop gets a corner of the office, members walk to the loop
   they work, and the feed tells it plainly. The Holodeck and the league stay.

## How we know it worked

- **Owner clicks a day:** at most the five kinds.
- **Talk turns:** none. Every turn works an item or plays.
- **Dollars per landed change:** lower than the team's last 7 days.
- **Lines in the team context:** under 50,000, and falling.
- **The checks:** they catch the loop's mistakes before the owner sees them.

## For the owner to say

- Keep the office games, the Holodeck and the league? (They are the fun part; this plan keeps
  them.)
- Members keep their names and avatars? (The plan keeps them: ants with names.)
- Merges into the fork stay yours, or may a loop merge docs-only and lint-only changes once the
  checks pass?
