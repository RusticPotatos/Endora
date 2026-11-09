# Responsibility-Oriented Clean Architecture (ROCA)

> The one canonical definition of how Endora organizes its code. Other documents link here and do
> not restate it. Decided in [ADR 0050](adr/0050-the-shape-of-the-system.md), amended 9 Nov 2026.
> Layer rules: [architecture.md](architecture.md). What each context owns: [domain-map.md](domain-map.md).
> Where a new thing goes: [where-things-go.md](where-things-go.md).

## The principle

- **Organize by responsibility.** A unit is named for what it is for, in the owner's words (the
  ubiquitous language), not for its technical layer.
- **Each unit owns its own domain, application and infrastructure**, and its interface where it has one.
- **Dependencies point inward at every size**: between crates, between folders, between files, and
  inside one file, where types and rules come before the code that does I/O with them. The domain
  depends on nothing but the shared kernel. Infrastructure implements the application's ports.

## Growth: a unit takes the next stage only when a reason appears

| Stage | Shape | Moves to the next stage when |
| --- | --- | --- |
| 0 | One file | Rules appear: policy or invariants that must hold |
| 1 | One folder with `domain`, `application` and `infrastructure` files | One file holds several concepts, or is too big to read whole |
| 2 | Those three as layer folders, one file per concept inside | The unit needs its own owner, release or reuse by something else |
| 3 | A crate | Deployment demands it: its own process, host or lifecycle |
| 4 | A service | (top of the ladder) |

- **Start at 0.** Climb for a reason the table names, never for tidiness (YAGNI).
- **A unit may step down** when the pressure that moved it up has shrunk: a crate with no second
  owner, release or reuse can fold back to the stage below. This describes a unit's growth stage
  only. It does not say which layers a dependency may skip.
- **The axis is responsibility at every stage.** A folder, crate or service is named for what it
  does, never for the layer it sits in.

## Where Endora stands (descriptive, not an audit)

- The bounded contexts are crates under `domains/`: `platform`, `capabilities`, `team`, `sound`
  and `mail`. [ADR 0026](adr/archive/0026-package-by-bounded-context.md) (archived) made each one a
  crate, and its recorded reason was the compile-time boundary between contexts. That reason is
  historical design context. It is not one of the triggers in the table, and this page does not
  ask any crate to change.
- `shared/` (`kernel`, `persistence`, `protocol`) holds code shared across contexts. The
  orchestration crate `crates/endora-application` and its adapters in `crates/endora-infrastructure`
  sit beside the contexts ([ADR 0050](adr/0050-the-shape-of-the-system.md)). The handheld is the
  `clients/terminal` crate.
- Core, Console and Audio are services (stage 4), each a separate deployable with its own compose
  file. The Core bundle includes copies of Console's and Audio's files ([ADR 0145](adr/0145-core-console-contract.md),
  [ADR 0147](adr/0147-full-system-bundle.md)). Audio's source lives in its own repository.

## Prior art, and how ROCA differs

ROCA combines and names existing practice. It adds no new pattern.

| Source | Shared with ROCA | Where ROCA differs |
| --- | --- | --- |
| Domain-driven design (Evans, 2003) | Bounded contexts and ubiquitous language are the unit of responsibility | DDD says what a context models, not how to package it or when to split it |
| Clean Architecture (Martin, 2012) | Dependencies point inward; use cases sit in the middle | The primary axis is responsibility, not layer |
| Hexagonal, ports and adapters (Cockburn, 2005) | The application owns the ports; adapters implement them outside | Ports sit in each context's application folder, with no separate top-level hexagon |
| Modular monolith (widely used term, no single source) | One deployable, with boundaries inside it | Adds the stage ladder: a boundary becomes a crate or service only for a stated reason |
| Package by component (Simon Brown) | Group code by component, not by layer | Adds inward rules inside a component, and the growth stages |
| YAGNI (Jeffries); rule of three (Roberts, via Fowler) | No layer or abstraction before a reason | Names the specific reasons that trigger each stage |
| Evolutionary architecture (Ford, Parsons, Kua, 2017) | Structure is expected to change, and is checked as it does | Names which reason moves a unit up; its checks are the standards' tests, not new ones |

The claim is narrow: Endora organizes by responsibility, keeps its dependencies inward, and grows a
unit only for a stated reason. The name is Endora's label for that combination ([ADR 0026](adr/archive/0026-package-by-bounded-context.md)).

## Sources

- Eric Evans, *Domain-Driven Design* (Addison-Wesley, 2003).
- Robert C. Martin, [The Clean Architecture](https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html) (blog, 13 Aug 2012).
- Alistair Cockburn, [Hexagonal Architecture](https://alistair.cockburn.us/hexagonal-architecture/) (2005; also called Ports and Adapters).
- Simon Brown, his writing and slides on package by component (*Software Architecture for Developers*).
- Ron Jeffries, *Extreme Programming Installed* (Addison-Wesley, 2001), for YAGNI.
- Don Roberts's rule of three, as credited by Martin Fowler in *Refactoring* (Addison-Wesley, 1999).
- Neal Ford, Rebecca Parsons and Patrick Kua, *Building Evolutionary Architectures* (O'Reilly, 2017).
- "Modular monolith" has no single source; it is used here as a descriptive term.
- [ADR 0026](adr/archive/0026-package-by-bounded-context.md) (archived): where the name and the context crates came from.
- [ADR 0050](adr/0050-the-shape-of-the-system.md): the decision, with its 9 Nov 2026 amendment for current contexts.
