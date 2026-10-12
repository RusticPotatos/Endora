# Cleanup plan: eight things holding the project back

Written 2026-10-12. One item at a time; each is finished and verified before the next starts.
Nothing here includes secrets or personal values; where a step needs one, it says the person
does it.

**Order, and why.** Rotate keys first (minutes, and nothing depends on it). Split the big PR
next, so everything after lands on a clean base. Housekeeping is independent and quick. The two
behaviour fixes need the clean base and a live check. Loose ends and docs follow the code they
describe. File splitting goes last: it is mechanical, it touches everything, and it is easiest
once nothing else is in flight.

Every item ends the same way: `cargo fmt --check`, `cargo clippy -D warnings`,
`cargo test --workspace`, `node scripts/check-console.mjs`, `make personal-check`, then a diff
scan for secrets before anything is committed. Deploys use a rollback tag first.

---

## 0. Rotate the three exposed keys (about 15 minutes, person does it)

**Why.** The deep-model key, the Home Assistant token and the Ticketmaster key were printed into
a working session while inspecting the live database.

**Steps.**
1. Deep-model provider: create a new key, revoke the old one, paste the new one in
   Settings → Models → Deep.
2. Home Assistant: create a new long-lived token (Profile → Security), delete the old one, paste
   it into the Home Assistant skill's settings.
3. Ticketmaster: the skill is off. Revoke the key at the provider; leave it blank.

**Done when.** The three old credentials are revoked and the node's Test buttons pass with the
new ones. **Risk.** None to the repo. Home Assistant automations that use the old token break
until updated.

## 1. Split PR #383 into reviewable pieces (about half a day)

**Why.** It carries the goal runtime, Ask deep, the retry, the chat redesign and the context
inspector. Reviewing it as one unit is not realistic, and the release gate (8 passed, 32
unexecuted) still blocks.

**Steps.**
1. Commit the pending context-inspector work (ADR 0088) on the branch.
2. Cut four branches from `develop`, in dependency order, each with its own tests and ADRs:
   goal runtime (0079–0085); Ask deep and the retry (0086–0087, `deep_turn.rs`); chat UI
   (markdown, composer, docked layout, thinking row); context inspector (0088).
3. Open one PR per branch, each stating its checks and what it was not tested against. Close
   #383 pointing at them.
4. Decide, per gate scenario, whether to **run it, rewrite it, or retire it**; record that in
   the gate file so "32 unexecuted" becomes a number that moves.

**Done when.** Four PRs, each green on its own, merge in order into `develop`. **Decision for the
person.** Whether to keep the gate as a release blocker or demote the scenarios that cannot run
on this hardware. **Risk.** Splitting commits that touch `usecases.rs` and `api.rs` will need
hunk-level staging; if a split is not clean, merge two pieces rather than force it.

## 2. Housekeeping: branches and dependency PRs (about 2 hours)

**Why.** 118 local and 54 remote branches (2 remote ones merged into `develop`); five Dependabot
PRs open since August.

**Steps.**
1. Produce a list of branches **merged into `develop`** and a second list of **unmerged branches
   with their last commit date and author**. Show both; delete nothing yet.
2. After approval: delete the merged ones locally and remotely; for each unmerged one, the
   person picks keep, archive as a tag, or delete.
3. Dependabot: merge `uuid`, `http-body-util`, `futures-util`, `ureq` after the suite passes on
   each. Take `rusqlite` 0.39 → 0.40 on its own branch, because a minor bump of a 0.x crate can
   break the database layer; run the full suite and a snapshot-restore check before merging.

**Done when.** Branch lists are short and intentional; no Dependabot PR older than a week.
**Risk.** Branch deletion cannot be undone from the client, which is why step 1 is a list and
tags preserve anything unsure.

## 3. Replace the word-list heuristics with a router (about 1 day)

**Why.** `asks_about_the_world`, `says_it_found_nothing`, `asks_what_it_did` and the kind-route
gates are keyword lists. One missed "what specific games did they win/loose" live and the model
invented the scores.

**Design.** One yes/no call to the **local** model before the turn: *does this need a live
lookup, is it about the person's own things, or is it small talk?* Nothing leaves the device.
Keep the word lists only as the fallback when the router is unavailable. Keep the deep-model
retry as the safety net behind it.

**Steps.**
1. Write the router as a port with a scripted test double; test the three classes and the
   fallback.
2. Wire it where the retry trigger is decided; measure it against a fixed set of 40 real
   phrasings (the live misses so far, plus controls) with the local model, and record the result
   in an ADR.
3. Delete the word lists that the router makes redundant.

**Done when.** The fixed set scores at least as well as the lists did on every item they got
right, and gets the known misses. **Risk.** A 7B router can be wrong; the cost of a wrong "yes"
is one deep-model call, and a wrong "no" is the status quo.

## 4. Smaller prompt, easier tool choice (about half a day, partly the person's settings)

**Why.** About 100 tools are offered; the skill list is sent twice (text and tool definitions);
the measured second-hop success of local models was 1 in 12.

**Steps.**
1. Stop sending the skill list as text when tool definitions are sent. Check with the context
   inspector that the section disappears and the total drops.
2. Person: in Skills, turn off tools they do not use (the 41-tool Pipeworx server first) and
   nominate the Brave tool as that server's reader so search is in front.
3. Re-run the tool-choice evaluation (`tool_choice_eval`, ignored test) before and after; record
   the numbers.

**Done when.** The inspector shows the reduction and the evaluation does not get worse.
**Risk.** Removing the text list could change a small model's behaviour; the evaluation decides.

## 5. Known loose ends (about 1 day, in this order)

1. **Pseudonyms in tool-call arguments.** `Deeper::continue_turn` does not restore redacted
   details inside a skill's inputs; a placeholder can reach a skill. Red test first, then fix.
2. **Streaming flash.** Text that is filed away as a Thinking row can briefly show in the
   bubble. Hold the first sentence until a step or the end arrives, or stream into a staging
   area; test with a scripted stream.
3. **Live evaluations have no schedule.** 31 `#[ignore]` tests only run when someone remembers.
   Add a make target and a weekly run on the NAS that writes results to a dated file.
4. **ansible-nas Mac copy** is 9 commits behind; it is not the working copy. Either fast-forward
   it or remove it so nothing is edited there by mistake.

**Done when.** Each has a test or a recorded run; the Mac copy is current or gone.

## 6. Refresh the docs (about 2 hours)

**Why.** The ADR index summary counts "eight accepted", 30 lines show no status, and ADRs
0079–0088 are not reflected in it.

**Steps.** Set a status on every ADR line; rewrite the "what they add up to" paragraph from the
accepted ones; add short pointers from `architecture.md` to the new modules (`deep_turn`,
`context_report`); update `docs/plans/goal-handoff-review.md` with what shipped and what did
not. Run the personal-data check.

**Done when.** The index's counts match the files and a reader can find each shipped rule.

## 7. Split the large files (about 2 days, one file per PR)

**Why.** `usecases.rs` 16.5k lines (about 260 tests inside it), capabilities
`infrastructure.rs` 10k, `api.rs` 9k, `app.js` 3.8k. Every change pays for it in compile time
and review.

**Method.** Move code without changing it, one seam per PR, tests moving with the code they
test. No behaviour change in these PRs, so the suite and the console check are the proof.

**Order.** `usecases.rs`: chat turn, egress and deep path, context building, goals. Then
`api.rs` by route area. Then capabilities `infrastructure.rs` by skill group. Then `app.js` by
view into plain script files served beside it.

**Done when.** No source file over about 3,000 lines, test counts unchanged, `git diff -M`
shows moves rather than rewrites. **Risk.** Merge conflicts with anything in flight, which is
why it is last.

---

## What needs the person

| Item | Decision or action |
|------|--------------------|
| 0 | Rotate the three keys |
| 1 | Keep or demote the 32 unexecuted gate scenarios |
| 2 | Approve branch deletions from the list |
| 4 | Turn off unused tools; nominate Brave |
| all | Say "deploy" after each change that should go live |
