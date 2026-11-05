#!/bin/zsh
# Release this checkout's HEAD to the NAS and the Pi: the gate and the personal-values check, then
# push the branch, a release worktree, rollback tags, snapshots of both databases (each checked to
# exist and not be empty), the drain (no turn is cut), deploy, smoke, and both hosts reporting
# this build. Any stage that fails stops the release there and exits nonzero: nothing after it
# runs. Check first that no turn is running (`runs.status = 'running'` in HQ). Addresses come from
# local.mk. Tests: scripts/test-release.sh.
setopt err_exit pipe_fail no_unset
stop() { echo "release stopped: $*" >&2; exit 1; }
cd "$(dirname "$0")/.." || stop "no checkout"
ROOT=$PWD
BRANCH=$(git rev-parse --abbrev-ref HEAD)
[ -f local.mk ] || stop "local.mk is missing: it holds the addresses"
[ -z "$(git status --porcelain)" ] || stop "the tree has uncommitted changes: commit or set them aside"
[ -z "${NO_DRAIN:-}" ] || stop "NO_DRAIN is set: a release always drains HQ before it restarts anything"
# Housekeeping only: a prune that fails stops nothing.
if [ -f scripts/prune-builds.sh ]; then
  PRUNE_BUILDS_KEEP="Endora-gate $(basename "$ROOT")" sh scripts/prune-builds.sh 15 "$(dirname "$ROOT")" | tail -1 || true
fi
# The checks come before anything leaves this machine: the full gate, then the personal values.
sh scripts/gate.sh || stop "the gate failed"
make -s personal-check || stop "the personal-values check failed"
git push -q origin "$BRANCH" || stop "the push failed"
H=$(git rev-parse --short HEAD); echo $H
REL="$(dirname "$ROOT")/Endora-release-$H"
git worktree add -q --detach "$REL" $H || stop "the release worktree could not be made"
cp local.mk "$REL/"
cd "$REL"
L=$ROOT/.local/release-$H; mkdir -p $L
docker --context nas tag endora-node:latest endora-node:before-$H || stop "the NAS rollback tag failed"
echo tagged
TS=$(date -u +%Y%m%dT%H%M%SZ)
NODE_DB=/data/backups/before-$H-node-$TS.db
HQ_DB_COPY=/data/backups/before-$H-hq-$TS.db
docker --context nas exec endora-node endora-node backup $NODE_DB >$L/backup-node.log 2>&1 \
  || { head -3 $L/backup-node.log; stop "the node's backup failed"; }
docker --context nas exec endora-node test -s $NODE_DB || stop "the node's backup is missing or empty"
docker --context nas exec endora-hq sh -c 'ENDORA_DB="$HQ_DB" endora-node backup '$HQ_DB_COPY >$L/backup-hq.log 2>&1 \
  || { head -3 $L/backup-hq.log; stop "HQ's backup failed"; }
docker --context nas exec endora-hq test -s $HQ_DB_COPY || stop "HQ's backup is missing or empty"
echo "backups: $NODE_DB $HQ_DB_COPY"
# DRAIN_STRICT=1: a drain that did not happen (refused, cap passed with turns running, NO_DRAIN
# in local.mk) stops the deploy before any restart (scripts/hq-drain.sh, test-deploy-recipes.sh).
make deploy-check DRAIN_STRICT=1 >$L/deploy-check.log 2>&1 \
  || { grep -E '^drain|test result|FAILED' $L/deploy-check.log; stop "the NAS deploy or its smoke failed"; }
grep -E '^drain|test result' $L/deploy-check.log || true
PI=$(make -s -f Makefile -f - v <<<'v: ; @echo $(ENDORA_PI_SSH)')
[ -n "$PI" ] || stop "ENDORA_PI_SSH is not set in local.mk"
ssh "$PI" "docker tag endora-node:latest endora-node:before-$H && docker tag endora-workbench:latest endora-workbench:before-$H" \
  || stop "the Pi's rollback tags failed"
make deploy-pi DRAIN_STRICT=1 >$L/deploy-pi.log 2>&1 || { grep -E '^drain' $L/deploy-pi.log; stop "the Pi deploy failed"; }
grep -E '^drain' $L/deploy-pi.log || true
URL=$(make -s -f Makefile -f - v <<<'v: ; @echo $(ENDORA_URL)')
HEALTH=$(curl -sk --fail --max-time 10 "$URL/health") || stop "the NAS did not answer its health check"
echo "$HEALTH"
[[ "$HEALTH" == *"\"build\":\"$H\""* ]] || stop "the NAS reports another build than $H"
PI_BUILD=$(ssh "$PI" 'docker exec endora-runner printenv ENDORA_BUILD') || stop "the Pi's runner did not say its build"
[ "$PI_BUILD" = "$H" ] || stop "the Pi's runner reports build $PI_BUILD, not $H"
echo "released $H (rollback: endora-node:before-$H on both hosts; backups as above)"
