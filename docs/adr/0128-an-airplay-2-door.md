# 0128 — An AirPlay 2 door

## Status

Proposed, 2026-10-18, for the owner to accept at the merge. Prepared with AI assistance
(Claude). Supersedes the choice of a **classic** AirPlay door that
[0104](0104-endora-sound.md) recorded on 2026-10-17 (the owner's, then), and narrows 0104's
"video through the hub" rejection to what this record allows. Built: the image, the container,
the network, the switch and the docs below. Not yet run against a real Apple TV.

## Context

The hub's AirPlay door, "Endora Mixer" (one general door since 2026-10-18, 0104), is a
shairport-sync receiver built for classic AirPlay, in the senders' container on the host's
network. Classic was chosen because an AirPlay 2 receiver needs the `nqptp` clock, which
needs UDP 319/320 to itself, and the senders' `airptpd` (the clock OwnTone sends AirPlay 2 to
the HomePods with) already holds those ports on the host.

The owner now casts from an Apple TV into the door, and the picture runs ahead of the sound.
A classic stream carries a fixed latency of about two seconds that the Apple TV does not hold
its picture back for. With an **AirPlay 2** receiver it is expected to (as it does for a
HomePod; not yet tried here): the Apple TV sends buffered audio stamped against a shared PTP
clock and keeps its picture to the times it stamped. What is left is what happens after the door (the hub's mix and the speaker
it plays on): a phone listening through the parallel live mode is about 0.3–1 s behind.

Two things about shairport-sync 5.6 shaped this, both read in its source:

- **It does not tell the sender about any extra delay.** Its AirPlay 2 `/info` answer has no
  output latency, and the `Audio-Latency` header is classic only. The Apple TV cannot be asked
  to hold its picture back further.
- **It can hand its sound over early.** `audio_backend_latency_offset_in_seconds`, negative,
  moves the anchor its output is timed to (fixed for buffered AirPlay 2 streams in 5.2.2), so
  the sound reaches the end of the hub's path on time instead of late. Upstream's guidance is
  to keep such adjustments small, "not more than about ± 250 milliseconds"
  ([AdjustingSync.md](https://github.com/mikebrady/shairport-sync/blob/master/ADVANCED%20TOPICS/AdjustingSync.md)),
  and it drops an offset the stream's own latency cannot cover. How far ahead an Apple TV's
  stream arrives, and so how much lead it allows, is not documented upstream.

## Decision

- **The door is AirPlay 2**: shairport-sync 5.6 built `--with-airplay-2`, with nqptp 1.2.9,
  in a container of its own (`endora-airplay2`, Dockerfile target `airplay2`, compose profile
  `airplay2`), still called "Endora Mixer". `service_type = "airplay2"`, so it never falls
  back to classic without anyone noticing.
- **On a LAN address of its own**, through a Docker **macvlan** network on the NAS's LAN
  interface, so its nqptp has UDP 319/320 to itself and nothing clashes with `airptpd`. The
  parent interface, subnet, gateway and address are the owner's values, in `local.mk` (or the
  `endora` role's `endora_sound_airplay2_*` variables); git carries only placeholders that
  fail loudly (an interface that does not exist, the documentation range). Docker hands out
  only that one address (`ip_range` /32), and the MAC is fixed so the router keeps its
  reservation.
- **Its own avahi** announces the door under its own host name (`endora-mixer`) with its own
  address. On the senders' container, the host's address is answered for by something else
  and avahi there publishes no address; here nothing else answers for it.
- **Only the pipes volume connects it to the hub.** It reads the same list the hub writes
  (`airplay-receivers.txt`) and writes into the same named pipe, `airplay-<id>.pcm`, in the
  hub's format (44.1 kHz, S16_LE, stereo; AirPlay 2's own pipe default would be 48 kHz S32).
  Named pipes work across containers through a shared volume on one host. The hub is not
  changed: to it, a door is a pipe.
- **The lead is the owner's to tune**: `SOUND_AIRPLAY2_LATENCY`, seconds from 0 to 2, default
  0.5 (the middle of the live phone path), written as a negative
  `audio_backend_latency_offset_in_seconds`. It is checked when the container starts.
- **One switch, `SOUND_AIRPLAY2`.** 1 in `local.mk` adds the `airplay2` profile and tells the
  senders' container to run no door (and not to restart its senders when the doors change).
  Anything else, the default in git: the classic door in the senders' container, exactly as
  before. The classic build stays in the senders' image as the fall-back; it retires once the
  AirPlay 2 door has been heard working.
- The senders, their HomePods and their `airptpd` are not changed.

## Consequences

- The Apple TV keeps lip-sync to the door, and the lead makes up for the hub's path after it,
  by ear. A lead past what the Apple TV's stream allows is dropped by shairport-sync with a
  warning in `docker logs endora-airplay2`; a lead over 0.25 s is past upstream's comfort,
  so it needs listening to, not trusting.
- **Speakers in sync with the picture are the ones near the door's timing.** A phone on the
  live stream can be made to line up. The HomePods, reached through OwnTone's own AirPlay 2
  buffer (seconds more; not measured here), probably cannot be, and stay for music.
- **The NAS cannot reach the door's address** (macvlan keeps a parent and its children apart),
  so nothing on the NAS (the senders' OwnTone included) sees it, and `ping` from the NAS fails
  by design. Nothing needs to: the pipe is the only connection.
- **A fourth sound container and a LAN address to keep**: reserved outside the router's DHCP
  pool. UniFi sees a new client with the fixed MAC; some switches and Wi-Fi bridges limit or
  refuse more than one MAC on a port, which would show as a door that is announced but cannot
  be played to.
- Several doors are still possible: each gets its own port and its own nqptp clock
  (`/nqptp-door-<n>`) on the one address.

## Alternatives considered

- **Classic, as before.** Rejected now: the Apple TV does not hold its picture for it.
- **Both clocks on the host's network.** Rejected: nqptp and airptpd both need UDP 319/320.
  nqptp serving OwnTone, or airptpd serving shairport-sync, would mean changing one of them.
- **ipvlan (L2) instead of macvlan.** Kept as the fall-back for a switch or bridge that will
  not take a second MAC: the same compose network with `driver: ipvlan` and `ipvlan_mode: l2`.
  The door then shares the NAS's MAC, which UniFi shows as one client with two addresses.
- **Telling the Apple TV a longer latency.** Not possible with stock shairport-sync 5.6 (see
  Context); it would mean patching it, which nothing here justifies yet.
- **A second NAS interface or a Pi as the door.** More hardware for the same result.
