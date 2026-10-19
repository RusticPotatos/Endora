#!/bin/sh
# Asks the running HQ to drain before a deploy restarts anything, and lifts the drain after
# (ADR 0134). `make deploy` and `make deploy-pi` run it; so can anyone.
#
#   sh scripts/hq-drain.sh drain     announce a restart, and wait until no turn is running or the
#                                    cap has passed (it prints what it did)
#   sh scripts/hq-drain.sh resume    lift the announcement, if HQ's own start has not
#
# It runs `endora-hq drain` and `endora-hq resume` inside the HQ container, which talk to HQ on
# its own address with its own token. It is set by:
#
#   DOCKER=docker             the docker to use: `docker --context nas` for the NAS
#   HQ_CONTAINER=endora-hq    HQ's container
#   DRAIN_CAP=300             seconds the restart waits for turns under way
#   NO_DRAIN=1                do nothing at all
#
# A deploy is never stopped by it: whatever goes wrong is said, and it exits 0. The one thing it
# will not do is run the command on an HQ that predates it, because that HQ would take the
# arguments for nothing and start a second HQ over the same database.
set -u

DOCKER="${DOCKER:-docker}"
HQ_CONTAINER="${HQ_CONTAINER:-endora-hq}"
DRAIN_CAP="${DRAIN_CAP:-300}"
DRAIN_RETRY_SLEEP="${DRAIN_RETRY_SLEEP:-3}"
RESUME_TRIES=10

# Whether HQ's container is running where DOCKER points.
hq_is_up() {
    # shellcheck disable=SC2086 # DOCKER is a command with its own words, `docker --context nas`
    $DOCKER ps --format '{{.Names}}' | grep -qx "$HQ_CONTAINER"
}

# Whether the HQ running there knows `drain`: its usage line is in its binary if it does.
hq_knows_drain() {
    # shellcheck disable=SC2086
    $DOCKER exec "$HQ_CONTAINER" grep -qaF 'usage: endora-hq [drain' /usr/local/bin/endora-hq
}

drain() {
    if [ -n "${NO_DRAIN:-}" ]; then
        echo "hq-drain: NO_DRAIN is set: HQ is not drained, and turns under way may be cut"
        return 0
    fi
    if ! hq_is_up; then
        echo "hq-drain: HQ is not running here: nothing to drain"
        return 0
    fi
    if ! hq_knows_drain; then
        echo "hq-drain: the running HQ predates 'endora-hq drain': nothing to drain this time"
        return 0
    fi
    # shellcheck disable=SC2086
    if ! $DOCKER exec "$HQ_CONTAINER" endora-hq drain --cap "$DRAIN_CAP" --wait; then
        echo "hq-drain: could not drain HQ; deploying anyway"
    fi
    return 0
}

resume() {
    if [ -n "${NO_DRAIN:-}" ]; then
        return 0
    fi
    # shellcheck disable=SC2086
    if ! $DOCKER ps -a --format '{{.Names}}' | grep -qx "$HQ_CONTAINER"; then
        return 0
    fi
    tries=0
    while [ "$tries" -lt "$RESUME_TRIES" ]; do
        tries=$((tries + 1))
        # shellcheck disable=SC2086
        if $DOCKER exec "$HQ_CONTAINER" endora-hq resume; then
            return 0
        fi
        sleep "$DRAIN_RETRY_SLEEP"
    done
    echo "hq-drain: could not lift the drain; HQ lifts it itself when it starts, or after half an hour"
    return 0
}

case "${1:-}" in
    drain) drain ;;
    resume) resume ;;
    *)
        echo "usage: sh scripts/hq-drain.sh drain|resume" >&2
        exit 2
        ;;
esac
