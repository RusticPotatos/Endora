# 0148 — The house watch: a machine outside the NAS that watches it, and a second Pi-hole

## Status

Accepted, 2026-10-20, by the owner ("you can do whatever" with a spare Raspberry Pi 2). Prepared
with AI assistance (Claude).

## Context

Everything that watches the house runs on the NAS: Gatus's uptime checks, HQ's alarms, Endora's
own reading of the lab, and the push channel they would use. The hosted healthchecks pinger the
NAS's playbook offers is off. So when the NAS hangs, misses its 04:00 reboot or loses its disk,
nothing notices and nothing says so: the owner finds out when something they use stops. The
house's DNS is the NAS's Pi-hole too; while the NAS is away the `home.arpa` names are gone and
devices fall back to a public resolver.

A Raspberry Pi 2 Model B (ARMv7, 1 GB, 100 Mb ethernet, no Wi-Fi) with an 8 GB SD card is spare.
It cannot run Endora's images (arm64) or be a backup target (8 GB of SD card), and the Pi 5
already is the team's runner. It can run a small program and a Pi-hole, which is what the house
lacks.

## Decision

**A house watch: a machine outside the NAS, set up by the `endora_watch` role in the
`endora.homelab` collection (playbook `deploy/ansible/watch.yml`), as the Pi 5 is by
`endora_dev_node`.**

- **The watch** is one Python file, standard library only, run once a minute by a systemd timer
  as a dynamic user with nothing but the network. It checks what the inventory lists (a TCP
  port, a DNS answer, an https page whose certificate must carry a given name, checked against
  the house's CA without needing DNS) and keeps, in memory, what is failing since when.
- **It speaks only on a change, after a grace**: a check failing for 10 minutes (longer than the
  NAS's nightly reboot) is said once, on the owner's phone, and its return once. One message per
  run, naming each. A message that cannot be sent waits for the next run. A start is said too,
  so a power cut at the Pi shows.
- **The alerts go through ntfy.sh**, over https, to a long random topic kept in the Mac's
  Keychain and never in a tracked file. It is free, needs no account, and does not run on the
  NAS (Gotify, which the NAS could run, would go quiet with it).
- **A second Pi-hole** with the NAS's local records, installed by Pi-hole's own installer at a
  pinned release and checksum, its settings declared as `FTLCONF_` variables as the NAS's role
  declares them, its web interface on the Pi alone. The Pi asks its own Pi-hole first, so the
  watch can reach ntfy while the NAS is away. The router hands it out as the second DNS server.
- **Shared, not copied**: the firewall (with its lock-out guard) and the SD card care are
  `endora_dev_node`'s tasks, called with this machine's settings.

## Consequences

- The NAS being away is now noticed and said within the grace, from outside it.
- Who watches the watch: the Pi saying it started covers a power cut; a Pi that dies quietly is
  not noticed until `endora_watch_heartbeat_url` points at an outside check (healthchecks.io or
  similar: an account, so the owner's choice).
- The alert topic is a secret by obscurity: anyone who learns it can read the alerts (names of
  the house's services, nothing more) and send to it. ntfy's access tokens are the step up if
  that ever matters.
- Two Pi-holes can drift apart in their block lists; the local records are declared on both,
  which is what matters for the house's names.
- Not chosen: Gatus on the Pi (it ships container images and no binaries, so a Go build to keep up), Docker on a 1 GB
  32-bit Pi, and Uptime Kuma (Node.js, too heavy for the Pi 2).
