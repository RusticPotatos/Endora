# Agent collaboration: foundation integration checkpoint

2026-10-22. AI-assisted work on `codex/agent-collaboration`, following
[ADR 0137](../adr/0137-addressed-conversations-and-bounded-assistance.md) and the
[reviewed plan](agent-collaboration-plan-reviewed-2026-10-21.md).

Accepted final goal, 2026-10-23: [ADR 0138](../adr/0138-one-conversation-and-assistance-lifecycle.md)
requires full retirement of the old conversation execution framework, with one
canonical conversation/assistance lifecycle and retained, readable UI history.
Thin entry-point adapters and read-only history views may remain; two execution
engines may not. Useful work-triggered assistance is required, not spontaneous
downtime chatter. The older four validation gates below describe the previous
integration scope, not the new definition of done. Replacement is not implemented
yet, and no new deployment is implied.

Standing execution consent, 2026-10-23: the owner authorizes commits, normal
branch pushes and deployment to the existing home Endora system, and automatic
progression once each step's evidence meets its criteria. Follow the
[canonical instructions](../../AGENTS.md#standing-authorization-for-the-collaboration-replacement).
Do not stop for routine "continue", phase, push or deployment approval. Diagnose
failed checks, preserve evidence and continue ready work; deploy only after the
existing preservation, backup, migration, privacy and release gates pass.
Earlier per-run approval records describe history, not a requirement to ask the
owner again for every evidence-backed follow-up within the agreed limits.

Current branch checkpoint, 2026-10-23: typed forward provenance, atomic source
readers, bounded ordinary task briefings, scoped v2 task assistance, v2 worker
caller defaults and retained task-question inspection are implemented and
tested. HQ startup now cancels legacy requests and stops/settles their expert
turns before opening HTTP or seeding work; failure aborts startup. The old
application admission, claim, answer/fallback, dispatch and recovery implementation
and its production storage writer APIs have been removed. Authenticated v1 worker calls receive HTTP 410 with the v2
route; invalid or ended tokens still receive 401. The service candidate selects
the source-classified ordinary task context when messaging is enabled.
Protocol aliases now share the canonical policy. Real worker startup preserves
identity and input classification, and accepted help holds automatic task recovery
until delivery/verification ends or the request terminates. Implicit model-post
and final-word mentions no longer dispatch peer work.
A synthetic old-binary startup/backup/restore rehearsal now passes against
`25edbd06`, including active accepted-help recovery and the extended findings
schema. Actual browser inspection through the candidate node and HQ also passes
the checks described below. A task-owned model diagnostic now passes for the
frozen synthetic case. The production-backup rehearsal passed and `6e67eb6c`
was deployed to the NAS and the Pi on 2026-10-24, with history preserved; see the
[release record](agent-collaboration-deployment-2026-10-22.md#replacement-cutover-6e67eb6c-2026-10-24).
Messaging is enabled in production and the owner has authorized three
conversations on Home lab upkeep (`161e27da` fixed the authorize form first).
A work-triggered request observed in production, and device checks, remain
outstanding.

### Path to agentic operation, 2026-10-24

A review of the reviewed plan, ADRs 0137 and 0138 and production found the
replacement complete and live, but the team not yet agentic: no agent can ask for
help, and its real work is stalled. This is the ordered plan to the plan's end
state: real work leads an agent to ask a colleague, the asker checks the answer
against originals and records a finding, the owner sees it, accounting holds, and
the owner decides adoption on a matched comparison. It supersedes the "Next" lines
below. Every step keeps the Confidential guard; none relaxes a data tier.

**Evidence.** All 14 production turns after the first conversations were
authorized were marked Confidential, which withholds `consult`,
`request_assistance` and `send_message` (`protocol.rs` `tools_in`), while their
briefing still told them to use `consult`. The older task briefing marks every
task turn unclassified because legacy posts lack source lineage (`engine.rs`,
`unclassified_message_input`); it serves leads, testers, the CEO and rework wakes.
The classified task context serves only developers and researchers on five wake
reasons, and fails closed on unclassified pre-cutover inputs. Tasks without a
project have no help scope. Separately, all 8 open tasks were blocked, most since
18 October; the paired developer and researcher had no turn after 05:00.

**A. Make help reachable without weakening the guard** (engineering)

1. Brief only what is offered: the colleague paragraph appears only on turns
   that actually have the help tools; reword `ask_record`'s "before you consult
   anyone" so it does not compete. Red test: a Confidential turn's briefing omits
   it. **Done, `285bd8f5`:** one rule, `may_ask_colleagues`, decides the tool list
   and the briefing after the briefing records its inputs; full gate passed
   (2,360 tests) and it is deployed. Production confirmation is the next task
   turn's briefing.
2. Classify the older briefing's inputs one by one instead of locking the whole
   turn, keeping its role sections (leads' area picture and team, testers'
   review, the CEO's context, memory). An inventory (2026-10-24) found two more
   blanket locks: `memory()` marks every lead and CEO turn because the house page
   and stories are always briefed, and 64 `post_held_lock` sites store posts with
   Unknown origin, as do routine-fired task bodies and criteria. Slices, each
   test-first and released on its own:
   a. A `system` origin for HQ text that embeds no borrowed words (input_origins
      CHECK change, recorded here). Older builds fail on an unknown origin value,
      so release the reader (table rebuild accepting `system`) one release before
      anything writes it, keeping image-only rollback safe. **Reader released,
      `435b97d0`:** rehearsed on a private production copy, then deployed; the
      production table was rebuilt keeping all 23 proofs and its 7 guards.
      **Writers released, `dbe38226`:** 14 kinds of post HQ writes for a member in
      that member's own turn (status moves, verdicts, questions up, drafts,
      handovers, area reports, huddle views and decisions) record that turn;
      "Evidence cited" and the evidence-met submission record `system`; a CEO
      direction and project decisions (quoting others' text) and alarms stay
      Unknown. Re-tagging rule for the rest: a member's own words
      as its run, owner desk words as owner, pure templates as system; posts that
      quote borrowed text (alarms, "Evidence cited", recovered text) stay Unknown.
   b. Routine-fired tasks carry the routine's origin for body and criteria.
   c. The task briefing classifies each section with existing lineage (body,
      criteria, parents, goal, serves, handover, documents, area-picture titles,
      thread posts read with their origins in one snapshot) and fails closed only
      for goal notes, not-now, check-backs, huddle views, desk text, strategies
      and project text. Receipt excerpts shown in review mark the turn when
      Confidential (today only the blanket covers them).
   d. `memory()` classifies only the pieces actually briefed, by their recorded
      runs; the timeline read returns its run; legacy rows without one fail closed.
   e. The CEO's taskless sections are classified explicitly, since its marks are
      inherited by everything it writes.
   **Progress, 2026-10-25.** b. `e6208f05` (live): a routine records its words'
      origin (owner or HQ template) and its fired task's body and criteria carry
      it; alarm-fired tasks stay Unknown; pre-existing routines are vouched for
      only when their title and words exactly match an HQ template. c. `a43105c8`:
      the older briefing classifies body, criteria, parents, goal, served
      criterion, area-picture titles and thread posts by recorded origin (the
      whole thread where a question or review reads it), and fails closed on goal
      notes, not-now, documents, check-backs, member-proposed project titles, alarm
      patterns, check results, directions, ladder text and cited receipts.
      d. `b2945c8d`: memory pieces by their writers' runs (house sections, stories,
      the house timeline's shown lines via a new reader); everything else fails
      closed. A lead's clean task turn is now offered help (test updated from the
      old blanket expectation). e. Decided: the CEO's context stays fail-closed as
      one explicit section, because it reads desk text, goals' notes and lessons,
      and the lab's trouble, none with lineage; the CEO asks for decisions, not
      help, so nothing it needs is withheld. Released with c-d in `3dcd768b`.
   Tests: a lead's routine Home lab turn with only owner, system and Internal
   inputs is offered help; a thread post from a Confidential run, an Unknown
   pre-cutover post, an unattributed briefed house section and a Confidential
   receipt excerpt each still lock the turn; a dropped piece does not.
3. Measure before and after: replay recent production briefings offline and
   report the share of task turns that would be offered help, by role and wake.
   Exit: a non-zero, explained share on routine Home lab work that read no
   Confidential receipt.
   **Measured, 2026-10-25 night**, from production replays of runs 2300-2400 (about
   17:00-22:20 EDT, 101 turns): a turn is counted as offered help when its briefing has
   the colleague paragraph, which one rule gates with the tools (A1). **64 of 67 task
   turns of members other than the CEO were offered help (96%)**, against 0 of 14 before
   A1. By wake: every review_requested (14), question_answered (11), assignment_recovery
   (5), sent_back (6), acceptance_asked (3), escalated (2) and check_back (2). The three
   not offered: runs 2313 and 2314 (leads' assigned turns at 18:00 that read Confidential
   lab data, before ADR 0141), and run 2334 (a Holodeck chess side, which gets the scenario
   tool alone). Not offered by design: the CEO (0 of 12; A2e) and a colleague's answer
   turns (0 of 3; an answer does not ask again). Downtime: 18 of 19 (Zed's run 2367 read
   an open office proposal's free text, which has no lineage and fails closed).
4. Give unscoped recurring work a trusted project where the owner names one
   (decision D2), so its turns have help scope and budgets.
   **Progress, 2026-10-25.** The owner can now move an open task into an active
   project (`5baab417`, live in `e6208f05`). Used on the stalled unscoped work, on
   the evidence of each task's subject: #676 and #677 (the ansible-nas fork's
   secret defaults) into ansible-nas clean-up, which gives them its shell and repo
   tools; #673 (gluetun) and #678 (house page) into Home lab upkeep. Ada and Cy
   took #676 and #677 up within a minute. Recurring routines' project (D2) is
   unchanged.

**B. Unstick the team's real work** (engineering and operations)

1. Rae's Claude Code shell. **Already fixed, so decision D1 is not needed:** after
   18 October the worker allows Claude Code's whole `Bash` in the sandbox, the
   container being the boundary (ADR 0117), and a test cites task 674's failure.
   The Pi runs that worker. The owner replied in #674 on 2026-10-25; Rae's turn
   (run 2217) then failed before acting: one model call to the Mac brain took
   over the worker's 10-minute limit. Diagnosis: a ~25.9k-token prompt read cold
   while the Mac was out of memory (about 70 MB free, swap nearly full), reading at
   47–108 tokens/s and writing at 2–10. The retry hit the 30-minute turn limit; the
   next was interrupted by the `435b97d0` release drain and is queued again. Freeing
   memory for the Mac brain is done (prompt cache off, ChatGPT quit: free memory
   about 1.2 GB, swap 3.3 GB), but Rae's next turn (run 2245) timed out the same
   way: at her 25–31k-token context the Mac writes about 3 tokens/s, so a turn
   cannot fit its limits. Moving Rae's work brain to a paid model, or slimming her
   prompt and raising the call limit, is the owner's decision.
2. Check c1's exit 127. **Tools verified, 2026-10-24:** the Pi's current workbench,
   with the runner's in-memory `/work`, runs `ansible-lint` 6.14.6,
   `ansible-playbook` 2.15.13 and `yamllint` 1.30.0 from `/opt/venv/bin` on the
   path. The c2 re-run follows Rae's re-submission (step 1).
3. Escalations left undecided since 18 October (#672, #675, #677, #678 "to Mara:
   the lead did not decide it"): find why the CEO's ladder does not decide them,
   fix it, and clear or decide the backlog on evidence.
4. Put the unscoped open tasks (#673 gluetun restarts, #676, #677, #678) in their
   project, and give the idle developer and researcher work, so authorized pairs
   have reasons to talk.

**L. Make local-model turns small enough to finish** (owner chose free local models,
2026-10-25). Rae's #674 turn is about 25.9k tokens before acting: about 17k node
contracts (inferred), 4.7k HQ tools (23 for a developer; a lead's 38 are about
8.3k), 2.5k briefing and 1.5k worker instructions. The Mac writes about 3 tokens/s
at that size; the NAS GPU (12 GB) is slower at a 32k context. Anthropic and OpenAI
both advise a small up-front tool set (OpenAI: "fewer than 20 functions available
at the start of a turn"), deferring the rest behind tool search (Anthropic: worth
it past about 10k tokens of definitions, keeping three to five tools loaded), and
tight, non-overlapping descriptions.
1. Per-project tool lists (existing `set_project_tools`, node `only`): ansible-nas
   clean-up gets `shell, repo_checkout, repo_submit`; re-run #674 and measure. Add a
   Work control for the list.
2. For local brains on projects without a list: a small always-loaded core plus a
   worker-provided `find_tools` that brings in matching definitions.
3. Trim the longest HQ tool descriptions and check integrations for overlap.
4. Keep a turn's tool set stable so the Mac's slot reuses its prompt prefix.
Progress, 2026-10-25: steps 1 and 2 released (`8adb32c8`, NAS and Pi). A project
list may name `team` to keep members' own HQ tools and narrow only the node's;
ansible-nas clean-up lists `team, shell, repo_checkout, repo_submit`. Local turns
past 12 tools start with their core and the worker's `find_tools` (office and
chess stay reachable, as ADRs 0119 and 0121 decided). Rae's next #674 turn read a
7,381-token first prompt (was about 25,900) in 113 seconds, and its first call
completed in about two and a half minutes instead of timing out at ten.
Sources: anthropic.com/engineering (advanced-tool-use, writing-tools-for-agents,
effective-context-engineering-for-ai-agents); developers.openai.com function-calling guide.

**O. Owner decisions, 2026-10-25.**
1. One team, open conversation (the owner, citing Conway's law: everyone is on
   the same team). Any member may message or ask any other, without the owner
   authorizing pairs or a shared project; help is charged to the asker's task's
   project, or the team pool when it has none. Escalation when stuck goes up model
   tiers (the house's model, then Claude, as ADR 0131's work and judgment turns
   already do), not through a manager relay; the CEO-directs-leads rule stays for
   authority only. Keep the Confidential guard and the per-turn and per-day
   limits; Manage switches messaging off or blocks a pair. Amend ADR 0138; after
   A2. Then check that a stuck local member actually reaches its Claude tier.
   Evidence (2026-10-25): Anthropic routes "easy/common questions to smaller,
   cost-efficient models" and its advisor tool lets a cheaper executor consult a
   stronger model "when stuck"; OpenAI mixes models by task and escalates to a
   human on "exceeding failure thresholds" or "high-risk actions". Both caution
   against free-form multi-agent chat (Anthropic: agents are "not yet great at
   coordinating and delegating to other agents in real time"; OpenAI: "maximize a
   single agent's capabilities first"), so open permission keeps bounded asks.
   Audit: about 40% in place (assistance lifecycle, charging, work/judgment split,
   budgets). Refactor, in order: (a) an escalate step, the same member re-taking a
   stuck problem on the next tier (local, Haiku, Sonnet) as a judgment turn,
   capped per task and paid from the existing pool; (b) split ask_owner into help
   (peer or tier) and decisions (authority holder); (c) team-wide conversations
   with the project from the asker's task; (d) the stuck ladder: tier step before
   the lead, open huddles, no CEO hop for an undecided lead, a tier retry instead
   of handed_up; (e) colleague answers not forced onto the local model; (f) a new
   ADR, "One team, escalate by tier", amending 0111, 0113, 0120, 0130, 0131, 0137
   and 0138.
   **Progress, 2026-10-25.** (f) [ADR 0139](../adr/0139-one-team-escalate-by-tier.md)
   accepted (92deff9d). (a) `escalate` live in 2be0fed5 (NAS and Pi, no turn cut).
   (c) a consult from an active project's task finds or makes the pair's
   conversation on that project (5654a704). (b) live in 85abc47b as a split by
   tool: `consult` for any colleague, `escalate` for a tier up, `ask_owner` for a
   decision, in the tool text, briefings and v2 notes. A text-format rule was
   tried and dropped: 31 existing flows showed decisions asked as plain questions
   ("Steel or titanium?", "May I restart it?"), so text cannot tell help from a
   decision. (d) 56456617: the first rung wakes the member on its Claude (not for
   work in review, work already on Claude, past the two-a-day cap, or in a
   scenario, since the Holodeck measures the player's model); three turns of
   nothing go a tier up instead of to a manager, and in a lead's area the ladder
   then takes it; any member may add a huddle view; an undecided lead after the
   huddle stops at the owner's digest, not the CEO. (e) 24655463: the expert's
   answer turn follows the colleague's role instead of the house's model. Not
   done: an answer on a tier above the colleague's own work when the asker is on
   Claude (no judgment consult reason exists; it waits on evidence that work-tier
   answers fall short).
2. Holodeck re-run: archive the earlier round's scenario results (kept and
   labelled) and queue every scenario again, so the agents re-attempt the whole
   set and rounds can be compared. Deleting old results needs its own owner yes.
   **Progress, 2026-10-25.** `dee12c81`: the owner's re-run labels every finished
   scenario with the round's name (nothing deleted) and queues each distinct setup
   to start again one at a time, so it does not crowd out real work. Scenarios
   never go a model tier up (ADR 0139), since the Holodeck measures the player's
   model. Slimming: the always-loaded tools' descriptions are trimmed
   (`afba2b5a`); the rest of their size is schema, already deferred for local
   turns, and the set changes within a turn only when `find_tools` appends.
   UI: Work's conversation, scenario and trial lists are one-line rows, and
   Knowledge no longer scrolls sideways at phone width (`3dcd768b`).
   **Re-run started, 2026-10-25 ~17:15Z** on `46bb5f89` (NAS and Pi; the Pi's first
   attempt lost SSH after loading the image and was retried cleanly): 15 earlier
   scenarios labelled "round of 2026-10-25" and kept; 12 distinct setups queued,
   the first (a lighthouse, task 792) playing. **Production evidence, same day:**
   the new ladder's first tier rung fired on #677 ("Cy takes it a model tier up");
   Cy's turn on Claude found the real blocker (a researcher has no repo_checkout)
   and asked Kit, the lead, for access: a decision, by the right route. #678 was
   done (Ada, passed by Dee, accepted by Kit); #673 was submitted and fairly sent
   back. No peer `consult` has happened yet (C1 still open).
   **Why none had (2026-10-25 evening):** replays of every turn after `3dcd768b` showed
   none offered `consult`. Real work reads the lab (`container_logs`, `home_assistant`,
   lab status), whose results are Confidential, and ADR 0138 withheld help from any
   Confidential turn. The owner's decision, [ADR 0140](../adr/0140-confidential-asks-stay-on-the-house.md):
   a Confidential turn may ask, and the colleague answers on the house's models; work
   turns always are (ADR 0131), and the colleague's and the asker's result turns are
   refused to a turn on Claude as a second check, each inheriting the mark.
   `send_message` stays closed to Confidential turns. Two smaller lineage gaps remain
   open (a goal's not-now strategy and huddle documents have no recorded writer).
   **Later the same evening:** `consult` joined the local turn's always-loaded core
   (`fe6c7c86`; behind `find_tools` no turn used it). The owner's activity no longer
   holds the Mac (ADR 0130 amended, `eec83bb0`; production's stored 10-minute wait set
   to 0): with nearly all work on the Mac, the team had stopped whenever the owner used
   it. The lineage gaps are closed (`ed98f50b`): a goal's not-now records its writer,
   and task documents are read by their recorded run. A colleague now answers on a tier
   at least the asker's (`55b99a7e`): a new `ConsultedUp` wake, judged on the expert's
   Claude, when the asking turn is on Claude and read nothing Confidential. HQ is four
   tabs (Now, Team, Work, Manage) with one-line rows throughout.
   **C1 and C2, 2026-10-25 ~20:15Z:** the first real work-triggered help request. Ada,
   on #791 (the home-lab check, run 2335), asked Dee which receipts to cite in
   `set_status`; the pair's conversation (#4, Home lab upkeep) was made on the spot
   with no setup (ADR 0139), and the request (#1) showed in Work. It **expired**: Dee's
   answer turn (run 2336), as a tester's, was pinned to the Mac and sat seventh in its
   queue past the deadline, a real failure outcome shown in Work (C2). Fixed in
   `898a809f`: a colleague's answer takes whichever house model is free. An answered
   request in production is still to be observed; C3–C5 remain the owner's.
   **Holodeck, chess and the model league, 2026-10-25 ~21:00–22:10 EDT:** Confidential
   is open to Claude (ADR 0141). The Holodeck re-run was started over on today's engine
   (`c87780e0`; 12 setups queued, the stuck match #801 dropped and kept). `chess` joined
   the local core (`6f3cb2f1`), and a downtime turn whose move is due is about the move
   alone (`24cefe89`; the house model, offered the office too, left it alone every turn).
   The owner's model league (`35016dfb`, `5e9c5d66` seating fix; ADR 0135 amended): project 5
   "Model league", capped at $10 a month, brains Local, Mac, Claude Haiku and Claude
   Sonnet (Opus is the CEO's alone and the CEO does not play), 6 pairs, Elo; its first
   game waits for the re-run queue. Next: see the office game's first move, and the
   league's first game.

**C. Prove it and decide** (observation, then owner decisions)

**Open items decided, 2026-10-25 night** (the owner asked for them to be answered on the
evidence; CLAUDE.md rule 5):
- **B3, settled.** #672, #677 and #678 met their criteria and closed once the ladder was
  fixed (ADR 0139). #676 shows a tier escalation working in production: three empty turns,
  then "Ada takes it a model tier up", and Ada on Claude used #677's list and moved on.
- **#677 repo access, moot.** Cy finished #677 without it.
- **Rae's work brain stays the Mac** (free). Since slimming (L1-L2) her turns finish: #671
  and #674 are in review and #675 is being done. Revisit only if a turn times out again.
- **D2.** A lead's standing reviews ("State of my area", "Patterns") belong to the lead's
  area project: Ada's to Home lab upkeep (1), Kit's to ansible-nas clean-up (2), giving their
  turns help scope and the project's budget. The CEO's "Daily review" and "Planning" stay
  unscoped and are paid from the pool, since the CEO's context stays fail-closed (A2e) and the
  CEO asks for decisions, not help. This needs an owner route that sets a routine's project
  (none exists).
- **D3.** The comparison spends no new money: (i) offline, recorded production outcomes
  since the cutover against the retained consult and manager-relay baseline; (ii) one
  Holodeck trial on the house's models (free): Silent Station crews with and without
  `consult`. A frozen cap of $5 from the pool applies if a Claude tier is used.
- **D4.** Adopt provisionally: the replacement is live and the old paths are retired. Record
  it as final in ADR 0138's status when C4's comparison is not worse; revise if it is.
- **C3.** Checked at phone width (375 px) in the browser pane; a check on the physical phone
  stays the owner's, and is optional.
- **Left with the owner** (outside actions on the NAS, which only the owner runs): desk 338
  (run two host commands so the team can name what stops gluetun) and desk 339 (merge
  `hq/task-672-role-readmes` on the fork and run the playbook).

**Loop plan, 2026-10-25 night**, each step test-first where it changes code, full gate,
release, production check, record:
1. A3: replay production briefings offline and report, by role and wake, the share of task
   turns offered help.
2. D2: an owner route that sets a routine's project; set Ada's and Kit's reviews.
3. C3: Work at 375 px in the pane; fix what is hard to read or overflows.
4. C1: an answered help request in production, end to end (answer, finding, task section,
   feed, accounting); diagnose if none comes within a day of real work.
5. The league's first game (pinned brains, Claude paid within its cap) and office chess
   progressing.
6. C4 per D3; then D4 recorded in ADR 0138.
7. Merge to develop after a privacy scan; record in the checkpoint and memory.

**Loop progress.** 1. A3 done (above). 2. D2 done: `set_routine_project` and its routes
(`4fd13066`, `f96fd8e7`, live); routines 4 and 5 (Ada) moved to Home lab upkeep, 6 and 7 (Kit) to
ansible-nas clean-up; the CEO's 2 and 3 stay unscoped. 3. C3 done: at 375 px Work has no
sideways scroll and is 4.5 screens; one-line rows' details were 10.4 px, now 12 px and muted
(a console check holds it), and the Holodeck card shows three scenarios before Show all
(`f96fd8e7`, live; that release's drain cut one turn).
4. C1, in progress: requests #2 and #3 (Cy to Dee on #677) were declined: Dee's local answer
turn had `answer_assistance` behind `find_tools`, and the question named `r877` without passing
it. Fixed in `470e1649` (live; ADR 0138 amended): the answering tools are always loaded, and a
consult naming unpassed sources is refused with the `evidence` to pass. Waiting for the next
real request.
5. Done, 2026-10-26 07:10: the re-run queue drained (the Holodeck chess match #810 ended at
the move limit; the last Silent Station, #827, succeeded) and the league's first game started,
task 830, Local (white, Rae) against the Mac (black, Dee); replays show every white turn on
`local` and every black turn on `mac`, whatever the players' own brains. Office chess had 31
half-moves. A Claude side's payment from the league's cap is checked when Haiku first plays.
Overnight the NAS stalled in a swap storm (about 03:50-04:10; load 467, swap 3.2 of 4 GiB, Plex
Transcoder the largest process) and recovered on its own; Endora rode it out.
Then game 830 stalled at 57 half-moves: Dee's black turn waited "for tomorrow" at the day's
60 free turns, spent on the night's Holodeck match and this game. Fixed in `95657412` (live,
ADR 0133 amended): a scenario's turns are bounded by its world, not its player's day, and the
tripwire still counts them. Dee's turn started at once after the release.
Game 830 ended drawn at the move limit (Local and Mac 1200, a draw each). Game 835, Local
against Claude Haiku, started at once; by 12:15 the Model league project had spent $0.014 of its
$10 cap, so a Claude side is paid from the league's capped project as designed.

1. Observe the first production request triggered by real work; verify its
   answer, finding, task section, feed lines and accounting in Work.
2. Observe a real failure outcome (expired, declined or cancelled) displayed.
3. Physical phone check of Work (owner).
4. Phase 6 matched comparison against the retained consult and manager-relay
   baseline, within a frozen budget (decision D3).
5. Record the owner's adopt, revise or disable decision (decision D4); ADR 0138
   is complete only then.

**Order.** A1 and B1–B2 first (independent and quick), then A2–A3, then B3–B4
and A4, then C. Each step: red test or reproduction, fix, full gate, release with
rollback tag and snapshots, production check, record.

**Owner decisions.** D1 is settled by the existing ADR 0117 decision; D2 which project owns unscoped recurring work; D3 the comparison budget;
D4 adoption. A Confidential turn sending only source locators, with no free
text, would be a data-tier change and needs its own ADR before it is proposed.

The requester-result finding is now retained with the canonical request and
shown in the existing Work history/export payload, rather than depending on an
expiring transcript. The repeated synthetic migration and initial actual browser
proof and task-owned model diagnostic are complete for the scopes below. The
remaining release checks must pass before deployment.

### Extended candidate migration and actual browser inspection, 2026-10-24

Rebuilt actual HQ/node/worker binaries from `25edbd06`. The sequential synthetic
upgrade, reopen, active recovery and compatible backup/old-binary restore pass
against the extended findings schema, preserving original rows and links. Old
requests explicitly return `finding: null`; migration invents no conclusion.
This uses the captured older pilot binary, not a current NAS backup/release.

Actual browser inspection through candidate node/HQ verifies migrated history,
ordered pages of 50 and 1 messages without overlap, empty history, cancelled
assistance, attributed expert/requester findings, escaped markup, invalid-source
feedback and original run replay. The task inspector retains original legacy
questions/answers and links. The first UI finding was explicitly seeded. The
later model-authored finding was displayed and its actual downloaded JSON
independently compared with the store, `r1` citation and run IDs. A stopped copy
also displays the completed model result while messaging is disabled. Its
message-page download matches the explicitly labelled synthetic message and run
ID, and an explicitly seeded failed request displays no invented answer/finding.
That rendering fixture is not a live model failure. Owner deletion
immediately hides a synthetic conversation; bounded maintenance then removes
messages, requests and findings with no foreign-key errors. The first checker
wrongly expected immediate physical purge; the corrected check follows the
existing tombstone/batched-purge contract.

The operator HTTP preflight passes claim, canonical admission, expert original
read/answer, requester independent original read/finding and unchanged task
ownership/status, including the worker's final status post. Initial fixture
failures are retained: a stopped task's wake was refused, and an empty project
allow-list correctly excluded peer tools. Explicitly granting only `consult`
and `read_messages` fixes the fixture without relaxing production policy or
granting an external capability.

The first live attempt proves model initiation but fails the experiment guard:
one worker, five calls, 9,292 prompt / 758 completion tokens, 72.81 seconds.
The guard rejected the worker's automatic final task post. Before another live
attempt, replaying all five captured model replies through the real worker and
explicitly fake transports reproduced exit 1, then passed with exit 0. The guard
now allows only the original task's bounded `last_word: true` post after the one
admission, without advertising post to the model. Other tasks, stages and status
changes are refused. The actual HTTP preflight and 26 harness checks pass.

The fresh live trial completes in 303.76 seconds: three workers, 14 application
calls, 26,049 prompt / 3,233 completion tokens, no extra control completion. The
model initiates exactly one request from its automatically claimed owner task.
Expert and requester independently read byte-identical original replies. Both
stored conclusions explicitly name health-check-v1, expected 200, observed 503
and unavailable fixture-cache, distinguish causal inference from co-occurrence,
and leave other causes unknown. The requester says restoring the dependency is
not proven to resolve the failure. The finding is retained with the request.
The task stays owned and Doing; there is no acceptance, legacy request, paid
charge, repair claim or external action. This passes one frozen synthetic case.

Across both live attempts, count four workers, 19 calls, 35,341 prompt / 3,991
completion tokens, including the failed harness attempt. Initial requester
polling used all six rounds and counts in the totals. As frozen, the operator
suppressed one synthetic idle lease without starting a model; this is not proof
of unassisted production scheduler liveness. General reliability and a matched
performance improvement are not established. Cleanup stopped/disabled the
isolated stores, disarmed execution and closed owned processes/listeners. The
resident process and argument fingerprint are unchanged.

Ignored evidence: `.local/replacement-migration-25edbd06/`,
`.local/replacement-ui-25edbd06/` and
`.local/replacement-task-pilot-25edbd06{,-v2}/`, including evaluation, original
traces, captured-response red/green replay, browser screenshots/downloads and all
failed attempts. Final stopped-history/message-download/failure-display evidence
is in `.local/replacement-ui-final-25edbd06/`; those services also shut down with
no workers or inference. No NAS deployment occurred. Tracked executable code and release
configuration are unchanged; reuse the passing `25edbd06` full gate (954 team
tests, 76 HQ API tests, 158 scenarios / 1,980 steps), with fresh documentation
privacy, relative-link and whitespace checks before push.

Production-backup rehearsal and the NAS/Pi cutover are done (see the
[release record](agent-collaboration-deployment-2026-10-22.md#replacement-cutover-6e67eb6c-2026-10-24)).
The [caller audit](agent-collaboration-caller-audit-2026-10-24.md) found no
violation of items 1 and 2; its follow-ups are done.
Next: the [path to agentic operation](#path-to-agentic-operation-2026-10-24). The full ADR 0138
definition of done remains open until those pass.

### Legacy cancellation preparation

`HistoryStore::retire_legacy_consultations` cancels at most fifty pending old
questions in one immediate transaction and returns at most fifty still-live,
taskless legacy expert turns for application cleanup. It preserves IDs,
questions, authors, project snapshots, answer links and notification history.
The cancellation explanation appends to an existing explanation. Cancelled
records remain visible through the retained-history reader; they create no
answer, record fallback, replacement work or requester notification.

The selection uses exact request/delivery associations and original wake reasons,
excluding canonical expert/result associations and all task-bearing turns.
Unfinished cleanup remains discoverable after reopening the database. The port
does not stop a process, settle accounting or revoke credentials: its documented
precondition is an application fence against old admission/dispatch. This first
storage preparation did not yet have a startup caller; the application cutover
below supplies it before publishing the protocol.

Seven focused tests cover preserved history and canonical work, cancellation
batches, bounded run batches, rollback, database reopen and archive growth.
The initial notification test caught cancellation being treated as an answer;
the growth test then caught notification scans walking cancelled history.
Filtered answer indexes correct both paths without falsely marking cancelled
requests as notified. The actual cancellation, live-run and notification SQL has
identical VM-step counts with 1,000 and 100,000 completed/cancelled archive rows.
Removing the live-run indexes makes the negative control exceed 100,000 steps.
This demonstrates bounded query work, not a wall-clock speed improvement.
Evidence is in ignored `.local/legacy-retirement-{red,notification-red,growth-red,green}.log`.
The full repository gate passed: formatting, workspace Clippy with denied
warnings, workspace all-feature tests, console, personal-value, collection
parity and whitespace checks. Logs are in ignored
`.local/legacy-retirement-gate/`. Handheld code did not change, so separate
device checks were not run. No model call or deployment is part of this
preparation.

### Application startup cutover

`Engine::retire_legacy_peer_work` holds the application lock, repeats bounded
cancellation batches, stops running legacy experts and ends each selected turn
through normal accounting and wake settlement. Cleanup preserves ordinary task
runs and canonical expert/result associations. A fault after accounting but
before run closure aborts startup; reopening and repeating cleanup settles the
remaining run without duplicate charges/releases or an invented answer.
The v1 HTTP boundary authenticates first and refuses entire batches with a
specific upgrade response. The sole served RPC dispatcher calls v2. Old task
consultation execution was replaced by a thin canonical adapter and explicit
retired-turn/answer refusals; no application record-fallback scheduler remains.

The source-classified ordinary task briefing is selected in the service candidate.
The rollout selector remains for the context migration tests; deleting it still
requires checking retained task/owner/review responsibilities. It does not select
between two executing consultation engines. At this application checkpoint,
legacy SQLite admission/completion writers were still present without production
callers; the storage cleanup below removes them. Task-post mention wakes and old protocol policy branches also need
the final responsibility audit. This is not full ADR 0138 completion or release
approval based solely on the focused tests.

Obsolete task-thread execution scenarios were replaced by retained-history
cleanup/restart coverage; explicit record search, transcript retention, privacy
and current assistance scenarios remain. A mixed quota test now imports retained
legacy records instead of calling a removed engine, and proves the seventh
canonical daily request is refused. The first transport check caught a stale
tool-count expectation after disabling legacy consultation advertising; the
test now explicitly checks that disabled peer tools are absent.
The retired-turn guard also exposed a whole-wake-history lookup. A primary-key
wake reader replaces it, with corruption isolation and archive-growth tests.
The final full repository gate passed: formatting, denied-warning workspace
Clippy, workspace all-feature tests (946 team tests, 76 HQ tests, and 158 team
scenarios / 1,971 steps), console, personal-value, collection-parity and whitespace
checks. Logs are in ignored `.local/peer-cutover-gate/`; the first transport-gate
failure is preserved there as an explicitly transcribed excerpt. Startup red/green
evidence is in `.local/startup-retirement-{red,green}.log`, and focused quota,
scenario, HTTP and wake-reader evidence uses the `.local/peer-cutover-` prefix.
Only prose/comments changed after that executable-code gate, followed by fresh
format, personal-value and whitespace checks. Handheld code did not change;
separate device checks were not run. No model call, NAS change or deployment is
included.

### Legacy storage execution removed

The production history port and SQLite adapter no longer admit, claim, complete,
save or deliver old consultations, query their work queues, or manufacture
fallback posts/events/knowledge effects. The old domain's execution policy is
also gone. Retained tables, IDs, question/answer links, project snapshots and
historical outcome readers remain. Startup's bounded cancellation is the only
production mutation of these legacy request rows. A separately named fixture
import exists only under `cfg(test)`; the behavior harness imports pre-upgrade
rows directly into its synthetic fixture database, without a production writer.

The last-slot race now runs two canonical requests in distinct authorized
conversations after one retained legacy acceptance, proving that only one can
spend the remaining allowance and replay cannot charge it twice. Archive-growth
checks retain the actual old-usage SQL used by canonical admission. Historical
project attribution is exercised through reserve, use, settlement and late use
after the current task project changes, without executing an old admission path.
Tests for the deleted execution APIs were removed; retained-history, migration,
accounting, privacy and canonical lifecycle coverage remains.

The full repository gate passed: formatting, workspace denied-warning Clippy,
workspace all-feature tests (936 team tests, 76 HQ tests, 158 team scenarios /
1,972 steps), console, personal-value, collection-parity and whitespace checks.
Ignored evidence is in `.local/legacy-writer-removal-gate/`. Only documentation
changed after the executable-code gate; fresh privacy/whitespace checks followed.
No model call, device change or deployment is included. Protocol aliases, the
ordinary-context selector and task-post wake responsibilities still require
cleanup/audit before the final migration/UI/model and NAS release gates.

### Canonical protocol and actual worker startup

The familiar `call` and `tools_in` entry points now use v2's policy. The temporary
canonical/legacy booleans and alternate task-owned consultation branches are gone.
Disabled help reports that it is disabled, rather than suggesting the retired
task-thread detour. Retired expert detection is an internal guard, not a lifecycle.

Reviewing both actual worker adapters exposed two startup defects that the earlier
direct-admission tests missed: `member_of` reads identity from the first line,
which the v2 banner had displaced; and automatic `claim` was being treated as an
unclassified input. Version guidance now follows the briefing, preserving the
first line and near-top owner constraints, with drain notices still last. The
exact claim response contains only its checked task ID. Its acknowledgment leaves
input classification unchanged; Unknown and Confidential test cases stay closed.

Replaying that real startup also reproduced an integration failure after the
task became Doing: ordinary recovery entered retry-wait while its expert was
answering, blocking requester notification. A minimized public-use-case test
reproduced this deterministically in 0.04 seconds. Recovery now reads canonical
request/delivery state and waits for pending help or live result verification.
A deliberate wait refreshes recovery bookkeeping; completed result turns,
cancellation and expiry release it even for previously claimed/nudged work.
Tests preserve ownership and Doing status, with one ordinary recovery afterward.
No peer answer completes a task or replaces independent acceptance.

The metadata query uses an active-request index and live-run lookup. Its actual
SQL has identical VM-step counts with 1,000 and 100,000 completed request/run
archives; removing its supporting indexes makes the negative control scan the
archive. Different tasks/members and taskless source runs do not inherit a hold.
The original end-to-end task-help scenario now passes through automatic claim,
expert answer and requester result delivery. This is offline evidence, not yet
proof of model initiation or final-finding quality.

The full repository gate passed: formatting, workspace denied-warning Clippy,
workspace all-feature tests (940 team tests, 76 HQ tests, 158 team scenarios /
1,972 steps), console, privacy, collection-parity and whitespace checks.
Evidence: ignored `.local/canonical-aliases-gate/`,
`.local/canonical-aliases-{red,green}.log`,
`.local/canonical-worker-startup-{red,green,first-fix-failure}.log`, and
`.local/task-help-{wait,resume}-{red,green}.log`. An earlier gate's two expectation
failures are retained in `first-tests-failure.log`; the owner-note assertion was
restored after fixing first-line compatibility. The diagnosis skill's temporary
instrumentation check found none to remove. Only documentation changed after the
final executable-code gate, followed by fresh privacy/whitespace checks. No model
call, device change or deployment is included.

### Task-post dispatch retirement

Task posts and worker final words retain their original text, attribution and
history, but member mentions schedule no peer work. Only an explicit
owner-authored post may dispatch mentions; generated text does not gain that
authority from its displayed actor. The tool description no longer tells models
to wake colleagues through posts. Canonical assistance is the peer-help path.

Actual management escalation still explicitly schedules `QuestionAsked`, and
management replies still return `QuestionAnswered` even when they mention their
requester. Owner requests, board decisions, task ownership and independent tester
acceptance retain their responsibilities. The complete board/manager/developer
round-trip test now includes named replies to catch a lost or duplicate dispatch.

Focused tests reproduced both ordinary-post and worker-final-word dispatch before
the fix. The history scenario now checks that a hostile model post schedules
nothing, then uses an explicit owner request to read the same retained, cleaned
post. An older protocol fixture also depended on implicit peer dispatch; it now
uses an explicit owner request while preserving its decision-recipient check.
Ignored evidence is in `.local/thread-wakes-{red,team-first,scenario-green}.log`,
`.local/final-word-wake-red.log` and `.local/thread-wakes-gate/`.
The first scenario command had invalid harness arguments and ran no scenario;
the corrected command passed all 12 steps. The full gate then passed: formatting,
workspace denied-warning Clippy, all-feature tests (942 team tests, 76 HQ API
tests, 158 team scenarios / 1,973 steps), console, configured privacy,
collection-parity and whitespace checks. Its earlier failed scenario expectation
is retained in `first-tests-failure.log`. Only documentation changed afterward.
No model call or deployment is part of this slice.

### Scope discovery across retained history

A focused test reproduced a valid task project's conversation being hidden from
the worker tools when fifty earlier scopes belonged to another project. Discovery
now uses an indexed metadata existence check against the authenticated member,
trusted task project and active project state. It does not use the first history
page as policy. Admission still independently revalidates the chosen scope;
discovery grants no participant, evidence or payer authority.

The lookup checks either fixed participant position, excludes deleted scopes and
inactive projects, and returns only a boolean. The actual task-linked and taskless
SQL performs identical VM work with 1,000 and 100,000 inactive-project/deleted-scope
archive rows; the missing-index negative control scans the archive. This does not
claim constant cost as active work grows or global scheduler latency invariance.
Focused admission/discovery and storage tests passed. Ignored evidence:
`.local/tool-discovery-page-{red,green}.log`,
`.local/tool-discovery-storage-green.log` and `.local/tool-discovery-gate/`.
The full repository gate passed: formatting, workspace denied-warning Clippy,
all-feature tests (945 team tests, 76 HQ API tests, 158 team scenarios / 1,973
steps), console, configured privacy, collection-parity and whitespace checks.
Only documentation changed afterward for this slice. No model call or deployment
is included.

### Context responsibilities and removal of the migration selector

`canonical_task_briefings` is removed from Settings, the service composition root
and test fixtures. The existing owner messaging switch selects source-classified
context for ordinary developer/researcher assigned work, retries, recovery and
owner replies. Messaging-disabled work retains its world/notebook responsibilities;
specialized management, review and decision contexts retain their conservative
guard. This is input-policy selection, not another conversation framework: the
retired consultation lifecycle is unavailable in either context.

The responsibility audit reproduced two missing inputs on the classified path:
actual management routing and the restart handover. Both are now retained.
Handover reads select the recipient's latest note, otherwise the task's latest,
using bounded indexed branches shared with the existing memory reader. Its
original source run determines privacy; missing/unknown lineage fails closed,
and Internal/Confidential source cases preserve their tier. Notes remain stored
unchanged. The actual SQL has equal VM work at 1,000 and 100,000 retained notes,
with a missing-index scan negative control. This does not establish bounds for
all existing scheduler/history reads.

A separate red test caught owner goal notes appearing too late in the classified
briefing. They now remain near the top without displacing worker identity, and
their older unsourced field still closes peer sharing. No constraint is dropped
or relabelled to make a model trial pass. The restart note precedes task bodies;
ordinary peer help is distinguished from actual authority questions.

The initial experiment forcing the smaller context onto messaging-disabled work
broke existing world/notebook responsibilities; that failed result is retained.
The final policy uses the real owner switch without a separate migration option.
No old behavioral tests were deleted or weakened for this change. The team suite
passed 949 tests, and 13 focused canonical tests passed after the owner-note fix.
Ignored evidence: `.local/context-responsibilities-{red,green}.log`,
`.local/context-owner-note-red.log`,
`.local/context-selector-forced-everywhere-failure.log`,
`.local/context-policy-{team,focused-green}.log`, and `.local/context-policy-gate/`.
The final repository gate passed: formatting, workspace denied-warning Clippy,
all-feature tests (949 team tests, 76 HQ API tests, 158 team scenarios / 1,973
steps), console, configured privacy, collection-parity and whitespace checks.
Only documentation changed afterward. No live model call or deployment is
included. Next rehearse versioned startup/history preservation, then freeze the
actual task-owned model envelope and verify UI/cutover evidence.

### Synthetic versioned startup and compatible restore rehearsal

The actual HQ binary at `c8f9cd0d` was exercised against a synthetic database
created by the captured, hash-verified older pilot binary `f59e1ec`. This is not
a rehearsal against the current NAS backup or a production deployment. Separate
SQLite backup copies were opened sequentially; no two engines shared a store.
The application sandbox denied outbound networking and writes outside its trial
directory. No runner connected: physical worker starts and model calls were zero.

Stopped and active startup, plus a second candidate startup for each, passed.
Original member/goal/task rows, posts, message bodies, transcripts, legacy answer
and project links, delivery associations and immutable canonical-request fields
were preserved. The older inputs acquired no fabricated source lineage. The
legacy queued question became visibly cancelled and its expert stopped. In the
active case, ordinary and canonical expert runs entered bounded platform retry;
the same accepted request stayed processing with the same expert association.
In the stopped case, the existing global Stop policy intentionally cancelled
canonical help and stopped its expert. Free-run accounting remained unchanged
across reopening; this rehearsal does not claim to test nonzero paid settlement.

The actual owner history HTTP route returned the cancelled question and original
completed answer with exact IDs and answer text. Valid v1 worker authentication
received 410, invalid authentication received 401, and the retired expert's token
was refused. Restoring the compatible older binary/database pair also passed;
the restored legacy question remained queued rather than inheriting the newer
database's cancelled status. Every temporary HQ process was terminated and waited
for before another was started.

Evidence is in ignored `.local/replacement-migration-c8f9cd0d-v5/`, including
`result.json`, binary hashes, profiles, database copies and the rehearsal script.
Four earlier trials are retained separately. Their fixture/checker failures were
a missing required message key/day, default member migration and an incorrect
global-Stop expectation, an incorrect expectation of zero-value accounting rows,
and epoch-era running timestamps causing normal timeouts. Offline checker and
timestamp precondition controls preceded the corrected final trial. No application
code was changed to make those fixture expectations pass.

The unchanged candidate reuses `.local/context-policy-gate/` for repository checks;
fresh documentation privacy/whitespace checks follow. Remaining release evidence
includes actual Work UI interaction, a frozen task-owned model pilot with a
complete independently grounded finding, the production-backup/NAS rehearsal and
the final controlled deployment/enablement/rollback decision.

### Retained requester findings, 2026-10-24

The UI/retention audit found that expert answers were retained, but the requester's
own final finding existed only in its expiring worker transcript. ADR 0138 now
spells out the existing lifecycle's retained-finding contract. The associated
requester-result turn uses `record_assistance_finding`; ordinary turns and experts
cannot write it. This adds no queue, wake, grant, payer, task completion or owner
approval authority. The owner sees an attributed claim, including an inconclusive
finding when original evidence is unavailable, not a deterministic truth verdict.

One bounded finding is stored with its authenticated result run, original source
locators and time. An immediate transaction revalidates current requester and
source-run identity, conversation/project membership, readiness revision, owner
switch/Stop state, input privacy, and cited original-source grants and revisions.
Identical live retries reuse it; conflicting reports cannot replace it. Two
concurrent reports retain one finding and one content-free operation record.
No finding body enters the general activity feed. A previously recorded finding
allows a recovering result turn to finish without another model attempt.

The existing Work request card escapes the finding and locators, labels it a
claim, follows original requester/expert/result run links, and explicitly shows
when no retained finding exists. The existing request-page export carries the
same payload. Old findings are never inferred from answers, completion status or
transcript fragments. Transcript pruning leaves the retained finding intact;
explicit owner deletion removes it in the existing bounded purge.

Red-first public-use-case checks reproduced the missing report tool, incomplete
deletion, mismatched original requester, and an unnecessary model restart after
a saved finding. The final code passes those cases. Storage tests cover current
privacy/authority/source refusals, immutable replay and a real two-connection
race. The actual primary-key lookup has identical VM work with 1,000 and 100,000
retained findings, with an unindexed scan negative control. The owner scenario
passes all 18 steps through reporting, transcript expiry and explicit deletion.
Component checks show the old UI omits the finding and the new UI escapes it;
this is not yet the actual browser smoke test.

The complete repository gate passed: formatting, workspace denied-warning Clippy,
all-feature tests (954 team tests, 76 HQ API tests, 158 team scenarios / 1,980
steps), console, configured privacy, collection-parity and whitespace checks.
Ignored evidence: `.local/retained-finding-*.log` and
`.local/retained-finding-gate/`. The earlier passing gate before the recovery fix
is retained separately. Only documentation changed after the final gate, followed
by fresh privacy/whitespace checks. No live model call or deployment occurred.
The earlier `c8f9cd0d` migration rehearsal must be repeated against this extended
candidate before release; the actual browser and task-owned model envelope are
the next ready validation work.

Earlier integration checkpoint: ordinary consultation is integrated with authorized
taskless assistance; task-thread consultation now has durable delivery,
idempotent admission, shared allowances, trusted expert project attribution and
atomic answer effects. These close the implementation gaps listed in the
earlier checkpoints below. The first authorized live diagnostic then failed:
its initial prompt exhausted the frozen context before any consultation was
accepted. The [result](agent-collaboration-pilot-result-2026-10-22.md) records one
local call, zero paid charges and the subsequent worker token-limit correction.
The later shared-runtime execution pilot passed its bounded flow criteria with
an answer-quality caveat; see the current result below. Operational completion
still requires quality/initiation evaluation, matched comparison and
release/device gates. The latest changes are not deployed.

The authorized 16,384-token follow-up avoided truncation but was blocked by the
private experiment guard's exact-question and discovery restrictions before
application admission. The [follow-up record](agent-collaboration-pilot-followup-2026-10-22.md)
distinguishes this harness defect from application behavior: an offline replay
through the actual protocol accepted the captured question and reused its
request on retry, without a carrier task or model start. The owner then
authorized the corrected-guard rerun. It ended after two local model calls with
no consultation proposed: the requester treated the owner notes as already
served and followed the downtime briefing's option to stop. There was no
truncation or guard refusal. All services stopped and conversations were disabled;
no production change occurred. The three-attempt series stops for offline review
of the initiating wake, owner objective and briefing before another live attempt.
A live assistance result is still unverified.

The next offline slice reproduced a concrete briefing inconsistency: the final
downtime tool summary omitted conversation tools already advertised to the same
authorized turn. It now lists them only for an enabled participant with a scope,
and downtime says no task is due rather than nothing is due. The regression also
checks disabled/absent scopes and continued refusal of task creation. This is a
briefing correction, not proof that a model will initiate the diagnostic. The
pilot still supplied its objective through retained notebook notes during an
optional downtime turn; it did not create a current owner assignment. No new
wake or scheduling contract was added to make the experiment pass.

The next [execution-only check](agent-collaboration-execution-pilot-result-2026-10-23.md)
used operator admission to separate model execution from model initiation. It
stopped before a worker or inference call: the expert was incorrectly marked
shell-eligible. A regression confirmed the application policy gap. Explicit
experts and their verification-result turns now have no shell or node tools;
verification also has read-only handler enforcement and a six-read-round limit.
The cancelled attempt is preserved. No guard relaxation or replacement request
was used. Live execution and autonomous initiation are still unproven.

The fixed-policy follow-up then passed an actual HTTP assignment/source-reading
rehearsal with no model or worker. Its live expert started with the correct
restrictions, but the first model call failed with a GPU insufficient-memory
error before a response. The failed call and cancelled request are retained;
production and other model processes were untouched. Runtime GPU computation
readiness is now the immediate gate before another request, ahead of validating
the expert/result model exchange. Host memory readings alone did not predict
the allocation failure. The full application gate for `64a4253a` remains passed.

Runtime diagnosis reproduced the limit after a successful four-token control;
the normal expert call still failed. Smaller physical batches did not establish
an improvement. Further duplicate-model attempts have stopped. The execution
record proposed reusing the already resident, identically pinned model with
isolated application/data and an explicit shared-runtime boundary. The owner
accepted shared model capacity and weaker model-process isolation. No resident
service was changed or stopped.

The [shared-runtime result](agent-collaboration-execution-pilot-result-2026-10-23.md#shared-runtime-execution-result)
now records a bounded-execution pass on `07eedb8d`: two real workers each read
the original, one answer and result were retained, project reservations matched,
and cleanup completed. Eight application calls plus one runtime control took
177.26 seconds with zero paid calls or external actions. The worker now sends
an explicit 4,096-token reply limit on both provider paths; the full gate passed.
The requester nevertheless made an unsupported inference excluding code defects
and tried advertised read tools outside the private fixture scope. Answer quality
and autonomous initiation remain unproven. This is not a production deployment.

Quality follow-up on `f59e1ecc` adds tested evidence-versus-inference guidance to
both real handoffs. The private pilot now advertises only its enforced read
scope, without widening product permissions. Full repository checks and 25
pilot harness tests passed. The subsequent quality activation stopped before
inference because the resident runtime was busy; zero workers/requests/calls
were started. Cleanup completed without changing that runtime. The frozen
quality diagnostic remains the next live gate when the shared slot is idle.

The fresh idle-slot quality trial completed with correct evidence reads, one
answer/result and only advertised tools. Its causal uncertainty handling passed
claim review, but the requester omitted expected 200 from its final finding.
Quality therefore remains incomplete. The result briefing now restores the
original question as bounded, untrusted quoted material, and both handoffs
require explicitly restating key evidence in the final finding. Live validation
of this further guidance is still outstanding; no production change occurred.

## This slice's acceptance criteria

1. A normal taskless member can use the familiar `consult` tool to obtain one
   attributed answer through existing authorized assistance, without a carrier
   task, duplicate request on retry, new scheduler or new service.
2. Routing never invents a participant/project grant. Multiple matching scopes
   require selection; original sources remain granted and revocable; a turn
   with Confidential inputs cannot send an Internal question. Existing task
   thread consultation keeps its behavior.
3. Pair discovery does bounded database work as unrelated conversations grow;
   normal eligible briefings explain how to use and discover the path without
   loading history into model context.

## Implementation

The existing `consult` protocol now adapts a taskless call without a task
argument to `request_assistance_in`. A single authorized pair scope routes
automatically. The optional `conversation`, `key` and `evidence` fields select
an existing scope and granted originals. Existing task-owned calls retain their
thread contract and refuse those taskless-only options.

Acceptance, expert scheduling, read-only tools, answer/result notification,
project charging, cancellation and restart recovery use the same assistance
records and use cases as explicit requests. The compatibility reply contains
only request/scope/status metadata and fixed guidance. Legacy task-thread
replies retain their conservative Confidential lineage handling.

Normal member briefings with an enabled conversation explain `consult`,
`read_messages` and explicit source selection. They append fixed guidance,
never conversation bodies. Quiz and restricted assistance briefings do not
receive that extra guidance. The owner still controls conversation grants;
agents cannot create permissions by naming another member.

A partial composite index on `(member1, member2, id)` supports the two possible
pair orientations. Each query arm and the combined result stop at two scopes,
enough to distinguish no match, unique and ambiguous. The regression test runs
the actual lookup against 1,000 and 100,000 unrelated conversations, requires
equal bounded SQLite VM work, checks reversed participants and removes the
index as a negative control. This proves this lookup's bound, not universal
latency as the entire database grows.

## Verification

The new behavior tests were observed failing before the adapter was added.
Coverage includes the ordinary protocol call through answer delivery, retry,
scope ambiguity, absent/foreign scopes, disabled policy, original receipt
admission and revocation, Confidential inputs and unchanged task-thread answers.
An executable owner scenario uses ordinary `consult` through one expert answer.
The complete repository gate passed: formatting, workspace Clippy with denied
lint warnings, workspace all-feature tests, console checks, personal-value
checks, collection parity and whitespace checks. The team suite passed 908
tests; its behavior harness passed 158 scenarios and 1,991 steps. Existing
explicitly ignored tests remained ignored. Live smoke, model inference and
device checks were not run for this branch change, which is not deployed.
No terminal-client source changed.

## Task-thread hardening follow-up

The next branch slice addresses two concrete legacy gaps before lifecycle
consolidation. Its acceptance criteria are bounded routine consultation reads,
one durable expert claim, and agreement between an answer's thread post and its
terminal status even when storage fails.

- Routine consultation admission now reads counts saturated at the existing
  two-per-turn and six-per-day limits. The application supplies the owner's day
  boundaries. Assigned-run lookup, expert claim and unread answers use dedicated
  indexed queries instead of loading every retained consultation. An unread
  batch is at most fifty answers; remaining answers stay pending for later
  requester turn completion. The outcomes snapshot's legacy archive aggregation
  remains outside this routine-query change.
- Expert claims now commit the oldest queued question's run association in one
  transaction. A retry reuses that association. Two SQLite connections racing
  for the same question cannot both claim it.
- Expert answers and labelled record fallback use the same conditional
  completion operation: terminal status and task-thread post commit together.
  Stale/repeated completions write nothing. Existing post-processing, feed
  events and requester wake policy still run after that transaction. Their
  crash recovery is not established by this change.

The original engine test reproduced a post left behind after a forced status
write failure. It now passes through failure, retry and reopening the actual
database. The reverse failure test refuses a post and proves the status rolls
back, for both expert and record-fallback completion. A two-connection race
proves one claim and one completion survive reopening. The actual routine SQL
is exercised with 1,000 and 100,000 retained consultations, with equal bounded
VM work and a missing-index negative control. A new owner scenario forces the
storage failure, recovers, retries and restarts HQ with exactly one answer.

Focused checks passed: 23 consultation-related tests and the new 17-step
storage-recovery scenario. The complete repository gate then passed:
formatting, workspace Clippy with denied lint warnings, workspace all-feature
tests, console checks, personal-value checks, collection parity and whitespace
checks. The team suite passed 913 tests; its behavior harness passed 159
scenarios and 2,008 steps. Existing explicitly ignored tests remained ignored.
There was no deployment, live model call or new device validation in this slice.

## Dispatch and notification recovery follow-up

Acceptance criteria: interruption must not leave a wake without its associated
run; an accepted answer must still reach its requester after restart; a replay
must not duplicate a delivery or restart an expert that already answered.

Expert and answer delivery now persist their wake/run association and wake audit
event atomically. New task-thread answer completions also save an exact post
association in the same transaction as the answer and terminal status. Legacy
consultation and conversation assistance share the transactional queue writer,
while existing deterministic application policy still supplies authorization,
fold/hold/refusal decisions and the different fallback contracts.

The tick reads batches of at most fifty pending dispatches, ended expert turns
still needing record fallback, and pending answer notices. These are indexed
queries, included in the 1,000/100,000 retained-consultation regression. It does
not retry a folded notice while its associated run is live. Once that turn ends,
delivery is decided again through the existing policy. Held and refused wakes
keep their previous meanings. The conversation switch does not disable legacy
task consultation; the global stop and drain still control scheduling.

The forced dispatch and notification failures first reproduced orphan wakes.
Recovery tests now reopen the real database, deliver the original request/result
once, and advance through platform retry without restarting an expert whose
answer is already accepted. A two-connection delivery race keeps one result run.
Separate checks cover replay, folded-notice deferral and adoption of an existing
unclaimed legacy expert turn without creating another wake. Three executable
owner scenarios cover status-write, run-write and notification-write failures,
storage recovery and HQ restart.

The restart test also reproduced a timestamp tie that hid the saved answer from
the next briefing. Result turns now retrieve their exact associated post; newer
posts continue through normal thread projection. This fix does not broaden task
or evidence access, fabricate a receipt, or treat the answer as task acceptance.

Existing stored consultations are preserved. Recovery adopts only an unstarted,
correctly addressed legacy expert run not already bound to either origin,
following the original oldest-question rule. New post associations apply to new
completions. Historical answers with no causal post association remain on their
existing thread-history path; matching text is not used to guess a binding.
Replay guarantees apply to the same persisted consultation ID. Deduplicating a
repeated task-owned API request after a lost acceptance response remains open.

The complete repository gate passed: formatting, workspace Clippy with denied
lint warnings, workspace all-feature tests, console checks, personal-value
checks, collection parity and whitespace checks. The team suite passed 917
tests; its behavior harness passed 161 scenarios and 2,041 steps. Existing
explicitly ignored tests remained ignored. No deployment, inference or new
device validation was performed for this branch slice.

## Remaining foundation work, in order

The owner expanded the final goal after the integration pilots. Execute these
stages against ADR 0138; do not use the earlier four-gate estimate as a complete
replacement plan.

### Retirement mapping and cutover contract, 2026-10-23

Repository inspection identifies these production seams. This is a mapping of
work to perform, not a claim that the listed code has been retired.

| Current seam | Replacement or retained responsibility |
|---|---|
| `application/engine/consult.rs`: `consult_in`, claim, record fallback, completion, delivery tick and post-turn hooks | Replace executing consultation with the canonical assistance use case. Move `ask_record`/historical excerpt formatting out before deleting the old lifecycle. |
| `application/protocol_history.rs`: `consult`, `answer_consult` | Version the incompatible tool contract; the new `consult` enters canonical assistance, and experts use `answer_assistance`. No task-thread/fallback branch in the new contract. |
| `application/protocol.rs`: tool listing, consulted-turn guard and reply lineage | New contract advertises only its own answer tool and uses the canonical guard. Keep live-token, drain, source lineage and role checks. |
| `application/engine.rs`: briefing, completion, dispatch/recovery and timestamp-tied answer injection | Replace old consult hooks with canonical associations. Preserve genuine task reporting and decision escalation; peer answers are shown through conversation history/task links, not a second post-and-wake loop. |
| `application/history_ports.rs`: `ConsultDelivery`, admission/claim/delivery/finish ports | Retire execution ports after callers move. Retain transcript retention, explicit record search and bounded historical reads. |
| `infrastructure/sqlite/history.rs`: `consults`, `consult_deliveries`, `consult_answer_posts`, `consult_projects` | Preserve rows and exact post/run/project links for historical inspection and cutover audit. No new legacy lifecycle writes after cutover. No guessed post binding from matching text. |
| `infrastructure/sqlite/assistance.rs` and `sqlite/conversations.rs` | Canonical request/delivery state, atomic acceptance, quotas and charging. Historical quota reads may remain read-only through the transition. |
| `domain/assistance.rs` importing legacy `consult` limits | Make canonical assistance own its bounds; temporary legacy aliases point toward it, not the other way round. |
| HQ `app/hq/src/team.rs` and `api.rs` | Add the versioned canonical team entry point without silently changing the current-major tool contract. Remove the old executing entry point at the recorded cutover. |
| Node `api/gateway.rs`, runner `main.rs`/`runner.rs`, worker `main.rs`/`claude_code.rs`, `scripts/pi-runner.sh`, node bench | Audit and move both direct and proxied callers, including environment overrides and Claude Code's MCP bridge. A default URL change alone is insufficient. |
| Work `47-hq.js` and `47e-hq-conversations.js`; replay/transcript owner API | Retain existing task and run history. Add bounded legacy consultation inspection where exact questions/results are otherwise missing; link canonical task-associated requests into this same Work home. |
| `post_in`, `post`, `ask_owner` and worker protocol tools | Retain task status and real authority decisions. Audit implicit question/mention answer wakes so they cannot keep a hidden peer-help engine alive. |
| Existing consultation scenarios, SQL races and HQ/gateway tests | Replace retired-behavior assertions at cutover; retain history, quota, idempotency, source, budget and recovery guarantees as tests of the canonical path. |

The public worker surface currently uses `/v1/team/mcp`; node gateway and runner
defaults explicitly point to it. MCP's date-based negotiation identifies the
transport specification, not Endora's tool semantics. Use an explicit Endora
team protocol v2 entry point (`/v2/team/mcp` and its versioned node gateway),
with the existing owner/history APIs unchanged. During migration, v1 retains
its published behavior. After caller migration and drain, record v1 peer-tool
retirement as a breaking team-protocol cutover; an old worker must receive an
explicit upgrade outcome, not silently enter different semantics. The final
release must contain no v1 execution engine. Transport-version negotiation alone
cannot claim compatibility with changed tool behavior.

Task-linked execution has a prerequisite the pilots did not exercise:
`briefing_body` marks every task briefing as unclassified because task bodies and
thread posts lack immutable input lineage. `Post` currently stores presentation
author, text, time and an unsupported flag, not an authoritative source run.
Do not remove the blanket guard to pass a task-linked test. Introduce trustworthy,
forward-recorded task/post provenance and inherit all actually rendered inputs;
legacy or missing lineage remains unclassified. Owner input needs a typed owner
origin, not a display-name match. Preserve permission/source grants and test
Confidential and unknown-input refusals before enabling task-linked requests.

Canonical task context comes from the authenticated requester's run and trusted
task/project records, never a model-selected payer. A task project must agree
with the selected conversation project; absent/mismatched/ambiguous scope fails
closed. Existing `requester_run` is the durable origin link. Any added task/history
projection must retain that link and not transfer task ownership or source reach.

Historical migration is read-only where possible: keep task posts/transcripts
in their current stores and expose stable tagged legacy IDs. Use exact
`consult_answer_posts` associations when present; older missing associations
remain explicitly unavailable. Do not copy old rows into an authorized active
conversation or create grants from them. Drain old live work before the final
engine removal, and explicitly cancel anything that cannot safely drain; retain
the outcome. Rehearse twice against a copied populated store and compare counts,
IDs, links, ordering and stopped/restart state before a live cutover.

The first implementation slice now makes canonical assistance own the unchanged
two-per-turn, six-per-day and six-read bounds; legacy constants temporarily alias
the canonical values. The new HQ `/v2/team/mcp` and team-listener-only node
`/team-gateway/v2/mcp` reuse the token/authority/drain gates. V2 `consult` always
selects canonical assistance, rejects task-routing/authority fields and the old
answer tool, and never selects automatic record fallback. Tool advertising omits
new peer sends when the scope/policy/input state cannot permit them. A v2
`my_turn` states its contract explicitly while retaining the original stored
briefing and final drain notice. Legacy expert turns must finish through v1.

The actual HQ HTTP regression first failed on the missing route, then passed
authentication, malformed/forged options, canonical retry identity, paused-peer
waiting and zero carrier/legacy requests. It also verifies an actual task turn
is refused by v2 before any legacy admission while v1 retains its published
record-fallback behavior. Node tests prove the exact v2 upstream path, unchanged
worker token/body, no node-owner credential leakage and exclusion from the owner
listener. Further red-first checks corrected unavailable-scope tool advertising
and the missing v2 `my_turn` contract. The existing legacy-expert scenario also
proves v2 cannot finish that old request and v1 still can.

Worker, runner and Claude Code defaults have not switched. Neither lifecycle has
been deleted and no deployment or inference is included in this slice. The
versioned boundary is a migration step, not completion of ADR 0138.

The complete repository gate passed for this boundary: formatting, workspace
Clippy with denied warnings, all-feature tests, console, configured personal-value,
collection parity and whitespace checks. No client/device behavior changed.
Private red/green captures and final gate logs are under `.local/team-v2-*.log`
and `.local/team-v2-gate/`. The unrelated baseline-script edit is excluded.

The next implementation is forward-recorded typed input provenance for task
work, before allowing task-linked canonical admission. Cover task title/body,
criteria, rendered parent/goal context and posts actually supplied to a turn;
tracking only one clean new post must not relabel the rest of a legacy thread.
Record real owner or source-run identity at the authenticated write, preserve
Confidential inheritance, detect untracked text changes and leave old/missing
origins unclassified. Test ownership/display-name forgery, rename, restart,
legacy migration and source changes. Keep the blanket task guard until the
complete rendered-input classification can replace it safely. History/UI
preservation can progress independently while this prerequisite is implemented.

### Forward source provenance: writer prerequisite

The next slice records typed `Owner`, authenticated `Run` or `Unknown` origins
for goal content, task bodies, criteria and individual task posts. The SQLite
metadata contains IDs and origin kinds only, not duplicated conversation bodies.
New authored writes commit their content and provenance together. Goal/task/post
content changes made without a tracked source invalidate the associated proof;
criteria changes cannot relabel the task body. Resource deletion and source-run
deletion remove stale proof. Key and source-run indexes bound these operations.

The application records actual owner goal/task creation and direct owner posts,
and authenticated member task creation and posts. Display author strings do not
establish identity. Generated work, copied decision/report text and legacy writes
remain unknown even when their display actor or budget is the owner. A red-first
scenario regression exposed that distinction before the writer slice shipped.
The engineering wing's generated goal is likewise not recorded as owner-authored.

Real SQLite tests cover valid and invalid sources, selective invalidation,
source loss, transactional rollback including an earlier provenance write in
the same failed transaction, and reopening a populated pre-lineage schema.
Historical content remains readable and an old `owner` display author is never
backfilled as trusted. Application tests cover genuine owner writes, member
rename with stable source-run identity and member-created task criteria/body.

This does not yet classify task briefings or permit task-linked conversation
requests: the existing blanket guard remains. The next step is atomic reads of
content and provenance, followed by classification of every input actually
rendered. Do not pair a new provenance lookup with an earlier content read:
another connection could change the content/source between them. Parent/goal
context, criteria, notes and rendered posts must all be covered; missing sources
stay unclassified. No model inference, UI behavior change or deployment is part
of this writer prerequisite.

The complete repository gate passed for the writer slice: formatting, workspace
Clippy, all-feature tests, console, configured personal-value, collection parity
and whitespace checks. The six focused SQLite cases and two authored-input use
cases passed, as did the generated-owner-context regression. Red/green captures
and final logs are retained under `.local/input-lineage-*.log` and
`.local/input-lineage-gate/`. The next slice must supply atomic content/source
readers before changing any task-briefing classification or v2 admission rule.

### Atomic content/source readers

Task body and criteria, goal content, and paged task posts now have joined
content/source readers. Each row uses one SQLite snapshot; the separate
metadata-only lookup remains an audit API, not a classifier input. Post pages
use stable IDs and a maximum of fifty records, preserving evidence references
and posts with identical timestamps. No source is inferred from a display name.

All nine focused lineage tests passed, including a two-connection writer/reader
race. Its deterministic negative control rejects an old criterion paired with
new source metadata, while the joined reader keeps each criterion paired with
its actual author. Captures are in `.local/input-readers-expanded.log`; the
initial missing-reader failure is in `.local/input-readers-red.log`.

The full repository gate passed for this reader slice: formatting, workspace
Clippy, all-feature tests, console, configured personal-value, collection parity
and whitespace checks. Rust logs are in `.local/input-readers-gate/`. No model
call, deployment or additional handheld check was performed for this change.

`my_turn` now uses the indexed latest-post reader, selecting at most twelve
posts in both protocol versions without loading the earlier archive. Sources
are inherited before the reply is returned; unknown and Confidential posts
raise the receiving turn's tier, and subsequent owner input cannot lower it.
The underlying reader caps every request at fifty and preserves original ID
order, evidence references and unsupported markings. Older UI history remains
stored. Source existence and Confidential status are checked in one SQLite
snapshot, so a deleted source fails closed.

Red-first tests caught an earliest-page implementation, omitted source
inheritance and the missing-source check. Those captures are under
`.local/latest-posts-*-red.log`; the first tail failure is
`.local/latest-posts-red.log`. The complete team library passed 939 tests, and
the full repository gate passed in `.local/latest-posts-gate/`: formatting,
workspace Clippy, all-feature tests, console, configured personal-value,
collection parity and whitespace. No model call or deployment was performed.
The blanket task-briefing guard remains. Next, cover every rendered task
briefing input before changing canonical task-linked admission; known task,
goal and criteria sources must not launder notes, evidence summaries or other
unknown context into Internal messages.

### Source-classified ordinary task briefing

An internal, default-off migration selector (`canonical_task_briefings`) now
selects a smaller briefing for developer/researcher assigned work, retries,
recovery and owner replies. It is not an owner-facing feature or an enabled
production setting. Remove this selector and the legacy branch at final
retirement. Management, review, decision and other specialized wake purposes
continue through their existing briefing until their contracts are migrated;
their blanket source guard remains.

The selected briefing consumes atomic task/criteria, parent and goal inputs,
and the latest twelve task posts. A served parent criterion inherits its own
source. It retains goal notes, strategy exclusions and document/branch context,
marking these older unsourced fields unclassified rather than dropping owner
constraints to permit a request. Source-run tiers survive member rename and
later clean owner posts. It does not automatically inject the broader world
memory; notebook/world reading remains available through guarded tools. Task
ownership, reporting, approvals and acceptance still use existing policy.

Focused tests show a fresh genuinely owner-authored task can start without
artificially clearing a tier. Separate cases preserve and refuse to relabel
unknown bodies, criteria, goals, ancestors, posts, notes and strategy constraints.
Both Internal and Confidential source-run tasks keep their source tier; sixty
stored posts yield twelve briefing posts while all sixty remain retained.
Captures are under `.local/task-context-*.log`. The original red test failed
against the blanket guard; an initial implementation also hit an unsupported
Rust let-chain and was rewritten for the repository's compiler.

The full repository gate passed for this source-context slice in
`.local/task-context-gate/`: formatting, workspace Clippy, all-feature tests,
console, configured personal-value, collection parity and whitespace. The team
library passed 943 tests. No model inference, deployment or handheld change is
included; both the production defaults and the separate legacy source guard
are unchanged.

This source-context slice left canonical task-linked admission closed; the next
slice below adds its explicit v2 path. Worker defaults, specialized briefing
migration, legacy execution retirement, UI/history proof and NAS cutover remain
unfinished.

### Canonical task-linked admission and offline execution

V2 `consult` and `request_assistance` now use the same canonical application and
persistence lifecycle for eligible task work. Both pass through the common live
turn, role/special-turn, project-tool, input-tier and drain checks. The task's
trusted project must match the authorized active conversation; absent or
mismatched scope fails closed. The migration selector is still default-off.
V1 retains its legacy consultation path and explicit task-owned assistance
refusal. No worker default or production setting has switched.

Acceptance rechecks the requester's actual run/member and task/project inside
the SQLite transaction. A project edit between application preflight and the
transaction cannot leave an accepted request or consume its key. Replay retains
the original task identity: another task cannot reuse the old key and question
as its own request, while a later turn on the original task can retry it.

The public protocol scenario covers accepted retry identity, one expert/result
flow, read-only expert capability restrictions, retained conversation answer and
unchanged task-completion authority, with zero legacy request rows or duplicated
task-thread answers. Missing/mismatched scope and unknown/Confidential inputs
are refused through both v2 entry points; unavailable assistance is omitted from
the v2 tool list. The owner-readable behavior scenario is
`features/canonical-task-assistance.feature` in the team context. Private
red/green captures are under `.local/task-assistance-*.log`. These are offline
simulated-worker checks, not proof of autonomous model initiation or final
answer quality. The initial scenario harness needed trait imports, a borrow
fix and its supported filter syntax before it ran successfully.

The complete repository gate passed for this admission slice in
`.local/task-assistance-gate/`: formatting, workspace Clippy, all-feature tests,
console, configured personal-value, collection parity and whitespace. The team
library passed 947 tests. Both storage regressions were observed failing before
their fixes and passing afterward. No paid or local model call, production
deployment or handheld change was performed.

Next audit the remaining task briefing purposes and migrate the callers, then
retire the old executing lifecycle behind the explicit version cutover. Retain
specialized approval/acceptance context and its source guards where needed;
remove its old peer-consult hooks rather than forcing unclassified management
context to become Internal. Keep historical rows/readers and verify the actual
UI, failure/restart behavior and bounded work-triggered model run before NAS
deployment. The complete ADR 0138 definition of done remains the release gate;
this slice alone is not a replacement release.

### Versioned worker caller migration

The branch's worker and runner now default to `/v2/team/mcp`, or
`/team-gateway/v2/mcp` through the node. The Pi launch script uses the latter.
Configured `ENDORA_HQ_MCP` and `WORKER_HQ_MCP` values are validated rather than
silently translated: old routes require an explicit configuration upgrade.
Claude Code's independently started MCP bridge validates its environment too.
The model relay, runner lease/check APIs and turn tokens retain their own paths
and contracts. The bench-token exclusion check also covers the new gateway.

One pure route validator lives in `shared/protocol`, used by worker setup,
Claude bridge setup and runner setup. These three real callers justify the two
new internal dependencies; no external crate or network stack was added. Errors
never echo configured URL credentials. Existing positive worker HTTP/Docker
fixtures now verify v2 paths; explicit negative tests preserve the old paths.
Red captures are `.local/team-callers-*-red.log`, and focused protocol/worker/
runner checks passed in `.local/team-callers-green.log`.

The complete repository gate passed in `.local/team-callers-gate/`: formatting,
workspace Clippy, all-feature tests, console, configured personal-value,
collection parity and whitespace. The Pi launch script also passed its shell
syntax check. Worker integration tests observed the exact v2 HTTP request path;
runner tests observed that path in generated container environments. No model
inference, live runner restart or deployment was performed.

This caller change is not a deployable replacement by itself. HQ's ordinary
task briefing selector remains default-off, v1 server routes and execution
still exist for migration, and the old engine must be drained and retired.
Next freeze the server upgrade response, separate retained record/history
reads from `engine/consult.rs`, and remove the old admission, claim, completion,
delivery and recovery callers without losing historical rows. Then enable the
canonical briefing path as part of the verified single-engine cutover. Record
actual migration/UI/model and NAS evidence before declaring the plan complete.

### Retained reads detached from legacy execution

Explicit `ask_record` search and testimony formatting now live in
`engine/record_search.rs`; canonical `consult_peer_in` lives with assistance.
Member-name resolution is a common engine helper. These responsibilities no
longer reside in the old consultation module, so deleting that module later
will not delete historical search or the canonical adapter. This is a move of
existing behavior; legacy consultation still executes during migration.

The complete repository gate passed in `.local/consult-detach-gate/`, including
the 947-test team library, workspace/all-feature checks, console, configured
personal-value, collection parity and whitespace. The first compile caught a
module-name collision with the domain record import; `record_search` resolves
it without changing that import or its callers. No data migration, inference
or deployment occurred. Next provide bounded owner inspection of retained
legacy questions and exact answer-post associations before removing old write
ports; never guess answer identity by matching text or participant names.

### Retained task-question inspection

The owner can inspect earlier questions in the existing Work task inspector,
independently of peer-messaging enablement. The signed HQ read route is
`/v1/tasks/{id}/consultations?after=...`, with a fixed node owner proxy. Worker
and runner tokens cannot read it. Pages contain at most fifty records, ordered
by original IDs; the query reads one additional row for its next-page flag.
The browser replaces its current page and retains at most 64 back cursors.
Navigation, refresh and page export stay within the task inspector.

The projection keeps original asker/expert/run/task IDs, question, outcome,
recorded project snapshot, and exact `consult_answer_posts` association. It
never fills a missing project from the task's current project or selects an
answer by matching text. Missing and wrong-thread answer links remain visibly
unavailable. No historical row is copied into an authorized active conversation,
and inspection creates no wake, grant, task or audit action.

Focused red/green tests cover exact and unavailable links, absent project
snapshots, same-time paging, signed HTTP access, the fixed node proxy and escaped
UI content. The actual query uses the new `(thread,id)` index: VM work stays
equal from 1,000 to 100,000 unrelated rows; dropping that index makes the negative
control exceed 10,000 steps. This is an indexed-read bound, not a latency claim.

The browser check rendered the actual component and stylesheet with synthetic
data at desktop and phone widths, and exercised next/previous navigation. That
review led to larger phone controls and clearer record separators. Screenshots
are in `.local/legacy-history-ui/`; this was not a production UI smoke check.
The temporary browser tab and local layout server were closed. Captures and
focused logs are under `.local/legacy-history-*.log`.

The repository gate passed: formatting, workspace Clippy, all-feature tests
(including the 950-test team library), console, configured personal-value,
collection parity and whitespace. Logs are in `.local/legacy-history-gate/`.
The first console job exposed a missing browser `performance` clock in its VM
when the sound keep-up timer fired. The harness now provides that clock and
explicitly tests fresh/stale stream behavior. Console and the remaining checks
then passed; the already-passing Rust checks used unchanged Rust sources. The
observed failure excerpt is retained separately because the corrected console
run replaced that log. No shipped sound code changed. No model call, history
deletion or production deployment occurred.

Next retire the old server/execution path with an explicit v1 upgrade outcome
and tested drain/cancellation of pending legacy work. Preserve these read ports
and populated history tables. Final migration rehearsal, restart/failure checks,
actual work-triggered model evidence and NAS cutover are still required.

1. **Map retirement and freeze cutover contracts.** Inventory callers, protocol
   tools, request/delivery records, scheduler branches and UI readers in the old
   `consult` path. Mark each canonical replacement, thin adapter, historical
   read projection or deletion. Version incompatible behavior instead of silently
   breaking current-major contracts. Define legacy queued/running work drain or
   cancellation and missing-scope refusals before writing the migration. ADR 0138
   fixes the final definition of done; this detailed caller/data mapping is next.
2. **Replace the active lifecycle.** Route supported task-linked and taskless
   help through the conversation/assistance application and persistence path.
   Use trusted conversation/project/task metadata; never invent a grant or payer.
   Replace old admission, claim, completion/fallback and recovery code rather
   than maintaining parallel engines. Remove obsolete branches, tools/config
   and behavior tests after their callers are accounted for. Thin compatibility
   adapters must not own another lifecycle.
3. **Preserve and verify visible history.** Rehearse idempotent mapping/read views
   for retained posts, questions, answers and final findings. Keep source IDs,
   authorship, ordering and task/run links; do not recreate missing history or
   transfer evidence access. Verify the actual Work UI for fresh/migrated history,
   progress and terminal failures, pagination, export/delete and archive growth.
   Historical tables may remain for reading, without their retired execution code.
4. **Prove the smaller workflow.** Write red-first public-use-case scenarios for
   the replacement and retired behavior. Validate complete, grounded final
   findings; work-triggered agent initiation; unavailable peers/sources; trusted
   charging; restart, retries, cancellation and permission changes. Retain prior
   pilots as evidence, not proof of the replacement. Compare equivalent workloads
   honestly and run the full gate and relevant device checks. Do not make optional
   downtime chatter or a performance win a release requirement.
5. **Cut over and retire in production.** Follow the existing NAS/Ansible workflow
   with backups, migration/version rehearsal, drain, one active engine, actual
   history/UI smoke checks and recorded enablement/rollback evidence. Rollback
   restores a compatible release/database pair. Confirm no obsolete engine is
   active before declaring ADR 0138 complete.

The foundation and full plan remain incomplete. The NAS remains on the prior
deployed candidate with conversations disabled; this checkpoint is branch work.

Planning validation: this goal update changes documentation only. It reuses the
observed full passing gate for the unchanged executable code in `e3f03e80`, with
fresh personal-value, document/link and whitespace checks. No migration, code
deletion, model call or deployment was performed for this decision.

## Admission, accounting and completion effects follow-up

Task-thread admission now checks exact replay before consuming an allowance,
then atomically counts both origins, accepts the question and snapshots its
thread's optional project. The canonical payload binds originating run, expert,
thread, topic and question. Same-turn retries retain one request and do not add
wakes or repeat an accepted answer. Different questions remain new admissions;
the third-distinct-question compatibility test still checks the existing limit.
Existing rank, task reach and live-token checks precede replay.

The shared allowance is two new requests per turn and six per requester/day,
using the configured owner-clock day. Conversation usage counters survive
cancellation/deletion, and legacy retained rows remain counted. A real-use-case
test combines two cancelled assistance requests with four task consultations in
subsequent turns and refuses the seventh. Its negative control omits legacy
daily usage and correctly detects the seventh being admitted. A two-connection
race between origins proves they cannot both spend the final turn allowance.
The Internal/Confidential guard remains unchanged; tests do not clear lineage to
force a turn across the two entry points.

The expert project comes from the requesting thread at acceptance, never the
question topic or a model-supplied payer. Tests move the thread to another
project after admission and verify reservation, use, charge, release and late
charge rows retain the admitted project. New receipt grants can recognize that
trusted legacy-expert project, with the existing original-source and tier checks.
Unscoped tasks and older requests with no snapshot retain their previous
behavior. Requester result turns retain ordinary task-thread budgeting.

Answer completion now commits the task post, terminal status, exact post link,
thread feed event and selected knowledge-use markers together. The application
resolves the answering member and cited knowledge IDs. Updates are idempotent
and use monotonic timestamps. Feed/status failures and a knowledge-update fault
roll back the answer; recovery and retry retain one answer and one feed event.
The feed-failure negative control restores the former event-after-commit order
and correctly detects a terminal answer left behind. The first trigger used an
incorrect event name; it was corrected to the actual `thread.posted` event before
the meaningful negative control and final verification.

The executable owner scenario retries the same question before and after its
answer and observes one request and one answer. The complete repository gate
passed: formatting, workspace Clippy with denied warnings, workspace all-feature
tests, console checks, personal-value checks, collection parity and whitespace
checks. The team suite passed 923 tests; its behavior harness passed 162
scenarios and 2,055 steps. Existing explicitly ignored tests remained ignored.
The first full gate caught a post index in the history-only schema fixture;
moving it to the schema that owns posts corrected the failure before this pass.
No model inference, deployment or new device validation is included.
