"""Two requests in one write; expect two replies without further prodding.

A server drains what it has buffered. If it only processes one request per
arriving packet, the second reply is stuck until unrelated traffic shakes it
loose, and any batching client hangs.
"""

from _harness import ECHO_PORT, connect, fail, ok, read_exactly

first = b"line-one\n"
second = b"line-two\n"

sock = connect(ECHO_PORT, timeout=8.0)
sock.sendall(first + second)
got = read_exactly(sock, len(first) + len(second))

if got == first + second:
    sock.close()
    ok("both pipelined requests answered")

# Distinguish "stuck until poked" from "lost entirely", because the fix
# differs: the first is a missing drain loop, the second is data loss.
sock.sendall(b"poke\n")
after = read_exactly(sock, len(second) + len(b"poke\n"))
sock.close()

if got == first and second in after:
    fail(
        "second pipelined request stalled until unrelated traffic arrived "
        f"(first reply {got!r}, released only after a poke)"
    )
fail(f"pipelined replies wrong: got {got!r} then {after!r}")
