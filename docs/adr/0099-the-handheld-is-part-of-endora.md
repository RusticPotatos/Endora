# 0099 — The handheld is part of Endora

## Status

Accepted, 2026-10-16, by the owner through a design interview (questions 1–3, 9, 19 and 21,
recorded in [the unification plan](../plans/unification.md)). Prepared with AI assistance
(Claude); the terminal itself was written with AI assistance (OpenAI Codex).

## Context

The handheld terminal (`endora-terminal`, a Rust TUI for a small Linux handheld) was built in a
repository of its own, never committed and never published, beside the node it talks to. It
imported nothing from Endora on purpose: it talks to the node over HTTP, and the node owns
models, integrations, authorization and persistence. But both sides spoke untyped JSON, the
server building objects field by field and the terminal reading fields by name, so a renamed
field broke the handheld with every check on both sides still green. It also did things the web
console could not (media requests, container controls, jobs, Spotify), and had no licence.

## Decision

- **One repository.** The terminal lives at `clients/terminal`, a member of the Cargo workspace:
  one lockfile, one CI, one changelog, one branch for a change that touches both sides. Its
  Python checks, its privacy check and its rendering checks run in Endora's CI, and the ARM64
  bundle is built there as an artifact. It is never built on the handheld.
- **Still a client.** It imports no server crate. The boundary is the HTTP API, and the only
  code both sides share will be a small protocol crate of wire types (`/v1` requests and
  responses) that the server serializes and the terminal deserializes, so a change to the wire
  fails the build on both sides at once. Device-only code (Wi-Fi, Bluetooth, sound, power,
  shell, browser) stays in the terminal.
- **Apache-2.0**, like the rest of Endora.
- **The web console is the full set; the handheld is a subset** for a smaller device. A server
  feature is built once and offered to both.
- **Endora's Ansible roles ship from this repository** as one versioned collection that the
  owner's ansible-nas pins, rather than living in the ansible-nas fork.

## Consequences

- The terminal's history starts here; the files were imported as they stood, after the privacy
  checks of both projects passed over exactly the files imported (its private notes and
  `privacy.local` stayed out).
- `cargo test --workspace` now builds a terminal client too: more to compile, and its
  dependencies (ratatui, crossterm, reqwest) join the lockfile.
- The terminal's build output stays in `clients/terminal/target` for packaging.
- Endora's personal-data check now covers the terminal's files, and learned that a systemd unit
  instance (`getty@tty1.service`) is not a mailbox.
- **The protocol crate is `shared/protocol` (`endora-protocol`).** It holds the handheld's page
  (`ConsoleView`), home screen (`Dashboard`), action outcome (`Operation`) and chat stream event
  (`StreamEvent`), and one sample of each. The node passes each response through its type and
  refuses one that does not fit; the console's tests prove its builders fit; the terminal reads
  stream events as the type and its tests render every sample. Renaming a field fails both
  sides' tests. Writing it down found a mismatch at once: the terminal waited for a `progress`
  event the node never sends and ignored the `step` and `error` events it does; it now shows
  both.
