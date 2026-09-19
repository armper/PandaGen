"""Sign command lines for PandaGen's TCP command port.

Mirrors `remote_ipc::line`: the request is

    <nonce as 32 hex digits> <caller> <base64 tag> <command>

where the tag is HMAC-SHA256 over `nonce_hex || 0 || caller || 0 || command`
under the caller's key, and the caller's key is itself
HMAC-SHA256(master, "caller:" || caller).

Reimplemented here rather than shelled out to `cargo xtask` so a gauntlet
script can control exactly what goes on the wire: when the line is split,
whether the write side is closed first, how many requests share a segment.
"""

import base64
import hashlib
import hmac
import os
import secrets
import time

DEFAULT_MASTER = "pandagen-dev"


def master_token():
    """The secret for the image under test.

    `cargo xtask iso` bakes a fresh random secret into each build and leaves
    it in dist/remote-token; the kernel refuses to open its remote ports
    without one. Fall back to the published development constant only when
    there is no build.
    """
    from_env = os.environ.get("PANDAGEN_REMOTE_TOKEN")
    if from_env:
        return from_env.encode()
    here = os.path.dirname(os.path.abspath(__file__))
    path = os.path.join(os.path.dirname(here), "dist", "remote-token")
    try:
        with open(path, encoding="utf-8") as handle:
            token = handle.read().strip()
            if token:
                return token.encode()
    except OSError:
        pass
    return DEFAULT_MASTER.encode()


def caller_name():
    return os.environ.get("PANDAGEN_REMOTE_CALLER", "gauntlet")


def caller_key(master=None, caller=None):
    master = master if master is not None else master_token()
    caller = caller if caller is not None else caller_name()
    return hmac.new(master, b"caller:" + caller.encode(), hashlib.sha256).digest()


def sign(command, caller=None, key=None, nonce=None):
    """One request line, without the trailing newline."""
    caller = caller if caller is not None else caller_name()
    key = key if key is not None else caller_key(caller=caller)
    # Nanoseconds since the epoch on top, randomness underneath. The kernel
    # refuses a nonce that has fallen behind the newest one it has seen, so
    # a captured line goes stale rather than coming back into range when the
    # replay window rolls over.
    if nonce is None:
        nonce = (time.time_ns() << 64) | secrets.randbits(64)
    nonce_hex = f"{nonce:032x}"
    payload = nonce_hex.encode() + b"\x00" + caller.encode() + b"\x00" + command.encode()
    tag = hmac.new(key, payload, hashlib.sha256).digest()
    return f"{nonce_hex} {caller} {base64.b64encode(tag).decode()} {command}"


def parse_reply(line):
    """Decode a reply line into (ok, text)."""
    line = line.rstrip("\r\n")
    if line.startswith("+"):
        return True, base64.b64decode(line[1:]).decode(errors="replace")
    if line.startswith("-"):
        return False, line[1:]
    return False, f"malformed reply {line!r}"
