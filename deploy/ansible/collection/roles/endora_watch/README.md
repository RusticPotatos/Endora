# The house watch (endora_watch)

A small always-on machine outside the NAS (a Raspberry Pi 2 with Raspberry Pi OS Lite is enough)
that does two things the NAS cannot do for itself (Endora ADR 0138, in `docs/adr/`):

- **It watches the NAS from outside.** Once a minute it checks what you list (a port open, a
  name answered, an https page with the right certificate) and, when one has been failing past
  the grace (10 minutes, longer than the NAS's nightly reboot), says so on your phone through an
  ntfy topic, once, and again once when it is back. Everything else that watches the house runs
  on the NAS, and goes quiet with it.
- **It is a second Pi-hole**, with the same local names as the NAS's, so names keep resolving
  and ads stay blocked while the NAS is away.

No Docker: a Pi 2 is 32-bit with 1 GB. The watch is one Python file with the standard library
alone (`files/watch.py`), run by a systemd timer as a throwaway user with nothing but the
network; Pi-hole is installed natively.

What it does, in this order:

1. **Checks the settings** and stops before touching anything when one is missing or wrong.
2. **A host firewall**: `endora_dev_node`'s (ufw, the SSH lock-out guard, the ruleset before
   its first run kept in `/etc/endora/firewall.before`), with DNS let in from the LAN first.
3. **SD card care**, as `endora_dev_node`'s: the journal in memory and capped, `/tmp` in memory
   from the next boot.
4. **Updates by themselves**: unattended-upgrades over Raspberry Pi OS's own archives, and a
   reboot when an update needs one at `endora_watch_reboot_time` (05:00, after the NAS's 04:00).
5. **Pi-hole**: Pi-hole's installer at a pinned release and checksum, unattended (a seed config
   goes first, so it asks nothing); its settings declared as `FTLCONF_` variables in
   `/etc/systemd/system/pihole-FTL.service.d/endora.conf`, as the NAS's Pi-hole role declares
   them; the web interface on this machine alone, with a random password nobody keeps (set your
   own with `sudo pihole setpassword`, then `ssh -L 8080:localhost:8080` and open
   http://localhost:8080/admin). The machine then asks its own Pi-hole first and 1.1.1.1 second.
6. **The watch**: `watch.py`, its settings in `/etc/endora/watch.json` (root's alone, checked by
   `watch.py --check` before they are written, handed to each run as a systemd credential) and
   `endora-watch.timer`. Its state (what is failing since when, and messages not yet sent) is
   kept in memory, so a reboot of the Pi starts afresh and says "House watch started".

## What you get on your phone

| When | Title | Priority |
|---|---|---|
| The watch starts (first run after a boot) | House watch started | low |
| A check has failed for the grace or longer | `2 down: NAS, Plex` and a line each | high |
| It answers again | `Back: NAS` | default |

A message that cannot be sent (the internet is down too) waits and goes with the next run; the
newest 20 are kept.

## Settings

| Variable | Default | What it is |
|---|---|---|
| `endora_watch_enabled` | `false` | Nothing happens unless true |
| `endora_watch_lan_cidr` | `""`, required | Your LAN as an IPv4 CIDR: where SSH and DNS may come from |
| `endora_watch_ssh_extra_cidrs` | `[]` | More places SSH may come from |
| `endora_watch_alert_url` | `""`, required | The ntfy topic, `https://ntfy.sh/<topic>`. A secret: from your inventory |
| `endora_watch_heartbeat_url` | `""` | Fetched after every run, so something outside notices this machine stop |
| `endora_watch_grace_minutes` | `10` | How long a check may fail before it is said |
| `endora_watch_checks` | `[]`, required | What is checked (kinds in `defaults/main.yml`) |
| `endora_watch_ca_cert` | `""` | Your CA's public certificate on the controller, for https checks |
| `endora_watch_pihole` | `true` | The second Pi-hole |
| `endora_watch_pihole_upstreams` | `[1.1.1.1, 1.0.0.1]` | Where Pi-hole asks |
| `endora_watch_pihole_dns_hosts` | `[]` | Local records, `"<address> <name>"`: keep them the NAS's |
| `endora_watch_pihole_rev_servers` | `[]` | Names asked of the router |
| `endora_watch_reboot_time` | `"05:00"` | When an update's reboot may happen |

## Running it

From `deploy/ansible`, with `inventory.watch.local.yml` (from `inventory.watch.example.yml`):

```sh
ansible-playbook -i inventory.watch.local.yml watch.yml --check --diff
ansible-playbook -i inventory.watch.local.yml watch.yml --diff
```

On the machine: `systemctl list-timers endora-watch.timer`, `journalctl -u endora-watch` (a line
for each failing check), and a run by hand: `sudo systemctl start endora-watch`.

## Rolling back

`sudo systemctl disable --now endora-watch.timer`; `sudo pihole uninstall`; the firewall as
`endora_dev_node`'s README says.
