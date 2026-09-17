# Phase 258: UDP And A Host-Reachable Echo Service

## Summary

Continues `docs/next_steps.md` item 4. The kernel now speaks UDP, and for the first time the host can send traffic into the guest and get an answer.

- `net_stack::wire::Udp`: header parse/build with the IPv4 pseudo-header checksum (zero checksum accepted on receive, `0xFFFF` substituted on send), plus `build_udp` for complete frames.
- `net_stack::Interface`: `bind`/`unbind` (up to 4 ports), `Event::Udp` carrying the payload's offset and length inside the received frame, `udp_send` with the same `NeedArp` contract as `ping`, and counters for received, sent, and unbound datagrams. IPv4 frames addressed to us now also teach the ARP cache the sender's MAC, because the QEMU gateway forwards host traffic without ever ARPing us.
- `kernel_bootstrap::bare_metal_net`: binds port 7777 at boot; `service()` runs from the main loop whenever the stack is not held by a command, answering ARP, ping, and echoing UDP datagrams (logged as `net: udp echo N bytes to ip:port`). `net udp <ip> <port> [text]` sends a datagram; `net status` gains a UDP line.
- `cargo xtask qemu*` forwards host UDP `127.0.0.1:7777` to the guest. `qemu-script` gains a `udp:<text>` step that sends the text from the host and fails the run unless the identical bytes come back within 2 s.

## Verification

- `cargo test --workspace` green (net_stack 9 tests: UDP checksum round trip and rejection cases, delivery only to bound ports with neighbour learning, bind slot limits).
- QEMU: two `udp:` steps from the host both echo (`udp echo ok`); serial shows `net: udp echo 15 bytes to 10.0.2.2:<port>` twice and `net status` reports `udp port 7777 recv=2 sent=2 echoed=2 unbound=0`; `net udp 10.0.2.2 99 hi` transmits without error.

## Next

Carry `remote_ipc` envelopes over UDP so a host tool can call kernel capabilities; then interrupts for receive so servicing does not depend on the main loop's cadence.
