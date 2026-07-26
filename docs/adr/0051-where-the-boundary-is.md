# 0051 — Where the boundary is

## Status

Accepted (2026-07-26). **Consolidates 0005, 0010, 0022, 0023, 0024 and 0044**, which are
archived. The single most load-bearing document in this repository.

## Context

Endora is trusted to act — to switch things on in someone's home, to send things outward,
to spend attention and eventually money. The question every other decision defers to is:
*what is allowed to authorize an action?*

The answer has been the same since [0005] and has survived every measurement since. This
model obeys an explicit, direct instruction about verification roughly **one run in three**.
It has invented floors, invented areas, put a noun in a category field, and once produced a
call that switched on **every light in the house**. None of that is a reason to distrust
models; it is a reason not to put them where a guarantee is required.

## Decision

### Models propose. Deterministic policy authorizes. Capabilities execute.

Model output is a **proposal**, never an authorization. A deterministic policy layer decides
whether a proposal may run, given the autonomy envelope, the person's consent, and the
action's reversibility. The capability then executes behind that boundary.

This is not a statement about how good models are. It is a statement about where a
guarantee can live, and a guarantee cannot live in a prompt.

### Bands, by reversibility, deny-by-default

Every capability carries a band, and the band — not confidence, not intent — decides what
may happen without a person:

| band | what it means | unattended |
| --- | --- | --- |
| `Observe` | reports the world, changes nothing | may act |
| `Reversible` | has an undo, or leaves no lasting mark | may act |
| `OutwardReversible` | reaches outside, but can be taken back | asks |
| `Irreversible` | spending, sending, deleting | blocked until deliberately opened |

Unclassified is treated as irreversible. Opening a capability's irreversible band moves it
from *blocked* to *confirm-each-use* — **never** to autonomous.

Unattended turns (the heartbeat, the nightly loop) are clamped to the reversible bands
regardless of what the person opened for conversation. Their openers and a widened envelope
clear an actuator to run while they are *present, watching, and able to say stop*; none of
that is true at 03:00.

### The act/ask loop

**Act** when the relevant preference is known *and* the action is reversible or
pre-authorized. **Ask** when uncertain, ambiguous, or consequential. The loop is run by
deterministic code; the model supplies the proposal and the words, not the verdict.

### The autonomy envelope

A declarative, person-owned policy along axes that are **deterministically classifiable** —
never a matter of the model's say-so. Widening it grants more independence within the same
boundary; it never moves the boundary.

### The egress guard and the data-loss tripwire

Before any model- or person-supplied URL is fetched: http(s) only, and the host is blocked
if it is or resolves to a non-public address — loopback, RFC1918, link-local (including the
cloud metadata address), unspecified, multicast. Outbound payloads are scanned for secrets
and personal identifiers are redacted. A system that browses on someone's behalf from inside
their network is an SSRF engine unless this exists.

### Deterministic autonomy is still autonomy

Where a finding is **arithmetic over stored records**, the action is **reversible**, and the
result is **announced**, policy acts without asking. All three conditions are load-bearing:

- arithmetic, because nothing generative may sit inside a policy decision;
- reversible, because being wrong must cost a click;
- announced, because a capability quietly disappearing is how trust erodes.

Drop any one and this stops being safe autonomy and becomes a model acting on a guess. The
first application was withdrawing a tool that has never once worked
([0054](0054-other-peoples-services.md)).

## Consequences

- The interesting question is never "is the model confident?" but "what band is this, and
  what did the person open?" — which is answerable in code and reviewable by a person.
- Endora can be made more autonomous by widening the envelope without weakening anything,
  because the envelope is enforced rather than suggested.
- **Being conservative has a cost**: things it could have done, it asks about. That cost is
  chosen.
- Every guarantee this system offers is enforced at exactly one layer, so there is one place
  to audit and one place to break.

## Rejected

- **Letting the model self-report reversibility or safety.** It is the untrusted party in
  this arrangement; a hostile or careless proposal says the same words as a good one.
- **Trusting a server's own claims** about whether its tools are read-only. Same reasoning,
  different untrusted party ([0054](0054-other-peoples-services.md)).
- **Confidence thresholds** as an authorization mechanism. Calibration is not a property this
  model has.
- **An "autonomous" mode that removes the boundary.** There is no setting that makes the
  model the enforcement layer; more autonomy is always more envelope, never less policy.
- **Prompting for safety.** Measured at 1-in-3, repeatedly.
