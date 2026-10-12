# 0084 — Tell the person what actually went wrong

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Claude).

## Context

ADR 0082 delivers every way a goal ends. Running the real pass loop against a
live model (local `qwen2.5:14b`, five runs, two objectives) showed that the
delivered text was unhelpful:

- A source the egress guard refused ended as `Partial`. The model's apology and
  its question back to the person ("Is there another URL I can use?") had been
  kept as findings and were shown under "What I found". The real reason — the
  tool's refusal, present on the receipts — never reached the message.
- "What did not work" printed the loop's own bookkeeping behind a placeholder
  step label ("next step: the pass produced nothing").
- A correct, source-backed answer was introduced with "I only got part of the
  way" and a "what is still missing" that was only the loop's reason for
  stopping, which undersold it.

## Decision

**A finding must be able to rest on something retrieved.** In `resume_goal`, the
model's prose for a pass is not committed as a finding when the pass retrieved
nothing and either (a) the pass made calls and every one failed, or (b) the pass
made none, nothing has ever been retrieved by this goal, and something has
failed. The real tool errors are remembered as a dead end instead. A pass that
read anything, a goal that has read something before, and a goal that has never
touched a tool are unchanged. Because no finding is stored, the existing
nothing-gathered rule ends such a goal `Blocked` rather than `Partial`.

**The notice says only what a person can use.**

- "What did not work" is the tool errors from the receipts, then any dead end the
  loop did not write about itself. Each is one line, a failure repeated or
  contained in a longer one is said once, and the most recent three are kept.
- The loop's own bookkeeping strings are named constants compared exactly, never
  guessed at in model or tool output, and are left out of the notice.
- `Partial` means unverified, not unsuccessful. With findings it reads "Here is
  what I found on…", lists them, and states "I couldn't confirm this fully
  answers your request." Without any it reads "I couldn't finish".

The live evaluation harness now runs each pass through the ownership-guarded
runner and renews its lease on a timer, as `spawn_goal_worker` does, and prints
the exact message and the tool receipts. Before that, a pass over thirty seconds
lost its claim and failed the evaluation for a reason unrelated to what it
measures.

## Validation and limits

Unit tests cover each notice shape, deduplication, the bookkeeping filter and the
bound. Application tests cover each branch of the finding rule, including the
controls that must not change; the refused-pass test was confirmed to fail with
the rule disabled. Against the live model, the refused-address objective ended
`Blocked` with the real refusal and no invented finding, and the answerable
objective's message carried its finding and the unconfirmed statement.

Not addressed: the answerable objective still ends `Partial`, because a general
completion verifier does not exist, and the live evaluation's `Complete`
assertion stays red for that reason. The model sometimes restates a finding in
different words, which the novelty check does not catch. A pass with no tool call
on a goal that has never touched a tool is still kept as a finding, so a model
question on such a goal can still appear as one. The older repeated-failure
route in `pursue` still ends `Partial` with nothing gathered, a case the worker
does not reach. This is five runs of one model on two tiny objectives, not a
reliability rate; web search, chat intake, the schedule verifier and the real
worker were not part of it.
