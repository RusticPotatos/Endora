# Endora Domain Map

> Status: foundation phase. This lists the **planned** bounded contexts and what
> each one owns. It deliberately stops short of naming detailed entities, fields,
> or APIs — those are designed with the first vertical slice that needs them, not
> invented up front.

Each context owns its own model and vocabulary. Contexts collaborate through
explicit application-layer boundaries, never by reaching into one another's
internals (see [architecture.md](architecture.md)).

## Identity & Values

Owns who the user is *to themselves*: their stated values and the principles
they want to live by. The source of the user's own definition of a good life.
Nothing here is inferred silently; values are user-owned and user-editable.

## Direction & Targets

Owns long-term **North Stars** (directions) and the concrete **targets** beneath
them, and the relationships between them. Turns values into direction the rest of
the system can reference.

## Experiments & Learning

Owns the learning loop as data: assumptions, hypotheses, small experiments,
their designs, and outcomes. This is where "run a small experiment and see what
happens" is represented.

## Reflection

Owns observations, retrospectives, and reflections. Consumes experiment
outcomes and lived signals; produces the material from which process-improvement
proposals are drawn.

## Memory

Owns durable, user-facing memory: what Endora retains as useful evidence over
time. Enforces the constitutional **memory rights** — visible, correctable,
exportable, deletable. Other contexts store *their* durable knowledge here or
reference it; Memory is the guardian of the rights, not a dumping ground.

## Policy & Consent

Owns the **deterministic authorization** rules: what is permitted, under what
autonomy level, with what consent. This is the enforcement boundary around
probabilistic models — models propose, this context authorizes. Owns autonomy
levels, consent records, and permission decisions.

## Capabilities

Owns the catalog of things Endora *can do* (capabilities) and their execution,
under least authority. A capability runs only what Policy & Consent authorized;
it never trusts a model's request directly.

## Protection

Owns safety and protective concerns that cut across contexts: guarding against
irreversible or disproportionate actions, rate/impact limits, and safe defaults.
Complements Policy & Consent (which decides *permission*) by tending to *harm
reduction*.

## Audit & Accountability

Owns the audit trail: what was proposed, what policy decided, what executed, and
why. Records exist to protect the user and are themselves subject to memory
rights. Provides the accountability the constitution requires without becoming
surveillance.

---

## Relationships (high level)

```text
Identity & Values ──▶ Direction & Targets ──▶ Experiments & Learning
                                                     │
                                                     ▼
                                                Reflection
                                                     │
                                                     ▼
                                        Proposed process change
                                                     │
   (all consequential actions pass through)          ▼
Capabilities ◀── Policy & Consent ◀────────── Human approval
     │                  ▲
     ▼                  │
  Protection        Audit & Accountability
                        ▲
        Memory ─────────┘  (durable, user-owned evidence throughout)
```

This diagram shows intent, not a wiring spec. Concrete contracts are defined per
vertical slice.
