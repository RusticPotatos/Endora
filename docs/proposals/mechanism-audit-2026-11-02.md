# Mechanism retirement audit (2 November 2026)

The owner asked for one answer per substantial mechanism: **what would we lose if this did not
exist?** Read-only inventory of the live build `49677595`, against HQ's database for 19 Oct to
2 Nov (3,526 runs; 1,486 with a transcript, so refusal counts are lower bounds), the ADRs and the
[baseline experiment](baseline-experiment-2026-10-28.md). Five readers, one an area, each held to
about 40 minutes; their full tables, with code paths, line counts, numbers and overlaps, are in
[mechanism-audit-2026-11-02/](mechanism-audit-2026-11-02/) (execution, context, verification,
safety, residual). About 100 mechanisms; the decisions below. Nothing was changed during the
inventory.

**Judged by kind.** Reliability: does it prevent a failure seen? Efficiency: does it save more
than it costs? Quality: does it improve independently verified outcomes? Safety: does it enforce
a required boundary? (Never removed for rarely firing.) Capability: does it enable useful work?
A mechanism whose original evidence no longer applies loses its presumption of usefulness; one
with clear value and negligible cost is not retested.

## Where the money went (since the one-team cut-over, 27 Oct: about $30)

| | Spend | Moved anything forward |
|---|---|---|
| Continuations after running out of room, and their hand-off notes | $10.39 (a third) | 17 of 473 turns; none since 1 Nov |
| Downtime turns (chess moves, on Sonnet and Opus) | $4.43 | 0 of 112 since 30 Oct |
| Model league and Holodeck scenarios | $3.99 | idle since 31 Oct |
| Reviewer turns one rank up | $3.56 | 11 send-backs; dormant since 1 Nov |
| Stall nudges and stranded recovery | $1.69 | 35 of 154 turns; 73% of these wakes were refused |

## Remove: unused or dead, no test needed (about 1,600 lines and some settings)

Each is either never called in the window, never reached from what runs, or retired by an ADR
whose code was left behind. Removed with the existing tests, history kept readable.
- Dead code: timers (nothing calls `add_timer`); consults (2 rows, archive first); the old wake
  reasons `Decide`, `AcceptanceAsked`, `LeadReview` (keep a read fallback for old rows),
  `Consulted` and `COLLEAGUE_HELP`; the CEO leftovers (keep the Opus turn cap under a new name);
  retraction and `GapReported`; `reports_to`; `recalled_at` and `Situation::Recalled`; the
  butler's leftovers (ADR 0136's unfinished step 5b2); the approval-must-name-a-criterion check
  and the same-question desk merge (0 uses ever); closures' `duplicate_of` and `successor`
  (never used); the names of retired tools in the worker (`tell_story`, `escalate`,
  `answer_quiz`, `note`, the `read_notebook` pointer).
- Settings: the 6 disabled routines left from the lead roles; the 7 stale `escalated:` rows; the
  never-set `loop_items`; hold-for-an-away-member (0 uses).
- **The briefing's world budget and weights:** 0 pieces dropped and 0 over budget in 3,374
  briefings; the situation label stays.
- **The bench for an outside harness:** 4 calls, all on 18 Oct, and still a live credential into
  the relay. The owner removes its token file from the environment (a lab change).
- **16 bridged tools offered to work turns** (weather, traffic, news, sports, safety alerts, city
  meetings, knowledge, Home Assistant, 8 Brave searches): **0 calls and 0 receipts in 3,482
  runs**, yet held back in 74% of Claude turns, where held-back calls already guess arguments a
  third of the time. A filter on team turns; the skills themselves stay. Stop loading `chess`
  in work turns too (it stays on downtime turns).
- ADR text: mark 0140 and the coordination budget superseded; fix the two safety comments that
  state the old rule (the bridge module, the house-model block), since agents read them.

## Simplify: clear evidence, small and reversible

- **The team machinery around the solo loop: climb, hand-up, continuation and HQ's hand-off
  note (about 1,500 lines).** Round 4 retired the development side by its pre-set rule, every
  run since 1 Nov has been solo, and this machinery took a third of the spend for 17 forward
  moves. Not rescued with another experiment. Order: first settle the two items still waiting
  in review (#796, #864), then remove; keep the solo loop's 9-failure stop. Continuation goes
  once the reply-cap window's logs show no turn ending out of room (0 on 2 Nov, since the
  budget counts the conversation once).
- **Stall nudge and stranded recovery:** merge into one, and skip archived and in-review work
  (110 wakes went to an archived project).
- **repo_submit's untracked-files refusal (D33):** 15 of its 17 refusals were `__pycache__`,
  one lost a finished held-out task, none caught anything. The node's upstream and private-path
  checks stay.
- **Setting a status a task already has:** a no-op, not a refusal (26 refusals never recovered).
- **The per-turn cap and model rules exist twice** (HQ by model, the relay by retired roles,
  which failed 37 turns in late October): one table by model.
- **The lab's build is stated three times** (instructions, briefing, map): once.
- `add_to_map` (2 calls); the world's stages, revisions and timeline serving 6 sections; the
  tester persona in prompts (a model told a tester will re-check may lean on a check that is not
  there; a prompt change, so it comes with a scenario).

## Protect: safety boundaries (verified as enforcing a real policy; not traded for speed)

Sealed credentials and the key file (the only at-rest protection live: the vault holds none of
Endora's keys yet); the relay's limits, spending caps and "house turns never reach Claude";
Restricted data and the default that a new tool is Restricted; the node's contract listing and
bridge policy (trim what the team is offered, never loosen the policy); the bridge's role table
until a per-loop read/act gate replaces it; the git fence (`hq/` branches, never main, forbidden
paths), the privacy scan, outside repositories never pushed; the runner sandbox and ring 1; the
drain (34 drains, 0 turns cut), the runaway tripwire and kill switch, the 30-minute turn limit;
"only the gate makes done"; receipts and the transcript store (every measured decision since
1 Nov rests on them); the worker's new logging.

## Keep: earning their place

The kept item sandbox; retries and the failed-start re-wake; queue order, start gating and wake
admission; the solo loop; the branch check (1 real catch in 94, no model cost; fix its 4
bundle 404s); check-backs; closures with reasons; the desk; history and `ask_record`;
`homelab_map` and `ansible_nas_doc`; `image_tags`; the repeat guard; prompt caching; 30 rounds
on Claude (it carried the held-out test); the 8,192 reply cap (judged on its logs); the round
tooling while experiments run; the cucumber suite.

## Unresolved, and the one test chosen first

| Mechanism | Why unresolved | How it gets settled |
|---|---|---|
| **The evidence rule's reference format** | about 350 refusals (28% of turns that post or set a status, 68 on 2 Nov alone); 85% passed on a reword alone, about 10 followed a new reading; about 300 rounds | **first test, below** |
| The reviewer turn | dormant now; 4 of 6 tester send-backs read caught real errors, 2 were wrong; lost 4 finished tasks in round 3 | stays dormant; next, offline: the judge's verdicts on the 20 sent-back tasks' first and final work |
| 6 KB result cut on Claude turns | fires in 52% of Claude turns since its cause (D20) was fixed; harm not shown | logs only; touched only if they show re-reading cost |
| Gist, stubs, `find_tools` (house turns only, ~670 lines) | house brains took 0 runs on 2 Nov | the owner's call on house models |
| Loop lessons; the outcomes card; live replay | little content; no view counts | 14 more days of logs; ask the owner |
| League; AirPlay 2 door | idle; one log line | the owner's call |

**First test: the evidence rule's reference format** (high frequency, measured cost, rising, and
settled offline from stored turns: no live runs).
- **Change tested:** accept a document (`dN`) and a world page (`world:…`) as references, and
  on a plain `post` drop a reference that is not one instead of refusing the post. The
  per-criterion check at `in_review` and `done` stays as it is.
- **Evidence:** replay the 257 stored "not a reference" refusals and the 25 "criterion uncited"
  ones through the changed rule.
- **Keep the change if:** at least 90% would have passed, and none of those passed would have
  preceded a task a judge later failed. Otherwise the rule stays.
- **Budget:** one hour, offline. If it cannot decide within that, the rule stays as it is.

## Questions only the owner can answer

1. **Downtime chess on paid models** ($4.43, 15% of the spend, nothing moved forward): keep the
   board but play it on the house model only?
2. **The league and Holodeck** ($3.99, idle since 31 Oct): park its scheduling?
3. **Confidential lab data going to DeepSeek** (227 container-log reads by DeepSeek turns):
   ADR 0141 asked about Anthropic; DeepSeek joined three days later.
4. **Does the offsite backup take `secrets.key`** as well as the databases? (Not checkable
   without opening the inventories.)
5. **The confidential marks** (89% of turns carry one; since 0141 they hold nothing back): drop
   the inheritance, keep the flag on each receipt? 0141 said marks stay.
6. **Per-member money shares and "out for today" cards** (14 of 17 approved; the day pool and
   the caps already enforce the policy): drop them?

## Live problems found (operational, not mechanisms)

- **The lab health check has been off since 1 Nov:** its routine skips while #864 ("Check the
  home lab") is open, and #864 waits in review for a reviewer turn that no longer comes (13 skips
  to 13:31 UTC 2 Nov). The failing NVMe makes this the most urgent item here.
- **The experiment's round hold never lifted** (it counts blocked items as unfinished): closed
  by hand on 2 Nov after this inventory; downtime and chess resumed.
- **Run 4060 (Opus) ran 60.1 minutes** past the 30-minute limit, ending when its project was
  dropped; cause not found.

## Done the same day

- **The evidence rule stays.** The replay, run as set above (1,489 stored turns; today's rule
  ported and checked against its own unit cases): of 426 refusals, 195 would have passed
  (45.8%; 59.6% of the "not a reference" ones), against a bar of 90%; and 8 calls that would
  have passed came before 7 tasks a judge later failed (883, 885, 915, 962, 965, 972, 974),
  though in each the cited document was the task's own and accurate and the work, not the
  citation, was what failed. Most of what remains refused is one list cited for two criteria
  (102) and free text in a judged list (98), which the change could not help. No further rounds
  on it. A side finding: a branch name such as `hq/task-974-…` inside a finding is read as
  "task 974".
- **#864 closed as obsolete, not done, and the lab check restored.** Live state checked first:
  in review since 28 Oct 23:28, when Kit (Sonnet) set it ready after it climbed from Bo; its
  review one rank up went to Mara on Opus, whose turn the relay refused with "Opus is the CEO's
  alone" (the role rule left from 0111, which the duplicated per-turn rules above keep alive),
  and no reviewer was woken again. The routine skips while its last task is open, so it had not
  checked since 1 Nov. The dependency was removed, not worked around: **Home lab upkeep
  (project 1) is now worked by Bo alone**, so a lab check closes on its own checks with no review
  step; #864 was closed through a project of its own, dropped (no owner route closes one task;
  HQ recorded it as obsolete); a fresh check was fired at once (#1041). HQ refuses posts on a
  closed task, so this record is where the reason lives.
- **"Opus is the CEO's alone" is current policy, not a remnant, and is no longer stranding
  work.** ADR 0142 made it "Opus for the member whose model is Opus; every other member is
  refused it"; the relay has enforced exactly that since 30 Oct (`7af1301f`), with a regression
  test (`since_one_team_the_member_whose_brain_is_opus_thinks_with_it_at_the_dollar_cap`). The
  37 refusals, #864's among them, all fell between 25 Oct and 30 Oct 03:23, before that fix; 84
  Opus turns since, none refused. What stranded #864 was the fixed bug plus no re-wake of a
  reviewer whose turn was refused, which goes with the reviewer. Kept; only the refusal's
  wording still names the CEO.
- **The lesson:** removing a mechanism must also remove what depends on it. The reviewer was
  retired in practice on 1 Nov, but an item already waiting for it, and a routine waiting for
  that item, kept the retired mechanism in control. Before each removal above: find the open
  items and settings that wait on it, and settle them first. Removing code is not retiring a
  mechanism: it is retired when no task, routing rule, permission or completion condition still
  needs it. So every decision above also answers **what active work or policy still depends on
  this mechanism?**

## The engine boundary: ordinary cleanup, not extraction (2 Nov)

The owner's sentence: the engine owns work, workers execute it, the node authorizes it, the UI
observes and controls it, capabilities do their own jobs. A read-only, source-level check of
`49677595` (45 minutes; boundaries judged by who decides, not by imports):
- **The crate graph already has that shape.** The worker needs only the kernel, the protocol
  and HTTP (`endora-team` is a test dependency); the node does not depend on the team crate;
  playing sound (console → node → sound hub) and reading the lab (console → node →
  capabilities) never touch `domains/team` or `app/hq`; `app/hq` is a thin HTTP shell.
- **Inside `domains/team` the split is by file, not by function:** about 30k lines run work,
  11k are organisation and simulation, 3.4k are the owner's views. The worst tangles: the tick
  and the queue interleave lifecycle steps with map, downtime, games, scenarios and the league
  (`engine.rs` `tick`, `start_queued`); completion picks its reviewer by rank; and **`Role`
  still decides who may act**: HQ sends the member's role to the node, whose contract offer and
  bridge table read it, and `recheck_in` refuses by it.
- **The owner's test already passes:** the cucumber scenario "one member works a loop alone"
  carries a task from claim to done with no UI and the protocol driven as a fake worker,
  through the gate and the evidence rule. It leaves out the real node's authorization (receipts
  are injected) and still has to hire members with roles.
- **Docs that disagree with the code:** `where-things-go.md` (no roles; roles still
  authorize), `domain-map.md` (done needs a second member; solo loops close alone),
  `architecture.md` (last reconciled 20 Oct).

**Verdict: (b), ordinary cleanup inside the existing crates.** By the owner's rule, one small
ownership correction with the existing tests, then stop and reassess. The candidate is retiring
`Role`, led by a scenario that finishes a task with no role named: the engine tells the node
what a turn may do instead of sending a role. It touches the node's authorization, so it waits
for the owner's word.

**What success looks like:** a smaller Endora that verifies at least as many tasks at no more
cost or attention, with no safety regression. Removals are not the goal; better decisions are.
