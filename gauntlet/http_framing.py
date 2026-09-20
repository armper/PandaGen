"""A response must end where it says it ends.

Three ways this server used to lose track of its own framing, all on one
keep-alive connection, all deterministic and none dependent on segment
boundaries (which slirp would hide):

  HEAD returned a body. The method was parsed and then used only to pick
  405, so HEAD fell through to the same writes as GET. A conformant client
  stops reading at the headers, so the body became the start of the next
  response. `curl -I` never noticed because it discards the spare bytes.

  A declared body was not consumed. Only the head was drained, so the body
  stayed in the receive buffer and was parsed as the next request: one
  request produced two responses.

  Transfer-Encoding was ignored, so the chunked framing bytes were read as
  a request of their own.
"""

import socket

from _harness import HTTP_PORT, connect, fail, ok


def send(sock, text):
    sock.sendall(text.encode())


def read_headers(sock):
    """Read exactly up to the blank line; return (headers, leftover bytes)."""
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = sock.recv(4096)
        if not chunk:
            break
        data += chunk
    head, _, rest = data.partition(b"\r\n\r\n")
    return head.decode(errors="replace"), rest


def content_length(head):
    for line in head.split("\r\n"):
        if line.lower().startswith("content-length:"):
            return int(line.split(":", 1)[1])
    return None


# 1. HEAD must send the headers a GET would send, and no body at all.
sock = connect(HTTP_PORT)
try:
    send(sock, "HEAD /bytes/4096 HTTP/1.1\r\nHost: x\r\n\r\n")
    head, leftover = read_headers(sock)
    if "200 OK" not in head:
        fail(f"HEAD did not return 200: {head[:80]!r}")
    if content_length(head) != 4096:
        fail(f"HEAD must still describe the body a GET would send: {head!r}")
    # Anything after the blank line is body the client will misread as the
    # next response.
    sock.settimeout(1.5)
    try:
        leftover += sock.recv(65536)
    except (socket.timeout, OSError):
        pass
    if leftover:
        fail(
            f"HEAD returned {len(leftover)} bytes of body; a conformant client "
            f"reads them as the next response: {leftover[:32]!r}"
        )
finally:
    sock.close()

# 2. A declared body must be consumed, not parsed as the next request.
body = "GET /health HTTP/1.1\r\nHost: x\r\n\r\n"
sock = connect(HTTP_PORT)
try:
    send(sock, f"GET / HTTP/1.1\r\nHost: x\r\nContent-Length: {len(body)}\r\n\r\n{body}")
    head, rest = read_headers(sock)
    if "200 OK" not in head:
        fail(f"the request with a body was not answered: {head[:80]!r}")
    declared = content_length(head)
    while len(rest) < declared:
        chunk = sock.recv(65536)
        if not chunk:
            break
        rest += chunk
    extra = rest[declared:]
    sock.settimeout(1.5)
    try:
        extra += sock.recv(65536)
    except (socket.timeout, OSError):
        pass
    if b"HTTP/1.1" in extra:
        fail(
            "the request body crossed the framing boundary and was served as "
            "a second request: one request, two responses"
        )
finally:
    sock.close()

# 3. A framing this server does not implement must be refused, not ignored.
sock = connect(HTTP_PORT)
try:
    send(sock, "GET / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n")
    head, _ = read_headers(sock)
    if "501" not in head.split("\r\n")[0]:
        fail(
            f"chunked encoding must be refused with 501, not ignored: "
            f"{head.split(chr(13))[0]!r}"
        )
finally:
    sock.close()

ok("HEAD sends no body, a declared body is consumed, chunked is refused")
