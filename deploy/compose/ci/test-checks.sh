#!/bin/sh
# The bundle test's own helpers, tested (ci/checks.sh). `eventually` must ask again on every
# try: run 42427566267 failed because its argument, a command substitution, was expanded once,
# before the first try, so it re-tested a stale answer for 28 seconds.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
passed="$work/passed"; : > "$passed"
failures=0
EVENTUALLY_TRIES=5 EVENTUALLY_PAUSE=0
[ -f "$here/checks.sh" ] || { echo "FAIL no ci/checks.sh"; exit 1; }
. "$here/checks.sh"

# A state that becomes true on the third look, as Console's view of Core does after its cache.
echo 0 > "$work/looks"
third_look() { n=$(($(cat "$work/looks") + 1)); echo "$n" > "$work/looks"; [ "$n" -ge 3 ]; }
eventually "becomes true on the third look" third_look >/dev/null
grep -qxF "becomes true on the third look" "$passed" || { echo "FAIL eventually did not ask again"; exit 1; }
[ "$failures" -eq 0 ] || { echo "FAIL eventually counted a failure for a check that passed"; exit 1; }

never() { false; }
eventually "never true" never >/dev/null
[ "$failures" -eq 1 ] || { echo "FAIL eventually passed a check that never held"; exit 1; }
grep -qxF "never true" "$passed" && { echo "FAIL a failed check was recorded as passed"; exit 1; }
# The call that failed: a command and its already-expanded arguments. It must be refused, not
# retried against a stale answer.
before=$failures
eventually "an expanded answer" test "same" = "same" >/dev/null 2>&1 || true
[ "$failures" -eq $((before + 1)) ] || { echo "FAIL eventually accepted an already-expanded command"; exit 1; }
# And the bundle test calls it with function names only.
if grep -nE '^ *eventually ' "$here/../test-bundle.sh" | grep -vE '^[0-9]+: *eventually "[^"]+" [a-z_]+$'; then
  echo "FAIL test-bundle.sh calls eventually with something other than one function name"; exit 1
fi
echo "ok   eventually asks again on every try, fails a check that never holds, and takes only a function"
