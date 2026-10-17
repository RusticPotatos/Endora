# 0104 — Endora sound

## Status

Accepted, 2026-10-16, by the owner through a design interview (questions Q1–Q46). Prepared
with AI assistance (Claude). The design is [Endora sound: the design](../plans/endora-sound.md).
Nothing is built yet.

## Context

The owner wants Spotify Connect for any sound. In one direction, many of their own devices
(the phone, the tablet, Spotify, a radio station, Endora's voice) play together into the
device they are on. In the other, one source spreads to several speakers at once, the
HomePods and the Mac together. The uses are music, audiobooks and Endora's voice. Video
plays natively from the device and is out of scope, because a relay cannot keep lip sync.

The home lab already runs stock OwnTone (the ansible-nas `owntone` role) with a librespot
Spotify receiver, and that sends AirPlay 2 to the HomePods in sync. OwnTone cannot do the
rest. Its player has exactly one playback session, with one queue, one input and one clock.
Making it hold several is a rewrite, not a patch: `player.c` alone is about 4,000 lines on
file-scope state. It plays one source at a time, so it cannot mix two at different levels.

## Decision

Endora gets a sound hub, **endora-sound**: the owner's own program, not a fork.

- **Sources, speakers and wires.** A wire joins one source to one speaker and has its own
  level (0–100, independent, with a limiter on the sum). Each speaker also has its own
  volume. A source can set all of its wires at once. Endora's voice ducks the other wires.
  A device is never wired to itself. There are no modes and no sessions.
- **Groups are derived.** Speakers with identical wires form one group and get one synced
  stream. Named setups save sets of wires and can run on a schedule.
- **It steps aside.** When another device takes a speaker natively (the Mac playing a video
  to a HomePod), the hub marks that speaker borrowed and does not fight for it. It restores
  the speaker's wires when the speaker is free.
- **Built in Rust in this repository**, at `app/sound`, Apache-2.0, in its own container.
  It decodes and encodes with `ffmpeg`, mixes one stream per group, serves the streams, and
  has its own HTTP API. It works without Endora.
- **Stock OwnTone only sends.** A pool of four unmodified OwnTone instances sharing one
  `airptpd` acts as AirPlay 2 senders, each limited to the speakers of the group it plays.
  We fork OwnTone only if its AirPlay code itself must change.
- **Classic AirPlay receivers, on the NAS's own address.** One shairport-sync receiver per
  device ("Endora · iPhone", "Endora · iPad", a guest "Endora"), built for classic AirPlay,
  in the senders' container. *Changed 2026-10-17, by the owner:* AirPlay 2 receivers were the
  plan, but their `nqptp` and the senders' `airptpd` both need UDP 319/320, which would have
  meant a container on a LAN address of its own; classic AirPlay needs no PTP clock, so it
  shares the host, with the same sound quality and delay. Moving to AirPlay 2 later changes
  nothing in the hub: both write the same pipes.
- **Endora is the remote.** A Speakers screen in the console shows the wires and changes them
  by tapping. Chat gets a fixed set of actions: apply a setup, set a level or a volume, play
  a station, say something on speakers, and stop a source. The model proposes; the hub's API
  is the only thing that acts, under the existing autonomy and quiet-hours settings. Away
  from home the hub is reachable only through Endora's sign-in and tunnel.
- **Deployed** as a `sound` role in `endora.homelab`, built on the Mac. The ansible-nas
  `owntone` role stays as the fallback until it is retired.

## Changes found by building it (2026-10-17)

- **The old OwnTone retires.** The senders cannot run beside the ansible-nas `owntone`
  container (both take the AirPlay clock's ports and the speakers), and once Spotify plays
  into the hub through a receiver of its own ("Endora", librespot) and the senders reach the
  HomePods, nothing it does is left: the owner chose to retire it.
- **Spotify is a hub source,** through a Spotify Connect receiver called Endora writing into
  the hub's pipe. The Music screen lists the hub's speakers beside Spotify's own devices.
- **OwnTone must not reconnect to a speaker by itself** (`reconnect = false` per speaker): in
  the first live test it took a HomePod back from the owner's Mac five seconds after the Mac
  took it. The hub alone takes a speaker back, when the owner says.
- **Every pipe is read with backpressure:** librespot decodes faster than real time and relies
  on its output to hold it back.

## Consequences

- One mix per group, several groups at once: an all-day radio station in the kitchen at 60%,
  and the same station at 10% under the phone on the office speakers.
- Endora gets a voice that plays on speakers (Kokoro, already running on the NAS GPU), and
  chat can control sound only through actions the hub defines.
- A third service, and a fourth container, to deploy and keep healthy. The receivers need an
  address the owner reserves in UniFi.
- Phone, tablet and handheld outputs play an HTTP MP3 stream and are not in sync with the
  AirPlay speakers.
- OwnTone's GPL code is run, not linked or copied. endora-sound talks to it only over its
  JSON API and pipes, and stays Apache-2.0.
- The first slice must test three things nobody has run yet: handing a speaker between two
  OwnTone instances, the Mac's "Allow?" prompt as an AirPlay speaker (OwnTone issue #1385),
  and stepping aside for native AirPlay.

## Alternatives considered

- **Fork OwnTone and add a mixing input.** Rejected once wires had levels. A per-group mix
  does not fit a single-session player, and the fork would carry a GPL codebase that has
  one core maintainer.
- **Write AirPlay 2 sending from scratch.** Rejected: it is undocumented, encrypted and timed
  to the millisecond. Stock OwnTone already does it, including the HomePod workarounds.
- **Snapcast, PipeWire, Liquidsoap or Music Assistant's AirPlay player.** Each does part of
  this. Snapcast syncs its own clients but does not mix. PipeWire mixes but sends only
  AirPlay 1. Liquidsoap mixes but has no AirPlay output. Music Assistant routes one source
  per player. None mixes several sources into several AirPlay 2 groups.
- **Sessions (a speaker in one session at a time).** Rejected: a session cannot put two
  sources on one speaker at different levels.
- **Video through the hub with a tuned offset.** Rejected: sync through a relay depends on
  the app and drifts. Native AirPlay from the device already keeps lip sync.
