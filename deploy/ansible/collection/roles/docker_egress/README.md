# Locked-down Docker networks (docker_egress)

Some containers should reach only a few things: a mail mirror that needs the internet on
HTTPS and nothing on your LAN, or a sandbox that should reach nothing at all. Docker does not
filter where a container may connect, and Docker bypasses `ufw` for container traffic, so
the `docker_egress` role adds iptables rules for one network at a time.

A role that needs it includes it with a policy:

```yaml
- name: My egress policy
  ansible.builtin.import_role:
    name: docker_egress
  vars:
    docker_egress_policy:
      name: myapp                      # a-z0-9, at most 12 characters
      bridge: br-myapp                 # the bridge name you give the network
      subnet: 172.31.240.0/24          # inside 172.16.0.0/12
      allow_dns: true
      allow_public_tcp_ports: [443]
      allow_lan: []                    # e.g. [{dest: "<address>", port: 8443}]
      allow_host_tcp_ports: []         # ports on the NAS itself
```

and creates its Docker network with that subnet and
`com.docker.network.bridge.name` set to that bridge.

What it does:

- A chain per network in `DOCKER-USER` (for traffic leaving the network) and in `INPUT` (for
  traffic to the NAS itself). Anything private that is not listed (your LAN, other
  containers, the NAS, link-local metadata addresses) is dropped, and so is anything public
  that is not listed. Traffic from other networks is not touched.
- Each policy is its own script in `/etc/docker-egress.d/`, so roles do not overwrite each
  other's rules. `docker-egress.service` applies them all whenever Docker starts.
- Applying twice changes nothing; `remove` takes a policy's rules out again.

`sudo iptables -S DOCKER-USER` shows the hooks, and `sudo iptables -S EG-<NAME>` a policy's
rules. Its tests render each policy with Ansible and walk packets through the result:
`python3 roles/docker_egress/tests/test_docker_egress.py`.
