# 0114 — Evidence, acceptance criteria and the tester

## Status

Amended by [0142](0142-one-team-of-loops.md), 2026-10-27: one team of loops.

Accepted, 2026-10-17, by the owner. Prepared with AI assistance (Claude). Amended 2026-10-19 by
[0124](0124-guards-against-how-agent-teams-fail.md): **the owner's work says what done means
too** (at least one criterion, or Give work is refused), a routine's task carries criteria from
its routine, and a sub-task of any task with criteria says which it serves. Amended 2026-10-18 by
[0124](0124-guards-against-how-agent-teams-fail.md): a tester's pass keeps a hash of the
criteria it judged and, once they change, reads "needs re-check" and does not count; a member
whose evidence already meets the criteria is told to submit, its re-checking turns after that
are talk only, and on the third HQ submits it; the back-off no longer reads the progress flag
for a turn with no label; done and dropped carry a reason from a small set. Amended 2026-10-18 by
[0120](0120-roles-as-an-enforced-raci.md): a criterion may wait (`after`) from the work's last
change, the lab's "no new alarm since" 60 minutes; a reading the turn before already took shows
nothing; the CEO's chain acts on a lead's area become `direct`, and a lead's stalled task is
never handed up to the CEO; stuck work, and work sent back three times, climb a ladder (the
lead, a huddle, then the CEO) instead of the owner's desk; a project's lead closes its own work
on its own acceptance. Amended 2026-10-18 by
[0117](0117-claude-code-as-a-developer-runtime.md): a Claude Code turn's own tools (its Bash,
Edit and the rest) leave **sandbox-reported** receipts, posted by its hook and recorded by the
node with the turn's token but not witnessed by it; one may be cited for what the sandbox did,
and a live criterion still needs a fresh reading the node witnessed. Amended 2026-10-17 by
[0115](0115-the-engineering-wing-and-the-git-flow.md): a branch the wing submits and the check
of it are receipts the node and HQ write (a document on the task holds the diff and the report),
the check is the task's gate, a branch that fails its checks is sent back by HQ itself, and the
owner's push is a receipt too. Hiring that acts, deferred below, is built there. Amends
[0107](0107-routines-alarms-and-check-backs.md) (the empty-turn back-off reads turn labels; a
routine's check closes on evidence; a check-back waits behind a wait),
[0108](0108-the-bridge.md) (every call through the contracts and the bridge leaves a receipt),
[0109](0109-a-brain-per-member.md) (the CEO's effort by the kind of turn),
[0111](0111-the-ceo-and-the-chain-of-command.md) (the reviewer is shown as the tester; what
reaches the board's desk, and the lead's word up to the CEO, carry evidence; the chain of
command may act on a subordinate's task) and [0113](0113-projects-budgets-and-reports-to.md)
(the CEO decides a project on evidence; one evidence rule for proposals and decisions alike).
Built: receipts and their route, criteria, the evidence rule, the member → tester → lead path
and its fallbacks, the cap on review rounds, unsupported posts, turn labels, nudges and the
hand-up, repeated readings, check-backs replaced by a wait, the manager chain's acts, the CEO's
effort by turn, the worker's and HQ's instructions, and the screen. Amended 2026-10-17
(evening) by the owner, from that day's record: **the tester passes or sends back, and never
makes work.** Her briefing says so, and that when the work is fine she passes it and says what
she checked; `create_task` is no longer offered to her (the engine refused it; live, she tried
three times before she asked). And **§5's unsupported rule is narrowed**: a member's post is
unsupported only when it cites nothing and its turn has no receipt at all (held or refused); the
worker's automatic last word is never tagged. Live, a closing post in a turn with two receipts,
which its submission cited, was tagged unsupported.

## Context

Live, task #663: the lead said it could not read the home lab, without once trying; the CEO
then confirmed the lab had recovered and was refused (the task was "held" by the lead), so a
recovered task stayed blocked for hours. The same day a developer read `homelab_status`
fourteen turns in a row to no effect, and the lead took six paid "still waiting" turns ($0.22)
on a task blocked on another. Every one of these was a model's words standing in for what the
record could have said: HQ counted posts as progress, took "done" from the member who did the
work once a reviewer approved it, and had no way to tell a reading from a claim.

The design HQ borrows checks work against what "done" means before it counts, by someone other
than whoever did it, a bounded number of times; counts what a turn did rather than what it said;
nudges a stalled worker before handing its work up; and lets a manager act on a report's work.
Endora takes those shapes and keeps its own rules: the node is the trust boundary, models
propose and code decides, and the owner is the board.

## Decision

**Owner decisions, 2026-10-17.**

1. **Receipts: tangible evidence a model cannot forge.** Every call a turn makes of Endora's
   tools through the node's contracts and bridge (done or refused) is told to HQ by the node,
   `POST /v1/team/receipt`, signed with the node's token and naming the turn by its own token;
   HQ keeps it only while that turn runs (a forged token or a finished turn is refused, 404).
   A receipt holds its id, run, member, task, tool, a short arguments summary, the first 300
   characters of what the tool gave back (what the turn itself was given, already scrubbed),
   whether it did what was asked, whether the tool acts, and whether its result is
   Confidential; it is never changed (a trigger refuses it) and, amended 2026-10-19, never
   deleted (a second trigger refuses that: evidence cited could otherwise go from under its
   citation). The model reads the id after each
   result, "(receipt r41)", to cite. A Confidential excerpt is never shown to a member on a
   cloud model: its line says so instead. HQ's own record (tasks, events: alarms, posts, moves)
   counts as evidence too, by number.
2. **Acceptance criteria.** A task carries a short list (at most eight), each one line and
   possibly **live**: about the lab as it is now. Work a member hands another member must have
   them (`create_task` refuses with `NoCriteria`); the owner's must, since 0124's amendment (the
   screen asks for at least one line, `live:` marks one, and work with none is refused;
   work already open with none is as it was); the home lab's routine and alarm tasks get two
   standard live ones: "Every
   container that alarmed is running and healthy, from a fresh homelab_status" and "No new alarm
   for the same containers since". Criteria are members' text: escaped on screen.
3. **One evidence rule** (`domain::evidence::judge`, pure): each criterion needs at least one
   reference, every reference must be in HQ's record (`NoSuchEvidence`), about this task's work
   (the task and its sub-tasks, and what it waits on: `NotThisWork`) and not a refused call
   (`RefusedCall`); a live criterion needs a receipt from a fresh turn (`NotFresh`: the
   claiming turn's own, and for an acceptance the tester's passing turn); work with no criteria
   still needs one reference. The refusal names the criterion: "criterion 2 (sonarr is running
   and healthy) has no evidence…". A decision needs one reference that is in the record.
4. **Member → tester → lead → done.** Submitting (`set_status in_review`, with `evidence`, one
   list per criterion) is judged, kept per criterion, and wakes the tester (the reviewer's role,
   shown as **Tester**; `Role::Reviewer` stays in code and data). She re-checks herself: her pass
   (`review`, `approve`) is judged with a reading of her own this turn required (`NoOwnReading`),
   and wakes the work's lead (`acceptance_asked`): its project's lead, else the worker's manager
   if it leads, else the earliest lead, else the CEO, never the worker. The lead (`accept`)
   accepts, judged with the tester's readings fresh, only once she passed it while there is a
   tester who did not do the work (`NotTested`), or sends it back. Gated work still needs the
   gate. **Nobody tests or accepts its own work**, and the fallbacks are said on the task: no
   other tester, the lead accepts alone; no other lead, the tester's pass closes it; neither,
   the owner's desk to read, as before. A routine's check closed as done by its member is judged
   the same way when it has criteria.
5. **Refuse or flag.** Missing evidence refuses: submission, a check closed, the tester's pass,
   the lead's acceptance, the CEO's `decide_project` and decisions for the board, a tester's
   passing on the CEO's decision, and the lead's question up to the CEO. A post may cite
   evidence; a member's post that cites none, in a turn with no reading, is kept and marked
   **unsupported** in every thread a member and the owner read.
6. **The tester's fact-check before the desk** (`REVIEW_DRAFT`) lists the receipts a draft
   cites, as the node recorded them; the evidence of submitted work is listed for her, and for
   the lead what she passed it on. *Amended 2026-10-19:* **a draft is never passed.** A CEO's
   draft for the board moves the work task itself into review, and `review` with `approve`
   then went through the draft's path: with the gate false it was refused for the gate, and
   with the gate true the task closed as done and its push was offered, with no evidence
   judged and no lead's acceptance (found on tasks 671, 672, 679 and 670). A pass on a draft is
   refused ("forward it with `ask_owner`, or send it back"); sending a draft back stays as it
   was.
7. **Turn labels**, from the record when a turn ends: `advanced` (a task moved or was made, a
   project changed, an act succeeded; the tester's verdict either way), `blocked` (it left its
   task blocked, or asked up the chain, the owner or for an approval), `checked` (it took
   readings), `talk only` (posts, nothing else), `empty`. Kept on the run, in `run.ended` and on
   screen. They replace the progress flag for the back-off (turns from before read the flag);
   posts are no longer progress.
8. **Nudges, then up.** After a member's turn on its task that showed nothing (talk only,
   empty, or a reading the same as its turn before), its next turn is told: "take the first
   concrete action now (a reading, a change, or evidence against a criterion), or mark the task
   blocked and say why", twice at most; the third such turn hands the task to whom the member
   reports to, once (a setting remembers it), with a post saying so. A check-back's own wake is
   not nudged.
9. **Review rounds.** Work sent back three times (by the tester or the lead) goes, on its next
   submission, to the owner's desk (`rounds`) with each reason, and only the owner advances it
   (accept as done, or send back).
10. **Waiting is not a turn.** A check-back on a task blocked on another task (what it waits on,
    or its open sub-tasks) does not fire while the wait lasts; it is replaced by the wait, said
    once in the thread, and the wait's end is the wake. Such a task is not "stranded" either.
11. **The chain of command acts.** Whom a member reports to, and anyone above, up to the CEO,
    may move, close (with evidence, as anyone), accept, send back or hand on (`hand_to`) a task
    held by that member; the post says who did it, for whom.
12. **The CEO's effort by turn** (amends 0109's cost notes): `low` for ordinary turns (a
    question up the chain, a post, a recovery, a routine wake that is not a review), `medium` for
    its daily review and planning, `high` when a project proposal waits for its decision or it
    carries something to the board (its decision sent back, the board's word). The lead stays
    `medium`, the researcher `low`.
13. **Instructions.** The worker's say how done is shown (criteria, receipts cited per
    criterion, a fresh reading for the lab now, the tester's own re-check, the lead's
    acceptance); HQ's briefings list a task's criteria and the evidence to check; every tool
    that takes evidence has an `evidence` parameter of ids saying what counts. The CEO's short
    briefing is unchanged but for two tool names (`accept`, `hand_to`).

Every check (roles, the evidence's existence and its work, freshness, tester before lead, the
rounds, the chain) is code; no model's word is taken for one.

## What was taken from the design HQ borrows

- **Done against criteria, checked by someone else**, a deliverable rather than an opinion: a
  tester's finding that work is wrong is a completed test, not a failure of hers.
- **A blocked state that is productive**: naming a concrete blocker (an approval, a credential,
  access, a clarification) is progress, so a member is not pushed to pretend.
- **Bounded nudges before escalation** in place of an open-ended back-off: a stalled worker gets
  a concrete prompt twice, then its manager the work, once.
- **A cap on review rounds**, so a disagreement between a worker and its checkers reaches the
  board instead of looping.
- **Never your own work** to check or accept, with explicit fallbacks.
- **Managers act on reports' work**, as the board already could.
- Endora keeps its own boundary: receipts are written by the node, not reported by a worker,
  which is what makes them evidence here.

## Consequences

- A turn costs a node→HQ call per tool call (one HTTP request on the team's network); a
  receipt HQ cannot keep is said in the node's log and the call's result stands.
- HQ now keeps short excerpts of Confidential results (already scrubbed by the node), shown
  only on the owner's screen and to members on the house's model.
- A routine's check with no criteria (the CEO's reviews) still closes on its member's word; the
  home lab's checks do not.
- Repeated identical readings and talk count against a member: a lead that only says it is
  waiting has its task's wait, not its turns, bring it back.
- **Deferred:** hiring that acts (tabled by the owner); commits and command output as receipts
  from the workbench's shell (its calls never pass the node); per-post ids as evidence; a
  member's own draft for the owner approved by the tester still closes as before, with no
  evidence rule of its own.

## Amended (2026-10-17): every event names its turn

For Replay (the owner's screen, [docs/plans/endora-hq.md](../plans/endora-hq.md)), every event
a turn's call emits carries the turn's run id (`"run"`), as the events of starts, ends, receipts
and spend already did; the feed's contract gains the field and loses nothing. HQ keeps when it
began (`events_name_their_turn_from`): a turn queued since has its events found by id, one from
before is matched by its member, task and time and is shown as matched so. Replay shows a
call's Confidential excerpt on the owner's screen, as the evidence card does; the node serves it
to a full credential only.

## Amended (2026-10-19): evidence as the models write it

A reference is read where the models put it, the words around it never kept: leading, with
words after it (`r228 check c7 (ok)`), and now also last, in brackets after what it showed, as
the tools' own results end (`owntone-spotify exited with code 137 (receipt r302)`). An object
is read as the ids among its values, its keys naming the kind of a bare number (`{"receipt":
"r301", "task": 695}`). A line or an object holding no id is refused as before. Run 1831, a
post-deploy check, was refused four times for these forms before citing the same ids plainly.

*Amended again (2026-10-20):* live, findings still cited receipts mid-sentence ("Uptime 'About
an hour' in r321 and r322 …"), and were refused. A finding is now read for every whole
reference anywhere in it, each once: `r12`, `receipt r12`, `e340`, `event 340`, `task 12`,
`task #12`. A bare `#1` is not one ("criterion #1" is common prose), nor a word that only looks
like one (`e2e`). This loosens nothing that matters: every reference read is judged against
the record as before (it exists, it is about this work, a live criterion's is fresh), so a
stray one cannot pass a claim, and a model's words are still not evidence.

## Alternatives considered

- **The worker reports its own receipts.** Rejected: the model's process would be the one
  vouching for itself; the node already sees every call.
- **Evidence as free text checked by a model.** Rejected: a model is never the enforcement
  boundary; ids are checked by code.
- **Flagging, not refusing, a done without evidence.** Rejected by the owner for anything that
  closes work or reaches the board; kept for posts.
