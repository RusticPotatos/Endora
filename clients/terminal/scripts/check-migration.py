#!/usr/bin/env python3
"""Verify the actual masked legacy migration in a PTY with a synthetic node."""
import fcntl
import json
import os
import pathlib
import pty
import select
import signal
import socket
import subprocess
import sys
import tempfile
import termios
import threading
import time

binary = str(pathlib.Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory() as directory:
    home = pathlib.Path(directory)
    config = home / '.config/endora-terminal'
    state = home / '.local/state/endora-terminal'
    for root in (config, state):
        root.mkdir(mode=0o700, parents=True)
    listener = socket.socket()
    listener.bind(('localhost', 0)); listener.listen(1); listener.settimeout(10)
    url = f'http://localhost:{listener.getsockname()[1]}'
    (config / 'config.toml').write_text(f'url = "{url}"\nallow_http = true\n')
    (state / 'session').write_text(json.dumps({'node': url, 'token': 'fictional-original-session'}))
    for path in (config / 'config.toml', state / 'session'):
        path.chmod(0o600)
    errors = []
    def serve():
        try:
            with listener.accept()[0] as connection:
                connection.settimeout(10)
                data = bytearray()
                while b'\r\n\r\n' not in data:
                    part=connection.recv(4096)
                    assert part
                    data.extend(part); assert len(data) < 8192
                assert data.startswith(b'POST /v1/session/dashboard ')
                assert b'authorization: bearer fictional-original-session' in data.lower()
                header, received=data.split(b'\r\n\r\n',1)
                length=next(int(line.split(b':',1)[1]) for line in header.split(b'\r\n') if line.lower().startswith(b'content-length:'))
                while len(received)<length:
                    part=connection.recv(length-len(received)); assert part
                    received.extend(part)
                assert json.loads(received)=={}
                body=b'{"token":"fictional-dashboard-token"}'
                connection.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(body)).encode()+b'\r\nConnection: close\r\n\r\n'+body)
        except Exception as error:
            errors.append(error)
    worker=threading.Thread(target=serve,daemon=True);worker.start()
    master, slave=pty.openpty(); original=termios.tcgetattr(slave)
    def terminal():
        os.setsid();fcntl.ioctl(slave,termios.TIOCSCTTY,0)
    child=subprocess.Popen(['sh','-c','"$1" lock-setup; rc=$?; printf "\\n__MIGRATION_%s__\\n" "$rc"; exec sleep 60','sh',binary],stdin=slave,stdout=slave,stderr=slave,env=dict(os.environ,HOME=str(home)),preexec_fn=terminal)
    output=bytearray()
    try:
        for prompt in (b'New device passcode', b'Repeat device passcode'):
            deadline=time.monotonic()+10
            while prompt not in output and time.monotonic()<deadline:
                if select.select([master],[],[],.05)[0]:output.extend(os.read(master,4096))
            assert prompt in output, 'Expected masked prompt was not displayed'
            deadline=time.monotonic()+1
            while termios.tcgetattr(slave)[3] & termios.ECHO and time.monotonic()<deadline:
                time.sleep(.005)
            assert not termios.tcgetattr(slave)[3] & termios.ECHO, 'Passcode entry must disable echo'
            os.write(master,b'4826\n')
        deadline=time.monotonic()+15
        while b'__MIGRATION_' not in output and time.monotonic()<deadline:
            if select.select([master],[],[],.05)[0]:
                chunk=os.read(master,4096)
                if not chunk:break
                output.extend(chunk)
        assert b'__MIGRATION_0__' in output, 'Migration rejected a valid four-character code'
        while select.select([master],[],[],0)[0]:
            try:chunk=os.read(master,4096)
            except OSError:break
            if not chunk:break
            output.extend(chunk)
        assert b'4826' not in output, 'Passcode was echoed'
        assert not (state/'session').exists(), 'Legacy plaintext token remains'
        vault=(state/'local.vault').read_bytes()
        assert b'fictional-original-session' not in vault
        assert json.loads((state/'local.dashboard').read_text())['token']=='fictional-dashboard-token'
        for path in (state/'local.vault',state/'local.dashboard'):
            assert path.stat().st_mode & 0o777 == 0o600
        restored=termios.tcgetattr(slave)
        # macOS sets the kernel's pending-input marker after canonical mode returns.
        # It is transient input state, rather than a terminal preference to restore.
        pending=getattr(termios,'PENDIN',0)
        restored[3] &= ~pending;original[3] &= ~pending
        assert restored==original, 'Terminal preferences were not restored'
        worker.join(timeout=2); assert not errors, 'Synthetic node contract failed'
    finally:
        listener.close()
        if child.poll() is None:os.killpg(child.pid,signal.SIGTERM)
        os.close(master);os.close(slave)
        child.wait(timeout=5)
print('Masked four-character migration and private session permissions passed')
