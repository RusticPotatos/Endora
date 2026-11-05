# Discovering and trusting existing information (3 November 2026)

**The question (the owner's).** Can Endora's workers pursue goals on accurate task history,
relevant documentation, correctly described tools and reliable evidence? Fix what direct evidence
proves wrong. Report what only the owner can confirm.

**The answer, in short.**
- **Corrected in code (2 commits, local):**
  - the read-only checkout paragraph now says what the tool does;
  - `ask_record` no longer offers two retired records;
  - briefings and downtime texts no longer point at retired tools.

  Each correction has a test first. The gate is green.
- **Corrected outside the repo:** the owner's house map said gluetun is restarted every 2 hours.
  A read of the NAS today shows the timer disabled and inactive.
- **Proposed, not written:** four of HQ's six world sections are contradicted by the record or the
  code. Three are the owner's own and reach every briefing. Exact replacement text is in
  section 3.
- **Not changed: the record search's ranking.** It does what ADR 0129 says. Replayed on live data,
  an unscoped question about the gluetun timer never even reaches #796's deciding post. That is a
  design question for the owner (section 4).

*Baseline:* `rc-2026-11-03` at `3fcee523` (the live build). *Work:* branch `knowledge-fidelity`
in `~/repos/Endora-knowledge`, commits `f35a0fc6` and `5a333e55`. Live HQ was only read. Nothing
was pushed, merged, deployed or written to HQ.

## 1. Corrections in code

| # | Incorrect or misleading text | Authoritative evidence | Change | Test |
|---|---|---|---|---|
| 1 | Read-only checkout paragraph (`app/worker/src/turn.rs` `instructions`): `repo_checkout` "with a branch's name … puts the team's submitted branch in /work/repo". It never says what a checkout without a branch gives. | `Mirror::bundle` (`domains/capabilities/src/repo.rs:662`): no branch → the fork's base branch, bundled whole (`git bundle create - refs/heads/<base>`). The tool's own contract says so ("its default branch, or one of the team's hq/ branches"). #796's deciding fact was in that history (`b3ad52a0`). | One sentence: "Without a branch it puts the fork's default branch there, whole history behind it, so git log shows what the lab has had committed and when." `f35a0fc6`. | Unit `a_tester_is_told_the_wings_git_…`, extended. It failed before the change. |
| 2 | `ask_record`'s description: it searches "… stories, notebooks and the conversations of turns". | Stories left the search in `9eb65f35`; notebooks were retired with their tables in `5c2bebd0` (both ADR 0142, 27 Oct). `SqliteTeamStore::search` reads posts, handovers, conversations, sections and timelines only. The text reached **29 live turns** since 28 Oct (HQ's `transcripts`). | "Stories, notebooks" dropped from the description and the port's doc comment. `5a333e55`. | Unit `ask_record_names_only_the_records_it_searches` |
| 3 | A briefing cut to its budget says "Read any of them with read_world (your notes with read_notebook)". | `read_notebook` is no longer offered by any code path (`5c2bebd0`). It did not reach a live turn: no transcript since 28 Oct holds it. | The parenthesis dropped. `5a333e55`. | The `situations` unit's exact string, updated first |
| 4 | Downtime refusal: "React to a proposal, propose a small change to the office, or note an idea in your notebook". Downtime briefing: "create_task and hand_to are refused". | Proposals and votes were deleted in `a451d20c` (`office_proposals` dropped). Notebooks were retired in `5c2bebd0`. `hand_to` is no tool. `MAKES_WORK` is `["create_task"]`. A downtime turn is offered `my_turn` and `chess` alone (`games_tests.rs:618`). | "Make the chess move it is for, if one is due, and stop"; "(create_task is refused)". `5a333e55`. | Unit `a_downtime_refusal_points_only_at_what_a_downtime_turn_has` |

No `.feature` scenario was added. These change what a tool or briefing says, not what HQ does. The
team's scenarios assert behaviour, and none read these strings.

**Gate:** green on `5a333e55` (see "Checks" at the end).

## 2. Correction outside the repo: the owner's house map

`~/.claude/house-map.md` (the owner's global instructions ask agents to keep it true) said
"gluetun (restarted every 2 h by a systemd timer; no playbook fallback)".

**Evidence, read 3 Nov ~05:50 UTC:**
- On the NAS, read-only: `systemctl is-enabled gluetun-stack-restart.timer` gives `disabled`;
  `is-active` gives `inactive`; `LastTriggerUSec` is empty.
- On ansible-nas `origin/develop` (`ee878f76`), `gluetun_periodic_restart_enabled: false` is in
  `group_vars/nas.yml:194`, set by `b3ad52a0` (26 Oct 20:08 EDT, "for the 48-72 h test window
  (Endora #796)"). The role default is also `false` (`roles/gluetun/defaults/main.yml:16`), and
  `nas_maintenance` installs the timer only when it is on.

**Now:** "its 2-hourly restart timer is off: `gluetun_periodic_restart_enabled: false` on
ansible-nas develop since b3ad52a, 27 Oct; `gluetun-stack-restart.timer` disabled and inactive on
the NAS, read 2026-11-03". This is not in a repo, so nothing was committed.

## 3. HQ's world pages: proposed text, for the owner to write

The owner's sections are always briefed: their headlines in every turn, their bodies through
`read_world` (`engine/memory.rs` `house_pieces`). Writing them is a production change, so here is
the text only.

### 3a. `world:house#known-facts` (the owner, revision of 18 Oct). Contradicted.

**Wrong:** the first bullet says gluetun's group is "restarted every 2 hours on purpose, by a
systemd timer … set up by hand on 10 May 2026 and **not in ansible-nas** … A gluetun-group restart
every 2 hours is expected, not a fault."

**Evidence:**
- the NAS and fork readings in section 2;
- the owner's own post 5221 in #796's thread (27 Oct 00:10 UTC): "the timer is stopped and
  disabled on the NAS since 00:10 UTC 27 Oct", ansible-nas `b3ad52a`.

Ada had already found the switch in ansible-nas on 26 Oct (post 5025).

**Proposed first bullet:**
> - gluetun's 2-hourly restart of its group (gluetun, deluge, nzbget, prowlarr) is **off** since
>   27 Oct 2026, for #796's test window. ansible-nas declares it: `gluetun_periodic_restart_enabled`
>   (role gluetun; the timer comes from role nas_maintenance), false on develop since `b3ad52a`.
>   `gluetun-stack-restart.timer` is disabled on the NAS. A gluetun-group restart is no longer
>   scheduled, so one is worth a look.

**Proposed heading:** "What we know about the lab (as of 3 Nov 2026)". The other bullets are
unverified here and are left as they are (section 5).

### 3b. `world:house#priorities` item 1 (the owner, 18 Oct). Overtaken.

**Wrong:** "the 2-hour gluetun restart is the owner's own timer … if so, bring it into ansible-nas
as a branch so it is declared, not hand-made."

**Evidence:** it is declared already (`roles/gluetun/defaults/main.yml:16–18`,
`roles/nas_maintenance/defaults/main.yml:56–59`), and switched off (section 2).

**Proposed:**
> 1. Ops: settle #796 on the test window's evidence: keep gluetun's periodic restart off, or turn
>    it back on (`gluetun_periodic_restart_enabled`, already in ansible-nas).

Whether to keep it off is the owner's decision (section 6).

### 3c. `world:area/1#2-hourly-gluetun-timer-alarms` (Ada, 20 Oct). Contradicted.

**Wrong:** "Every 2 hours the owner's timer restarts gluetun … Mara (CEO) decided: keep the timer."

**Evidence:**
- the readings in section 2;
- the owner's "yes" to the timer-off window (post 5195, 26 Oct);
- post 5221.

**Proposed:** supersede it. Its "why": "The timer is off since 27 Oct (ansible-nas b3ad52a; NAS
timer disabled). Exited alarms for gluetun's group are no longer explained by it."

### 3d. `world:house#how-we-work` (the owner, 18 Oct). Contradicts ADR 0142. Owner's call.

**Wrong:**
- "Mara (CEO) sets direction and decides proposals … Ops (Ada) … Engineering (Kit)";
- "Stuck work climbs a ladder: the lead decides, then a huddle, then Mara."

**Evidence:**
- ADR 0142 (accepted by the owner, 27 Oct) supersedes the chain of command (0111) and the roles
  (0120). It says "escalation by model tier is the only escalation".
- `protocol.rs` `tools`: "There are no roles". `create_task`: "work is never handed to another
  member".
- HQ's stuck-work messages now say "takes it a model tier up".

**Proposed** (the first two bullets; the other three stand):
> - The owner is the board. There are no roles or chain of command: each member works its own
>   loop's items, and what it cannot do goes one model tier up, then to the owner's desk.

This page is the owner's rules, so it is offered for confirmation, not as a settled fact.

## 4. The record search: why a broad question finds the stale page and not #796

**Replayed on live HQ** (read-only, 3 Nov ~05:30 UTC), with the store's own queries and `choose`'s
rules, unscoped, for "gluetun restart timer disabled":
- **Posts.** The scan takes posts holding **any** of the words, the 60 newest. On a busy service
  those reach back only to 1 Nov 15:56. The owner's post 5221 (27 Oct), which holds all four words,
  is **never a candidate**.
- **Asked from a cloud model** (Sol's case), the five excerpts are posts from 1–2 Nov that mention
  gluetun and "restarting". Recent Confidential turns are dropped by the tier rule.
- **Asked from the house model,** the first excerpts are the latest turns' conversations. With
  "gluetun periodic restart timer" these include R5316, R5315 and R5308, which quote the stale
  known-facts from their briefings ("see known facts").
- **Requiring half the words in the scan** (what `choose` keeps anyway) changes nothing here: the
  60 newest still begin on 1 Nov.
- **Scoped to `task: 796`,** the first excerpt is post 5221 (premature-close report, section 1).

**Why it was not changed.** The code does what ADR 0129 decided: "any of them", "the newest five".
Ranking by match strength, or scanning further back, would change what every member gets back.
That is a design change to an accepted ADR, and the evidence is one replayed question. No run has
yet asked unscoped and been misled (Sol did not ask at all).

**What would fix the stale echoes at their source is section 3a.** Turns stop quoting the wrong
fact once the page is right. Old transcripts keep it for 90 days, as testimony of what was said.

## 5. What remains uncertain (owner to confirm)

| Fact | Where | Why it stays |
|---|---|---|
| "Four downloads are stuck on import … there is no import tool yet" | house known-facts, 18 Oct | No local evidence either way. Not inferred from old records. |
| owntone, owntone-spotify, mongodb-old-9.0 retired | house known-facts; area/1 | Consistent with the house map. Not re-read live. |
| Priorities 2 and 3 (AGENTS.md draft, Docker-socket access, Watchtower) | house priorities | Watchtower's coverage changed on 31 Oct (goal-understanding report); whether these are still the priorities is the owner's call. |
| Whether the timer should stay off | #796 | A decision, not a fact. The evidence (gluetun recovered on its own on 1 Nov) is in the goal-understanding report. |

## 6. Does this improve tool or context fidelity?

**Yes, by small and verifiable amounts.**

**Tool fidelity:**
- A worker with a read-only checkout is now told it can read the fork's history: the path to
  #796's answer.
- `ask_record` no longer promises records it cannot return.

**Context fidelity:**
- No briefing or refusal points at a retired tool.
- The owner's map is true about the timer.
- The largest remaining error is in HQ's own pages (section 3). It is one write by the owner.

**What this does not show:**
- that any of it changes a worker's decisions;
- why Sol did not try the record or the fork (its thinking was stored empty).

Both are for a held-out run, if the owner wants one. Nothing was benchmarked here.

## 7. What was deliberately not built

Nothing was built:
- no new search, ranking, index, memory store, agent or prompt framework;
- no change to tool permissions, security policy, deferral or the evidence rule;
- no HQ page written;
- no benchmark;
- the ADRs and CHANGELOG that still mention stories and notebooks were left alone: they are
  history.

## 8. Owner decisions required

1. **Write sections 3a–3c** (or your own text) on HQ's pages: the timer facts.
2. **Confirm or rewrite how-we-work** (3d): it still describes the retired chain of command.
3. **The record's ranking** (section 4): keep newest-first (ADR 0129), or consider match strength
   for unscoped questions. Recommendation: keep it until a run is shown misled, and fix the page
   first.

## Checks

`scripts/gate.sh` on `5a333e55`: all green.
- fmt and clippy (`-D warnings`);
- 2,224 unit tests, 224 scenarios, 2,201 steps, 0 failed;
- check-console, nothing-personal, collection parity and `git diff --check`.

The scenario count is unchanged.

*AI-assisted (Claude). Live HQ was read only. The NAS was read with `systemctl` (read-only).
Changes are local commits on `knowledge-fidelity` (Endora) and this report on
`codex/agent-collaboration`. Nothing was pushed or deployed.*
