"""Vercel entry for the editor. Local use remains python3 tools/editor/server.py."""

import io
import sys
from pathlib import Path
from types import SimpleNamespace

from flask import Flask, Response, request

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / "tools" / "editor"))

import server

app = Flask(__name__)


class Bridge(server.Handler):
    def __init__(self, path, headers, body):
        self.path = path
        self.headers = headers
        self.rfile = io.BytesIO(body)
        self.wfile = io.BytesIO()
        self.connection = SimpleNamespace(settimeout=lambda _timeout: None)
        self.server = SimpleNamespace(server_port=443)

    def send_response(self, code, message=None):
        self._status = code

    def send_header(self, key, value):
        if key.lower() == "content-type":
            self._content_type = value

    def end_headers(self):
        return None

    def log_message(self, fmt, *args):
        return None


@app.route("/", defaults={"path": ""}, methods=["GET", "POST"])
@app.route("/<path:path>", methods=["GET", "POST"])
def dispatch(path):
    query = request.query_string.decode()
    target = request.path + (("?" + query) if query else "")
    bridge = Bridge(target, request.headers, request.get_data(cache=False))
    bridge._status = 500
    bridge._content_type = "text/plain"
    if request.method == "POST":
        server.Handler.do_POST(bridge)
    else:
        server.Handler.do_GET(bridge)
    return Response(bridge.wfile.getvalue(), status=bridge._status, content_type=bridge._content_type)
