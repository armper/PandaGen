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

DEFAULT_MASTER = "pandagen-dev"


def master_token():
    return os.environ.get("PANDAGEN_REMOTE_TOKEN", DEFAULT_MASTER).encode()


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
    nonce = nonce if nonce is not None else secrets.randbits(128)
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
