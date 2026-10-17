#!/usr/bin/env python3
"""Loopback-only, fixed-route Docker dashboard proxy. Run on the Docker host."""
import argparse
import http.client
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import re
import socket

ALLOWED = {'/version', '/containers/json?all=true'}
# One container's inspection, by a name Docker could have given it, for the home lab's map
# (ADR 0112): only its id, name, image, state, restart policy, network mode and three compose
# labels are passed on. Never its environment, mounts, binds or any other label.
INSPECT = re.compile(r'/containers/[A-Za-z0-9][A-Za-z0-9_.-]{0,127}/json')
COMPOSE_LABELS = ('com.docker.compose.service', 'com.docker.compose.project', 'com.docker.compose.depends_on')

def inspected(value):
    """The map's few fields of an inspection; nothing else of it leaves."""
    if not isinstance(value, dict):
        raise ValueError()
    text = lambda v: v if isinstance(v, str) else ''
    part = lambda v: v if isinstance(v, dict) else {}
    config, state, host = part(value.get('Config')), part(value.get('State')), part(value.get('HostConfig'))
    labels, policy = part(config.get('Labels')), part(host.get('RestartPolicy'))
    count = policy.get('MaximumRetryCount')
    return {
        'Id': text(value.get('Id')),
        'Name': text(value.get('Name')),
        'Config': {'Image': text(config.get('Image')),
                   'Labels': {key: text(labels.get(key)) for key in COMPOSE_LABELS if key in labels}},
        'State': {'Status': text(state.get('Status'))},
        'HostConfig': {'NetworkMode': text(host.get('NetworkMode')),
                       'RestartPolicy': {'Name': text(policy.get('Name')),
                                         'MaximumRetryCount': count if isinstance(count, int) else 0}},
    }

class UnixConnection(http.client.HTTPConnection):
    def __init__(self, path):
        super().__init__('localhost', timeout=5)
        self.path = path

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(5)
        self.sock.connect(self.path)

def handler(socket_path):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path not in ALLOWED and not INSPECT.fullmatch(self.path):
                self.send_error(403, 'Route not permitted')
                return
            connection = UnixConnection(socket_path)
            try:
                connection.request('GET', self.path)
                response = connection.getresponse()
                raw = response.read(1024 * 1024 + 1)
                if response.status != 200 or len(raw) > 1024 * 1024:
                    raise ValueError()
                value = json.loads(raw)
                if INSPECT.fullmatch(self.path):
                    value = inspected(value)
                elif self.path.startswith('/containers/'):
                    value = [{key: item.get(key) for key in ('Names', 'State', 'Status')} for item in value[:100]]
                else:
                    value = {key: value.get(key) for key in ('ApiVersion', 'Version')}
                body = json.dumps(value).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            except (OSError, ValueError, TypeError, http.client.HTTPException):
                self.send_error(502, 'Docker observation unavailable')
            finally:
                connection.close()

        def log_message(self, *args):
            pass

    return Handler

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--socket', default='/var/run/docker.sock')
    parser.add_argument('--port', type=int, default=9876)
    args = parser.parse_args()
    HTTPServer(('127.0.0.1', args.port), handler(args.socket)).serve_forever()
