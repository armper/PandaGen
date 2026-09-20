"""A new client must not inherit the previous one's expired deadline.

The head deadline is indexed by TCP slot and was never cleared when a
connection ended mid-head -- a browser's speculative connection, a scanner,
an aborted request. The tick stayed. The next client to land in that slot
was already past the deadline before it had a tick of its own, so the first
time its head did not arrive in one segment it was reset on sight.

Abandon a partial head, wait past the deadline, then make an ordinary
request in two segments.
"""

import socket
import time

from _harness import HTTP_PORT, connect, fail, ok

# The kernel's deadline is ten seconds.
WAIT_SECONDS = 13

# A connection that sends part of a head and hangs up.
sock = connect(HTTP_PORT)
sock.sendall(b"GE")
sock.close()

time.sleep(WAIT_SECONDS)

# An ordinary client, whose head arrives in two segments as heads often do.
try:
    fresh = connect(HTTP_PORT, timeout=6.0)
except OSError as err:
    fail(f"the HTTP port refuses connections after an abandoned head: {err!r}")
try:
    fresh.sendall(b"GET /health HTTP/1.1\r\n")
    time.sleep(0.3)
    fresh.sendall(b"Host: x\r\n\r\n")
    reply = fresh.recv(4096)
except OSError as err:
    fail(
        f"an ordinary two-segment request was reset: {err!r}. It inherited an "
        "expired deadline from a client that had already gone."
    )
finally:
    fresh.close()

if b"200 OK" not in reply:
    fail(f"an ordinary two-segment request got {reply[:60]!r}")

ok("a new connection starts its own deadline rather than the previous one's")
