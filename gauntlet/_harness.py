"""Shared helpers for gauntlet scripts.

Every script is a question a Linux server answers correctly. Exit 0 means
PandaGen matched that behaviour; any other exit means it diverged, and the
`gauntlet:` step turns that into a failed run.

Ports arrive in the environment so several QEMU instances can be under test
at once (`qemu-script --port-base N`).
"""

import os
import socket
import sys

ECHO_PORT = int(os.environ.get("PANDAGEN_TCP_ECHO_PORT", "7779"))
COMMAND_PORT = int(os.environ.get("PANDAGEN_TCP_COMMAND_PORT", "7780"))
UDP_PORT = int(os.environ.get("PANDAGEN_UDP_PORT", "7777"))
REMOTE_PORT = int(os.environ.get("PANDAGEN_REMOTE_PORT", "7778"))
HTTP_PORT = int(os.environ.get("PANDAGEN_HTTP_PORT", "8080"))

HOST = "127.0.0.1"


def connect(port, timeout=8.0):
    """A connected TCP socket, or raise."""
    sock = socket.create_connection((HOST, port), timeout=timeout)
    sock.settimeout(timeout)
    return sock


def drain(sock, limit=1 << 20):
    """Read until the peer closes or we hit `limit`. Returns what arrived."""
    chunks = []
    total = 0
    try:
        while total < limit:
            data = sock.recv(65536)
            if not data:
                break
            chunks.append(data)
            total += len(data)
    except socket.timeout:
        pass
    return b"".join(chunks)


def read_exactly(sock, count):
    """Read `count` bytes, or as many as arrive before the socket times out."""
    chunks = []
    total = 0
    try:
        while total < count:
            data = sock.recv(min(65536, count - total))
            if not data:
                break
            chunks.append(data)
            total += len(data)
    except socket.timeout:
        pass
    return b"".join(chunks)


def ok(message):
    print(f"ok: {message}")
    sys.exit(0)


def fail(message):
    print(f"FAIL: {message}")
    sys.exit(1)
