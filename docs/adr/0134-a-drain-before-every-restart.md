# 0134 - A drain before every restart

## Status

Accepted, 2026-10-19, by the owner, who agreed the design before it was built. Prepared with AI
assistance (Claude). Amends [0131](0131-the-two-kinds-of-turn.md) (a turn that waits, and the
turn a restart interrupts) and [0132](0132-outcomes-and-a-desk-that-interrupts-only-for-decisions.md)
(the digest gains one kind of item, and only when something was cut), and builds on
[0124](0124-guards-against-how-agent-teams-fail.md) (an interrupted turn is tried again and is
not held against its member) and [0122](0122-the-teams-world-stages-stories-and-situations.md) (the handover note). Built:
everything below, and the owner's scenarios in
`domains/team/features/a-drain-before-a-restart.feature`.

## Context

Two things restart HQ and the runner under turns that are running: a deploy (`make deploy`,
`make deploy-pi`), and the NAS's own security reboot at four in the morning. Since 0124 a turn
cut by a restart is tried again, does not count against its member and raises nothing. That
settled the bookkeeping and not the loss: a turn on the owner's Mac takes ten to fifteen minutes,
and everything it had done since its last note went with it. A deploy mid-afternoon, or a reboot
at four, throws away a quarter of an hour of the team's work each time a turn is caught.

The cheap fix is to stop starting turns shortly before a restart and let the ones under way
finish. Nothing in HQ could say that.

## Decision

### A drain: nothing new starts, and the turns under way are told

HQ keeps a **drain** while a restart is coming: when it began, the cap a restart will wait, and
which turns were running. It is a row in HQ's own settings, so it survives an HQ restart (which
is how it is lifted, below), and needs no migration.

While it lasts:

- **No new turn starts.** The one place turns are started (`start_queued`) starts none. What is
  queued stays queued, nothing is dropped, and each queued turn says why in the words the roster
  already shows for a wait: "waiting for a restart: nothing new starts until it has been". Like
  a wait for budget or for the Mac, it is a wake and not a stall: no nudge, no finding, no climb
  to the lead.
- **Every answer HQ gives a running turn ends with a fixed notice**: "A restart is coming: finish
  this step, leave a handover note, and end your turn." The team protocol has one function every
  tool call passes through, so the notice is added there and nowhere else, to a refusal as much as
  to a reading. A turn that has just been ended by its own call is not told.
- **A member can end its turn with a handover and keep its task.** Handover notes were only for
  work handed on (`hand_to`, `decide_stuck`). `post` now takes the same four-part note as an
  optional `handover`; with a drain it keeps the note for the member's own next turn, ends the
  turn, and queues it again as a retry that does not count against the member (the start of the
  turn that wrapped up is given back). The task is not handed on, blocked or stuck, and the
  member's briefing for the retry opens with "Your own note, left on #N when a restart ended your
  turn". Without a drain it is refused, saying so, and the turn goes on. No new tool: the number
  of tools a model is offered is a measured failure mode for the house's model, and `post` is
already in every list.

### The routes, and who asks

`POST /v1/team/drain {cap_seconds}` begins one (the default cap is 300 seconds, never under one
or over 1,800); `GET /v1/team/drain` says `{draining, since, cap_seconds, running, waiting,
last}`; `POST /v1/team/resume` lifts it. They are signed with HQ's own token like the owner's
actions, and are HQ-only: the console does not need them, so the node does not proxy them.
Beginning a drain that lasts changes nothing: it began once, and keeps its cap.

The command is a subcommand of HQ's own binary, `endora-hq drain [--cap SECONDS] [--wait]` and
`endora-hq resume`. It runs inside HQ's container, reads the token HQ's own file holds, talks to
HQ on its own address over a plain socket (no new dependency), and prints what it did. With
`--wait` it looks every five seconds and returns once no turn is running or the cap has passed,
counted from when the drain began, so asking twice never gives a restart more time. Any argument
the binary does not know is an error and never starts a second HQ over the same database.

### The drain does not kill; the restart cuts, and says so

The drain only stops starting and asks. Whoever restarts waits for `running == 0` or the cap and
then restarts; a turn still running is cut by the restart and tried again by the path every
interrupted turn takes (0124). That keeps the authority to stop a turn where it was: with the
restarter, and the kill switch.

**HQ starting again lifts the drain.** The first tick of an HQ that finds a drain it did not begin
lifts it, before it looks at any turn, so the turns the restart cut are counted. A drain nobody
ended (a deploy that failed, a reboot that never came) **lifts itself after 30 minutes**, so a
forgotten drain never stalls the team, and `resume` lifts it by hand. A deploy also asks to resume
afterwards, in case HQ was not the thing it restarted.

Each drain keeps what it came to: how many turns were running when it began, how many ended
before it did, how many the restart cut, and how it ended (resume, restart, max age). That is in
the feed (`drain.began`, `drain.ended`) and in `GET /v1/team/drain` as `last`.

### What the owner sees

A drain is **silent**: one line in the Activity, "The restart came: 2 turns ended before it, none
cut". If, and only if, it cut a turn, there is **one item in the digest** (a `notice`, 0132) that
reads "A restart cut N turns: Zed on #12 Cap sonarr's memory; ...", naming each. Nothing reaches
the desk: no decision is waiting on the owner.

### Deploys and the NAS's reboot

`make deploy` and `make deploy-pi` run `scripts/hq-drain.sh drain` before they restart anything
and `resume` after. It runs `endora-hq drain --cap 300 --wait` in HQ's container through the same
Docker context the deploy uses (`docker --context nas exec endora-hq ...`). When the image is
built elsewhere the drain comes after the build and just before the restart; otherwise before the
build, since build and restart are one command. `DRAIN_CAP=<seconds>` changes the cap and
`NO_DRAIN=1` skips it all. It never stops a deploy: HQ not running, or not drained for any
reason, is said and the deploy goes on. It never runs the command on an HQ that predates it (its
usage line is looked for in the binary first), because that HQ would take the arguments for
nothing and start a second HQ over the same database. `deploy-pi` restarts the runner and not HQ,
so it lifts the drain by name; the runner takes its turns from HQ, so draining HQ is enough.

For the owner's NAS, the `endora` role in the `endora.homelab` collection gains an optional unit
(`endora_drain_on_shutdown`, off by default; `endora_drain_cap_seconds`, 300). It is the idiom
for "run this as the host shuts down, while Docker is still up": a oneshot that does nothing when
it starts and stays active (`RemainAfterExit=yes`), ordered `After=docker.service`. Units stop in
the reverse of the order they started, so it is stopped, and its `ExecStop` run, before Docker
begins to stop, with HQ's container still up. Its `TimeoutStopSec` is the cap and a minute, since
the default 90 seconds would kill a five-minute wait. It is not `DefaultDependencies=no` (that is
for units that run before the system is up or after it is nearly down, and this one needs Docker)
and it does not `Require` Docker (restarting Docker would then run the drain). The `-` before
`ExecStop` means a drain that fails never holds a shutdown. It runs the deploy's own script, so a
reboot has the guards a deploy has; the role brings its copy (`/usr/local/sbin/endora-hq-drain`)
and the variable is its own switch, not `endora_enabled`'s, because the owner's NAS runs Endora
with `make deploy` and has no checkout of the role's. The one file is the role's, and
`scripts/hq-drain.sh` a link to it, so the two cannot drift. The owner's `ansible-nas`
repository is not touched: it turns the variable on.

## Consequences

- A deploy or a reboot with the team idle costs nothing; with turns under way it waits, up to the
  cap, for them to end. A turn that is still running after five minutes is cut as before.
- A turn that wraps up costs the member nothing and loses nothing it wrote down. A turn that does
  not (a small model that ignores the notice) is no worse off than it was.
- New: a `drain` setting and a `drain_last` setting in HQ's store (rows, no table, no migration),
  two feed kinds, one wait's words, one refusal, and one optional argument on `post`.
- A drain from a deploy that died between its two commands holds the team for up to 30 minutes
  unless someone resumes; that is the price of never asking a person to remember to.
- The HQ binary has a second job, a command, beside serving. It is a few hundred lines that talk
  to its own address.

## Alternatives considered

- **Let HQ drain itself when it is stopped (on SIGTERM).** Rejected: a container is given ten
  seconds by default and a reboot's timeout is Docker's, not HQ's; the wait would have to be
  configured in the compose file and the host's service manager, and the runner (which a deploy
  also restarts) is not HQ. The one who restarts should be the one who waits.
- **A tool for wrapping up.** Rejected: one more tool in every list, for a turn a few times a
  month. An optional argument on a tool every member has costs a sentence.
- **Pausing the turns instead of ending them.** Rejected: a paused turn is a process holding a
  model's context, which a restart destroys anyway.
- **Raising a card on the desk when a restart cuts turns.** Rejected: nothing needs the owner's
  word; the turns are tried again. A digest item says it happened, and only when it did.
- **A longer default cap.** Rejected: a turn is stopped after 30 minutes anyway, and a deploy that
  waits half an hour is one nobody runs. Five minutes catches the turns that are nearly done, and
  `DRAIN_CAP` is there for a deploy that can wait.
