#!/usr/bin/env python3
"""Host-only private setup. Never place this generated catalog in a repository."""
import getpass
import json
import os
import pathlib
import secrets
import shutil
import tempfile

def main():
    if os.getuid() == 0:
        raise SystemExit('Run as the existing non-root NAS administration user')
    os.umask(0o077)
    repository = pathlib.Path(input('Existing Ansible NAS checkout: ').strip()).expanduser().resolve(strict=True)
    if not (repository / 'nas.yml').is_file():
        raise SystemExit('The checkout needs nas.yml')
    inventory = pathlib.Path(input('Private inventory path: ').strip()).expanduser().resolve(strict=True)
    roles = input('Selected service roles, comma-separated: ').strip().split(',')
    jobs = [{'id': 'syntax', 'label': 'Validate NAS playbook', 'kind': 'syntax'}]
    for role in map(str.strip, roles):
        if not role or not all(c.isascii() and (c.islower() or c.isdigit() or c == '_') for c in role):
            raise SystemExit('Use existing role names')
        if role in ('cloudflared', 'portainer', 'endora', 'endora_terminal_agent'):
            raise SystemExit('Use SSH for control-path maintenance')
        if not (repository / 'roles' / role / 'tasks/main.yml').is_file():
            raise SystemExit('A selected role is absent from this checkout')
        jobs.extend([{'id': f'check-{role}', 'label': f'Check {role}', 'kind': 'check', 'role': role}, {'id': f'apply-{role}', 'label': f'Apply {role}', 'kind': 'apply', 'role': role}])
    program = shutil.which('ansible-playbook')
    if program is None:
        raise SystemExit('Install the NAS existing Ansible runtime first')
    root = pathlib.Path.home() / '.config/endora-jobs'
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    if root.is_symlink() or root.stat().st_mode & 0o077:
        raise SystemExit('Private config directory must have mode 0700')
    destination = root / 'config.json'
    if destination.is_symlink():
        raise SystemExit('Symlinked private config refused')
    old = json.loads(destination.read_text()) if destination.exists() else {}
    config = {'api_key': old.get('api_key') or secrets.token_urlsafe(48), 'repository': str(repository), 'inventory': str(inventory), 'ansible_program': program, 'jobs': jobs}
    fd, temporary = tempfile.mkstemp(dir=root)
    try:
        with os.fdopen(fd, 'w') as output:
            json.dump(config, output)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, destination)
    finally:
        pathlib.Path(temporary).unlink(missing_ok=True)
    print('Private job catalog saved. Use its key only in the server-side masked setup prompt.')

if __name__ == '__main__':
    main()
