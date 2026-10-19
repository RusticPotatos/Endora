# 0117 — Claude Code as a developer runtime

## Status

Accepted, 2026-10-19 (proposed 2026-10-18), on the owner's decisions of 2026-10-17. Prepared
with AI assistance (Claude). Amends
[0110](0110-the-teams-rings.md) (the workbench image carries Claude Code, and a turn in it may
be Claude Code's), [0114](0114-evidence-acceptance-criteria-and-the-tester.md) (receipts
reported by the sandbox, which no live criterion rests on) and
[0115](0115-the-engineering-wing-and-the-git-flow.md) (a wing developer may run on Claude Code;
the fork is checked out before it starts). Built: the member's runtime in HQ's store, its route
and the pilot seeded once; the runner handing it to the workbench; the worker's Claude Code mode,
its MCP bridge and its receipt hook; Claude Code in the workbench image with its settings; the
relay's pass-through for a Claude Code turn; sandbox-reported receipts and the evidence rule;
each member's cost of a finished task on the scorecard. Not built: a control on the HQ screen
(the owner's route is there; the button is wired with the screen's roster code).

Amended 2026-10-19: **its shell is the sandbox's whole Bash**, not six command prefixes. Live
(task 674) `--allowedTools "Bash(git *)" …` under `dontAsk` denied every command with a subshell
group, a `for` loop or `$?` in it, and the one denial in a turn became "the shell refused every
command"; the turn blocked its task. The list protected nothing the container does not (see the
table: the container is the boundary; `Bash(git *)` already runs anything git can be made to
run, and `cat /proc/self/environ` matched `Bash(cat *)`), and it cost a turn's work. Decision 4's
command line and the table's row say so below. A call the shell refuses, or a command that
fails, is also kept as a refused receipt now: Claude Code reports both as a `PostToolUse`
whose response is a string starting "Error:", which the node used to keep as done.

## Context

HQ's members take their turns on Endora's own worker (0102, 0109): a small loop that offers the
model HQ's tools and the node's contracts, and in the workbench a shell (0110). It is built for
a small model and does its job, but a developer on the wing (0115) works on a real repository:
reading files, editing them, running the checks, committing. Claude Code is built for exactly
that, and the owner decided to try it **as the developer's runtime, headless, with an API key
through Endora's relay**, on one developer first, and to measure it against the worker.

**The terms.** Claude Code is used here as Anthropic documents it for third-party products: an
API key (here, the turn's token through the node's relay, which adds the owner's API key
upstream), never a claude.ai login. Anthropic's Agent SDK overview and its legal and compliance
page say third-party developers may not offer claude.ai login, nor route requests through Free,
Pro or Max plan credentials on behalf of their users, and point to API key authentication
instead. Nothing in Endora reads, stores or passes a subscription credential: no OAuth token or
`/login` exists in the sandbox, and the worker strips `ANTHROPIC_API_KEY`,
`CLAUDE_CODE_OAUTH_TOKEN` and `ANTHROPIC_OAUTH_TOKEN` from Claude Code's environment.

## Decision

**Owner decisions, 2026-10-17.**

1. **A runtime per member**: `worker` (every member's default) or `claude-code`, the owner's to
   set (`POST /v1/members/{id}/runtime`, through the node's `/v1/hq/members/{id}/runtime`,
   audited). Claude Code is offered **only to a developer who thinks with Claude**, refused for
   anyone else in the domain (`Refusal::NotARuntime`); a member whose brain later goes home runs
   on the worker again, decided at each turn. The CEO, the lead, the tester and every member on
   the house's model keep Endora's worker. Kept in a guarded `members.runtime` column; HQ hands it
   to the runner (`/v1/runner/next`) and to the node's relay (`/v1/team/turn`).
2. **The pilot**: HQ sets **Rae**, a wing developer on Haiku, to Claude Code once on start (an
   idempotent setting flag; the owner's later choice stands); **Zed** stays on the worker. The
   success measure is the scorecard's **cost of a finished task by member**: Rae's against Zed's
   over the same weeks, with the share of turns that showed nothing and the work sent back
   beside it. Claude Code is widened only if it finishes tasks for less, or for about the same
   with less sent back.
3. **Where it runs**: in the workbench (ring 1, 0110) and nowhere else. The runner passes
   `ENDORA_RUNTIME=claude-code` to a workbench turn only; it refuses a Claude Code turn outside
   the workbench, or for a brain that is not Claude, and never runs it on the worker instead (the
   pilot compares the two).
4. **The worker stays the entrypoint.** It opens HQ's protocol, reads `my_turn`, claims the task
   and checks the fork out into `/work/repo` (0115) as it always did, then runs, in that folder:

   `claude -p --output-format json --model <brain> --effort <HQ's effort> --max-turns <30>
   --permission-mode dontAsk --permission-prompts none --setting-sources user
   --no-session-persistence --strict-mcp-config --mcp-config <file>
   --append-system-prompt-file <Endora's rules> --tools Read Edit Write Glob Grep Bash
   --allowedTools <the bridge's tools as mcp__endora__*> Read Edit Write Glob Grep Bash
   --disallowedTools WebFetch WebSearch`

   with the briefing on stdin (amended 2026-10-19: `Bash` whole, where it was six
   `Bash(prefix *)` rules). `--setting-sources user` keeps a fork's own `.claude/` settings
   and hooks from ever being read; `dontAsk` denies whatever is not allowed, so nothing waits on
   a prompt. Its environment: `ANTHROPIC_BASE_URL` is the node's Anthropic relay,
   `ANTHROPIC_AUTH_TOKEN` the turn's token (sent as `Authorization: Bearer`, which the relay
   already takes), every model alias (`ANTHROPIC_MODEL`, `ANTHROPIC_SMALL_FAST_MODEL`, the
   default Haiku, Sonnet and Opus) the member's brain, so nothing it sends names another. Its
   JSON result is the last word, posted with the line of what the turn did, its rounds and its
   own cost estimate; the exit codes are the worker's (a 401 or 402 from the relay exits 3).
5. **One MCP server, the worker itself.** The node's contracts are not MCP (they are listed and
   called over plain HTTP), and the wing's git needs the worker to carry the bundles (0115). So
   Claude Code's one server is `endora-worker --mcp`, over stdio: HQ's tools and the node's
   contracts, each call sent where its tool came from by the worker's own code, with its repeat
   guard and its line of what the turn did. The token is taken out of anything the model sends
   on; a question to the owner, or an action put on the owner's desk, ends the turn (the bridge
   refuses every call after it, and the worker posts nothing). Its configuration names the token
   only as `${ENDORA_TURN_TOKEN}`, expanded by Claude Code from its environment (its own
   credential variables read empty inside MCP configuration).
6. **Receipts from its own tools.** A `PostToolUse` and `PostToolUseFailure` hook (matcher `*`)
   baked into the image's managed settings runs `endora-worker --receipt`, which posts the call
   (tool, input, response, cut to 2,000 characters a field; the bridge's tools skipped) to the
   node's `POST /team-relay/receipt` with the turn's token, never prints, and always exits 0. The
   node hears only a running turn HQ says Claude Code runs, scrubs secrets and the token, and
   tells HQ with `sandbox: true`. HQ keeps it as a receipt marked **sandbox-reported** (shown so
   in its line). Edit, Write and NotebookEdit count as acts; a command counts as a look, since
   the sandbox is disposable and only a submitted branch leaves it.
7. **What a sandbox-reported receipt proves.** The node witnessed none of these calls: they ran
   in the sandbox, and the sandbox reported them. The one evidence rule (0114) may cite one for
   what the sandbox did (a file edited, a lint run), but **a live criterion still needs a fresh
   reading the node witnessed**; a sandbox's word is no reading of the lab. Enforced in the
   domain rule (`judge`), with a test.
8. **The relay's pass-through.** Claude Code sends a shape of its own: a `system` list with cache
   marks, its tools, `metadata`, adaptive `thinking`, an effort, `role: "system"` messages part
   way through the conversation, and betas of its own (`claude-code-20250219`,
   `mid-conversation-system-2026-04-07`, `effort-2025-11-24`, `interleaved-thinking`). The
   worker's strict allowlist would refuse or strip it. For a turn HQ says Claude Code runs, the
   relay forwards its fields and betas as sent, and keeps enforcing: the model must equal the
   member's brain; no `speed`, `fallbacks`, `mcp_servers`, `container` or `service_tier`; no tool
   run on Anthropic's side (web search and fetch, code execution, MCP); no content fetched from a
   URL or a file; a cache mark's `1h` rewritten to `5m` and any other mark refused; the effort one
   of five; `max_tokens` clamped to what the turn can pay for, with the money cap and the per-call
   pre-pricing as for every call. Betas that cost or reach out (fast mode, server-side
   fallback, MCP client, code execution, web fetch and search, files, the 1M context) are
   dropped. Endora's worker's turns keep the strict rule.
9. **The image.** The workbench image carries **Claude Code 2.1.296** (2.1.293 is the first that
   knows Haiku 5.5, 2.1.295 the first whose hooks can block; this is a week older than the newest
   at the time): the native binary of that release, fetched from Anthropic's release bucket at
   build time and checked against the release manifest's digest pinned per architecture. The
   sandbox never fetches it or updates it. Its **managed settings** (`/etc/claude-code`, read-only,
   above any other) set `DISABLE_AUTOUPDATER`, `DISABLE_UPDATES`, `DISABLE_TELEMETRY`,
   `DISABLE_ERROR_REPORTING`, `DISABLE_FEEDBACK_COMMAND`, `CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY`,
   `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`, `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS` and
   `CLAUDE_CODE_PROMPT_CACHE_TTL=5m`; no attribution of its own (the wing's commits carry Endora's
   trailer); WebFetch and WebSearch denied; bypass mode disabled; and the hook. The same
   variables are in the image's environment. `CLAUDE_CONFIG_DIR` is under the turn's `/work`
   tmpfs. No credential is in the image.

## What is enforced where

| Rule | Where |
| --- | --- |
| Claude Code only for a developer on Claude | the domain (`Runtime::for_member`, `effective`), the runner (refuses it on a non-Claude brain or outside the workbench), the worker (refuses to start it outside the workbench) |
| No subscription login, no owner's key in the sandbox | the worker strips Anthropic's key and OAuth variables; the relay adds the owner's key upstream |
| The model is the member's brain | the relay (`for_claude_code`), and every alias pointed at it by the worker |
| Money: the turn's cap, per-call pre-pricing, `max_tokens` clamped | the relay, as for every turn |
| Five-minute cache only | the relay (rewrites `1h`, refuses other marks); Claude Code's own setting |
| No server-side tools, MCP servers, fast mode, fallbacks, fetched content | the relay |
| No web tools in the sandbox | `--tools`, `--disallowedTools`, the managed settings' deny, and the offline sandbox |
| Its shell: the sandbox's whole Bash | `--allowedTools Bash` with `dontAsk`; the container is the boundary (amended 2026-10-19; it was git and the checks, by prefix) |
| A fork's own Claude settings or hooks never load | `--setting-sources user` |
| A sandbox's report is no reading of the lab | the evidence rule (`judge`) |
| Endora's tools still decide every act | HQ and the node, as for the worker: the bridge acts on nothing |

## Consequences

- **The token is in Claude Code's environment**, and so in its Bash's: the model could read it.
  It opens only this turn, while it runs, and only the gateway is reachable from the sandbox; the
  bridge takes it out of what the model posts, the node out of what the hook reports, and the
  worker out of the last word. Claude Code's own subprocess environment scrubbing
  (`CLAUDE_CODE_SUBPROCESS_ENV_SCRUB`) needs bubblewrap inside the container, and is not used.
- **The receipts of its own tools are reported, not witnessed.** They make a Claude Code turn's
  work visible on the record (ADR 0114 left shell evidence with none), but they are the
  sandbox's word, and the rule treats them so.
- **Claude Code's fixed prompt is large and undocumented.** In the image, with three team
  tools, its first request was about 24 KB (its system prompt with Endora's rules about 11 KB,
  six built-in tools' definitions about 11 KB), roughly 6,000 tokens by the usual
  four-characters-a-token reckoning, against well under that for the worker; every round sends
  it again, read from the cache after the first. What it actually costs a finished task is what
  the pilot measures: the relay meters every call, and the scorecard compares Rae with Zed.
- Claude Code's own `total_cost_usd` is its estimate at list prices; the relay's charge is what
  HQ's ledger counts.
- The workbench image grows by the binary: about 630 MB in all, from 373 MB.

## Verified, and not

Built here, 2026-10-18, on the Mac for linux/arm64 (`docker --context desktop-linux buildx
build --target workbench`):

- `claude --version` in the image says `2.1.296 (Claude Code)`.
- With `--network none`, a read-only root and the runner's fence, a whole turn ran against a
  fake HQ, node and relay on the container's loopback in about a second: the worker's
  bookkeeping, then Claude Code's three model calls through the relay URL with the token as
  Bearer, a Bash call (`git --version`) reported by the hook as a receipt, a post through the
  bridge, and the last word. **It did not hang on startup traffic**: with egress gone and the
  settings above, the only requests were to the relay, HQ and the node.
- The first build ran Claude Code with **none of its settings, silently**: `COPY --chmod=444`
  gave the new `/etc/claude-code` folder the file's mode, the workbench user could not open it,
  and Claude Code logged the error only in `--debug`. The folder is now made 755 first; the
  hook, the attribution and the deny rules were then seen to apply.
- The request shape the relay's tests use is the one Claude Code 2.1.296 sent the fake relay,
  cut down.

Not verified: the relay's pass-through against **Anthropic's real API** with Claude Code's
traffic (the betas it forwards, the `role: "system"` messages, counting such a call's tokens);
a turn on the Pi itself; Claude Code against the real HQ and node; its cost per task. The first
real turn on the Pi is that check, and its receipts and the relay's charges are the record of
it.

Not verified either (2026-10-19, the whole-Bash amendment): that a bare `Bash` under
2.1.296's `dontAsk` runs what a model writes. The documentation leaves two points open: a
command the parser cannot split, and a redirect to a path outside the working directory. One
real turn on the Pi settles both, and each sandbox receipt must be `ok` with no "don't ask
mode" in it: `git checkout -b hq/task-N-x develop && for f in a.yml b.yml; do printf '\n' >> "$f"; done && (git add -A || true) && git commit -qm x`,
then `diff <(git show develop:nas.yml) nas.yml > /tmp/d.txt; echo "diff exit $?"`. If only the
redirect is still denied, `--add-dir /tmp` lets Claude Code write there: a follow-up, not a
blocker for delivering a branch.

## Deferred

- **Per-task `--resume`**: Claude Code's session dies with the turn's tmpfs
  (`--no-session-persistence`). Keeping `~/.claude/projects` per task on a volume would let a
  developer's next turn on the same task go on from where it stopped; deferred until the pilot
  says Claude Code is worth keeping.
- A control on the HQ screen for the runtime (the owner's route and the snapshot's
  `runtime` and `claude_code` per member are there).
- Claude Code for the lead or the tester; for members on the house's model (it speaks only
  Anthropic's API).
- Hooks that block (`onFailure: block`): the receipt hook never blocks a turn.
- `--max-budget-usd`: the relay's cap is the real limit, and HQ's reservation is not known to
  the sandbox.

## Alternatives considered

- **A subscription login.** Refused: Anthropic's terms forbid offering it or routing a product's
  requests through Pro or Max credentials, and it would put a personal credential in a sandbox.
- **Two HTTP MCP servers, HQ's protocol and the node's contracts.** The node's contracts are not
  MCP, and the wing's git needs the worker to unpack and bundle; the worker as a stdio bridge
  reuses the code every other turn runs, and keeps one place where calls are sent.
- **The Agent SDK in-process.** It spawns the same CLI; the Rust worker spawning the CLI is
  simpler and keeps one language in the sandbox.
- **Claude Code's own sandbox (bubblewrap).** It needs privileges the container does not give,
  and often fails inside containers; the container is the boundary, as for the worker's shell.
- **Passing every field and beta Claude Code sends.** Some change what a call costs beyond its
  usage, or reach past the fence; the pass-through keeps the rules that guard money and the
  fence, and only those.
