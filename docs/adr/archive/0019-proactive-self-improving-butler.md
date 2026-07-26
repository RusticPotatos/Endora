# 0019 — The proactive, self-improving butler (heartbeat, check-ins, capabilities, hospitality)

## Status

Accepted (2026-07-20). The north-star direction for the butler; implemented in slices
(see the sequencing at the end). Builds on
[ADR 0010](0010-autonomy-model.md) (act/ask autonomy),
[ADR 0014](0014-the-butler-conversation-values-attention.md) (the butler),
[ADR 0016](0016-adaptive-attention.md) (attention), and
[ADR 0017](0017-persona-and-voice.md) (persona).

## Context

Today the butler is **reactive**: it answers when spoken to. The vision is a butler
with **agency and growth** — one that runs alongside the person 24/7, improves itself,
reaches for skills, and — most of all — **knows the person well enough to serve before
being asked** (the concierge instinct). We already built the two hard things: the
**learning loop** (Direction→Assumption→Experiment→Observation→Reflection→ProcessChange)
and the **act/ask autonomy model**. This ADR points them at the butler itself and adds
the machinery for proactivity, skills, and deep user-modelling — without weakening the
invariant that **models propose and deterministic policy authorizes**.

## Decision

### 1. The heartbeat — the butler's clock

The always-on node gains a background loop (a `tokio` interval; the node is already a
24/7 server). On each tick it does not *act* directly — it **evaluates** what, if
anything, is due (a check-in? a morning briefing? a follow-up?) and routes each
candidate through the **autonomy model**: `act` (low-stakes, do it — e.g. gather
research), `ask` (surface for confirmation), or `notify`. Nothing consequential fires
from a tick without passing the policy boundary. The cadence and quiet-hours are
configuration the person owns (Settings), so the butler is never noisy or surprising.

### 2. Check-ins — scheduled, person-controlled, self-improving

On a cadence the person sets (**when and how often** — e.g. a morning and/or evening
check-in), the butler opens a **natural** conversation — *"Good morning — if you have a
moment, I noticed X; what would you like me to focus on more?"* A check-in is a normal
chat turn (an `ask`, low-stakes), and its purpose is **self-improvement**: it asks for
clarification of what it can do better and what to focus on. The person's answer feeds
the learning loop (below) and their preference memory.

### 3. Capabilities / skills — modular, MCP-based

Skills are **MCP (Model Context Protocol) servers** — the open, modular standard for
giving a model tools and data. Endora's node is the **MCP host/client**; each skill is a
server it connects to (locally over stdio, or a remote server), so a skill can be added
or removed without touching core, and Endora can draw on the existing ecosystem — a
**browser/web** server, search, filesystem, git, and (later) Home Assistant. A Rust MCP
SDK exists, so the node speaks it natively.

The **Endora-specific adaptation is the whole point.** Plain MCP has the model invoke
tools directly; Endora's invariant is *models propose, deterministic policy authorizes*.
So a tool call the butler wants to make is a **proposal**, not an execution: the node's
policy layer plus the skill's autonomy level decide — **act** (run it now: low-stakes,
read-only, local — e.g. "read today's weather") or **ask** (surface for confirmation:
consequential or external — e.g. "submit this form", "unlock the door"). Only then does
the MCP client actually call the server. Endora is a **governed MCP host** — the model
never becomes the enforcement boundary.

Each connected server is wrapped as a `Capability` that adds what MCP does not itself
carry: its **autonomy level** and whether it **leaves the machine** (a browser obviously
does → opt-in, marked external). This keeps *local-first* honest: a purely-local skill
and an internet-reaching one are visibly different. First skills: a **browser/web** skill
(read-only browsing for briefings and research; any consequential web action is
ask-gated) plus **light touches** (a joke, something it thinks you'd find interesting).
**Home Assistant is just another MCP server, later** (consequential → strictly gated).

### 4. The learning loop, turned on the butler itself

The elegant reuse: the cycle we built for the person's goals also models **the butler's
own behaviour**. A check-in or observed outcome is an *observation*; the person's
feedback is a *reflection*; "focus more on mornings / stop re-asking X / brief me before
7am" becomes a *process change* to how the butler operates — proposed, approved, and
audited like any other. The butler improves the way it already helps the person improve.

### 5. Deep user-modelling for hospitality

The butler continuously enriches an evolving model of the person and uses it to
**anticipate**:
- **Patterns** — observed over time (e.g. typical wake time, routines). Stored as
  observations about the person, never as fact stated by them.
- **Values & likes** — explicit ("I love good espresso") *and* **inferred** from repeated
  signals. Inferred items are **proposed, not assumed**: *"I've noticed you mention coffee
  a lot — shall I remember you value a good café?"* — confirmable and correctable, per the
  visible/correctable/deletable memory principle. This is the guard against creepy or
  wrong-and-stuck inference.
- **Opportunity matching** — the hospitality payoff: connect something happening in the
  world (a new café opened on your route; a holiday; a change in your commute) to
  something the model knows you value, and surface it as a warm, proactive suggestion.
  Overheard "fun things" become service opportunities, not just stored facts.

### Invariants (non-negotiable)

- **Models propose; deterministic policy authorizes.** Proactivity never bypasses this;
  the heartbeat only *raises candidates*.
- **Autonomy is graded and person-tunable.** Cadence, quiet hours, and per-capability
  autonomy levels are the person's to set.
- **Inference is transparent.** Anything the butler concludes about the person is
  visible, confirmable, and deletable — never silently assumed.
- **External reach is explicit and opt-in.** A skill that leaves the machine is marked
  as such; local-first is the default.
- **Everything consequential is audited.**

## Consequences

- The node grows a scheduler and a capability registry; the butler gains a proactive
  path alongside the reactive chat. No change to the propose→confirm safety model.
- Earlier queued work becomes supporting pieces: the **persistent-suggestions "events"
  memory** is the butler's proposal inbox; the **Settings page** holds cadence + skill
  toggles; **Kokoro voice** gives check-ins a real voice.
- External skills introduce the first non-local data flows — hence the explicit
  opt-in + data-access declaration.

## Sequencing (proposed build order)

1. **Persistent-suggestions memory** (durable proposal "events" + the target-id fix) —
   the substrate proactivity writes into. *(Tangible, self-contained.)*
2. **Heartbeat + first check-in** (with cadence in Settings) — the autonomy backbone and
   immediate self-improvement value.
3. **MCP host + a browser/web skill** (the first governed MCP server: read-only browsing
   for a morning briefing — weather/events + a light touch) — modularity proven and the
   hospitality debut.
4. **Inference & opportunity-matching** — proposed patterns/likes and world→you matches.
5. **Home Assistant capability** — later, once the skill + policy plumbing is proven.
