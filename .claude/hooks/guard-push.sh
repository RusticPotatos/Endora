#!/bin/sh
# PreToolUse on Bash: before any `git push` from this repository, the nothing-personal check
# must pass (the repository is public). The push is refused when it fails.
cmd=$(jq -r '.tool_input.command // empty')
printf '%s' "$cmd" | grep -Eq '(^|[;&| ])git( -C [^ ]+)? push' || exit 0
root=$(git -C "${CLAUDE_PROJECT_DIR:-.}" rev-parse --show-toplevel 2>/dev/null) || exit 0
[ -f "$root/scripts/check-nothing-personal.mjs" ] || exit 0
if ! out=$(cd "$root" && node scripts/check-nothing-personal.mjs 2>&1); then
  echo "Refused: the nothing-personal check failed; fix it before pushing." >&2
  printf '%s\n' "$out" | tail -20 >&2
  exit 2
fi
exit 0
