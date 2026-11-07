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

## Alternatives considered

- **Profiles in the root `docker-compose.yml`** (what existed): one file for every product,
  building from source with `make`: a monolith and a second orchestrator in all but name.
- **An install script or a new orchestrator:** more to maintain than Compose's own `include:`,
  which the products were already shaped for.
- **Remote `include:` of each product's repository:** no vendored copies, but not supported by
  every Compose a user has, and a moving branch would change an installed system silently.
- **Ansible as the only path:** right for the owner's lab, too much for one ordinary machine.
