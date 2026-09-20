"""Per-slot state must not be applied to the next client in that slot.

A client that declares a body and leaves has an unpaid remainder recorded
against its TCP slot. Nothing cleared it when the connection ended, and
`Tcp::accept` picks the first closed slot -- so the next client's request
was drained as body bytes and never answered, and it was reset ten seconds
later by a deadline it had only just started. Six of those and the HTTP
server answers nobody, unauthenticated, at negligible traffic.

This is the same defect as `http_deadline_reuse`, in the array the same
commit added.
"""

import socket

from _harness import HTTP_PORT, connect, fail, ok

ABANDONED = 6

# Each of these makes a keep-alive request that declares a body it never
# sends, waits for its answer, and then hangs up. The answer matters: the
# unpaid remainder is only recorded for a connection the server expects to
# reuse, and closing before the handshake completes would test something
# else entirely (slirp leaves those half-open).
for index in range(ABANDONED):
    try:
        sock = connect(HTTP_PORT, timeout=6.0)
    except OSError as err:
        fail(f"could not open connection {index}: {err!r}")
    try:
        sock.sendall(
            b"GET /health HTTP/1.1\r\nHost: x\r\n"
            b"Connection: keep-alive\r\nContent-Length: 100000\r\n\r\n"
        )
        reply = sock.recv(4096)
        if b"200 OK" not in reply:
            fail(f"connection {index} was not answered: {reply[:60]!r}")
    except OSError as err:
        fail(f"connection {index} failed: {err!r}")
    finally:
        sock.close()

# Every slot has now held one. An ordinary request must still be answered.
for attempt in range(ABANDONED + 2):
    try:
        fresh = connect(HTTP_PORT, timeout=4.0)
    except OSError as err:
        fail(
            f"request {attempt} could not connect after {ABANDONED} abandoned "
            f"bodies: {err!r}"
        )
    try:
        fresh.sendall(b"GET /health HTTP/1.1\r\nHost: x\r\n\r\n")
        reply = fresh.recv(4096)
    except OSError as err:
        fail(
            f"request {attempt} was reset: {err!r}. It inherited an unpaid body "
            "from a client that had already gone."
        )
    finally:
        fresh.close()
    if b"200 OK" not in reply:
        fail(
            f"request {attempt} got {reply[:60]!r}; its request was drained as "
            "somebody else's body"
        )

ok(f"{ABANDONED} abandoned bodies did not follow their slots to the next client")
