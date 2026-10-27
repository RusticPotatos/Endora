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
