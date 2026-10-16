#!/usr/bin/env python3
"""Check publishable files without printing matched private values."""
import ipaddress
import os
import pathlib
import re
import subprocess
import sys

EXAMPLES = {'127.0.0.1', '0.0.0.0', '192.168.1.10'}
PATTERNS = [
    re.compile(r'-----BEGIN (?:OPENSSH |RSA |EC |)PRIVATE KEY-----'),
    re.compile(r'\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})\b'),
    re.compile(r'\b[0-9a-f]{32,}\b', re.I),
    re.compile(r'\b(?:password|api_key|token|psk)\s*[:=]\s*["\']?(?!fictional|example|test|\$)[A-Za-z0-9_-]{12,}', re.I),
    re.compile(r'\b[A-Za-z0-9._%+-]+@(?!example\.(?:com|org|net)\b|tty1\.service\b)[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b'),
]

def inspect(path, private_values):
    if path.is_symlink():
        return ['symlink requires review']
    try:
        # A read bit check also catches unreadable fixtures when tests run as root.
        if path.stat().st_mode & 0o444 == 0:
            raise OSError()
        data = path.read_bytes()
    except OSError:
        return ['unreadable candidate']
    if b'\0' in data:
        return ['binary file requires explicit review']
    try:
        text = data.decode('utf-8')
    except UnicodeError:
        return ['non-text file requires review']
    problems = []
    for value in private_values:
        if value and value.casefold() in text.casefold():
            problems.append('configured private value')
    for hit in re.finditer(r'\b(?:\d{1,3}\.){3}\d{1,3}\b', text):
        try:
            ipaddress.ip_address(hit.group())
        except ValueError:
            continue
        if hit.group() not in EXAMPLES:
            problems.append('network address')
    # Lockfile hashes are expected; other patterns still apply.
    for index, pattern in enumerate(PATTERNS):
        if index == 2 and path.name == 'Cargo.lock':
            continue
        if pattern.search(text):
            problems.append('credential or personal-data pattern')
    return sorted(set(problems))

def main():
    # The terminal's own folder: it lives inside Endora's repository, whose files Endora's own
    # check covers. `git ls-files` run here lists only this subtree.
    root = pathlib.Path(__file__).resolve().parents[1]
    local = root / 'privacy.local'
    if local.exists() and subprocess.run(['git', 'check-ignore', '-q', str(local)], cwd=root).returncode:
        raise RuntimeError('private-value file must be ignored')
    private_values = local.read_text().splitlines() if local.exists() else []
    private_values += os.environ.get('ENDORA_PERSONAL_VALUES', '').split(',')
    raw = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=root)
    failures = []
    for name in set(raw.decode().split('\0')) - {''}:
        for reason in inspect(root / name, private_values):
            failures.append((name, reason))
    for name, reason in sorted(failures):
        # Do not print matching text, line contents, or configured values.
        print(f'{name}: {reason}', file=sys.stderr)
    if failures:
        return 1
    print('Publishable files passed privacy checks')
    return 0

if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception:
        print('Privacy check could not complete safely', file=sys.stderr)
        sys.exit(1)
