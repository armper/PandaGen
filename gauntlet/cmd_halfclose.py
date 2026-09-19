"""`echo <command> | nc host 7780` must produce a reply.

The client sends one signed command line and then shuts down its write side,
which is what a pipe into `nc` does and what Linux does whenever an
application writes and immediately closes. The FIN rides on the same segment
as the data. A server that treats "peer closed" as "conversation over" loses
the reply, even though it already ran the command.

RFC 9293: CLOSE-WAIT is a *half* close. The local side may still send.
"""

import socket

from _harness import COMMAND_PORT, connect, drain, fail, ok
from _sign import parse_reply, sign

line = sign("cpus") + "\n"

sock = connect(COMMAND_PORT, timeout=10.0)
sock.sendall(line.encode())
sock.shutdown(socket.SHUT_WR)
raw = drain(sock).decode(errors="replace")
sock.close()

if not raw.strip():
    fail(
        "no reply after half close: the command port answered nothing. "
        "The command still ran kernel-side, so the result is silently lost."
    )

accepted, text = parse_reply(raw.splitlines()[0])
if not accepted:
    fail(f"command rejected after half close: {text}")
if "online=" not in text:
    fail(f"unexpected reply after half close: {text!r}")
ok(f"reply survived the client's half close: {text.splitlines()[0]}")
