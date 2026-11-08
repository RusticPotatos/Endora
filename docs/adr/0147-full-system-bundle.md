# 0147 - Core's standalone configuration and the full-system bundle

## Status

Accepted, 2026-11-07, by the owner (P1 step 3 of the product reset: "a full-system install on
one ordinary Linux host"). AI-assisted (Claude). Builds on [0145](0145-core-console-contract.md)
and [0146](0146-console-forwards-cores-session.md). The guide is [install.md](../install.md).

## Context

The owner's deployment principle (7 Nov): *someone can easily deploy the full system,
individually or all.* Its criteria: each runtime product ships its own compose file with
published images; Core gets a complete supported configuration with the runner's trust boundary
and limits explicit; one documented, tested, version-pinned bundle reuses the products' files
through Compose `include:`, not a second orchestrator; public users never need the NAS, the Pi,
`local.mk` or `release.sh`; one-time bootstrap is described honestly.

Before this, Core ran only through the repository's root `docker-compose.yml`, built on the host
by `make deploy` from `local.mk`, with the runner on a Pi. Audio (v1.0.0) and Console already
shipped include-ready compose files with published images; Core published no image at all, and a
first owner was made only through a fifteen-minute claim window in the browser.

## Decision

1. **Core's own compose file** is `deploy/compose/core/compose.yaml` with a documented
   `core.env.example`: the node, HQ, and the runner behind the `workers` profile, off unless
   named. The runner's trust boundary (Docker's socket is root on the host; workers fenced on an
   internal network, read-only, no capabilities, 256 MB / one CPU / 128 processes, 30 minutes)
   is written in the file and the guide. Volume and network names are fixed, so the same data
   serves Core alone and Core in the bundle. The root `docker-compose.yml` stays the owner's
   development and home-lab file.
2. **`endora-core setup`** makes the owner on first run, from a shell on the host: a password
   from stdin and an authenticator secret, printed once. It refuses to replace an owner and
   provisions nothing else. The image names the node's binary `endora-core` for it.
3. **The bundle** is `deploy/compose/compose.yaml`, in Core's repository (no new repository). It
   `include:`s Core's file and **verbatim copies** of Console's and Audio's own files, each
   pinned in `SOURCES.json` (repository, commit, SHA-256, image version) and proved by
   `check-sources.sh`. Copies, not remote includes: Compose's git and OCI includes are not
   available everywhere. Console is pinned to its `sha-` build (it has no tagged release yet);
   Audio to its latest release, v1.0.1. Each product's version is set once, in
   `bundle.env.example`, and `check-sources.sh` holds it equal to the pin.
4. **One origin, Console in front.** In the bundle Console takes the public port (8787) and Core
   is bound to the host's loopback, plain HTTP on the bundle's network; Console serves HTTPS with
   the owner's certificate or sits behind their proxy. Console still holds no authority: Core
   authorizes every request.
5. **Products stay optional.** Removing a product's `include:` line removes it; Audio's AirPlay
   door keeps its own macvlan option and joins no other product's network. OpenBao is not part
   of Endora; Home's watch is a systemd job elsewhere, out of the bundle.
6. **Core's image is published** as `ghcr.io/rusticpotatos/endora-node` (the name kept, not
   renamed) from develop, main and release tags (`image.yml`): `sha-<commit>`, `latest` on main,
   `X.Y.Z` on `vX.Y.Z`.
7. **Tested, not illustrative.** `full-system.yml` builds Core from the commit and runs
   `test-bundle.sh` on a plain ubuntu runner with a stub model host: every product's health,
   Core's API 1.0, Console serving and passing only the contract, Core refusing what is
   unsigned, Audio's health, and `endora-core setup` then a sign-in through Console. Each check
   is a step of `deploy/compose/features/full-system-bundle.feature`, and a step not passed
   fails the run.

## Consequences

- A new user installs Core, or everything, with Docker alone; the guide counts the bootstrap
  steps that remain theirs (owner, model and keys, Audio's token, the runner's opt-in).
- Moving the bundle to a new Console or Audio release is a copy of that release's file, a new
  `SOURCES.json` entry and a green bundle run. Only the current set is tested (principle 7).
- The bundle's Core pin is the checkout's own release; until Core's first published release,
  the guide says to build the image.
- Setup found and fixed a real defect: making the node's token rewrote the whole sign-in row,
  so an owner made before the node's first start was erased by it.

## Amendment, 8 Nov 2026: containers and networks follow the project

The owner's full-system install on a plain Linux host (relayed 8 Nov; the report was not read
here) brought the documented `docker compose up` to a failure on an existing bundle: Core's team
network had a fixed name, `endora-core-team`, which a Core started alone already held. Decision 1's
"network names are fixed" is changed:

- Core's team network is no longer named outright. Compose names it `<project>_team`
  (`endora-core_team` alone, `endora_team` in the bundle), and the runner is given that name by
  Compose's own project name (`TEAM_NETWORK: ${COMPOSE_PROJECT_NAME}_team`). Core's containers
  already followed the project.
- Volume names stay fixed, so an upgrade keeps the data (decision 1's reason stands).
- Console's and Audio's files, vendored unchanged, still fix their container names and Audio's
  AirPlay network. Two bundles on one host therefore need those files changed in their own
  repositories and a new pin; until then the guide says one bundle per host. The check
  `deploy/compose/check-isolation.sh` names them as known limits.
- Also: the bundle's `ENDORA_CORE_VERSION` is set to a published build (it was empty, which
  made the example fail to render), the guide's owner setup has a scripted form with `-T`, and
  the cold backup hands its tarballs back to the user instead of root.

## Amendment 2, 8 Nov 2026: a second install is Core alone, with its own settings

The owner asked that concurrent installs on one host be isolated by project name. Checked against
the files (offline renders with `docker compose config`, 8 Nov 2026):

- **Volume names stay fixed.** Dropping the fixed `name:` would render Core's volumes as
  `endora-core_endora-core-data` and so on: a standalone Core upgraded to it would open an empty
  database and ask for a new owner, beside its old data. That breaks existing installs, so the
  defaults stay. A second install sets the `*_VOLUME` variables instead.
- **The checker was incomplete.** It compared networks and containers only, so two bundles with
  the defaults sharing Core's four volumes passed. It now compares containers, networks, volumes
  and published host ports. A second Core with the defaults shares four volumes and port 8787
  with the bundle (red, shown on a copy of the folder before this change).
- **The second install is Core alone**: `core/second.env.example` (its project name, its four
  volumes, port 8796). Rendered beside the bundle and beside Core alone, it shares no container,
  network, volume or host port with either (`deploy/compose/check-isolation.sh`). Core's built-in
  screens stand in for Console; Audio is left out, since its senders and Spotify use host
  networking.
- **Two full bundles on one host stay unsupported**, and the guide says so. The checker lists the
  names they share as KNOWN LIMIT. Naming cannot fix Audio's host-network ports (3701-3704,
  UDP 319/320), so renaming Console's and Audio's containers would not make two bundles run.
- **Not verified here: runtime.** This working host also runs the owner's production Endora under
  fixed names, so a test of two running installs here would touch production. A rootless Docker daemon
  for a sandbox needs `newuidmap` (root to install) and there is no passwordless sudo. The
  runtime test of two Cores is open until a sandbox daemon is available.

## Amendment 3, 8 Nov 2026: what two full bundles still share, and the staged plan

Checked offline (`docker compose config`, 8 Nov 2026: nothing started, no daemon contacted). A
second full bundle with every documented override set (Core's and Audio's ten volume variables,
the three host ports, `SOUND_AIRPLAY2_NETWORK`) still shares, with the first:

- **Container names fixed in Console's and Audio's own files**: `endora-console`, `endora-audio`,
  `endora-audio-spotify`, `endora-audio-senders`, `endora-audio-airplay`. Those files are pinned
  verbatim by SHA-256 (`SOURCES.json`), so this repository cannot rename them without breaking the pin.
- **Two host-network services**, `audio-senders` and `audio-spotify`. Compose shows no ports for
  them. Their ports belong to the host (the AirPlay clock's UDP 319/320, the OwnTone APIs on
  3701-3704), so naming cannot give two of them one host.

Nothing else is shared: volumes and ports are isolated by their variables today. Two gaps in the
check were closed first, test first (`check-isolation.sh` failed on both before the change): it did
not render the AirPlay door, so `endora-audio-airplay` was never checked, and it listed no
host-network service. A second bundle with one override left out (a volume, a port) is caught by a
negative control, so the override test cannot pass by accident.

**Staged plan** (nothing below is started; each stage is its own change):

1. **Console and Audio names** (their repositories, not this one): read each container name from a
   variable whose default is today's name (`CONSOLE_CONTAINER_NAME`, Audio's five), so standalone
   installs keep their names. Release, then here: a new `SOURCES.json` entry and SHA-256, the
   variables in `bundle.env.example`, `check-sources.sh --remote`, and a green `full-system.yml`.
   The checker's known-names rule then needs the new names only where a name is still fixed.
2. **Audio on the host's network, twice on one host** (owner's decision, its own ADR if taken).
   Options: keep one full bundle per host (the current, documented limit); or move each install's
   senders to an address of its own, as the AirPlay door already does. The second needs a runtime
   proof on the XPS. Recommended: keep the limit until stage 1 is done and the XPS run is in.
3. **Charter milestone c, the single clean-host install**: not changed by this amendment. It still
   needs the XPS run from scratch with no overrides. Not verified here: whether a stock Ubuntu host's
   own avahi daemon (UDP 5353) conflicts with the senders' avahi, which uses host networking.

## Amendment 4, 8 Nov 2026: the isolation check is a required part of the handoff gate

Amendments 1 to 3 landed on `develop` and were reverted whole (Director ruling, 8 Nov 2026), together
with a candidate that pinned Console and Audio to unreleased scratch-branch commits so their container
names could be variables. This change lands the Core side again and leaves the pins alone:
`SOURCES.json` still names Console's `fd9cf17` and Audio's `v1.0.1`, both published, and each pin
still matches its source (`check-sources.sh --remote`). Container names in those two files stay
fixed, so two full bundles on one host stay unsupported, as amendment 3 says; stage 1 waits for
released Console and Audio files.

What was missing was a gate that fails when the isolation check is not run. A green run of the other
checks said nothing about it. `scripts/check-required.sh` runs the checks in `scripts/required-checks.txt`
and exits nonzero for a check that is missing from the list, whose script is not there, that fails,
or that exits 0 without printing its success marker (a stub). `scripts/gate.sh` calls it with the
names it will not run without, so removing a line from the list fails the gate instead of shrinking it.
`scripts/test-check-required.sh` holds it to that with a negative control for each way a check can be
absent. Not verified here: a runtime install. The check renders models with `docker compose config`
and starts nothing; the clean-host run of milestone c is still open.

## Alternatives considered

- **Profiles in the root `docker-compose.yml`** (what existed): one file for every product,
  building from source with `make`: a monolith and a second orchestrator in all but name.
- **An install script or a new orchestrator:** more to maintain than Compose's own `include:`,
  which the products were already shaped for.
- **Remote `include:` of each product's repository:** no vendored copies, but not supported by
  every Compose a user has, and a moving branch would change an installed system silently.
- **Ansible as the only path:** right for the owner's lab, too much for one ordinary machine.
