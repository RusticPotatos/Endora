#!/usr/bin/env sh
# Tests for scripts/release.sh: every stage that fails stops the release there, with a nonzero
# exit, and nothing after it runs; the checks come before anything leaves this machine. Fakes
# stand in for git, docker, make, ssh, curl and the gate: they record what they were asked and
# answer as the case says, so nothing is pushed, deployed or reached.
#
#   sh scripts/test-release.sh
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d "${TMPDIR:-/tmp}/release-test.XXXXXX")
trap '[ -n "${KEEP:-}" ] || rm -rf "$WORK"' EXIT

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

mkdir -p "$WORK/bin"
fake() {
  # $1 the command; the rest of stdin is its body after the logging line.
  {
    echo '#!/bin/sh'
    echo "echo \"$1 \$*\" >>\"\$FAKE_LOG\""
    cat
  } >"$WORK/bin/$1"
  chmod +x "$WORK/bin/$1"
}

fake git <<'EOF'
case "$1 ${2:-}" in
  "rev-parse --abbrev-ref") echo remediation-1 ;;
  "rev-parse --short") echo abc1234 ;;
  "status --porcelain") [ "${FAKE_DIRTY:-no}" = yes ] && echo " M app/x.rs"; exit 0 ;;
  "push -q") [ "${FAKE_PUSH_FAILS:-no}" != yes ] ;;
  "worktree add") eval "where=\${$(($# - 1))}"; mkdir -p "$where" ;;
  *) exit 0 ;;
esac
EOF

fake docker <<'EOF'
[ "${1:-}" = "--context" ] && shift 2
case "$1" in
  tag) [ "${FAKE_TAG_FAILS:-no}" != yes ] ;;
  exec)
    case "$*" in
      *"test -s"*hq*) [ "${FAKE_HQ_BACKUP_EMPTY:-no}" != yes ] ;;
      *"test -s"*) [ "${FAKE_NODE_BACKUP_EMPTY:-no}" != yes ] ;;
      *endora-hq*backup*) [ "${FAKE_HQ_BACKUP_FAILS:-no}" != yes ] ;;
      *backup*) [ "${FAKE_NODE_BACKUP_FAILS:-no}" != yes ] ;;
      *) exit 0 ;;
    esac ;;
  *) exit 0 ;;
esac
EOF

fake make <<'EOF'
case "$*" in
  *"-f -"*) if grep -q ENDORA_PI_SSH; then echo pi.example; else echo https://nas.example; fi ;;
  *personal-check*) [ "${FAKE_PERSONAL_FAILS:-no}" != yes ] ;;
  *deploy-check*) echo "drain: 0 cut"; [ "${FAKE_DEPLOY_FAILS:-no}" != yes ] ;;
  *deploy-pi*) echo "drain: 0 cut"; [ "${FAKE_PI_DEPLOY_FAILS:-no}" != yes ] ;;
  *) exit 0 ;;
esac
EOF

fake ssh <<'EOF'
case "$*" in
  *printenv*) echo "${FAKE_PI_BUILD:-abc1234}" ;;
  *"docker tag"*) [ "${FAKE_PI_TAG_FAILS:-no}" != yes ] ;;
  *) exit 0 ;;
esac
EOF

fake curl <<'EOF'
[ "${FAKE_HEALTH_FAILS:-no}" = yes ] && exit 22
echo "{\"status\":\"ok\",\"build\":\"${FAKE_NAS_BUILD:-abc1234}\"}"
EOF

# A checkout for the release to run from: the script under test, local.mk, a gate that passes
# unless the case says it fails.
REPO="$WORK/repo"
mkdir -p "$REPO/scripts"
cp "$HERE/release.sh" "$REPO/scripts/release.sh"
echo "ENDORA_PI_SSH = pi.example" >"$REPO/local.mk"
cat >"$REPO/scripts/gate.sh" <<'EOF'
#!/bin/sh
echo "gate" >>"$FAKE_LOG"
[ "${FAKE_GATE_FAILS:-no}" != yes ]
EOF

# Runs the release with the fakes first on PATH and the case's settings; sets STATUS and LOG.
release() {
  rm -rf "$WORK/Endora-release-abc1234" "$REPO/.local"
  LOG="$WORK/calls.log"
  : >"$LOG"
  env FAKE_LOG="$LOG" PATH="$WORK/bin:$PATH" "$@" zsh "$REPO/scripts/release.sh" >"$WORK/out.log" 2>&1
  STATUS=$?
}
ran() { grep -q "$1" "$LOG"; }
not_ran() { ! grep -q "$1" "$LOG"; }

echo "A release that goes well"
release
expect "exits 0" "$([ "$STATUS" -eq 0 ]; echo $?)"
expect "the gate and the personal check run before the push" \
  "$(awk '/^gate/{g=NR} /personal-check/{p=NR} /^git push/{u=NR} END{exit !(g && p && u && g<u && p<u)}' "$LOG"; echo $?)"
expect "both backups are made and checked" "$(ran 'exec endora-node endora-node backup' \
  && ran 'exec endora-node test -s' && ran 'exec endora-hq sh -c .*backup' && ran 'exec endora-hq test -s'; echo $?)"
expect "the Pi is deployed" "$(ran 'deploy-pi'; echo $?)"
expect "it says it released abc1234" "$(grep -q 'released abc1234' "$WORK/out.log"; echo $?)"

stops_before() {
  # $1 what, $2 the call that must not have happened
  expect "$1: exits nonzero" "$([ "$STATUS" -ne 0 ]; echo $?)"
  expect "$1: nothing from '$2' on" "$(not_ran "$2"; echo $?)"
}

echo "A tree with uncommitted changes"
release FAKE_DIRTY=yes
stops_before "dirty tree" "git push"

echo "The gate fails"
release FAKE_GATE_FAILS=yes
stops_before "gate" "git push"

echo "The personal-values check fails"
release FAKE_PERSONAL_FAILS=yes
stops_before "personal check" "git push"

echo "The push fails"
release FAKE_PUSH_FAILS=yes
stops_before "push" "docker --context nas tag"

echo "The rollback tag on the NAS fails"
release FAKE_TAG_FAILS=yes
stops_before "NAS rollback tag" "backup"

echo "The node's backup fails"
release FAKE_NODE_BACKUP_FAILS=yes
stops_before "node backup" "deploy-check"

echo "The node's backup is empty"
release FAKE_NODE_BACKUP_EMPTY=yes
stops_before "empty node backup" "deploy-check"

echo "HQ's backup fails"
release FAKE_HQ_BACKUP_FAILS=yes
stops_before "HQ backup" "deploy-check"

echo "HQ's backup is empty"
release FAKE_HQ_BACKUP_EMPTY=yes
stops_before "empty HQ backup" "deploy-check"

echo "The NAS deploy or its smoke fails"
release FAKE_DEPLOY_FAILS=yes
stops_before "NAS deploy" "ssh"

echo "The rollback tags on the Pi fail"
release FAKE_PI_TAG_FAILS=yes
stops_before "Pi rollback tags" "deploy-pi"

echo "The Pi deploy fails"
release FAKE_PI_DEPLOY_FAILS=yes
stops_before "Pi deploy" "curl"

echo "The NAS does not answer its health check"
release FAKE_HEALTH_FAILS=yes
stops_before "NAS health" "printenv"

echo "The NAS reports another build"
release FAKE_NAS_BUILD=fff0000
stops_before "NAS build" "printenv"

echo "The Pi's runner reports another build"
release FAKE_PI_BUILD=fff0000
expect "Pi build: exits nonzero" "$([ "$STATUS" -ne 0 ]; echo $?)"
expect "Pi build: it does not say it released" "$(! grep -q 'released abc1234' "$WORK/out.log"; echo $?)"

if [ "$FAILED" -ne 0 ]; then
  echo "release tests FAILED"
  exit 1
fi
echo "release tests passed"
