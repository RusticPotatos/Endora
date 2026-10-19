#!/usr/bin/env sh
# Tests for scripts/hq-drain.sh, what `make deploy` and `make deploy-pi` run before they restart
# anything and after (ADR 0134). A fake `docker` stands in for the real one: it records what it
# was asked and answers as the case says, so no daemon is needed.
#
#   sh scripts/test-hq-drain.sh
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
SCRIPT="$HERE/hq-drain.sh"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/hq-drain-test.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

FAILED=0
expect() {
  # $1 what, $2 the status of the check (0 is a pass)
  if [ "$2" -eq 0 ]; then
    echo "  ok    $1"
  else
    echo "  FAIL  $1"
    FAILED=1
  fi
}

# The fake docker. It skips `--context NAME`, logs every call to $FAKE_LOG, and answers:
#   ps                       the containers $FAKE_UP names (one per line), or none
#   exec C grep ...          succeeds when FAKE_CAN_DRAIN=yes
#   exec C endora-hq drain   succeeds unless FAKE_DRAIN_FAILS=yes
#   exec C endora-hq resume  fails the first $FAKE_RESUME_FAILS times, then succeeds
mkdir -p "$WORK/bin"
cat >"$WORK/bin/docker" <<'FAKE'
#!/bin/sh
echo "docker $*" >>"$FAKE_LOG"
if [ "${1:-}" = "--context" ]; then shift 2; fi
case "${1:-}" in
  ps)
    for name in $FAKE_UP; do echo "$name"; done
    exit 0
    ;;
  exec)
    shift 2
    case "$1" in
      grep) [ "$FAKE_CAN_DRAIN" = yes ]; exit $? ;;
      endora-hq)
        case "$2" in
          drain) [ "${FAKE_DRAIN_FAILS:-no}" != yes ]; exit $? ;;
          resume)
            n=$(cat "$FAKE_COUNT" 2>/dev/null || echo 0)
            echo $((n + 1)) >"$FAKE_COUNT"
            [ "$n" -ge "${FAKE_RESUME_FAILS:-0}" ]; exit $?
            ;;
        esac
        ;;
    esac
    ;;
esac
exit 0
FAKE
chmod +x "$WORK/bin/docker"

CASE=0
fresh() {
  CASE=$((CASE + 1))
  T="$WORK/case-$CASE"
  mkdir -p "$T"
  : >"$T/log"
  rm -f "$T/count"
}

# Runs `hq-drain.sh $1` with the fake docker, and any extra NAME=value settings given after.
# Leaves its output in $OUT, its exit status in $STATUS, and the docker calls in $T/log.
hq() {
  sub="$1"
  shift
  OUT=$(env PATH="$WORK/bin:$PATH" FAKE_LOG="$T/log" FAKE_COUNT="$T/count" \
    FAKE_UP="endora-node endora-hq" FAKE_CAN_DRAIN=yes DRAIN_RETRY_SLEEP=0 \
    "$@" sh "$SCRIPT" "$sub" 2>&1)
  STATUS=$?
}

called() { grep -qF -- "$1" "$T/log"; }
never_exec() { ! grep -q 'docker.* exec ' "$T/log"; }
says() { printf '%s' "$OUT" | grep -qF -- "$1"; }

echo "draining before a deploy"

fresh
hq drain
expect "asks the running HQ to drain and wait, for five minutes" "$(called 'exec endora-hq endora-hq drain --cap 300 --wait'; echo $?)"
expect "  and exits 0" "$([ $STATUS -eq 0 ]; echo $?)"

fresh
hq drain DRAIN_CAP=120
expect "takes the cap it is given" "$(called 'endora-hq drain --cap 120 --wait'; echo $?)"

fresh
hq drain DOCKER="docker --context nas"
expect "runs where the deploy runs: through the context it is given" "$(called 'docker --context nas exec endora-hq endora-hq drain'; echo $?)"

fresh
hq drain NO_DRAIN=1
expect "does nothing when NO_DRAIN is set" "$(never_exec; echo $?)"
expect "  and says so" "$(says 'NO_DRAIN'; echo $?)"
expect "  and exits 0" "$([ $STATUS -eq 0 ]; echo $?)"

fresh
hq drain FAKE_UP="endora-node"
expect "does nothing when HQ is not running here" "$(never_exec; echo $?)"
expect "  and says so" "$(says 'HQ is not running'; echo $?)"
expect "  and exits 0" "$([ $STATUS -eq 0 ]; echo $?)"

fresh
hq drain FAKE_CAN_DRAIN=no
expect "never runs the command on an HQ that predates it: that would start a second HQ" "$(! called 'endora-hq drain'; echo $?)"
expect "  and says so" "$(says 'predates'; echo $?)"
expect "  and exits 0" "$([ $STATUS -eq 0 ]; echo $?)"

fresh
hq drain FAKE_DRAIN_FAILS=yes
expect "an HQ that cannot be drained does not stop the deploy" "$([ $STATUS -eq 0 ]; echo $?)"
expect "  and says it deploys anyway" "$(says 'deploying anyway'; echo $?)"

echo "lifting the drain after a deploy"

fresh
hq resume
expect "asks HQ to resume" "$(called 'exec endora-hq endora-hq resume'; echo $?)"
expect "  and exits 0" "$([ $STATUS -eq 0 ]; echo $?)"

fresh
hq resume FAKE_RESUME_FAILS=2
expect "tries again while HQ is starting" "$([ "$(cat "$T/count")" = 3 ]; echo $?)"
expect "  and exits 0" "$([ $STATUS -eq 0 ]; echo $?)"

fresh
hq resume FAKE_RESUME_FAILS=99
expect "gives up after a while, leaving it to HQ's own start and its half hour" "$([ "$(cat "$T/count")" = 10 ]; echo $?)"
expect "  says so" "$(says 'lifts it itself'; echo $?)"
expect "  and still exits 0" "$([ $STATUS -eq 0 ]; echo $?)"

fresh
hq resume NO_DRAIN=1
expect "does nothing when NO_DRAIN is set" "$(never_exec; echo $?)"

fresh
hq resume FAKE_UP="endora-node"
expect "does not wait for an HQ that is not there" "$(never_exec; echo $?)"

echo "usage"

fresh
hq nonsense
expect "an unknown word is refused" "$([ $STATUS -eq 2 ]; echo $?)"

exit "$FAILED"
