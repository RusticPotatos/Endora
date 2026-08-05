# 0070 — Taint is derived, not remembered

## Status

Accepted (2026-08-04). The second slice [0068](0068-facts-are-arguments-not-recall.md)
named; completes the security spine [0064](0064-what-a-stranger-said.md) →
[0067](0067-one-way-to-the-deep-model.md) → [0069](0069-one-door-off-the-device.md).
Pattern six of [the pattern budget](../patterns.md).

## Context

[0064](0064-what-a-stranger-said.md) marks a stranger's words where they enter
(`STRANGER_MARK` on the tool result) and narrows what a tainted turn may do. The mark was
designed well — *carried in the result itself rather than in a variable, because a flag
threaded through four signatures is a flag somebody forgets*. But beside the mark lived
exactly such a flag: `a_stranger_spoke: bool`, set at the marking site, read at the
actuator clearance and the go-ahead message, and absent everywhere else — including the
second escalation path that bypassed the whole rule ([0067](0067-one-way-to-the-deep-model.md)).

A flag is a fact **remembered**. Every new decision site must know to consult it, and the
one that didn't is the one that shipped a bypass. The evidence — the marks — was in the
conversation the entire time; the flag was a cached copy of a question the conversation
could answer itself.

The plan called this slice "typestate": `Turn<Tainted>` without `escalate()` or
`act_unasked()`. Designing it against the real loop showed the generic split is the wrong
Rust for this shape — the transition happens mid-loop, so every round would carry an enum
of both states and the loop would double without gaining safety. What the promise actually
requires is narrower: **a decision that needs cleanliness must be unable to obtain it
except from the evidence.**

## Decision

### A proof token, derived from the marks

`egress::NoStrangerSpoke` is a zero-sized type with a private field and one constructor:

```rust
NoStrangerSpoke::given(conversation) -> Option<NoStrangerSpoke>
```

It reads the conversation as it stands and answers for it. Nothing outside the module can
forge one; there is nothing to set, clear, or thread; and holding one *means* the
conversation contains no stranger's words — the same derived-never-stored rule the
record's grants follow ([0062](0062-one-permission-surface.md)).

Both consumers derive it where they decide:

- **the door** (`Deeper::continue_turn`) — no proof, no escalation;
- **the actuator clearance** — acting unasked on anything beyond `Observe` requires the
  proof; its absence *is* the taint, and the go-ahead message says so.

The `a_stranger_spoke` flag is retired.

### The semantics sharpen, deliberately

Derivation makes taint a fact about the conversation *at the moment of each decision*. The
new test pins the consequence: the same actuator in the same turn is **cleared before**
the stranger's words arrive and **refused after**. A stored flag gave the same answer only
as long as every site remembered it; the derivation cannot be forgotten.

The proof reads `ToolResult` content only. A person *mentioning* the mark, or a seeded
finding inside an assistant message, is not a stranger speaking — the mark taints where a
tool wrote it, on the result it rode in on.

### Measured: one derivation, both consumers

With the constructor stubbed to always grant, **five tests fail across both consumers** —
the door's refusal, the clearance, and the mid-turn transition. That is the
single-point-of-truth property demonstrated rather than claimed.

## Consequences

- **A third decision site cannot forget the flag**, because there is no flag. A future
  "may this turn do X?" either demands the proof — and gets the rule for free — or
  visibly doesn't, in review, as a signature without it.
- **The spine is now uniform**: the mark travels with the result (0064), every road off
  the device is one door (0069), and cleanliness is evidence, not memory (this record).
  Nothing about taint is stored anywhere.
- **Recomputation cost** — a scan of the conversation per decision. Turns are bounded at
  a handful of rounds and messages; this is nanoseconds against a model call measured in
  seconds.
- **The clearance still composes booleans around the proof.** `configured`, `autonomous`
  and the band remain ordinary checks; only cleanliness got the unforgeable treatment,
  because only cleanliness had shipped a bypass.

## Rejected

- **`Turn<Clean>` / `Turn<Tainted>` generics.** The transition is mid-loop, so the loop
  must handle both states anyway — an enum of typestates, twice the code paths, and the
  compiler guarantee reduces to the same one the proof token gives at a fraction of the
  surface. Typestate earns its cost across API boundaries; this boundary is one loop.
- **Keeping the flag beside the proof** "for cheap reads". Two sources of truth about one
  fact is the disease this record treats.
- **A constructor that takes the flag's word for it** (`NoStrangerSpoke::because(bool)`).
  Forgeable is the whole failure; a proof you can assert into existence is a comment.
- **Extending the proof to cover seeded history.** Findings carried from past replies
  live in assistant prose, not tool results, and re-tainting every turn after any past
  web search would make graduation meaningless. 0064 scoped taint to the turn; this
  record keeps that scope.
