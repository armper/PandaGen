"""A declared body must be consumed even when it arrives late.

Phase 311 drained head-plus-body in one call -- but the drain stops the
moment the receive buffer runs dry, and the parser answers as soon as the
blank line arrives. So a body split across segments was never consumed, and
arrived afterwards into an empty buffer where the next pass parsed it as a
request. That is the smuggle the phase claimed to close, surviving for any
body that is not already buffered: and a Content-Length above the 2 KiB
receive buffer can never be fully buffered, so that is every large body.

Send a head declaring a body, wait for the response, then send the body --
which is itself a valid request. One response, not two.
"""

import socket
import time

from _harness import HTTP_PORT, connect, fail, ok

BODY = b"GET /nothing-the-client-asked-for HTTP/1.1\r\nHost: x\r\n\r\n"


def read_available(sock, seconds=1.5):
    sock.settimeout(seconds)
    data = b""
    try:
        while True:
            chunk = sock.recv(65536)
            if not chunk:
                break
            data += chunk
    except (socket.timeout, OSError):
        pass
    return data

sock = connect(HTTP_PORT)
try:
    sock.sendall(
        f"GET /health HTTP/1.1\r\nHost: x\r\nContent-Length: {len(BODY)}\r\n\r\n".encode()
    )
    first = read_available(sock)
    if b"200 OK" not in first:
        fail(f"the request was not answered: {first[:80]!r}")
    if first.count(b"HTTP/1.1") != 1:
        fail(f"one request produced {first.count(b'HTTP/1.1')} responses before the body arrived")

    # The body, arriving in its own segment well after the response.
    time.sleep(0.5)
    sock.sendall(BODY)
    second = read_available(sock)
    if b"HTTP/1.1" in second:
        fail(
            "the request body was parsed as a second request: one request, two "
            f"responses. Got {second[:80]!r}"
        )
finally:
    sock.close()

ok("a body split across segments is consumed, not served as a request")
