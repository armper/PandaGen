# Phase 257: Bare-Metal Networking — virtio-net, ARP, ICMP Ping

## Summary

First real network traffic from the kernel (`docs/next_steps.md` item 4). `cargo xtask qemu*` now attaches `virtio-net-pci` on QEMU user-mode networking, and the kernel can ping the gateway.

- `hal_x86_64::pci`: virtio-net device ids, `is_virtio_net`, and a generic `find_on_bus0(predicate)` that `find_virtio_blk` / `find_virtio_net` share.
- `hal_x86_64::virtio_net::VirtioNetDevice<T: VirtioTransport>`: legacy virtio-net over the same transport trait as the block driver. Queue 0 receives into pre-posted 2 KiB buffers that are re-posted after each frame; queue 1 transmits with a two-descriptor chain (10-byte header, frame) and polls for completion; the MAC comes from device config. The descriptor's unused `next` field records which buffer slot a receive descriptor owns. Fake-transport tests cover MAC/buffer posting, transmit framing and descriptor recycling, receive header stripping and re-posting, and 50 receives through 3 buffers.
- New `net_stack` crate (no_std, no alloc): Ethernet II, ARP, IPv4 (checksum-verified, options skipped), ICMP echo; `Interface` answers ARP requests and echo requests for its address, learns MACs from ARP, and runs a ping (`NeedArp` first if the next hop is unknown, then an echo request matched by ident and sequence). `Config::qemu_user` is 10.0.2.15/24 via 10.0.2.2. Tests: RFC 1071 checksum example, address parsing and formatting, ARP request/reply round trip with cache learning, cache replacement and eviction, ping needing ARP then matching only the right reply, and echo-request replies with corrupted/foreign frames dropped.
- `kernel_bootstrap::bare_metal_net::NetStack`: probes PCI, lays out two legacy queues and the DMA buffers in page-aligned statics (physical addresses via the image mapping, as for storage), and drives the interface from the command path with tick-based timeouts (1 s per ARP or echo wait, 3 ARP attempts). The kernel logs `net: virtio-net-pci mac=... ip=...` at boot.
- Commands: `net` / `net status` (MAC, IP, gateway, frame and protocol counters) and `net ping <ip>`; `net poll` drains the receive queue (answering ARP and pings addressed to us).

## Verification

- `cargo test --workspace` green (hal_x86_64 103, net_stack 6).
- QEMU: `net ping 10.0.2.2` twice prints `reply from 10.0.2.2: seq=1 ttl=255 time=0 ticks` and `seq=2`; `net status` afterwards shows `rx=3 tx=3 arp_cache=1 echo_sent=2 echo_recv=2 dropped=0` (one ARP exchange plus two echo round trips).

## Not Yet

No interrupts (receive is polled while a command waits), no DHCP, UDP, or TCP, and no way for the host to reach the guest (QEMU user networking does not route inbound without port forwarding). UDP would be the natural next step, since it makes the remote IPC transport in `remote_ipc` reachable from outside.
