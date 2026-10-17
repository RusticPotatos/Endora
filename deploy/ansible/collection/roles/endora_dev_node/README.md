# Endora dev node (endora_dev_node)

The Raspberry Pi that runs Endora's team runner and each turn's sandbox container: the first of
the team's rings (Endora ADR 0110, in `docs/adr/`). A turn's
sandbox reaches one thing: the NAS's team gateway, through which it talks to HQ, the models and
Endora's bridge. No internet, no DNS, and nothing else on your LAN; its shell works offline.

This role prepares the host. It never builds Endora: Endora's `make deploy-pi` builds the node and
workbench images for `linux/arm64` on another machine, copies them over SSH, and starts the runner.

What it does, in this order:

1. **Checks the settings** and stops before touching anything when one is missing or wrong.
2. **A host firewall** (ufw): incoming denied, except SSH from your LAN; established
   connections, loopback, ICMP and mDNS (`endora-dev.local`) stay allowed by ufw's own rules.
   It never locks you out: it checks the SSH connection the run comes over is one the new rules
   let in, adds the SSH rule before the default deny, and keeps the ruleset from before its
   first run in `/etc/endora/firewall.before`.
3. **Docker CE** from Docker's own apt repository (signed with Docker's key, `arm64`), with
   `/etc/docker/daemon.json`: the `local` log driver with small caps, `live-restore`, and no
   TCP socket (the role fails if anything listens on 2375 or 2376). The `endora` user joins
   the `docker` group, which is as good as root on this host.
4. **SD card care**: the journal in memory and capped; `/tmp` as tmpfs from the next boot when
   it is not already (Debian trixie does this itself).
5. **The `endora_sandbox` network** (bridge `br-sandbox`, `172.31.250.0/24`), locked by the
   [`docker_egress`](../docker_egress/README.md) role: no DNS, no public ports, no ports on the
   Pi, and on the LAN only `endora_nas_addr` port 8788 (the team gateway).
   The rules go in before the network exists.

## Settings

| Variable | Default | What it is |
|---|---|---|
| `endora_dev_node_enabled` | `false` | Nothing happens unless true |
| `endora_nas_addr` | none, required | The NAS running endora-node, an IPv4 address (e.g. `192.168.1.10`) |
| `endora_dev_node_lan_cidr` | `""`, required | Your LAN as an IPv4 CIDR (e.g. `192.168.1.0/24`): where SSH may come from |
| `endora_dev_node_ssh_extra_cidrs` | `[]` | More places SSH may come from, e.g. `["fe80::/10"]` |
| `endora_dev_node_ssh_port` | `22` | The SSH port let in |
| `endora_dev_node_user` | `endora` | Joins the `docker` group and owns the runner's token folder |
| `endora_dev_node_docker_repo` | `https://download.docker.com/linux/debian` | Docker's apt repository |
| `endora_dev_node_docker_key_url` | `<repo>/gpg` | Docker's signing key |
| `endora_dev_node_docker_arch` | from `ansible_architecture` (`arm64` on a Pi) | The repository's architecture |
| `endora_dev_node_docker_suite` | `ansible_distribution_release` | `bookworm` or `trixie` |
| `endora_dev_node_docker_packages` | `docker-ce`, `docker-ce-cli`, `containerd.io` | What is installed |
| `endora_dev_node_docker_log_max_size` | `5m` | Per log file, per container |
| `endora_dev_node_docker_log_max_file` | `2` | Log files kept per container |
| `endora_dev_node_journald_storage` | `volatile` | `volatile` (in memory) or `persistent` (on the card) |
| `endora_dev_node_journald_max_use` | `64M` | The journal's cap, either way |
| `endora_dev_node_tmp_tmpfs` | `true` | `/tmp` in memory when it is not already |
| `endora_dev_node_tmp_size` | `512M` | Its size |
| `endora_dev_node_firewall` | `true` | Set up ufw as above |
| `endora_dev_node_cmdline` | `/boot/firmware/cmdline.txt` | The kernel command line, where the memory cgroup is turned on (Raspberry Pi OS ships it off, and Docker then ignores containers' memory limits); takes effect at the next reboot, the old line kept as a backup beside it |
| `endora_dev_node_firewall_before` | `/etc/endora/firewall.before` | The ruleset from before the first run |
| `endora_dev_node_runner_token_dir` | `/etc/endora/runner-token` | Where `make pi-runner-token` puts the runner's token |
| `endora_dev_node_sandbox_network` | `endora_sandbox` | The sandbox network's name |
| `endora_dev_node_sandbox_policy` | the locked policy above | Checked: the role refuses DNS, public ports, host ports, or any LAN address or port beyond the NAS's 8788 |

Your real values go in the git-ignored `deploy/ansible/inventory.pi.local` (copy
[`inventory.pi.example`](../../../inventory.pi.example)), never in this repository.

## Running it

From `deploy/ansible` in Endora's checkout:

```bash
ansible-playbook -i inventory.pi.local pi.yml --check --diff --ask-become-pass
ansible-playbook -i inventory.pi.local pi.yml --diff --ask-become-pass
```

Before the first real run, check mode cannot ask Docker's repository for packages it has not
added yet, so it skips installing Docker and the sandbox network and says so; the next check
run shows them. ufw's commands show up only in a real run. If the role changed `/etc/fstab`,
`sudo reboot` puts `/tmp` in memory.

Then, from the checkout's root: `make pi-runner-token` once, and `make deploy-pi`.

## Rolling back the firewall

```bash
sudo ufw disable
```

takes every ufw rule out at once and lets everything in again; nothing else of the role depends
on it. To put back exactly what was there before the role's first run:

```bash
sudo ufw disable
sudo iptables-restore < /etc/endora/firewall.before
sudo systemctl restart docker
```

(the Docker restart puts back Docker's own rules and, through `docker-egress.service`, the
sandbox's). If you are locked out anyway, the Pi's own keyboard and screen still work:
`sudo ufw disable` there.

## The SD card

The card wears with every write, and Docker writes a good deal: images, and each turn's
container. The role keeps the journal and `/tmp` in memory and container logs small, but the
lasting fix is an SSD on an **M.2 HAT** (the Raspberry Pi M.2 HAT+ or similar), booted from or
holding `/var/lib/docker`. Not a USB SSD: the Pi 5's USB ports give 600 mA in all unless it has
the 5 A supply, which many SSDs exceed under load, and they drop out.

## Checking it

```bash
sudo ufw status verbose                 # deny (incoming), SSH from your LAN
docker info --format '{{.LoggingDriver}} {{.LiveRestoreEnabled}}'   # local true
docker network inspect endora_sandbox --format '{{range .IPAM.Config}}{{.Subnet}}{{end}}'
sudo iptables -S EG-SANDBOX             # the NAS's 8788, then DROP
```

Its tests: `python3 roles/endora_dev_node/tests/test_endora_dev_node.py` walks packets through
the sandbox's rules as this role sets them, and `tests/render.yml` checks the settings files,
what the role refuses, and that its steps stay in their safe order. `sh check.sh` runs both.
