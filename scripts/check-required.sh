#!/bin/sh
# The fail-loud part of the handoff gate. Runs every check in a manifest and exits nonzero
# unless each one ran, printed its success marker and exited 0. It never skips: a check whose
# script is missing is MISSING, one that exits 0 without printing its marker (a stub) is
# NO-MARKER, and a name the caller requires that the manifest does not list stops the run.
#
#   sh scripts/check-required.sh [--root DIR] [--require NAME]... [--list] MANIFEST
#
# MANIFEST has one check a line, `name|marker|command` (blank lines and # comments ignored). The
# command runs from the root (default: this checkout) and must print `marker` when it passes.
# scripts/gate.sh calls this with the names it will not run without (--require), so deleting a
# line from scripts/required-checks.txt fails the gate instead of shrinking it.
# scripts/test-check-required.sh holds this script to all of that, negative controls included.
set -u

root=$(cd "$(dirname "$0")/.." && pwd)
list=0
required=""
manifest=""
while [ $# -gt 0 ]; do
  case $1 in
    --root) root=$2; shift 2 ;;
    --require) required="$required $2"; shift 2 ;;
    --list) list=1; shift ;;
    *) manifest=$1; shift ;;
  esac
done

if [ -z "$manifest" ] || [ ! -f "$manifest" ]; then
  echo "FAIL no manifest: ${manifest:-(none given)}"
  exit 1
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/check-required.XXXXXX")
trap 'rm -rf "$work"' EXIT

total=0; ran=0; passed=0; failed=0; missing=0; names=""

while IFS='|' read -r name marker cmd; do
  case $name in ''|'#'*) continue ;; esac
  total=$((total + 1))
  names="$names $name"
  if [ -z "$marker" ] || [ -z "$cmd" ]; then
    echo "FAIL $name (manifest line needs name|marker|command)"
    failed=$((failed + 1))
    continue
  fi
  [ "$list" -eq 1 ] && { echo "$name: $cmd"; continue; }

  # shellcheck disable=SC2086 # the command's own words are what we want
  set -- $cmd
  path=$1
  [ "$1" = sh ] && path=${2:-}
  if [ -z "$path" ] || [ ! -f "$root/$path" ]; then
    echo "MISSING $name ($path does not exist under $root)"
    missing=$((missing + 1))
    continue
  fi

  ran=$((ran + 1))
  (cd "$root" && sh -c "$cmd") > "$work/$name.out" 2>&1
  code=$?
  if [ "$code" -ne 0 ]; then
    echo "FAIL $name (exit $code)"
    tail -5 "$work/$name.out" | sed 's/^/     | /'
    failed=$((failed + 1))
    continue
  fi
  if ! grep -qF -- "$marker" "$work/$name.out"; then
    echo "NO-MARKER $name (exit 0, but it never printed: $marker)"
    failed=$((failed + 1))
    continue
  fi
  echo "ok   $name (exit 0, printed: $marker)"
  passed=$((passed + 1))
done < "$manifest"

stop=0
if [ "$total" -eq 0 ]; then
  echo "FAIL the manifest lists no checks"
  stop=1
fi
for r in $required; do
  case " $names " in
    *" $r "*) ;;
    *) echo "FAIL required check $r is not in the manifest"; stop=1 ;;
  esac
done

[ "$list" -eq 1 ] && exit "$stop"

echo "required checks: ran $ran, passed $passed, failed $failed, missing $missing (listed $total)"
[ "$stop" -eq 0 ] && [ "$failed" -eq 0 ] && [ "$missing" -eq 0 ] && [ "$passed" -eq "$total" ] && exit 0
echo "required checks FAILED"
exit 1
