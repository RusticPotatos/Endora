# 0115 — The engineering wing and the git flow

## Status

Amended by [0142](0142-one-team-of-loops.md), 2026-10-27: one team of loops.
Amended 2026-10-31 by the owner: accepted work reaches the fork without a card, and HQ may merge
develop behind a setting (see the last section).
Amended 2026-10-31 by the one-team clean-up (ADR 0142, "What goes"): the check after a deploy is
retired. There is no Deployed card (kind `deploy`), no check of the lab after one, and no
`deploy_check` card; the section on it below is history.
Amended 2026-11-01 for round 4's F4: an item's project may name one public repository outside
the lab, which the node mirrors and the wing's two contracts work in; its branches stay on the
node and are never pushed (see the last section).

Accepted, 2026-10-17, by the owner. Prepared with AI assistance (Claude). Amends
[0106](0106-team-contracts.md) (two more contracts, `repo_checkout` and `repo_submit`, for a
lead or a developer on any brain), [0110](0110-the-teams-rings.md) (the check job is the first
piece of ring 2, the lab, run model-free on the Pi's runner), [0112](0112-the-team-knows-how-the-lab-is-built.md)
(the fork is read at its own line, asked of GitHub when the owner names none), [0113](0113-projects-budgets-and-reports-to.md)
(hiring that acts: the owner's form, and a lead's request within the wing's seats) and
[0114](0114-evidence-acceptance-criteria-and-the-tester.md) (a branch and its check are
receipts and documents; the check is the task's gate). Built: the mirror and the two contracts
on the node, the worker's checkout and submission, the check job on the runner and in the
workbench image, HQ's branches, documents, checks, hiring and the wing seeded once, the push on
the owner's approval, and the HQ screen's wing, cards and forms. The owner's scenarios in
`domains/team/features/the-engineering-wing.feature`. Amended by
[0118](0118-elastic-staffing-and-notebooks.md) (2026-10-17): a lead with a backlog asks for
staff with `request_staff` (the project's open tasks as evidence, at least twice its active
developers); a developer on furlough is offered back to the lead first, and with none the
request becomes the hire request of rule 7, under the same seats. A member on furlough still
counts as a seat.

Amended 2026-10-18 by [0117](0117-claude-code-as-a-developer-runtime.md): a wing developer on
Claude may run on Claude Code instead of Endora's worker, the owner's choice per member; HQ
seeds the pilot once, Rae on Claude Code and Zed on the worker, compared by the cost of a
finished task. The worker checks the fork out into `/work/repo` before Claude Code starts, and
`repo_submit` reaches it through the worker's MCP bridge, so the bundles are still the worker's
to carry.

Amended 2026-10-19, after the wing's first 39 hours (six branches submitted, none reached the
owner; the diagnosis found every blocker on the path): **a member may check out the team's own
kept branch** (decision 2) and **a branch is named for its task** (decision 4); the check's
base compare is hardened (decision 5); the tester may check a branch out for her own reading,
a lead or a developer may ask for a **recheck**, work in review is never demoted to blocked, a
draft never closes work, and a new head sends work back for a new review (decision 6). Each is
marked where it lands.

Amended 2026-10-19, at the owner's request: **the check after a deploy.** HQ never sees the
owner merge a branch or run the playbook, so nobody looked at the lab after a change landed.
Now a push the node reports done puts a card on the owner's desk (kind `deploy`): **Deployed**
or **Won't deploy**. Deployed hands the member of the routine that takes the home lab's alarms
(the Ops lead today) one task, in that routine's goal and project, "Check the home lab after
deploying <branch>", with one live criterion and the change's own criteria quoted. The routine's
own task is left to its alarms. The card takes one word: a second click is refused, as any
answered desk item is.

That check's turn is narrowed in code at three layers, whatever the model is told:

- HQ offers it four of its own tools (`my_turn`, `claim`, `post`, `set_status` to done or
  blocked) and refuses every other one.
- HQ's answer to the node's `/v1/team/turn` names its node tools (`only`: `homelab_status`,
  `container_logs`, `nas_status`), and the node lists and runs those alone. Every other
  contract or bridged tool (a restart, the fork, its docs, the lab's map, anything a tool server
  offers) is refused before it runs, and the refusal is kept as a failed receipt.
- HQ tells the runner `shell: false`, and the runner starts the turn in the same fence without
  the workbench's shell, git or Claude Code.

The check closes as done only on the evidence rule's terms: a reading of the turn's own,
cited. Its member may close it, as a routine's check is closed.

**One turn, one verdict.** When that turn ends, HQ gives the verdict once:

- **Healthy** if the check was closed done (the digest);
- **Unhealthy**, in the member's words, if it was set blocked;
- **Inconclusive** otherwise.

Each is said as "Post-deploy infrastructure check: Healthy" (or Unhealthy, or Inconclusive),
and a Healthy one says it checks the infrastructure, not that every part of the change works:
never "verified".

A check that is not healthy is closed as dropped, and an interrupting card says so (kind
`deploy_check`). Each verdict is also posted in the change's thread. A wake on a check that has
its verdict is refused (`checked`).

Limits, accepted: the reads see container health, logs and the NAS's metrics, not files on the
NAS or the rules Prometheus loaded, so a healthy verdict means the lab ran clean after the
deploy, not that every line of the change was seen working. The owner's Deployed is the only
signal that a deploy happened. Scenarios: "Deployed starts one read-only check of the lab, and
its healthy verdict comes back" and "A deploy check that ends without a verdict is
inconclusive, never healthy".

Amended 2026-10-18: the check job has every collection and role the fork needs, offline. The
image builds `endora.homelab` from the Endora source it was built from, into the same path as
the other baked collections, because the fork pins it to a git commit the sandbox cannot fetch;
the image build fails if the collection or its roles are absent.

Amended 2026-10-18: the trailer no longer depends on a hook running. The sandbox's `/work` is
a Docker tmpfs, mounted `noexec`, where git ignores the `prepare-commit-msg` hook, so every
submission was refused for lacking its trailer. The worker now stamps the trailer onto any
commit of the member's `hq/` branch that lacks it, with git plumbing, when it bundles the
branch (the node's check is unchanged); the hook stays for where it can run.

## Context

The team could keep the home lab healthy (0106, 0108) and plan (0113), but could not change
how the lab is built: the lab is the owner's public ansible-nas fork on GitHub, and nothing
reached it. Ring 1 (0110) gave a turn a shell and git in an offline sandbox on the Pi, with no
way to bring code in or send it back. The owner wanted a second team that works on the fork
through branches, read and checked before anything leaves the house, and decided how far that
goes for now: **an audit period**, in which HQ pushes nothing on its own, opens no pull request
and merges nothing; the owner alone pushes an accepted branch, and merges it on GitHub.

## Decision

**Owner decisions, 2026-10-17.**

1. **The repository** is the owner's public ansible-nas fork (`ENDORA_ANSIBLE_NAS_REPO`,
   `owner/name`), and only that. Its **base** is `ENDORA_ANSIBLE_NAS_REF` when the owner sets
   it (the fork's own line is `develop`; `main` only mirrors upstream), else the fork's default
   branch, which the node asks GitHub for once per process (`GET /repos/{owner}/{name}`,
   `default_branch`) and takes as `main` when GitHub does not say. The docs reader (0112) reads
   the same branch. Compose no longer defaults the ref to `master`.
2. **Checkout into the sandbox** (`repo_checkout`, a read, Internal, offered to a lead or a
   developer on any brain: the fork is public). The node keeps a **bare mirror** of the fork
   under its data folder (`/data/team-repo/mirror.git`), cloned and fetched over plain HTTPS
   with no credential, refreshed at most every five minutes, and hands the turn a **bundle** of
   the base branch, or of one of the team's `hq/*` branches to review, as base64 in the
   contract's result (32 MB cap; the fork is about 6 MB; a bundle carries the branch's whole
   history, since a shallow one cannot be cloned alone). *Amended 2026-10-19:* a team branch
   the fork does not have is the team's own **submission**, kept as `refs/submissions/…` on
   the node until the owner's push, and a member could never have it back (three refusals of
   a branch the node was keeping; each ended in the work recreated). The node now asks HQ
   (`POST /v1/team/branch/checkout`, naming the turn by its token), whose **record** decides,
   never the branch's name: the newest branch recorded under that name must be for the
   turn's own work (its task, the tasks under it, the tasks above it) or have been submitted
   by the turn's member, and HQ answers with the head and base it recorded. The node hands
   out one bundle of the base and that kept ref, at that head only (a branch submitted again
   since is refused, not swapped), and the checkout's receipt names the branch and its head.
   HQ answers 404 both for a token that opens no running turn and for a name it never
   recorded, and says which; *amended again 2026-10-19,* the node tells them apart: a turn
   that has ended is told "no running turn has that token: this turn is over" (401, which
   ends it), not that the fork has no such branch. A branch the fork has is handed out from the fork as before. The worker, in the
   workbench only, unpacks it into `/work/repo` with git set up the team's way: commits authored
   `Endora HQ <hq@endora.invalid>`, no remote at all, and a `prepare-commit-msg` hook that adds
   the trailer `Made-by: Endora HQ (<members>) for task #N` when a commit lacks it. The model
   reads where the checkout is and what its base is; it never sees a bundle. The sandbox stays
   offline. For a kept branch the worker clones the base, fetches the kept ref as a head of
   its own and checks it out, with the base still a head behind it, so it can be added to and
   submitted again; and it empties `/work/repo` rather than replacing the folder, so a shell
   standing in it (Claude Code's) is not left in a folder that is gone.
3. **The work**: branches named `hq/task-<N>-<slug>` (the slug from the task's title, `[a-z0-9-]`,
   at most 40), cut from the base. The model never pushes: no remote and no credential exist in
   the sandbox.
4. **Submit** (`repo_submit`, an automatic act, Internal, a lead's or a developer's). The worker
   bundles the branch from the base (`git bundle create base..hq/…`; uncommitted changes refuse
   it) and sends it as base64. The node takes it only when: it holds exactly that one branch,
   under `hq/` and **named for the turn's own task**, `hq/task-<N>-<slug>` (amended
   2026-10-19: the kept ref is keyed by the name alone, so a name that carries its task is a
   key no other task can write; a sub-task that goes on with its parent's branch submits it
   under its own name); it applies to the mirror and **sits on top of the base** (a fast-forward; a
   branch behind the fork's base is sent back to rebase); it changes **at least one file** (a
   probe with no file in it was accepted and checked on task 679; commits that add up to nothing
   are no branch to review) and at most **200 files** and
   **1 MB** of diff; **every commit is HQ's** (the author and the trailer); and it **touches
   nothing of the owner's values**: `group_vars/*.yml`, `host_vars/`, any inventory or anything
   named for a vault is refused whole, and the briefing says never to edit, cite or copy from
   them. *Amended 2026-10-30 (review of round 2):* both sides of a move count, and a name is
   read as it is: the node lists the files with `git -c core.quotePath=false diff --no-renames
   --name-only -z`, so moving `group_vars/all.yml` to `roles/all.yml` touches `group_vars/`
   (git listed only the new name), and `group_vars/é.yml` is under `group_vars/` (git quoted it
   as `"group_vars/\303\251.yml"`, which matched nothing); HQ's own reading of a recorded diff
   takes both sides of each `diff --git` line and unquotes them. What passes is kept under the task (`submissions/task-N/<slug>-<sha>.bundle`, and as
   `refs/submissions/hq/…` in the mirror, which the mirror's fetch never prunes: the
   submission is **staged** apart and moved there only once every rule has passed, so a
   refused attempt, a missing trailer being the commonest, leaves the kept branch as it was),
   and told to HQ
   with the turn's own token (`POST /v1/team/branch`): HQ records the **branch**, writes a
   **document** on the task, "branch hq/… (N files, +a −b)", with the stat and the diff (64 KB
   shown), says so in the thread, and queues the **check**. The call's receipt (0114) is the
   turn's to cite. No desk notice per submission: the thread, the document and the push card
   are where the owner reads it. *Amended 2026-10-19:* **the push is bound to the head that was
   reviewed.** A new head submitted while the task is in review voids the review (the task goes
   back to `doing`, saying a new head needs a new review) and resets the gate to wait for the
   new head's own check; the gate is the newest head's, so a check of an older head that reports
   late is recorded and sets nothing, wakes nobody and sends nothing back; and the card offers
   the newest submitted head, which is therefore the one the tester and the lead judged.
5. **The check job**: the first piece of the lab (ring 2), **model-free**. HQ hands the Pi's
   runner a check before any turn (`POST /v1/runner/next` → `kind: "check"`), with a token of
   the job's own. The runner starts the workbench image with the same fence as a turn and
   nothing of HQ's but that token (`ENDORA_CHECK=1`, no run token, no model, no relay). The
   job fetches the branch's whole history from the node's team listener
   (`GET /team-relay/check/{id}/bundle`, opened by the job token, which the node checks with HQ),
   runs the fork's own checks each within five minutes — `ansible-playbook --syntax-check nas.yml
   -i tests/inventories/integration_testing` (the inventory the fork's tests use),
   `ansible-lint --offline nas.yml` (the fork's CI pins ansible-lint 6.14.6; offline because the
   sandbox cannot fetch `requirements.yml`, so the image bakes the fork's collections and roles
   in), `yamllint .` (1.30.0) — and reports pass or fail, a line ("syntax ok, ansible-lint 2
   findings, yamllint ok") and each command's last 4 KB to HQ through the gateway
   (`POST /v1/runner/check/{id}/done`, the job token as the bearer; taken once). **A branch is
   judged against its base** ("no worse than develop"): the fork's own develop already carries
   findings, so a branch with any failure has the same checks run on the commit it was cut from
   (the node names it, from the branch HQ recorded, in an `x-endora-base` header on the bundle),
   and passes when no check is worse there: ansible-lint findings and yamllint errors no more
   than on the base, syntax failing only where the base's did, a timeout always a fail. The line
   then says both ("ansible-lint 25 findings (develop 25), …: no worse than develop"; since
   2026-11-01 "…: no new findings against develop: met", ADR 0142's D22; since 2026-11-03
   "…: no more findings than develop: met", which is what a comparison of counts proves) and the
   report holds the base's outcomes; a clean branch costs no second run, and a base that cannot
   be found leaves the plain pass or fail, said in the line. *Amended 2026-10-19:* a check that
   **could not run** (its tool not found or not executable, exit 127 or 126; killed; out of
   time) is no finding. A base on which any check could not run was not read and excuses
   nothing: the failure stands and the line says "develop could not be checked: …", so a box
   without its tools (every check at 127 on both sides, as c1 and c2 were) passes no branch,
   where "develop failed too" had read as "no worse". A branch's own check that could not run
   is never excused by a failing base either. HQ keeps the
   result as a **receipt** on the task and a document with the report, and as the task's
   **gate** (0114: code work closes only once it passed); a pass wakes the submitter and,
   for work already in review, its tester (*amended 2026-10-20*: before review a pass gave the
   tester nothing to judge; of eight such wakes on 18-19 Oct, five turns asked the lead what
   to do), and **a branch that fails its checks is sent back** by HQ itself, the tester
   untroubled. A check that never reports is given up after thirty minutes; the runner reports
   one that never started or ended without reporting. *Amended 2026-10-19:* a check was queued
   only by submitting, from a sandbox that held the branch, so a check that failed for the
   box's own reasons cost a recreated branch each time (task 674 existed only to check one
   commit again). A lead or a developer on the work now asks for **`recheck`**: a new check of
   the newest kept branch of the work, with no sandbox and no new submission, one at a time
   (refused while a check of that branch waits or runs), judged against develop as before; the
   result is a receipt on the turn that asked, and wakes it with the branch's member and the
   tester. The workbench image gains a venv with
   `ansible-core` 2.15.13 (the engine alone, not the `ansible` package of every collection: the
   checks never load them and the arm64 build would take minutes more), the two linters, the
   fork's four collections and two roles.
6. **Review and push**, the 0114 flow: the developer submits the task (`in_review`) citing the
   branch's and the check's receipts; Dee, the tester, reads the diff and the report
   (`read_document`) and **checks the branch out** (`repo_checkout`) and passes it on her own
   readings; the wing's lead accepts; the task is done. *Amended 2026-10-19:* this was
   written but not true: the node offered the tester neither wing contract, and
   `read_document` leaves no receipt, so she had no reading of the branch to take, and none of
   her three turns on task 670 could pass it (0114 holds a pass to a reading of her own).
   The node now offers her `repo_checkout` alone, never `repo_submit`: checking the kept
   branch out (HQ lets her, as the work's tester) is a witnessed receipt on the task that
   names the branch and its head, which she cites with the check's. Her briefing says so, and
   her turn, a turn of judgment on her Haiku (0131), runs in the same sandbox as the others.
   *Amended 2026-10-19:* **work in review is never demoted to `blocked`** by HQ's sweep of
   stranded work. After one nudge of the tester and a quiet window it was set `blocked` (task
   670, 44 minutes after it entered review), out of the tester's reach (she passes work in
   review) and settable back by its holder alone; it climbed to the CEO five times. It stays
   in review and its lead is woken to decide it, up the same ladder (0120).
   **Then** the push
   is the owner's: a desk card, "Push hq/… to your fork (N files, checks passed)", with Approve
   and Decline. On Approve, HQ names the branch to the node, which pushes the **stored
   submission** to the fork over SSH with the deploy key at `ENDORA_GIT_PUSH_KEY_FILE`
   (mounted from `ENDORA_GIT_PUSH_KEY`), with GitHub's own host keys and nothing else trusted
   (`StrictHostKeyChecking=accept-new`, a known-hosts file the node writes). The push code
   refuses any ref not `refs/heads/hq/*`, never forces (no `+`, no `--force`: a branch the fork
   has moved is refused by git and the owner hears why), never touches the base or `main`, and
   refuses a remote with a credential in its address. What came of it is a receipt in the
   thread, which wakes the member, and an audit line; the card's outcome says where the branch
   is. With no key on the node, the card says pushing is not set up and offers the branch's
   bundle to download (`GET /v1/hq/bundles/{task}/{slug}`) for the owner to fetch into their own
   clone. **HQ never opens a pull request and merges nothing**: the owner merges on GitHub.
7. **Hiring.** The owner hires on the HQ screen (`POST /v1/members`, through the node and its
   audit log: a name, a role, a brain the role may think with, whom the member reports to);
   HQ refuses a second CEO. A lead may **request** a hire (`request_hire`: a role below its own,
   a brain, why, evidence from HQ's record, at most one request waiting per lead): a card on the
   owner's desk, and Approve hires, reporting to the lead who asked. **The wing's seats are a
   rule in code**: one lead on Claude Sonnet (or the house's model) and at most **five**
   developers on Claude Haiku (or the house's model), counted among the wing's members (its
   projects' leads and whoever reports up to them); a request beyond them is refused with the
   count. The owner's own form is not limited; the screen says the agreed seats.
8. **The wing**, seeded once on start with the demo off (a setting remembers it): the owner's
   goal **"Improve the lab"**, with the wing's notes for the team (the job, the base, the flow,
   the rule about the owner's values, and that the owner merges); the project **"ansible-nas
   clean-up"** under it, led by **Kit** (a lead on Claude Sonnet, reporting to the CEO); **Rae**
   and **Zed** (developers on Claude Haiku, reporting to Kit); and Kit's first task, "Plan the
   clean-up: read the fork's docs and propose the first three branches", with criteria that it
   cites the files it read by receipt and names three `hq/task-N-<slug>` branches. Dee stays the
   one tester for both teams. No routines for the wing yet. What an older HQ already has (a Kit,
   the goal) is kept, not doubled.
9. **Documents.** A task carries documents (a branch's diff and stat, a check's report), named
   in its briefing and read with `read_document` (16,000 characters a part) by a member whose
   work it is or who leads; the HQ screen shows them as text.
10. **The office** gains the wing's room: six desks and a door onto the lower lane, where the
    wing's members sit and rest; the front door moves to the floor's right wall.

Models propose; deterministic policy authorizes: every refusal above (ref names, sizes, the
fast-forward, the author and the trailer, the owner's paths, the seats, the roles, the evidence,
one report per check, one push per approval) is code on the node or in HQ's engine.

## Consequences

- The team can change how the lab is built, and nothing of it leaves the house without the
  owner's two acts: Approve on the card, and the merge on GitHub.
- A deploy key with write access to the fork lives on the NAS, readable by the node alone. It
  can push `hq/*` branches and nothing else by Endora's code; GitHub itself should protect
  `main` and `develop` (branch protection rules), so a mistaken push elsewhere is refused there
  too.
- The check job is the lab's first piece: a long-lived test environment is not. The checks are
  the fork's own linters, offline; a role's `molecule` test, which needs Docker in the sandbox,
  is not run.
- The workbench image grows by the venv and the collections (on the order of a hundred
  megabytes); it is built on the Mac for arm64 and copied to the Pi as before.
- A cloud developer on Haiku reads the diff it wrote and the fork's public files: nothing
  Confidential, since the owner's values never enter a branch.

## Deferred

- The full lab on the Pi (ring 2): long-lived containers a branch is tried in before the house.
- Endora's own repository as a second target; a second fork.
- A pull request opened by HQ; HQ merging anything. Not until the owner says the audit period
  is over, in a record of its own.
- The egress proxy (0110) stays deferred: the fork's docs and the checks' tools are baked in.

## Alternatives considered

- **Pushing from the sandbox with a key.** Rejected by the owner (0106, 0110): no key enters a
  sandbox; the node pushes, and only on approval.
- **A pull request opened by HQ.** Deferred for the audit period: the owner reads the diff on
  their desk first, and merges by hand.
- **The full `ansible` package in the image.** Rejected for size and build time on arm64; the
  checks load only the fork's collections, which are baked in.
- **The check as a model's turn.** Rejected: the lab's own linters are the gate (0114), and a
  model's word about them would be a claim, not a reading.

## Amended 2026-10-31: accepted work reaches the fork without a card

**The owner's decision, 31 Oct.** Merging accepted work "is just not something I need to review",
and no "Ready to merge" or "Push" cards on the desk. This is the record the Deferred list above
waited for ("HQ merging anything. Not until the owner says the audit period is over"), and it
amends [0142](0142-one-team-of-loops.md)'s "plus ready PRs" on the desk. Scenarios:
`domains/team/features/accepted-work-reaches-the-fork.feature`.

**What changes.**

- **No ready-to-merge or push cards.** An accepted branch is listed on HQ's Manage tab, folded
  to one line: "Accepted, waiting to merge", with its task, its branch, when it was accepted and
  where it stands (pushing, on the fork, held, failed, not pushed and why). A merged branch leaves
  the list. Cards already waiting stay until answered. What a researcher wrote is still the
  owner's to read, and work nobody on the team can review still reaches the desk as before.
- **The push is HQ's, by rule.** A branch is due to be pushed only when its review passed (the
  item is accepted), its newest check passed, and HQ's mechanical checks find no forbidden path
  and no CI criterion that cannot be met (`domains/team/src/domain/delivery.rs`, `may_push`). Any
  other branch is listed with why it was not pushed; without a deploy key, with its bundle.
- **The privacy scan, before every push.** The node asks HQ what is due every minute
  (`GET /v1/team/deliveries`) and scans the branch's whole diff against where it left develop,
  and every commit message, by code (`domains/capabilities/src/outgoing.rs`): only what the
  branch adds is read. It looks for the shapes of `scripts/check-nothing-personal.mjs` (a real
  mailbox, a real network address, coordinates, a phone number, a long hex string, with the same
  allowed examples), the kernel's secret tripwire (`shared/kernel/src/secrets.rs`), and the
  owner's own values. If it finds anything, nothing is pushed and one desk card (kind `privacy`)
  says what and where (`a real network address (roles/x/defaults/main.yml line 3)`), never the
  value; the owner sends it back to the team or, having read it, pushes it anyway. A branch the
  node cannot read is not pushed: the scan could not tell.
- **Where the owner's values come from.** `ENDORA_PERSONAL_VALUES`, the same comma-separated list
  `make personal-check` reads, in the node's environment and in no tracked file: the Makefile
  exports it from the git-ignored `local.mk`, Compose passes it to the node, and the `endora`
  role renders it from `endora_personal_values`, which is meant to come from the owner's vault
  (`{{ vault_endora_personal_values }}`). Absent, the scan reads shapes and the tripwire only, and
  every push says so in the thread ("no personal values are set on the node").
- **What needs the owner still reaches the desk.** From the same diff, by code: a role default
  that now reads a `vault_*` variable it did not before, and an assert the change adds that a
  variable is set (`is defined`, `| length > 0`, `!= ""`, `is not none`). Each puts one card (kind
  `needs_owner`) naming the variable and the file. Everything else, a push or a merge, is a
  notice that clears itself, with a receipt and a post in the change's thread.
- **Merging develop, behind a setting that is off.** With `delivery_merge_develop` on, the node
  merges each pushed branch into the fork's develop: a fast-forward when develop has not moved,
  else one merge commit authored by Endora HQ with its trailer; a conflict merges nothing and is
  listed for the owner's hand. Never a force, never `main` or `master`, never upstream, and only
  branches HQ pushed under this rule. The setting stays off while the baseline experiment's rounds run
  (the fork's develop is frozen at ee878f7); the owner, or the assistant, turns it on when they
  end.
- **The cards come back on a setting.** With `delivery_cards` on, accepted work puts the "Ready
  to merge" and "Push" cards on the desk exactly as before, and nothing is pushed without them.
  Both switches are on the Manage line, and `POST /v1/hq/delivery` through the node.

**Can today's deploy key update develop?** Yes, with no new credential. Read on 31 Oct from
GitHub's API: the fork's deploy key `endora-hq` has write access, and `develop` is not protected
and has no rulesets. The push code refused the base branch; the merge is a separate path that
pushes the base only as a fast-forward of what the fork has. The consequence above, "GitHub
itself should protect main and develop", is now the owner's to weigh: protecting develop against
everyone but the deploy key would keep a mistaken push elsewhere refused there too.

**Safeguards, all code.** Models propose; deterministic policy authorizes: the review is a
model's, but whether a branch is pushed is `may_push`, the scan, and the push path's own refusals
(only `hq/` branches, only the stored head, never a force); a held branch is pushed only on the
owner's word. Each step is told to HQ once (a second report is refused), and every push and
merge is an audit line on the node.

**Limits, accepted.** The shapes give false positives (a 40-character commit id pinned in a
requirements file reads as a live-looking secret); the owner's "push anyway" is the way past
one. The scan reads what leaves in the branch, not what is already on the fork. A personal value
split across a line break is not seen, as in the repository's own check.

## Amended 2026-11-01: a repository outside the lab (round 4's F4)

**Why.** Round 4 of the baseline experiment adds F4: each round, one small task in a public
open-source repository HQ has never seen, done by both arms against criteria written first, its
branch kept locally and pushed nowhere (`docs/proposals/baseline-experiment-2026-10-28.md`,
"Not only the homelab"). A member's sandbox has no internet, and `repo_checkout` served only the
lab's fork.

**What changes.**

- **The owner names the repository on the item's project**, once, when the task is set up:
  `POST /v1/hq/projects/{id}/repo` with `{"url": "https://github.com/<owner>/<name>"}` (null
  takes it away), kept as the setting `project_repo:<project>`. Only a public
  `https://github.com/<owner>/<name>` is taken, by one rule in the kernel
  (`endora_kernel::lab::outside_repo`): any other scheme or host, a credential, a port, a query
  or a path past the repository is refused. A task with no project of its own takes the nearest
  one above it.
- **HQ tells the node** in its answer to `/v1/team/turn` (`"repo"`). The node holds the address
  to the same rule again, clones it once into a mirror of its own beside the fork's
  (under `outside-repos/` in the node's data folder, `Mirror::outside`), worked from the default branch the
  repository names, and fetches it on the fork's five-minute schedule. No upstream rides with it.
- **`repo_checkout`** hands that repository's default branch (or the team's kept branch of it)
  to `/work/repo`, as it does the fork's. **`repo_submit`** takes the branch by the same rules
  (named for the task, on top of the default branch, HQ's commits, the limits) and keeps it as
  `refs/submissions/<branch>` in that mirror, with its bundle under the task, for judging. HQ is
  told `can_push: false` and the repository.
- **Never pushed, by code at three places.** The outside mirror refuses `push` and
  `merge_into_base` for every branch (`repo::OUTSIDE_KEPT`); HQ never lists such a branch as due
  (`accept_branch` keeps it "accepted" with why) and never puts a push card for it, whatever the
  card setting; and the node's delivery and the owner's push work from the fork's mirror alone,
  which does not hold the submission.
- **No check of the lab's.** The dev node's check is ansible's and says nothing of another
  repository, so none is queued: the item's criteria are its gate, which opens on submission,
  and its reviewer checks the branch out and runs its tests. Acceptance has no consequence
  outside the node, since nothing is delivered.

**Limits, accepted.** The submission rules are the fork's, but for one (*amended 2026-11-05,
below*). A repository past
the 32 MB checkout bundle cannot be handed out. The repository must fetch from the node; its
tests must run in the offline sandbox, so a candidate's dependencies are the experiment's to
vendor. Scenarios: `domains/capabilities/features/the-team-works-in-an-outside-repository.feature`.

## Amended 2026-11-05: the owner's values are guarded where they live

The rule against the owner's values (`group_vars`, `host_vars`, any inventory, anything named for
a vault) was applied to every repository, and its limit was accepted above. It cost real work:
two of the harness confirmation's items were refused for an ordinary `inventory/` path in an
outside repository, and in the autonomy climb's round-1 fixtures it refused 4 of 8 harmless
software paths (`shopkeep/inventory/stock.py`, `inventory/models.py`,
`app/vaultwarden_client.py`, `pkg/vaulted_cache.py`). The rule's names describe the lab fork's
layout, where the owner's values are kept; in another repository the same names are that
project's own code. Scoping the filename rule is all this changes: it does not establish that an
outside repository holds no secret, and the briefing's rule never to copy the owner's values
anywhere still holds.

- **The rule follows the repository.** `endora_kernel::lab::refused_as_owners_values(path,
  outside_lab)` is `forbidden_path` in the fork and nothing outside the lab. `outside_lab` is
  trusted context, never a member's word: the node's mirror is an outside one only when HQ's
  turn names the repository the owner set on the item's project, and HQ's review asks the same
  project setting. The node holds a submission to it and HQ holds the review to it, so the two
  agree. In the fork every path refused before is refused still.
- **The fork is never an outside repository.** The node refuses to mirror the lab's own fork,
  under any of its addresses, as a repository outside the lab, so naming it there cannot step
  around the rule.
- **Unchanged.** The briefing's words (see "an outside repository is briefed as one" below), the
  files HQ reads for its checks (role READMEs and defaults, workflows not named for a vault) and
  the outgoing scan.

Scenarios: `domains/capabilities/features/the-team-works-in-an-outside-repository.feature` and
`domains/team/features/the-owners-values-are-guarded-where-they-live.feature`.

## Amended 2026-11-05: a resumed item knows what it submitted

In the minimum-viable-autonomy climb a turn killed after its repair was committed and before it
was submitted was tried again, read "Nothing new in the thread" and "Already checked out … Go on
from it", and took the commit for the repair: two interrupted items of eight lost their repair.

- **HQ says where the submission stands.** A turn tried again, and any turn on an item that has a
  submission, is told HQ's newest submission for the item's work, full head and status (work in
  progress said as not offered as done), or that HQ has recorded none, and that a commit is not a
  submission. "None" is HQ's word only: the node may hold a submission HQ never heard of.
- **The kept checkout says its head.** The worker's "already checked out" names the branch's full
  head and says that unless the briefing names it as HQ's newest submission, it is not submitted.
- **One revision is one submission.** The node takes a revision it already keeps again (it always
  did). HQ, told of the branch and head its newest submission of the item already has, records
  nothing new: no second branch, document, check or post, and work in review is not sent back. So
  a turn unsure whether its submission arrived (killed after the node took it, or before the answer
  came back) may submit it again.

No new storage: HQ's branch records, the node's kept refs and the sandbox already hold all of it.
Scenarios: `domains/team/features/a-resumed-item-knows-what-it-submitted.feature`,
`app/worker/features/a-kept-checkout-says-a-commit-is-not-a-submission.feature`, and the
outside-repository feature's resubmission scenario.

## Amended 2026-11-05: an outside repository is briefed as one

The amendment above scoped the rule of the owner's values to the fork and left the briefing's
words unchanged. So a member on an item in a repository outside the lab was still told how the
lab is built (ansible-nas roles and playbooks) and, at every checkout, "never edit, cite or copy
from group_vars/*.yml, host_vars or any inventory": a prohibition the node no longer enforces
there and that names that project's own code (the defect-remediation handoff's O8).

- **The briefing names the repository.** HQ's briefing of an item whose project names a
  repository outside the lab says which one it is, that `repo_checkout` and `repo_submit` work in
  it, that the fork's rules of which paths hold the owner's values do not apply, that the owner's
  own values, credentials and personal details never go into it, and that a submitted branch is
  kept on the node and never pushed. It says this in place of how the lab is built. An item of the
  lab keeps that paragraph.
- **The checkout says where it is.** The node's checkout answer carries `outside` when its
  mirror is an outside one, the same trusted context as above (the project setting, never a
  member's word). The worker then says what holds there instead of the fork's rule, for a fresh
  checkout, the team's kept branch and a reviewer's checkout alike. A checkout of the fork keeps
  the fork's rule word for word.
- **The worker's instructions scope it.** The git paragraph says `repo_checkout` brings the item's
  repository (the fork unless the briefing names one outside the lab), that an outside branch is
  never pushed, and states the fork's rule as the fork's.
- **Unchanged.** What the node and HQ refuse, non-delivery outside the lab, and the files HQ reads
  for its checks.

Scenarios: `domains/team/features/an-item-outside-the-lab-is-briefed-as-outside.feature` and
`app/worker/features/a-checkout-outside-the-lab-carries-no-lab-rule.feature`.
