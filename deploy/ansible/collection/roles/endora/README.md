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

## Every other setting

Each is empty by default, which renders nothing and leaves the compose file's own default, so
a run with none of them set changes nothing. They match the names in Endora's `local.mk`.
Secrets are folders or files, never values.

| Variable | Compose variable | Meaning (default when empty) |
|---|---|---|
| `endora_workers` | profile `workers` | The team's runner; holds Docker's socket. Needs `endora_hq` (false) |
| `endora_lan_dns` | `ENDORA_LAN_DNS` | The router the node asks for device names (1.1.1.1) |
| `endora_hq_tick_seconds` | `ENDORA_HQ_TICK_SECONDS` | Seconds between HQ engine ticks (30) |
| `endora_hq_dir` | `ENDORA_HQ_DIR` | Host folder for HQ's data (a Docker volume) |
| `endora_sound_dir` | `ENDORA_SOUND_DIR` | Host folder for the sound hub's patchbay (a Docker volume) |
| `endora_utc_offset_minutes` | `ENDORA_UTC_OFFSET_MINUTES` | Fixed UTC offset fallback, e.g. `-300` (0) |
| `endora_router_model`, `endora_synth_model` | `ENDORA_ROUTER_MODEL`, `ENDORA_SYNTH_MODEL` | Models for routing and synthesis (the main model) |
| `endora_ansible_nas_repo`, `endora_ansible_nas_ref` | `ENDORA_ANSIBLE_NAS_REPO`, `ENDORA_ANSIBLE_NAS_REF` | The lab's ansible-nas the team reads, and its branch (none; the default branch) |
| `endora_lab_retired` | `ENDORA_LAB_RETIRED` | Retired containers for the team's map: `name=note;name=note` (none) |
| `endora_git_push_key_dir` | `ENDORA_GIT_PUSH_KEY` | Folder holding the file `key`, a deploy key (ADR 0115) (the node cannot push) |
| `endora_bench_token_dir` | `ENDORA_BENCH_TOKEN` | Folder holding the file `token` (ADR 0123) (no bench) |
| `endora_bench_cap_cents` | `ENDORA_BENCH_CAP_CENTS` | What the bench may spend a day (200) |

## A drain before this host shuts down (optional)

Endora ADR 0134: `make deploy` asks HQ to stop starting turns and to wait for the ones under way
before it restarts anything. The same can happen when this host shuts down, so the NAS's 04:00
security reboot does not cut a turn of ten or fifteen minutes. It is its own switch: it works
whether this role runs Endora (`endora_enabled`) or Endora's own `make deploy` does, as on a NAS
with no checkout of the role's:

```yaml
endora_drain_on_shutdown: true     # off by default: it adds a wait to every shutdown
endora_drain_cap_seconds: 300      # the most a shutdown waits; the unit's timeout is this and a minute
```

The role installs `/etc/systemd/system/endora-drain.service`, and the deploy's own drain script
as `/usr/local/lib/endora/hq-drain.sh` (`files/hq-drain.sh`; Endora's `scripts/hq-drain.sh` is a
link to it). The unit is a oneshot that does nothing when it starts and stays active, ordered
`After=docker.service`. Units stop in the reverse of their start order, so at shutdown it is
stopped, and its `ExecStop` runs the script's `drain` while Docker and HQ are still up. A drain
that fails never holds the shutdown. Nothing is lifted afterwards: HQ lifts the drain itself when
its container starts again at boot. Setting the variable back to false disables the unit and
removes its files without stopping it (stopping it would run the drain). The variables are not
compose settings, so they are not in `endora.env`; `tests/render.yml` renders the unit and checks
it.

## Parity with the compose file

Anything Endora's `docker-compose.yml` reads from the environment can be declared here, so this
role reproduces what `make deploy` with the owner's `local.mk` does. The rule: **a new compose
variable (or profile) needs a role variable, rendered in `templates/endora.env.j2` and tested in
`tests/render.yml`, in the same change.** Endora's `scripts/check-collection-parity.sh` (run by
CI) fails when the compose file interpolates a variable the template does not render, or a
profile `endora_profiles` cannot start. Its Mac-only allow-list is two entries: `ENDORA_BUILD`
(stamped at build time) and the `bundled` profile. The rest of `local.mk`
(`DOCKER_CONTEXT`, `ENDORA_MAC_*`, `ENDORA_PI_SSH`, `ENDORA_BUILD_CONTEXT`, ...) only matters on
the Mac and is not compose input.

## Sound, and the AirPlay 2 door

- **The sound hub** (Endora ADR 0104): `endora_sound: true` starts it (the `sound` profile,
  with its Spotify receiver) and gives the node its address; `endora_sound_senders: true`
  starts its AirPlay senders (not beside an ansible-nas `owntone` container: both take UDP
  319/320). Their images come from `make deploy` like the node's; the role stops without them.
- **The AirPlay 2 door** (Endora ADR 0128): "Endora Mixer" as AirPlay 2, so an Apple TV keeps
  lip-sync, in its own container on a LAN address of its own (a macvlan network on this host's
  LAN interface); the senders stop running the classic door while it is on. It needs the hub,
  and four values that are yours (the role stops without them):

  ```yaml
  endora_sound: true
  endora_sound_airplay2: true
  endora_sound_airplay2_parent: eth0        # `ip -br link` on the NAS: UP, with the LAN address
  endora_sound_airplay2_subnet: <your LAN as CIDR>
  endora_sound_airplay2_gateway: <your router>
  endora_sound_airplay2_ip: <a free address outside the DHCP pool, reserved in the router>
  # endora_sound_airplay2_latency: 0.5      # seconds it hands its sound over early; tune by ear
  # endora_sound_airplay2_network: <name>   # join a macvlan the LAN already has (one per subnet)
  ```

  The NAS cannot reach the door's address (macvlan), by design. Endora's `docs/homelab.md`,
  "The AirPlay 2 door", has the UniFi steps and lip-sync tuning.

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
