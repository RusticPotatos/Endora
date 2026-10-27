# One team of loops: retiring the company structure

2026-10-27. The owner's decision: retire the company structure. One team, with none of a
company's machinery. Boring under the hood and fun to watch, the way you can watch ants dig
tunnels. Every line of code is another bug and another hour of work. Agreed with the owner
point by point (a grilling session, 27 Oct); due within 24 hours.

## Why, from the record

- **The structure produces most of the work.** The last 24 hours: 80 turns of talk, 46 of work.
  #796 went round the chain of command for a day (17 coordination turns for 1 change) and never
  reached the owner. The fix was more structure (74eef5ae).
- **The conversation machinery goes unused.** 3 help requests ever, none answered. 5
  conversations, none with a message.
- **The owner's desk fills with process.** On 26 Oct, about 3 of some 20 desk decisions needed
  the owner. The rest were duplicate branches, merge and deploy bookkeeping, checks with no
  baseline, and stale notices.
- **Memory is read, never kept.** 607 turns in 7 days: 75 read a page and 36 searched history.
  One edited a page, and none used the proposal, canon, stage, story, gap, quiz or notebook
  tools.
- **The weight.** The team context is about 104,000 lines (38,000 of them tests).

The model is Ralph (Huntley; snarktank/ralph):
- a loop takes the most important unfinished item from its plan;
- it does one thing, runs the checks, and commits or reverts;
- it writes down what it learned, and goes round again.

Checks push back on bad work; review by another agent does not. When a loop keeps going wrong,
the fix is a line in its plan or prompt, not a new role. This matches the guidance on file:
Anthropic's (route by difficulty, with a stronger model to consult; notes kept outside the
context; fetch what is needed when it is needed) and OpenAI's (a mix of models, a human after
a failure threshold, start simple).

## What was decided

### The team

- **Five members, one per model, ranked:**

  | Rank | Member | Model |
  |---|---|---|
  | 1 | Mara | Claude Opus (may carry a title such as "CEO") |
  | 2 | Kit | Claude Sonnet |
  | 3 | Ada | Claude Haiku |
  | 4 | Rae | the Mac's model |
  | 5 | Bo | the NAS's local model |

  A title is a name only: nothing in the code depends on it.
- **Names:** the eight names (Ada, Bo, Cy, Dee, Mara, Kit, Rae, Zed) are fixed. A generator
  names any members past eight, and every name is unique, like a key. Cy, Dee and Zed retire;
  their history is archived.
- **No company machinery:** no roles, chain of command, testers, conversations, help
  requests, stuck ladder or huddles.
- **One turn at a time per member.** The local members (Rae, Bo) work whenever there is work.
  The Claude members cost money only when an item escalates or a public PR needs review, always
  inside the pool's cap.

### The work

- **A loop is a project with a plan and checks.** Its plan is its existing task list in HQ,
  ordered. "Give work" adds an item to a loop's plan.

  | Loop | From today's project | Checks |
  |---|---|---|
  | ansible-nas | 2 | the fork's checks on a submitted branch, the dry-run diff |
  | The lab | 1 and 4 (#796 becomes an item) | health and alarms against a baseline taken before |
  | Holodeck and league | 3 and 5 | the world's own rules |

- **Recurring items:** the lab's 6-hourly health check becomes a recurring item in its loop.
  The CEO's daily review and weekly planning, and the leads' daily area reports and weekly
  patterns, go.
- **Who works an item:** every item starts with Bo. After 3 failed checks it moves up one rank
  (Rae, Ada, Kit, Mara). If Mara fails it, it comes to the owner. A loop may raise the rank it
  starts at. A failed check means the check failed, or the item was sent back.
- **What loops may do:** anything in their own sandboxes and branches. Restarting a container
  stays capped and announced, and playbook runs stay the owner's.
- **Public PRs:** the member one rank up reviews a branch before it becomes a PR (Mara's own
  work goes to Kit). A send-back counts as a failure. Then the PR goes to the owner's desk, with
  one button to make and merge it once the owner has made a GitHub token; until then, a link to
  GitHub.
- **Endora working on its own code, and loops running the playbook:** decided later, from the
  Holodeck's and the league's results.

### What stays

- **Endora's runner:** the tick, turn placement, the Mac queue, the workers.
- **The safety caps:** the runaway tripwire, the free-turn cap, the pool and per-loop caps, the
  kill switch and the drain.
- **"Models propose, code authorizes":** contracts, the bridge, gated lab tools.
- **Confidentiality tiers and secrets.**
- **The record:** runs, receipts, transcripts and replay, as a log. They age out after 30 days.
- **Memory:**
  - the house page and the lab page;
  - one notes page per loop, which the loop appends to each turn;
  - history search;
  - **the map.** When a member finds where something lives, it adds one line, with no proposal
    or approval. A line unread for 30 days fades; a used line rises. The owner watches it grow
    in the World card, with what is new this week and how often each line was used.
- **Fun:** chess, the league and the Holodeck. Ping-pong and pool become a pure animation:
  idle members play, the browser makes up a score, nothing is recorded.

### The owner's desk

Five kinds of card: money, credentials, accepting a risk, anything irreversible, and taste.
Plus "this PR is ready". Notices clear themselves once their cause is gone. HQ checks the fork
itself to see a PR merged, so there is no "Deployed" click, and a docs-only change gets no
post-deploy check.

### What goes

| Part | ADRs | About (code, before tests) |
|---|---|---|
| CEO, chain of command, board decisions, planning | 0111 | 1,400 lines |
| Leads, areas, area reports, wings, staffing, furlough, seats | 0110, 0113 (part), 0115 (part), 0118 | 3,700 lines |
| Enforced RACI, roles, discipline | 0120 | 850 lines |
| The tester's review of every change | 0114 (part) | 800 lines |
| Stuck ladder, huddles, decide_stuck | 0124 (part) | 900 lines |
| Conversations, assistance, consult, peers | 0137, 0138, 0139 (part) | 1,900 lines |
| Guards that police coordination | 0124 (part) | 2,000 lines |
| Office proposals and votes; tracked ping-pong and pool | 0119 (part), 0121 (part) | 2,900 lines |
| The world's stages, canon, stories, gaps, quiz, notebooks, retraction | 0118, 0122, 0126 (part) | 5,000 lines |

### Data

A full backup first. Then each retired table is copied to a dated read-only archive file on the
NAS, kept indefinitely, and only then dropped.

## The 24-hour work plan

Every step: test first, the full gate in `~/repos/Endora-gate`, release only when no turn is
running (rollback tag and snapshots), a production check, and a line in this file. The
standing authorization covers releases. Steps 1–9 each ship on their own; a failure stops at
that step and rolls back to the previous release. A step's exit check is what proves it done.

**Before step 1:** tag `before-loops` on the NAS and Pi images, and take a full snapshot of HQ
and the node (kept with the archives). This is the rollback for the whole cut.

| # | Step | Exit check | Hours |
|---|---|---|---|
| 0 | **Decide in writing.** ADR 0142 "One team of loops", superseding 0110, 0111, 0120, 0137 and 0138; amending 0113, 0114, 0115, 0118, 0119, 0121, 0122, 0124, 0126, 0132 and 0139. Where-things-go updated. | ADR merged; links resolve | 1 |
| 1 | **Five members.** Members gain a rank from their model. Mara, Kit, Ada, Rae and Bo set as above; Cy, Dee and Zed retired, with history kept. The name generator past eight; unique names enforced. | Production lists five members, ranked; a sixth gets a generated name | 2 |
| 2 | **Loops.** A project carries a plan (its ordered tasks), its checks and a starting rank. Projects 1 and 4 merge into the lab loop, 3 and 5 into Holodeck and league. The tick gives a free member the top unfinished item of a loop. The health-check routine becomes the lab's recurring item; the other routines go. | A new item on the ansible-nas plan is taken by Bo with no one asked | 3 |
| 3 | **Escalation and review.** Three failed checks move an item up one rank; a failure at Mara's rank goes to the desk. A branch headed for a PR is reviewed by the next rank up, and a send-back counts as a failure. | Test: a failing item climbs Bo → Rae → Ada → Kit → Mara → desk; production: a branch reviewed one rank up | 2 |
| 4 | **Switch the company off.** Briefings lose the chain, roles and asking up; the tools go from tool lists: `consult`, `request_assistance`, `decide_stuck`, the tester's `review` and `accept`, `hand_to`, `propose_project`, `request_hire` and `ask_owner` up the chain (it now reaches the desk only, for the five kinds). The coordination guards stop. | A day of production turns: no talk turns, none of the removed tools offered | 2 |
| 5 | **The desk.** Only the five kinds and "PR ready" are created; notices clear themselves; HQ checks the fork for merges; no check after docs-only changes; the PR card links to GitHub, and has a make-and-merge button once a token is set. | Production desk shows only those kinds; a merged PR closes its own card | 2 |
| 6 | **Memory.** The loop notes page, appended each turn. The map: an `add_to_map` line per discovery, fading after 30 days unread, rising with use; a "new this week" and use count in the World card. | A turn adds a map line, and the next turn's briefing shows it | 2 |
| 7 | **Office.** Five desks; ping-pong and pool become the animation; office proposals and votes go; chess, league and Holodeck untouched. | Office renders at 375 px with five members; the league plays on | 1 |
| 8 | **Archive and delete.** Archive the retired tables, then delete each part in the "What goes" table's order, with its tests, routes, screens and tables. | Gate passes after each part; the team context about 55,000–60,000 lines | 7 |
| 9 | **Verify and record.** One production cycle on all three loops; owner clicks counted; talk turns 0; spend per landed change against the last 7 days; merged to develop after a privacy scan. | Numbers recorded here; develop merged | 2 |

About 24 hours of work, steps 0–9 in order. Steps 5, 6 and 7 touch separate code and can run
side by side if time runs short.

**Days 2–3:** fold the remaining knowledge machinery into the map and the notes pages, and
trim the tests that guarded deleted behaviour, to under 50,000 lines.

**Needs the owner:** a fine-grained GitHub token for the one-button merge (step 5 works without
it). Nothing else in the plan waits on the owner.

## How we know it worked

- **Owner clicks a day:** the five kinds and ready PRs only.
- **Talk turns:** none. Every turn works an item or plays.
- **Spend per landed change:** below the team's last 7 days.
- **Lines in the team context:** 55,000–60,000 after 24 hours, under 50,000 after 3 days.
- **The checks:** they catch a loop's mistakes before the owner sees them.

## Progress

- **Rollback point, 27 Oct 01:17 UTC:** `before-loops` image tags on the NAS and the Pi;
  snapshots `before-loops-hq-20261027T011732Z.db` and `before-loops-node-20261027T011732Z.db`.
- **Step 0:** ADR 0142 accepted (fdf50788). Sixteen ADRs marked superseded or amended, and
  where-things-go updated.
- **Steps 1–4, code:**
  - members as ranked models, with the name generator (step 1);
  - loops handing out their plans' items (step 2);
  - three failures climbing a rank, and review one rank up (step 3);
  - the company switched off for members in the loops, and the `become_one_team` cut-over
    (step 4).

  Each step was written test first; the full gate passed for steps 1, 2 and 3. Nothing changes
  in production until the cut-over runs.
- **Released d1bf01ff (steps 1–4), cut over 27 Oct about 02:40 UTC:**
  - `become_one_team` seated Mara (Opus, rank 1), Kit (Sonnet, 2), Ada (Haiku, 3), Rae (the
    Mac, 4) and Bo (local, 5), all plain members.
  - Cy and Dee retired; Zed retires when its league game ends.
  - Only the lab's health check runs on, as Bo's.
  - **Step 1's exit check met:** five ranked members. A sixth's generated name is proven by the
    tests.
  - **Step 2's exit check met:** the owner's item #855, put on the ansible-nas plan with no
    assignee, was taken by Bo unasked and claimed within the minute.
- **Step 5 (the desk) and step 6 (memory), code:** written test first.
  - Notices are kept but never wait; there are no furlough offers and no Deployed cards.
  - The map: `add_to_map`, where the same line again confirms it and unconfirmed lines fade
    after 30 days; shown in briefings and in the World card.
  - Each loop's latest lines go into its turns' briefings.
  - "Give work" now asks which loop, not which lead.
- **Released cef31e89 (steps 5–7).** Checked at 375 px:
  - the desk empty, the floor without the retired, no sideways scroll;
  - the league on its rematches (Mac against Local, game 851).
- **Found and fixed (c3d724dd, live):** the restart for steps 5–7 ran an ADR 0131 migration on
  every start. It gave Bo and Rae a Claude again, which took them out of the loops, and #855 fell
  to the old stuck path. The migration now touches only members with no work brain yet, as it
  was meant to. The cut-over ran again (both seated), and #855 went back on Bo's plan.
  - On the record, Bo's two turns on the local model claimed #855 and did nothing. The loops'
    answer to that is three stalls climbing to Rae.
- **Step 8, part A, conversations and help requests deleted** (gated next): the conversation
  and help-request code, routes, screens, features and tests, and the canonical briefing.
  - The input lineage stays, always on.
  - The tables go after the release snapshot, which is their archive.
  - The team context is about 96,300 lines.
  - Released ace5bc11; the gate passed with 4,469 tests.
- **Step 8, part B, office proposals and votes deleted** (released f6b6e062): the office keeps its
  layout, ping-pong, pool and chess; proposals, votes, undo, reset and veto go, with their tables.
- **Step 3's exit check met in production:** Bo's stalls on #855 climbed it a rank, to Rae (the
  Mac, rank 4). No desk card, no huddle.
- **Step 8, parts C and D, the stuck ladder, huddles and the coordination guards deleted**
  (−6,974 lines):
  - In the loops a stall is one of the item's failures. Outside them, parked work is marked
    stuck once it has sat as long as stranded work, and keeps its finding.
  - Gone: the budget of talk, the ceremony, carousel, redundant-wake and fence-push guards,
    `decide_stuck`, `huddle_view`, the guard modes and the CEO's rescue count.
  - Kept: the stuck and same-ask findings, the "woken for nothing" refusal (it saves money),
    the runaway tripwire and the day's cap on the house's models, and the talk-against-work line.
  - The team context is about 88,200 lines.
- **Found and fixed (e2b1cd37):** the review tool's approval still asked for a tester's role, so
  a loop reviewer could only send work back. Live, Ada could not pass #855 and asked the owner to
  record it. Now the member one rank up passes work on a reading of its own, and its pass closes
  it (gated work through the gate).
- **Released 790c1822 (parts C, D and E):** gate 3,581 tests; the drain cut nothing.
- **Step 8, part E, the chain of command's roles retired:**
  - Every member gets one tool list and one briefing: its name, its model's rank and its work.
  - Gone: the CEO's, leads' and tester's tool lists and briefings; the RACI; the CEO's seat,
    daily review and weekly planning; area reports and patterns; asking up a chain (every
    question goes to the owner's desk); the tester's and lead's review rounds (review is one rank
    up, and with nobody one rank up the owner reads it).
  - The demo mode's scripted team retired too. It walked work through those rounds, so HQ now
    always waits for a real runner, and its tests seed their own small team.
  - The Role type stays in the store for now; nothing branches on it in a turn.
- **Step 8, part F, first cut, the world's curation retired:** the nightly quiz, gaps and broken
  roads, members' proposals to pages, retraction and its sweep, and the notebooks, with their
  tables. Kept: the house and lab pages (every briefing now carries how the lab is built),
  read_world, the map and the history search.
  - Left for days 2–3: section stages and canon, stories and lessons, the world migration, and
    the dead Refusal variants and domain types.
- **The team context is about 71,100 lines** (from about 96,300 before step 8).
- **Found and fixed (4b869e9b):** work with no branch waited for a gate that never came, so
  #855 could not pass. Now only work with a branch needs the gate.
- **Step 8, part F, second cut, and the rest of the table (released 1429b0a5, no turn cut):**
  - Gone: stories, lessons and the world's move-in; ping-pong's and pool's scores (the
    animation stays); staffing offers, `request_staff` and `reports_to`; the guards' findings,
    verdicts and precision; goal strategies and area reports; sub-task limits, "serves" on a
    split and the "evidence already enough" nudge; the owner's furlough and recall (a retired
    member keeps its column); hire requests, review rounds and canon cards on the desk; the
    wing's seats; and the refusals nothing raises any more.
  - In the loops, three turns that show nothing count as one of the item's failures.
  - The wing's note (seeded, and the live goal's note) no longer names a tester and a lead:
    the member one rank up reads the work.
  - Kept: an approval still says which of its task's criteria it serves.
  - The gate passed each commit; the last ran 3,125 tests.
- **The team context is about 62,800 lines.**
- **Step 8, the last cuts (released bf823895, no turn cut):**
  - Gone: the member's write path to the world (no member has a tool that writes it) and the
    owner's canon (promote, demote and their buttons). The owner writes the written pages;
    HQ derives the rest.
  - The map's area and role pages and their stewards are gone. A member's map holds the house,
    the lab, the team, its goal and the projects.
  - Also gone: dead events, proposal states, seven wake reasons nothing raises, and the
    stories' scope.
  - Found and fixed: every map still told members to use the retired `report_gap`. It now
    points at `add_to_map`.
  - Zed's league game had ended, so the cut-over ran again and retired Zed. Five members are
    at work: Mara, Kit, Ada, Rae and Bo.
- **Step 8's exit check, as measured:** the gate passed after every part (3,118 tests at the
  last). The team context (`domains/team`, Rust) is **61,330 lines**, from 104,000; that is
  2% over the 60,000 aimed for.
  - What is left is the days 2–3 fold: the `Role` type, which still sets brains, budgets and
    effort; the CEO's branches that hang off it; the sections' stages and trust; and the tests
    that guard them.
- **Chess reviewed (the owner, 27 Oct), released 389decc3 and 2c21cecb:**
  - Every league move was legal and every win a real mate, checked by replaying the logs with
    python-chess. All four draws were the 60-half-move limit with one side far ahead.
  - No pawn ever reached the last rank, so no promotion came up. A new test shows the engine
    promotes however a model writes it.
  - Players were coached: every view and every refusal listed the legal moves. Worse, a game
    turn was shown "what this loop did lately", which held the other side's reasoning.
  - Now only the standard rules end a game (mate, stalemate, threefold repetition, fifty moves,
    too little material); no legal move is told; a game turn gets no loop notes or map lines.
  - The owner's reset: the league's 11 games set aside (kept), every brain at 1200 again. The
    Holodeck queued its 11 scenarios again ("before the standard rules, 27 Oct"); a re-run no
    longer queues old matches.
  - Still in a game turn: the work-turn system prompt, the house, lab and team pages, and the
    full tool list. A briefing of its own for games is the next fix.
