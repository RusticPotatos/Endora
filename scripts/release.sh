#!/bin/zsh
# Release this checkout's HEAD to the NAS and the Pi: push the branch, a release worktree,
# rollback tags, snapshots of both databases, the drain (no turn is cut), deploy, health. Check
# first that no turn is running (`runs.status = 'running'` in HQ). Addresses come from local.mk.
cd "$(dirname "$0")/.." || exit 1
ROOT=$PWD
BRANCH=$(git rev-parse --abbrev-ref HEAD)
[ -f local.mk ] || { echo "local.mk is missing: it holds the addresses"; exit 1; }
[ -f scripts/prune-builds.sh ] && PRUNE_BUILDS_KEEP="Endora-gate $(basename "$ROOT")" sh scripts/prune-builds.sh 15 "$(dirname "$ROOT")" | tail -1
git push -q origin "$BRANCH" || exit 1
H=$(git rev-parse --short HEAD); echo $H
REL="$(dirname "$ROOT")/Endora-release-$H"
git worktree add -q --detach "$REL" $H && cp local.mk "$REL/" && cd "$REL" || exit 1
L=$ROOT/.local/release-$H; mkdir -p $L
docker --context nas tag endora-node:latest endora-node:before-$H && echo tagged
TS=$(date -u +%Y%m%dT%H%M%SZ)
docker --context nas exec endora-node endora-node backup /data/backups/before-$H-node-$TS.db 2>&1 | head -1
docker --context nas exec endora-hq sh -c 'ENDORA_DB="$HQ_DB" endora-node backup /data/backups/before-'$H'-hq-'$TS'.db' 2>&1 | head -1
make deploy-check > $L/deploy-check.log 2>&1; echo nas-exit=$?; grep -E '^drain|test result' $L/deploy-check.log
PI=$(make -s -f Makefile -f - v <<<'v: ; @echo $(ENDORA_PI_SSH)')
ssh "$PI" "docker tag endora-node:latest endora-node:before-$H; docker tag endora-workbench:latest endora-workbench:before-$H"
make deploy-pi > $L/deploy-pi.log 2>&1; echo pi-exit=$?; grep -E '^drain' $L/deploy-pi.log
curl -sk "$(make -s -f Makefile -f - v <<<'v: ; @echo $(ENDORA_URL)')/health"; echo
ssh "$PI" 'docker exec endora-runner printenv ENDORA_BUILD'
