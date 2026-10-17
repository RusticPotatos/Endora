# Home-lab dashboard

For explicitly confirmed controls and durable jobs, see
[the handheld console](terminal-console.md). This observer remains read-only.

The dashboard is read-only and independent of AI models. The terminal uses
`GET /v1/homelab?page=1` with its Endora bearer session. The response contains an
observation timestamp and three service summaries. States are `unconfigured`,
`healthy`, `degraded`, `unavailable`, or `stale`. Items are paged in groups of 20;
up to 100 items per source are cached. Queue totals can exceed that cache.

## Private server setup

Run as the same OS user and with the same `ENDORA_DB` as the node:

```sh
endora-node homelab-setup
```

Enter origins interactively. API keys are masked. Blank preserves the current
setting; `-` disables a source and clears its key. Settings remain in the node's
private SQLite database. Keep the database and backups private; they are not
encrypted by this feature. No keys are accepted through command arguments.

Sonarr and Radarr use `/api/v3/queue`; give their origin without a path or query.
Their keys permit broader operations, but this adapter only sends fixed GETs.
TLS uses the existing HTTP library's verified trust roots; do not disable TLS.
HTTP is possible on a trusted private network, but does not encrypt keys.

## Docker boundary

For the supplied Compose deployment, enable the optional `homelab` profile after
building the new node image:

```sh
docker compose --profile homelab up -d docker-observer
docker compose exec node endora-node homelab-setup
```

Use your configured private Docker context if deploying remotely. The observer
shares the node's network namespace and binds only loopback; Docker's origin is
`http://127.0.0.1:9876`. The socket mount is **not** a read-only API restriction:
the Python proxy's exact method/route allowlist is the boundary. The observer has
privileged host socket access but is not exposed on a published port. Use the
profile when recreating the node so its observer is recreated with that namespace.

On the Docker host, start the supplied proxy as a trusted account with socket
access, preferably through a host service supervisor:

```sh
python3 scripts/homelab-docker-proxy.py
```

It binds only loopback on port 9876 and accepts exactly `GET /version`,
`GET /containers/json?all=true`, and `GET /containers/<name>/json` for a name
matching `[A-Za-z0-9][A-Za-z0-9_.-]{0,127}` (the team's lab map, ADR 0112: only
the id, name, image, state, restart policy, network mode and the compose
service, project and `depends_on` labels are passed on; never the environment,
mounts or binds). Other routes, queries and write methods are refused.
Configure Docker's dashboard origin as `http://127.0.0.1:9876` when the node
runs on that host. For a remote host, forward that loopback endpoint through a
private SSH tunnel. Do not publish the proxy or raw daemon port to the internet.
If Endora is containerized, loopback is inside its container: arrange a private
tunnel accessible there, rather than assuming the host endpoint is reachable.

The proxy's socket access is powerful even though its HTTP interface is narrow.
Never give the Pi socket access or mount the socket into the terminal client.

Each source has a five-second budget. Polling starts every 30 seconds, with missed
ticks skipped and refreshes serialized. Failures yield safe categories rather than upstream text.
Snapshots older than 60 seconds are marked stale. No environment variables,
mounts, filesystem paths, or raw service errors appear in dashboard responses.
