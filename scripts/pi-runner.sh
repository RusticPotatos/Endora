#!/bin/sh
# Starts (or restarts) Endora's runner on the dev node, the Raspberry Pi (ADR 0110). Runs ON the
# Pi, sent there by `make deploy-pi`:
#
#   ssh <the Pi, as endora> sh -s -- <nas address> [token folder] < scripts/pi-runner.sh
#
# The runner asks HQ for turns through the NAS's team gateway and starts each turn's sandbox on
# the endora_sandbox network, whose only way out is that gateway (the endora_dev_node Ansible
# role sets it up): no internet, no DNS, no proxy. The image each turn runs in is the runner's
# own default (the workbench, endora-workbench:latest, which make deploy-pi copies here too).
#
# It holds Docker's socket, which is as good as holding this host, so it runs as the deploying
# user (in the docker group) rather than root, read-only, with no capabilities and a hard cap
# on memory, CPU and processes.
set -eu
nas="${1:-}"
tokens="${2:-/etc/endora/runner-token}"

case "$nas" in
    "" | *[!0-9.]*)
        echo "pi-runner: the NAS address must be an IPv4 address, got '$nas'" >&2
        exit 2
        ;;
esac
if [ ! -s "$tokens/token" ]; then
    echo "pi-runner: no runner token in $tokens. Copy it from the NAS first: make pi-runner-token" >&2
    exit 1
fi
if ! docker network inspect endora_sandbox >/dev/null 2>&1; then
    echo "pi-runner: no endora_sandbox network here. Prepare the Pi first: deploy/ansible/pi.yml" >&2
    exit 1
fi
for image in endora-node:latest endora-workbench:latest; do
    if ! docker image inspect "$image" >/dev/null 2>&1; then
        echo "pi-runner: no $image image here. make deploy-pi builds and copies it" >&2
        exit 1
    fi
done

gateway="http://$nas:8788"
socket_group=$(stat -c %g /var/run/docker.sock)

docker rm -f endora-runner >/dev/null 2>&1 || true
docker run -d --name endora-runner --restart unless-stopped \
    --entrypoint endora-runner \
    --user "$(id -u):$(id -g)" --group-add "$socket_group" \
    --read-only --cap-drop ALL --security-opt no-new-privileges:true \
    --memory 64m --memory-swap 64m --cpus 0.5 --pids-limit 64 \
    -e HQ_URL="$gateway/team-gateway" \
    -e HQ_RUNNER_TOKEN_FILE=/runner-token/token \
    -e TEAM_NETWORK=endora_sandbox \
    -e WORKER_HQ_MCP="$gateway/team-gateway/v2/mcp" \
    -e WORKER_MODEL_URL="$gateway/team-relay/local/v1" \
    -e WORKER_CLOUD_URL="$gateway/team-relay/v1" \
    -v /var/run/docker.sock:/var/run/docker.sock \
    -v "$tokens:/runner-token:ro" \
    endora-node:latest >/dev/null
echo "pi-runner: endora-runner started, asking $gateway/team-gateway for turns"
