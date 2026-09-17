# Phase 276: DHCP Lease Renewal

## Summary

Leases obtained in Phase 268 are now kept alive (`docs/next_steps.md` item 4).

- `net_stack::dhcp::Client::renew(ip, server)`: a REQUEST in the RENEWING state (client address in `ciaddr`, no requested-address or server-id options, fresh transaction id) to be unicast to the granting server; the server's ACK re-binds through the existing path. `Step::Bound` now carries the server. Tested: renewal request layout and re-bind on ACK.
- `kernel_bootstrap::bare_metal_net`: remembers the server and the tick the lease started; `service` renews automatically once half the lease has elapsed, and a failed renewal falls back to a fresh DISCOVER. `net renew` forces a renewal (QEMU's lease is 24 h, so the automatic path is not observable there); `net status` reports `renewals=` and the server.

## Verification

- `cargo test --workspace` green (net_stack 23 tests).
- QEMU: `net renew` logs `net: dhcp renewed ip=10.0.2.15 ... server=10.0.2.2`, status shows `renewals=1`, and UDP echo plus TCP remote calls keep working on the renewed lease.
