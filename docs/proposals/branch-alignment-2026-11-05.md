# Branch alignment, 5 November 2026

The new architecture is the project's direction (owner, 5 Nov 2026). `develop`, `main` and
`codex/agent-collaboration` were aligned with the released and validated engine. Nothing was
force-pushed and no history was rewritten.

## Before

| Branch | Tip | Its own commits (not merges) | Behind the live build |
|---|---|---|---|
| `main` | `934ffaf0` | 0 | 1,185 |
| `develop` | `82403643` | 0 | 395 |
| `codex/agent-collaboration` | `5b2595d3` | 82 (research records and tooling only) | 37 |
| live build | `ea72ecf0` (branch `remediation-1`) | | |

Merging `main` or `develop` with `ea72ecf0` gave exactly `ea72ecf0`'s tree: they held no content
of their own. `codex/agent-collaboration` added 20 files: the experiment ledgers, audits and
proposals in `docs/proposals/` and the round tooling in `.claude/`. No product code.

## Integration (on `develop`)

1. `559dc1d9`: merge of `ea72ecf0`. The tree equals the live build.
2. `138fb95b`: merge of `codex/agent-collaboration`, its records and tooling.
3. `f9ada8ac`: merge of `apparatus-v6`, with the defect-remediation handoff
   (`defect-remediation-autonomy-handoff-2026-11-05.md`) and harness rig v6.
4. `abd49bcf`: merge of `main`'s history, so all three lines share one history.

Product code at `abd49bcf` is byte-identical to `ea72ecf0`. The diff outside `docs/`, `.claude/`
and `scripts/harness-probes/` is empty, so no release was needed. The live build stays `ea72ecf0`.

## Checks on `abd49bcf`

- `scripts/gate.sh`: fmt, clippy `-D warnings`, 2,267 unit tests and 283 scenarios / 2,565 steps
  with 0 failed, console, nothing-personal, collection parity, diff check.
- `scripts/harness-probes`: 184 tests, OK.
- `scripts/test-release.sh`: release tests passed.
- The 14 commits new to the remote were scanned for personal values: none.

## After

`develop`, `main` and `codex/agent-collaboration` all point at `abd49bcf`, a fast-forward for each.
From here on, `develop` is the integration branch and `main` the validated baseline
(CONTRIBUTING.md). New work branches from `develop`.

## Research apparatus

The harness rig stays in `scripts/harness-probes/` as research tooling. The only product
dependency is the observer program the runner embeds (`sidefx.py`, off by default). Confirmation
seeds and their generated contracts are chosen and made only after the freeze, in each climb's
git-ignored folder (`.local/research-climb/`), and are never committed. The three seeds named in
the handoff are spent seeds from the agentic-harness climb, not hidden ones.
