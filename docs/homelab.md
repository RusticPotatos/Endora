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
private SQLite database, sealed under the node's secrets key (below); run setup with
the same `ENDORA_SECRETS_KEY_FILE` as the node, if you set one. No keys are accepted
through command arguments.

## Secrets at rest (ADR 0125)

Every credential the node stores (provider API keys, MCP bearer tokens and environments,
the Home Assistant token and every other capability setting, the authenticator secret and
the bootstrap token) is sealed in the database with a data key, and that key is wrapped by
the **secrets key**: `secrets.key` beside the database (`/data/secrets.key` in the
container), or wherever `ENDORA_SECRETS_KEY_FILE` names. The node makes it, owner-only, on
its first start; on that start it also seals what an older node stored in plaintext and
says `secrets: sealed N stored credential(s) at rest`. Sign-in sessions are kept only as a
digest; a console session ends after thirty days, or after seven unused.

**Back it up apart from the database.** Either one alone opens nothing; the two together
open everything. So:

1. Keep a copy in your password manager, as text:

   ```sh
   docker compose exec node base64 /data/secrets.key
   ```

2. Exclude `secrets.key` from the house's backup of the data folder (the nightly
   `backups/daily-*.db` copies are beside it and stay included), the way the vault
   password is kept out of the checkout's backup.
3. On a restore, put it back beside the database before the node starts (as `0600`):
   `base64 -d > secrets.key` from the copy.

A node whose database holds sealed secrets and whose key file is missing will not start,
and says so: restore the file. If no copy survives, `endora-node secrets forget
--i-have-lost-the-key` empties every sealed credential and the password so the node can
start with a new key; sign in again with the bootstrap token from the log and enter every
key and password again. `endora-node secrets rekey` seals everything again under a new
data key (the key file stays the same).

This protects copies of the database: backups, snapshots, a disk that leaves the house. It
does not protect a running host — anything with root on the NAS, which includes any
container holding the Docker socket, can read the key and the database together.

Sonarr and Radarr use `/api/v3/queue`; give their origin without a path or query.
Their keys permit broader operations, but this adapter only sends fixed GETs.
TLS uses the existing HTTP library's verified trust roots; do not disable TLS.
HTTP is possible on a trusted private network, but does not encrypt keys.

## The vault (ADR 0127)

OpenBao keeps the house's secrets, beside the node, and locks itself on every restart. You
unlock it with one passkey tap in the console.

### Setting it up

1. **Run it.** In your ansible-nas, after the Endora node's role (it makes the network the two
   share), add the `endora.homelab.openbao` role and these variables, then run the playbook:

   ```yaml
   openbao_enabled: true
   openbao_data_dir: "{{ docker_home }}/openbao"
   endora_vault_url: http://openbao:8200
   ```

   For `make deploy` from your Mac, put `ENDORA_VAULT_URL = http://openbao:8200` in `local.mk`
   too, so both ways of starting the node agree. Check it: `docker ps` shows `endora-openbao`
   healthy, and the node's log says `vault: OpenBao, unlocked with a passkey for
   endora.home.arpa`. It uses about 20 MB (capped at 256 MB).
2. **Open the console by its name**: `https://endora.home.arpa:8787`. Passkeys are bound to that
   name; opened by the NAS's address, the Vault screen says so and offers nothing.
3. **Settings › Vault › Add a passkey.** Your iPhone (iOS 18 or later) offers to save a passkey
   for Endora; Face ID. A passkey that cannot do PRF is refused with the reason.
4. **Initialise the vault**, one more tap. You are shown **three break-glass shares, once**:
   save them now, in your password manager, or on paper apart from it. Any two of them open the
   vault if every passkey is lost; Endora never shows them again.
5. **Add a second passkey** (an iPad, a security key): Add a passkey asks your phone first,
   then the new one twice. Any registered passkey then unlocks on its own.
6. **When you are ready, Endora's own keys** (folded under the passkeys): moving them into the
   vault means a copy of the NAS's disk opens nothing without the vault, and that **after every
   restart Endora waits for your tap**. It cannot be undone from the console.

### After a reboot

The console shows **Vault locked → Unlock**; tap it and Face ID. If Endora's own keys are in
the vault, the whole console is the unlock screen until you do, before sign-in.

### If something is lost

- **A passkey** (a lost phone): remove it under Settings › Vault from another device, and add
  the new phone. The last passkey that can unlock cannot be removed.
- **Every passkey**: on the NAS, unlock with two break-glass shares,

  ```sh
  docker exec -it endora-openbao bao operator unseal -address=http://127.0.0.1:8200
  ```

  twice (it asks for a share each time, without echoing it), restart the node if it was
  waiting (`docker restart endora-node`), sign in, and under **Settings › Vault › Lost every
  passkey?** give two shares and a new passkey. The vault makes three new shares — save them;
  the old ones stop working — and only the new passkey can unlock until you add others.
- **The vault's data**: restore `openbao_data_dir` from the backup; your shares and passkeys
  still fit it. If it is gone for good and Endora's keys were not in it, **Start over** (folded
  on the Vault screen) forgets this node's copy so a new vault can be initialised. If Endora's
  keys were in it, they are gone with it: `endora-node secrets forget --i-have-lost-the-key`
  (above) is the way out.

Back up `openbao_data_dir` with everything else; keep the break-glass shares apart from it,
the way the secrets key is kept apart from the database.

## The AirPlay 2 door (ADR 0128)

"Endora Mixer" as an **AirPlay 2** receiver, so an Apple TV casting into it holds its picture
back to match the sound (lip-sync). It runs in its own container, `endora-airplay2`, on a LAN
address of its own (a macvlan network), because its clock (nqptp) needs UDP 319/320, which the
senders' clock already holds on the NAS. Without it, the classic door in the senders' container
works as before, about two seconds behind the picture.

### What you need, once

1. **The NAS's LAN interface.** On the NAS: `ip -br link` lists the interfaces; the one that is
   `UP` and carries the NAS's LAN address (check with `ip -br addr`) is the parent, often
   `eth0`, `enp3s0` or `eno1`. Not `docker0`, a `br-…` or a `veth…`. If the NAS's address is on
   a bridge it made itself (say `br0`), use the bridge.
2. **The LAN** as CIDR (`ip -br addr` shows it, for example `/24` after the address) and its
   **gateway** (`ip route show default`, after `via`).
3. **A free address outside the router's DHCP pool.** In UniFi: *Settings › Networks › (your
   LAN) › DHCP*, note the range; pick an address on the LAN outside it that nothing uses
   (`ping` it first, from another machine). After the first start, UniFi lists a new client
   with the door's MAC (default `02:00:00:00:a2:01`, a locally administered one): give it a
   **fixed IP** of the same address and a name, so DHCP never hands it out. Docker hands out
   only that one address.

### Turning it on

In `local.mk`, with your values (never in git):

```make
SOUND_AIRPLAY2 = 1
SOUND_AIRPLAY2_PARENT = eth0
SOUND_AIRPLAY2_SUBNET = <your LAN, e.g. a /24>
SOUND_AIRPLAY2_GATEWAY = <your router>
SOUND_AIRPLAY2_IP = <the free address>
# Optional:
# SOUND_AIRPLAY2_MAC = 02:00:00:00:a2:01
# SOUND_AIRPLAY2_HOSTNAME = endora-mixer
# SOUND_AIRPLAY2_LATENCY = 0.5
# SOUND_AIRPLAY2_NETWORK = <an existing macvlan network on the LAN>
```

Docker allows one macvlan network per LAN subnet. If another container already has one (a
Pi-hole on its own address, say), `make deploy` fails with "failed to allocate gateway …
Address already in use": run `docker network ls --filter driver=macvlan` on the NAS and set
`SOUND_AIRPLAY2_NETWORK` to that network's name, and the door joins it with its own address.

Then `make deploy`. It adds the `airplay2` profile, builds the door's image on the Mac for the
NAS, and the senders' container stops running the classic door (it restarts once, which
interrupts any HomePod playing for a few seconds). `make deploy` refuses to start if one of the
four network values is missing. With the `endora.homelab` collection instead: in the `endora`
role, `endora_sound: true`, `endora_sound_airplay2: true` and the four
`endora_sound_airplay2_*` values (its README).

Check it: `docker logs endora-airplay2` says `1 AirPlay 2 receiver`, and on an iPhone or the
Apple TV, "Endora Mixer" is in the AirPlay list. `ping` to the door's address works **from
another machine but not from the NAS**: a macvlan address cannot be reached from its own
host, which nothing needs (the door and the hub share only the pipe).

### Tuning lip-sync

`SOUND_AIRPLAY2_LATENCY` (seconds, 0 to 2, default 0.5) is how early the door hands its sound
to the hub, to make up for the mix and the speaker after it. Play a video with speech from the
Apple TV into the door, listen on the speaker you watch with, and:

- sound **late** (lips move first): raise it by 0.1;
- sound **early**: lower it by 0.1;

then `make deploy` again (only the door restarts). If `docker logs endora-airplay2` warns that
the offset is "too short" or "out of range", the Apple TV's stream does not arrive far enough
ahead for that much: lower it. Upstream recommends staying within about 0.25 s; past that, it is
worth listening for. A phone on the live stream can be lined up this way; the HomePods, through
the senders' own AirPlay buffer, are seconds further behind and are for music.

### If it is not found, or will not play

- **Not in the AirPlay list**: the router or a Wi-Fi bridge may not pass mDNS from a second MAC
  on the NAS's port. In UniFi, check the client appears; turn on *Multicast DNS* for the
  network if devices are on another VLAN. Some switches and powerline or Wi-Fi bridges allow one
  MAC per port: then use ipvlan instead (in `docker-compose.yml`'s `airplay2-lan` network,
  `driver: ipvlan` and `ipvlan_mode: l2` under `driver_opts`), which shares the NAS's MAC.
- **Found but silent**: `docker logs endora-airplay2`; the hub must be running (`sound`
  profile), since the door's pipe has no reader without it.
- **Back to classic**: `SOUND_AIRPLAY2 = 0` (or remove the line) and `make deploy`; then
  `docker rm -f endora-airplay2`, since a profile that is off is not stopped by Compose.

## The wing's deploy key (ADR 0115)

The engineering wing's branches reach your ansible-nas fork only when you approve a push on the
HQ screen, and then the node pushes them with a **deploy key** that can write to that one
repository. Without one, the desk offers each accepted branch's bundle to download instead, and
nothing is lost. To set it up:

1. On the Mac, make a key for this alone, with no passphrase (the node runs unattended):

   ```sh
   ssh-keygen -t ed25519 -f ./endora-git-push -C "endora-hq deploy key" -N ""
   ```

2. On GitHub, open the fork → Settings → Deploy keys → Add deploy key: paste
   `endora-git-push.pub`, tick **Allow write access**, and name it for Endora. A deploy key
   belongs to one repository, which is the point: it cannot reach anything else of yours.
3. Protect the branches the team must never move: Settings → Branches → add a rule for `main`
   and for `develop` (at least "Require a pull request before merging"). Endora's push code
   refuses anything but `hq/*` already; the rule makes GitHub refuse it too.
4. Put the private key on the NAS in a folder of its own, as one file called `key`, readable by
   the node alone (for example `/srv/endora/git-push-key/key`, mode `0600`), and in `local.mk`:

   ```make
   ENDORA_GIT_PUSH_KEY = /srv/endora/git-push-key
   ```

   `make deploy` mounts the folder read-only at `/git-push-key`; the node reads
   `ENDORA_GIT_PUSH_KEY_FILE=/git-push-key/key`. Delete the local copy of the private key once
   it is there.
5. The push trusts GitHub's published SSH host keys (baked into the node, from
   `https://api.github.com/meta`) and nothing else. If GitHub ever rotates them, the push says
   so and refuses; update the keys in `domains/capabilities/src/repo.rs`.

Then, when a card says "Push hq/… to your fork", Approve pushes that branch; you merge it on
GitHub when you are ready. HQ never opens a pull request and never merges.

## The bench's credential (ADR 0123)

The bench measures an external agent harness against HQ's wing on one task, in the Pi's
sandbox. Its seats reach Claude only through the node's relay, with a token of the bench's
own that opens the relay alone (Haiku or Sonnet, never Opus), at most `ENDORA_BENCH_CAP_CENTS`
a day ($2.00 unless set), metered in the node's usage log as `bench` and never charged to HQ.
It is off until you create its file, and off again the moment you delete it. To turn it on:

1. On the NAS, make a folder of its own and a random token in it, readable by the node alone:

   ```sh
   sudo install -d -m 0700 /srv/endora/bench-token
   openssl rand -hex 32 | sudo tee /srv/endora/bench-token/token >/dev/null
   sudo chmod 0600 /srv/endora/bench-token/token
   ```

   (Any folder works; `/srv/endora/bench-token` is only an example.)
2. In `local.mk`, name the folder, and the day's cap in cents if not $2.00:

   ```make
   ENDORA_BENCH_TOKEN = /srv/endora/bench-token
   # ENDORA_BENCH_CAP_CENTS = 200
   ```

3. `make deploy`. The node mounts the folder read-only at `/bench-token` and says
   `bench: a credential for the model relay alone, Haiku or Sonnet, $2.00 a day` at start.
   It never prints the token.

The token is read from that file when the bench runs, passed into the bench's container and
nowhere else; never paste it into a chat, a file in git or an image. What the bench has spent
today: `GET http://<the NAS>:8788/team-relay/bench` with the token as `Authorization: Bearer`.
To turn it off: delete the file (no restart needed), and remove the line from `local.mk`.

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
