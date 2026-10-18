# OpenBao

The house's secrets manager, beside the Endora node ([Endora ADR
0127](../../../../../docs/adr/0127-openbao-and-the-passkey-unlock.md)). OpenBao is the open-source
fork of HashiCorp Vault (MPL-2.0).

This role **only runs it**. Endora initialises it once, from the console, and unlocks it after
every restart with the owner's passkey: Settings › Vault. Nothing here holds a key, a share or a
token, and nothing here unseals anything.

## What it runs

- The official image, **pinned by digest** (`openbao_image`), never auto-updated (a
  `watchtower.enable=false` label), on the Docker network Endora's `docker-compose.yml` makes
  for the node and the vault: `endora_vault`, internal (no internet, no LAN, no host address).
  The node reaches it as `http://openbao:8200`; nothing else can.
- Integrated (raft) storage and the audit log in `openbao_data_dir`, owned by the image's
  `openbao` user (100:1000), mode `0700`. The storage is encrypted by the vault's barrier; a copy
  of the folder opens nothing without the unseal shares.
- Plain HTTP inside that network: the only other member is the node (the ADR says why TLS is
  left for the PKI work).
- Read-only root filesystem, every capability dropped, `no-new-privileges`, a 256 MB memory cap
  (it idles near 20 MB), and a health check that passes while it answers, sealed or not.
- No port on the NAS unless `openbao_host_port` asks for one (e.g. `127.0.0.1:8200`, for the
  `bao` command line on the NAS itself).

## Settings

```yaml
openbao_enabled: true
openbao_data_dir: "{{ docker_home }}/openbao"   # in your backups
```

and give the node the vault's address with the `endora` role:

```yaml
endora_vault_url: http://openbao:8200
```

Order matters once: **the node first** (it makes the `endora_vault` network), then this role.
The role refuses to start the vault without that network.

## Backups

Back up `openbao_data_dir` with everything else; for a consistent copy while it runs,
`bao operator raft snapshot save` (with a token that may) is the tool. The unseal shares are
**not** in it: they are on the node, wrapped by the owner's passkeys, and the owner holds a
break-glass copy. Keep the two apart.

## Turning it off

Set `openbao_enabled: false` and run the role. The container is removed; the data folder is
kept.
