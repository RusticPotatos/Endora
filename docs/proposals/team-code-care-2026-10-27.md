# Care for the team code: long functions, nesting, the store's surface, panics

2026-10-27. After the tidy ([team-code-tidy-2026-10-27.md](team-code-tidy-2026-10-27.md)) the owner
asked about the other Rust and SOLID principles, "happy path on root, that kind of TLC", then:
"make a plan and loop this". This plan carries it out on `codex/agent-collaboration`, one step at
a time. Behaviour does not change in any step.

## Baseline (8aa96645), the team crate without tests

| Measure | Value |
|---|---|
| `else` after an early `return`/`continue`/`break` | 0 (the happy path already holds) |
| `unwrap` / `panic!` / `expect` outside tests | 0 / 0 / 1 |
| Functions over 100 lines (`clippy::too_many_lines`) | 11 |
| Blocks nested deeper than 4 (`clippy::excessive_nesting`) | 48 |
| `TeamStore`'s own methods | 93 |
| Slice indexing that can panic (`clippy::indexing_slicing`) | 156 |
| Lints beyond Clippy's defaults enforced by the gate | none |

The longest functions: `call_tool` (284 lines), `tick` (215), `start_queued` (149),
`briefing_body` (140), `finish_check` (122), `house_pieces` (116).

Left out on purpose: the `# Errors` sections pedantic Clippy asks for (cut in the tidy: the
return type says it), lock-scope tightening (one connection; little to gain), and splitting the
engine into services, an architecture change that would need its own ADR.

## How each step runs

As the tidy: the tests already there hold a step that only moves code; a step that adds a store
query is test-first. Then the full gate in `~/repos/Endora-gate`, a release only with no turn
running, a production check (health, a snapshot, a tick), and a line under Progress and in memory.
A failed step stops and rolls back to the previous release. The loop stops when step 6 is
recorded.

## Steps

1. **Long functions become named steps.** Split the six longest into helpers that each say one
   thing, keeping each call site's order. A `Display` that is one table of messages may stay
   long. *Exit:* at most 3 functions over 100 lines, each a table, not logic.
2. **Flatter blocks.** `let … else`, an early `continue` or a small helper for the blocks nested
   deeper than 4. *Exit:* at most 10 left, none deeper than 5.
3. **A smaller store interface.** Split more of `TeamStore` into supertraits the way `RunStore`
   and `OfficeStore` are: the event feed, the desk, and whatever else groups by table.
   *Exit:* `TeamStore`'s own methods under 60.
4. **Index access that cannot panic.** Read each slice index; one not bounded by a check in the
   same function becomes `.get()` with the case handled. *Exit:* under 50 left, each bounded
   where it stands.
5. **The gate keeps it.** Workspace lints for what steps 1–4 reached (`too_many_lines`,
   `excessive_nesting` at 4, `indexing_slicing` where clean, and the pedantic lints the code
   already meets), each exception allowed in place with its reason. *Exit:* the gate fails on a
   regression, shown by a deliberate one.
6. **Record.** The baseline measured again, and a line in memory.

## Progress

- **Step 1, done (9246bd0d, live).** The six longest and four more became named steps: `tick`'s
  numbered steps are methods, `call_tool` a table with `refusal` before it, `start_queued` hands
  a `Placement` to `launch`. Over 100 lines: 11 → 1, `Refusal`'s Display, a table of 63 messages.
- **Step 2, done (f5c49793, live).** Loop bodies that were chains of `continue` became methods with
  early returns (`settle_one`, `stalled_one`, …); `section_hop` returns a `ControlFlow`. Deeper
  than 4: 48 → 8; deeper than 5: none. The league reset reads the tasks once, not once a game.
- **Step 3, done (4143952b, live).** `EventStore`, `DeskStore`, `ScheduleStore`, `WingStore` and
  `ReceiptStore` split off, each with its SQLite file: `TeamStore`'s own methods 96 → 55.
- **Step 4, done (b063b91c, live with step 5).** Slice and vector indexes Clippy says may panic:
  81 → 37, each bounded where it stands; the record's rows are fixed-width arrays (`run_of::<N>`).
  Clippy's other 75 hits there are JSON lookups, which give `null`, not a panic.
- **Step 5, done (23dc9bbd, live).** The team crate warns on `clippy::pedantic` and
  `excessive_nesting`; its `clippy.toml` sets 100 lines and depth 5; what pedantic asks and this
  code does not do is allowed by name at the root, each with its reason. A deliberate 101-line
  function and a six-deep block failed the gate. `indexing_slicing` stays off (the JSON lookups).
- **Step 6, recorded.** The baseline measured again:

  | Measure | Before | After |
  |---|---|---|
  | `else` after an early exit / `unwrap`, `panic!` outside tests | 0 / 0 | 0 / 0 |
  | Functions over 100 lines | 11 | 1 (a table) |
  | Blocks deeper than 4 / deeper than 5 | 48 / 7 | 8 / 0 |
  | `TeamStore`'s own methods | 96 | 55 |
  | Slice indexing that can panic, unbounded where it stands | 81 | 0 (37 bounded) |
  | Lints beyond Clippy's defaults the gate enforces | none | pedantic, nesting, length |
  | Team Rust, all of it | 61,313 | 62,046 |

  The lines grew by 733: each new step, store trait and helper has its signature and doc. Steps 4
  and 5 went out in one release; every step was gated alone. Production after each: health, a
  snapshot (286–406 ms) and ticks going on; no turn was cut by any drain.
