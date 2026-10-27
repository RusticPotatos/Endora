# Tidying the team code: reads, structure and repetition

2026-10-27. The owner asked what else could be optimised, reused or kept DRY, then: "make a
plan and loop it all". This plan carries that out on `codex/agent-collaboration`, one step at
a time. The office chess board stays (the owner, 27 Oct).

## Baseline (f63b7790)

| Measure | Value |
|---|---|
| Team Rust, all of it | 61,584 lines |
| Of which tests (test files 19,701, test modules about 8,580) | about 28,280 |
| Code without tests | about 33,300 |
| `///` doc lines in code without tests | 5,149 |
| Whole-table reads of turns (`.runs()?`) / of tasks (`.tasks()?`) | 39 / 26 |
| Queries per read of every task (one per task, for what it waits on) | 1 + 872 |
| `TeamStore` methods / functions in `engine.rs` / lines in `engine.rs` | 117 / 153 / 4,374 |
| Hand-written JSON settings parses / copies of the event row mapping | 7 / 3 |
| HQ snapshot (`GET /v1/hq`), five calls in the browser | 941–1,588 ms |

HQ holds 872 tasks, 3,304 turns and 27,678 events today; every whole-table read grows with them.

## How each step runs

Each step that changes behaviour is test-first; a step that only moves or merges code is held
by the tests already there. Then:
- the full gate in `~/repos/Endora-gate`;
- release only with no turn running;
- a production check against the step's exit check;
- a line under Progress here, and in memory.

A failed step stops and rolls back to the previous release. The loop stops when step 6 is recorded.

## Steps

1. **Store reads that ask for what they need.** Add store queries for the live turns and for
   one task's turns, and read every task's waits in one query instead of one per task. Replace
   the whole-table reads that filter for those.
   *Exit:* at most 15 `.runs()?` and 12 `.tasks()?` calls left; the HQ snapshot is faster than
   the baseline on the same five calls; ticks go on (a turn starts and ends on production).
2. **One event row mapping.** The three copies in `sqlite.rs` become one `event_from`.
   *Exit:* one copy; gate green.
3. **One JSON settings helper.** A typed `setting_json` on the engine replaces the seven
   hand-written parses, the league's two among them.
   *Exit:* no `from_str(&s).ok()` left outside the helper; gate green.
4. **Turn bookkeeping out of `engine.rs`, and a smaller store interface.** Move the turn
   functions (queue, retry, bring forward, progress, empty turns, ended turns) to
   `engine/runs.rs`. Split the run methods off `TeamStore` into a `RunStore` supertrait, the
   way `GuardStore`, `MapStore`, `HistoryStore` and `WorldStore` already are.
   *Exit:* `engine.rs` under 3,800 lines; `TeamStore`'s own methods under 100; gate green.
5. **Doc comments say what and why, not who and when.** Cut dated attributions ("the owner,
   2026-10-25"), the history of earlier designs and restatements of the code: git and the
   ADRs keep those. Keep every comment that says why, a unit, an invariant or a safety rule.
   *Exit:* at least 1,000 fewer `///` lines; no behaviour changed (gate green, same tests).
6. **Record.** The baseline table measured again, and a line in memory.

## Progress

- **Step 1, done (8f6f4186, 61b6ccce; live).** The cost was the snapshot, not the reads: it sent
  every done task's thread, evidence and documents (1.1 MB of 1.6). Finished tasks now come in
  brief (no thread, evidence, documents, description or chain of why), and opening one asks
  `GET /v1/hq/tasks/{id}`; one `task_row` builds both. The store gained `live_runs` (a partial
  index) and `runs_of(task)`: whole-turn reads 39 → 14. Every task's waits are one query, not
  872. The `.tasks()?` target (12) was dropped: each read is now two queries, so the count no
  longer costs anything. Production: snapshot 1.6 MB → 441 KB, 941–1,588 → 684–1,317 ms; a done
  task opens with its whole thread.
- **Steps 2 and 3, done (c819a821, 13e55790; live, released with step 1's second commit).**
  One `event_from`; one typed `setting_json` for the seven hand-written parses (`serde` named
  directly for its bound; `serde_json` already built it). Events and the league read on production.
- **Step 4, done (b2c36b59).** `RunStore` (turns, tokens, the ledger) and `OfficeStore` (the
  room's items, chess) split off `TeamStore`: its own methods 119 → 93. engine.rs's wakes and
  turns sections moved to `engine/waking.rs` and `engine/running.rs`: 4,401 → 3,763 lines.
- **Step 5, done short of its exit.** The premise was wrong: dated "who and when" notes are rare
  (8 doc lines, 6 others), and most of them carry a reason or an incident, so they stay. The
  boilerplate was 171 `# Errors` sections saying only that the store can fail, which the return
  type says: cut, 509 lines. Not the 1,000 the exit asked; nothing else met the rule.
