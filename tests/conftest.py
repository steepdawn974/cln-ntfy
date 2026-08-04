import os
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

import pytest


class _MockNtfyHandler(BaseHTTPRequestHandler):
    """Capture POSTs to /<topic> the way a real ntfy server would."""

    # Shared state is set on the server instance by the fixture.
    def log_message(self, *args, **kwargs):
        # Keep test output quiet.
        pass

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length) if length else b""
        captured = {
            "path": self.path,
            "headers": {k.lower(): v for k, v in self.headers.items()},
            "body": body,
        }
        self.server.received.append(captured)
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"{}")

    def do_GET(self):
        # ntfy clients sometimes GET the topic; respond with empty JSON.
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(b"{}")


@pytest.fixture
def mock_ntfy(node_factory):
    """Start a local HTTP server that captures ntfy publishes.

    Returns a small object with ``url`` (base URL to pass as ``ntfy-url``)
    and ``received`` (a list of captured request dicts).
    """
    port = node_factory.get_unused_port()
    server = ThreadingHTTPServer(("127.0.0.1", port), _MockNtfyHandler)
    server.received = []
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    class _MockNtfy:
        def __init__(self, port, server):
            self.port = port
            self.server = server
            self.url = f"http://127.0.0.1:{port}"

        @property
        def received(self):
            return self.server.received

        def reset(self):
            self.server.received.clear()

        def stop(self):
            self.server.shutdown()
            self.server.server_close()

    mock = _MockNtfy(port, server)
    # Give the server a moment to bind.
    time.sleep(0.1)
    try:
        yield mock
    finally:
        mock.stop()
