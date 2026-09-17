# Phase 252: PCI Enumeration And virtio-pci Legacy Transport

## Summary

First step of the persistent bare-metal storage track (`docs/next_steps.md` item 2). `cargo xtask qemu` attaches `virtio-blk-pci`, but the kernel only probed virtio MMIO windows, so storage always fell back to a RAM disk and nothing survived a reboot.

- `hal_x86_64::port_io::PortIo` gains `inw`/`outw`/`inl`/`outl`. `RealPortIo` uses native 16/32-bit `in`/`out`; the trait defaults compose little-endian byte accesses so `FakePortIo` scripts keep working, with `script_read16`/`script_read32`/`last_write32` helpers.
- `hal_x86_64::pci`: legacy 0xCF8/0xCFC configuration access, `probe`, `enumerate_bus`, `find_virtio_blk` (vendor 0x1AF4, device 0x1001 transitional or 0x1042 modern), BAR0 I/O decoding, and `enable_io_and_bus_master` which preserves the status half of the command/status register.
- `hal_x86_64::virtio_pci`: the legacy I/O register block (`VirtioPciLegacy`) and `LegacyQueueLayout`, which places the descriptor table, available ring, and used ring (page-aligned) in one contiguous region as legacy virtio requires.

All of it is host-tested against `FakePortIo` byte for byte: config-address encoding, enumeration finding the device in slot 4 with an I/O BAR, bus-master enable, legacy layout for 128 and 256 entries, and the transport's register traffic including PFN and notify writes.

## Next

Phase 253 wires the transport into the virtio-blk driver with physical addresses for the queue and bounce buffers, probes PCI before the MMIO fallback, and mounts an existing on-disk filesystem instead of reformatting at every boot.
