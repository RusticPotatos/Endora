# 0123 — A bench for an external agent harness

## Status

Proposed, 2026-10-18, for the owner to accept at the merge. Prepared with AI assistance
(Claude). Amends [0102](0102-endora-hq.md) (a second credential on the model relay, which is no
turn of HQ's), [0110](0110-the-teams-rings.md) (a second image on the Pi's sandbox network, run
once and fenced as a turn is) and [0117](0117-claude-code-as-a-developer-runtime.md) (the
relay's Claude Code pass-through, held to two models instead of a member's brain). Built: the
bench credential on the node, its daily cap and its record, the bench image, and the comparison
below. Not built: anything that makes the harness part of HQ. Run: not yet; the credential is
off until the owner creates its file.

## Context

The owner, 2026-10-18, about an open-source multi-agent harness for coding agents (Apache-2.0;
a daemon that runs Claude Code sessions in terminal panes, with a queue of owned work, a lead
that delegates, and a watchdog that wakes a stopped seat still holding work): it is "pretty
great, should really consider it… you could start integrating and make evidence by the
architecture." The decision it feeds has three answers: keep building the wing into HQ (0115,
0117, 0120); take pieces of the harness as code; or run the harness as the wing's runtime.

Its source was read first (privately; the review stays out of this repository). It trusts its
agents: they run as the user, in the project folder, with the harness's permission modes (an
edit-accepting floor, full bypass on request), no container, no network fence, no budget, and
its default agent specs enable two third-party MCP servers on the internet. HQ's wing is the
opposite on each count. A judgement on paper would compare designs; the owner asked for
evidence. So the harness is measured on **one real task**, the wing's own first branch, inside
Endora's fence, with its money capped and metered, and compared with the wing doing the same.

## Decision

1. **The bench credential** (node, `app/node/src/api/bench.rs`). A token read from a file on the
   NAS (`ENDORA_BENCH_TOKEN_FILE`; compose mounts the folder named by `ENDORA_BENCH_TOKEN` in
   `local.mk` at `/bench-token`), **read on every call**: no file, or fewer than 32 characters
   in it, and there is no bench; deleting the file turns it off at once. It is compared whole,
   in constant time. It opens **the model relay and nothing else**: `POST
   /team-relay/v1/messages` and its `count_tokens`, and `GET /team-relay/bench`, its own day so
   far. Every other route on the team listener (the contracts, the bridge, receipts, check
   bundles, the local model, the gateway to HQ) refuses it with 403 **before HQ is asked**, so
   the bench's token never leaves the node.
2. **Its calls are a Claude Code turn's** (0117, rule 8): the same pass-through and the same
   refusals (no `speed`, `fallbacks`, `mcp_servers`, `container`, `service_tier`, no server-side
   tool, no content fetched from a URL or a file, a cache mark's hour made five minutes, the
   effort one of five, the betas that cost or reach out dropped), with the owner's Anthropic key
   added upstream. The model must be **`claude-haiku-5-5` or `claude-sonnet-5-5`**; anything
   else, Opus included, is refused with 403.
3. **Its money.** A hard cap a day (UTC), `ENDORA_BENCH_CAP_CENTS`, **200 ($2.00) unless set**.
   Every call is counted first (free), then its prompt and as much output as it asks for are
   **set aside** from what is left of the day: the day's cap, less what the node's record says
   was spent (`bench_spend`, a table made if missing, so the cap holds across a restart), less
   what calls in flight have set aside, and never more than a developer's turn (40¢, 0111).
   `max_tokens` is clamped to that; a day that cannot pay for one token out is refused with 402
   before anything is sent. When the reply ends, what it used is charged at the relay's prices
   and the rest comes back; a stream cut short is charged all it may have sent. The bench's
   calls share one token, so a cached prompt is never counted on for another seat's call: the
   whole prompt is set aside at the cache-write price, and the usage the reply reports is what
   is kept. Calls side by side are not serialised (a bench runs several seats at once); setting
   aside is what keeps them under the cap.
4. **Its record.** Each call is noted in the node's usage log as **`bench`**, never `team`, and
   its cost in `bench_spend`. HQ is never asked about a bench call and never charged for one:
   the bench is not in HQ's pool, its ledger or its scorecard.
5. **The bench image** (`Dockerfile`, `--target bench`, linux/arm64). Node 22, tmux, git, tini,
   the harness installed from its npm package **packed from one pinned commit** (a named build
   context `harness` holding `harness.tgz`; the commit is the image's label
   `org.endora.bench.harness-commit`), and, shared with the workbench through two new stages,
   the same lab checks (`lab-checks`: ansible-core 2.15.13, ansible-lint 6.14.6, yamllint 1.30.0,
   the fork's collections and roles) and the same Claude Code (`claude-code`: 2.1.296, digest
   pinned). Its managed settings (`deploy/bench/managed-settings.json`) are the workbench's
   without the receipt hook: no updates, telemetry, error reports or other traffic, no
   experimental betas, the five-minute cache, no attribution, WebFetch and WebSearch denied,
   **bypass mode refused**, and the workbench developer's shell allowlist (git, the linters,
   `ls`, `cat`). A non-root user (uid 10001), no credential in the image. `make bench-image`
   builds it on the Mac and copies it to the Pi.
6. **How it runs.** One container on the Pi's `endora_sandbox` network (the NAS's team gateway
   on 8788 and nothing else; DNS `192.0.2.1`), read-only root, `/work` and `/tmp` in memory, no
   capabilities, 4 GB and three CPUs, a wall-clock limit of 45 minutes. The token is read from
   the NAS file at run time and passed in the container's environment, never printed and never
   written; before evidence leaves the container, any file holding it is removed and named. The
   fork arrives as a git bundle of `develop` in `/work`, never in the image. The run's driver,
   the team's spec and the brief are the owner's private notes, not this repository: they name
   the harness.
7. **The task** is the wing's first branch: the fork's root `AGENTS.md`, from the draft in
   Endora's library (`docs/drafts/ansible-nas-AGENTS.md`, given as the brief), on
   `hq/bench-agents-md`, with the criteria: the file exists and is committed; it follows the
   draft's sections; it keeps the link to the global standards; the branch passes the fork's
   lint (ansible-lint 6.14.6, yamllint 1.30.0, the fork's markdownlint configuration); nothing
   personal; no change outside `AGENTS.md` and `CLAUDE.md`. The team: one lead on Sonnet, two
   developers on Haiku, the same seats the wing has (0115, rule 7). No human answers it during
   the run; where it would have asked is recorded.

## The comparison

The harness's run (side B) and the wing doing the same task (side A, HQ's own run, not started
by the bench) are put side by side on these fields. Side A's come from HQ's own record; side
B's from the node's `bench` record and the run's evidence.

| Field | Side A: the wing (HQ) | Side B: the harness |
| --- | --- | --- |
| Task | the task's number, its criteria as HQ holds them | the brief above |
| Finished | task `done`, and when | the work item closed, or the run's stop reason |
| Wall time | the task's created → done, from `GET /v1/hq/tasks/{id}/replay` | the run's start → stop |
| Money | the scorecard's `finished.per_done` and the task's members' `turns.members[].done_spent`, in micros | `GET /team-relay/bench` after less before: `spent_micros` |
| Model calls and tokens | the relay's `team` rows in the node's usage log for the task's runs | the same log's `bench` rows: `calls`, `prompt_tokens`, `completion_tokens`, `cached_tokens` |
| Turns, and turns that showed nothing | the replay's runs, and their labels (`advanced`, `blocked`, `checked`, `talk only`, `empty`) | the harness's own work items and transitions, and the seats' transcripts |
| Sent back | the scorecard's `review.sent_back` for the task (the tester's and the check's returns) | review rounds in the harness's queue, if any |
| The checks | HQ's check job on the submitted branch (syntax, ansible-lint, yamllint) | the same three, run on the bundle afterwards, and markdownlint for both |
| Criteria met | the tester's and the lead's acceptance, by receipt | each criterion, checked by hand on the diff |
| Scope | the submitted branch's files (`repo_submit` refuses the owner's values) | the bundle's files |
| Needed the owner | desk items for the task | prompts that waited, and any work routed to a person |
| Fence | nothing reached past the gateway (by design) | the same, and what it tried to reach |
| Replay | `GET /v1/hq/tasks/{id}/replay` | the harness's database, daemon log and transcripts |

Equal footing: the same brief and criteria, the same models per seat, the same lint versions,
the same fence, the same money meter (the node's relay prices). The lead's choices are each
side's own.

## Consequences

- A second credential exists on the relay. It is narrow by construction (two models, a day's
  cap, no other route, never shown to HQ), off unless the owner creates its file, and turned off
  by deleting it. Its token is in the bench container's environment, readable by the model, as
  a Claude Code turn's is (0117): it opens only the relay, only while the file exists, and only
  for the day's cap.
- The bench spends real money, at most the day's cap, outside HQ's budget and scorecard; the
  node's usage log and `bench_spend` are its account.
- The workbench's build now takes its lab checks and Claude Code from two shared stages; what
  it contains is unchanged.
- The bench image is about 1.7 GB on the Pi (Node, the harness and its packages, Claude Code,
  the lab checks).

## Verified, and not

Here, 2026-10-18: the node's tests for every rule above (the model, the cap and its
persistence, holds side by side, the fence on every other route, the file read each call,
`bench` in the usage log and nothing to HQ). The image built on the Mac for arm64 and copied
to the Pi. On the Pi's sandbox network, from the image: the relay answers (a stranger's token
401), the NAS's main port, the internet by address and every name lookup fail. With a stand-in
relay that answers every call with a canned reply and logs every name looked up (on the Mac
and on the Pi, on a network of its own, removed afterwards): the harness started, launched its
three seats on the models asked, each reached the relay with the bearer and Claude Code's
betas, and nothing else was reached; what the harness and its agents' settings tried to look
up is in the owner's private report.

Not verified: a run against the real relay, its cost and its result. The node with the bench
needs a deploy, and the token file the owner's yes.

## Alternatives considered

- **A turn of HQ's for the harness** (a run token from HQ). Rejected: it would put an outside
  harness's spending in HQ's pool and scorecard, and give it HQ's doors; the bench is apart.
- **The owner's own API key in the container.** Rejected: no key enters a sandbox (0110); the
  relay adds it upstream.
- **The harness's full-bypass posture** (its "non-interruptive" teams). Refused by the image's
  settings; its default floor and the workbench's allowlist are what it runs with, and where
  that makes a seat wait, the record says so.
- **Running it on the Mac.** Rejected: the comparison needs the wing's fence and the wing's
  meter.
