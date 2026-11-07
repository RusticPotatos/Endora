"""A stub model host for the bundle's test (ADR 0147): answers like an OpenAI-compatible server
and Ollama's /api/ps, and logs every path asked, so the test can see Core reached it. Standard
library only; it thinks nothing."""

import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODEL = "stub-model"


class Stub(BaseHTTPRequestHandler):
    def answer(self, body):
        data = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        print(f"stub-model: GET {self.path}", flush=True)
        if self.path.startswith("/api/ps"):
            return self.answer({"models": [{"name": MODEL, "model": MODEL, "size_vram": 0}]})
        if self.path.startswith("/v1/models"):
            return self.answer({"object": "list", "data": [{"id": MODEL, "object": "model"}]})
        return self.answer({"ok": True})

    def do_POST(self):
        print(f"stub-model: POST {self.path}", flush=True)
        length = int(self.headers.get("content-length") or 0)
        self.rfile.read(length)
        return self.answer({
            "id": "stub", "object": "chat.completion", "model": MODEL,
            "choices": [{"index": 0, "finish_reason": "stop",
                         "message": {"role": "assistant", "content": "stub"}}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
        })

    def log_message(self, *_):
        pass


ThreadingHTTPServer(("0.0.0.0", 11434), Stub).serve_forever()
