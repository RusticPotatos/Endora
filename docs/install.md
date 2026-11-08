# Installing Endora on one Linux host

How to run Endora on one ordinary Linux machine: **Core** on its own, or the **full system**
(Core, Console and Audio) from one pinned bundle. Nothing here needs `make`, `local.mk`,
`release.sh`, a NAS or a Pi. Those belong to the owner's home lab (the Ansible roles in
`deploy/ansible`, and the repository's root `docker-compose.yml` for development); they are
another path, never a requirement. The decision is [ADR 0147](adr/0147-full-system-bundle.md).

| You want | Use | Files |
|---|---|---|
| Endora Core alone (headless autonomy: node, HQ, runner) | Core's compose file | [`deploy/compose/core/`](../deploy/compose/core/) |
| Everything on one host | the bundle | [`deploy/compose/`](../deploy/compose/) |
| Endora Console alone, in front of a Core you already run | Console's own repository | [RusticPotatos/endora-console](https://github.com/RusticPotatos/endora-console) |
| Endora Audio alone (needs no Core) | Audio's own repository | [RusticPotatos/endora-audio](https://github.com/RusticPotatos/endora-audio), README "Deploy" |

Each product works alone. The bundle adds no service and no orchestration: it includes each
product's own compose file unchanged, pinned to a released version.

## Prerequisites

- **Linux on amd64** with **Docker Engine 28 or later** and **Docker Compose v2.20 or later**
  (`include:`). Core and Console also run on arm64; Audio's AirPlay senders and door are amd64
  only. Engine 28 lets the workers' network have no address on the host (see the runner, below).
- **About 2 GB of memory** for everything, within these caps: node 768 MB, HQ 384 MB, Console
  128 MB, Audio's hub 256 MB, senders 512 MB, Spotify receiver 128 MB.
- **A model:** any OpenAI-compatible endpoint (an Ollama on the host is the default). Core starts
  and signs people in without one; the team needs one to do work.
- **For Audio's speakers:** a host on the same LAN as the AirPlay speakers and phones (its
  senders and Spotify receiver use the host's network for mDNS; Docker Desktop's "host" is a VM,
  so discovery does not work there). Do not run it beside a stock OwnTone.
- **Core's image.** Core is published as `ghcr.io/rusticpotatos/endora-node` from develop, main
  and release tags (`.github/workflows/image.yml`). Until a release has gone through it, build
  it yourself from a checkout: `docker build -t ghcr.io/rusticpotatos/endora-node:local .` and
  set `ENDORA_CORE_VERSION=local`.

## First start: the full system

```sh
git clone https://github.com/RusticPotatos/Endora && cd Endora/deploy/compose
cp bundle.env.example .env        # ENDORA_CORE_VERSION is set to a published build; read every other line
docker compose up -d --wait
docker compose exec node endora-core setup        # make Core's owner (once), at a terminal
```

To script the owner's setup instead (no terminal), give the password on stdin and turn off the
terminal with `-T`:

```sh
printf '%s\n' "$ENDORA_OWNER_PASSWORD" | docker compose exec -T node endora-core setup
```

It prints the authenticator key once; keep it. A second run refuses, because there is an owner.

Then open `http://<host>:8787`: that is Console, and Console is where people sign in. To serve
it over HTTPS (recommended, since a password and codes cross the network), put `cert.pem` and
`key.pem` in `console/certs/` and set `CONSOLE_TLS_CERT` and `CONSOLE_TLS_KEY` in `.env`, or put
a TLS reverse proxy in front and set `CONSOLE_ORIGIN` to the address people type.

To leave a product out, delete its line under `include:` in `compose.yaml`. The others do not
depend on it: Core works without Audio, Audio without Core, and Console is the only one that
needs another (it is Core's screens).

## First start: Core alone

```sh
cd Endora/deploy/compose/core
cp core.env.example .env          # ENDORA_CORE_VERSION is set to a published build
docker compose up -d --wait
docker compose exec node endora-core setup        # or the scripted form with -T, above
```

Core then answers on `https://<host>:8787` with its own self-signed certificate (accept it once;
set `ENDORA_TLS_SAN` to the names people use), serving its API and, until the Console cutover,
its built-in screens. Put Console in front later by running Console's compose with
`CORE_URL` pointing at it.

## One-time bootstrap: what is yours to do

`docker compose up` makes no secret for you beyond the tokens each product makes for itself.
These are your steps, once:

1. **Core's owner.** `docker compose exec node endora-core setup` (or the scripted form above) asks for a password (12
   characters or more, not echoed) and prints a QR code, an `otpauth://` URI and a key: add it to
   an authenticator app. That is the whole of Core's sign-in today, a password and a code
   ([ADR 0146](adr/0146-console-forwards-cores-session.md)). It refuses to run again once there
   is an owner: change the password in Console's Settings while signed in. There is no reset of
   a lost password or authenticator yet beyond `endora-node secrets forget
   --i-have-lost-the-key` with the node stopped, which also empties every sealed credential. It makes nothing else.
2. **Your model.** `ENDORA_MODEL_URL` and `ENDORA_MODEL` in `.env` say where; a key, if the
   endpoint needs one, goes in **Settings > AI** in Console, sealed in Core's own store
   ([ADR 0144](adr/0144-endora-keeps-its-own-secrets.md)). Paid providers and their limits are
   set there too. Never put a key in `.env`.
3. **Connections and tokens** Core uses (Home Assistant, the lab's services, script tokens):
   Console's Settings, after sign-in.
4. **Audio's token.** Set `SOUND_TOKEN` (`openssl rand -hex 24`) before the first start, or read
   the one Audio makes: `docker compose exec audio-hub cat /token/token`. Paste it into Audio's
   page at `http://<host>:8791`. For Core's sound tools, also set `ENDORA_SOUND_URL` and
   `ENDORA_SOUND_TOKEN` (optional: Core works the same without them).
5. **Spotify:** pick "Endora" in the Spotify app's devices once (Premium needed).
6. **Audio's AirPlay 2 door** (optional): its own macvlan LAN address, set with Audio's
   `SOUND_AIRPLAY2_*` values and `--profile airplay`. It joins no other product's network.
7. **The runner** (optional): `docker compose --profile workers up -d`, after reading its trust
   boundary below. Workbench turns (Claude Code) need the workbench image, which is not
   published: build it with `docker build --target workbench -t endora-workbench:latest .`.

## The runner's trust boundary and limits

**The runner mounts Docker's socket, which is root on this host; it is off unless you name its
`workers` profile.**

The runner takes the team's turns and starts each as a fresh worker container. It is **off**
unless its `workers` profile is named, because it mounts Docker's socket, which is as good as
root on the host. It trusts HQ with nothing but a run and its token: the image, mounts, network
and limits of every worker are fixed in the runner's own code, not taken from HQ. Every worker:

- runs on the team network, `<project>_team` (`endora-core_team` alone, `endora_team` in the
  bundle), which is internal: it reaches the node and HQ and
  nothing else, no internet, no LAN, and (Docker 28+) not the host itself;
- has a read-only root, every capability dropped and `no-new-privileges`;
- gets at most **256 MB, one CPU and 128 processes**, `/tmp` of 64 MB in memory (a workbench
  turn: 1 GB, two CPUs, 256 processes), and is killed after **30 minutes**;
- is opened only by its own turn's token, which ends with the turn.

`ENDORA_RUNNER_CAPACITY` (default 2) is how many turns run at once. The runner itself is capped
at 64 MB. With the demo on (`ENDORA_HQ_DEMO=1`) HQ hands it nothing.

## Ports

| Port | What | Reachable from |
|---|---|---|
| 8787/tcp | Console (bundle); Core's API and screens (Core alone) | the LAN |
| 8786/tcp | Core's API (bundle) | this host's loopback only |
| 8790/tcp | HQ | the bundle's own networks only; never published |
| 8788/tcp | Core's team listener (model relays and HQ's gateway for workers) | the workers' fenced network; publish it only for a runner on another host |
| 8791/tcp | Audio's page and API | the LAN |
| 3701–3704/tcp, 319–320/udp, 5353/udp, AirPlay and Spotify Connect ports | Audio's senders and receiver | the host's network (needed for speaker discovery) |

## Volumes and data

Every volume has a fixed name, so Core alone and Core in the bundle use the same data. That is
deliberate: an upgrade keeps the data. Without the fixed names, Compose would name each volume
`<project>_<name>`, and an upgrade would open an empty volume beside the old data.

**Two installs on one host.** A second install of Core is the supported way to run two. Copy
`core/compose.yaml` and `core/second.env.example` (as `.env`) into the second install's own folder
and run `docker compose up -d` there, then make its owner as above. Its project name, volumes and
port (`http://127.0.0.1:8796`) are its own, and its data starts empty. It can run beside the bundle
or beside Core alone. `deploy/compose/check-isolation.sh` checks that it shares no container,
network, volume or host port with them. Do not point its volume names at the first install's
volumes: the two would share one database and one secrets key.

Two **full bundles** on one host are not supported: run one full bundle per host. Each container's
name is a setting (`CONSOLE_CONTAINER_NAME`, `ENDORA_AUDIO_CONTAINER_NAME`,
`ENDORA_AUDIO_SPOTIFY_CONTAINER_NAME`, `ENDORA_AUDIO_SENDERS_CONTAINER_NAME`,
`ENDORA_AUDIO_AIRPLAY_CONTAINER_NAME`, in `bundle.env.example`), and a default install keeps today's
names. Setting them lets a second bundle's containers take their own names, but it does not make
the second bundle run: Audio's senders and Spotify use the host's network, with ports (3701-3704,
UDP 319/320) that one host has only once. The Console and Audio commits that add the settings are
pinned from scratch branches (ADR 0147 amendment 4) until those products are released.

| Volume | Holds | Back it up |
|---|---|---|
| `endora-core-data` | Core's database (settings, sealed credentials, sign-in, records), its secrets key `secrets.key` and its TLS certificate | **yes**, and keep `secrets.key` apart from the database copies: without it the sealed credentials cannot be opened |
| `endora-core-hq-data` | HQ's database: the team's work, record and budgets | **yes** |
| `endora-core-hq-token`, `endora-core-hq-runner-token` | the tokens HQ makes for the node and the runner | no: remade if lost |
| `endora-audio-data` | Audio's settings (`sound.json`) | **yes** |
| `endora-audio-token`, `endora-audio-spotify-login`, `endora-audio-senders-data`, `endora-audio-secret` | Audio's token, Spotify login, senders' state and password | optional (see Audio's README) |
| `endora-audio-pipes` | runtime only | no |

Console keeps no data. Back up Core, not Console.

## Backup and restore

**Core's database while it runs** (a consistent, integrity-checked copy):

```sh
docker compose exec node endora-node backup /data/endora-backup.db
docker compose cp node:/data/endora-backup.db ./endora-backup.db
docker compose cp node:/data/secrets.key ./secrets.key     # store it apart from the backups
```

**Everything, cold** (the simplest whole-system copy): stop, copy each volume, start.

```sh
docker compose stop
for v in endora-core-data endora-core-hq-data endora-audio-data; do
  # The container runs as root; the tarball is handed back to you, not left root-owned.
  docker run --rm -e OWNER="$(id -u):$(id -g)" -v "$v":/v:ro -v "$PWD":/b alpine:3.21 \
    sh -c 'tar czf "/b/$1.tgz" -C /v . && chown "$OWNER" "/b/$1.tgz"' sh "$v"
done
docker compose start
```

**Restore** into the same volume names, with the stack stopped, then start it:

```sh
docker compose stop
docker run --rm -v endora-core-data:/v -v "$PWD":/b alpine:3.21 sh -c 'rm -rf /v/* && tar xzf /b/endora-core-data.tgz -C /v'
# the same for endora-core-hq-data and endora-audio-data
docker compose start
```

Restoring the online copy instead: stop the stack, put the copy in `endora-core-data` as
`endora.db` (remove any `endora.db-wal` and `endora.db-shm` beside it) with the matching
`secrets.key`, then start.

## Upgrade and rollback

Each product moves on its own, by its version in `.env`:

| Product | Setting | Its release notes |
|---|---|---|
| Core | `ENDORA_CORE_VERSION` | this repository's CHANGELOG |
| Console | `CONSOLE_TAG` | Console's repository |
| Audio | `ENDORA_AUDIO_VERSION` | Audio's releases |

```sh
# back up first (above), then:
docker compose pull && docker compose up -d --wait
```

**Core started before the project-named network.** Its team network was `endora-core-team` and is
now `<project>_team`. Once, run `docker compose down` (without `-v`: the volumes, and so the data,
stay) and then `docker compose up -d --wait`.

**Rollback** is the same with the previous version. Core's database migrations run forward on
start, so an older Core may not open a database a newer one has migrated: roll Core back
together with the backup taken before the upgrade. Console holds no data and works with any
Core of the same contract major (it refuses another, saying so). Audio's state file is pinned
within v1, so its rollback keeps settings.

**The bundle's pins.** The bundle's `console/` and `audio/` folders are those products' own
compose files, copied verbatim at the commits in `SOURCES.json`; `check-sources.sh` proves they
are unchanged (`--remote` also compares them with their sources). Each product's version is
set in one line of `bundle.env.example` (`CONSOLE_TAG`, `ENDORA_AUDIO_VERSION`), which the check
holds equal to its pin. Moving the bundle to a newer product release is that line, the
release's compose file in place of the old one, and its `SOURCES.json` entry, checked together
by `check-sources.sh` and the bundle's CI run.

## How it is tested

`.github/workflows/full-system.yml` runs on a plain ubuntu runner for every change to Core's
deployable or the bundle: it builds Core from the commit, brings the bundle up with a stub model
host (`deploy/compose/ci/`), and `deploy/compose/test-bundle.sh` checks each product's health,
Core's API 1.0, Console serving and passing only the contract (and Core refusing what is not
signed), Audio's health, and `endora-core setup` followed by a sign-in through Console, then
takes everything down. Each check is a step of
[`features/full-system-bundle.feature`](../deploy/compose/features/full-system-bundle.feature).

## Not in the bundle

- **OpenBao** is not part of Endora: Core keeps its own secrets
  ([ADR 0144](adr/0144-endora-keeps-its-own-secrets.md)).
- **Endora Home's watch** watches the house from outside, on another machine, as a systemd job,
  not a container. It is installed by its own role (source on the `feat/house-watch` branch,
  to move to `endora-home`).
- **The owner's home lab**: the root `docker-compose.yml`, `make deploy`, `release.sh` and the
  Ansible collection stay the owner's path ([hosting.md](hosting.md), [homelab.md](homelab.md)).
