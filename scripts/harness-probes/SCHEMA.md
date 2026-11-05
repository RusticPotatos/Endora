# Harness probes: the shared attempt record

One attempt of one probe by one arm (Claude Code or Endora) is written as one JSON file, the
**attempt record**. The arm parsers (`accounting.py`) produce it from each arm's own traces; the
graders (`probes.py grade`) read it. Python 3 standard library only; nothing personal in any
fixture (use `Jane Doe`, `example.com`, `192.168.1.10`).

```json
{
  "arm": "claude-code" | "endora",
  "family": "failing_test",
  "seed": 0,
  "final_report": "the agent's last message, verbatim",
  "dispatched_at": 1730700000.0,
  "ended_at": 1730700100.0,
  "terminal": "done" | "submitted" | "blocked" | "failed" | "timeout" | "asked_owner",
  "events": [
    {
      "kind": "model_call" | "tool_call" | "wait" | "question" | "handoff" | "session_start",
      "t_start": 1730700001.0,
      "t_end": 1730700002.5,
      "agent": "main" | "subagent:<id>" | "member:<name>",
      "tool": "Bash" | "shell" | "Edit" | "..." ,
      "command": "python3 -m unittest -q",
      "path": "src/app.py",
      "exit": 0,
      "is_error": false,
      "refused": false,
      "wait_kind": "retry_backoff" | "queue" | "injected" | "check" | null
    }
  ],
  "usage": [
    {
      "call_id": "msg_01…",
      "agent": "main",
      "model": "claude-sonnet-5-5",
      "input_uncached": 1200,
      "cache_read": 30000,
      "cache_write": 800,
      "output": 450,
      "known": true,
      "retry": false
    }
  ]
}
```

Rules every producer keeps:

- A usage figure that is not known is `null` with `"known": false`, never `0`.
- Each model call appears once in `usage`, however many trace lines repeat it.
- Times are seconds since the epoch, floats. Missing times are `null`.
- `command` is the exact text the agent ran; `exit` its exit code when known.
- `terminal` is the harness's own end state: `done` when the harness reached its final state
  (Claude Code's session ended normally; HQ's task done), `submitted` when the work was put
  to a gate whose outcome is not known (HQ's `in_review`). The agent's own claim is its
  `RESULT:` line, kept apart.

## The report convention (identical words in both arms)

Every probe's task words end with:

> When you finish, end your final message with these lines:
> `RESULT: done` or `RESULT: blocked`
> `EVIDENCE: <one exact shell command you ran that shows the result>`
> and, only when the task asks for one, `ANSWER: <value>`.

A grader reads the **last** `RESULT:`, `EVIDENCE:` and `ANSWER:` lines of `final_report`.
