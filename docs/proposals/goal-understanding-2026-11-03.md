# Goal understanding and autonomous execution (3 November 2026)

**The question.** Given a goal instead of a specified task, can Endora's existing worker find what
it needs, act, verify and stop appropriately?

**The answer, from one task.** Partly.
- The worker found the contradiction that mattered, kept to its authority, and recovered from two
  evidence refusals.
- It never consulted the two sources that already held the deciding fact: HQ's record of task 796,
  and the fork's history. So it asked the owner to look up what Endora itself knew.
- It closed the task as done on a "not yet known" answer.
- Claude Code, on the same model, verified the same goal.

The first consequential divergence is **model judgment**, primed by **inaccurate information** in
Endora's own world page. No harness defect was shown to decide the result, so **no code was
changed**.

*Baseline:* branch `claude/integrity-audit-ceb3c2` at `354409a9`. *Live HQ:* build `38668a2b`,
which has neither Milestone 1 nor the audit's fixes (nothing is deployed). The HQ arm ran on the
live build. The goal was chosen so those fixes barely bear on it (see "Limits").

## 1. The goal-handling path, from intent to verified outcome

What each of the owner's seven behaviours rests on in the code, and what run 5316 showed.

| Behaviour | Harness (code) | Model | Seen in run 5316 |
|---|---|---|---|
| 1. A broad goal | HQ task: title, description, criteria (at most 8 lines). `my_turn` briefs the task, its criteria marked `live:`, the goal chain, thread news, and the world-page sections matched to the item. | Reads it | Understood: answered the question asked, kept to "investigate only" |
| 2. Discovering context and tools | 9 tools offered up front; 12 held back behind `tool_search_tool_bm25`, `ask_record` and `homelab_map` among them (`turn.rs` `opening`, `DEFER_PAST`). Pages through `read_world`. The fork through `repo_checkout`. | Chooses what to look at | Used up-front tools and `read_world`; never searched the held-back tools; never checked out the fork |
| 3. Unknowns and dependencies | No mechanism | All of it | Named the key unknown: is the timer running? |
| 4. Choosing the next action | 30 rounds on Claude; a repeat guard; results over 6 KB cut and the cut said | All of it | 6 reads in 3 rounds, then answered. Read gluetun's newest 100 lines twice (r3992, r3997). |
| 5. Execution and verification | Node contracts with receipts. The evidence rule (ADR 0114): every criterion cites a receipt, and `live:` criteria need one from this turn. | Cites | Refused twice (free text as evidence; criterion 5 uncited), corrected both times. The rule checks that a citation exists, not that it is enough: it passed an answer that left criterion 2's "since its last start" unmet. |
| 6. Recovery when evidence contradicts | No mechanism | All of it | Good: the house page said "restarted every 2 hours"; "Up 2 days" contradicted it; Sol did not trust the page |
| 7. Continue, stop or ask | Rounds and room limits; `set_status`; `ask_owner` ends the turn; a solo loop closes on `in_review` once the checks pass | Decides | Stopped after about 3 minutes. Put an owner check in its final post rather than `ask_owner`. The loop closed the task "done" with the answer "not yet known", so the owner's report-back has nowhere to land. |

**Tool limits on this path.**
- `container_logs` reads the newest 100 lines at most: Portainer is asked `tail=100`
  (`terminal.rs:1005`), and `lines` is clamped to 100. It cannot read by time.
- For gluetun, 100 lines is about the last hour of more than 7,000 lines since the stack's last
  start.
- When the cut happens at Portainer, nothing in the result says older lines exist. The node's own
  "older lines left out" note fires only when the node cuts (`contracts.rs` `container_logs`).
- No lab tool reads systemd.

## 2. The experiment

**Goal.** Endora #796, "Gluetun 2-hourly timer: is it still needed?".
- In review and stranded since 27 Oct (audit item 2). Never settled.
- The lab changed since: on 31 Oct the VPN stack was moved out of Watchtower.
- Not rehearsed in this form: task 8 of rounds 1–4 asked what stops gluetun, not this.

Pre-registered at 03:26 UTC: the goal, five criteria, a hidden judge's note, the budget and the
time limits.

| | |
|---|---|
| Given to the worker | Outcome and constraints only: "Settle it on evidence from the lab as it is now, and leave the owner one decision they can act on. Investigate only; change nothing; submit no branch." |
| Criteria | 1. answer (yes, no, or not yet known) and why; 2. rests on the lab's evidence since the stack's last start, each fact cited; 3. names contradictions and what would change the answer; 4. one exact owner action; 5. nothing changed |
| HQ arm | Task 1057, project 44, Sol (Claude Haiku 5.5, solo). Tools narrowed to reads plus `repo_checkout` and `shell`; no `repo_submit`, so nothing could be pushed to the fork. Cap $1.50. |
| Claude Code arm | Subagent on Haiku. The same words. A read-only clone of the fork; `docker ps` and `docker logs`, `systemctl` and `journalctl` read-only, allowed by instruction, not enforced. |
| Truth, read-only, 03:20 UTC | The timer has been disabled since 27 Oct 00:10 UTC (fork `b3ad52a0`; owner's post in 796's thread). The stack was recreated 31 Oct 21:00 UTC, with 0 restarts since. gluetun's own healthcheck restarted the VPN 7 times on 1 Nov, 08:53–09:02 UTC; DNS was back by 09:32. Downloads completed after that. The house page still says the timer runs. |

## 3. Results

| | Endora worker (Sol) | Claude Code (same model) |
|---|---|---|
| Answer | "Not yet known"; the owner should check the timer | "Not yet known, leaning no; keep it off, watch 7 days" |
| Timer state | Not established; "I cannot read systemd" | Established from `git show b3ad52a` and `systemctl status` |
| VPN since the last start | Last ~1 h only (the 100-line tail) | Whole window: the 1 Nov episode and its recovery, DNS counts, prowlarr timeouts |
| Contradictions named | The house page against "Up 2 days" | The fork's "being measured" doc; "healthy" while DNS fails; swap nearly full |
| Owner action | Run three `systemctl`/`journalctl` commands and report back: **an unnecessary question**, since HQ's record and the fork held the answer | Watch two counts until 9 Nov; how to re-enable and undo, exactly |
| Wrong material facts | None; "stall symptoms recur" overstates nzbget's errors on one release | None found |
| Criteria met (judge) | 1, 3, 4, 5. **Not 2**: no evidence since the last start. | 1–5 |
| **Verified** | **No** (HQ marked it done) | **Yes** |
| Human attention | 1 question, avoidable | 0 |
| Repeated actions | 1 (gluetun log read twice) | None seen |
| Calls | 8 receipts, 6 rounds | 36 commands |
| Time | 2 min 49 s | 3 min 3 s |
| Cost | **$0.009** (HQ ledger: 6,029 output tokens, 126K cache reads) | Not metered: 119,582 subagent tokens; under $0.60 even at the output rate |
| Stopped appropriately | No: closed "done" while its own answer was pending an owner check | Yes |

**The comparison is uneven in one way that matters.**
- The Claude Code arm was allowed `systemctl`, which no worker tool offers.
- Its decisive first fact came from the fork's git history as well, which the worker could have
  checked out.
- It could not read HQ's record of 796. The worker could, with `ask_record`.

So the worker had a route to the deciding fact by two paths, and took neither. One task decides
nothing about general performance.

## 4. Diagnosis: the first consequential divergence

**Where.** Round 1 of run 5316.
- Sol read `world:house#known-facts`. It said the timer runs every 2 hours and is "not in
  ansible-nas". Both are stale since 26–27 Oct.
- Sol read `world:project/4`: "No sections yet."
- It never read task 796, which the description names, and never checked out the fork.
- Claude Code's first deciding move was the fork's history.

**Classified as:**
1. **Incorrect model judgment.** Sol named the unknown itself (is the timer running?), then sent
   it to the owner without trying `ask_record` on a task the description named.
2. **Missing or inaccurate information.** The owner-kept house page, revision 1 of 18 Oct, says
   the timer is "not in ansible-nas". That removes the reason to look in the fork.

**What the harness contributed, and why no code was changed.**
- *The checkout paragraph.* With `repo_checkout` but no `repo_submit`, the worker's instructions
  describe the checkout as reading "the team's submitted branch … as the task's thread gives it"
  (`turn.rs` `instructions`). They never say that a plain checkout gives the fork's develop. The
  audit kept this paragraph because no read-only loop was live; this experiment made one. Whether
  it stopped Sol checking out cannot be told: thinking is stored empty, and Sol never mentions the
  fork.
- *`ask_record` was held back* behind tool search. Calling it would have cost one search.
- *The 100-line log tail.* It is silent when the cut happens at Portainer, and it makes "since its
  last start" unreadable for a chatty container. That is a real limit, and now seen live. But Sol's
  post dated its window correctly (02:37–03:28), so it was not misled about what it saw. Even
  knowing the timer was off, it could have settled the question on uptime and restart counts.

Each of these is a contributing condition, not a demonstrated cause. The owner's rules are to build
only on observed harm, and not to compensate for model judgment with prompts. Under them, none
justified the 90-minute implementation window.

**The verification gap is by design, but it showed here.**
- HQ's evidence rule passed a final post whose criterion 2 cited a one-hour log tail for a
  two-day question. The rule checks that a citation exists, not that it is sufficient.
- The solo close then made "done" of "not yet known". A judge, or the owner, still has to read it.

## 5. What was learned

- **What Endora understood.** The question, its authority (it changed nothing), and that the
  house page and the lab disagreed.
- **What it discovered on its own.** The stack's 2-day uptime, which contradicts a 2-hourly
  restart; recent DNS and resolve errors; that the house page is stale.
- **Where it acted correctly.** It stayed read-only, cited every claim, recovered from the evidence
  refusals, and did not trust a stale record over a live reading.
- **Where it acted incorrectly.**
  - It did not consult HQ's record of 796 or the fork for the unknown it had named.
  - It read the same log twice.
  - It closed the task done on a pending answer.
- **What required human intervention.** None was taken. Its answer asks the owner for a check that
  Endora's own record already answers.
- **What the existing harness already supports.** All seven steps have a path:
  - briefing with goal and criteria;
  - tool discovery (up front and searched);
  - the record, the world pages and the fork;
  - receipted reads;
  - an evidence rule that refuses malformed claims and is recovered from;
  - limits on rounds and room;
  - ways to stop or ask.

  Claude Code, on the same model and given comparable reach, verified the goal: the model is
  capable of it.
- **What was changed.** Nothing in code. One experimental project (44) and task (1057) exist in
  live HQ. The task is done; the project is still active, with no open items, for the owner to
  drop. Nothing was pushed, merged, released or reconfigured.
- **The single most important remaining limitation.** The worker can read only the newest 100
  lines of a container's log, and is not told that older lines exist. So any goal phrased "since"
  or "over a period" on a chatty service is unanswerable through the lab tools.

  This is the one gap that would have stopped even a perfect model from weighing the 1 Nov
  episode.

## 6. Is further investment warranted?

**Not on this evidence alone.** The outcome turned on model judgment and on a stale world page,
not on a harness defect.

Two small, owner-level follow-ups are justified by what was seen:
- **Correct the house page's known-facts** (owner-kept): the timer is disabled since 27 Oct, and
  is in ansible-nas as `gluetun_periodic_restart_enabled`. This is data, not code.
- **Settle 796 from this evidence.** Claude Code's answer holds up: keep the restart off; gluetun
  recovered on its own on 1 Nov. Close the stranded 796.

If a later held-out set again shows "since"-style log goals lost, the 100-line Portainer tail is
the place to start. First make it say what it leaves out (the audit's principle, a few lines).
Only then consider a time-bounded read.

## Limits

- **One task, one sample per arm.** Run-to-run variance is known to be large; this ranks nothing.
- **Unequal reach.** The Claude Code arm had `systemctl` and an unenforced allow-list; the worker
  had HQ's record. Its cost was not metered the same way.
- **The live build lacks the audit's fixes.** Its instructions still said "the tester then
  re-checks" and "sub-tasks for members below you". Neither was acted on in this run.
- **Owner's values in the fork.** Claude Code cited `group_vars/nas.yml`, which the worker is told
  never to cite. The line it quoted is a feature flag, not a personal value.

*AI-assisted (Claude). HQ was written once: project 44 and task 1057. Everything else was read
only. Budget used: $0.009 metered plus at most $0.60 estimated, of the $10 ceiling.*
