"""One ordinary HTTP request against a short deadline.

A cheap liveness probe: the machine answers /health within a second and a
half. Used in the suite after a `net ping` to an unreachable address, which
holds the network lock for about three seconds while it spins.

Honest note: this does **not** demonstrate that stall. I could not build a
judge that does -- the request survives in the receive buffer and is
answered when the spin ends, so the defect is a stall rather than a loss,
and slirp's buffering plus the keystroke latency kept every version of this
test under its deadline on both the fixed and the unfixed kernel. The fix
(keeping TCP's timers turning and its queued output moving during those
waits) rests on reading the code, not on this.
"""


from _harness import HTTP_PORT, connect, fail, ok

try:
    sock = connect(HTTP_PORT, timeout=1.5)
except OSError as err:
    fail(f"the HTTP port refused a connection: {err!r}")
try:
    sock.settimeout(1.5)
    sock.sendall(b"GET /health HTTP/1.1\r\nHost: x\r\n\r\n")
    reply = sock.recv(4096)
except OSError as err:
    fail(f"the HTTP port accepted a connection and then went silent: {err!r}")
finally:
    sock.close()

if b"200 OK" not in reply:
    fail(f"/health returned {reply[:60]!r}")

ok("the stack served an ordinary request")
