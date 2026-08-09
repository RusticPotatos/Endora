# 0069 — One door off the device

## Status

Accepted (2026-08-04). The first of the two slices [0068](0068-facts-are-arguments-not-recall.md)
named; makes [0067](0067-one-way-to-the-deep-model.md)'s "one way to the deep model"
structural rather than observed. Pattern six of [the pattern budget](../patterns.md).

## Context

[0067](0067-one-way-to-the-deep-model.md) deleted a second escalation path that had carried
the person's raw sentence to a third-party API past a refusal the first path had correctly
made. The deletion was right and it fixed the instance, not the class: after it, "one way to
the deep model" was still a fact about the current code rather than a property of the code.
Nothing prevented a third path — a convenience in a new use case, a helper in a test that
migrates to production — from growing beside the turn exactly as the second one had.

Building the door found the class was already live again in miniature. The surviving path
applies the pseudonym table but **not the outbound secret scan**; the manual button applies
the scan but not the table. Two sites, two different subsets of the guarantees, drifted —
which is [0067](0067-one-way-to-the-deep-model.md)'s disease with the roles reshuffled. A
key pasted into chat would have ridden an automatic escalation off the device.

## Decision

### The connection is a private field

`egress::Deeper` wraps the deep-model connection, and the trait's `deeper()` returns it
instead of the bare butler. There is no accessor. The only operations are the ones the
module defines, and each runs the module's checks on the way through:

- **`continue_turn`** — escalating a turn: refuses a stranger's words
  ([0064](0064-what-a-stranger-said.md)), disguises the person's messages and context
  ([0051](0051-where-the-boundary-is.md)), scans what would actually leave and fails
  closed, restores the reply's real values, sets `escalated`.
- **`word`** — wording prose from facts Endora assembled: disguises the facts, scans,
  restores. The instruction travels verbatim because it is Endora's own sentence.
- **`TheirOwnQuestion::checked`** — the manual button: refuses an apparent secret, redacts
  the one PII shape a bare question carries. Consent is the press
  ([0055](0055-the-model-layer.md)); `ask_deep_model` accepts nothing but this type.

A caller states what it has and receives an answer already restored. It cannot skip a
check, reorder one, or see the hidden form — the misuse does not compile, and the module
carries a `compile_fail` doctest that pins exactly that.

### The scan runs after the disguise

On what would actually leave: a value the disguise already hid is accounted for, and
anything still secret-shaped is not Endora's to send. This ordering is now a property of
the module rather than a decision each call site makes — which matters because the two
sites that existed had already made it differently.

### What moved, and what was retired

The taint check at the escalation site, the disguise assembly (`personal_values_in`), and
`STRANGER_MARK` all moved into the door; the site now decides only *whether* escalation is
wanted, never what may leave. `word_the_brief` became a sentence handed through the door.
Retired in the same PR, per the retirement rule: the site's `!a_stranger_spoke` filter, the
inline hiding block, and the button's inline scan-and-redact.

### What stays outside the door, and why

- **The notion-proposer** disguises but talks to the *local* model. Nothing leaves the
  device; the disguise there is hygiene, not egress.
- **Skill GETs** already have their own door — the two egress helpers
  [0061](0061-answers-worth-keeping.md) built caching into are the only functions that
  carry a built-in skill's outbound request, and their checks (stance policy, key
  handling) live at that seam. One door *per kind of leaving*, not one function.
- **MCP transports** stay live by design ([0061](0061-answers-worth-keeping.md)): the house
  must be current, and their inputs pass the runner's policy gate instead.

## Consequences

- **A third escalation path cannot be written.** The connection is unreachable except
  through methods that check. This converts three review obligations into type errors.
- **The secret-scan gap is closed**, and closed for future paths too — a new `Deeper`
  method without the scan would be a diff inside the one module whose whole subject is
  what leaves.
- **The consent gate stays where it was** — `deeper()` returns `None` unless the person
  opted into automatic escalation — and now guards every automatic path there is.
- **Callers lost expressive power on purpose.** A use case wanting a novel deep-model
  interaction must add a method here, in review, beside the checks — which is the point.
- **The turn still marks; the door refuses.** `STRANGER_MARK` is written where tools run
  and read where egress happens. Phase 2 (`Turn<Tainted>`) will move the marking itself
  into a type; this record deliberately does not reach for that yet.

## Rejected

- **Sealed argument types** (`Disguised` passed by the caller). Enforces that a disguise
  *happened*, not that the scan and taint check happened too, and the caller still holds
  the raw material beside the door. Hiding the connection enforces all of it at once.
- **A door in the infrastructure crate.** The checks are application-layer facts — the
  pseudonym table is built from the butler context, the taint mark from the conversation.
  Infrastructure implements the connection; it does not get to define what may use it.
- **Routing the local model through the same door.** Nothing leaves the device on that
  path, and disguising local turns would cost prompt fidelity on the model that most needs
  it, for no boundary crossed.
- **A runtime registry of "approved egress functions".** A list is convention wearing a
  uniform; the compiler already has a mechanism for "this is unreachable", and it does not
  go stale.
