"""Send a request, half-close, then expect the reply.

This is the `echo request | nc host port` idiom. The client shuts down its
write side to say "that is the whole request"; the server must still be able
to answer. RFC 9293 calls this a half close, and every Linux line server
supports it.
"""

import socket

from _harness import ECHO_PORT, connect, drain, fail, ok

payload = b"half-close-probe\n"

sock = connect(ECHO_PORT, timeout=10.0)
sock.sendall(payload)
sock.shutdown(socket.SHUT_WR)
got = drain(sock)
sock.close()

if got == payload:
    ok("reply survived the client's half close")
if not got:
    fail("no reply at all after half close; the server answered nothing")
fail(f"wrong reply after half close: wanted {payload!r}, got {got!r}")
