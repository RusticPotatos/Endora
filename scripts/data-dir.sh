#!/usr/bin/env sh
# Where Endora keeps its data, and not losing it when that changes.
#
#   data-dir.sh check     Before a deploy: refuse to start the node on an EMPTY
#                         ENDORA_DATA_DIR while the old Docker volume still holds its data.
#   data-dir.sh migrate   One-time copy of that volume into ENDORA_DATA_DIR, verified file by
#                         file. The volume is left in place as the way back.
#
# Environment (the Makefile passes these): DOCKER_CONTEXT, COMPOSE_PROJECT_NAME,
# ENDORA_DATA_DIR. Nothing here deletes anything.
#
# Why this exists: the node keeps everything — every setting, API key and sign-in secret —
# in one SQLite file under /data. Pointing /data at a host folder so it can be backed up is
# one line of compose, and getting it wrong is silent: deploy first and the node starts on an
# empty folder, which is a brand-new node (new token, no settings) that looks like data loss.
set -eu

PROJECT="${COMPOSE_PROJECT_NAME:-endora}"
VOLUME="${PROJECT}_endora-data"
IMAGE="${ENDORA_IMAGE:-endora-node:latest}"
DIR="${ENDORA_DATA_DIR:-}"

dk() {
  if [ -n "${DOCKER_CONTEXT:-}" ]; then
    docker --context "$DOCKER_CONTEXT" "$@"
  else
    docker "$@"
  fi
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

absolute() {
  case "$DIR" in
    /*) ;;
    *) die "ENDORA_DATA_DIR must be an absolute path on the machine Docker runs on (on a NAS, a path on the NAS); got '$DIR'." ;;
  esac
}

# Every container started here runs with --pull never. The image is the one this project built
# on that host; without the flag, a missing image would be pulled *by name* from the public
# registry, and something this project never built would run against your data.
have_image() {
  dk image inspect "$IMAGE" >/dev/null 2>&1
}

# Does a database sit at /data/endora.db in this mount? (-s: it exists and is not empty.)
# A bind source that does not exist makes the run fail, which correctly reads as "no".
holds_a_database() {
  dk run --rm --pull never --entrypoint test "$@" "$IMAGE" -s /data/endora.db >/dev/null 2>&1
}

volume_exists() {
  dk volume inspect "$VOLUME" >/dev/null 2>&1
}

check() {
  [ -n "$DIR" ] || return 0
  absolute
  # Nothing built on this host yet means a first deploy: there is nothing to protect.
  have_image || return 0
  if holds_a_database --mount "type=bind,source=$DIR,target=/data,readonly"; then
    return 0
  fi
  # `volume_exists` first: mounting a named volume that is absent would create an empty one.
  if volume_exists && holds_a_database -v "$VOLUME":/data:ro; then
    die "Refusing to deploy.
ENDORA_DATA_DIR ($DIR) has no database, but the Docker volume $VOLUME still holds this node's data.
Starting now would create a brand-new node: a new token, and none of your settings.

  make deploy-down && make migrate-data     copy the data across (nothing is deleted), then deploy again
  unset ENDORA_DATA_DIR                     keep using the volume as before"
  fi
  return 0
}

migrate() {
  [ -n "$DIR" ] || die "Set ENDORA_DATA_DIR (an absolute host path) in local.mk first."
  absolute
  volume_exists || die "No Docker volume $VOLUME on this host, so there is nothing to migrate. Is DOCKER_CONTEXT pointing at the right machine?"
  have_image || die "The image $IMAGE is not on this host. Deploy once (make deploy) so it exists, then stop the node and run this again."
  if [ -n "$(dk ps --filter 'name=^endora-node$' --filter status=running -q)" ]; then
    die "The node is running, so its database could change while it is copied. Stop it first: make deploy-down"
  fi

  echo "Copying $VOLUME -> $DIR (the volume is not modified or removed)"
  # The copy and its verification run in one container so a failure leaves nothing half-trusted.
  # SRC and DST default to the container paths; they are variables only so the test can run
  # this exact script against temporary directories.
  dk run --rm --pull never -i --entrypoint sh \
    -v "$VOLUME":/from:ro -v "$DIR":/to "$IMAGE" -s <<'INSIDE'
set -eu
SRC="${SRC:-/from}"
DST="${DST:-/to}"
[ -s "$SRC/endora.db" ] || { echo "The volume has no endora.db: refusing to copy." >&2; exit 1; }
if [ -n "$(ls -A "$DST")" ]; then
  echo "The destination is not empty: refusing to copy over it." >&2
  exit 1
fi
cp -a "$SRC"/. "$DST"/
(cd "$SRC" && find . -type f -exec sha256sum {} + | sort -k2) >/tmp/source.sums
(cd "$DST" && find . -type f -exec sha256sum {} + | sort -k2) >/tmp/copy.sums
cmp /tmp/source.sums /tmp/copy.sums || { echo "THE COPY DOES NOT MATCH THE ORIGINAL." >&2; exit 1; }
echo "Copied and verified: $(wc -l </tmp/copy.sums | tr -d ' ') files, every checksum identical."
INSIDE

  cat <<EOF

Done. Your data is now in $DIR as well as in the volume.
Next:
  1. make deploy          (ENDORA_DATA_DIR is set, so the node now uses that folder)
  2. confirm the console still has your settings and sign-in
  3. keep the volume $VOLUME until you are sure; remove it only then:
       docker volume rm $VOLUME
EOF
}

case "${1:-}" in
  check) check ;;
  migrate) migrate ;;
  *) die "usage: data-dir.sh check | migrate" ;;
esac
