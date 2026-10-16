#!/usr/bin/env python3
"""Private, fixed-job Ansible agent. No caller-supplied commands or arguments."""
import argparse
import fcntl
import hmac
import http.server
import json
import os
import pathlib
import re
import selectors
import signal
import socketserver
import sqlite3
import stat
import subprocess
import threading
import time

IDENTIFIER = re.compile(r'^[a-zA-Z0-9_.-]{1,128}$')
ROLE = re.compile(r'^[a-z0-9_]+$')

def arguments(job, repository, inventory):
    kind = job.get('kind')
    role = job.get('role', '')
    if kind not in ('syntax', 'check', 'apply') or (kind != 'syntax' and not ROLE.fullmatch(role)):
        raise ValueError('Invalid private job definition')
    command = ['ansible-playbook', str(pathlib.Path(repository) / 'nas.yml'), '-i', str(inventory)]
    if kind == 'syntax':
        return command + ['--syntax-check']
    if kind == 'check':
        command.append('--check')
    return command + ['--tags', role]

def redact(text):
    # Handles quoted JSON values and whole Authorization values, including Bearer.
    return re.sub(r'''(?i)(["']?(?:password|api[_-]?key|secret|token|authorization)["']?\s*[:=]\s*)(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\n,}]+)''', r'\1<redacted>', text)

class Database:
    def __init__(self, path):
        self.path = str(path)
        with self.connection() as db:
            db.execute('CREATE TABLE IF NOT EXISTS jobs (id TEXT PRIMARY KEY, job TEXT NOT NULL, state TEXT NOT NULL, output TEXT NOT NULL, at REAL NOT NULL)')
            db.execute("UPDATE jobs SET state='uncertain' WHERE state='running'")
        os.chmod(self.path, 0o600)

    def connection(self):
        return sqlite3.connect(self.path, timeout=10)

    def get(self, identifier):
        with self.connection() as db:
            row = db.execute('SELECT id,job,state,output FROM jobs WHERE id=?', (identifier,)).fetchone()
        if row is None:
            raise KeyError('Unknown job')
        return dict(zip(('id', 'job', 'state', 'output'), row))

    def submit(self, identifier, job):
        if not IDENTIFIER.fullmatch(identifier):
            raise ValueError('Invalid request id')
        with self.connection() as db:
            db.execute('BEGIN IMMEDIATE')
            found = db.execute('SELECT job FROM jobs WHERE id=?', (identifier,)).fetchone()
            if found is not None and found[0] != job:
                raise ValueError('Request id belongs to a different job')
            if found is None:
                db.execute('INSERT INTO jobs VALUES (?,?,?,?,?)', (identifier, job, 'queued', '', time.time()))
                # Compact output, never IDs: forgetting an ID could replay an old deployment.
                db.execute("UPDATE jobs SET output='Output expired; completion record retained' WHERE id IN (SELECT id FROM jobs WHERE state IN ('complete','failed','uncertain') ORDER BY at DESC LIMIT -1 OFFSET 256)")
        return self.get(identifier)

    def update(self, identifier, state, output):
        with self.connection() as db:
            db.execute('UPDATE jobs SET state=?,output=? WHERE id=?', (state, redact(output)[-32768:], identifier))

    def next_job(self):
        with self.connection() as db:
            db.execute('BEGIN IMMEDIATE')
            row = db.execute("SELECT id,job FROM jobs WHERE state='queued' ORDER BY at LIMIT 1").fetchone()
            if row is not None:
                db.execute("UPDATE jobs SET state='running' WHERE id=?", (row[0],))
        return row

    def recent(self):
        with self.connection() as db:
            return [dict(zip(('id', 'job', 'state'), row)) for row in db.execute('SELECT id,job,state FROM jobs ORDER BY at DESC LIMIT 20')]

def worker(database, config, stop):
    catalog = {job['id']: job for job in config['jobs']}
    while not stop.wait(0.2):
        row = database.next_job()
        if row is None:
            continue
        identifier, name = row
        job = catalog.get(name)
        if job is None:
            database.update(identifier, 'failed', 'Private job definition no longer exists')
            continue
        output = ''
        process = None
        selector = None
        try:
            command = arguments(job, config['repository'], config['inventory'])
            command[0] = config.get('ansible_program', 'ansible-playbook')
            process = subprocess.Popen(command, cwd=config['repository'], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True, env=dict(os.environ, ANSIBLE_NOCOLOR='1', ANSIBLE_FORCE_COLOR='0'))
            started = time.monotonic()
            selector = selectors.DefaultSelector()
            selector.register(process.stdout, selectors.EVENT_READ)
            while selector.get_map():
                if stop.is_set() or time.monotonic() - started > 1800:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                    output += '\nAgent stopped or job exceeded its time budget; inspect target before retrying'
                    database.update(identifier, 'uncertain', output)
                    break
                for key, _ in selector.select(timeout=0.2):
                    chunk = os.read(key.fileobj.fileno(), 4096)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    output = (output + chunk.decode('utf-8', errors='replace'))[-32768:]
                    database.update(identifier, 'running', output)
            else:
                database.update(identifier, 'complete' if process.wait() == 0 else 'failed', output)
        except (OSError, ValueError):
            database.update(identifier, 'uncertain', 'Job runner failed; inspect target before retrying')
        finally:
            if selector is not None:
                selector.close()
            if process is not None:
                if process.poll() is None:
                    try:
                        os.killpg(process.pid, signal.SIGTERM)
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                process.wait()
                process.stdout.close()

class Handler(http.server.BaseHTTPRequestHandler):
    def setup(self):
        super().setup()
        self.connection.settimeout(8)

    def log_message(self, *_):
        pass

    def reply(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def authorized(self):
        offered = self.headers.get('Authorization', '')
        expected = 'Bearer ' + self.server.config['api_key']
        if not hmac.compare_digest(offered.encode(), expected.encode()):
            self.reply(403, {'error': 'authentication'})
            return False
        return True

    def do_GET(self):
        if not self.authorized():
            return
        if self.path == '/jobs':
            catalog = self.server.config['jobs']
            recent = self.server.database.recent()
            items = []
            for job in catalog:
                latest = next((item for item in recent if item['job'] == job['id']), None)
                items.append({'id': job['id'], 'label': job['label'], 'detail': latest['state'] if latest else 'not run', 'latest_id': latest['id'] if latest else None})
            self.reply(200, {'summary': 'Allowlisted Ansible jobs', 'items': items})
            return
        identifier = self.path.removeprefix('/jobs/')
        if self.path.startswith('/jobs/') and IDENTIFIER.fullmatch(identifier):
            try:
                job = self.server.database.get(identifier)
                self.reply(200, {'summary': job['output'], 'state': job['state'], 'items': []})
            except KeyError:
                self.reply(404, {'error': 'unknown_job'})
            return
        self.reply(404, {'error': 'route'})

    def do_POST(self):
        if not self.authorized():
            return
        if self.path != '/jobs':
            self.reply(404, {'error': 'route'})
            return
        try:
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 < length <= 4096:
                raise ValueError()
            request = json.loads(self.rfile.read(length))
            if set(request) != {'job', 'request_id'}:
                raise ValueError()
            catalog = {job['id'] for job in self.server.config['jobs']}
            if request['job'] not in catalog:
                raise ValueError()
            result = self.server.database.submit(request['request_id'], request['job'])
            self.reply(200, {'job_id': result['id'], 'state': result['state']})
        except (ValueError, TypeError, KeyError):
            self.reply(400, {'error': 'invalid_job_request'})

class Server(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', required=True)
    parser.add_argument('--state', required=True)
    parser.add_argument('--socket', default='/run/endora-jobs/agent.sock')
    options = parser.parse_args()
    os.umask(0o077)
    path = pathlib.Path(options.config)
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o077 or metadata.st_uid != os.getuid():
        raise SystemExit('Private job config must be owned by the service user with mode 0600')
    config = json.loads(path.read_text())
    if not isinstance(config.get('api_key'), str) or len(config['api_key']) < 32:
        raise SystemExit('Private job API key is missing')
    catalog = config['jobs']
    if len(catalog) > 64 or len({job['id'] for job in catalog}) != len(catalog):
        raise SystemExit('Invalid private job catalog')
    for job in catalog:
        if not IDENTIFIER.fullmatch(job['id']):
            raise SystemExit('Invalid private job id')
        arguments(job, config['repository'], config['inventory'])
    state = pathlib.Path(options.state)
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    if state.is_symlink() or state.stat().st_uid != os.getuid() or state.stat().st_mode & 0o077:
        raise SystemExit('Private state directory must be owned by the service user with mode 0700')
    lock = os.open(state / 'agent.lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise SystemExit('Another agent is already supervising this deployment')
    database = Database(state / 'jobs.db')
    socket = pathlib.Path(options.socket)
    socket.parent.mkdir(mode=0o750, parents=True, exist_ok=True)
    if socket.exists():
        if not stat.S_ISSOCK(socket.lstat().st_mode):
            raise SystemExit('Refusing unmanaged socket path')
        socket.unlink()
    stop = threading.Event()
    thread = threading.Thread(target=worker, args=(database, config, stop), daemon=True)
    with Server(str(socket), Handler) as server:
        os.chmod(socket, 0o660)
        server.config, server.database = config, database
        thread.start()
        try:
            server.serve_forever()
        finally:
            stop.set()
            thread.join(timeout=10)
            socket.unlink(missing_ok=True)
    os.close(lock)

if __name__ == '__main__':
    main()
