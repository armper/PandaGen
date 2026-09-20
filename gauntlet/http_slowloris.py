"""A client that never finishes its request must not hold a slot for ever.

Eight connection slots serve every port on this machine, six per port. TCP's
reaper asks whether a segment was exchanged, not whether the request made
progress -- so a client sending one byte every so often looked perfectly
alive and kept its slot until the 120-second idle timeout. Six such sockets
took the whole machine off the network, including the signed command port,
with no authentication and almost no traffic.

Six dribbling sockets, then an ordinary request and a control-plane call.
"""

import socket
import time

from _harness import COMMAND_PORT, HTTP_PORT, connect, fail, ok
from _sign import parse_reply, sign

HOLD = 6
# The kernel's head deadline is ten seconds; dribble for comfortably longer.
DRIBBLE_SECONDS = 14

held = []
for index in range(HOLD):
    try:
        sock = connect(HTTP_PORT, timeout=6.0)
    except OSError as err:
        fail(f"could not open slow connection {index}: {err!r}")
    # A request head that never ends.
    sock.sendall(b"GET / HTTP/1.")
    held.append(sock)

deadline = time.time() + DRIBBLE_SECONDS
while time.time() < deadline:
    time.sleep(1.0)
    for sock in held:
        try:
            sock.sendall(b"1")
        except OSError:
            pass

# An ordinary client must still be served.
try:
    fresh = connect(HTTP_PORT, timeout=6.0)
except OSError as err:
    fail(
        f"after {HOLD} unfinished requests the HTTP port refuses connections: "
        f"{err!r}"
    )
try:
    fresh.sendall(b"GET /health HTTP/1.1\r\nHost: x\r\n\r\n")
    reply = fresh.recv(4096)
except OSError as err:
    reply = b""
    _ = err
finally:
    fresh.close()
if b"200 OK" not in reply:
    fail(
        f"after {HOLD} unfinished requests an ordinary request got {reply[:60]!r}; "
        "the connection table is held by clients that never finished"
    )

# And the control plane, on another port, must be unaffected.
try:
    control = connect(COMMAND_PORT, timeout=6.0)
except OSError as err:
    fail(f"the command port refuses connections: {err!r}")
try:
    control.sendall((sign("cpus") + "\n").encode())
    raw = control.recv(4096).decode(errors="replace")
finally:
    control.close()
if not raw.strip() or not parse_reply(raw.splitlines()[0])[0]:
    fail("the command port is gone; six slow HTTP sockets took the machine off the network")

for sock in held:
    try:
        sock.close()
    except OSError:
        pass

ok(f"{HOLD} unfinished requests released their slots; the machine still serves")
