# 0115 — The engineering wing and the git flow

## Status

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
   A branch the fork has is handed out from the fork as before. The worker, in the
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
   branch behind the fork's base is sent back to rebase); it changes at most **200 files** and
   **1 MB** of diff; **every commit is HQ's** (the author and the trailer); and it **touches
   nothing of the owner's values**: `group_vars/*.yml`, `host_vars/`, any inventory or anything
   named for a vault is refused whole, and the briefing says never to edit, cite or copy from
   them. What passes is kept under the task (`submissions/task-N/<slug>-<sha>.bundle`, and as
   `refs/submissions/hq/…` in the mirror, which the mirror's fetch never prunes), and told to HQ
   with the turn's own token (`POST /v1/team/branch`): HQ records the **branch**, writes a
   **document** on the task, "branch hq/… (N files, +a −b)", with the stat and the diff (64 KB
   shown), says so in the thread, and queues the **check**. The call's receipt (0114) is the
   turn's to cite. No desk notice per submission: the thread, the document and the push card
   are where the owner reads it.
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
   then says both ("ansible-lint 25 findings (develop 25), …: no worse than develop") and the
   report holds the base's outcomes; a clean branch costs no second run, and a base that cannot
   be found leaves the plain pass or fail, said in the line. *Amended 2026-10-19:* a check that
   **could not run** (its tool not found or not executable, exit 127 or 126; killed; out of
   time) is no finding. A base on which any check could not run was not read and excuses
   nothing: the failure stands and the line says "develop could not be checked: …", so a box
   without its tools (every check at 127 on both sides, as c1 and c2 were) passes no branch,
   where "develop failed too" had read as "no worse". A branch's own check that could not run
   is never excused by a failing base either. HQ keeps the
   result as a **receipt** on the task and a document with the report, and as the task's
   **gate** (0114: code work closes only once it passed); a pass wakes the submitter and the
   tester, and **a branch that fails its checks is sent back** by HQ itself, the tester
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
