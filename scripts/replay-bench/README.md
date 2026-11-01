# Replay bench (prototype)

The scripts of 1 Nov that compared the house models qwen3.5:9b and qwen3:8b on Bo's stored turns
(the record's "The house model: qwen3.5:9b against qwen3:8b, replayed"), kept as they ran. Python 3
standard library only. Not yet general: the member, the era split (`NINE_B_FROM_MS`) and the
sample (15 points an era) are set in `build.py`.

**Nothing in a bench folder is committed.** Transcripts, points and replies are the house's: keep
the bench folder outside this repository (for example `~/bench`).

1. **Transcripts**, read-only from HQ's store, into `<bench>/transcripts.json`: a JSON list of
   `{run, task, brain, at, cut, body}` from the `transcripts` table, for the member's turns.
2. **Tool schemas**, into `<bench>/hq.json` (HQ's `tools/list` answer for a turn),
   `<bench>/node.json` (the node's `GET /team-relay/contracts` for a turn) and
   `<bench>/worker.json` (the worker's `shell` and `find_tools`). On 1 Nov they were dumped by
   three throwaway tests in a detached worktree, each calling its crate's own test helpers and
   writing the JSON; the worktree was then removed.
3. **Points**: `python3 -I build.py <bench>` writes `<bench>/points.jsonl`.
4. **Replay** each model: `BENCH_URL=http://<ollama-host>:11434/v1 python3 -I run.py <bench> <model>`
   writes `<bench>/out-<model>.jsonl` (resumes where it stopped).
5. **Score**: `python3 -I score.py <bench> <model> <model>` prints the mechanical table (tool call
   made, tool offered, arguments fit the schema, Endora tool via the shell, same tool as Bo,
   medians of seconds and tokens). The blind A/B file for the judge (`judge.md`, its key apart)
   was written by hand from the two outputs on 1 Nov.
