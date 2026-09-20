"""The signed command port must not be held by clients that never finish.

Phase 311 gave the HTTP port a deadline for an unfinished request head. The
command port waits for a newline and had none, so the identical attack aimed
one port over still worked: `read_line` only yields at a newline or a full
2 KiB buffer, so six connections each dribbling a byte now and then held
every slot on the control plane for ever -- unauthenticated, at almost no
traffic, and with HTTP starved of slots as a side effect.

Six dribbling command connections, then an ordinary signed call and an
ordinary HTTP request.
"""

import socket
import time

from _harness import COMMAND_PORT, HTTP_PORT, connect, fail, ok
from _sign import parse_reply, sign

HOLD = 6
DRIBBLE_SECONDS = 14

held = []
for index in range(HOLD):
    try:
        sock = connect(COMMAND_PORT, timeout=6.0)
    except OSError as err:
        fail(f"could not open slow connection {index}: {err!r}")
    # A command line with no newline, ever.
    sock.sendall(b"00000000000000000000000000000000 gauntlet")
    held.append(sock)

deadline = time.time() + DRIBBLE_SECONDS
while time.time() < deadline:
    time.sleep(1.0)
    for sock in held:
        try:
            sock.sendall(b".")
        except OSError:
            pass

# The control plane must still answer.
try:
    control = connect(COMMAND_PORT, timeout=6.0)
except OSError as err:
    fail(f"after {HOLD} unfinished command lines the command port refuses connections: {err!r}")
try:
    control.sendall((sign("cpus") + "\n").encode())
    raw = control.recv(4096).decode(errors="replace")
except OSError:
    raw = ""
finally:
    control.close()
if not raw.strip() or not parse_reply(raw.splitlines()[0])[0]:
    fail(
        f"after {HOLD} unfinished command lines the control plane is gone; "
        "six sockets took the machine over"
    )

# And HTTP, which shares the eight-slot table, must be unaffected.
try:
    web = connect(HTTP_PORT, timeout=6.0)
    web.sendall(b"GET /health HTTP/1.1\r\nHost: x\r\n\r\n")
    reply = web.recv(4096)
    web.close()
except OSError as err:
    reply = b""
    _ = err
if b"200 OK" not in reply:
    fail(f"HTTP was starved of slots by the command port: {reply[:60]!r}")

for sock in held:
    try:
        sock.close()
    except OSError:
        pass

ok(f"{HOLD} unfinished command lines released their slots; both ports still serve")
