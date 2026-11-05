# Why #796's goal closed "done" on an unresolved answer (3 November 2026)

**The question (the owner's).** Why did HQ task 1057 (run 5316, Sol on Claude Haiku, a loop worked
alone) close as done when its own answer was "not yet known", and did missing historical evidence
cause the early hand to the owner?

**The answer, in short.**
- **Access was not the cause.** The worker had two working paths to the deciding fact (HQ's
  record of #796 and the fork's history) and used neither.
- **Completion semantics were the cause of the close.** In a loop worked alone, an item with no
  branch is done once its criteria are cited. Nothing checks that the evidence is enough. The task's
  own first criterion allowed "not yet known", so a cited inconclusive answer passed every rule
  HQ has.
- **The log tool hid what it left out.** The 100-line tail was cut silently for every Claude turn.
  The worker then cited a 51-minute tail as the VPN's behaviour "since the last start". This
  bears on criterion 2 being missed. It did not cause the escalation.
- **Two harness defects were fixed, test first:**
  - the log cut is now said;
  - `set_status` now reports the item's real status.

  One policy question is left for the owner (section 4).

*Baseline:* branch `claude/integrity-audit-ceb3c2` at `354409a9`. *Fixes:* branch
`premature-close`, commits `8d340f86` and `a3506081`, local and unpushed. Live HQ was only read.

## 1. Did the worker have a usable path to the task history and the fork?

**Yes, two. Both worked, and neither was tried.**

| Path | Offered how (run 5316's transcript) | Usable? | What it would have returned |
|---|---|---|---|
| `ask_record` (HQ's record) | Held back behind tool search. But the briefing lists it by name ("Your tools at HQ: … ask_record"), and a held-back tool can be called by name: Sol called `read_world`, also held back, in round 1 without searching, and it answered. | Yes, at no extra cost | Replayed on a local copy of HQ's database, as of the turn's start. Scoped to task 796 (which the description names), the **first** excerpt is the owner's post: the timer "is stopped and disabled on the NAS since 00:10 UTC 27 Oct", ansible-nas `b3ad52a`. The second is "The periodic timer stays off (disabled)." |
| `repo_checkout` (the fork) | Up front. The tool's own description: "Check out the lab's ansible-nas fork … its default branch, or one of the team's hq/ branches". | Yes | The fork's develop, with `b3ad52a` (the Claude Code arm's first deciding fact) |

**What made the paths less likely to be taken:**
- *The briefing was stale.* It showed the house page's known-facts (revision 1, 18 Oct): the timer
  runs every 2 hours and is "not in ansible-nas". That removes the reason to look in the fork.
  It is the owner's page, not code.
- *The checkout paragraph narrows the tool.* With `repo_checkout` and no `repo_submit`, the
  instructions describe it only as reading "the team's submitted branch … as the task's thread
  gives it" (`app/worker/src/turn.rs:1033`). The tool's description says otherwise. Whether
  this stopped Sol cannot be told (see 2).
- *Unscoped, the record answers with the stale page.* Asked "gluetun restart timer disabled" with
  no task, all five excerpts are recent turns' conversations. They hold their briefings, which
  quote the same stale known-facts. Newest-first ranking puts those echoes first
  (`domain/record.rs` `choose`). This would have misled a worker that asked without naming 796.
  Sol did not ask at all, so it caused no harm in this run, and nothing was changed.

**The briefing had no history of #796.** The description names "Task 796" in plain words, and
`world:project/4` (#796's project) says "No sections yet". HQ does not inline a named task's
thread. Nothing in the evidence shows that it should: the record held it and was one call away.

## 2. Why did the worker stop investigating?

**No limit stopped it. It stopped because the criteria, as written, were satisfiable without the
deciding fact.**

- **Limits, all far from reached:** 6 of 30 rounds; $0.009 of a $1.50 cap; an opening of
  ~15.6K tokens with nothing cut from the conversation. No refusal came before its answer.
  Two cuts happened, and both were disclosed: the 6 KB result cap, on the gluetun log and the
  map ("10445 bytes of this result's middle left out").
- **Its path:** round 1, status plus the house page and #796's project page. Rounds 2–4, six
  reads of logs and the map, gluetun's twice (r3992, r3997: the same 100 lines). Then the answer.
- **What it named as the missing fact:** "What would change the answer: the NAS timer state (I
  cannot read systemd from the lab tools)". It treated the timer's state as readable only from
  systemd.
- **What the criteria allowed:** criterion 1 reads "yes, no, or not yet known". Criterion 4 asks
  for "one action for the owner". An answer of "not yet known; owner, run these commands" meets
  both as worded.
- **What it was told:** "Check its criteria yourself … setting it ready closes it". The turn's
  steps end with "exactly one of" set_status, sub-tasks or `ask_owner`.

**Unattributed.** Every thinking block in the transcript is empty, so the reason it never tried
the record or the fork cannot be read. "Model judgment" stays a hypothesis.
- The comparison does not settle it. The Claude Code arm had `systemctl`, which no worker tool
  offers, and an unenforced allow-list. It was not a same-capabilities benchmark.
- Equivalent tools and evidence were not shown to give equivalent decisions.
- Why the thinking text is empty was not traced: the request asks for adaptive thinking, and the
  stored blocks have no text.

## 3. Did the 100-line log limit conceal material evidence without disclosure?

**Yes, on both counts.**

- **The limit.** The console asks Portainer for `tail=100`
  (`domains/capabilities/src/infrastructure/terminal.rs:1005`). For gluetun, that was
  02:37–03:28 UTC on 3 Nov: 51 minutes of a stack up 2 days.
- **Material.** The best evidence on the question was outside the window. gluetun's own
  healthcheck restarted the VPN 7 times on 1 Nov, 08:53–09:02 UTC, and DNS recovered by 09:32
  with no timer.
- **Undisclosed.** The node's "older lines left out" note fired only when the node cut the log
  itself, **and** the turn asked for no number of lines, **and** it was not a Claude turn
  (`app/node/src/api/contracts.rs` `container_logs`, before `8d340f86`). Sol was on Claude and
  asked for 100 lines, so in this run the note could never fire. Its receipts read "The newest
  lines of gluetun's log", with no word that older lines existed.
- **It reached the evidence.** Sol's first `set_status` labelled criterion 2 "Stack runs and VPN
  behaviour since the last start: gluetun DNS EOF warnings 3 Nov 02:37–03:28 (r3997)". That was
  the tail, cited as the whole window. (Its post dated the window correctly, and the goal
  report took that to mean it was not misled. The evidence list shows it was, for criterion 2.)
- **It did not cause the escalation.** The escalation turned on the timer's state, which no log
  holds.

**Fixed:** `8d340f86`. A read that comes back at the console's most (100 lines) now says "older
lines left out: this tool reads at most a log's newest 100 lines, so it cannot show what came
before the first line here", whoever asked and however. Below the most, it adds the hint to ask
with `lines`. Portainer's tail is the same constant. A log shorter than the most is said to be
whole.

## 4. How did an inconclusive result become a completed task?

The path, call by call (HQ's record: posts 8963–8965, closure of task 1057):

1. Sol calls `set_status in_review` with the reason "Settled as not yet known; the owner's NAS
   timer check decides it", and five citation lists.
2. `set_status_citing` → `judge_work` (`domains/team/src/application/engine/evidence.rs`). It
   checks that every criterion cites a receipt, and that the live criteria (1, 2) cite one from
   this turn. It does not check that the citations are enough. Two attempts were refused and
   corrected; the third passed.
3. `set_status_held_lock` (`engine.rs`): the loop is worked alone, so `ready_alone` runs. The task
   has no branch, so it is not gated. The only check is `ci_can_be_had`, which passes.
4. `review_asked` → `done_alone`: HQ posts "HQ's checks passed and Sol set it ready: done, with
   no review". It sets the task Done and records the closure `met_criteria`, by `endora`.
5. The tool answered Sol "Task 1057 is in_review." By then the task was already done.

**The three claims, as HQ kept them in this run:**

| Claim | Where HQ records it | Run 5316 |
|---|---|---|
| The agent finished its turn | run `succeeded`, `end_reason` "ended its turn" | True |
| The agent completed its investigation | the member sets `in_review` | Sol said the answer awaited an owner check, in the reason HQ stored |
| The user's goal is satisfied | closure `met_criteria` ("it met its criteria") | Not verified; the judge found criterion 2 unmet |

HQ merged the second claim into the third. In the team's loops, a tester re-reads the work with
her own readings before it is done (ADR 0114: member → tester → lead). The solo arm removed the
tester on purpose (ADR 0142, amended 2026-11-01, the owner's piping audit). So for a solo item
with no branch, "done" means only that the criteria were cited.

**Fixed:** `a3506081`. `set_status` reads the item back. When HQ moved it on, it says so: "Task
1057 is done, not in_review: HQ moved it on as you set it (its thread says why)."

**Left unchanged: what a solo close may count as done.** This is a policy question. Neither the
ADRs nor the evidence settle it. The options:

| Option | What changes | Against it |
|---|---|---|
| A. Keep it | Nothing. The arm measures the model unaided; a judge or the owner reads the result. | "Done" keeps reading as verified for unverified work. |
| B. Name the closure | A solo, ungated close is recorded as "its criteria were cited, not checked", not `met_criteria`. It is a new name in ADR 0124's set; nothing else changes. | A new closure name, with its display and API surface |
| C. Use the existing non-success outcomes | The rule says that a member whose answer waits on the owner uses `ask_owner` or `blocked` (both exist). | It is a prompt change, which the owner barred until code and receipts settle it. HQ cannot tell "not yet known" from text without a judge. |
| D. Owner reads solo results | A solo close lands as "ready to read" on the owner's desk | Against the owner's "no routine approval asks" and the arm's purpose |

The cheapest honest change is B. It is the owner's call, since it changes what HQ may call done.

## 5. Corrections and their tests

| Commit | Defect (confirmed in run 5316) | Test |
|---|---|---|
| `8d340f86` fix(node) | The 100-line log cut was silent for Claude turns and for turns that asked `lines` | Unit: `a_log_read_to_the_consoles_most_says_older_lines_may_be_left_out` (Claude asked and unasked, house asked: told; 5 lines: told, with the hint; 30-line log: whole, nothing said). No `.feature`: the node's contracts have no scenario suite, and the team's scenarios fake the node. |
| `a3506081` fix(team) | `set_status` reported the asked status, not the item's | Scenario: *One member works a loop alone*, first scenario, a new step "the answer says 'is done, not in_review'" |

What these do not change:
- the worker's prompt;
- the evidence rule;
- the solo close;
- tool deferral and `ask_record`'s ranking.

No new agents, planners or stores were added.

**Gate** (`scripts/gate.sh` on `premature-close` at `a3506081`), all green:
- fmt and clippy (`-D warnings`);
- 2,219 unit tests, 223 scenarios, 2,182 steps, 0 failed;
- check-console, nothing-personal, collection parity and `git diff --check`.

The baseline's scenario count is unchanged (179 in `endora-team`); one step was added.

## What remains unattributed

- Why Sol did not try `ask_record` on #796 or check out the fork. The thinking text is empty.
  The stale house page and the narrow checkout paragraph are contributing conditions, not
  demonstrated causes.
- Whether the record's newest-first echoes of a briefing would mislead an unscoped search in
  practice. They would have here, but no run has asked that way.
- Whether option B (or any) would change outcomes, rather than only labels.

*AI-assisted (Claude). Live HQ was read only. A copy of its database was read in a scratch folder
and then deleted. Nothing was pushed, merged, released or reconfigured, and no live run was
started.*

## 6. Addendum: option B, built (the owner's choice, 3 Nov)

**Built.** Commit `9b0e7ce2` on `premature-close`, local. A solo item with no branch still closes
on its own, with its own reason: `completed_unverified` (`DoneWhy::CompletedUnverified`, ADR
0124 amended), said as "it was completed, not verified". Where the difference now shows:

| Where | Before | Now |
|---|---|---|
| Closure record | `met_criteria`, "it met its criteria" | `completed_unverified` |
| Thread | "HQ's checks passed … done, with no review" | "… completed, not verified. Nothing checked that the evidence settles it …" |
| `set_status` answer | "is done, not in_review" | adds ": it was completed, not verified …" |
| Feed `task.changed` | status only | `"unverified": true` |
| Scorecard (`finished`) | counted in `done` | in `done`, and again in `unverified`; the card says "N done, k not verified" |
| `experiment.py metrics` | "Reported (HQ done)" | adds `reported_done_unverified`; the row reads "2 (1 not verified)" |
| Console task panel | "done" | "Completed · Not verified" |

**What is unchanged:**
- authorization and execution;
- the solo item's automatic close, with no owner review;
- the gated solo path, which still closes as `met_criteria` and is tested so;
- the team's tester path;
- the worker's prompt.

The new reason is HQ's own: it is not in `Closure::names`, the set a member or the owner gives.
Past closings, `met_criteria` included, read back as they were. No rows were migrated, and none
had to be: the `closures` table's `why` is free text, with no check constraint.

**Tests:**
- Scenario: *An item with no branch is completed, not verified*, in
  `one-member-works-a-loop-alone.feature`. The item cites a reading for each criterion and is
  closed with nothing checked.
- The gated scenario now asserts `met_criteria`, and a feed that says it was verified.
- Units: `closure` (the name, round trip, old rows); scorecard
  `a_task_completed_with_nothing_checked_is_counted_apart_as_not_verified`;
  `test_experiment.py` `test_done_with_nothing_checked_is_counted_apart`, which covers stores
  with and without the closures table.

**Gate** on `9b0e7ce2`, all green:
- fmt and clippy;
- 2,221 unit tests, 224 scenarios, 2,201 steps, 0 failed;
- console, nothing-personal, parity and diff checks.

`scripts/test_experiment.py` (27 tests) passes too. The gate does not run it.
