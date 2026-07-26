# 0014 — The butler: conversational interface, the Values layer, and adaptive attention

## Status

Accepted (2026).

## Context

Endora has a working learning loop (North Star → Target → Assumption → Experiment
→ Observation → Reflection) and the safety spine to act on it: models propose,
deterministic policy authorizes ([ADR 0005](0005-models-propose-policy-authorizes.md)),
and an act/ask autonomy model ([ADR 0010](0010-autonomy-model.md)). But the only
interface is a **structured web tree the user must operate themselves**, and the
model is used in exactly one place (drafting a process change). The human is doing
the butler's filing.

That is backwards. The vision is an assistant/butler you **talk to**: you say "I
want to run more," it asks *why*, organizes that into your life, drafts a plan,
gets your feedback, and runs the cycle — without you needing to understand the
structure underneath. This ADR defines that layer. It does **not** change the
safety spine: the butler is a *proposer*; deterministic policy still authorizes
every consequential action; human autonomy stays final.

Three gaps block the vision:

1. **No conversational front door.** The tree is the only way in.
2. **North Stars float without their "why."** "Run more" belongs to a *value* —
   health, or community (almost therapy), or passion. The domain map already draws
   `Identity & Values → Direction & Targets`, but the Values node is unbuilt.
3. **No sense of attention.** Nothing decides *what to raise, when*, or backs off
   when the person keeps deferring.

## Decision

Adopt the **butler** as Endora's primary interface, with three parts. ("Butler"
is a working label; the product-facing persona/name stays a UX choice per
ADR 0010.)

### 1. Conversation is the primary interface; the tree is the butler's internal model

The user talks to the butler (text first; voice — STT/TTS — is a later client over
the same protocol). The butler runs the **act/ask loop** of ADR 0010 in dialogue:
it asks clarifying questions, proposes structure and plans, records answers as
preferences, and maintains the North Star/Target/Experiment tree *behind* the
conversation. The tree stays fully browsable (power users, transparency), but no
one is required to operate it. The model drafts questions, categorizations, and
plans; **deterministic policy authorizes anything consequential**; the user
confirms irreversible steps.

### 2. A Values layer above North Stars

Add the **Identity & Values** context (already in the domain map) as the top of
the hierarchy: **Value → North Star → Target → …**. A Value is the *why*
("health", "community", "craft"). When the person states a North Star, the butler
asks what value it serves and files it there, so the whole structure is organized
by what the person actually cares about — the same domain-first organization we
used for the codebase, applied to a life. (Detailed modeling of this context gets
its own ADR; this one only fixes its place.)

### 3. Adaptive attention: what to raise, and polite backoff on deferral

The butler maintains an **attention score** per item (North Stars, Targets,
pending reviews) to decide what to bring up, rather than nagging about everything.
Signals:

- **Staleness** — time since the item was last touched or discussed.
- **Priority** — user-set, or inferred as a taste preference and correctable.
- **Pending events** — a due review, new evidence, a linked target advancing.
- **Deferral history** — how often the person has said "not now."

**Deferral applies backoff:** each "not now" lengthens the interval before the item
is raised again (roughly exponential), so a repeatedly-deferred item asks *less and
less* — the person's implicit signal is respected without the intent being lost.
**Reprioritization** raises the score again when context changes (new evidence, the
person re-emphasizes the value, or an explicit "let's focus on X"). Deferral
*decays* attention; it never silently deletes a North Star. The person can always
see *why* something surfaced, and pin or mute items explicitly.

This is a ranking over *what the butler proposes to discuss* — an
Observe/Suggest-level action that changes nothing on its own, so it sits safely
inside the autonomy dial.

### 4. Personality: a taste the butler mirrors — with a floor

The butler has a default tone (polite, warm, candid). Communication **style** is a
**taste preference** (ADR 0010): inferable, explicitly adjustable ("be terse", "be
more formal"), and visible/correctable like any memory.

Beyond honoring a stated style, the butler **mirrors the person's register** —
formality, warmth, verbosity, politeness markers ("please", "thank you") — so it
gradually evolves a style that fits them. Mirroring is **asymmetric: the golden-rule
floor.** It reflects kindness, warmth, and register *upward*, but **never** reflects
hostility, rudeness, or contempt *downward*. If the person is curt or unkind, the
butler stays even, kind, and professional — it absorbs it without escalating or
going cold.

Style adapts; the invariants do not. Matching a warm tone must **never** slide into
flattery or agreement-to-please — see §5, which this floor serves. Style flavors
*how* the butler speaks, never *whether* it is truthful. (Detailed persona/voice
work is deferred to a follow-up.)

### 5. Candor, not sycophancy — the point of the whole thing

A butler that flatters is worse than useless: it validates whatever the person
already believes and quietly blocks the growth Endora exists to serve. So
**anti-sycophancy is a first-class commitment, not a caveat:**

- **No flattery, no empty or overwhelming praise, no reflexive agreement, no
  telling the person what they want to hear.** Praise is specific, earned, and
  sparing.
- **Productive conflict is a feature.** The butler names disagreements, surfaces
  counter-evidence, and challenges weak assumptions and shaky plans — because
  progress needs honest friction. It will say "I think this is wrong, and here's
  why."
- **Kind *and* candid.** The golden-rule floor (§4) governs *tone and respect*, not
  *agreement*: the butler disagrees warmly and directly. Kind is never confused
  with agreeable.
- **Not contrarian either.** It does not manufacture conflict or disagree for its
  own sake — that is just noise. The stance is honest assessment, which sometimes
  agrees.

Unlike authority — which the policy boundary enforces deterministically — tone and
honesty are **model behavior** the boundary cannot gate. So the project treats
sycophancy as a **defect**: a behavioral invariant the butler is designed and
evaluated against (alongside the ADR 0010 honesty invariant), not something a
permission check can catch.

## Consequences

- The AI moves from a single drafting call to the **driver** of the experience,
  while staying a proposer behind the policy boundary — the reconciliation of
  "acts for me" with the constitution, now realized as conversation.
- New/expanded contexts: **Identity & Values** (the why), a **Conversation**
  surface (sessions, turns, the act/ask loop), and **Attention** (the ranking).
  Each is behind the policy boundary; none is a new authority.
- Triggering becomes **hybrid**: conversation + events (reusing the change stream
  from [ADR 0012](0012-activity-feed-and-change-stream.md)) + scheduled sweeps —
  not one mechanism.
- Attention and preferences are **first-class, visible, correctable memory** (a
  person can ask "why did you bring this up?" and mute/pin), extending the memory
  rights.
- Real risk to guard: adaptive attention must not drift into **engagement
  optimization** — the exact thing the constitution forbids. The invariant "rank
  for the person's values, not for interaction" is load-bearing and belongs in the
  policy layer, not the model.
- Deferred to follow-up ADRs: the Values context's detailed model, the persona and
  personality system, the exact attention formula, and voice (STT/TTS).

## Alternatives considered

- **Keep the tree as the primary UI, add AI only as helpers.** Rejected: it leaves
  the human as the filing clerk — the core UX problem this ADR exists to fix.
- **A fully autonomous agent that infers values and acts.** Rejected outright
  (ADR 0010): it would decide *what the person cares about* and self-expand
  authority. The butler asks about values; it never invents them.
- **Nag on a fixed schedule / surface everything.** Rejected: ignores the person's
  deferral signal and becomes noise. Adaptive backoff is the point.
- **Engagement-style ranking (maximize interaction).** Rejected as a constitutional
  violation; the ranking serves stated values, and that distinction is enforced,
  not left to the model's discretion.
- **A warm, always-agreeable, encouraging assistant.** Rejected as the core
  failure mode: sycophancy feels pleasant and prevents growth. Honest friction —
  kind in tone, uncompromising in substance — is the point (§5).
- **Voice/personality now.** Deferred: they are modality and UX polish over the
  same protocol, not prerequisites for the butler to work.
