"""Four clients connect at once and each expects its own echo.

A Linux echo server serves all four. This is the concurrency question: does
every connection that completes a handshake actually get served, or do
simultaneous arrivals lose their replies?
"""

import threading

from _harness import ECHO_PORT, connect, fail, ok, read_exactly

CLIENTS = 4
results = {}


def client(index):
    payload = f"client-{index}\n".encode()
    try:
        sock = connect(ECHO_PORT, timeout=10.0)
        sock.sendall(payload)
        got = read_exactly(sock, len(payload))
        sock.close()
        results[index] = got
    except Exception as err:  # noqa: BLE001 - any failure is a finding
        results[index] = err


threads = [threading.Thread(target=client, args=(i,)) for i in range(CLIENTS)]
for thread in threads:
    thread.start()
for thread in threads:
    thread.join()

bad = []
for index in range(CLIENTS):
    want = f"client-{index}\n".encode()
    got = results.get(index)
    if isinstance(got, Exception):
        bad.append(f"client {index}: {got!r}")
    elif got != want:
        bad.append(f"client {index}: wanted {want!r}, got {got!r}")

if bad:
    fail(f"{len(bad)} of {CLIENTS} concurrent clients failed: " + "; ".join(bad))
ok(f"all {CLIENTS} concurrent clients echoed correctly")
