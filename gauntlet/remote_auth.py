"""The remote control plane must not answer to a published secret.

The kernel's compiled-in default token is a `pub const` in this repository.
An image that accepts it is controllable by anyone who has read the source.
Each build therefore bakes its own random secret into the boot config, and
the kernel refuses to open its remote ports without one.

This checks the three cases that matter: the real secret works, the published
default does not, and a caller who signs with the right secret but names a
command that is not on the read-only allowlist is refused.
"""

import os
import socket

from _harness import COMMAND_PORT, connect, fail, ok
from _sign import caller_key, parse_reply, sign

REPO_TOKEN = "pandagen-dev"


def ask(command, key=None, timeout=4.0):
    """Send one signed line; return the reply text, or None on silence."""
    line = sign(command, key=key) + "\n"
    try:
        sock = connect(COMMAND_PORT, timeout=timeout)
    except OSError as err:
        return f"<no connection: {err!r}>"
    try:
        sock.sendall(line.encode())
        raw = sock.recv(4096).decode(errors="replace")
    except OSError:
        return None
    finally:
        sock.close()
    if not raw.strip():
        return None
    return raw.splitlines()[0]


# 1. The secret this image was actually built with.
reply = ask("cpus")
if reply is None:
    fail("the build's own secret was not accepted; remote control is broken")
accepted, text = parse_reply(reply)
if not accepted or "online=" not in text:
    fail(f"the build's own secret produced {text!r}")

# 2. The secret published in the repository must not work.
published = caller_key(master=REPO_TOKEN.encode())
reply = ask("cpus", key=published)
if reply is not None:
    accepted, text = parse_reply(reply)
    if accepted:
        fail(
            "the kernel accepted a command signed with the token published in "
            "this repository; anyone with the source can drive this machine"
        )

# 3. A correctly signed request for something off the read-only allowlist.
for forbidden in ("halt", "boot", "write secret.txt x"):
    reply = ask(forbidden)
    if reply is None:
        continue
    accepted, text = parse_reply(reply)
    if accepted:
        fail(f"remote caller was allowed to run {forbidden!r}: {text!r}")

ok("build secret accepted, published secret refused, allowlist enforced")
