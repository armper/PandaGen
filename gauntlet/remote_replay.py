"""A captured command must not become valid again by waiting.

The kernel remembers the last 256 nonces it accepted. That stops an
immediate replay, but it is a ring: once 256 further commands have been
accepted, a request captured earlier falls out of the window and is accepted
again. An attacker does not need the key for this -- only a packet capture
and the patience to let the operator work.

Capture one signed line, push it out of the window with ordinary traffic,
then send the captured line again. It must still be refused.
"""

from _harness import COMMAND_PORT, connect, fail, ok
from _sign import parse_reply, sign

WINDOW = 256


def send(line, timeout=5.0):
    """Send one line; return the reply text, or None on silence."""
    sock = connect(COMMAND_PORT, timeout=timeout)
    try:
        sock.sendall((line + "\n").encode())
        raw = sock.recv(4096).decode(errors="replace")
    except OSError:
        return None
    finally:
        sock.close()
    return raw.splitlines()[0] if raw.strip() else None


# The line an attacker captures off the wire.
captured = sign("cpus")
reply = send(captured)
if reply is None or not parse_reply(reply)[0]:
    fail(f"the captured line was not accepted in the first place: {reply!r}")

# An immediate replay must be refused. This much already worked.
reply = send(captured)
if reply is not None and parse_reply(reply)[0]:
    fail("an immediate replay was accepted")

# Now the operator does ordinary work, and the window rolls over.
for index in range(WINDOW + 4):
    if send(sign("ticks")) is None:
        fail(f"the machine stopped answering after {index} ordinary commands")

# The captured line, sent again, long after.
reply = send(captured)
if reply is not None and parse_reply(reply)[0]:
    fail(
        f"a command captured {WINDOW + 4} requests ago was accepted again: "
        f"{parse_reply(reply)[1]!r}. The replay window is a ring, so waiting "
        "is enough to make a captured request valid."
    )

ok(f"a captured request is still refused after {WINDOW + 4} later commands")
