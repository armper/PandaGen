# Phase 278: The Gauntlet, Round 1 — A Renewal Hang And A Pipelining Stall

## Summary

First closed findings from the standing adversarial loop described in
`GAUNTLET.md`. Three fresh-context critics reviewed TCP against RFC 9293,
the packet parsers against hostile input, and the post-Phase-275 SMP locking.
Their claims were then held to the loop's rule: nothing counts until it
reproduces as a failing automated test.

Two fixes here, both reproduced first.

## Finding: DHCP renewal spins the boot CPU forever

`maybe_renew` built its clock as `let clock = move || now`, capturing the
current tick into a **constant**. The reply-wait inside `dhcp_exchange` reads
`while now().saturating_sub(start) < REPLY_TIMEOUT_TICKS`, which with a frozen
clock is `0 < 100` on every iteration. Once half the lease elapsed and no DHCP
server answered, the boot CPU spun there until reset: no panic, no exception,
no diagnostic. The comment I wrote beside it in Phase 276 asserted the
opposite ("cannot wait for replies here"), which is why it survived review.

`service` now takes a live `&dyn Fn() -> u64` and threads it through, so the
wait terminates. Found by the packet-parser critic, confirmed by inspection;
it is not reachable at either test tier, because provoking it needs a hostile
DHCP server offering a short lease and then going silent, and QEMU's slirp
offers a fixed 24-hour lease.

## Finding: the command port served one request per packet

`gauntlet/cmd_pipeline.py` writes two signed command lines in a single
`sendall` and waits for two replies. It failed: the second reply arrived only
after unrelated traffic was sent. `Event::TcpReady` fires only when a frame is
received, and `tcp_command_service` takes exactly one line per event, so a
batching client hung until something else woke the stack.

`service` now makes a drain pass: when no request came out of the receive
loop, it checks whether any command-port connection already has a complete
line buffered. The test passes.

## Verification

- `cargo test --workspace` green.
- `gauntlet:cmd_pipeline` fails before, passes after.
- No regression in `cmd_halfclose`, `tcp_concurrent`, `tcp_bulk_echo`.

## Harness added

- `qemu-script --port-base N` shifts the forwarded ports and uses a private
  disk image, so several QEMU instances run concurrently. Verified with two,
  then five, simultaneous boots.
- `qemu-script --keys "gauntlet:<name>"` runs `gauntlet/<name>.py` against the
  boot with the ports in its environment, and fails the run on a non-zero exit.
- `gauntlet/_sign.py` reimplements the signed line protocol in Python, so a
  test can control exactly what reaches the wire: where the line is split,
  whether the write side closes first, how many requests share a segment.
