# 0092 — The home lab is a server-owned read view

Status: accepted for implementation.

## Context

A small terminal client needs container health and media queues. Giving each
client service keys would duplicate integration behavior and spread credentials.
Giving the model a generic LAN request tool would cross the egress boundary.

## Decision

Capabilities owns a dashboard application port and fixed-route infrastructure
readers. The node polls them sequentially every 30 seconds, with a five-second
budget per source and missed ticks skipped; refreshes cannot overlap. Its authenticated
`GET /v1/homelab?page=1` serves bounded projections. Pages contain 20 items, with
at most 100 cached items per service. The actual upstream total is also returned.

Origins are explicit host-held settings in the existing settings repository.
Redirects, ambient proxies, credential URLs and arbitrary request routes are
refused. This is a dedicated integration path, not a relaxation of guarded web
fetch. Readers are absent from the model tool catalogue and observations are not
automatically sent to a model. There are no service writes.

Docker is reached through a loopback fixed-route proxy that permits only version
and container-summary reads and strips other fields. The proxy, unlike the Pi,
has privileged socket access and must run only on the trusted Docker host.

## Consequences

Clients stay thin and carry only their Endora session. Source failures are
independent safe categories. Reads do not require model availability. Later
controls need a separate deterministic authorization design; this ADR grants
none. No database tables, domain dependencies, or new bounded context are added.

The application port uses JSON display projections, consistent with existing
capabilities application contracts; no transport type enters the domain.
