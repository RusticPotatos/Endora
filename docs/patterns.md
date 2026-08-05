# The pattern budget

> The closed set of design patterns this codebase uses, each under its canonical
> name. New code composes these; introducing a pattern not on this list requires
> an [ADR](adr/README.md) that also names what it retires.

## Why a budget

The failure this document exists to prevent has already happened. Two
implementations of one responsibility — escalation to the deep model — coexisted
because nothing named the pattern the first one used, so the second did not read
as a duplicate ([ADR 0067](adr/0067-one-way-to-the-deep-model.md)). It bypassed
the taint rule, the consent gate and the pseudonym layer, and it was found by a
user-visible failure rather than by review.

A mixed bag of patterns is not eight patterns; it is mechanisms without names.
Named, the second implementation of an existing pattern is visible in a diff as
a violation. This is the same move that collapsed eight permission axes into one
`Stance` ([ADR 0062](adr/0062-one-permission-surface.md)): enumerate, collapse,
close the set.

Alignment note: this list uses each pattern's **canonical** name — Gang of Four
where a GoF name exists, and the honest source elsewhere. Half of what carries
this codebase postdates GoF (1994): Ports & Adapters is Cockburn, Repository is
DDD, typestate is a Rust idiom. Renaming them to the nearest GoF entry would
mislabel the architecture, not simplify it.

## The set

### 1. Ports & Adapters *(Cockburn; GoF cousins: Strategy, Adapter)*

Every boundary. The application layer declares a trait (`Butler`,
`CapabilityRunner`, `ChatRepository`, `AuditLog`, `Clock`, `IdSource`);
infrastructure implements it; dependencies point inward. This is ROCA's
load-bearing pattern ([ADR 0050](adr/0050-the-shape-of-the-system.md))
and the reason the model vendor, the storage engine and the clock are all
swappable in tests.

**Rule:** a use case never names a concrete adapter. If a use case needs
something new from the world, it grows a port, not an import.

### 2. Decorator *(GoF)* — runners that narrow authority

`CapabilityRunner` wrapping `CapabilityRunner`, each wrapper removing or
reshaping what may run, never adding: `ReversibleOnlyRunner` (the overnight
envelope), `OpenerRunner` (deferral), `WithdrawnRunner`, `AliasRunner`,
`TargetSearchRunner`. `CompositeRunner` is the one **Composite** *(GoF)* joining
them.

**Rule:** authority only narrows through the chain. A decorator that *grants*
something its inner runner would refuse is the bug, not a variant.

### 3. Facade *(GoF)* — the orchestration layer

`endora-application`'s use cases are the only way a composition root reaches a
turn. The butler-turn contract lives here; contexts talk
application-to-application, never reaching into each other's internals.

**Rule:** one facade. A second path to the same effect — however convenient — is
[ADR 0067](adr/0067-one-way-to-the-deep-model.md) again. When a new mechanism
supersedes part of an old one, the retirement ships in the same PR.

**Corollary — an offer is a promise.** Every control the console renders is an
offer, and the handler behind it must not be allowed to refuse it for data the
renderer can produce. The "It's gone" button erred exactly here: the card
derived the offer from the trouble list, the handler enforced a precondition
(a native channel exists) the renderer never checked, and the person got a
true-but-useless error every pass. When a handler keeps an error branch for
"the UI shouldn't send this", that branch is the bug's half-built nest: either
the list endpoint computes the offer from the same predicate the handler
enforces, or the handler learns to honour the offer for every shape the data
can take. A 4xx reachable from a rendered control is a defect by definition.

### 4. Observer *(GoF, in closure form)* — progress and tokens

`on_step` and `on_token` callbacks: the turn reports what it is doing without
knowing who is watching. No subscription registry, no event bus — a `&mut dyn
FnMut` is the whole pattern, deliberately.

**Rule:** observers observe. Nothing decided inside a turn may depend on what a
callback did.

### 5. Repository *(DDD/Fowler; not GoF)* — persistence behind intent

`*Repository` traits expose domain intent (`list`, `save`, `react`), never
storage vocabulary. What is derived from records — graduation, withdrawal,
rarity — is **derived, never stored**, so deleting a record retracts its
consequences ([ADR 0062](adr/0062-one-permission-surface.md),
[0066](adr/0066-their-verdict-decides-too.md)).

**Rule:** no second store for something the record already implies.

### 6. Smart constructor & typestate *(Rust idioms; "parse, don't validate")* — guarantees by construction

The direction of travel for every guarantee that today lives as a runtime check
inside the turn ([ADR 0068](adr/0068-facts-are-arguments-not-recall.md) names
the two slices ahead: the egress door, and `Turn<Tainted>` without
`escalate()`). A value that exists is a value that passed its checks; the
invalid call does not compile.

**Rule:** when a guarantee is bypassable by "calling the other function", it is
in the wrong pattern — move it here.

## Deliberately not in the set

- **Singleton** — the composition root wires exactly one of things; a global is
  the same idea minus the testability.
- **Template Method** — trait default methods stay thin conveniences
  (`take_turn_streaming` collecting tokens); logic in a default method is logic
  a test can't reach around.
- **Abstract Factory, Visitor, Mediator, Memento…** — absent not because they
  are bad but because nothing here needs them. The first real need is an ADR,
  not an import.
- **Frameworks for any of the above.** Every pattern in the set is expressible
  in plain traits and closures; a framework would be a dependency doing what a
  page of Rust does.
