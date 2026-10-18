# Endora

Homepage: [https://github.com/RusticPotatoes/Endora](https://github.com/RusticPotatoes/Endora)

Endora is a self-hosted butler for the house: a chat console that reads your Home Assistant,
your mail copy (see [Mail Mirror](../mail_mirror/README.md)) and your containers, using a model that
runs on your own hardware (see the `ollama` role).

This role starts Endora on the NAS with Endora's own `docker-compose.yml` (checked out at a
branch or tag) and this host's settings. **It never builds the image, and never pulls one.**
A release build needs several GB of memory for minutes; on a NAS that shares its memory with
Home Assistant, one ran the box out of memory and swap and the house stopped answering. Build
on another machine with Endora's `make deploy`, which compiles there for the NAS's processor
and copies the image here (set `ENDORA_BUILD_CONTEXT` in Endora's `local.mk`), then run this
role. Without an image the role stops and says so.

The Compose project is named `endora`, the name `make deploy` uses, so a node started either
way is the same node with the same data. Both set the node's settings, so keep this role's in
step with Endora's `local.mk`, or run this role after a deploy.

## Settings

```yaml
endora_enabled: true
endora_version: develop          # where the compose file comes from: a branch, tag or commit
endora_docker_observer: true     # a read-only view of this host's containers
```

- **Mail** is mounted read-only from `mail_mirror_root` when the `mail_mirror` role is
  enabled; set `endora_mail_dir: ""` to keep it from Endora.
- **The model** defaults to the `ollama` role on this host (`endora_model_url`,
  `endora_model`).
- **The time zone** is the NAS's own (`ansible_nas_timezone`) unless `endora_time_zone`
  names another, so patterns are on your clock and the daily brief keeps its hour when the
  clocks change.
- **The vault** (Endora ADR 0127): `endora_vault_url: http://openbao:8200` gives the node the
  OpenBao the [`openbao`](../openbao/README.md) role runs, on the internal `endora_vault`
  network Endora's compose file makes for the two. Run this role first: it makes that network.
  Passkeys are bound to `endora.home.arpa` unless `endora_passkey_rp_id` names another.
- **The certificate.** The node serves HTTPS with a self-signed certificate issued for the
  NAS's own address (`endora_tls_san`, by default the address Ansible sees). Accept the
  browser's warning once.

## Where its data lives

Everything Endora keeps (every setting, API key and sign-in secret) is one SQLite database. By
default it is in the Docker volume `endora_endora-data`, which is **not** in your backups. To
keep it in a folder that is:

1. Copy the volume there with Endora's `make migrate-data` (it verifies every file and deletes
   nothing).
2. Set `endora_data_dir` to that folder, for example `"{{ docker_home }}/endora/data"`.

The role refuses to start on an empty `endora_data_dir` while the volume still holds a
database: that would be a brand-new node with no settings, which looks like data loss.

## Checking it

```bash
docker logs --tail 20 endora-node
```

The console's header shows the build (the short commit) that is running. Open it at
`https://<your-nas>:8787`.

## Turning it off

Set `endora_enabled: false` and run the role. It stops a node it started (it looks for its
own checkout under `endora_root`); a node started some other way is left alone. The data volume
is never removed.
