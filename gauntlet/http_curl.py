"""Real `curl` must work against PandaGen.

curl is the harshest judge available here: it is unmodified, it has never
heard of this kernel, and it fails loudly on anything it does not consider
valid HTTP. Content-Length that disagrees with the body, a missing blank
line, a connection that closes early, a header block it cannot parse.

This covers the status page, a small text route, a 404, a streamed body far
larger than the TCP send buffer, keep-alive across two requests on one
connection, and HEAD.
"""

import shutil
import subprocess

from _harness import HTTP_PORT, fail, ok

BASE = f"http://127.0.0.1:{HTTP_PORT}"
PATTERN = b"PandaGen-stream\n"

if shutil.which("curl") is None:
    fail("curl is not installed; this judge is not optional")


def curl(args, timeout=20):
    """Run curl and return (exit status, stdout bytes, stderr text)."""
    result = subprocess.run(
        ["curl", "-sS", "--max-time", "15", *args],
        capture_output=True,
        timeout=timeout,
        check=False,
    )
    return result.returncode, result.stdout, result.stderr.decode(errors="replace")


# 1. The status page, with curl enforcing the framing.
code, body, err = curl(["-i", f"{BASE}/"])
if code != 0:
    fail(f"curl could not fetch the status page: {err.strip() or code}")
text = body.decode(errors="replace")
if not text.startswith("HTTP/1.1 200 OK\r\n"):
    fail(f"status page did not start with a 200 status line: {text[:60]!r}")
if "Content-Length:" not in text:
    fail("status page has no Content-Length, so a client cannot frame it")
head, _, page = text.partition("\r\n\r\n")
declared = int(
    [line for line in head.split("\r\n") if line.lower().startswith("content-length:")][
        0
    ].split(":")[1]
)
if declared != len(page.encode()):
    fail(f"Content-Length says {declared} but the body is {len(page.encode())} bytes")
if "PandaGen" not in page or "cpus" not in page:
    fail(f"status page body does not look like the status page: {page[:80]!r}")

# 2. A small text route.
code, body, err = curl([f"{BASE}/health"])
if code != 0 or body != b"ok\n":
    fail(f"/health returned {body!r} (curl {code}) {err.strip()}")

# 3. An unknown route is a clean 404, not a hang or a reset.
code, body, err = curl(["-o", "/dev/null", "-w", "%{http_code}", f"{BASE}/no-such-page"])
if code != 0:
    fail(f"curl failed on an unknown route instead of getting a 404: {err.strip()}")
if body != b"404":
    fail(f"unknown route returned {body!r}, expected 404")

# 4. A body far larger than the 2 KiB TCP send buffer, so it must be
#    streamed across many passes, and must arrive byte for byte.
size = 200_000
code, body, err = curl(["--max-time", "30", f"{BASE}/bytes/{size}"], timeout=40)
if code != 0:
    fail(f"curl failed on a {size}-byte streamed body: {err.strip() or code}")
if len(body) != size:
    fail(f"streamed body is {len(body)} bytes, expected {size}")
want = (PATTERN * (size // len(PATTERN) + 1))[:size]
if body != want:
    index = next(i for i in range(size) if body[i] != want[i])
    fail(f"streamed body diverges at byte {index}: {body[index - 8:index + 8]!r}")

# 5. Keep-alive: two requests on one connection. curl reuses the connection
#    when the server's framing is trustworthy, and complains when it is not.
code, body, err = curl(["-i", f"{BASE}/health", f"{BASE}/health"])
if code != 0:
    fail(f"two requests over one connection failed: {err.strip() or code}")
if body.count(b"HTTP/1.1 200 OK") != 2:
    fail(f"expected two responses on one connection, got {body.count(b'HTTP/1.1 200 OK')}")

# 6. HEAD must send the headers and no body.
code, body, err = curl(["-I", f"{BASE}/health"])
if code != 0:
    fail(f"HEAD failed: {err.strip() or code}")
if b"200 OK" not in body:
    fail(f"HEAD did not return a 200: {body[:60]!r}")

ok(f"curl: status page, /health, 404, {size}-byte stream, keep-alive, HEAD")
