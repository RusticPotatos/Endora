#!/bin/sh
# PreToolUse on Edit|Write|NotebookEdit: the lab's secrets and inventories are the owner's
# (hq-round's standing rules). Refuse any write to group_vars, host_vars, an inventory or a
# vault file, in any repository.
f=$(jq -r '.tool_input.file_path // .tool_input.notebook_path // empty')
[ -n "$f" ] || exit 0
if printf '%s' "$f" | grep -Eq '(^|/)(group_vars|host_vars)/|(^|/)inventor[^/]*$|(^|/)inventories/|vault[^/]*\.ya?ml$'; then
  echo "Refused: $f is the owner's (group_vars, host_vars, inventories and vaults are never edited by an assistant)." >&2
  exit 2
fi
exit 0
