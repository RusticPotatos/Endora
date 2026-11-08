#!/bin/sh
# Every check this collection has: each role's tests, syntax and rendering. Needs ansible-core and
# community.docker >= 3.7 on the collections path.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT HUP INT TERM
# Make this folder visible as the installed collection `endora.homelab`, as a pinned install is.
mkdir -p "$scratch/ansible_collections/endora"
ln -s "$here" "$scratch/ansible_collections/endora/homelab"
# After it, the path as it was, or Ansible's default when unset: setting the variable replaces the
# default, which hid every collection ansible-galaxy had installed in ~/.ansible (community.docker
# among them): CI failed on that from 7 Nov 2026, when nothing else on the runner provided it.
export ANSIBLE_COLLECTIONS_PATH="$scratch:${ANSIBLE_COLLECTIONS_PATH:-$HOME/.ansible/collections:/usr/share/ansible/collections}"
export ANSIBLE_ROLES_PATH="$here/roles"
cd "$here"
python3 -I roles/mail_mirror/tests/test_mail_mirror.py
python3 -I roles/docker_egress/tests/test_docker_egress.py
python3 -I roles/endora_terminal_agent/tests/test_terminal_jobs.py
python3 -I roles/endora_dev_node/tests/test_endora_dev_node.py
for playbook in roles/mail_mirror/tests/syntax.yml roles/endora/tests/syntax.yml \
    roles/endora_dev_node/tests/syntax.yml ../pi.yml; do
  ansible-playbook --syntax-check "$playbook"
done
ansible-playbook roles/endora/tests/render.yml
ansible-playbook roles/endora_dev_node/tests/render.yml
echo "endora.homelab: every check passed"
