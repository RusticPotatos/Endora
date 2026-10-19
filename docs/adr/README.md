# Architecture Decision Records

An **Architecture Decision Record (ADR)** captures a single significant
architectural decision: its context, the decision itself, the consequences, and
the alternatives that were considered. ADRs are short, immutable once accepted,
and numbered in sequence.

## What they add up to

Eighty-four records stand, every one accepted, and forty-nine earlier ones are archived into them
(the *Consolidates* column below). A handful of ideas sit underneath all of them. Read this before
reading any individual record; each line is a rule the code actually follows, with the decision
that established it.

### The spine

- **Models propose; deterministic policy authorizes** ([0005](0051-where-the-boundary-is.md)).
  Every autonomy question in this repository reduces to this one. A model may suggest,
  phrase, or interpret; it is never the enforcement boundary.
- **Deny by default, banded by reversibility** ([0024](0051-where-the-boundary-is.md)). What a
  capability can do unattended depends on whether its effect can be taken back — not on how
  confident anything is.
- **Deterministic autonomy is still autonomy** ([0044](0051-where-the-boundary-is.md)).
  Where a finding is arithmetic over stored records and the action is reversible and
  announced, policy acts without asking. All three conditions are load-bearing; drop one
  and it becomes a model acting on a guess.

### What we learned the hard way about models

- **Prompting is not where a guarantee goes** ([0037](0053-honesty-about-what-it-did.md),
  [0034](0053-honesty-about-what-it-did.md)). This model obeys an explicit, direct instruction about
  verification roughly one run in three. Fourteen consecutive times it failed to copy a name
  that was in its context. Anything that must be true belongs in code.
- **The interface discloses; the reply is not the record** ([0037](0053-honesty-about-what-it-did.md)).
  What was done is shown regardless of what the model says about it, and a turn that changed
  nothing says so.
- **Claim and observation are stored apart and never reconciled** ([0034](0053-honesty-about-what-it-did.md),
  [0035](0053-honesty-about-what-it-did.md)). A tool reporting success while
  nothing changed is the failure the record exists to catch; merging the two would hide it.

### What we learned the hard way about other people's services

- **Mechanisms, not per-integration patches** ([0038](0054-other-peoples-services.md)). Six
  hardcodes and a leak into the protocol adapter came from fixing one integration at a time.
  Genuine quirks are allowed — behind that integration's own named boundary, never in shared
  code.
- **Say what you know; everything else follows** ([0059](0059-one-fact-source-many-consumers.md)). One question — `current_states` — feeds the watch loop, the turn and the thinking.
  A source implements it and is thereby joined up, with nothing to nominate and no wiring per
  integration. The turn hears what **changed**, bounded, which is why it can be automatic.
- **MCP unless Endora needs a relationship** ([0058](0058-how-an-integration-gets-in.md)). A tool protocol expresses "call this"; it cannot express watching a service,
  supplying context, a setup flow, reversibility, or undo. Answers → MCP; relationship →
  native; both → both, as Home Assistant does.
- **Ranked trust: confirmed > observed > declared** ([0038](0054-other-peoples-services.md)).
  What the person said beats what Endora saw, which beats what the server claims about
  itself. A server announcing "I only read" is not evidence of anything.
- **A tool surface is a product decision, not the service** ([0042](0054-other-peoples-services.md)).
  Where Endora is given the service's own interface it uses it: things have ids there, and an
  id cannot be mis-matched.
- **Recovery only, bounded, disclosed** ([0039](0054-other-peoples-services.md),
  [0041](0054-other-peoples-services.md)). Repair fires after a failure so
  it can never hijack a working call, is capped, and says what it substituted.
- **Never widen a call while recovering** ([0041](0054-other-peoples-services.md)).
  Written after argument hygiene turned one aimed-at-nothing call into every light in the
  house.
- **Resumable workers share one lease boundary** ([0078](0078-shared-execution-leases.md)).
  Research keeps its own evidence record, and reservations, bounded retries and crash
  recovery follow one durable ownership rule. Durable goals, which shared it, were removed
  ([0100](0100-the-clean-slate.md)).

### What we learned about asking a bigger model, and a smaller one

- **Two models, two jobs, and the split is the defence** ([0086](0086-ask-deep-plans-and-the-house-reads.md)).
  The bigger model chooses which skills to call and is told only *that* a skill ran and roughly
  how much came back, in a sentence code wrote; the local model reads the results with no skills
  to call. A stranger's words never reach a model that could be commanded by them, and never
  leave the device ([0064](0053-honesty-about-what-it-did.md) still holds).
- **A confident answer is not evidence** ([0085](0085-a-question-about-the-world-is-not-answered-from-the-house.md),
  [0087](0087-a-local-answer-that-found-nothing-is-checked-once.md)). A small model with no skill
  run invented a game's date, time and opponent. A question about the world answered with no
  lookup is a guess, and gets one more try with the skills.
- **The model sorts the message; word lists are only the fallback** ([0089](0089-a-model-sorts-the-message-not-a-word-list.md)).
  Measured, not argued: 40/40 against the word list's 25/40 on the same phrasings.
- **Say what is sent, and send it once** ([0088](0088-say-what-is-sent-to-the-model-and-let-a-topic-end.md),
  [0090](0090-skills-are-sent-once.md)). A ring shows how full the window is, an inspector lists
  every section, and "New topic" ends what the model reads without deleting anything.
- **Unattended work is announced, visible and stoppable** ([0079](0079-goal-checkpoints-require-ownership.md),
  [0080](0080-completion-uses-tool-observations.md), [0082](0082-announce-every-way-a-goal-ends.md),
  [0083](0083-show-unattended-work-and-let-it-be-stopped.md)). Completion comes from tool
  observations, every way a goal ends is told to the person, and a goal can be stopped.
  Durable goals were removed in [0100](0100-the-clean-slate.md); the rule carries over to the
  agenda loop that replaces them.
- **What it knows about you is pages you can open** ([0101](0101-the-brain.md)). Markdown in
  a folder with its own history: memory read every turn, other pages one read away, every
  change a commit, and nothing the person wrote ever overwritten unseen. It learns only from
  the person's own words, and the folder is never trusted (no links followed, no hooks run).
- **Signals become items; rules decide, and only restarts happen unasked**
  ([0103](0103-the-agenda-loop.md)). One loop turns what Endora already reads into items at
  four levels (handled, FYI, needs you, urgent). A persisting signal is one item, a failed
  read closes nothing, mail can only tell, and Stop teaches a rule the owner can undo.
- **Models are ordered lists; data has a tier, models a clearance**
  ([0105](0105-models-in-ordered-lists-by-data-tier.md)). Providers are entered once; everyday
  and deep are sorted lists, the first working, cleared entry answering. A cloud model sees
  Confidential only disguised and Restricted never; every cloud entry goes through the one door.
- **The team gets contracts, never keys** ([0106](0106-team-contracts.md)). HQ's workers reach
  the home lab only through narrow abilities the node serves and decides: reads are free, a
  safe fix runs on its own within limits kept in the node's database and is told to the
  owner, and anything riskier asks. No key and no raw access enter a worker's container.
- **Goals are passive; work arrives as tasks** ([0107](0107-routines-alarms-and-check-backs.md)).
  A routine hands its member a task on a rhythm, one open at a time; an alarm from the
  home lab joins it; a member checks back on its own task. Nobody goes looking for work, a
  clock with nothing to do starts no turn, and a task whose turns move nothing backs off.
- **Endora decides what a tool is; the team gets Endora's tools under Endora's policy**
  ([0108](0108-the-bridge.md)). Every capability and tool-server tool is offered to the team
  unless it is off, Restricted, administration or the owner's own; Confidential only to local
  turns; acts only for the roles that act; an act that is on runs within limits and is told,
  an act that asks waits on the owner's desk, and the owner's yes runs exactly what was shown.
  A turn on the house's own small model on the NAS is offered none of them
  ([0131](0131-the-two-kinds-of-turn.md)).
- **Each member has a brain the owner sets; a cloud turn reads only what the owner allowed**
  ([0109](0109-a-brain-per-member.md)). Local by default, or a Claude the role may use, paid
  from the team's pool. HQ tells the node which turns are cloud; the lead's may read scrubbed
  logs and the download queues, no other Confidential result reaches a cloud turn, and a turn
  on the house's model never reaches Claude.
- **The team works in rings; Endora is the only way out** ([0110](0110-the-teams-rings.md)).
  A turn's sandbox on the Pi has a shell and git, no keys and no internet, and reaches only
  the node's team gateway, which passes it to HQ with its own token. No keys in the sandbox;
  scanners catch accidents, narrow exits and keeping data away catch intent. The egress proxy
  (an allowlist the owner grows from the desk) is designed and deferred.
- **The owner is the board; the CEO runs the team** ([0111](0111-the-ceo-and-the-chain-of-command.md)).
  A member's question goes up the chain (to the lead, the lead's to the CEO) and is answered in
  the task's thread; only the CEO's decision, with its recommendation and checked by the
  reviewer, reaches the board's desk. The CEO thinks with Opus at up to $1 a turn, wakes only
  for escalations and a daily review, keeps the house log, reads Endora's public records, and is
  given nothing Confidential. The pool is shared among the paid members alone.
- **Work worth its own effort is a project, with a lead and a cap; proposals carry evidence**
  ([0113](0113-projects-budgets-and-reports-to.md)). A project serves one goal; the owner may
  cap its month, and at its cap its Claude turns wait until the month turns or the owner raises
  it. Leads and the CEO propose with task numbers and event ids HQ checks; the CEO decides, the
  board may overrule, and caps are the owner's. Each member reports to someone the owner may
  change, and the CEO plans once a week from patterns.
- **The ask says what done means, and nothing is asked for that serves nothing**
  ([0124](0124-guards-against-how-agent-teams-fail.md), amended). The owner's work is refused
  without at least one criterion; a routine's task carries criteria from its routine; a lead's
  split of any task with criteria names the one each part serves; and a request for the owner's
  yes names the criterion of its task it serves, or never reaches the desk.
- **A guard that gains authority is tried in shadow first; a runaway is stopped without asking**
  ([0133](0133-caps-on-talk-and-runaway.md)). A guard that would hold the team's work records
  what it would have held, where the owner reads it and judges it, and changes nothing until the
  owner switches it on. The caps that exist because a runaway costs most when nobody is watching
  (a member's day of turns on the house's own models, forty turns in ten minutes) are on from the
  start: what they hold is kept, not lost, and the owner resumes in one tap.
- **Start fresh rather than keep what makes noise** ([0100](0100-the-clean-slate.md)). What
  Endora did unasked (check-ins, the nightly loop, notions, own work, goals) produced stale
  beliefs and repeated questions, so it went, code and storage. A table nothing reads is
  dropped, because data nothing can export or delete is outside the memory rights.

### What we learned about the person's side

- **Derived, never stored** ([0039](0054-other-peoples-services.md)). Findings are
  computed on read, so there is no queue to groom and nothing to dismiss — the structural
  version of the promise [0029](0052-what-it-knows-about-you.md) made by deletion.
- **Answering is the dismissal.** A finding whose answer has been given stops being derived.
- **A half-formed thought is Endora's to carry, not the person's to resolve**
  ([0057](0057-thinking-between-turns.md)). It may hold tentative statements
  between turns — capped, self-expiring, readable but with no verb on them — and only a
  matured one reaches the person, as a belief they can correct.
- **What it changed outside itself is kept and reversible** ([0045](0054-other-peoples-services.md)).
  A prior value nobody stores is not a reversibility story. That log survives "forget
  everything", because deleting a receipt does not undo the change.
- **Understanding decays** ([0032](0052-what-it-knows-about-you.md)) and admits only what it
  should ([0033](0052-what-it-knows-about-you.md)).

### How we work

- **Test against real data, not tidy fixtures.** Four corrections to one rule shipped and
  failed live in a single evening, every one because the tests used a five-line reading
  written by hand. The real 5KB reading — one unbroken line, forty entities with overlapping
  names — is now a fixture, and it catches what synthetic ones cannot.
- **Deploy, then read production.** Two features passed CI and could not work at all: one
  added a column existing databases never re-run, one added a table production never
  creates. Both were found by exercising the endpoints on the running system.
- **Count the rules.** Four signals now decide which thing a person meant. Each came from an
  observed failure and each has a test — and a fifth would be evidence that the instrument is
  wrong, not that it needs another refinement.

## Why we keep ADRs

Endora is designed to outlive specific models, vendors, and frameworks. ADRs
record *why* the structure is the way it is, so future contributors can revisit a
decision deliberately rather than by accident.

## When an ADR is required

- Any change to layer boundaries or dependency directions.
- Adopting or replacing a load-bearing technology (protocol, storage, runtime).
- Any change that touches the deterministic policy boundary around models.
- Adding a new runtime dependency that is not obviously justified.
- Introducing a design pattern outside [the pattern budget](../patterns.md).

**An ADR that adds a mechanism names what it retires, and the retirement ships
in the same PR.** Two implementations of one responsibility is how a refusal
got overridden ([0067](0067-one-way-to-the-deep-model.md)): the superseded path
ran last and won. Addition without subtraction is a decision too — record it.

See [CONTRIBUTING.md](../../CONTRIBUTING.md).

## Status values

`Proposed` → `Accepted` → (later) `Superseded by NNNN` / `Deprecated`.

## Format

Each ADR includes: **Status**, **Context**, **Decision**, **Consequences**, and
**Alternatives considered**. Keep them concise. To add one, copy the structure
of an existing record and take the next number.

## Index

| # | Decision | Consolidates |
| --- | --- | --- |
| [0050](0050-the-shape-of-the-system.md) | The shape of the system | 0001–0004, 0007, 0009, 0012, 0018, 0026 |
| [0051](0051-where-the-boundary-is.md) | Where the boundary is | 0005, 0010, 0022–0024, 0044 |
| [0052](0052-what-it-knows-about-you.md) | What it knows about you | 0006, 0011, 0013–0016, 0020, 0029, 0032, 0033, 0036 |
| [0053](0053-honesty-about-what-it-did.md) | Honesty about what it did | 0028, 0034, 0035, 0037 |
| [0054](0054-other-peoples-services.md) | Other people's services | 0021, 0038–0043, 0045–0048 |
| [0055](0055-the-model-layer.md) | The model layer | 0008, 0027, 0030 |
| [0056](0056-how-it-behaves-toward-you.md) | How it behaves toward you | 0017, 0019, 0025, 0031 |
| [0057](0057-thinking-between-turns.md) | Thinking between turns | — |
| [0058](0058-how-an-integration-gets-in.md) | How an integration gets in | — |
| [0059](0059-one-fact-source-many-consumers.md) | One fact source, many consumers | — |
| [0060](0060-what-the-turn-is-offered.md) | What the turn is offered | — |
| [0061](0061-answers-worth-keeping.md) | Answers worth keeping | — |
| [0062](0062-one-permission-surface.md) | One permission surface | — |
| [0063](0063-waking-on-the-world.md) | Waking on the world | — |
| [0064](0064-what-a-stranger-said.md) | What a stranger said | — |
| [0065](0065-a-place-is-not-the-models-to-remember.md) | A place is not the model's to remember | — |
| [0066](0066-their-verdict-decides-too.md) | Their verdict decides too | — |
| [0067](0067-one-way-to-the-deep-model.md) | One way to the deep model | — |
| [0068](0068-facts-are-arguments-not-recall.md) | Facts are arguments, not recall | — |
| [0069](0069-one-door-off-the-device.md) | One door off the device | — |
| [0070](0070-taint-is-derived-not-remembered.md) | Taint is derived, not remembered | — |
| [0071](0071-capabilities-it-writes-itself.md) | Capabilities it writes itself | Proposed |
| [0072](0072-the-base-has-rungs-too.md) | The base has rungs too | — |
| [0073](0073-a-dead-session-heals-itself.md) | A dead session heals itself | — |
| [0074](0074-the-brief-is-a-standing-order.md) | The brief is a standing order | — |
| [0075](0075-failures-become-specimens.md) | Failures become specimens | — |
| [0076](0076-standing-questions.md) | Standing questions | — |
| [0077](0077-seeing-something-through.md) | Seeing something through | — |
| [0078](0078-shared-execution-leases.md) | Shared execution leases | — |
| [0079](0079-goal-checkpoints-require-ownership.md) | Goal checkpoints require ownership | — |
| [0080](0080-completion-uses-tool-observations.md) | Completion uses tool observations | — |
| [0081](0081-declare-work-and-search-for-sources.md) | Declare work and search for sources | — |
| [0082](0082-announce-every-way-a-goal-ends.md) | Announce every way a goal ends | — |
| [0083](0083-show-unattended-work-and-let-it-be-stopped.md) | Show unattended work and let it be stopped | — |
| [0084](0084-tell-the-person-what-actually-went-wrong.md) | Tell the person what actually went wrong | — |
| [0085](0085-a-question-about-the-world-is-not-answered-from-the-house.md) | A question about the world is not answered from the house | — |
| [0086](0086-ask-deep-plans-and-the-house-reads.md) | Ask deep plans, and the house reads | — |
| [0087](0087-a-local-answer-that-found-nothing-is-checked-once.md) | A local answer that found nothing is checked once | — |
| [0088](0088-say-what-is-sent-to-the-model-and-let-a-topic-end.md) | Say what is sent to the model, and let a topic end | — |
| [0089](0089-a-model-sorts-the-message-not-a-word-list.md) | The local model sorts the message, not a word list | — |
| [0090](0090-skills-are-sent-once.md) | Skills are sent once | — |
| [0091](0091-what-the-deep-model-costs.md) | What the deep model costs | — |
| [0092](0092-the-home-lab-is-a-server-owned-read-view.md) | The home lab is a server-owned read view | — |
| [0093](0093-the-handheld-is-an-explicit-home-console.md) | The handheld is an explicit home console | — |
| [0094](0094-work-it-finds-for-itself.md) | Work it finds for itself | — |
| [0095](0095-more-sources-for-work-it-finds.md) | More sources for the work it finds | — |
| [0096](0096-patterns-it-notices.md) | Patterns it notices | — |
| [0097](0097-it-reads-the-mail.md) | It reads the mail | — |
| [0098](0098-it-can-be-rebuilt.md) | It can be rebuilt | — |
| [0099](0099-the-handheld-is-part-of-endora.md) | The handheld is part of Endora | — |
| [0100](0100-the-clean-slate.md) | The clean slate | — |
| [0101](0101-the-brain.md) | The brain | — |
| [0102](0102-endora-hq.md) | Endora HQ | — |
| [0103](0103-the-agenda-loop.md) | The agenda loop | — |
| [0104](0104-endora-sound.md) | Endora sound | — |
| [0105](0105-models-in-ordered-lists-by-data-tier.md) | Models in ordered lists, cleared by data tier | — |
| [0106](0106-team-contracts.md) | Team contracts: what Endora offers HQ's workers | — |
| [0107](0107-routines-alarms-and-check-backs.md) | Routines, alarms and check-backs: how the team keeps working | — |
| [0108](0108-the-bridge.md) | The bridge: Endora's capabilities as the team's tools | — |
| [0109](0109-a-brain-per-member.md) | A brain per member, and what a cloud turn may read | — |
| [0110](0110-the-teams-rings.md) | The team's rings: a sandbox on the Pi, the team gateway, the egress proxy, and undo | — |
| [0111](0111-the-ceo-and-the-chain-of-command.md) | The CEO and the chain of command | — |
| [0112](0112-the-team-knows-how-the-lab-is-built.md) | The team knows how the lab is built | — |
| [0113](0113-projects-budgets-and-reports-to.md) | Projects, budgets and reports_to | — |
| [0114](0114-evidence-acceptance-criteria-and-the-tester.md) | Evidence, acceptance criteria and the tester | — |
| [0115](0115-the-engineering-wing-and-the-git-flow.md) | The engineering wing and the git flow | — |
| [0116](0116-the-teams-standards.md) | The team's standards | — |
| [0117](0117-claude-code-as-a-developer-runtime.md) | Claude Code as a developer runtime | — |
| [0118](0118-elastic-staffing-and-notebooks.md) | Elastic staffing and notebooks | — |
| [0119](0119-the-open-office.md) | The open office | — |
| [0120](0120-roles-as-an-enforced-raci.md) | Roles as an enforced RACI | — |
| [0121](0121-games-in-the-office.md) | Games in the office | — |
| [0123](0123-a-bench-for-an-external-agent-harness.md) | A bench for an external agent harness | — |
| [0122](0122-the-teams-world-stages-stories-and-situations.md) | The team's world: stages, stories and situations | — |
| [0125](0125-secrets-at-rest-in-the-node.md) | Secrets at rest in the node | — |
| [0124](0124-guards-against-how-agent-teams-fail.md) | Guards against how agent teams fail | — |
| [0126](0126-the-teams-map.md) | The team's map | — |
| [0127](0127-openbao-and-the-passkey-unlock.md) | OpenBao and the passkey unlock | — |
| [0128](0128-an-airplay-2-door.md) | An AirPlay 2 door | — |
| [0129](0129-the-teams-history-and-asking-the-past.md) | The team's history, and asking the past | — |
| [0130](0130-the-macs-brain.md) | The Mac's brain | — |
| [0131](0131-the-two-kinds-of-turn.md) | The two kinds of turn: the house's models work, Claude judges | — |
| [0132](0132-outcomes-and-a-desk-that-interrupts-only-for-decisions.md) | Outcomes, and a desk that interrupts only for decisions | — |
| [0133](0133-caps-on-talk-and-runaway.md) | Caps on talk and runaway | — |

The forty-nine decisions these consolidate are in [archive/](archive/) — kept in full,
because the reasoning that produced a rule is the reason it survives an argument later.
