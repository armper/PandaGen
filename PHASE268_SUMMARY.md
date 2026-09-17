# Phase 268: DHCP Client

## Summary

The kernel no longer assumes QEMU's fixed user-network addresses: it obtains its address, netmask, gateway, DNS, and lease from a DHCP server at boot (`docs/next_steps.md` item 4).

- `net_stack::dhcp`: BOOTP/DHCP request builder (DISCOVER and REQUEST with client id, requested address, server id, parameter list; broadcast flag set; padded to 300 bytes), reply parser (message type, `yiaddr`, subnet mask, router, DNS, server id, lease), and a `Client` state machine `Init -> Selecting -> Requesting -> Bound` that ignores foreign transaction ids and mismatched ACKs and returns to `Init` on NAK. Host tests cover the header layout, option parsing and rejection of malformed replies, the full handshake, and NAK.
- `net_stack::Interface`: `Config::unconfigured(mac)` (0.0.0.0), `set_config`, and `udp_broadcast` (limited broadcast without ARP). While unconfigured, IPv4 frames unicast to our MAC are accepted as ours (DHCP servers reply to the offered address before the client has it); the limited broadcast address is always ours.
- `kernel_bootstrap::bare_metal_net`: starts unconfigured with port 68 bound, runs `dhcp()` right after probing the device (1 s budget) and falls back to the static QEMU configuration if it fails; `net status` reports `via dhcp` or `via static` plus lease and DNS; `net dhcp` re-runs the exchange. The UDP echo service now answers only on its own port, so DHCP replies are never echoed.

## Verification

- `cargo test --workspace` green (net_stack 14 tests).
- QEMU: boot logs `net: dhcp bound ip=10.0.2.15 mask=255.255.255.0 gw=10.0.2.2 lease=86400s`; `net` shows `via dhcp` and `dns=10.0.2.3`; `net ping 10.0.2.2` is answered; a host `udp:` echo and a `remote:net` call both succeed on the leased address; `net dhcp` binds again.

## Limits

Leases are not renewed (the lease is only reported), and a failed exchange falls back to the QEMU static addresses rather than retrying with backoff.
