# Phase 253: Transport-Generic virtio-blk Driver, PCI Probe, And Persistent Mounts

## Summary

Second and final step of the persistent bare-metal storage track (`docs/next_steps.md` item 2). Files written in the workspace now survive a reboot on the `virtio-blk-pci` disk that `cargo xtask qemu` attaches.

- `hal_x86_64::virtio::VirtioTransport` abstracts the device side of a virtio driver (`begin`, feature negotiation, `setup_queue` with a `QueuePlacement` of physical addresses, `driver_ok`, `notify`, config reads). `VirtioMmioDevice` and `VirtioPciLegacy<P: PortIo>` both implement it.
- `hal_x86_64::virtio_blk::VirtioBlkDevice<T: VirtioTransport>` is rewritten around the trait. It takes `QueueMemory` (ring pointers plus physical placement) and `DmaBuffers` (header, status, one-block bounce buffer with physical addresses), so descriptors carry real physical addresses instead of kernel virtual ones. Reads and writes bounce through the DMA block; an `UNSUPP` answer to flush is treated as success because no flush feature is negotiated.
- `kernel_bootstrap::bare_metal_storage` probes PCI first (`find_virtio_blk`, I/O BAR0, bus-master enable, legacy queue layout in a page-aligned static), then the MMIO windows, then the RAM disk. `StorageBootInfo` carries the Limine HHDM offset and kernel physical/virtual base so static buffers can be translated to physical addresses.
- `services_storage`: `ObjectId::from_bytes`, `BlockStorage::has_valid_superblock`, and `PersistentFilesystem::format_with_root`. The kernel formats a blank disk with a well-known root id and mounts an existing one instead of reformatting on every boot; the example files are only created on a fresh format.
- Boot log now reads `Filesystem ready (backend: virtio-blk-pci, formatted|mounted existing)`.

## Verification

- `cargo test --workspace`: green. New fake-transport tests in `virtio_blk` drive the real driver through an in-memory device model that walks the descriptor chain by physical address: capacity, write/read round trip through the bounce buffer, descriptor recycling over 40 requests, bounds/size errors, and unsupported flush.
- QEMU, two boots on the same `dist/pandagen.disk`:
  1. `write persist.txt hello-run1` after `Filesystem ready (backend: virtio-blk-pci, formatted)`; `ls` lists `persist.txt` alongside the example files.
  2. `Filesystem ready (backend: virtio-blk-pci, mounted existing)`; `cat persist.txt` prints `hello-run1`.

## Notes

- The MMIO backend keeps using the split `VIRTQ_*` statics; the PCI backend needs the contiguous legacy layout, so it has its own `LEGACY_QUEUE` area.
- `qemu-script --keys` takes QEMU `sendkey` names per keystroke (`spc`, `dot`, `minus`), not raw strings.

## Next

Persistent storage is done. Remaining `docs/next_steps.md` items: SMP bring-up, networking, and formal verification of the capability model.
