"""Run synthetic reads against isolated Endora/OpenClaw instances and local Ollama.

Successful execution is evidence, never an automatic quality pass. Review answer
claims against fixtures. No ambient credentials or production state are loaded.
"""
import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
VERSION = '2026.9.2'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', choices=['endora', 'openclaw'], required=True)
    parser.add_argument('--openclaw-root', type=pathlib.Path,
                        help='Isolated npm prefix with node@24.15.0 and openclaw@2026.9.2')
    parser.add_argument('--model', default='qwen3:14b')
    parser.add_argument('--case', choices=['Q01', 'Q02', 'Q03'], required=True)
    parser.add_argument('--output', type=pathlib.Path, required=True,
                        help='New directory; existing results are never overwritten')
    parser.add_argument('--timeout', type=int, default=180)
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    fixture_path = ROOT / 'scripts/foundation/fixtures.json'
    fixture = json.loads(fixture_path.read_text())
    task = next(c for c in fixture['cases'] if c['id'] == args.case)
    for name in ['home', 'workspace', 'state']:
        (args.output / name).mkdir()
    trace = args.output / 'tools.jsonl'
    env = {'PATH': os.environ.get('PATH', '/usr/bin:/bin'),
           'HOME': str(args.output / 'home'),
           'ENDORA_FIXTURE_FILE': str(fixture_path), 'ENDORA_FIXTURE_CASE': args.case,
           'ENDORA_FIXTURE_TRACE': str(trace), 'ENDORA_MODEL': args.model,
           'ENDORA_MODEL_URL': 'http://127.0.0.1:11434/v1'}
    config = None
    if args.runtime == 'endora':
        subprocess.run(['cargo', 'build', '-p', 'endora-infrastructure', '--example',
                        'foundation_probe'], cwd=ROOT, check=True)
        command = [str(ROOT / 'target/debug/examples/foundation_probe')]
    else:
        if not args.openclaw_root:
            parser.error('--openclaw-root is required for the reference runtime')
        modules = args.openclaw_root.resolve() / 'node_modules'
        package = json.loads((modules / 'openclaw/package.json').read_text())
        if package['version'] != VERSION:
            raise ValueError('Reference version must be pinned to ' + VERSION)
        config = {
            'models': {'providers': {'ollama': {
                'baseUrl': 'http://127.0.0.1:11434', 'api': 'ollama',
                'apiKey': 'ollama-local', 'models': [{'id': args.model, 'name': args.model,
                'contextWindow': 32768, 'maxTokens': 4096}]}}},
            'agents': {'defaults': {'model': {'primary': 'ollama/' + args.model}}},
            'tools': {'allow': [t['name'] for t in fixture['tools']],
                      'web': {'search': {'enabled': False}, 'fetch': {'enabled': False}}},
            'plugins': {'allow': ['endora-foundation-fixtures', 'ollama'],
                'load': {'paths': [str(ROOT / 'scripts/foundation/openclaw-plugin')]},
                'entries': {'endora-foundation-fixtures': {'enabled': True}}}}
        config_path = args.output / 'config.json'
        config_path.write_text(json.dumps(config, indent=2))
        prompt_path = args.output / 'prompt.txt'
        prompt_path.write_text(task['prompt'])
        env.update(OPENCLAW_STATE_DIR=str(args.output / 'state'),
                   OPENCLAW_CONFIG_PATH=str(config_path))
        command = [str(modules / 'node/bin/node'), str(modules / 'openclaw/openclaw.mjs'),
                   'agent', 'exec', '--config', str(config_path), '--cwd',
                   str(args.output / 'workspace'), '--code-mode', 'direct', '--thinking',
                   'off', '--timeout', str(args.timeout), '--json', '--message-file', str(prompt_path)]
    start = time.monotonic()
    with (args.output / 'stdout.json').open('w') as out, (args.output / 'stderr.log').open('w') as err:
        try:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=out, stderr=err,
                                    timeout=args.timeout + 20, check=False)
            status = result.returncode
        except subprocess.TimeoutExpired:
            status = 'timeout'
    log = (args.output / 'stderr.log').read_text()
    # A collision means a real built-in could have replaced a synthetic tool.
    valid = status == 0 and 'plugin tool name conflict' not in log
    report = {'runtime': args.runtime, 'reference_version': VERSION if config else None,
              'model': args.model, 'case': args.case, 'exit': status,
              'elapsed_ms': round((time.monotonic() - start) * 1000),
              'fixture_sha256': hashlib.sha256(fixture_path.read_bytes()).hexdigest(),
              'valid_execution': valid, 'review_status': 'unreviewed',
              'review_criteria': task['review'],
              'transport': 'ollama-native' if config else 'openai-compatible',
              'limitation': 'Different production prompts and model transports; synthetic reads only.'}
    (args.output / 'run.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))
    return 0 if valid else 1


if __name__ == '__main__':
    raise SystemExit(main())
