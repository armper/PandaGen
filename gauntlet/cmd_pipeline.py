"""Two signed commands in one write must produce two replies.

A server drains what it has buffered. If it processes exactly one request per
arriving packet, the second request sits in the receive buffer until
unrelated traffic happens to arrive, and any client that batches hangs.
"""

from _harness import COMMAND_PORT, connect, fail, ok
from _sign import parse_reply, sign


def read_line(sock, buffer):
    """Pull one newline-terminated line, extending `buffer` as needed."""
    while b"\n" not in buffer:
        try:
            chunk = sock.recv(4096)
        except OSError:
            return None, buffer
        if not chunk:
            return None, buffer
        buffer += chunk
    line, _, rest = buffer.partition(b"\n")
    return line.decode(errors="replace"), rest


batch = (sign("cpus") + "\n" + sign("ticks") + "\n").encode()

sock = connect(COMMAND_PORT, timeout=6.0)
sock.sendall(batch)

buffer = b""
first, buffer = read_line(sock, buffer)
if first is None:
    sock.close()
    fail("no reply at all to the first of two pipelined commands")

second, buffer = read_line(sock, buffer)
if second is not None:
    sock.close()
    accepted_a, text_a = parse_reply(first)
    accepted_b, text_b = parse_reply(second)
    if accepted_a and accepted_b:
        ok("both pipelined commands answered")
    fail(f"pipelined commands answered but rejected: {text_a!r}, {text_b!r}")

# Nothing came back. Distinguish "stuck until poked" from "lost entirely",
# because the fix differs: a missing drain loop versus dropped input.
try:
    sock.sendall(b"\n")
    third, _ = read_line(sock, buffer)
except OSError as err:
    sock.close()
    fail(f"second pipelined reply never arrived and the socket died: {err!r}")
sock.close()

if third is not None:
    fail(
        "second pipelined command stalled until unrelated traffic arrived; "
        "the server reads one request per packet instead of draining"
    )
fail("second pipelined command produced no reply even after a poke")
