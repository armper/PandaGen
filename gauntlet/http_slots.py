"""Ordinary HTTP clients must not wedge the connection table.

Every HTTP client on earth opens a keep-alive connection, gets its answer,
and hangs up. When it does, the server sees FIN and the connection enters
CLOSE_WAIT; it leaves that state only when the *server* closes its own half.
A server that never does leaks a connection slot per request.

PandaGen's table is eight slots shared by every port, so a handful of plain
curl requests can take out the echo port and the control plane with it --
no hostile traffic, no authentication, nothing the client did wrong.

Six sequential requests, each on its own connection, then a check that the
machine still answers on a different port.
"""

import socket

from _harness import COMMAND_PORT, HTTP_PORT, connect, fail, ok
from _sign import parse_reply, sign

REQUESTS = 8

for index in range(REQUESTS):
    try:
        sock = connect(HTTP_PORT, timeout=6.0)
    except OSError as err:
        fail(
            f"request {index} of {REQUESTS} could not connect at all: {err!r}. "
            f"The first {index} ordinary requests exhausted the connection table."
        )
    try:
        sock.sendall(b"GET /health HTTP/1.1\r\nHost: pandagen\r\n\r\n")
        try:
            reply = sock.recv(4096)
        except OSError as err:
            fail(
                f"request {index} of {REQUESTS} was reset: {err!r}. The first "
                f"{index} requests hung up cleanly but their slots are still held."
            )
        if b"200 OK" not in reply:
            fail(f"request {index} got {reply[:80]!r} instead of a 200")
    finally:
        # A clean close, exactly what curl does at the end of a keep-alive
        # connection. Nothing abrupt, no RST.
        sock.close()

# The control plane lives on another port and must be unaffected.
try:
    control = connect(COMMAND_PORT, timeout=6.0)
except OSError as err:
    fail(
        f"after {REQUESTS} ordinary HTTP requests the command port refuses "
        f"connections: {err!r}. The control plane is gone."
    )
try:
    control.sendall((sign("cpus") + "\n").encode())
    raw = control.recv(4096).decode(errors="replace")
finally:
    control.close()

if not raw.strip():
    fail(
        f"after {REQUESTS} ordinary HTTP requests the command port accepts a "
        "connection but answers nothing; the table is full of slots held by "
        "clients that already hung up"
    )
accepted, text = parse_reply(raw.splitlines()[0])
if not accepted or "online=" not in text:
    fail(f"command port answered {text!r} after {REQUESTS} HTTP requests")

ok(f"{REQUESTS} keep-alive requests released their slots; control plane intact")
