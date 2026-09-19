"""Idle connections to the echo port must not deny service to the command port.

Four clients connect to the unauthenticated echo port and simply hold the
connections open, which is what any idle client does. Both TCP services share
one connection table, so if nothing reaps idle connections, four sockets take
the machine's signed control plane offline until reboot.

A Linux server keeps accepting. At minimum it refuses with a reset rather
than black-holing the connection.
"""

import socket

from _harness import COMMAND_PORT, ECHO_PORT, connect, fail, ok
from _sign import parse_reply, sign

HOLD = 4
held = []

for index in range(HOLD):
    try:
        held.append(connect(ECHO_PORT, timeout=8.0))
    except OSError as err:
        fail(f"could not open idle connection {index}: {err!r}")

# Hold them. Send nothing, close nothing: an ordinary idle client.
try:
    try:
        control = connect(COMMAND_PORT, timeout=6.0)
    except OSError as err:
        fail(
            f"{HOLD} idle connections to the echo port took the command port "
            f"offline: {err!r}. Unauthenticated clients can deny the control plane."
        )

    try:
        control.sendall((sign("cpus") + "\n").encode())
        reply = control.recv(4096).decode(errors="replace")
    except OSError as err:
        control.close()
        fail(f"command port accepted a connection but could not serve it: {err!r}")
    control.close()

    if not reply.strip():
        fail(
            f"command port connected but answered nothing while {HOLD} idle "
            "connections were held"
        )
    accepted, text = parse_reply(reply.splitlines()[0])
    if not accepted:
        fail(f"command rejected while idle connections were held: {text}")
    ok(f"command port still served while {HOLD} idle connections were held")
finally:
    for sock in held:
        try:
            sock.close()
        except OSError:
            pass
