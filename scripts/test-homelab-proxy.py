#!/usr/bin/env python3
"""Offline proxy boundary tests; no Docker daemon needed."""
import http.client
import importlib.util
from http.server import HTTPServer
import pathlib
import threading
import unittest

spec = importlib.util.spec_from_file_location('proxy', pathlib.Path(__file__).with_name('homelab-docker-proxy.py'))
proxy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(proxy)

class ProxyTests(unittest.TestCase):
    def test_only_fixed_reads_and_no_sensitive_fields(self):
        calls = []
        class Connection:
            def __init__(self, _): pass
            def request(self, method, path): calls.append((method, path))
            def getresponse(self): return self
            status = 200
            def read(self, _): return b'[{"Names":["/example"],"State":"running","Status":"Up","Env":["fictional-secret"],"Mounts":["/private"]}]'
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
            for method, path in [('POST','/containers/json?all=true'),('GET','/containers/json?all=false'),('GET','/containers/example/json'),('GET','/../version')]:
                self.assertNotEqual(request(method, path)[0], 200)
            self.assertEqual(calls, [('GET','/containers/json?all=true')])
        finally:
            server.shutdown()
            thread.join()
            server.server_close()
            proxy.UnixConnection = original

if __name__ == '__main__': unittest.main()
