# endora.homelab

Endora's Ansible roles, shipped from Endora's own repository and versioned with it
([ADR 0099](../../../docs/adr/0099-the-handheld-is-part-of-endora.md)). A role and the code it
deploys change in one commit and are checked together.

| Role | What it does |
|---|---|
| [`endora`](roles/endora/README.md) | Starts the Endora node from an image built elsewhere (`make deploy` copies it); never builds on the server |
| [`openbao`](roles/openbao/README.md) | OpenBao, the house's secrets manager, beside the node on their own internal network; Endora initialises it and unlocks it with the owner's passkey (ADR 0127) |
| [`mail_mirror`](roles/mail_mirror/README.md) | A read-only copy of your mail as a Maildir. Endora reads none of it now; it is kept for a later HQ contract that reads mail locally (Endora ADR 0136) |
| [`docker_egress`](roles/docker_egress/README.md) | Locked-down Docker networks: what a network may reach, enforced by the host's firewall |
| [`endora_terminal_agent`](roles/endora_terminal_agent/) | The host job agent: allowlisted Ansible jobs for the handheld and Endora, nothing else |
| [`endora_dev_node`](roles/endora_dev_node/README.md) | The Raspberry Pi that runs the team's runner and sandboxes: Docker CE, a host firewall, SD card care, and the locked `endora_sandbox` network (ADR 0110); run by `deploy/ansible/pi.yml` |

## Using it from ansible-nas

In `requirements.yml`, pinned to a commit:

```yaml
collections:
  - name: https://github.com/RusticPotatos/Endora.git#/deploy/ansible/collection/
    type: git
    version: <commit>
```

Then `ansible-galaxy collection install -r requirements.yml`, and in the playbook:

```yaml
    - role: endora.homelab.mail_mirror
      tags: [mail_mirror]
```

Each role is off until its `*_enabled` variable is true, and takes its private values from your
vault; nothing personal belongs in this repository.

## Checks

`sh check.sh` runs every role's tests, syntax checks and rendering checks. It needs
`ansible-core` and `community.docker` 3.7 or later; CI runs it on every push.
