# Endora sound: the design

Agreed with the owner on 2026-10-16 in a design interview (Q1–Q46); recorded in
[ADR 0104](../adr/0104-endora-sound.md). Prepared with AI assistance (Claude). This document
is the specification the build follows. Where it says *unverified*, nobody has run it yet
and the first slice must.

## 1. What it is

Spotify Connect for any sound. Sources go in, speakers come out, and **wires** join them.
It serves two uses, which are the same model:

- **Gathering.** Many of the owner's sources play into the device they are on: the phone,
  the tablet and Endora's voice, all into the Mac's speakers.
- **Spreading.** One source plays on many speakers: Spotify on both HomePods and the Mac.

The uses are music, audiobooks (Audible on the phone or tablet) and Endora's voice. A few
seconds of delay is fine for all three. **Video is out of scope**: it plays natively from the
device, which keeps lip sync, and the hub steps aside (section 5).

## 2. The model

- **Source**: anything that makes sound. The first sources are:
  - an AirPlay receiver per device;
  - Spotify Connect;
  - an internet radio station;
  - Endora's voice.
- **Speaker**: anything that plays. The speakers are:
  - the HomePods and the Apple TV, over AirPlay;
  - the Mac, as an AirPlay speaker;
  - the phone, the tablet and the handheld, through a live stream.
- **Wire**: one source to one speaker, with a **level** from 0 to 100.
  - Levels are independent: nothing rebalances them.
  - A limiter keeps the sum from clipping.
  - A source can **set all** of its wires at once ("classical at 10% everywhere"), and
    each wire stays adjustable after that.
- **Volume**: each speaker's own loudness, separate from the wire levels. The wire levels set
  the balance (YouTube at 90, classical at 10); the speaker's volume sets how loud that
  balance plays (the office HomePod at 40).
- **A device is never wired to itself.** The phone can't feed the phone's own stream: it would
  hear itself about four seconds late.
- **Ducking.** While Endora speaks, its other wires drop, then come back.
- **Groups are derived, never managed.** Speakers whose wires are identical (same sources, same
  levels) form one group and get one stream, in sync. There are no modes and no sessions.
- **Named setups** save a set of wires ("Everything to my Mac", "Share with the home",
  "Kitchen mornings") and can run on a schedule.
- A device's own volume slider controls only that device's source, not the speakers.

Example: in the morning, classical radio goes to the kitchen HomePod at 60, started on a
schedule. In the evening, classical goes to every speaker at 10, and the phone goes to the
office HomePod and the Mac at 90. That makes two groups and two mixes, running at once.

## 3. The parts

| Part | What it is | Notes |
|---|---|---|
| **endora-sound** | The owner's own program, in Rust at `app/sound`, Apache-2.0, in its own container | Ingests every source, mixes one stream per group, serves the streams, holds the wires and setups, has its own HTTP API. Decodes and encodes with `ffmpeg`. Works without Endora. |
| **Senders** | A pool of four **stock, unmodified** OwnTone instances | AirPlay 2 out, in sync, with OwnTone's HomePod and Apple TV workarounds. Each plays one group's stream from a pipe, and is limited to that group's speakers. |
| **airptpd** | OwnTone's shared PTP timing daemon | It must start before the senders. All four senders share it. |
| **Receivers** | shairport-sync built for classic AirPlay, one instance per device, in the senders' container | Named "Endora · iPhone", "Endora · iPad" and "Endora" (for guests). Each writes PCM into its pipe, `airplay-<device>.pcm`, which endora-sound reads; the hub lists which to run. |
| **Spotify** (the hub's own) | librespot, called "Endora" | A Spotify Connect receiver writing into `spotify.pcm`. Picked once in the Spotify app, it keeps its own login. |
| **Voice** | Kokoro-FastAPI, already on the NAS GPU | endora-sound asks it to speak and mixes the result in with ducking. |

**Why the receivers are classic AirPlay.** An AirPlay 2 receiver needs `nqptp`, and `nqptp`
and the senders' `airptpd` both bind UDP 319/320 with no port sharing. AirPlay 2 receivers
would have needed a container on a LAN address of their own. Classic AirPlay needs no PTP
clock, so its receivers share the NAS's address, beside the senders, with the same lossless
sound and the same two-second delay. The owner chose it on 2026-10-17; moving to AirPlay 2
later needs that address, and nothing else in the hub changes.

**Why stock OwnTone, and why four.** OwnTone's player is single-session: one queue, one input
and one clock, on file-scope state throughout `player.c`, `input.c` and `outputs/`. Several
instances side by side are supported and documented (`docs/advanced/multiple-instances.md`).
Each needs its own ports (`websocket_port`, `library.port`, `mpd.port`), its own `db_path` and
`cache_dir`, and its own `library.name`. Each is limited to its speakers with per-device
`exclude` or `exclusive`. Four instances means four groups can play at once. We fork OwnTone
only if its AirPlay code itself must change.

**Licences.**
- endora-sound is Apache-2.0.
- OwnTone is GPL-2.0-or-later, and `nqptp` is GPL-2.0-only. Both run as separate programs,
  reached only through OwnTone's JSON API and named pipes. Nothing of theirs is linked or
  copied into this repository.
- shairport-sync is mostly MIT.

## 4. Streams and delay

- **AirPlay speakers** play about 2 s behind (OwnTone's `start_buffer_ms` defaults to 2250).
  A source that itself arrives over AirPlay adds its own hop, so the phone → hub → speakers
  path runs about 4 s late. That's fine for music and voice.
- **Phone, tablet and handheld** play an HTTP MP3 live stream that endora-sound serves. These
  appear by name while they listen, like Spotify Connect's device list. They are not in sync
  with the AirPlay speakers or with each other. *Verified 2026-10-17:* an iPhone listening in
  Endora's web app keeps playing with the screen locked, so no native app is needed.
  *Unverified:* the stream's actual delay.
- **The Mac** plays as an AirPlay speaker, through macOS's AirPlay Receiver, so it stays in
  sync with the HomePods. It needs "Anyone on the same network", because OwnTone cannot sign
  in to an Apple Account. *Unverified:* OwnTone issue #1385 reports an "Allow?" prompt on
  every new session. If it nags, the Mac falls back to the browser stream.
- **Low latency** (Opus over WebRTC, under a second) waits until the MP3 delay actually
  bothers the owner.

## 5. Sharing speakers with everything else

- **Native AirPlay wins.** When another device takes a speaker directly (the Mac playing a
  YouTube video to the office HomePod), the hub shows it as *in use by another device*. It
  doesn't reconnect, so it doesn't cut the Mac off, and it keeps the speaker's wires saved.
  When the speaker is free, the hub restores them. If OwnTone can't be told which
  disconnects to ignore, the hub turns OwnTone's automatic reconnect off and reconnects
  speakers itself. *Unverified.*
- **Handover between senders.** When a speaker's wires change, so does its group, and another
  sender must take it over. *Unverified:* what happens when two OwnTone instances select the
  same HomePod. Presumably the last one to connect wins. The hub always releases a speaker
  from the old sender before the new one selects it.
- **Radio has a watchdog.** OwnTone's `http` input and ffmpeg reconnect on a dropped
  connection, but a hard end of stream stops playback. endora-sound restarts its own
  stations, so a station can play all day.
- **One Spotify stream at a time.** One Spotify account plays on one device at a time, so all-day
  background music is radio, not Spotify.

## 6. Endora as the remote

- **The Speakers screen** in the console (Music → Speakers) draws the sources, the speakers
  and the wires between them. Tap to add or remove a wire, and drag to set a level. It's
  phone first; desktop drag-to-wire can come later on the same picture.
- **Chat gets a fixed set of actions**, defined by the hub's API:
  - apply a named setup;
  - set a wire's level, or a speaker's volume;
  - play a station on chosen speakers;
  - say something on speakers (Kokoro, with ducking);
  - stop a source.

  The model proposes an action and the hub's API acts. Anything else is refused. Requested
  changes are low-risk and simply happen, under the existing autonomy settings. Announcements
  Endora decides to make itself follow the proactive check-in rules and quiet hours, and play
  on the speakers that are on (or a chosen default set when nothing plays).
- **Away from home** the hub is reachable only through Endora's sign-in and its tunnel. The
  hub itself answers only the NAS's own containers.
- **The hub owns its state**: wires, volumes, setups and schedules. That way it works while
  Endora is down, and other systems can use its API.

## 7. Deployment

- A `sound` role in Endora's `endora.homelab` collection, beside the node, the mail mirror
  and HQ. Images are built on the Mac and never on the NAS, and get memory limits. Before
  deploying, check the NAS's memory headroom (see the capacity notes).
- The stack is endora-sound, `airptpd`, the four senders, the receiver container on its own
  address, and the librespot receiver.
- The ansible-nas `owntone` role stays as the fallback, and is retired as the last step.

## 8. The build order

1. **Measure the slow voice path.** Done on 2026-10-16.
   - Ollama was unloading the model, which meant a reload of about 13 s. It now keeps the
     model loaded.
   - The prompt opened with the time, which broke Ollama's prompt cache and cost about 5 s a
     message. It is now ordered stable parts first.
   - Afterwards a call took a median of 3.1 s (p90 5.1 s).
   - Left over: the reply's `done` event waits for the learning call, and speech starts on
     `done`. That's a small fix to the node.
2. **This ADR and this design.**
3. **The first slice.**
   - The endora-sound mixer, with two sources: the iPhone receiver and one radio station.
   - Two senders sharing `airptpd`.
   - The HomePods and the iPhone stream as speakers.
   - The Speakers screen.
   - Tests, built in from the start, for the three unverified behaviours: the handover
     between senders, the Mac's prompt, and stepping aside.

   Before the slice: a view-only UniFi entry in Endora's Connections, so a free address for
   the receivers can be found, and the owner reserving that address.
4. **Then, in order:**
   - Kokoro announcements with ducking;
   - Spotify;
   - the iPad and guest receivers;
   - named setups and their schedules;
   - the Mac and handheld speakers;
   - the chat actions;
   - retiring the `owntone` role.

**Later, only if needed:**
- a native iOS app;
- Opus over WebRTC;
- the hub playing Spotify or Plex itself;
- the handheld as a source;
- Bluetooth (only via PulseAudio);
- different speakers for each kind of announcement.
