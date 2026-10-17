#!/usr/bin/env python3
"""Offline proxy boundary tests; no Docker daemon needed."""
import http.client
import importlib.util
import json
from http.server import HTTPServer
import pathlib
import threading
import unittest

spec = importlib.util.spec_from_file_location('proxy', pathlib.Path(__file__).with_name('homelab-docker-proxy.py'))
proxy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(proxy)

INSPECTED = json.dumps({
    'Id': 'ab' * 32, 'Name': '/deluge', 'Image': 'sha256:' + 'ef' * 32,
    'Config': {'Image': 'lscr.io/linuxserver/deluge:latest', 'Env': ['DELUGE_PASSWORD=fictional-secret'],
               'Labels': {'com.docker.compose.service': 'deluge',
                          'traefik.http.middlewares.auth.basicauth.users': 'fictional'}},
    'State': {'Status': 'running', 'Pid': 42},
    'HostConfig': {'NetworkMode': 'container:' + 'cd' * 32, 'Binds': ['/srv/fictional:/config'],
                   'RestartPolicy': {'Name': 'unless-stopped', 'MaximumRetryCount': 0}},
    'Mounts': [{'Source': '/srv/fictional'}],
}).encode()

class ProxyTests(unittest.TestCase):
    def test_only_fixed_reads_and_no_sensitive_fields(self):
        calls = []
        class Connection:
            def __init__(self, _): pass
            def request(self, method, path): calls.append((method, path))
            def getresponse(self): return self
            status = 200
            def read(self, _):
                if calls[-1][1].endswith('/json') and calls[-1][1] != '/containers/json':
                    return INSPECTED
                return b'[{"Names":["/example"],"State":"running","Status":"Up","Env":["fictional-secret"],"Mounts":["/private"]}]'
            def close(self): pass
        original = proxy.UnixConnection
        proxy.UnixConnection = Connection
        server = HTTPServer(('127.0.0.1', 0), proxy.handler('/fictional/socket'))
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        try:
            def request(method, path):
                connection = http.client.HTTPConnection('127.0.0.1', server.server_port)
                connection.request(method, path)
                response = connection.getresponse()
                status, body = response.status, response.read()
                connection.close()
                return status, body
            status, body = request('GET', '/containers/json?all=true')
            self.assertEqual(status, 200)
            self.assertNotIn(b'Env', body)
            self.assertNotIn(b'Mounts', body)
            # One container's inspection, for the home lab's map: only its few fields.
            status, body = request('GET', '/containers/deluge/json')
            self.assertEqual(status, 200)
            self.assertEqual(json.loads(body), {
                'Id': 'ab' * 32, 'Name': '/deluge',
                'Config': {'Image': 'lscr.io/linuxserver/deluge:latest',
                           'Labels': {'com.docker.compose.service': 'deluge'}},
                'State': {'Status': 'running'},
                'HostConfig': {'NetworkMode': 'container:' + 'cd' * 32,
                               'RestartPolicy': {'Name': 'unless-stopped', 'MaximumRetryCount': 0}},
            })
            for never in [b'Env', b'fictional-secret', b'Mounts', b'Binds', b'/srv', b'basicauth', b'Pid']:
                self.assertNotIn(never, body)
            for method, path in [('POST','/containers/json?all=true'),('GET','/containers/json?all=false'),('POST','/containers/deluge/json'),
                                 ('GET','/containers/deluge/json?size=1'),('GET','/containers/../json'),('GET','/containers/-x/json'),
                                 ('GET','/containers/deluge/logs'),('GET','/containers/a/b/json'),('GET','/containers//json'),('GET','/../version')]:
                self.assertNotEqual(request(method, path)[0], 200, path)
            self.assertEqual(calls, [('GET','/containers/json?all=true'), ('GET','/containers/deluge/json')])
        finally:
            server.shutdown()
            thread.join()
            server.server_close()
            proxy.UnixConnection = original

if __name__ == '__main__': unittest.main()
