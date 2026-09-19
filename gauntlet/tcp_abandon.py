"""Abandoned connections must not disable the server.

Four clients connect and then vanish without closing, which is what happens
when a host crashes or a user hits Ctrl-C. A Linux server reaps them and
keeps serving. If the connection table is a fixed four slots with no reaper,
this is an unauthenticated, permanent denial of service.
"""

import socket
import time

from _harness import ECHO_PORT, connect, fail, ok, read_exactly

HOLD = 4

held = []
for index in range(HOLD):
    try:
        sock = connect(ECHO_PORT, timeout=8.0)
        # Send nothing; just occupy the connection and walk away. SO_LINGER
        # with a zero timeout makes close() send RST-or-nothing rather than
        # a clean FIN, which is what an abrupt client death looks like.
        sock.setsockopt(
            socket.SOL_SOCKET, socket.SO_LINGER, __import__("struct").pack("ii", 1, 0)
        )
        held.append(sock)
    except Exception as err:  # noqa: BLE001
        fail(f"could not even open abandoned connection {index}: {err!r}")

# Drop our references the hard way, without a graceful shutdown handshake.
for sock in held:
    try:
        sock.close()
    except OSError:
        pass

time.sleep(2.0)

payload = b"still-alive\n"
try:
    fresh = connect(ECHO_PORT, timeout=8.0)
except Exception as err:  # noqa: BLE001
    fail(
        f"after {HOLD} abandoned connections the server refuses new ones: {err!r}. "
        "The connection table is wedged."
    )

try:
    fresh.sendall(payload)
    got = read_exactly(fresh, len(payload))
finally:
    fresh.close()

if got == payload:
    ok(f"server still serving after {HOLD} abandoned connections")
fail(
    f"after {HOLD} abandoned connections a new client connects but gets no service "
    f"(wanted {payload!r}, got {got!r})"
)
