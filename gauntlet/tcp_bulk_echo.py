"""Echo 64 KiB and require it back byte for byte.

An echo server returns the stream unmodified. Flow control is what the
receive window is for: a server that cannot keep up must stop acknowledging,
never silently discard. A short or holed reply is data corruption.
"""

import hashlib
import socket
import threading

from _harness import ECHO_PORT, connect, drain, fail, ok

SIZE = 64 * 1024
data = bytes((i * 7 + 3) & 0xFF for i in range(SIZE))

sock = connect(ECHO_PORT, timeout=25.0)

send_error = []


def send_all():
    try:
        sock.sendall(data)
        sock.shutdown(socket.SHUT_WR)
    except Exception as err:  # noqa: BLE001
        send_error.append(err)


sender = threading.Thread(target=send_all)
sender.start()
got = drain(sock, limit=SIZE * 2)
sender.join(timeout=30)
sock.close()

if send_error:
    fail(f"sending {SIZE} bytes failed: {send_error[0]!r}")

if got == data:
    ok(f"{SIZE} bytes echoed byte for byte")

# Characterise the damage so the fix is aimed correctly.
prefix = 0
while prefix < min(len(got), len(data)) and got[prefix] == data[prefix]:
    prefix += 1

detail = (
    f"sent {len(data)} bytes, got {len(got)} back; "
    f"first {prefix} bytes match. "
    f"sha256 sent={hashlib.sha256(data).hexdigest()[:16]} "
    f"got={hashlib.sha256(got).hexdigest()[:16]}"
)
if prefix == len(got) < len(data):
    fail(f"echo truncated (no corruption, just short): {detail}")
fail(f"echo corrupted: a hole starts at byte {prefix}: {detail}")
