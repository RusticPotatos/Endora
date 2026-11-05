#!/usr/bin/env sh
# Tests for the real `make deploy` (built elsewhere, as a laptop builds for the NAS) and `make
# deploy-pi` recipes, with scripts/hq-drain.sh as it is: a release's strict drain (DRAIN_STRICT=1)
# that did not happen stops the recipe before any restart, and a failure inside an image's
# `save | gzip | load` pipe stops it too. Fakes stand in for docker, ssh, gzip and git; they record
# what they were asked, so nothing is built, copied or started. The Makefile runs from a copy
# with no local.mk, so the owner's settings are never read.
#
#   sh scripts/test-deploy-recipes.sh
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d "${TMPDIR:-/tmp}/deploy-recipes-test.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

FAILED=0
expect() {
  if [ "$2" -eq 0 ]; then
    echo "  ok    $1"
  else
    echo "  FAIL  $1"
    FAILED=1
  fi
}

mkdir -p "$WORK/bin" "$WORK/repo/scripts"
cp "$HERE/../Makefile" "$WORK/repo/Makefile"
cp "$HERE/hq-drain.sh" "$WORK/repo/scripts/hq-drain.sh"
cp "$HERE/data-dir.sh" "$WORK/repo/scripts/data-dir.sh"
cp "$HERE/pi-runner.sh" "$WORK/repo/scripts/pi-runner.sh"

cat >"$WORK/bin/docker" <<'EOF'
#!/bin/sh
echo "docker $*" >>"$FAKE_LOG"
while [ "${1:-}" = "--context" ]; do shift 2; done
case "$1" in
  version) echo amd64 ;;
  save) [ "${FAKE_SAVE_FAILS:-no}" != yes ] || exit 1; echo image-bytes ;;
  load) cat >/dev/null ;;
  ps) echo endora-node; echo endora-hq ;;
  exec)
    case "$*" in
      *"grep -qaF"*) exit 0 ;;
      *"endora-hq drain"*)
        [ "${FAKE_DRAIN_FAILS:-no}" != yes ] || exit 1
        if [ "${FAKE_CAP_EXPIRES:-no}" = yes ]; then
          echo "drain: the cap of 300 s passed with 1 turn still running: the restart cuts them, and each is tried again"
        else
          echo "drain: no turn is running: the restart can go ahead"
        fi ;;
      *) exit 0 ;;
    esac ;;
  *) exit 0 ;;
esac
EOF
cat >"$WORK/bin/ssh" <<'EOF'
#!/bin/sh
echo "ssh $*" >>"$FAKE_LOG"
case "$*" in
  *"docker load"*) cat >/dev/null; [ "${FAKE_PI_LOAD_FAILS:-no}" != yes ] ;;
  *"sh -s"*) cat >/dev/null; echo "runner started" >>"$FAKE_LOG" ;;
  *) exit 0 ;;
esac
EOF
cat >"$WORK/bin/gzip" <<'EOF'
#!/bin/sh
cat
EOF
cat >"$WORK/bin/git" <<'EOF'
#!/bin/sh
echo abc1234
EOF
chmod +x "$WORK/bin/"*

# Runs `make $1` in the copy, with the fakes first on PATH, the NAS and Pi settings a laptop has,
# and the case's extra settings. Leaves the exit status in STATUS and the calls in LOG.
recipe() {
  target=$1
  shift
  LOG="$WORK/calls.log"
  : >"$LOG"
  env FAKE_LOG="$LOG" PATH="$WORK/bin:$PATH" "$@" make -s -C "$WORK/repo" "$target" \
    DOCKER_CONTEXT=nas ENDORA_BUILD_CONTEXT=builder ENDORA_PI_SSH=pi.example \
    ENDORA_NAS_ADDR=192.168.1.10 COMPOSE_PROFILES= ENDORA_DATA_DIR= DRAIN_RETRY_SLEEP=0 \
    >"$WORK/out.log" 2>&1
  STATUS=$?
}
ran() { grep -q -- "$1" "$LOG"; }

echo "make deploy, built elsewhere"

recipe deploy DRAIN_STRICT=1
expect "a strict drain that drained restarts the node" "$([ "$STATUS" -eq 0 ] && ran 'compose up -d --no-build'; echo $?)"

recipe deploy DRAIN_STRICT=1 FAKE_DRAIN_FAILS=yes
expect "a strict drain HQ refused: nonzero" "$([ "$STATUS" -ne 0 ]; echo $?)"
expect "  and nothing is restarted" "$(! ran 'compose up'; echo $?)"
expect "  and the announcement is lifted" "$(ran 'endora-hq resume'; echo $?)"

recipe deploy DRAIN_STRICT=1 FAKE_CAP_EXPIRES=yes
expect "a strict drain whose cap passed with turns running: nonzero" "$([ "$STATUS" -ne 0 ]; echo $?)"
expect "  and nothing is restarted" "$(! ran 'compose up'; echo $?)"

recipe deploy DRAIN_STRICT=1 NO_DRAIN=1
expect "NO_DRAIN with a strict drain: nonzero, nothing restarted" "$([ "$STATUS" -ne 0 ] && ! ran 'compose up'; echo $?)"

recipe deploy FAKE_DRAIN_FAILS=yes
expect "without DRAIN_STRICT, a drain HQ refused still deploys (ADR 0134)" "$([ "$STATUS" -eq 0 ] && ran 'compose up -d --no-build'; echo $?)"

recipe deploy DRAIN_STRICT=1 FAKE_SAVE_FAILS=yes
expect "an image save that fails inside its pipe: nonzero" "$([ "$STATUS" -ne 0 ]; echo $?)"
expect "  and nothing is drained or restarted" "$(! ran 'endora-hq drain' && ! ran 'compose up'; echo $?)"

echo "make deploy-pi"

recipe deploy-pi DRAIN_STRICT=1
expect "a strict drain that drained starts the runner" "$([ "$STATUS" -eq 0 ] && ran 'runner started'; echo $?)"

recipe deploy-pi DRAIN_STRICT=1 FAKE_DRAIN_FAILS=yes
expect "a strict drain HQ refused: nonzero" "$([ "$STATUS" -ne 0 ]; echo $?)"
expect "  and the runner is not restarted" "$(! ran 'runner started'; echo $?)"

recipe deploy-pi DRAIN_STRICT=1 FAKE_CAP_EXPIRES=yes
expect "a strict drain whose cap passed: the runner is not restarted" "$([ "$STATUS" -ne 0 ] && ! ran 'runner started'; echo $?)"

recipe deploy-pi DRAIN_STRICT=1 FAKE_SAVE_FAILS=yes
expect "an image save that fails inside its pipe: nonzero, runner not restarted" "$([ "$STATUS" -ne 0 ] && ! ran 'runner started'; echo $?)"

recipe deploy-pi DRAIN_STRICT=1 FAKE_PI_LOAD_FAILS=yes
expect "the Pi's load that fails at the end of the pipe: nonzero, runner not restarted" "$([ "$STATUS" -ne 0 ] && ! ran 'runner started'; echo $?)"

if [ "$FAILED" -ne 0 ]; then
  echo "deploy recipe tests FAILED"
  exit 1
fi
echo "deploy recipe tests passed"
