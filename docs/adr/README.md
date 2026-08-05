# Architecture Decision Records

An **Architecture Decision Record (ADR)** captures a single significant
architectural decision: its context, the decision itself, the consequences, and
the alternatives that were considered. ADRs are short, immutable once accepted,
and numbered in sequence.

## What they add up to

Eight accepted records, two proposed, forty-eight archived, and a handful of ideas underneath
all of them. Read this before reading
any individual record; each line is a rule the code actually follows, with the decision
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
- **Say what you know; everything else follows** ([0059](0059-one-fact-source-many-consumers.md),
  proposed). One question — `current_states` — feeds the watch loop, the turn and the thinking.
  A source implements it and is thereby joined up, with nothing to nominate and no wiring per
  integration. The turn hears what **changed**, bounded, which is why it can be automatic.
- **MCP unless Endora needs a relationship** ([0058](0058-how-an-integration-gets-in.md),
  proposed). A tool protocol expresses "call this"; it cannot express watching a service,
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
| [0061](0061-answers-worth-keeping.md) | Answers worth keeping | — (proposed) |
| [0062](0062-one-permission-surface.md) | One permission surface | — |
| [0063](0063-waking-on-the-world.md) | Waking on the world | — |
| [0064](0064-what-a-stranger-said.md) | What a stranger said | — |
| [0065](0065-a-place-is-not-the-models-to-remember.md) | A place is not the model's to remember | — |
| [0066](0066-their-verdict-decides-too.md) | Their verdict decides too | — |
| [0067](0067-one-way-to-the-deep-model.md) | One way to the deep model | — |
| [0068](0068-facts-are-arguments-not-recall.md) | Facts are arguments, not recall | — |

The forty-eight decisions these consolidate are in [archive/](archive/) — kept in full,
because the reasoning that produced a rule is the reason it survives an argument later.
