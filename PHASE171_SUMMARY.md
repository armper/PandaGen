# Phase 171: Bare-Metal Storage Backend Selection with Virtio-Blk Initialization Path

## Summary

This phase replaces the single hard-coded RAM-disk boot storage path in `kernel_bootstrap` with a typed backend selection flow:

- Prefer `VirtioBlkDevice` initialization on bare-metal (`target_os = "none"`).
- Fall back to `RamDisk` when virtio MMIO is unavailable.

The bootstrap now reports which backend is active during boot.

## Change Set

1. `kernel_bootstrap/src/bare_metal_storage.rs`
   - Added `StorageBackend` enum implementing `hal::BlockDevice`.
   - Added `StorageBackendKind` for explicit backend reporting.
   - Changed `BareMetalFilesystem` to use `PersistentFilesystem<StorageBackend>`.
   - Added `BareMetalFilesystem::new_with_hhdm(hhdm_offset)` for MMIO probing context.
   - Implemented bare-metal virtio probe path using pre-allocated virtqueue memory and bounded MMIO slot scans.
   - Kept deterministic fallback to `RamDisk` if probe fails.

2. `kernel_bootstrap/src/main.rs`
   - Filesystem initialization now calls `BareMetalFilesystem::new_with_hhdm(kernel.boot.hhdm_offset)`.
   - Boot log now prints selected backend (`ramdisk` or `virtio-blk-mmio`).

3. `kernel_bootstrap/src/bare_metal_storage_tests.rs`
   - Added test asserting test-mode backend defaults to `StorageBackendKind::RamDisk`.

4. `docs/qemu_boot.md`
   - Updated limitations section to document backend-dependent persistence and backend logging line.

5. `TODO_HIGH_VALUE_RANKING.md`
   - Marked item #4 complete with implementation note.

## Rationale

- Removes the previous “TODO + always RamDisk” behavior and introduces a real initialization path for persistent storage hardware.
- Keeps the boot flow robust in environments where virtio MMIO is not exposed, preserving deterministic startup via fallback.
- Makes backend choice explicit and observable in logs for debugging and validation.

## Tests

Executed:

- `cargo fmt`
- `cargo check -p kernel_bootstrap --target x86_64-unknown-none -Zbuild-std=core,alloc`
- `cargo xtask iso`

Notes:

- The virtio probe path itself is only compiled for bare-metal (`target_os = "none"`).
- `cargo test -p kernel_bootstrap ...` is currently blocked by pre-existing `sim_kernel` compile errors (`E0063` missing `syscall_gate`, `E0502` scheduler borrow conflict).
