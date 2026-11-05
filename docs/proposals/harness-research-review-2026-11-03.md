# Harness research review: what the literature supports for Endora's worker (3 November 2026)

This is an independent, read-only review. Its question is which research-backed practices would
measurably move Endora's minimal coding worker toward Claude Code's verified task-completion rate
on the same model. It reads the harness-engineering literature against Endora's own record. It
changes no code and runs no benchmark.

**Endora's evidence, read for this review:**
- the [mechanism audit](mechanism-audit-2026-11-02.md) and its five area tables: set 3, the
  log-read fix, and Milestone 1's results;
- the [baseline experiment](baseline-experiment-2026-10-28.md), rounds 0–4: the runtime proof of
  concept, H1/H2, the held-out test and the gap costs;
- ADR 0142 and its amendments;
- the worker, node and team code on branch `milestone1` (`efceca76`, on live `38668a2b`);
- Milestone 1's final report.

**What Milestone 1 already addressed.** These are counted as fixed below:
- checkout caps (`ab0fb8fb`, #1051–1053);
- the outside-branch recheck (`bbf009d3`, #1048);
- criteria kept whole up to 1,000 characters (`efceca76`, #1049).

**What the literature review covered.**
- *The sources.* The Marmelab survey (Zaninotto, *The State of AI Harness Engineering 2026*,
  24 Sep 2026), followed to its primary sources.
- *How it was read.* A research subagent did the reading. Its two strongest papers were then
  re-read at the abstract here.
- *What could not be reached or found:*
  - The Reddit post (r/AI_Agents, "How to build a harness that actually improves"): every route
    to it was blocked, so nothing here rests on it.
  - OpenAI's post refused the fetch (403), so its content is known only from search excerpts.
  - Microsoft's post loaded as a title only.
  - No "University of Toronto harness paper" and no "6×" figure were found, in the survey or in
    its references. The nearest match is Ben Sghaier et al., arXiv 2607.03691 (35 Qwen Code
    releases on 50 SWE-bench tasks), whose affiliation was not confirmed.

## The evidence, graded

**Primary: measured, with a method**

- **Context files (AGENTS.md / CLAUDE.md) do not reliably raise success, and they cost about
  20% more.** Gloaguen et al., *Evaluating AGENTS.md*, arXiv 2602.11988 (ETH Zurich).
  - Setup: SWE-bench Lite and 138 tasks from repositories whose developers committed such files;
    four agents.
  - Result: LLM-written files changed success by −0.5 to −2 points. Developer-written files
    added about +2.4 points, not significant. Agents do follow the instructions.
  - Limits: Python only, one run per task, not reproduced.
  - The survey quotes "about 4%" for human-written files; the paper does not say that.
- **Prompt-file security rules seldom have an enforcing control.** T. Yan, *When "Do Not" Is
  Not Deny*, arXiv 2608.23550.
  - Setup: 481 public CLAUDE.md files.
  - Result: 4–16% of the extracted rules have a matching built-in control, depending on how
    strictly a match is defined; 4.4% (CI 2.6–6.7%) at the strictest.
  - What it measured: whether an enforcing control *exists*. It did not measure whether agents
    obey the rules or break them. Extraction caught 66% of the rules.
- **Harness modules ablated on one model.** Pan et al., *Natural-Language Agent Harnesses*,
  arXiv 2603.25723.
  - Setup: gpt-5.4-mini, single runs, no confidence intervals.

  | Module | SWE-bench Verified | OSWorld |
  |---|---|---|
  | File-backed state | +2.6 | +13.9 |
  | Context compression | −1.0 | −8.3 |
  | A verifier step | +0.2 | +8.4 |

  - Parent-to-child hand-off recall: 0.32 and 0.55, against 1.0 with direct context.
  - The survey says a second reviewer "lowered success by 8%"; the paper does not say that.
- **The same model varies 68–88% across eight harnesses**, graded on end state, not on the
  agent's claim. Composio, *8 Best AI Agent Harnesses in 2026*.
  - Setup: 25 SaaS tasks, not coding; one attempt each.
  - The author notes that one task moves the score 4 points.
- **Tool reduction.** Vercel (Qu, Dec 2025).
  - Change: 17 tools cut to 2 (bash and SQL), same model.
  - Result: 4/5 to 5/5 queries succeeded. Latency fell 3.5×, tokens about 40%.
  - The author credits a well-documented data layer. Five queries, from a vendor, not reproduced.
    This is the "80→100%".

**Secondary: practice reports, no numbers**
- OpenAI's ~100-line AGENTS.md "as a map", with docs kept in-tree and checked mechanically.
- Microsoft's SRE agent went from 100+ narrow tools and 50+ agents to a few broad CLIs and
  generalists. "More than four hand-offs fail" comes from a later summary.
- Anthropic's engineering posts:
  - *Writing effective tools for agents*: high-signal outputs, truncation that says so, errors
    that say what to fix.
  - *Effective context engineering*: context rot, compaction, clearing old results.
  - *Effective harnesses for long-running agents*: a progress file, a feature list the agent may
    only mark as passing, end-to-end checks before done.
  - *Beyond permission prompts*: enforcement in the OS and network layer.
- Anthropic's tools post is the only source that separates **information quality through a
  tool** from tool count.

**Speculation, or untraced**
- "The harness matters more than the model (46% vs 80%)."
- "Cut tools and success rises" as a general law.
- "Separate evaluator agents beat self-checking."
- Any multiplier as a planning number.

The survey's own counts are primary but descriptive. For example, only 1 of 26 repositories with
evals publishes a before/after comparison. They show what people ship, not what works.

**The owner's reading holds on every point checked:**
- the 6× figure was not found as a result;
- 80→100% is n=5 and confounded;
- information through a tool matters at least as much as tool count;
- Yan measured controls, not obedience.

The four properties (Access, Continuity, Feedback, Authority) are a sound frame. The rest of this
report uses them.

## 1. The five most important findings

1. **Endora's measured harness losses were nearly all Access and Feedback defects of one kind:
   a limit that cut or refused silently, or said something false.** This is the finding that
   matters, and it comes from Endora's own record, not the literature. Every case:

   | Where | The limit | What the model saw | Result | Status |
   |---|---|---|---|---|
   | Node and worker, repository checkout | 16 MiB node read; 10 MB worker read, error discarded | "the contract failed" / "could not be read as a bundle" | 3 of set 3's 5 losses | fixed in Milestone 1 |
   | HQ, task criteria | cut at 200 characters | the criterion, silently shortened | the #1049 loss; 21 of 49 set-3 criteria cut | fixed in Milestone 1 |
   | Container log read | 64 KB | "Portainer did not answer (protocol)" | the held-out drive task, 4 of 4 runs blocked | fixed in `cedbda73` |
   | Lab check box (#1048) | a 404 from a check that should never have been queued | a failed check | shut the gate and counted as loop failures | fixed in Milestone 1 |
   | Worker turns | 12 rounds | turns cut mid-work | 3 of 12 held-out homelab tasks for the old worker | fixed: 30 rounds (H1) |

   In each case the model reasoned correctly from what it was told, and what it was told was
   wrong. Anthropic's tools guidance says the same (truncation and errors must say what happened).
   It is secondary evidence; Endora's record is the primary.
2. **Prompt-level behaviour changes did not carry over, and the literature agrees.**
   - H2, a "check before done" paragraph, scored 4/4 on the tasks it was written from. It cleared
     the bar on the held-out homelab set and failed the off-domain one, so it was withdrawn.
   - ETH found that agents follow context files but gain nothing reliable from them, at about 20%
     more cost.
   - The cheapest-looking lever, more words to the model, has the weakest record in both places.
3. **Multi-agent structure cost money and bought no quality, in Endora and in the literature.**
   - Endora's rounds 2–3: 83–84% of turns changed nothing. Climbs moved three fifths of round 3's
     money to Sonnet. Reviews lost four finished tasks. The solo loop replaced it.
   - NLAH measured hand-off recall at 0.32–0.55. Microsoft collapsed its 50+ agents. No primary
     source shows a reviewer agent reliably helping (NLAH's verifier: +0.2 on SWE-bench).
4. **Authority in Endora is already enforced in code, which is what Yan's paper shows prompt
   files usually fail to do.**
   - The sandbox is offline. Contracts are the only way out.
   - The git fence, the forbidden paths and the privacy scan are checked by the node.
   - A new tool defaults to Restricted.
   - Receipts are written by the node, not reported by the model.
   - "Only the gate makes done."
   - Some lab rules in the worker's instructions are reminders of these enforced limits (one
     restart an hour, never touch inventories). They are not themselves the boundary. That is the
     right shape.
5. **The literature has almost no controlled evidence at Endora's scale. Endora's own held-out
   method is stronger than most of what it would borrow.**
   - The strongest external results are single-run ablations, n=25 to n=300, on models other
     than Haiku.
   - Endora's held-out sets are blind-judged, run on the same model, with the bar fixed before any
     run. They decided H1 and H2 more reliably than any paper could.
   - So the literature sets direction: state lives in files, enforcement in code, cuts are
     honest. It does not justify building anything.

## 2. What Endora already does correctly

- **Continuity through file-backed state, not compaction.**
  - What Endora has: the kept item sandbox; branches kept on the node and checked out again
    whole; the turn's work committed at a budget cut; a handover line.
  - What the literature supports: this is the NLAH module with the largest gain. Compression,
    which Endora does not do on Claude turns, was its loss.
  - Measured: prompt-budget cuts have been 0 since 1 November.
- **One loop, one agent, one fresh context a pass** (the solo loop, ADR 0142's Ralph design).
  There is no parent-to-child hand-off for information to leak through.
- **Feedback that reports what happened:**
  - Shell results give the exit code and keep 16 KB, head and tail.
  - Results past 6 KB keep the start and the end, and say what was left out and how to ask
    for less.
  - `image_tags` reports how many tags it read and how many matched.
  - Since Milestone 1, a too-large answer says it is too large.
- **Done is judged on end state.**
  - Node-written receipts.
  - The evidence rule, kept after its own replay: 45.8% would have passed, against a 90% bar.
  - Blind external judges for experiments.
  - This matches the only practice the strong sources share: grade the state, not the claim.
- **Enforcement in code** (finding 4), kept apart from efficiency mechanisms in the audit's
  Protect list.
- **Tools trimmed by use.**
  - Held-back tools are deferred.
  - The audit removes 16 bridged tools (0 calls in 3,482 runs) from work turns.
  - Deferral measured its own cost: held-back calls guessed arguments 33% of the time, about
    0.1 round a turn. That is small and known.
- **Changes are measured one at a time, against a pre-set bar, on held-out tasks**, with harm
  counted apart (H2 withdrawn, H1 kept). The survey says this is rare: 1 in 26.

## 3. Gaps supported by actual Endora failures

| Failure (task, run) | Cause, by the record | Property | Status |
|---|---|---|---|
| #1051–1053, pyparsing never reached the sandbox | two buffer caps below a 19 MB answer; one error discarded | Access, Feedback | **fixed** (`ab0fb8fb`); whether the worker then solves the tasks is unmeasured |
| #1049, a requirement lost | criterion cut at 200 characters, silently | Feedback | **fixed** (`efceca76`) |
| #1048, gate shut, loop failures counted | `recheck` lacked the outside-repository guard | Authority (a missing check) | **fixed** (`bbf009d3`); the remaining part is model judgement |
| Held-out drive task, 4 of 4 blocked | 64 KB log read reported as "protocol" | Access, Feedback | **fixed** (`cedbda73`, verified live) |
| Held-out homelab, 3 of 12 lost by the old worker | 12-round cut, each turn starting over | Continuity | **fixed** (H1, 30 rounds) |
| Round 4 R3, a release two majors back | the model asked for `semver_only`; the tool reported its counts truthfully | model judgement | no harness gap |
| #1042, R2 (all Haiku arms) | model judgement or depth | — | no harness correction |
| #1048, 7 of 13 runs "HQ answered 502" or timed out | HQ unavailable around releases | Continuity (infrastructure) | **open**; cost retries, decided no verdict |

**What is left open.** No measured loss remains that a harness change explains. Two things
remain without a loss on record:

- **One more silent drop of the class Milestone 1 fixed.**
  - Where: `Criterion::from_lines` (`domains/team/src/domain/evidence.rs:227`) keeps the first
    `MOST_CRITERIA` = 8 criteria with `.take()`, and drops the rest without a word.
  - Status: never observed; set 3 averaged about 4 criteria a task (49 across 12).
- **The 502 window.**

## 4. Recommendations to reject

Each of these lacks an Endora failure it would fix, or has evidence against it.

- **An AGENTS.md or context file for the worker, or for the repositories it works in.** ETH found
  no reliable gain at about 20% more cost. No Endora loss traces to missing repository
  conventions. The worker's instructions are already about 5 KB.
- **A reviewer, verifier or evaluator agent; any new role.** Endora's rounds measured the cost
  with no quality gain. The literature is mixed at best, and the survey's "−8%" is misquoted.
- **Context compression or summarising history.** NLAH measured it as negative. Endora has had 0
  budget cuts since 1 November, and the kept sandbox already carries state.
- **Markdown memory, "self-evolution" or loop-lesson stores.** These were mixed in NLAH. The
  audit found Endora's loop lessons carry little content.
- **Further tool reduction as a success lever.** The case is Vercel's n=5. Endora's remaining
  cost is about 0.1 round a turn. The audit's removal of unused tools is housekeeping, not a
  reliability change.
- **A prompt checklist such as "verify before done".** H2's held-out result rejects it.
- **Security or approval rules written as instructions.** Yan shows these seldom have an
  enforcing control. Endora's boundaries must stay in code, and none should move into prompts to
  save code.
- **A shell receipt, an edit tool, API retries, a larger reply cap.** Each was measured on
  2 November as not needed, or is already decided on real traffic.
- **Any multiplier from the literature (6×, 80→100%, 46→80%) as a planning number or a target.**

## 5. Improvements, ranked by expected value (two; the evidence supports no third)

**1. Finish Milestone 1's class: no limit on what reaches the model cuts or drops silently.**
- *The claim.* Every past-limit input either is refused with a statement of the limit, or is cut
  with a statement of what was cut. This applies on the worker's read paths, the node's contract
  answers, and HQ's task fields.
- *Sources.* Endora's record: five failures of this class, four of them decisive (finding 1).
  Anthropic's tools guidance is secondary support.
- *What Endora has.* Most sites already comply (the 6 KB cut, the shell's head and tail,
  `image_tags`' counts, the new contract-answer cap).
- *The known exception.* `from_lines`' `.take(8)`.
- *The smallest correction.*
  - A one-hour, read-only sweep for silent `take`, `truncate`, read-limit and `.ok()` sites on
    those paths.
  - Each one found becomes a stated refusal, as `written` already does for length. Nothing new
    is added.
- *How each fix is justified.* A unit test per site where an input past the limit fails before
  the fix and is stated after it. This is Milestone 1's method; it needs no benchmark.
- *Expected impact.* It closes the class that caused most measured harness losses. Its value on
  future tasks is unknown but cheap to buy. Tokens and attention are unaffected, except that a
  loss is understood at once instead of after a trace hunt.
- *If the sweep finds no other site.* Fixing `from_lines` alone is a few lines.

**2. Remove the instructions' statements about checks that no longer exist.**
- *The claim.* The worker's instructions say things about the loop that are no longer true:
  - `app/worker/src/turn.rs:1055`: "The tester then re-checks it with her own readings, and the
    lead accepts it."
  - `turn.rs:940`: a home-lab question "goes to the team's reviewer first".
- *Why they are false.* ADR 0142 removed testers. The reviewer turn is dormant. Solo items close
  on mechanical checks.
- *The evidence of harm is weak but direct.*
  - The audit names the risk: a model told someone will re-check may lean on a check that is not
    there.
  - ETH found that agents follow what such text says.
  - In round 4 the solo item "closed on lint and syntax alone".
  - No loss has been traced to these two sentences. This is a correction to existing text, not
    a gain claim.
- *The smallest correction.* Delete or correct the two sentences, together with the audit's
  already-decided removal of the tester persona. The existing unit tests that pin the
  instructions' text change with them, and the change comes with a scenario, as the audit asks.
- *How it is judged.* On real traffic, by the evidence-in-proportion rule. It is a reversible
  text change, so it needs no blind set.
- *Expected impact.* Small and unmeasured. It removes one way the harness misinforms the model,
  in the same direction as improvement 1 (the Feedback property).

**No third.**
- The worker's shell leaves no receipt. That causes evidence-rule friction (127 free-text
  refusals), but no verdict was decided by it, and Milestone 1 rejected it on that ground.
- The HQ 502 window is operational: the release drain and its timing. It is not a harness
  practice.
- Neither earns a place here.

## 6. The one recommended next action

**Release Milestone 1, then run the held-out comparison Milestone 1 itself named.**

**The test, as Milestone 1 framed it:**
- 12 fresh tasks picked blind, the worker against Claude Code.
- The same model, Haiku, on the same whole criteria.
- At least two of the repositories larger than the old caps, so that the transport fix is
  exercised.
- Judged blind, with the bar set before any run.

**Why this test, and not more harness work.**
- Set 3's 7/12 is partly an artifact: 21 of its 49 criteria differed between the arms. So no one
  yet knows the worker's real gap after the four fixes.
- The literature has no result that would outrank that measurement.

**Improvement 1 can land first, in the same release.** It is read-only to find and changes only
how limits are stated, so it cannot move a verdict except by preventing a silent loss.

**What would show more work is needed.** Harness work beyond these two changes is justified only
if the rerun shows a loss whose trace diverges from Claude Code's at a harness cause. If the
remaining losses are model judgement, as #1042 and R2 were, the evidence is sufficient, and the
harness work stops there.

*AI-assisted (Claude). Read-only: no code, benchmark, push or release was part of this review.*
