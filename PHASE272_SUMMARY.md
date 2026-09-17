# Phase 272: TCP

## Summary

The kernel now speaks TCP (`docs/next_steps.md` item 4): standard tools can open a connection to it. This phase ships the protocol core and an echo service; the authenticated command channel over TCP follows.

- `net_stack::wire::Tcp`: segment parse/build with the pseudo-header checksum, data-offset handling, and an MSS option on SYN|ACK; `build_tcp` frames complete Ethernet/IPv4/TCP packets.
- `net_stack::tcp::Tcp`: a server-side state machine for up to four connections: passive open (SYN -> SYN|ACK -> ACK), in-order receive with immediate ACKs (out-of-order data is dropped and re-acknowledged), one outstanding send segment within the peer's window, retransmission after a 1 s timeout with abandonment after five tries, passive close (FIN -> CloseWait -> LastAck) and active close (FinWait1/FinWait2), RST handling, and RST replies to segments for nothing that listens. 2 KiB receive and send buffers per connection. Six host tests cover the handshake with echo and close, active close, out-of-order drop, retransmission and give-up, zero/small windows plus RST, and resets plus a full table.
- `net_stack::Interface`: `tcp_listen`, TCP dispatch in the IPv4 path yielding `Event::TcpReady { conn }`, `tcp_tick` for timers, and `tcp_next_frame`, which frames immediate replies first and then data, FINs, and retransmissions using the learned neighbour MAC.
- `kernel_bootstrap::bare_metal_net`: listens on TCP 7779 and echoes whatever arrives, closing after the peer does; `service` now takes the tick count, flushes pending segments after each pass, and `net status` reports connections, accepted count, segments in/out, retransmits, resets, and echoed bytes. The boot stack grew from 64 KiB to 256 KiB: the network stack (now carrying 16 KiB of TCP buffers) is built as a stack temporary in `rust_main`, and the first attempt double-faulted with `cr2` just below `rsp` (Phase 262's handler made that a one-line diagnosis).
- `cargo xtask qemu*` forwards host TCP 127.0.0.1:7779; `qemu-script` gains `tcp:<text>`, which connects with a real `TcpStream`, sends the line, requires it back, half-closes, and requires EOF.

## Verification

- `cargo test --workspace` green (net_stack 21 tests).
- QEMU: `tcp:hello-tcp` and `tcp:second-line-over-tcp` both echo and close cleanly from host sockets; serial logs `net: tcp conn0 from 10.0.2.2:<port> (SynReceived)` for each, and `net` afterwards shows `conns=0` with the accepted/segment counters; UDP echo keeps working alongside.

## Limits

No congestion control, no out-of-order reassembly, one segment in flight per connection, no TIME_WAIT (a late peer segment after our close is answered with RST), and no active open.
