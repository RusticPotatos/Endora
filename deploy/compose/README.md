# Installing Endora with Docker Compose

- `core/`: Endora Core on its own (node, HQ, and the runner behind the `workers` profile).
- `compose.yaml`: the full system on one Linux host. It includes Core's file plus Console's and
  Audio's own compose files, copied verbatim at the releases pinned in `SOURCES.json`.

Start, bootstrap, ports, volumes, backup and restore, upgrade and rollback:
[docs/install.md](../../docs/install.md). The decision is
[ADR 0147](../../docs/adr/0147-full-system-bundle.md).
