# The bundle test's helpers (test-bundle.sh sources this; ci/test-checks.sh tests it).
# The caller sets $passed (a file) and failures=0.

pass() { printf 'ok   %s\n' "$1"; printf '%s\n' "$1" >> "$passed"; }
fail() { printf 'FAIL %s\n' "$1"; failures=$((failures + 1)); }

# check NAME COMMAND...: one named check, its command's success is the verdict.
check() { name=$1; shift; if "$@"; then pass "$name"; else fail "$name"; fi; }

# eventually NAME FUNCTION: as check, for a state another product caches (Console re-checks
# Core at most every 10 seconds), so FUNCTION is asked again on each try, for up to
# EVENTUALLY_TRIES tries EVENTUALLY_PAUSE seconds apart (15 and 2: 30 seconds).
#
# Only a function name: a command with arguments would carry them already expanded, so every
# try would re-test the first answer. That is how run 42427566267 failed on develop.
eventually() {
  name=$1
  if [ "$#" -ne 2 ]; then
    fail "$name (eventually takes one function name, which it asks again on every try)"
    return 0
  fi
  tries=0
  until "$2"; do
    tries=$((tries + 1))
    [ "$tries" -lt "${EVENTUALLY_TRIES:-15}" ] || { fail "$name"; return 0; }
    sleep "${EVENTUALLY_PAUSE:-2}"
  done
  pass "$name"
}
