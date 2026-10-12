"""Fail closed: incomplete acceptance coverage is never a green release gate."""
import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]


def classify(output, name):
    matches = re.findall(r'^test ' + re.escape(name) + r' \.\.\. (ok|FAILED)$', output, re.M)
    if len(matches) != 1:
        return 'unexecuted'
    return 'passed' if matches[0] == 'ok' else 'failed'


def summarize(statuses, returncode):
    return {'passed': bool(statuses) and returncode == 0 and all(s == 'passed' for s in statuses),
            'counts': {s: statuses.count(s) for s in ['passed', 'failed', 'unexecuted']},
            'command_exit': returncode}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=pathlib.Path, default=ROOT / 'target/foundation/gate')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    manifest = json.loads((ROOT / 'scripts/foundation/scenarios.json').read_text())
    command = ['cargo', 'test', '-p', 'endora-infrastructure', '--test', 'foundation_gate',
               '--', '--ignored', '--test-threads=1', '--color=never']
    result = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True, check=False)
    (args.output / 'tests.log').write_text(result.stdout)
    cases = [{**case, 'status': classify(result.stdout, case['test']) if case.get('test')
              else 'unexecuted'} for case in manifest['cases']]
    # Identify source contents, including untracked implementation files. Never read
    # ignored configuration, credentials, fixture outputs or personal settings.
    listed = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others',
                                      '--exclude-standard'], cwd=ROOT).split(b'\0')
    digest = hashlib.sha256()
    for name in sorted(set(listed)):
        if not name:
            continue
        path = ROOT / name.decode()
        if path.is_file() and not path.is_symlink():
            digest.update(name + b'\0' + hashlib.sha256(path.read_bytes()).digest())
    report = {**summarize([case['status'] for case in cases], result.returncode),
              'scope': 'Endora candidate; reference results are separate',
              'source_sha256': digest.hexdigest(), 'manifest_version': manifest['version'],
              'command': command, 'cases': cases}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: report[key] for key in ['passed', 'counts', 'source_sha256']}))
    print('Evidence:', args.output / 'report.json')
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
