# Explicit handheld home console

The terminal's Home, Media, NAS and Jobs pages use the capabilities context's
explicit application port. They do not accept model-generated routes or commands.
These interfaces supplement the existing `/v1/homelab` read-only observer.

## Private setup

On the node host, use its normal `ENDORA_DB` and administration user:

```sh
endora-node terminal-setup
```

The wizard stores origins, masked service keys, Portainer environment ID,
Prometheus Telegraf host label, Home Assistant favorites and the job agent key
through existing private capability settings. Blank preserves settings; `-`
disables an integration. No credentials are accepted in arguments or returned
to the terminal. Home Assistant reuses the existing integration's URL and token.

The Homepage, Overseerr, Portainer and Prometheus settings, and the Overview page's Docker,
Sonarr and Radarr sources, can also be entered in the console under Settings → Connections →
Home lab, each named for its service.
The handheld home screen mirrors Homepage (`GET /v1/terminal/dashboard`): its groups and
service names, each with the live state of its container, NAS figures and firing
alerts. Set the Homepage address under Connections → Home lab → Homepage. Only names and states
are returned; Homepage links, icons and widget settings stay on the node.
For a fixed list of Homepage widget types (Overseerr, Radarr, Sonarr, NZBGet, Prowlarr,
Gatus, Prometheus, Backrest) the node also asks Homepage's own widget proxy for a few
numbers and returns them as a short `detail` string per service; Homepage keeps the
service credentials and the endpoints are fixed in code. These entries only hold
settings: they are never ready for chat themselves (`configured: false`), and their `invoke`
backs the settings Test button with one read-only request that reports a category, never
upstream text or the key. Both routes write the same keys.

Once Docker, Portainer, Sonarr, Radarr or NAS metrics are set up here, chat can read them too,
through separate read-only skills that borrow these settings: "What the containers are doing",
"What the containers are doing (Portainer)", "What Sonarr is downloading", "What Radarr is
downloading" and "How the NAS is doing". Nothing is configured twice, and a skill is offered only
once its entry is set up. They take no input, so the model chooses no address, and they return
names, states, titles, progress and figures only. They never start, stop or restart anything,
never read container logs and never search media; those stay on the handheld, and Overseerr and
the home screen stay handheld-only. Their results mark the turn like any other outside source
(ADR 0064): it can answer but cannot act, and it is never escalated to the deep model, so
container names and NAS figures stay on the device.

Overseerr submissions require an explicitly selected numeric requesting user.
The adapter sends `X-Api-User` alongside the server-side key so that user's
request permissions and approval rules apply. Without a configured user,
submission is disabled; the adapter does not default to the administrator.
Search and request status are separate from approval and actual availability.
The existing Overseerr service is retained; migration is unrelated maintenance.

Portainer exposes fixed inventory, bounded log and start/stop/restart routes for
the configured environment. The adapter checks inventory membership and protects
the recognized Endora, Cloudflare, Portainer and Docker observer control path
using names, image and Compose service label. Host maintenance remains an SSH
operation. The read-only Docker observer is unchanged.

Metrics use fixed Prometheus queries with the privately configured instance
label. Unsupported measurements remain unavailable. There is no new monitoring
stack and observations are not automatically sent to AI context.

## Host job agent

Run `python3 scripts/terminal-jobs-setup.py` as the existing non-root NAS
administration user. Select the existing checkout, private inventory and service
roles explicitly. The generated catalog lives under `~/.config/endora-jobs/`,
mode 0700 with a mode 0600 config. It is not copied into Git. A key is generated
locally; transfer it only through the node's masked setup prompt.

The generic role in `deploy/ansible/roles/endora_terminal_agent` installs
`scripts/terminal-jobs.py` from an explicitly selected server-side source path.
Review and merge this role into the existing NAS playbook while preserving local
edits. The role defaults to disabled; supply `endora_terminal_agent_source`,
the existing non-root user and private config path. Enable it deliberately.
The service owns `/run/endora-jobs/agent.sock` and `/var/lib/endora-jobs/`.
The node's Compose mount shares only that runtime directory; the key and exact
agent routes provide authorization in addition to filesystem permissions.
Start the host service before recreating the node with this mount.

Catalog entries are syntax validation, `--check` of a selected role, or applying
a selected role. The client supplies only the catalog ID and request ID. No
extra variables, arbitrary paths, shell strings or credentials are accepted.
Never configure roles that disable the remote-access/control path. Verify role
tags against the actual playbook before enabling selected-role jobs.

Jobs run serially against one checkout, with a process-wide lock. SQLite retains
queued work and operation IDs. Restart marks interrupted running jobs uncertain;
it does not replay them. Output is bounded and obvious credential assignments
are redacted, but Ansible output remains private and must not be published.
Old output is compacted while IDs and final state remain for deduplication.

## Authentication and APIs

`POST /v1/session/dashboard`, with a full session, creates a separate narrow
credential for `GET /v1/homelab/overview`. This returns only timestamps, service
states and aggregate counts. Dashboard sessions cannot read chat, activity,
private entity/container names, logs, jobs or any mutation. Unknown persisted
scope values deny access.

Full-session reads include `/v1/terminal/home`, `/home/catalog`, `/media`,
`/media/requests`, `/containers`, `/logs/{container_id}`, `/jobs`,
`/jobs/{request_id}` and `/operations/{request_id}` under `/v1/terminal`.
The node uses fixed upstream routes, timeouts, response budgets and safe error
categories. Redirects are refused. These adapters do not alter general web
egress policy or accept model-supplied destinations.

`POST /v1/terminal/action` requires `request_id`, `kind`, `action`, `target` and
`confirmed: true`. The deterministic validator rejects unknown fields and action
vocabulary. The journal reserves an ID before a side effect. Same-ID retries
return the recorded result; conflicting reuse fails. Transport/read-back failure
means uncertain outcome. The client queries status after reconnect rather than
repeating a mutation. Home controls report observed state and verification;
job submission reports queued/running state rather than claiming completion.

Existing chat and dashboard interfaces remain compatible. The new locked
appliance must be paired with a node supporting these narrow credentials.
