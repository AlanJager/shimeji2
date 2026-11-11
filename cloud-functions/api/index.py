"""EdgeOne entry for the hosted editor.

The scanner only registers a module that defines ``class handler(BaseHTTPRequestHandler)``.
Handler mode keeps the original request path. A framework app would strip ``/api`` first,
so paths that arrive either way are put back into the form ``server.Handler`` already uses.
Static pages stay in ``edgeone-public`` and are not served here.
"""

import base64
import importlib.util
import io
import os
import sys
import tempfile
from http.server import BaseHTTPRequestHandler
from pathlib import Path
from types import SimpleNamespace

_SERVER = None


def editor_path(path):
    bare, sep, query = path.partition("?")
    if not bare.startswith("/"):
        bare = "/" + bare
    if bare != "/api" and not bare.startswith("/api/"):
        bare = "/api" if bare == "/" else "/api" + bare
    return bare + (("?" + query) if sep else "")


def load_server():
    global _SERVER
    if _SERVER is not None:
        return _SERVER
    here = Path(__file__).resolve().parent
    staged = here / "editor"
    if (staged / "server.py").is_file():
        sys.path.insert(0, str(staged))
        root = materialize(staged)
    else:
        root = repo_root(here)
        sys.path.insert(0, str(root / "tools" / "editor"))
    os.environ["EDGEONE"] = "1"
    os.environ["SHIMEJI_EDITOR_ROOT"] = str(root)
    import server
    _SERVER = server
    return server


def materialize(staged):
    blobs = load_blobs(staged / "blobs.py")
    root = Path(tempfile.mkdtemp(prefix="shimeji-editor-"))
    for rel, encoded in blobs.items():
        dest = root / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(base64.b64decode(encoded))
    catalog = staged / "assets" / "catalog" / "sources.json"
    if catalog.is_file():
        dest = root / "assets" / "catalog" / "sources.json"
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(catalog.read_bytes())
    return root


def load_blobs(path):
    spec = importlib.util.spec_from_file_location("shimeji_edgeone_blobs", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.BLOBS


def repo_root(here):
    repo = here
    for _ in range(6):
        if (repo / "tools" / "editor" / "server.py").is_file():
            return repo
        if repo.parent == repo:
            break
        repo = repo.parent
    raise RuntimeError("找不到编辑器文件")


def serve(method, path, headers, rfile):
    server = load_server()
    bridge = Bridge(server, editor_path(path), headers, rfile)
    if method == "POST":
        server.Handler.do_POST(bridge)
    else:
        server.Handler.do_GET(bridge)
    return bridge.status, bridge.header_list, bridge.wfile.getvalue()


class Bridge:
    def __init__(self, server, path, headers, rfile):
        self.path = path
        self.headers = headers
        self.rfile = rfile if rfile is not None else io.BytesIO()
        self.wfile = io.BytesIO()
        self.connection = SimpleNamespace(settimeout=lambda _timeout: None)
        self.server = SimpleNamespace(server_port=443)
        self.status = 500
        self.header_list = []
        self._respond = server.Handler.respond
        self._local = server.Handler.local_request
        self._media = server.Handler.media_request

    def local_request(self):
        return self._local(self)

    def respond(self, status, body, content_type, immutable=False):
        return self._respond(self, status, body, content_type, immutable)

    def media_request(self, route):
        return self._media(self, route)

    def send_response(self, code, message=None):
        self.status = code

    def send_header(self, key, value):
        self.header_list.append((key, value))

    def end_headers(self):
        return None

    def log_message(self, fmt, *args):
        return None


class handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self._serve()

    def do_POST(self):
        self._serve()

    def _serve(self):
        try:
            status, headers, body = serve(self.command, self.path, self.headers, self.rfile)
        except Exception as error:
            status = 500
            headers = [("Content-Type", "text/plain; charset=utf-8")]
            body = str(error).encode()
        self.send_response(status)
        for key, value in headers:
            self.send_header(key, value)
        if not any(key.lower() == "content-length" for key, _value in headers):
            self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt, *args):
        return None
