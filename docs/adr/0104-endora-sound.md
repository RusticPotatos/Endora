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
  device ("Endora Mixer (iPhone)", "Endora Mixer (iPad)", a guest "Endora Mixer"), built for classic AirPlay,
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
- **One general AirPlay door (2026-10-18, by the owner).** A device's own door ("Endora Mixer
  (iPhone)") could not play back on that device (the loop guard), so Apple TV casting to the
  iPhone's door and the owner listening on the iPhone gave silence, and nothing said why. The
  hub now has one door, "Endora Mixer", of no device, made once at start (idempotent). Any
  device casts into it and it can be wired to any speaker, phones included. The console no
  longer makes per-device doors; at start the hub merges existing ones into the general door:
  each wire moves to the general door's same speaker (the higher level where two meet, also
  in saved setups) and the device doors go. The loop guard stays for a device's own source;
  the general door has no device, so a device wired to it hears what it casts in, which the
  card says. Removing a speaker removes its wires and nothing else. The receivers list
  changes, so the senders' container restarts its receivers (and senders) once.
- **Live mode for phones and browsers (2026-10-18, by the owner).** A phone listening to
  its speaker through the MP3 stream is about seven seconds behind a television: the
  browser buffers the MP3 for seconds (4.7 s measured in a desktop Chromium), and classic
  AirPlay adds about two more. The owner chose to keep that stream, for music and for playing
  with the screen locked, and to add a **live** one beside it.
  - *The stream.* `GET /v1/live/{speaker}` on the hub (and `/v1/sound/live/{id}?ticket=` on
    the node, under the same one-minute ticket as listening, which now also returns a `live`
    URL) sends the speaker's mix as raw PCM in 20 ms frames: a 24-byte header (`EL`, flags,
    channels, a wrapping sequence number, the hub's clock in microseconds at the frame's
    first sample, the sample rate, the byte count) and 16-bit little-endian stereo. No
    encoder, no decoder in the browser, one chunk per frame, `TCP_NODELAY` on the hub and
    on the node's plain-HTTP listener. It is 44.1 kHz (what the mixer runs at), 1.4 Mbit/s,
    not 48 kHz: resampling in the hub for nothing was not worth it. A listener that falls
    more than about a second behind skips ahead, and the sequence numbers say what it missed.
    A fetch stream was chosen over a WebSocket: it passes through the node's existing
    streaming proxy and ticket gate unchanged, and needs no new dependency.
  - *The player.* An AudioWorklet built from the same pure functions the console check tests.
    Its buffer starts at 300 ms, grows by half when it runs dry and by a quarter when frames
    go missing (up to 1 s), and shrinks 25 ms for each 5 s without trouble; a half-percent
    nudge to the playing speed brings the buffer to its target, and a big surplus is skipped
    after a 5 ms fade. At every jump, and before every dry spell, the gain fades over 5 ms.
  - *The mode.* **Auto** is live when an AirPlay source (the door) is wired to this speaker,
    listening otherwise; the owner may pin **Live** or **Listening** on the speaker's card
    (kept in the browser). When the page goes to the background the page plays the MP3 in its
    `<audio>` element, which is what plays on with the screen locked, and takes live back when
    the page returns. The element is unlocked by playing a moment of silence in the tap.
  - *Measuring.* **Measure delay** asks the hub to replace the speaker's mix for 80 ms with a
    2 kHz tone (a short gap and a beep; replaced, not added, so music cannot hide it). The
    answer carries the hub's clock at request, at the tone and at reply, so the page takes the
    round trip out and never compares clocks. The page finds the tone with a Goertzel filter at
    the worklet's output (live) and, for listening, at a second `<audio>` element's analyser.
    The hub also reports how long sound waits in each source's buffer before it is mixed. The
    probe does not touch the patchbay.
  - *Limits.* The hub cannot see inside the AirPlay receiver (shairport's own latency, which
    the sender sets); it can see only how much sound waits in the pipe's buffer, which the
    probe's answer and the hub's log give. A device's own speaker or Bluetooth delay is not
    measured. Live needs AudioWorklet and a streaming `fetch`; without them the card stays on
    Listening.

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
